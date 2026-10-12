//! 플랫폼 부수 효과: 파일 다이얼로그, 데스크톱 알림, URL 열기, 업데이트 후 재실행.
//! `app`은 이 함수들을 부르기만 한다(`docs/ARCHITECTURE.md` → `arch-io-in-services`).

use super::RelaunchPlan;
use crate::utils::DIALOG_CANCELLED;
use rfd::AsyncFileDialog;

/// 네이티브 파일/폴더 선택 다이얼로그를 연다. 취소 시 `DIALOG_CANCELLED` 센티넬을 Err로 반환한다.
pub async fn pick_path(
    title: &'static str,
    filter: Option<(&'static str, &'static [&'static str])>,
    folder: bool,
) -> Result<String, String> {
    let mut dialog = AsyncFileDialog::new().set_title(title);
    if let Some((name, extensions)) = filter {
        dialog = dialog.add_filter(name, extensions);
    }
    let handle = if folder {
        dialog.pick_folder().await
    } else {
        dialog.pick_file().await
    };
    match handle {
        Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
        None => Err(String::from(DIALOG_CANCELLED)),
    }
}

/// 백그라운드(비포커스) 상태에서 실행이 끝났을 때 OS 데스크톱 알림을 표시.
/// 정상 종료/비정상 종료/스폰 실패를 모두 구분해 알리며, UI 스레드를 막지
/// 않도록 별도 스레드에서 발행하고 발행 실패는 조용히 무시한다.
pub fn notify_run_finished(name: &str, result: Result<i32, &str>) {
    let (summary, body) = match result {
        Ok(0) => (
            String::from("Run finished"),
            format!("{name} completed successfully"),
        ),
        Ok(code) => (
            String::from("Run failed"),
            format!("{name} exited with code {code}"),
        ),
        Err(error) => (
            String::from("Run failed"),
            format!("{name} failed: {error}"),
        ),
    };

    std::thread::spawn(move || {
        // app_id를 지정하지 않으면 Windows에서 PowerShell AppUserModelID로
        // 폴백한다. 커스텀 AUMID는 Start Menu 등록이 선행되어야 토스트가 뜨므로,
        // 미패키징 앱에서는 폴백을 그대로 사용하는 편이 안전하다.
        let _ = notify_rust::Notification::new()
            .summary(&summary)
            .body(&body)
            .show();
    });
}

/// 새 버전 발견 시 OS 데스크톱 알림을 표시한다. 토스트 클릭으로 링크를 여는
/// 동작은 플랫폼별 편차가 커 신뢰할 수 없으므로, 알림은 안내용으로만 쓰고
/// 실제 다운로드 진입점은 상태바의 클릭 가능한 링크가 영속적으로 제공한다.
/// UI 스레드를 막지 않도록 별도 스레드에서 발행하고 실패는 조용히 무시한다.
pub fn notify_update_available(latest: &str, url: &str) {
    let summary = String::from("Update available");
    let body = format!("Version {latest} is available.\n{url}");

    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .summary(&summary)
            .body(&body)
            .show();
    });
}

/// 인앱 업데이트 적용 후 재실행 계획을 실행한다. spawn 성공 여부만 확인하며,
/// 프로세스 종료는 호출 측(`handle_update_install_completed`)이 담당한다.
/// - macOS: 새 번들을 `open -n`으로 실행 (이미 제자리에 설치된 상태)
/// - Windows: 검증된 MSI를 `msiexec /i`(full UI)로 실행 — 설치·재실행은 설치관리자가 담당
/// - Linux: 교체된 바이너리를 그대로 재실행
pub fn execute_relaunch(plan: &RelaunchPlan) -> Result<(), String> {
    match plan {
        RelaunchPlan::MacOs { app_path } => std::process::Command::new("open")
            .arg("-n")
            .arg(app_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch updated app: {e}")),
        RelaunchPlan::Windows { msi_path } => std::process::Command::new("msiexec")
            .arg("/i")
            .arg(msi_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch installer: {e}")),
        RelaunchPlan::Linux { exe_path } => std::process::Command::new(exe_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to relaunch: {e}")),
    }
}

/// 기본 브라우저로 URL을 연다.
///
/// 테스트 빌드는 실제로 열지 않는다 — 이 경로를 지나는 유닛 테스트가 cargo test마다 개발 머신에
/// 진짜 브라우저 탭(가짜 릴리스 페이지 404 등)을 띄우는 사고가 실제로 있었다. 호출 측의 상태
/// 메시지 계약은 그대로 유지된다.
pub fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(test)]
    {
        let _ = url;
        Ok(())
    }
    #[cfg(not(test))]
    {
        open::that(url)
    }
}
