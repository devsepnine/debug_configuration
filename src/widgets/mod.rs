// Custom widgets module.
//
// We keep a vendored `pane_grid` because upstream iced 0.14 still excludes the
// title text area from the drag pick area in `TitleBar::is_over_pick_area`,
// which breaks dragging when users click the title text itself.

pub mod configuration_list;
pub mod pane_grid;
