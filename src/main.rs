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
use iced::{Color, Element, Font, Point, Size, Subscription, Task, Theme, theme, window};
use messages::Message;

/// 창 최소 크기. `window::Settings`와 저장된 geometry 복원 clamp가 공유하는 SSOT.
const MIN_WINDOW_SIZE: Size = Size::new(600.0, 400.0);

/// 저장된 창 배치를 window 설정값으로 변환한다.
/// 크기는 최소 크기 미만으로 줄지 않게 clamp하고, 위치는 좌표가 비정상 범위(모니터
/// 좌표로 볼 수 없는 값)면 무시해 OS 기본 배치로 폴백한다 (멀티 모니터 음수는 유효).
fn restore_window_geometry(settings: &services::AppSettings) -> (Size, window::Position) {
    // 유한성 검사를 clamp보다 먼저 — f32::max는 NaN.max(x) == x라 NaN이 유한값으로
    // 세탁되어 뒤의 is_finite 필터가 무력화된다.
    let size = settings
        .window_size
        .filter(|(w, h)| w.is_finite() && h.is_finite())
        .map(|(w, h)| Size::new(w.max(MIN_WINDOW_SIZE.width), h.max(MIN_WINDOW_SIZE.height)))
        .unwrap_or(Size::new(800.0, 600.0));
    let position = settings
        .window_position
        .filter(|(x, y)| x.is_finite() && y.is_finite() && x.abs() < 16_384.0 && y.abs() < 16_384.0)
        .map_or(window::Position::Default, |(x, y)| {
            window::Position::Specific(Point::new(x, y))
        });
    (size, position)
}

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

    // 저장된 창 배치 복원 (없으면 800×600 / OS 기본 위치).
    // RunConfigManager::new()도 load_settings를 호출하지만, 창 설정은 앱 상태 생성
    // 이전에 필요해 여기서 한 번 더 읽는다 (수 KB 파일 1회 읽기 — 공유 상태보다 단순).
    let (window_size, window_position) = restore_window_geometry(&services::load_settings());

    iced::application(RunConfigManager::new, update, view)
        .subscription(subscription)
        .font(D2CODING_FONT)
        .theme(theme)
        .style(application_style)
        .window(window::Settings {
            size: window_size,
            position: window_position,
            min_size: Some(MIN_WINDOW_SIZE),
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

#[cfg(test)]
mod tests {
    use super::*;
    use services::AppSettings;

    fn settings_with(size: Option<(f32, f32)>, position: Option<(f32, f32)>) -> AppSettings {
        AppSettings {
            window_size: size,
            window_position: position,
            ..AppSettings::default()
        }
    }

    #[test]
    fn geometry_defaults_when_unset() {
        let (size, position) = restore_window_geometry(&settings_with(None, None));
        assert_eq!(size, Size::new(800.0, 600.0));
        assert!(matches!(position, window::Position::Default));
    }

    #[test]
    fn geometry_clamps_size_to_minimum() {
        let (size, _) = restore_window_geometry(&settings_with(Some((100.0, 5000.0)), None));
        assert_eq!(size.width, MIN_WINDOW_SIZE.width);
        assert_eq!(size.height, 5000.0);
    }

    #[test]
    fn geometry_keeps_negative_multi_monitor_position() {
        // 주 모니터 왼쪽 배치는 음수 x가 정상이다 (실사용 값: -1552).
        let (_, position) = restore_window_geometry(&settings_with(None, Some((-1552.0, 197.0))));
        assert!(matches!(
            position,
            window::Position::Specific(p) if p.x == -1552.0 && p.y == 197.0
        ));
    }

    #[test]
    fn geometry_rejects_nonfinite_and_out_of_range() {
        // NaN 크기: clamp 전에 걸러져 기본 크기로 (f32::max의 NaN 세탁 방지 회귀 테스트).
        let (size, _) = restore_window_geometry(&settings_with(Some((f32::NAN, 600.0)), None));
        assert_eq!(size, Size::new(800.0, 600.0));
        let (size, _) = restore_window_geometry(&settings_with(Some((f32::INFINITY, 600.0)), None));
        assert_eq!(size, Size::new(800.0, 600.0));
        // 비정상 위치 → OS 기본 배치 폴백.
        let (_, position) = restore_window_geometry(&settings_with(None, Some((f32::NAN, 0.0))));
        assert!(matches!(position, window::Position::Default));
        let (_, position) = restore_window_geometry(&settings_with(None, Some((99_999.0, 0.0))));
        assert!(matches!(position, window::Position::Default));
    }
}
