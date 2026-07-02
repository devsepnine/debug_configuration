//! 앱 설정 모달의 상태 타입과 업데이트 핸들러.
//!
//! `env_modal`과 동일하게, 부모 모듈(`app.rs`)의 `RunConfigManager`에 대한 `impl`을
//! 자식 모듈에 둔다. 자식 모듈은 부모의 비공개 설정 필드(`show_environment_on_run`,
//! `max_output_lines` 등)와 `save_app_settings`에 접근할 수 있어 모달 로직을 응집한다.

use super::RunConfigManager;
use crate::messages::Message;
use crate::services::check_latest_release;
use iced::Task;

/// 설정 모달의 staging 상태. Confirm 전까지 임시값을 보관하며 Cancel 시 폐기한다.
pub struct SettingsModalState {
    /// 실행 시 Environment 라인 표시 토글 (staging)
    pub show_environment: bool,
    /// 출력 최대 라인 수의 raw 입력 문자열. 타이핑 중에는 검증하지 않고 Confirm 시
    /// 파싱·클램프한다(타이핑 방해 방지 — env 메인 input과 동일 철학).
    pub max_output_lines_text: String,
    /// 새 세션 자동 스크롤 기본값 (staging)
    pub default_auto_scroll: bool,
    /// 시작 시 업데이트 자동 확인 (staging)
    pub auto_check_updates: bool,
}

/// 출력 라인 수 설정의 허용 범위. 너무 작으면 출력이 즉시 잘리고, 너무 크면 메모리
/// 압박이 커지므로 합리적 범위로 클램프한다(바이트 예산은 별도로 항상 적용됨).
const MIN_OUTPUT_LINES: usize = 1_000;
const MAX_OUTPUT_LINES_LIMIT: usize = 1_000_000;

impl RunConfigManager {
    pub(super) fn handle_open_settings_modal(&mut self) -> Task<Message> {
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        // 현재 적용 중인 설정값을 staging으로 복사 (Cancel 시 원상복구를 위해).
        self.settings_modal = Some(SettingsModalState {
            show_environment: self.show_environment_on_run,
            max_output_lines_text: self.max_output_lines.to_string(),
            default_auto_scroll: self.default_auto_scroll,
            auto_check_updates: self.auto_check_updates,
        });
        Task::none()
    }

    pub(super) fn handle_confirm_settings_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.settings_modal.take() else {
            return Task::none();
        };
        let updates_newly_enabled = modal.auto_check_updates && !self.auto_check_updates;
        self.show_environment_on_run = modal.show_environment;
        self.max_output_lines =
            Self::parse_max_output_lines(modal.max_output_lines_text.trim(), self.max_output_lines);
        self.default_auto_scroll = modal.default_auto_scroll;
        self.auto_check_updates = modal.auto_check_updates;
        self.save_app_settings();
        // 업데이트 자동 확인을 새로 켜면 다음 시작까지 기다리지 않고 즉시 1회 확인한다
        // (시작 시 자동 체크와 동일 경로). 이미 확인 중이면 중복 발행하지 않는다.
        if updates_newly_enabled && !self.is_checking_update {
            self.is_checking_update = true;
            return Task::perform(check_latest_release(), Message::UpdateCheckCompleted);
        }
        Task::none()
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

    /// 라인 수 입력을 파싱한다. 파싱 실패(빈 값·비숫자)면 `fallback`을 유지하고,
    /// 성공하면 `MIN_OUTPUT_LINES..=MAX_OUTPUT_LINES_LIMIT`로 클램프한다.
    fn parse_max_output_lines(text: &str, fallback: usize) -> usize {
        text.parse::<usize>()
            .map(|n| n.clamp(MIN_OUTPUT_LINES, MAX_OUTPUT_LINES_LIMIT))
            .unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::RunConfigManager;

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
}
