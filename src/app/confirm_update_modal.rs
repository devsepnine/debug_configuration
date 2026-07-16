//! 인앱 업데이트 확인 모달의 상태 타입과 업데이트 핸들러.
//!
//! 상태바 업데이트 버튼은 즉시 설치하는 대신 이 모달을 띄운다 — 설치가 성공하면
//! 앱이 곧바로 재시작되므로, 원클릭 오조작으로 작업 흐름이 끊기는 것을 막는다.
//! 확정 시 기존 설치 경로(`handle_install_update`)를 재사용한다.
//! 인앱 설치가 불가하거나(구 릴리스) 이전 설치가 실패한 경우에도 모달을 거친다
//! (수동 모드 — 확정 시 릴리스 페이지를 브라우저로 연다). 예전의 "확인 없는 즉시
//! 브라우저 오픈"은 배지가 버전 라벨처럼 보여 오클릭할 때마다 페이지가 떠
//! "앱이 멋대로 띄운다"로 읽혔다(사용자 실측 피드백).

use super::RunConfigManager;
use crate::messages::Message;
use iced::Task;

/// 업데이트 확인 모달의 상태.
pub struct ConfirmUpdateModalState {
    /// 설치할 새 버전 (v 접두사 없는 정규화 형태, 모달 본문 **표시용** 스냅샷).
    /// 실제 설치는 확정 시점의 `update_available`을 다시 읽으므로 항상 최신이다.
    /// 현재는 체크가 시작 시 1회뿐이라 열려 있는 동안 값이 바뀔 수 없지만,
    /// 주기적 재확인을 도입하면 표시 문구가 한 박자 늦을 수 있음을 전제한다.
    /// (실행 중 세션 경고/버튼 비활성은 뷰가 매 프레임 라이브로 계산 — 상태에 없음.)
    pub latest: String,
    /// true = 인앱 설치 불가(자산/서명 없음)·직전 실패 상태에서 열림 — 확정은
    /// 설치 대신 릴리스 페이지를 브라우저로 연다 (수동 설치 안내 모드).
    pub manual: bool,
}

impl RunConfigManager {
    /// 상태바 업데이트 버튼 → 확인 모달 열기 (인앱/수동 공통 — 브라우저 열기도
    /// 확정 후에만). 실행 중 세션이 있어도 모달은 열되 Update를 비활성화하고
    /// 사유를 표시한다 — 조용한 상태바 거부는 "버튼을 눌러도 반응이 없다"로
    /// 읽혔다(사용자 실측 피드백).
    pub(super) fn handle_request_install_update(&mut self) -> Task<Message> {
        if self.is_updating {
            return Task::none();
        }
        let Some(update) = &self.update_available else {
            return Task::none();
        };
        let manual = update.download.is_none() || self.update_install_failed;
        let latest = update.latest.clone();
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        self.confirm_update_modal = Some(ConfirmUpdateModalState { latest, manual });
        Task::none()
    }

    /// 모달의 확정 처리.
    /// - 수동 모드: 릴리스 페이지를 브라우저로 연다 — 앱 재시작이 없으므로 실행 중
    ///   세션 가드가 필요 없다. URL은 확정 시점의 `update_available`에서 다시 읽는다.
    /// - 인앱 모드: 실제 설치 시작 (기존 설치 경로 재사용). 열림 이후 세션이
    ///   시작됐을 수도 있으므로 여기서도 재검사한다 — 그 사이 생긴 세션이 있으면
    ///   시작하지 않고 모달을 닫으며 사유를 남긴다 (설치 경로의
    ///   `handle_install_update`에도 동일 가드가 있어 삼중 방어).
    pub(super) fn handle_confirm_install_update(&mut self) -> Task<Message> {
        let Some(state) = self.confirm_update_modal.take() else {
            return Task::none();
        };
        if state.manual {
            let Some(update) = &self.update_available else {
                return Task::none();
            };
            let url = update.url.clone();
            return self.handle_open_url(&url);
        }
        if self.sessions.iter().any(|session| session.is_running) {
            self.status_message = String::from("Stop running sessions before updating");
            return Task::none();
        }
        self.handle_install_update()
    }

    /// 모달 취소 (Cancel/X/Esc).
    pub(super) fn handle_cancel_install_update(&mut self) -> Task<Message> {
        self.confirm_update_modal = None;
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{AvailableUpdate, RunConfigManager};
    use crate::models::RunSession;
    use crate::services::UpdateDownload;

    fn download() -> UpdateDownload {
        UpdateDownload {
            asset_name: String::from("run_config_manager-macos-arm64-app.tar.gz"),
            asset_url: String::from("https://github.com/x/y/releases/download/v9/app.tar.gz"),
            asset_size: 1024,
            checksums_url: String::from(
                "https://github.com/x/y/releases/download/v9/checksums.txt",
            ),
            checksums_sig_url: String::from(
                "https://github.com/x/y/releases/download/v9/checksums.txt.minisig",
            ),
        }
    }

    fn manager_with_update(download_available: bool) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.update_available = Some(AvailableUpdate {
            latest: String::from("9.9.9"),
            url: String::from("https://github.com/x/y/releases/tag/v9.9.9"),
            download: download_available.then(download),
        });
        app
    }

    #[test]
    fn request_opens_modal_without_starting_install() {
        let mut app = manager_with_update(true);
        let _ = app.handle_request_install_update();
        assert!(app.confirm_update_modal.is_some());
        assert!(!app.is_updating, "install must not start before confirm");
        assert_eq!(app.confirm_update_modal.as_ref().unwrap().latest, "9.9.9");
    }

    #[test]
    fn confirm_starts_install_and_closes_modal() {
        let mut app = manager_with_update(true);
        let _ = app.handle_request_install_update();
        let _ = app.handle_confirm_install_update();
        assert!(app.confirm_update_modal.is_none());
        assert!(app.is_updating, "confirm must start the install");
    }

    #[test]
    fn cancel_closes_modal_without_installing() {
        let mut app = manager_with_update(true);
        let _ = app.handle_request_install_update();
        let _ = app.handle_cancel_install_update();
        assert!(app.confirm_update_modal.is_none());
        assert!(!app.is_updating);
    }

    #[test]
    fn request_opens_modal_even_while_sessions_running() {
        // 실행 중에도 모달은 열린다 — 조용한 상태바 거부는 "반응 없음"으로 읽혔다.
        // (Update 버튼 비활성/사유 표시는 뷰가 라이브 세션 수로 처리한다.)
        let mut app = manager_with_update(true);
        app.sessions.push(RunSession::new("x".to_string())); // is_running=true로 생성
        let _ = app.handle_request_install_update();
        assert!(app.confirm_update_modal.is_some());
        assert!(!app.is_updating);
    }

    #[test]
    fn confirm_refuses_if_session_started_while_modal_open() {
        // 모달이 열린 사이 세션이 시작된 경우 — 확정해도 설치를 시작하지 않는다.
        let mut app = manager_with_update(true);
        let _ = app.handle_request_install_update();
        app.sessions.push(RunSession::new("x".to_string()));
        let _ = app.handle_confirm_install_update();
        assert!(app.confirm_update_modal.is_none());
        assert!(!app.is_updating);
        assert!(app.status_message.contains("Stop running sessions"));
    }

    #[test]
    fn request_without_download_opens_manual_modal_not_browser() {
        // 오클릭 방지: 즉시 브라우저를 열지 않고 수동 설치 모달을 띄운다.
        let mut app = manager_with_update(false);
        let _ = app.handle_request_install_update();
        let modal = app.confirm_update_modal.as_ref().expect("manual modal");
        assert!(modal.manual);
        assert!(!app.is_updating);
        assert!(
            !app.status_message.contains("Opening URL"),
            "browser must not open before confirm"
        );
    }

    #[test]
    fn request_after_failed_install_opens_manual_modal() {
        // 인앱 설치가 한 번 실패하면 이후 클릭도 모달(수동 모드)을 거친다.
        let mut app = manager_with_update(true);
        app.update_install_failed = true;
        let _ = app.handle_request_install_update();
        assert!(app.confirm_update_modal.as_ref().is_some_and(|m| m.manual));
    }

    #[test]
    fn manual_confirm_opens_release_page_even_while_sessions_running() {
        // 수동 모드 확정은 브라우저만 연다 — 앱 재시작이 없으므로 실행 중 세션
        // 가드를 타지 않는다. (테스트 빌드의 handle_open_url은 실제로 열지 않고
        // 상태 메시지 계약만 수행한다.)
        let mut app = manager_with_update(false);
        app.sessions.push(RunSession::new("x".to_string()));
        let _ = app.handle_request_install_update();
        let _ = app.handle_confirm_install_update();
        assert!(app.confirm_update_modal.is_none());
        assert!(!app.is_updating);
        assert!(
            app.status_message.contains("Opening URL"),
            "confirm must route to the release page: {:?}",
            app.status_message
        );
        assert!(app.status_message.contains("releases/tag/v9.9.9"));
    }
}
