use crate::messages::{ConfigurationDropPosition, Message};
use crate::models::RunConfiguration;

pub fn view_configuration_list(
    configurations: &[RunConfiguration],
    selected_config_index: Option<usize>,
    name_max_width: usize,
    dragging_config_index: Option<usize>,
    hovered_drop_target: Option<(usize, ConfigurationDropPosition)>,
) -> iced::Element<'_, Message> {
    crate::widgets::configuration_list::configuration_list(
        configurations,
        selected_config_index,
        name_max_width,
        dragging_config_index,
        hovered_drop_target,
    )
}
