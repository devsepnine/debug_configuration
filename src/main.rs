#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

/// `IntelliJ` 스타일 Run/Debug 구성 관리자
///
/// iced GUI 프레임워크를 사용하여 Elm Architecture 패턴을 따름
/// - Model: `app::RunConfigManager`
/// - Update: `app::RunConfigManager::update`
/// - View: `app::RunConfigManager::view`
mod ansi;
mod app;
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
const D2CODING_FONT: &[u8] = include_bytes!("../fonts/D2Coding.ttf");

/// D2 Coding 폰트 정의
pub const D2CODING: Font = Font::with_name("D2Coding");

/// 애플리케이션 진입점
fn main() -> iced::Result {
    // 시그널 핸들러 등록: Ctrl+C, SIGINT, SIGTERM 캐치
    // 시그널 수신 시 std::process::exit(0) 호출 → Drop trait 실행 → 프로세스 정리
    ctrlc::set_handler(move || {
        eprintln!("\n[Signal] Termination signal received (Ctrl+C / SIGTERM)");
        eprintln!("[Signal] Initiating graceful shutdown...");
        std::process::exit(0);
    })
    .expect("Failed to set signal handler");

    eprintln!("[Startup] Run/Debug Configuration Manager");
    eprintln!("[Startup] Press Ctrl+C for graceful shutdown");

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
