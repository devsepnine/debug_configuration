//! 인앱 업데이트 확인 모달의 상태 타입과 업데이트 핸들러.
//!
//! 상태바 업데이트 버튼은 즉시 설치하는 대신 이 모달을 띄운다 — 설치가 성공하면
//! 앱이 곧바로 재시작되므로, 원클릭 오조작으로 작업 흐름이 끊기는 것을 막는다.
//! 확정 시 기존 설치 경로(`handle_install_update`)를 재사용한다.
//! 인앱 설치가 불가하거나(구 릴리스) 이전 설치가 실패한 경우에는 모달 없이
//! 릴리스 페이지 폴백을 그대로 탄다 (브라우저 열기는 확인이 필요 없는 조작).

use super::RunConfigManager;
use crate::messages::Message;
use iced::Task;

/// 업데이트 확인 모달의 상태.
pub struct ConfirmUpdateModalState {
    /// 설치할 새 버전 (v 접두사 없는 정규화 형태, 모달 본문 **표시용** 스냅샷).
    /// 실제 설치는 확정 시점의 `update_available`을 다시 읽으므로 항상 최신이다.
    /// 현재는 체크가 시작 시 1회뿐이라 열려 있는 동안 값이 바뀔 수 없지만,
    /// 주기적 재확인을 도입하면 표시 문구가 한 박자 늦을 수 있음을 전제한다.
    pub latest: String,
}

impl RunConfigManager {
    /// 상태바 업데이트 버튼 → 확인 모달 열기.
    /// 인앱 설치 불가/실패면 릴리스 페이지로 즉시 폴백하고, 실행 중 세션이 있으면
    /// 모달을 띄우기 전에 조기 거부한다 (확인까지 받고 나서 거부하는 것보다 낫다).
    pub(super) fn handle_request_install_update(&mut self) -> Task<Message> {
        if self.is_updating {
            return Task::none();
        }
        let Some(update) = &self.update_available else {
            return Task::none();
        };
        // 인앱 설치 불가/실패 → 브라우저 폴백 (기존 수동 설치 경로, 확인 불필요).
        if update.download.is_none() || self.update_install_failed {
            let url = update.url.clone();
            return self.handle_open_url(&url);
        }
        if self.sessions.iter().any(|session| session.is_running) {
            self.status_message = String::from("Stop running sessions before updating");
            return Task::none();
        }
        let latest = update.latest.clone();
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        self.confirm_update_modal = Some(ConfirmUpdateModalState { latest });
        Task::none()
    }

    /// 모달의 Update 확정 → 실제 설치 시작 (기존 설치 경로 재사용).
    pub(super) fn handle_confirm_install_update(&mut self) -> Task<Message> {
        if self.confirm_update_modal.take().is_none() {
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
    fn request_refuses_while_sessions_running() {
        let mut app = manager_with_update(true);
        app.sessions.push(RunSession::new("x".to_string())); // is_running=true로 생성
        let _ = app.handle_request_install_update();
        assert!(app.confirm_update_modal.is_none());
        assert!(app.status_message.contains("Stop running sessions"));
    }

    #[test]
    fn request_without_download_falls_back_to_release_page() {
        let mut app = manager_with_update(false);
        let _ = app.handle_request_install_update();
        // 브라우저 폴백 경로 — 모달을 띄우지 않는다 (상태 메시지는 open 결과에 따름).
        assert!(app.confirm_update_modal.is_none());
        assert!(!app.is_updating);
    }
}
