use crate::messages::Message;
use crate::utils::{ICON_ADD, ICON_FOLDER_OPEN, ICON_SAVE, ICON_SETTINGS};
use crate::views::shared::{IconButtonState, icon_button_foreground, icon_tooltip};
use iced::{
    Element, Length, Theme,
    widget::{button, container, row, svg, tooltip},
};

/// `Configurations` 헤더 액션 렌더링
///
/// 추가, 열기, 저장 아이콘 버튼을 헤더 우측에 배치
pub fn view_toolbar() -> Element<'static, Message> {
    let add_btn = toolbar_button("Add", ICON_ADD, Message::AddConfiguration);
    let open_btn = toolbar_button("Open", ICON_FOLDER_OPEN, Message::OpenConfigurations);
    let save_btn = toolbar_button("Save", ICON_SAVE, Message::SaveConfigurations);
    let settings_btn = toolbar_button("Settings", ICON_SETTINGS, Message::OpenSettingsModal);

    row![add_btn, open_btn, save_btn, settings_btn]
        .spacing(4)
        .into()
}

fn toolbar_button(
    label: &'static str,
    icon: &'static [u8],
    on_press: Message,
) -> Element<'static, Message> {
    let icon = container(
        svg(svg::Handle::from_memory(icon))
            .width(18)
            .height(18)
            .style(toolbar_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    let button = button(icon)
        .width(28)
        .height(28)
        .padding(0)
        .on_press(on_press)
        .style(icon_button_style);

    tooltip(button, icon_tooltip(label), tooltip::Position::Bottom).into()
}

fn toolbar_icon_style(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(icon_button_foreground(theme, IconButtonState::Active)),
    }
}

fn icon_button_style(theme: &Theme, status: button::Status) -> button::Style {
    crate::views::shared::icon_button_style(theme, status, IconButtonState::Active, 4.0)
}
