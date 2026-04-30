use crate::messages::{ConfigurationDropPosition, Message};
use crate::models::{ConfigurationType, RunConfiguration};
use crate::utils::{ICON_DELETE, ICON_PLAY};
use iced::{
    Alignment::{self},
    Background, Border, Color, Element, Length, Padding, Theme,
    alignment::Horizontal,
    widget::text::Wrapping,
    widget::{
        Column, Space, button, column, container, mouse_area, row, scrollable, svg, text, tooltip,
    },
};
use unicode_width::UnicodeWidthChar;

const LIST_ITEM_CONTENT_HEIGHT: f32 = 32.0;
const LIST_ITEM_HEIGHT: f32 = 36.0;

pub fn configuration_list(
    configurations: &[RunConfiguration],
    selected_config_index: Option<usize>,
    name_max_width: usize,
    dragging_config_index: Option<usize>,
    hovered_drop_target: Option<(usize, ConfigurationDropPosition)>,
) -> Element<'_, Message> {
    let mut list_items = Column::new()
        .spacing(1)
        .padding(Padding::new(6.0).left(0.0).right(6.0));

    for (index, config) in configurations.iter().enumerate() {
        let item_state = ConfigurationItemState {
            index,
            is_selected: selected_config_index == Some(index),
            is_dragging: dragging_config_index == Some(index),
            is_drop_target: dragging_config_index.is_some()
                && hovered_drop_target.is_some_and(|(hovered_index, _)| hovered_index == index),
            dragging_config_index,
            hovered_drop_target,
            name_max_width,
        };

        list_items = list_items.push(view_configuration_item(config, item_state));
    }

    if dragging_config_index
        .is_some_and(|dragging_index| dragging_index + 1 != configurations.len())
    {
        let trailing_drop_target =
            hovered_drop_target == Some((configurations.len(), ConfigurationDropPosition::Before));

        list_items = list_items.push(view_trailing_drop_target(
            configurations.len(),
            trailing_drop_target,
        ));
    }

    scrollable(list_items)
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default()
                .width(4)
                .scroller_width(4)
                .spacing(0.0),
        ))
        .into()
}

#[derive(Clone, Copy)]
struct ConfigurationItemState {
    index: usize,
    is_selected: bool,
    is_dragging: bool,
    is_drop_target: bool,
    dragging_config_index: Option<usize>,
    hovered_drop_target: Option<(usize, ConfigurationDropPosition)>,
    name_max_width: usize,
}

fn view_configuration_item(
    config: &RunConfiguration,
    item_state: ConfigurationItemState,
) -> Element<'_, Message> {
    let content_row = row![
        selection_bar(item_state.is_selected, item_state.is_dragging),
        view_configuration_summary(
            config,
            item_state.is_selected,
            item_state.name_max_width,
            item_state.is_dragging,
        ),
        action_button(
            ICON_PLAY,
            16,
            24,
            Message::RunConfiguration(Some(item_state.index)),
        ),
        action_button(
            ICON_DELETE,
            16,
            24,
            Message::DeleteConfiguration(Some(item_state.index)),
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fixed(LIST_ITEM_CONTENT_HEIGHT));

    let item_area: Element<'_, Message> = mouse_area(
        container(content_row)
            .width(Length::Fill)
            .padding([2, 4])
            .style(move |theme| {
                configuration_item_style(
                    theme,
                    item_state.is_selected,
                    item_state.is_dragging,
                    item_state.is_drop_target,
                )
            }),
    )
    .on_press(Message::StartConfigurationDrag(item_state.index))
    .on_move(move |point| {
        Message::ConfigurationDragHovered(
            item_state.index,
            if point.y < LIST_ITEM_HEIGHT / 2.0 {
                ConfigurationDropPosition::Before
            } else {
                ConfigurationDropPosition::After
            },
        )
    })
    .on_exit(Message::ConfigurationDragExited(item_state.index))
    .into();

    let preview_position = drop_preview_position(
        item_state.dragging_config_index,
        item_state.hovered_drop_target,
        item_state.index,
        item_state.is_drop_target,
    );

    column![
        if preview_position.is_before {
            drop_gap()
        } else {
            Space::new().height(0).into()
        },
        item_area,
        if preview_position.is_after {
            drop_gap()
        } else {
            Space::new().height(0).into()
        },
    ]
    .spacing(0)
    .into()
}

fn view_trailing_drop_target(drop_index: usize, is_active: bool) -> Element<'static, Message> {
    let gap = if is_active {
        drop_gap()
    } else {
        trailing_drop_hit_area()
    };

    mouse_area(gap)
        .on_enter(Message::ConfigurationDragHovered(
            drop_index,
            ConfigurationDropPosition::Before,
        ))
        .on_move(move |_point| {
            Message::ConfigurationDragHovered(drop_index, ConfigurationDropPosition::Before)
        })
        .on_exit(Message::ConfigurationDragExited(drop_index))
        .into()
}

fn trailing_drop_hit_area() -> Element<'static, Message> {
    container(Space::new().height(0))
        .height(LIST_ITEM_HEIGHT)
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style::default())
        .into()
}

struct DropPreviewPosition {
    is_before: bool,
    is_after: bool,
}

fn drop_preview_position(
    dragging_config_index: Option<usize>,
    hovered_drop_target: Option<(usize, ConfigurationDropPosition)>,
    index: usize,
    is_drop_target: bool,
) -> DropPreviewPosition {
    if !is_drop_target
        || dragging_config_index.is_some_and(|dragging_index| {
            is_same_configuration_position(dragging_index, index, hovered_drop_target)
        })
    {
        return DropPreviewPosition {
            is_before: false,
            is_after: false,
        };
    }

    let is_after = hovered_drop_target == Some((index, ConfigurationDropPosition::After));

    DropPreviewPosition {
        is_before: !is_after,
        is_after,
    }
}

fn is_same_configuration_position(
    dragging_index: usize,
    index: usize,
    hovered_drop_target: Option<(usize, ConfigurationDropPosition)>,
) -> bool {
    hovered_drop_target.is_some_and(|(_, position)| {
        dragging_index == index
            || (dragging_index + 1 == index && position == ConfigurationDropPosition::Before)
            || (dragging_index == index + 1 && position == ConfigurationDropPosition::After)
    })
}

fn drop_gap() -> Element<'static, Message> {
    container(Space::new().height(0))
        .height(LIST_ITEM_HEIGHT)
        .width(Length::Fill)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();

            container::Style {
                background: Some(Background::Color(Color {
                    a: 0.14,
                    ..palette.primary.base.color
                })),
                border: Border {
                    width: 1.0,
                    color: Color {
                        a: 0.42,
                        ..palette.primary.strong.color
                    },
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            }
        })
        .into()
}

fn view_configuration_summary(
    config: &RunConfiguration,
    is_selected: bool,
    name_max_width: usize,
    is_dragging: bool,
) -> Element<'_, Message> {
    let display_name = truncate_text(&config.name, name_max_width);

    let summary = container(
        row![
            container(type_label(config.config_type.clone(), is_selected)).center_y(Length::Fill),
            Space::new().width(8),
            container(themed_name_text(display_name, is_selected))
                .width(Length::Fill)
                .center_y(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(24)
    .padding([2, 8])
    .style(move |theme| summary_style(theme, is_selected, is_dragging));

    tooltip(
        summary,
        view_name_tooltip(&config.name),
        tooltip::Position::Top,
    )
    .into()
}

fn truncate_text(name: &str, max_width: usize) -> String {
    const ELLIPSIS_WIDTH: usize = 3;

    if name
        .chars()
        .map(|ch| ch.width().unwrap_or(1))
        .sum::<usize>()
        <= max_width
    {
        return name.to_owned();
    }

    let mut truncated = String::new();
    let mut current_width = 0;

    for ch in name.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if current_width + ch_width + ELLIPSIS_WIDTH > max_width {
            break;
        }
        truncated.push(ch);
        current_width += ch_width;
    }

    format!("{truncated}...")
}

fn type_label(
    config_type: ConfigurationType,
    is_selected: bool,
) -> iced::widget::Container<'static, Message> {
    let label = match config_type {
        ConfigurationType::Application => "APP",
        ConfigurationType::ShellScript => "SH",
        ConfigurationType::Node => "Node",
    };

    container(themed_badge_text(label, is_selected))
        .padding([2, 6])
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            let base_color = match config_type {
                ConfigurationType::Application => palette.primary.base.color,
                ConfigurationType::ShellScript => palette.success.base.color,
                ConfigurationType::Node => palette.danger.base.color,
            };

            container::Style {
                background: Some(Background::Color(Color {
                    a: if is_selected { 0.32 } else { 0.24 },
                    ..base_color
                })),
                border: Border {
                    radius: 999.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..container::Style::default()
            }
        })
}

fn view_name_tooltip(name: &str) -> iced::widget::Container<'_, Message> {
    container(text(name).size(14))
        .padding(8)
        .style(name_tooltip_style)
}

fn action_button(
    icon: &'static [u8],
    icon_size: u16,
    button_size: u16,
    on_press: Message,
) -> iced::widget::Button<'static, Message> {
    button(icon_container(icon, icon_size))
        .width(u32::from(button_size))
        .height(u32::from(button_size))
        .padding(0)
        .on_press(on_press)
        .style(action_button_style)
}

fn icon_container(
    icon: &'static [u8],
    icon_size: u16,
) -> iced::widget::Container<'static, Message> {
    container(
        svg(svg::Handle::from_memory(icon))
            .width(u32::from(icon_size))
            .height(u32::from(icon_size))
            .style(|theme: &Theme, _status| svg::Style {
                color: Some(Color {
                    a: 0.60,
                    ..theme.extended_palette().background.base.text
                }),
            }),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
}

fn selection_bar(
    is_selected: bool,
    is_dragging: bool,
) -> iced::widget::Container<'static, Message> {
    let bar = container(Space::new().width(0))
        .width(2)
        .height(Length::Fixed(24.0));

    if is_dragging {
        bar.style(|theme| selection_bar_style(theme, true))
    } else if is_selected {
        bar.style(|theme| selection_bar_style(theme, false))
    } else {
        bar
    }
}

fn themed_name_text(label: String, is_selected: bool) -> iced::widget::Text<'static, Theme> {
    text(label)
        .align_x(Horizontal::Left)
        .width(Length::Fill)
        .size(14)
        .line_height(1.0)
        .wrapping(Wrapping::None)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(Color {
                a: if is_selected { 0.94 } else { 0.74 },
                ..theme.extended_palette().background.base.text
            }),
        })
}

fn themed_badge_text(label: &'static str, is_selected: bool) -> iced::widget::Text<'static, Theme> {
    text(label)
        .size(10)
        .style(move |theme: &Theme| iced::widget::text::Style {
            color: Some(Color {
                a: if is_selected { 0.86 } else { 0.64 },
                ..theme.extended_palette().background.base.text
            }),
        })
}

fn name_tooltip_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(iced::Background::Color(palette.background.base.color)),
        border: Border {
            width: 1.0,
            color: Color {
                a: 0.10,
                ..palette.background.strong.color
            },
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn selection_bar_style(theme: &Theme, is_dragging: bool) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(iced::Background::Color(if is_dragging {
            palette.primary.strong.color
        } else {
            Color {
                a: 0.88,
                ..palette.primary.base.color
            }
        })),
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn configuration_item_style(
    theme: &Theme,
    is_selected: bool,
    is_dragging: bool,
    is_drop_target: bool,
) -> container::Style {
    let palette = theme.extended_palette();

    let background = if is_dragging {
        Color {
            a: 0.10,
            ..palette.primary.base.color
        }
    } else if is_drop_target {
        Color {
            a: 0.05,
            ..palette.background.weak.color
        }
    } else if is_selected {
        Color {
            a: 0.04,
            ..palette.background.base.text
        }
    } else {
        Color::TRANSPARENT
    };

    let border_color = if is_dragging {
        Color {
            a: 0.42,
            ..palette.primary.strong.color
        }
    } else if is_drop_target {
        Color {
            a: 0.10,
            ..palette.background.strong.color
        }
    } else {
        Color::TRANSPARENT
    };

    container::Style {
        background: Some(Background::Color(background)),
        border: Border {
            width: if is_dragging || is_drop_target {
                1.0
            } else {
                0.0
            },
            color: border_color,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn action_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    let background = match status {
        button::Status::Hovered => Color {
            a: 0.10,
            ..palette.background.strong.color
        },
        button::Status::Pressed => Color {
            a: 0.28,
            ..palette.primary.base.color
        },
        _ => Color::TRANSPARENT,
    };

    button::Style {
        background: Some(Background::Color(background)),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
        text_color: Color {
            a: 0.66,
            ..palette.background.base.text
        },
        ..button::Style::default()
    }
}

fn summary_style(theme: &Theme, is_selected: bool, is_dragging: bool) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(if is_dragging {
            Color {
                a: 0.06,
                ..palette.background.strong.color
            }
        } else if is_selected {
            Color {
                a: 0.05,
                ..palette.background.base.text
            }
        } else {
            Color::TRANSPARENT
        })),
        border: Border {
            width: 0.0,
            color: Color::TRANSPARENT,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}
