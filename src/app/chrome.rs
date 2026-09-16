//! 윈도우 chrome / 세션 패널의 프레젠테이션(스타일) 헬퍼.
//!
//! `RunConfigManager`의 상태에 의존하지 않는 순수 스타일 함수들을 한곳에 모아
//! `app.rs`(컨트롤러/모델)와 분리한다.

use crate::models::RunSession;
use crate::views::shared::{
    IconButtonState, chrome_border_color, icon_button_foreground, icon_button_style,
};
use iced::widget::{button, container, svg, text};
use iced::{Background, Border, Color, Theme, border};

/// 세션 리스트 항목의 액션 버튼 종류 (스타일 분기용).
#[derive(Clone, Copy)]
pub(crate) enum SessionActionKind {
    Stop,
    Remove,
}

/// 플랫폼별 윈도우 모서리 반경.
pub(crate) fn window_radius() -> f32 {
    if cfg!(target_os = "windows") {
        8.0
    } else {
        10.0
    }
}

pub(crate) fn window_chrome_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: window_radius().into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn window_border_overlay_style(theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border {
            width: 1.0,
            color: chrome_border_color(theme),
            radius: window_radius().into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn status_bar_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.32,
            ..theme.extended_palette().background.base.color
        })),
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: border::Radius::default().bottom(window_radius()),
        },
        ..container::Style::default()
    }
}

/// 상태바 우측 버전/업데이트 버튼 스타일. 배경 없는 텍스트 링크 형태이며,
/// 새 버전이 있으면 강조색(primary)으로, 없으면 흐린 텍스트색으로 표시한다.
pub(crate) fn status_bar_version_style(
    theme: &Theme,
    status: button::Status,
    has_update: bool,
) -> button::Style {
    let palette = theme.extended_palette();
    let base = if has_update {
        palette.primary.base.color
    } else {
        Color {
            a: 0.5,
            ..palette.background.base.text
        }
    };
    let text_color = match status {
        button::Status::Hovered | button::Status::Pressed => Color { a: 1.0, ..base },
        _ => base,
    };

    button::Style {
        background: None,
        text_color,
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

pub(crate) fn status_bar_top_border_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome_border_color(theme))),
        ..container::Style::default()
    }
}

pub(crate) fn session_list_panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(Color {
            a: 0.56,
            ..palette.background.base.color
        })),
        border: Border {
            width: 1.0,
            color: chrome_border_color(theme),
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn workspace_tab_bar_island_style(theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border {
            width: 1.0,
            color: chrome_border_color(theme),
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn workspace_content_island_style(theme: &Theme) -> container::Style {
    let _ = theme;

    container::Style {
        background: None,
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn workspace_content_surface_style(theme: &Theme) -> container::Style {
    let _ = theme;

    container::Style {
        background: None,
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn empty_workspace_content_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(Color {
            a: 0.03,
            ..palette.background.base.text
        })),
        text_color: Some(Color {
            a: 0.64,
            ..palette.background.base.text
        }),
        border: Border {
            width: 1.0,
            color: Color {
                a: 0.08,
                ..palette.background.base.text
            },
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn session_empty_state_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.04,
            ..theme.extended_palette().background.base.text
        })),
        border: Border {
            width: 1.0,
            color: Color {
                a: 0.08,
                ..theme.extended_palette().background.base.text
            },
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn session_list_item_container_style(
    theme: &Theme,
    is_open_in_workspace: bool,
    is_hovered: bool,
) -> container::Style {
    let palette = theme.extended_palette();
    let background = if is_open_in_workspace {
        Some(Color {
            a: 0.14,
            ..palette.primary.base.color
        })
    } else if is_hovered {
        Some(Color {
            a: 0.07,
            ..palette.background.base.text
        })
    } else {
        None
    };
    let border_alpha = if is_hovered { 0.08 } else { 0.0 };

    container::Style {
        background: background.map(Background::Color),
        text_color: Some(palette.background.base.text),
        border: Border {
            width: 1.0,
            color: Color {
                a: border_alpha,
                ..palette.background.base.text
            },
            radius: 7.0.into(),
        },
        ..container::Style::default()
    }
}

pub(crate) fn session_action_button_style(
    theme: &Theme,
    status: button::Status,
    _kind: SessionActionKind,
    is_active: bool,
) -> button::Style {
    let state = if is_active {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };

    icon_button_style(theme, status, state, 6.0)
}

pub(crate) fn session_action_icon_style(
    theme: &Theme,
    _kind: SessionActionKind,
    is_active: bool,
) -> svg::Style {
    svg::Style {
        color: Some(if is_active {
            icon_button_foreground(theme, IconButtonState::Active)
        } else {
            icon_button_foreground(theme, IconButtonState::Inactive)
        }),
    }
}

/// 종료 세션 구분선의 라벨/카운트. 목록의 주인공은 실행 중인 세션이므로 경계 표시는
/// 배경에 가깝게 죽인다.
pub(crate) fn session_divider_label_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(Color {
            a: 0.45,
            ..theme.extended_palette().background.base.text
        }),
    }
}

/// 구분선의 실선(1px). 상태바 상단선과 같은 chrome 경계색을 쓴다 — 창 안의 hairline은
/// 한 가지 색으로 통일해야 패널마다 경계 밝기가 달라 보이지 않는다.
pub(crate) fn session_divider_line_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome_border_color(theme))),
        ..container::Style::default()
    }
}

pub(crate) fn session_list_status_dot_style(
    theme: &Theme,
    session: &RunSession,
) -> container::Style {
    let palette = theme.extended_palette();
    let (color, alpha) = if session.is_running {
        (palette.success.base.color, 0.92)
    } else if let Some(code) = session.exit_code {
        if code == 0 {
            (palette.background.base.text, 0.42)
        } else {
            (palette.danger.base.color, 0.88)
        }
    } else {
        (palette.background.base.text, 0.24)
    };

    container::Style {
        background: Some(Background::Color(Color { a: alpha, ..color })),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
