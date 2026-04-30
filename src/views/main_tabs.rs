use crate::messages::{Message, ViewMode};
use crate::utils::{ICON_CONFIGURATIONS, ICON_SESSIONS};
use crate::views::shared::icon_tooltip;
use iced::{
    Background, Border, Color, Element, Length, Shadow, Theme,
    widget::{Space, button, container, row, stack, svg, tooltip},
};

/// 메인 네비게이션 탭 렌더링
///
/// 구성 관리와 실행 세션 간 전환을 위한 탭 버튼
///
/// # Arguments
/// * `current_view` - 현재 활성화된 뷰 모드
/// * `has_sessions` - 실행 중인 세션이 있는지 여부
pub fn view_main_tabs(current_view: ViewMode, has_sessions: bool) -> Element<'static, Message> {
    let config_btn = view_main_tab_button(
        "Configurations",
        ICON_CONFIGURATIONS,
        current_view == ViewMode::Configuration,
        Message::SwitchView(ViewMode::Configuration),
        false,
    );
    let sessions_btn = view_main_tab_button(
        "Sessions",
        ICON_SESSIONS,
        current_view == ViewMode::Sessions,
        Message::SwitchView(ViewMode::Sessions),
        has_sessions,
    );

    row![config_btn, sessions_btn].spacing(4).into()
}

fn view_main_tab_button(
    label: &'static str,
    icon: &'static [u8],
    is_active: bool,
    on_press: Message,
    show_indicator: bool,
) -> Element<'static, Message> {
    let button = button(main_tab_icon(icon, is_active, show_indicator))
        .padding(0)
        .width(30)
        .height(24)
        .on_press(on_press)
        .style(move |theme: &Theme, status| main_tab_style(theme, status, is_active));

    tooltip(button, icon_tooltip(label), tooltip::Position::Bottom).into()
}

fn main_tab_icon(
    icon: &'static [u8],
    is_active: bool,
    show_indicator: bool,
) -> Element<'static, Message> {
    let icon = container(
        svg(svg::Handle::from_memory(icon))
            .width(15)
            .height(15)
            .style(move |theme: &Theme, status| main_tab_icon_style(theme, status, is_active)),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    if show_indicator {
        stack![
            icon,
            container(live_session_dot())
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Top)
                .padding([3, 6]),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        icon.into()
    }
}

fn live_session_dot() -> iced::widget::Container<'static, Message> {
    container(Space::new())
        .width(7)
        .height(7)
        .style(live_session_dot_style)
}

fn main_tab_icon_style(theme: &Theme, _status: svg::Status, is_active: bool) -> svg::Style {
    svg::Style {
        color: Some(main_tab_foreground_color(theme, is_active)),
    }
}

fn main_tab_foreground_color(theme: &Theme, is_active: bool) -> Color {
    let palette = theme.extended_palette();

    if is_active {
        palette.primary.base.text
    } else {
        Color {
            a: 0.78,
            ..palette.background.base.text
        }
    }
}

fn live_session_dot_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(palette.success.base.color)),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        shadow: Shadow {
            color: Color {
                a: 0.35,
                ..palette.success.base.color
            },
            offset: iced::Vector::ZERO,
            blur_radius: 8.0,
        },
        ..container::Style::default()
    }
}

fn main_tab_style(theme: &Theme, status: button::Status, is_active: bool) -> button::Style {
    let palette = theme.extended_palette();

    let background = if is_active {
        Color {
            a: 0.28,
            ..palette.primary.base.color
        }
    } else {
        match status {
            button::Status::Hovered => Color {
                a: 0.46,
                ..palette.background.strong.color
            },
            button::Status::Pressed => Color {
                a: 0.24,
                ..palette.primary.weak.color
            },
            _ => Color::TRANSPARENT,
        }
    };

    let border = if is_active {
        Border {
            width: 1.0,
            color: Color {
                a: 0.22,
                ..palette.primary.strong.color
            },
            radius: 6.0.into(),
        }
    } else {
        Border {
            width: 1.0,
            color: Color::TRANSPARENT,
            radius: 6.0.into(),
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        border,
        text_color: main_tab_foreground_color(theme, is_active),
        shadow: if is_active {
            Shadow {
                color: Color {
                    a: 0.18,
                    ..palette.background.base.text
                },
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 10.0,
            }
        } else {
            Shadow::default()
        },
        ..button::Style::default()
    }
}
