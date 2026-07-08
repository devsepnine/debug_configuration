use crate::messages::Message;
use iced::{
    Background, Border, Color, Shadow, Theme,
    widget::{button, container, text},
};
use uuid::Uuid;

/// 세션 출력 검색바 입력 위젯의 안정적 Id (세션별 고유). Ctrl+F 포커스(app)와
/// text_input `.id()`(pane_view)가 동일 Id를 공유하도록 한 곳에서 생성한다.
pub(crate) fn session_search_input_id(session_id: Uuid) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!("session-search-{session_id}"))
}

/// 세션 stdin 입력바 위젯의 안정적 Id (세션별 고유). Cmd+I 포커스(app)와
/// text_input `.id()`(pane_view)가 동일 Id를 공유한다.
pub(crate) fn session_stdin_input_id(session_id: Uuid) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!("session-stdin-{session_id}"))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum IconButtonState {
    Active,
    Inactive,
}

pub(crate) fn icon_button_style(
    theme: &Theme,
    status: button::Status,
    state: IconButtonState,
    radius: f32,
) -> button::Style {
    let palette = theme.extended_palette();
    let is_disabled =
        matches!(state, IconButtonState::Inactive) || matches!(status, button::Status::Disabled);
    let background = if is_disabled {
        Color {
            a: 0.0,
            ..palette.background.base.text
        }
    } else {
        match status {
            button::Status::Hovered => Color {
                a: 0.24,
                ..palette.primary.base.color
            },
            button::Status::Pressed => Color {
                a: 0.34,
                ..palette.primary.base.color
            },
            _ => Color {
                a: 0.16,
                ..palette.primary.base.color
            },
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: icon_button_foreground(theme, state),
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: radius.into(),
        },
        ..button::Style::default()
    }
}

pub(crate) fn icon_button_foreground(theme: &Theme, state: IconButtonState) -> Color {
    let palette = theme.extended_palette();

    match state {
        IconButtonState::Active => Color {
            a: 0.94,
            ..palette.primary.strong.color
        },
        IconButtonState::Inactive => Color {
            a: 0.22,
            ..palette.background.base.text
        },
    }
}

pub(crate) fn chrome_border_color(theme: &Theme) -> Color {
    Color {
        a: 0.10,
        ..theme.extended_palette().background.base.text
    }
}

pub fn icon_tooltip(label: &'static str) -> iced::widget::Container<'static, Message> {
    container(text(label).size(12))
        .padding([5, 8])
        .style(icon_tooltip_style)
}

fn icon_tooltip_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        text_color: Some(palette.background.strong.text),
        border: Border {
            width: 1.0,
            color: Color {
                a: 0.18,
                ..palette.background.base.text
            },
            radius: 6.0.into(),
        },
        shadow: Shadow {
            color: Color {
                a: 0.24,
                ..palette.background.base.text
            },
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}
