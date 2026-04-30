use crate::messages::Message;
use crate::models::WorkspaceTab;
use crate::utils::{ICON_ADD, ICON_CLOSE};
use crate::views::shared::{IconButtonState, icon_button_foreground, icon_button_style};
use iced::{
    Alignment, Border, Color, Element, Length, Shadow, Theme,
    widget::{Space, button, container, mouse_area, row, scrollable, svg, text, text_input},
};

/// 워크스페이스 탭 바 렌더링
///
/// # Arguments
/// * `tabs` - 워크스페이스 탭 목록
/// * `selected_index` - 현재 선택된 탭 인덱스
/// * `editing_tab_name` - 편집 중인 탭 정보 (`tab_index`, `current_value`)
pub fn view_workspace_tab_bar<'a>(
    tabs: &'a [WorkspaceTab],
    selected_index: usize,
    hovered_index: Option<usize>,
    editing_tab_name: Option<&'a (usize, String)>,
) -> Element<'a, Message> {
    let mut tabs_row = row![]
        .height(32)
        .spacing(6)
        .padding(0)
        .align_y(Alignment::Center);

    for (index, tab) in tabs.iter().enumerate() {
        let is_editing = editing_tab_name.is_some_and(|(idx, _)| *idx == index);
        let show_close_button = is_editing || hovered_index == Some(index);
        let tab_content = row![
            view_tab_label(tab, is_editing, editing_tab_name),
            view_close_button(index, is_editing, show_close_button)
        ]
        .align_y(Alignment::Center)
        .spacing(4);
        let tab_content = container(tab_content)
            .height(Length::Fill)
            .center_y(Length::Fill);

        let tab_button = button(tab_content)
            .height(Length::Fill)
            .padding([0, 12])
            .style(move |theme: &Theme, status| {
                tab_button_style(theme, status, index == selected_index)
            })
            .on_press(Message::TabNameClicked(index));

        // mouse_area로 감싸서 드래그 중 탭 hover 감지
        let tab_with_hover = mouse_area(tab_button)
            .on_enter(Message::TabBarHovered(Some(index)))
            .on_exit(Message::TabBarHovered(None));

        tabs_row = tabs_row.push(tab_with_hover);
    }

    tabs_row = tabs_row.push(view_add_tab_button());

    // 탭 바를 가로 스크롤 가능하게
    let scrollable_tabs = scrollable(tabs_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new()
                .width(4.0)
                .spacing(0.0)
                .scroller_width(4),
        ))
        .height(Length::Shrink);

    container(scrollable_tabs)
        .width(Length::Fill)
        .padding(0)
        .style(tab_bar_style)
        .into()
}

fn view_add_tab_button() -> Element<'static, Message> {
    let add_icon = container(
        svg(svg::Handle::from_memory(ICON_ADD))
            .width(12)
            .height(12)
            .style(add_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    button(add_icon)
        .width(32)
        .height(Length::Fill)
        .padding(0)
        .style(add_tab_button_style)
        .on_press(Message::AddWorkspaceTab)
        .into()
}

fn view_tab_label<'a>(
    tab: &'a WorkspaceTab,
    is_editing: bool,
    editing_tab_name: Option<&'a (usize, String)>,
) -> Element<'a, Message> {
    if is_editing {
        let current_value = editing_tab_name.map_or("", |(_, value)| value.as_str());

        return text_input("Tab name", current_value)
            .size(12)
            .width(Length::Fixed(100.0))
            .on_input(Message::TabNameInputChanged)
            .on_submit(Message::FinishEditingTabName)
            .into();
    }

    text(&tab.name)
        .size(12)
        .wrapping(text::Wrapping::None)
        .into()
}

fn view_close_button(
    tab_index: usize,
    is_editing: bool,
    is_visible: bool,
) -> Element<'static, Message> {
    if !is_visible {
        return Space::new().width(16).height(16).into();
    }

    let close_icon = container(
        svg(svg::Handle::from_memory(ICON_CLOSE))
            .width(10)
            .height(10)
            .style(close_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);
    let close_message = if is_editing {
        Message::CancelEditingTabName
    } else {
        Message::CloseTab(tab_index)
    };

    button(close_icon)
        .width(16)
        .height(16)
        .padding(0)
        .style(close_button_style)
        .on_press(close_message)
        .into()
}

fn close_icon_style(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(icon_button_foreground(theme, IconButtonState::Active)),
    }
}

fn add_icon_style(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(icon_button_foreground(theme, IconButtonState::Active)),
    }
}

fn close_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let _ = (theme, status);

    button::Style {
        background: None,
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 0.0.into(),
        },
        ..button::Style::default()
    }
}

fn add_tab_button_style(theme: &Theme, status: button::Status) -> button::Style {
    icon_button_style(theme, status, IconButtonState::Active, 6.0)
}

fn tab_button_style(theme: &Theme, status: button::Status, is_selected: bool) -> button::Style {
    let palette = theme.extended_palette();
    let _ = status;

    button::Style {
        background: None,
        text_color: if is_selected {
            Color {
                a: 0.92,
                ..palette.background.base.text
            }
        } else {
            Color {
                a: 0.74,
                ..palette.background.base.text
            }
        },
        border: Border {
            width: if is_selected { 1.0 } else { 0.0 },
            color: if is_selected {
                Color {
                    a: 0.22,
                    ..palette.primary.strong.color
                }
            } else {
                Color::TRANSPARENT
            },
            radius: 6.0.into(),
        },
        shadow: if is_selected {
            Shadow {
                color: Color {
                    a: 0.08,
                    ..palette.primary.base.color
                },
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 6.0,
            }
        } else {
            Shadow::default()
        },
        ..button::Style::default()
    }
}

fn tab_bar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}
