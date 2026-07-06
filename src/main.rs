#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

/// `IntelliJ` 스타일 Run/Debug 구성 관리자
///
/// iced GUI 프레임워크를 사용하여 Elm Architecture 패턴을 따름
/// - Model: `app::RunConfigManager`
/// - Update: `app::RunConfigManager::update`
/// - View: `app::RunConfigManager::view`
mod ansi;
mod app;
mod env_string;
mod messages;
mod models;
mod services;
mod utils;
mod views;
mod widgets;

use app::RunConfigManager;
use iced::{Color, Element, Font, Size, Subscription, Task, Theme, theme, window};
use messages::Message;

/// D2 Coding 폰트 임베드
pub(crate) const D2CODING_FONT: &[u8] = include_bytes!("../fonts/D2Coding.ttf");

/// D2 Coding 폰트 정의
pub const D2CODING: Font = Font::with_name("D2Coding");

/// macOS 앱 번들 식별자(알림 귀속에 사용). 이 상수가 SSOT이며, `build_macos_app.sh`가
/// Info.plist의 `CFBundleIdentifier`를 여기서 파생시키므로 두 값은 항상 일치한다.
#[cfg(target_os = "macos")]
const MACOS_BUNDLE_ID: &str = "dev.runconfigmanager.app";

/// 애플리케이션 진입점
fn main() -> iced::Result {
    // 시그널 핸들러 등록: Ctrl+C, SIGINT, SIGTERM 캐치
    // std::process::exit(0)는 Drop을 실행하지 않으므로(스택 unwind 없음), 시그널 핸들러가
    // 직접 자식 프로세스를 정리해야 한다. RunConfigManager에는 접근할 수 없으므로 전역 PID
    // 레지스트리를 통해 실행 중인 모든 프로세스 트리를 강제 종료한 뒤 종료한다.
    ctrlc::set_handler(move || {
        eprintln!("\n[Signal] Termination signal received (Ctrl+C / SIGTERM)");
        eprintln!("[Signal] Killing child processes and exiting...");
        services::kill_all_running_processes();
        std::process::exit(0);
    })
    .expect("Failed to set signal handler");

    eprintln!("[Startup] Run/Debug Configuration Manager");
    eprintln!("[Startup] Press Ctrl+C for graceful shutdown");

    // PowerShell 탐지(LazyLock, 1회 blocking subprocess)를 창이 뜨기 전 메인 스레드에서
    // 미리 초기화 → 첫 구성 실행이 tokio 워커에서 블로킹되지 않는다 (Windows 외 no-op).
    services::prewarm_shell_detection();

    // macOS 26+에서 notify-rust는 알림 전송 시 앱 번들 식별자를 자동 탐지하려다
    // (get_bundle_identifier_or_default) 멈추거나 "choose application" 다이얼로그를
    // 띄운다 — 이 API는 메인 스레드 전용인데 알림은 백그라운드 스레드에서 발행되기
    // 때문이다 (mac-notification-sys#81). 시작 시 메인 스레드에서 번들 식별자를 1회
    // 지정하면(set_application은 Once로 성공/실패 무관하게 자동 탐지 경로를 봉인한다)
    // 이후 모든 알림이 안전해진다. dev(cargo run)에서는 번들 미등록으로 Err이지만
    // 자동 탐지 봉인은 그대로 유효하므로 non-fatal이다 — 관측을 위해 로그만 남긴다.
    #[cfg(target_os = "macos")]
    if let Err(err) = notify_rust::set_application(MACOS_BUNDLE_ID) {
        eprintln!(
            "[Startup] set_application({MACOS_BUNDLE_ID:?}) failed (expected without an app bundle, e.g. `cargo run`): {err}"
        );
    }

    iced::application(RunConfigManager::new, update, view)
        .subscription(subscription)
        .font(D2CODING_FONT)
        .theme(theme)
        .style(application_style)
        .window(window::Settings {
            size: Size::new(800.0, 600.0),           // 초기 창 크기
            min_size: Some(Size::new(600.0, 400.0)), // 최소 크기: 가로 600px, 세로 400px
            decorations: false,
            transparent: true,
            ..Default::default()
        })
        .title("Run/Debug Configuration Manager")
        .run()
}

/// Subscription 함수 - 마우스 이벤트 구독 (탭 드래그용)
fn subscription(state: &RunConfigManager) -> Subscription<Message> {
    state.subscription()
}

/// 테마 함수 - Catppuccin Mocha 테마 반환
fn theme(_state: &RunConfigManager) -> Theme {
    Theme::CatppuccinMocha
}

fn application_style(_state: &RunConfigManager, theme: &Theme) -> theme::Style {
    theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.extended_palette().background.base.text,
    }
}

/// Update 함수 - 상태 업데이트를 위임
fn update(state: &mut RunConfigManager, message: Message) -> Task<Message> {
    state.update(message)
}

/// View 함수 - UI 렌더링을 위임
fn view(state: &RunConfigManager) -> Element<'_, Message> {
    state.view()
}
