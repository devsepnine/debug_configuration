//! 앱 설정 모달의 상태 타입과 업데이트 핸들러.
//!
//! `env_modal`과 동일하게, 부모 모듈(`app.rs`)의 `RunConfigManager`에 대한 `impl`을
//! 자식 모듈에 둔다. 자식 모듈은 부모의 비공개 설정 필드(`show_environment_on_run`,
//! `max_output_lines` 등)와 `save_app_settings`에 접근할 수 있어 모달 로직을 응집한다.

use super::{McpServerStatus, RunConfigManager};
use crate::messages::Message;
use crate::services::{McpPermission, check_latest_release, regenerate_token};
use iced::Task;

/// 설정 모달의 staging 상태. OK/Apply 전까지 임시값을 보관하며, Cancel은 마지막 Apply 이후의
/// 변경만 폐기한다.
pub struct SettingsModalState {
    /// 실행 시 Environment 라인 표시 토글 (staging)
    pub show_environment: bool,
    /// 출력 최대 라인 수의 raw 입력 문자열. 타이핑 중에는 검증하지 않고 OK/Apply 시
    /// 파싱·클램프한다(타이핑 방해 방지 — env 메인 input과 동일 철학).
    pub max_output_lines_text: String,
    /// 새 세션 자동 스크롤 기본값 (staging)
    pub default_auto_scroll: bool,
    /// 시작 시 업데이트 자동 확인 (staging)
    pub auto_check_updates: bool,
    /// 로컬 MCP 서버 활성화 (staging)
    pub mcp_enabled: bool,
    /// MCP 포트의 raw 입력 문자열. 라인 수 입력과 같이 OK/Apply 시 파싱·클램프한다.
    pub mcp_port_text: String,
    /// MCP 권한 단계 (staging)
    pub mcp_permission: McpPermission,
    /// MCP 응답에 환경변수 값 노출 (staging)
    pub mcp_expose_env_values: bool,
}

/// 출력 라인 수 설정의 허용 범위. 너무 작으면 출력이 즉시 잘리고, 너무 크면 메모리
/// 압박이 커지므로 합리적 범위로 클램프한다(바이트 예산은 별도로 항상 적용됨).
const MIN_OUTPUT_LINES: usize = 1_000;
const MAX_OUTPUT_LINES_LIMIT: usize = 1_000_000;

/// MCP 포트 하한. 1024 미만은 대부분의 OS에서 관리자 권한을 요구하고, 0은 매 실행마다
/// 포트가 달라져 사용자가 등록해 둔 `claude mcp add` 명령이 무효가 된다.
const MIN_MCP_PORT: u16 = 1_024;

impl RunConfigManager {
    pub(super) fn handle_open_settings_modal(&mut self) -> Task<Message> {
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        // 현재 적용 중인 설정값을 staging의 시작점으로 복사한다.
        self.settings_modal = Some(SettingsModalState {
            show_environment: self.show_environment_on_run,
            max_output_lines_text: self.max_output_lines.to_string(),
            default_auto_scroll: self.default_auto_scroll,
            auto_check_updates: self.auto_check_updates,
            mcp_enabled: self.mcp_enabled,
            mcp_port_text: self.mcp_port.to_string(),
            mcp_permission: self.mcp_permission,
            mcp_expose_env_values: self.mcp_expose_env_values,
        });
        Task::none()
    }

    pub(super) fn handle_confirm_settings_modal(&mut self) -> Task<Message> {
        let Some(mut modal) = self.settings_modal.take() else {
            return Task::none();
        };
        self.apply_settings(&mut modal)
    }

    /// 모달을 닫지 않고 staging을 반영한다. 토큰 발급·서버 시작 결과를 모달 안에서
    /// 바로 확인할 수 있고, 이후 Cancel은 Apply 시점의 값으로 남는다.
    pub(super) fn handle_apply_settings_modal(&mut self) -> Task<Message> {
        if !self.settings_modal_can_apply() {
            return Task::none();
        }
        let Some(mut modal) = self.settings_modal.take() else {
            return Task::none();
        };
        let task = self.apply_settings(&mut modal);
        self.settings_modal = Some(modal);
        task
    }

    /// staging을 검증해 설정에 반영하고 저장한다. raw 입력 문자열은 클램프된 실제 적용값으로
    /// 되돌려 써서, 모달을 유지하는 Apply 뒤에도 화면이 적용값과 어긋나지 않게 한다.
    fn apply_settings(&mut self, modal: &mut SettingsModalState) -> Task<Message> {
        let updates_newly_enabled = modal.auto_check_updates && !self.auto_check_updates;
        self.show_environment_on_run = modal.show_environment;
        self.max_output_lines =
            Self::parse_max_output_lines(modal.max_output_lines_text.trim(), self.max_output_lines);
        modal.max_output_lines_text = self.max_output_lines.to_string();
        self.default_auto_scroll = modal.default_auto_scroll;
        self.auto_check_updates = modal.auto_check_updates;

        let mcp_port = Self::parse_mcp_port(modal.mcp_port_text.trim(), self.mcp_port);
        let mcp_newly_enabled = modal.mcp_enabled && !self.mcp_enabled;
        let mcp_restarting = mcp_newly_enabled || (modal.mcp_enabled && mcp_port != self.mcp_port);
        self.mcp_enabled = modal.mcp_enabled;
        self.mcp_port = mcp_port;
        modal.mcp_port_text = mcp_port.to_string();
        self.mcp_permission = modal.mcp_permission;
        self.mcp_expose_env_values = modal.mcp_expose_env_values;
        if !self.mcp_enabled {
            self.mcp_status = McpServerStatus::Disabled;
        } else if mcp_restarting {
            // 리스너가 (재)시작되므로 아직 유효하지 않은 이전 상태를 계속 보여주지 않는다.
            self.mcp_status = McpServerStatus::Starting;
        }

        self.save_app_settings();

        let mut tasks = vec![];
        // 토큰이 없으면 리스너가 뜨지 못한다. "새로 켰다"가 아니라 "켜져 있는데 토큰이 없다"를
        // 조건으로 삼아, 이전 로드가 실패한 뒤에도 Confirm/Apply 한 번으로 재시도된다 — 토글을
        // 껐다 켜는 우회 경로가 유일한 복구 수단이 되지 않도록. 로드가 진행 중이면
        // `mcp_token_task`가 중복 발행을 막는다.
        if self.mcp_enabled && self.mcp_token.is_none() {
            tasks.push(self.mcp_token_task());
        }
        // 업데이트 자동 확인을 새로 켜면 다음 시작까지 기다리지 않고 즉시 1회 확인한다
        // (시작 시 자동 체크와 동일 경로). 이미 확인 중이거나 인앱 업데이트 설치가
        // 진행 중이면 중복 발행하지 않는다 (진행 상태 메시지를 덮어쓰는 것 방지).
        if updates_newly_enabled && !self.is_checking_update && !self.is_updating {
            self.is_checking_update = true;
            tasks.push(Task::perform(
                check_latest_release(),
                Message::UpdateCheckCompleted,
            ));
        }
        Task::batch(tasks)
    }

    /// 모달의 staging이 적용된 설정과 다른지. 포트·라인 수는 입력 문자열이 아니라 Apply가 실제로
    /// 쓸 클램프 값으로 비교한다 — 빈 입력이나 같은 값을 다시 친 경우는 변경이 아니다.
    pub(super) fn settings_modal_is_dirty(&self) -> bool {
        let Some(modal) = self.settings_modal.as_ref() else {
            return false;
        };
        modal.show_environment != self.show_environment_on_run
            || modal.default_auto_scroll != self.default_auto_scroll
            || modal.auto_check_updates != self.auto_check_updates
            || modal.mcp_enabled != self.mcp_enabled
            || modal.mcp_permission != self.mcp_permission
            || modal.mcp_expose_env_values != self.mcp_expose_env_values
            || Self::parse_max_output_lines(
                modal.max_output_lines_text.trim(),
                self.max_output_lines,
            ) != self.max_output_lines
            || Self::parse_mcp_port(modal.mcp_port_text.trim(), self.mcp_port) != self.mcp_port
    }

    /// Apply가 할 일이 있는지. 변경이 없어도, 켜져 있는데 토큰이 없고(로드 실패) 로드 중도
    /// 아니면 재시도 수단이 되어야 한다 — 그렇지 않으면 OK로 닫았다 다시 여는 우회가 필요하다.
    pub(super) fn settings_modal_can_apply(&self) -> bool {
        if self.settings_modal.is_none() {
            return false;
        }
        self.settings_modal_is_dirty() || self.mcp_token_retry_needed()
    }

    fn mcp_token_retry_needed(&self) -> bool {
        self.mcp_enabled && self.mcp_token.is_none() && !self.mcp_token_loading
    }

    pub(super) fn handle_cancel_settings_modal(&mut self) -> Task<Message> {
        self.settings_modal = None;
        Task::none()
    }

    pub(super) fn handle_settings_toggle_environment(&mut self, value: bool) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.show_environment = value;
        }
        Task::none()
    }

    pub(super) fn handle_settings_max_lines_changed(&mut self, text: String) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.max_output_lines_text = text;
        }
        Task::none()
    }

    pub(super) fn handle_settings_toggle_auto_scroll(&mut self, value: bool) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.default_auto_scroll = value;
        }
        Task::none()
    }

    pub(super) fn handle_settings_toggle_auto_check_updates(
        &mut self,
        value: bool,
    ) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.auto_check_updates = value;
        }
        Task::none()
    }

    pub(super) fn handle_settings_toggle_mcp_enabled(&mut self, value: bool) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.mcp_enabled = value;
        }
        Task::none()
    }

    pub(super) fn handle_settings_mcp_port_changed(&mut self, text: String) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.mcp_port_text = text;
        }
        Task::none()
    }

    pub(super) fn handle_settings_mcp_permission_changed(
        &mut self,
        permission: McpPermission,
    ) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.mcp_permission = permission;
        }
        Task::none()
    }

    pub(super) fn handle_settings_toggle_mcp_expose_env(&mut self, value: bool) -> Task<Message> {
        if let Some(modal) = self.settings_modal.as_mut() {
            modal.mcp_expose_env_values = value;
        }
        Task::none()
    }

    pub(super) fn handle_settings_copy_mcp_token(&mut self) -> Task<Message> {
        let Some(token) = self.mcp_token.clone() else {
            self.status_message = String::from("MCP token is not available yet");
            return Task::none();
        };
        self.status_message = String::from("MCP token copied to clipboard");
        iced::clipboard::write(token)
    }

    pub(super) fn handle_settings_copy_mcp_command(&mut self) -> Task<Message> {
        if self.mcp_token.is_none() {
            self.status_message = String::from("MCP token is not available yet");
            return Task::none();
        }
        self.status_message = String::from("MCP registration command copied to clipboard");
        iced::clipboard::write(self.mcp_add_command(true))
    }

    pub(super) fn handle_settings_regenerate_mcp_token(&mut self) -> Task<Message> {
        // 진행 중인 로드/재발급과 같은 토큰 파일을 건드리지 않는다 — 늦게 끝난 쪽이 이기면
        // 방금 만든 토큰이 낡은 값으로 덮인다.
        if self.mcp_token_loading {
            return Task::none();
        }
        // 새 토큰이 도착하기 전에 기존 토큰을 버린다 — subscription 정체성이 사라져 낡은
        // 토큰으로 리스닝하는 창이 남지 않고, 기존 클라이언트는 즉시 연결이 끊긴다.
        self.mcp_token = None;
        self.mcp_token_loading = true;
        if self.mcp_enabled {
            self.mcp_status = McpServerStatus::Starting;
        }
        self.status_message = String::from("MCP token regenerated — re-register your MCP client");
        Task::perform(async { regenerate_token() }, Message::McpTokenLoaded)
    }

    /// 라인 수 입력을 파싱한다. 파싱 실패(빈 값·비숫자)면 `fallback`을 유지하고,
    /// 성공하면 `MIN_OUTPUT_LINES..=MAX_OUTPUT_LINES_LIMIT`로 클램프한다.
    fn parse_max_output_lines(text: &str, fallback: usize) -> usize {
        text.parse::<usize>()
            .map(|n| n.clamp(MIN_OUTPUT_LINES, MAX_OUTPUT_LINES_LIMIT))
            .unwrap_or(fallback)
    }

    /// 포트 입력을 파싱한다. `u16` 범위를 넘는 값도 클램프로 흡수하기 위해 `u32`로 읽는다.
    fn parse_mcp_port(text: &str, fallback: u16) -> u16 {
        let Ok(value) = text.parse::<u32>() else {
            return fallback;
        };
        u16::try_from(value.clamp(u32::from(MIN_MCP_PORT), u32::from(u16::MAX))).unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::{McpPermission, McpServerStatus, RunConfigManager};

    #[test]
    fn parse_max_output_lines_clamps_to_range() {
        // 범위 내는 그대로
        assert_eq!(
            RunConfigManager::parse_max_output_lines("50000", 50_000),
            50_000
        );
        // 너무 작으면 하한
        assert_eq!(
            RunConfigManager::parse_max_output_lines("10", 50_000),
            1_000
        );
        // 너무 크면 상한
        assert_eq!(
            RunConfigManager::parse_max_output_lines("99999999", 50_000),
            1_000_000
        );
    }

    #[test]
    fn parse_max_output_lines_keeps_fallback_on_invalid() {
        // 빈 값·비숫자는 기존값 유지
        assert_eq!(RunConfigManager::parse_max_output_lines("", 50_000), 50_000);
        assert_eq!(
            RunConfigManager::parse_max_output_lines("abc", 42_000),
            42_000
        );
    }

    #[test]
    fn parse_mcp_port_clamps_and_keeps_fallback() {
        assert_eq!(RunConfigManager::parse_mcp_port("47355", 47_355), 47_355);
        // 특권 포트와 0은 하한으로 끌어올린다.
        assert_eq!(RunConfigManager::parse_mcp_port("80", 47_355), 1_024);
        assert_eq!(RunConfigManager::parse_mcp_port("0", 47_355), 1_024);
        // u16을 넘는 값은 상한으로.
        assert_eq!(RunConfigManager::parse_mcp_port("999999", 47_355), 65_535);
        // 파싱 불가는 기존값 유지.
        assert_eq!(RunConfigManager::parse_mcp_port("", 47_355), 47_355);
        assert_eq!(RunConfigManager::parse_mcp_port("-1", 47_355), 47_355);
        assert_eq!(RunConfigManager::parse_mcp_port("abc", 47_355), 47_355);
    }

    #[test]
    fn opening_the_modal_stages_the_live_mcp_settings() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;
        app.mcp_port = 51_000;
        app.mcp_permission = McpPermission::Execute;
        app.mcp_expose_env_values = true;

        let _ = app.handle_open_settings_modal();
        let modal = app.settings_modal.as_ref().expect("modal opened");

        assert!(modal.mcp_enabled);
        assert_eq!(modal.mcp_port_text, "51000");
        assert_eq!(modal.mcp_permission, McpPermission::Execute);
        assert!(modal.mcp_expose_env_values);
    }

    #[test]
    fn cancelling_discards_staged_mcp_changes() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_toggle_mcp_enabled(true);
        let _ = app.handle_settings_mcp_permission_changed(McpPermission::Edit);
        let _ = app.handle_settings_toggle_mcp_expose_env(true);

        let _ = app.handle_cancel_settings_modal();

        assert!(!app.mcp_enabled, "cancel must not start a listener");
        assert_eq!(app.mcp_permission, McpPermission::ReadOnly);
        assert!(!app.mcp_expose_env_values);
    }

    #[test]
    fn confirming_applies_the_staged_mcp_settings() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_toggle_mcp_enabled(true);
        let _ = app.handle_settings_mcp_port_changed("50123".to_string());
        let _ = app.handle_settings_mcp_permission_changed(McpPermission::Execute);
        let _ = app.handle_settings_toggle_mcp_expose_env(true);

        let _ = app.handle_confirm_settings_modal();

        assert!(app.mcp_enabled);
        assert_eq!(app.mcp_port, 50_123);
        assert_eq!(app.mcp_permission, McpPermission::Execute);
        assert!(app.mcp_expose_env_values);
        assert_eq!(app.mcp_status, McpServerStatus::Starting);
    }

    #[test]
    fn disabling_the_server_resets_the_status() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;
        app.mcp_status = McpServerStatus::Listening(47_355);

        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_toggle_mcp_enabled(false);
        let _ = app.handle_confirm_settings_modal();

        assert!(!app.mcp_enabled);
        assert_eq!(app.mcp_status, McpServerStatus::Disabled);
    }

    #[test]
    fn regenerating_drops_the_current_token_immediately() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_token = Some("f".repeat(64));

        let _ = app.handle_settings_regenerate_mcp_token();

        assert!(
            app.mcp_token.is_none(),
            "the old token must stop authenticating before the new one arrives"
        );
    }

    #[test]
    fn applying_keeps_the_modal_open_and_normalizes_staged_text() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_toggle_mcp_enabled(true);
        let _ = app.handle_settings_mcp_port_changed("80".to_string());

        let _ = app.handle_apply_settings_modal();

        assert!(app.mcp_enabled);
        assert_eq!(app.mcp_port, 1_024);
        let modal = app.settings_modal.as_ref().expect("modal stays open");
        assert_eq!(modal.mcp_port_text, "1024");
        assert_eq!(app.mcp_status, McpServerStatus::Starting);
    }

    #[test]
    fn a_freshly_opened_modal_has_no_changes() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();

        assert!(!app.settings_modal_is_dirty());
    }

    #[test]
    fn a_toggled_field_counts_as_a_change() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();

        let _ = app.handle_settings_toggle_mcp_expose_env(true);

        assert!(app.settings_modal_is_dirty());
    }

    #[test]
    fn retyping_the_applied_value_is_not_a_change() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let applied_port = app.mcp_port.to_string();

        let _ = app.handle_settings_mcp_port_changed(String::new());
        assert!(
            !app.settings_modal_is_dirty(),
            "blank falls back to the applied port"
        );
        let _ = app.handle_settings_mcp_port_changed(applied_port);
        assert!(!app.settings_modal_is_dirty());
    }

    #[test]
    fn applying_without_changes_does_nothing() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();

        let _ = app.handle_apply_settings_modal();

        assert!(app.settings_modal.is_some());
        assert_eq!(app.mcp_status, McpServerStatus::default());
    }

    #[test]
    fn applying_clears_the_pending_changes() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_mcp_port_changed("80".to_string());
        assert!(app.settings_modal_is_dirty());

        let _ = app.handle_apply_settings_modal();

        assert!(!app.settings_modal_is_dirty());
    }

    #[test]
    fn cancelling_after_apply_keeps_the_applied_values() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_mcp_port_changed("50123".to_string());
        let _ = app.handle_apply_settings_modal();
        let _ = app.handle_settings_mcp_permission_changed(McpPermission::Execute);

        let _ = app.handle_cancel_settings_modal();

        assert!(app.settings_modal.is_none());
        assert_eq!(app.mcp_port, 50_123);
        assert_eq!(app.mcp_permission, McpPermission::ReadOnly);
    }

    #[test]
    fn confirming_closes_the_modal() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();

        let _ = app.handle_confirm_settings_modal();

        assert!(app.settings_modal.is_none());
    }

    #[test]
    fn applying_normalizes_the_output_lines_text() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_max_lines_changed("5".to_string());

        let _ = app.handle_apply_settings_modal();

        let modal = app.settings_modal.as_ref().expect("modal stays open");
        assert_eq!(modal.max_output_lines_text, "1000");
    }

    #[test]
    fn a_second_apply_while_the_token_loads_is_a_noop() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        let _ = app.handle_open_settings_modal();
        let _ = app.handle_settings_toggle_mcp_enabled(true);
        let _ = app.handle_apply_settings_modal();
        assert!(app.mcp_token_loading);

        assert!(
            !app.settings_modal_can_apply(),
            "nothing changed and a load is in flight"
        );
        let second = app.handle_apply_settings_modal();

        assert_eq!(second.units(), 0);
    }

    #[test]
    fn a_pending_load_does_not_start_another() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;

        let first = app.mcp_token_task();
        let second = app.mcp_token_task();

        assert_eq!(first.units(), 1);
        assert_eq!(second.units(), 0);
    }

    #[test]
    fn a_token_result_clears_the_loading_flag_on_success_and_failure() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();

        app.mcp_token_loading = true;
        let _ = app.handle_mcp_token_loaded(Ok("a".repeat(64)));
        assert!(!app.mcp_token_loading);

        app.mcp_token_loading = true;
        let _ = app.handle_mcp_token_loaded(Err("denied".to_string()));
        assert!(!app.mcp_token_loading);
    }

    #[test]
    fn regenerating_marks_the_token_as_loading_and_ignores_repeats() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        app.mcp_token = Some("f".repeat(64));

        let first = app.handle_settings_regenerate_mcp_token();
        let second = app.handle_settings_regenerate_mcp_token();

        assert!(app.mcp_token_loading);
        assert_eq!(first.units(), 1);
        assert_eq!(
            second.units(),
            0,
            "a repeat must not race the first on the token file"
        );
    }

    #[test]
    fn apply_stays_available_to_retry_a_failed_token_load() {
        let mut app = RunConfigManager::new().0;
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;
        app.mcp_status = McpServerStatus::Failed("permission denied".to_string());
        let _ = app.handle_open_settings_modal();
        assert!(!app.settings_modal_is_dirty());

        assert!(app.settings_modal_can_apply());
        let retry = app.handle_apply_settings_modal();

        assert_eq!(retry.units(), 1);
        assert!(app.mcp_token_loading);
        assert!(!app.settings_modal_can_apply());
    }

    #[test]
    fn confirming_retries_a_token_load_that_failed() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        // 토큰 로드가 실패한 상태: 서버는 켜져 있지만 토큰이 없어 리스너가 뜨지 못한다.
        app.mcp_enabled = true;
        app.mcp_status = McpServerStatus::Failed("permission denied".to_string());

        let _ = app.handle_open_settings_modal();
        let _ = app.handle_confirm_settings_modal();

        assert_eq!(
            app.mcp_status,
            McpServerStatus::Starting,
            "confirm must re-issue the token load instead of leaving the user stuck on Failed"
        );
    }

    #[test]
    fn copy_without_a_token_reports_instead_of_copying_a_placeholder() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let _ = app.handle_settings_copy_mcp_command();
        assert!(app.status_message.contains("not available"));
    }
}
