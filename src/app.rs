use crate::messages::{ConfigurationDropPosition, Message, ViewMode};
use crate::models::{
    ConfigTypeData, ConfigurationType, DropZone, ExecuteMode, ExecuteModeType, LayoutId,
    PackageManager, RunConfiguration, RunSession, WorkspaceTab,
};
use crate::services::{
    AppSettings, load_from_path, load_settings, open_configurations, run_configuration_stream,
    save_configurations, save_settings,
};
use crate::utils::{DIALOG_CANCELLED, ICON_DELETE, ICON_STOP};
use crate::views::shared::{
    IconButtonState, chrome_border_color, icon_button_foreground, icon_button_style, icon_tooltip,
};
use crate::views::{
    EditorLoadingState, EditorSelectState, EnvModalView, FileDialogLoadingState, NodeLoadingState,
    env_modal_key_id, env_modal_value_id, view_configuration_editor, view_configuration_list,
    view_env_modal, view_main_tabs, view_pane_layout, view_toolbar, view_workspace_tab_bar,
};
use crate::widgets::pane_grid;
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Size, Subscription, Task, Theme,
    border, event, keyboard, mouse,
    widget::{
        Space, button, column, container, mouse_area, row, scrollable, stack, svg, text, tooltip,
    },
    window,
};
use rfd::AsyncFileDialog;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Instant, SystemTime};
use uuid::Uuid;

#[derive(Default)]
struct FileDialogState {
    is_loading_folder: bool,
    is_loading_script_file: bool,
    is_loading_interpreter: bool,
}

#[derive(Default)]
struct NodeUiState {
    is_loading_node_scripts: bool,
    is_loading_project_directory: bool,
}

#[derive(Default)]
struct DragState {
    is_dragging_pane: bool,
    pending_configuration_index: Option<usize>,
    pending_configuration_origin: Option<iced::Point>,
    dragging_configuration_index: Option<usize>,
    hovered_configuration_target: Option<(usize, ConfigurationDropPosition)>,
    tab_dragging: Option<(Uuid, pane_grid::Pane)>,
    hovered_pane: Option<pane_grid::Pane>,
    last_hovered_pane: Option<pane_grid::Pane>,
    hovered_drop_zone: Option<(pane_grid::Pane, DropZone)>,
    hovered_outer_zone: Option<DropZone>,
    hovered_tab_index: Option<usize>,
    cursor_position: Option<iced::Point>,
    dragging_pane_id: Option<pane_grid::Pane>,
}

#[derive(Default)]
struct TabUiState {
    editing_tab_name: Option<(usize, String)>,
    last_tab_name_click: Option<(usize, Instant)>,
}

#[derive(Default)]
struct ConfigurationUiState {
    editor_focus_area_active: bool,
}

/// 환경변수 편집 모달의 staging 상태 (always-inline 패턴).
///
/// 모든 row가 항상 편집 가능. 모달 OK 시 entries를 `environment_variables`에 commit
/// (빈 키 row 제거 + HashMap 변환으로 자동 dedupe). Cancel 시 폐기.
pub struct EnvModalState {
    /// 어떤 구성의 모달인지
    pub config_id: Uuid,
    /// staging entries (입력 순서 유지). 빈 row는 view에서 자동 표시되며
    /// 사용자가 빈 row에 타이핑하기 시작하면 entries 끝에 push됨.
    pub entries: Vec<(String, String)>,
    /// Tab cycle 추적용 — 현재 focus 가진 cell. 마우스 클릭으로 외부 변경된 경우
    /// 다음 Tab까지는 stale일 수 있으나 cycle 자체는 항상 모달 내부에 머무름.
    pub focused_cell: ModalCell,
}

/// 모달 내부의 한 input cell 위치 (row index + key/value 구분).
#[derive(Debug, Clone, Copy)]
pub struct ModalCell {
    pub row: usize,
    pub kind: ModalCellKind,
}

#[derive(Debug, Clone, Copy)]
pub enum ModalCellKind {
    Key,
    Value,
}

impl ModalCell {
    /// row * 2 + (Key=0, Value=1). cycle 모듈로 연산용.
    pub fn linear_index(self) -> usize {
        self.row * 2
            + match self.kind {
                ModalCellKind::Key => 0,
                ModalCellKind::Value => 1,
            }
    }

    /// linear_index의 역변환.
    pub fn from_linear(linear: usize) -> Self {
        let row = linear / 2;
        let kind = if linear.is_multiple_of(2) {
            ModalCellKind::Key
        } else {
            ModalCellKind::Value
        };
        Self { row, kind }
    }
}

#[derive(Debug, Clone, Copy)]
enum ConfigurationScreenPane {
    Configurations,
    Editor,
}

#[derive(Clone, Copy)]
enum SessionActionKind {
    Stop,
    Remove,
}

fn configuration_split_ratio(state: &pane_grid::State<ConfigurationScreenPane>) -> f32 {
    match state.layout() {
        pane_grid::Node::Split { ratio, .. } => *ratio,
        pane_grid::Node::Pane(_) => 0.30,
    }
}

fn configuration_name_max_width(window_size: Size, split_ratio: f32) -> usize {
    const ROOT_HORIZONTAL_PADDING: f32 = 20.0;
    const PANE_SPACING: f32 = 12.0;
    const AVERAGE_CHARACTER_WIDTH: f32 = 7.4;
    const ITEM_HORIZONTAL_PADDING: f32 = 8.0;
    const SELECTION_BAR_WIDTH: f32 = 2.0;
    const MOVE_BUTTON_COLUMN_WIDTH: f32 = 12.0;
    const ITEM_GAP_WIDTH: f32 = 14.0;
    const ACTION_BUTTONS_WIDTH: f32 = 54.0;
    const SUMMARY_HORIZONTAL_PADDING: f32 = 16.0;
    const TYPE_BADGE_WIDTH: f32 = 44.0;
    const BADGE_NAME_GAP: f32 = 8.0;

    let list_reserved_width = ITEM_HORIZONTAL_PADDING
        + SELECTION_BAR_WIDTH
        + MOVE_BUTTON_COLUMN_WIDTH
        + ITEM_GAP_WIDTH
        + ACTION_BUTTONS_WIDTH
        + SUMMARY_HORIZONTAL_PADDING
        + TYPE_BADGE_WIDTH
        + BADGE_NAME_GAP;

    let content_width = (window_size.width - ROOT_HORIZONTAL_PADDING).max(600.0);
    let left_pane_width = ((content_width - PANE_SPACING) * split_ratio).max(260.0);
    let title_width = (left_pane_width - list_reserved_width).max(72.0);

    (title_width / AVERAGE_CHARACTER_WIDTH)
        .floor()
        .to_string()
        .parse::<usize>()
        .unwrap_or(12)
        .clamp(8, 48)
}

/// 애플리케이션의 메인 상태를 관리하는 구조체
/// Elm Architecture의 Model에 해당
pub struct RunConfigManager {
    /// 현재 활성화된 뷰 모드 (구성 관리 / 실행 세션)
    current_view: ViewMode,

    /// 저장된 실행 구성 목록
    configurations: Vec<RunConfiguration>,
    /// 현재 선택된 구성의 인덱스
    selected_config_index: Option<usize>,
    /// 상태바 메시지
    status_message: String,

    /// 각 구성별 환경변수 메인 input의 raw text (`config_id` -> `KEY=val;...`)
    /// `None`이면 `environment_variables`를 직렬화한 값을 표시
    env_bulk_inputs: HashMap<Uuid, String>,
    /// 환경변수 편집 모달 상태 (`Some`이면 모달 열림)
    env_modal: Option<EnvModalState>,
    file_dialog: FileDialogState,

    /// Node package script 캐시 (`config_id` 기준)
    node_available_scripts: HashMap<Uuid, Vec<String>>,
    node_ui: NodeUiState,
    editor_select_state: EditorSelectState,
    /// Node 프로젝트에서 발견된 `package.json` 목록 (`config_id` -> 상대 경로 리스트)
    node_available_package_jsons: HashMap<Uuid, Vec<String>>,
    /// 시스템에서 감지된 Node.js Runtime 목록 (label, path)
    available_node_runtimes: Vec<(String, String)>,

    /// 실행 중인 세션 목록
    sessions: Vec<RunSession>,
    /// 현재 선택된 세션의 인덱스
    selected_session_index: Option<usize>,
    /// 세션 리스트에서 hover 중인 항목
    hovered_session_index: Option<usize>,

    /// 워크스페이스 탭 목록 (각 탭은 독립적인 `pane_grid` 레이아웃을 가짐)
    workspace_tabs: Vec<WorkspaceTab>,
    /// 현재 선택된 워크스페이스 탭 인덱스
    selected_tab_index: usize,
    configuration_layout: pane_grid::State<ConfigurationScreenPane>,
    configuration_ui: ConfigurationUiState,

    drag: DragState,
    tab_ui: TabUiState,
    window_id: Option<window::Id>,
    window_size: Size,
    is_window_maximized: bool,
    /// 마지막으로 사용한 구성 파일 경로 (Open/Save)
    last_file_path: Option<PathBuf>,
}

impl RunConfigManager {
    /// 새로운 애플리케이션 인스턴스 생성 및 초기화
    ///
    /// # Returns
    /// (애플리케이션 인스턴스, 초기 Task)
    /// 마지막으로 사용한 파일이 있으면 자동으로 로드
    pub fn new() -> (Self, Task<Message>) {
        // 앱 설정에서 마지막 파일 경로 확인
        let settings = load_settings();
        let last_file_path = settings.last_file_path.clone();
        let configuration_split_ratio = settings.configuration_split_ratio.unwrap_or(0.30);

        let app = Self {
            current_view: ViewMode::Configuration,
            configurations: vec![],
            selected_config_index: None,
            status_message: String::from("Ready"),
            env_bulk_inputs: HashMap::new(),
            env_modal: None,
            file_dialog: FileDialogState::default(),
            node_available_scripts: HashMap::new(),
            node_ui: NodeUiState::default(),
            editor_select_state: EditorSelectState::new(),
            node_available_package_jsons: HashMap::new(),
            available_node_runtimes: vec![("Default (system)".to_string(), "node".to_string())],
            sessions: vec![],
            selected_session_index: None,
            hovered_session_index: None,
            workspace_tabs: vec![WorkspaceTab::empty(String::from("Workspace"))],
            selected_tab_index: 0,
            configuration_layout: pane_grid::State::with_configuration(
                pane_grid::Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: configuration_split_ratio,
                    a: Box::new(pane_grid::Configuration::Pane(
                        ConfigurationScreenPane::Configurations,
                    )),
                    b: Box::new(pane_grid::Configuration::Pane(
                        ConfigurationScreenPane::Editor,
                    )),
                },
            ),
            configuration_ui: ConfigurationUiState::default(),
            drag: DragState::default(),
            tab_ui: TabUiState::default(),
            window_id: None,
            window_size: Size::new(800.0, 600.0),
            is_window_maximized: false,
            last_file_path: last_file_path.clone(),
        };

        // 초기화 Task들
        let mut tasks = vec![];

        // 1. Node Runtime 감지 (백그라운드)
        tasks.push(Task::perform(
            async { crate::utils::detect_node_runtimes() },
            Message::NodeRuntimesDetected,
        ));

        // 2. 마지막 파일이 있으면 자동 로드
        if let Some(path) = last_file_path
            && path.exists()
        {
            tasks.push(Task::perform(
                load_from_path(path),
                Message::ConfigurationsLoaded,
            ));
        }

        (app, Task::batch(tasks))
    }

    /// 메시지를 처리하고 상태를 업데이트
    /// Elm Architecture의 Update 함수
    ///
    /// # Arguments
    /// * `message` - 처리할 메시지
    ///
    /// # Returns
    /// 다음에 실행할 Task (부수 효과가 필요한 경우)
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.dispatch_message(message);
        self.ensure_selected_env_bulk_input();
        task
    }

    fn dispatch_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchView(view_mode) => {
                self.current_view = view_mode;
                Task::none()
            }
            Message::AddConfiguration
            | Message::DeleteConfiguration(_)
            | Message::RunConfiguration(_)
            | Message::StartConfigurationDrag(_)
            | Message::ConfigurationDragHovered(_, _)
            | Message::ConfigurationDragExited(_)
            | Message::NameChanged(_)
            | Message::TypeChanged(_)
            | Message::CommandChanged(_)
            | Message::ArgumentsChanged(_)
            | Message::ExecuteModeChanged(_)
            | Message::ScriptPathChanged(_)
            | Message::BrowseScriptPath
            | Message::ScriptPathSelected(_)
            | Message::ScriptOptionsChanged(_)
            | Message::InterpreterPathChanged(_)
            | Message::BrowseInterpreterPath
            | Message::InterpreterPathSelected(_)
            | Message::InterpreterOptionsChanged(_)
            | Message::ScriptTextChanged(_)
            | Message::BrowseProjectDirectory
            | Message::ProjectDirectorySelected(_)
            | Message::PackageJsonsScanned(_, _, _)
            | Message::PackageJsonDropdownChanged(_)
            | Message::NodeRuntimeChanged(_)
            | Message::NodeRuntimesDetected(_)
            | Message::PackageManagerChanged(_)
            | Message::NodeCommandChanged(_)
            | Message::NodeScriptNameChanged(_)
            | Message::NodeArgumentsChanged(_)
            | Message::NodeOptionsChanged(_)
            | Message::NodeRefreshScripts
            | Message::WorkingDirectoryChanged(_)
            | Message::BrowseWorkingDirectory
            | Message::WorkingDirectorySelected(_)
            | Message::EnvBulkInputChanged(_)
            | Message::EnvBulkInputSubmitted
            | Message::OpenEnvModal
            | Message::ConfirmEnvModal
            | Message::CancelEnvModal
            | Message::EnvModalRowKeyChanged(_, _)
            | Message::EnvModalRowValueChanged(_, _)
            | Message::EnvModalDuplicateEntry(_)
            | Message::EnvModalRemoveEntry(_)
            | Message::EnvModalFocusNext
            | Message::EnvModalFocusPrev
            | Message::MoveEditorFocus(_)
            | Message::EditorFocusAreaChanged(_)
            | Message::ConfigurationsLoaded(_)
            | Message::OpenConfigurations
            | Message::ConfigurationsOpened(_)
            | Message::SaveConfigurations
            | Message::ConfigurationsSaved(_) => self.handle_configuration_messages(message),
            Message::ProcessStarted(_, _)
            | Message::OutputReceived(_, _)
            | Message::RunCompleted(_, _)
            | Message::RerunSession(_)
            | Message::StopSession(_)
            | Message::RemoveSession(_)
            | Message::OpenSessionInWorkspace(_)
            | Message::SessionListItemHovered(_)
            | Message::CopyToClipboard(_)
            | Message::OpenUrl(_)
            | Message::SessionScrollChanged(_, _)
            | Message::ToggleAutoScroll(_) => self.handle_session_messages(message),
            Message::AddWorkspaceTab
            | Message::CloseTab(_)
            | Message::TabNameClicked(_)
            | Message::TabNameInputChanged(_)
            | Message::FinishEditingTabName
            | Message::CancelEditingTabName
            | Message::CancelEditingTabNameOnOutsideClick
            | Message::PaneGridDragged(_)
            | Message::CursorMoved(_)
            | Message::ConfigurationPaneResized(_)
            | Message::PaneGridResized(_)
            | Message::ClosePane(_)
            | Message::TogglePaneMaximize(_)
            | Message::TabBarHovered(_)
            | Message::PaneHovered(_)
            | Message::PaneUnhovered(_)
            | Message::DropZoneHovered(_, _)
            | Message::DropZoneUnhovered
            | Message::OuterDropZoneHovered(_)
            | Message::OuterDropZoneUnhovered
            | Message::MouseReleased
            | Message::WindowOpened(_)
            | Message::WindowResized(_, _)
            | Message::WindowMaximized(_)
            | Message::StartWindowDrag
            | Message::ResizeWindow(_)
            | Message::MinimizeWindow
            | Message::ToggleWindowMaximize
            | Message::CloseWindow => self.handle_workspace_messages(message),
        }
    }

    fn handle_configuration_messages(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddConfiguration => self.handle_add_configuration(),
            Message::DeleteConfiguration(index_opt) => self.handle_delete_configuration(index_opt),
            Message::RunConfiguration(index_opt) => self.handle_run_configuration(index_opt),
            Message::StartConfigurationDrag(index) => self.handle_start_configuration_drag(index),
            Message::ConfigurationDragHovered(index, position) => {
                self.handle_configuration_drag_hovered(index, position)
            }
            Message::ConfigurationDragExited(index) => self.handle_configuration_drag_exited(index),
            Message::NameChanged(name) => self.handle_name_changed(name),
            Message::TypeChanged(config_type) => self.handle_type_changed(&config_type),
            Message::CommandChanged(command) => self.handle_command_changed(command),
            Message::ArgumentsChanged(arguments) => self.handle_arguments_changed(arguments),
            Message::ExecuteModeChanged(mode_type) => self.handle_execute_mode_changed(mode_type),
            Message::ScriptPathChanged(value) => self.handle_script_path_changed(value),
            Message::BrowseScriptPath => self.handle_browse_script_path(),
            Message::ScriptPathSelected(result) => self.handle_script_path_selected(result),
            Message::ScriptOptionsChanged(value) => self.handle_script_options_changed(value),
            Message::InterpreterPathChanged(value) => self.handle_interpreter_path_changed(value),
            Message::BrowseInterpreterPath => self.handle_browse_interpreter_path(),
            Message::InterpreterPathSelected(result) => {
                self.handle_interpreter_path_selected(result)
            }
            Message::InterpreterOptionsChanged(value) => {
                self.handle_interpreter_options_changed(value)
            }
            Message::ScriptTextChanged(value) => self.handle_script_text_changed(value),
            Message::BrowseProjectDirectory => self.handle_browse_project_directory(),
            Message::ProjectDirectorySelected(result) => {
                self.handle_project_directory_selected(result)
            }
            Message::PackageJsonsScanned(config_id, project_directory, relative_paths) => {
                self.handle_package_jsons_scanned(config_id, &project_directory, &relative_paths)
            }
            Message::PackageJsonDropdownChanged(relative_path) => {
                self.handle_package_json_dropdown_changed(&relative_path)
            }
            Message::NodeRuntimeChanged(label) => self.handle_node_runtime_changed(label),
            Message::NodeRuntimesDetected(runtimes) => self.handle_node_runtimes_detected(runtimes),
            Message::PackageManagerChanged(package_manager) => {
                self.handle_package_manager_changed(package_manager)
            }
            Message::NodeCommandChanged(cmd) => self.handle_node_command_changed(cmd),
            Message::NodeScriptNameChanged(name) => self.handle_node_script_name_changed(name),
            Message::NodeArgumentsChanged(value) => self.handle_node_arguments_changed(value),
            Message::NodeOptionsChanged(value) => self.handle_node_options_changed(value),
            Message::NodeRefreshScripts => self.handle_node_refresh_scripts(),
            Message::WorkingDirectoryChanged(dir) => self.handle_working_directory_changed(&dir),
            Message::BrowseWorkingDirectory => self.handle_browse_working_directory(),
            Message::WorkingDirectorySelected(result) => {
                self.handle_working_directory_selected(result)
            }
            Message::EnvBulkInputChanged(text) => self.handle_env_bulk_input_changed(text),
            Message::EnvBulkInputSubmitted => self.handle_env_bulk_input_submitted(),
            Message::OpenEnvModal => self.handle_open_env_modal(),
            Message::ConfirmEnvModal => self.handle_confirm_env_modal(),
            Message::CancelEnvModal => self.handle_cancel_env_modal(),
            Message::EnvModalRowKeyChanged(idx, key) => {
                self.handle_env_modal_row_key_changed(idx, key)
            }
            Message::EnvModalRowValueChanged(idx, value) => {
                self.handle_env_modal_row_value_changed(idx, value)
            }
            Message::EnvModalDuplicateEntry(idx) => self.handle_env_modal_duplicate_entry(idx),
            Message::EnvModalRemoveEntry(idx) => self.handle_env_modal_remove_entry(idx),
            Message::EnvModalFocusNext => self.handle_env_modal_focus_shift(false),
            Message::EnvModalFocusPrev => self.handle_env_modal_focus_shift(true),
            Message::MoveEditorFocus(backward) => Self::handle_move_editor_focus(backward),
            Message::EditorFocusAreaChanged(is_active) => {
                self.handle_editor_focus_area_changed(is_active)
            }
            Message::ConfigurationsLoaded(result) => self.handle_configurations_loaded(result),
            Message::OpenConfigurations => self.handle_open_configurations(),
            Message::ConfigurationsOpened(result) => self.handle_configurations_opened(result),
            Message::SaveConfigurations => self.handle_save_configurations(),
            Message::ConfigurationsSaved(result) => self.handle_configurations_saved(result),
            _ => unreachable!("non-configuration message routed to handle_configuration_messages"),
        }
    }

    fn handle_session_messages(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ProcessStarted(session_id, pid) => {
                self.handle_process_started(session_id, pid)
            }
            Message::OutputReceived(session_id, output) => {
                self.handle_output_received(session_id, &output)
            }
            Message::RunCompleted(session_id, result) => {
                self.handle_run_completed(session_id, result)
            }
            Message::RerunSession(session_index) => self.handle_rerun_session(session_index),
            Message::StopSession(session_index) => self.handle_stop_session(session_index),
            Message::RemoveSession(session_index) => self.handle_remove_session(session_index),
            Message::OpenSessionInWorkspace(session_index) => {
                self.handle_open_session_in_workspace(session_index)
            }
            Message::SessionListItemHovered(session_index) => {
                self.hovered_session_index = session_index;
                Task::none()
            }
            Message::CopyToClipboard(text) => iced::clipboard::write(text),
            Message::OpenUrl(url) => self.handle_open_url(&url),
            Message::SessionScrollChanged(session_id, progress) => {
                self.handle_session_scroll_changed(session_id, progress)
            }
            Message::ToggleAutoScroll(session_id) => self.handle_toggle_auto_scroll(session_id),
            _ => unreachable!("non-session message routed to handle_session_messages"),
        }
    }

    fn handle_workspace_messages(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddWorkspaceTab => self.handle_add_workspace_tab(),
            Message::CloseTab(tab_index) => self.handle_close_tab(tab_index),
            Message::TabNameClicked(tab_index) => self.handle_tab_name_clicked(tab_index),
            Message::TabNameInputChanged(new_value) => {
                self.handle_tab_name_input_changed(new_value)
            }
            Message::FinishEditingTabName => self.finish_editing_tab_name(),
            Message::CancelEditingTabName => self.cancel_editing_tab_name(),
            Message::CancelEditingTabNameOnOutsideClick => {
                self.cancel_editing_tab_name_on_outside_click()
            }
            Message::ConfigurationPaneResized(event) => {
                self.handle_configuration_pane_resized(event)
            }
            Message::PaneGridDragged(event) => self.handle_pane_grid_dragged(event),
            Message::CursorMoved(position) => self.handle_cursor_moved(position),
            Message::PaneGridResized(event) => self.handle_pane_grid_resized(event),
            Message::ClosePane(pane_id) => self.handle_close_pane(pane_id),
            Message::TogglePaneMaximize(pane_id) => self.handle_toggle_pane_maximize(pane_id),
            Message::TabBarHovered(tab_index) => self.handle_tab_bar_hovered(tab_index),
            Message::PaneHovered(pane_id) => self.handle_pane_hovered(pane_id),
            Message::PaneUnhovered(pane_id) => self.handle_pane_unhovered(pane_id),
            Message::DropZoneHovered(pane_id, zone) => self.handle_drop_zone_hovered(pane_id, zone),
            Message::DropZoneUnhovered => self.handle_drop_zone_unhovered(),
            Message::OuterDropZoneHovered(zone) => self.handle_outer_drop_zone_hovered(zone),
            Message::OuterDropZoneUnhovered => self.handle_outer_drop_zone_unhovered(),
            Message::MouseReleased => self.handle_mouse_released(),
            Message::WindowOpened(id) => self.handle_window_opened(id),
            Message::WindowResized(id, size) => self.handle_window_resized(id, size),
            Message::WindowMaximized(is_maximized) => self.handle_window_maximized(is_maximized),
            Message::StartWindowDrag => self.handle_start_window_drag(),
            Message::ResizeWindow(direction) => self.handle_resize_window(direction),
            Message::MinimizeWindow => self.handle_minimize_window(),
            Message::ToggleWindowMaximize => self.handle_toggle_window_maximize(),
            Message::CloseWindow => self.handle_close_window(),
            _ => unreachable!("non-workspace message routed to handle_workspace_messages"),
        }
    }

    fn handle_window_opened(&mut self, id: window::Id) -> Task<Message> {
        self.window_id = Some(id);
        window::is_maximized(id).map(Message::WindowMaximized)
    }

    fn handle_window_resized(&mut self, id: window::Id, size: Size) -> Task<Message> {
        if self.window_id == Some(id) {
            self.window_size = size;
            window::is_maximized(id).map(Message::WindowMaximized)
        } else {
            Task::none()
        }
    }

    fn handle_window_maximized(&mut self, is_maximized: bool) -> Task<Message> {
        self.is_window_maximized = is_maximized;
        Task::none()
    }

    fn handle_start_window_drag(&mut self) -> Task<Message> {
        self.window_id.map_or_else(Task::none, window::drag)
    }

    fn handle_resize_window(&mut self, direction: window::Direction) -> Task<Message> {
        if self.is_window_maximized {
            Task::none()
        } else {
            self.window_id
                .map_or_else(Task::none, |id| window::drag_resize(id, direction))
        }
    }

    fn handle_minimize_window(&mut self) -> Task<Message> {
        self.window_id
            .map_or_else(Task::none, |id| window::minimize(id, true))
    }

    fn handle_toggle_window_maximize(&mut self) -> Task<Message> {
        self.window_id.map_or_else(Task::none, |id| {
            self.is_window_maximized = !self.is_window_maximized;
            Task::batch([
                window::toggle_maximize(id),
                window::is_maximized(id).map(Message::WindowMaximized),
            ])
        })
    }

    fn handle_close_window(&mut self) -> Task<Message> {
        self.prepare_running_sessions_for_shutdown();
        self.window_id.map_or_else(Task::none, window::close)
    }

    fn prepare_running_sessions_for_shutdown(&mut self) {
        for session in &self.sessions {
            if session.is_running {
                session.cancel_flag.store(true, Ordering::Relaxed);
            }
        }
    }

    fn handle_configuration_pane_resized(
        &mut self,
        event: pane_grid::ResizeEvent,
    ) -> Task<Message> {
        self.configuration_layout.resize(event.split, event.ratio);
        self.save_app_settings();
        Task::none()
    }

    fn save_app_settings(&self) {
        save_settings(&AppSettings {
            last_file_path: self.last_file_path.clone(),
            configuration_split_ratio: Some(configuration_split_ratio(&self.configuration_layout)),
        });
    }

    fn handle_tab_name_clicked(&mut self, tab_index: usize) -> Task<Message> {
        if let Some(tab) = self.workspace_tabs.get(tab_index) {
            self.selected_tab_index = tab_index;

            let now = Instant::now();
            let is_double_click =
                self.tab_ui
                    .last_tab_name_click
                    .is_some_and(|(last_idx, last_time)| {
                        last_idx == tab_index && now.duration_since(last_time).as_millis() < 300
                    });

            if is_double_click {
                self.tab_ui.editing_tab_name = Some((tab_index, tab.name.clone()));
                self.tab_ui.last_tab_name_click = None;
            } else {
                self.tab_ui.last_tab_name_click = Some((tab_index, now));
            }
        }

        Task::none()
    }

    fn handle_tab_name_input_changed(&mut self, new_value: String) -> Task<Message> {
        if let Some((tab_index, _)) = &self.tab_ui.editing_tab_name {
            self.tab_ui.editing_tab_name = Some((*tab_index, new_value));
        }

        Task::none()
    }

    fn finish_editing_tab_name(&mut self) -> Task<Message> {
        if let Some((tab_index, new_name)) = self.tab_ui.editing_tab_name.take()
            && let Some(tab) = self.workspace_tabs.get_mut(tab_index)
        {
            tab.name = if new_name.trim().is_empty() {
                String::from("Workspace")
            } else {
                new_name
            };
        }

        Task::none()
    }

    fn cancel_editing_tab_name(&mut self) -> Task<Message> {
        self.tab_ui.editing_tab_name = None;
        Task::none()
    }

    fn cancel_editing_tab_name_on_outside_click(&mut self) -> Task<Message> {
        let Some((editing_tab_index, _)) = self.tab_ui.editing_tab_name.as_ref() else {
            return Task::none();
        };

        if self.drag.hovered_tab_index != Some(*editing_tab_index) {
            self.tab_ui.editing_tab_name = None;
        }

        Task::none()
    }

    fn handle_execute_mode_changed(&mut self, mode_type: ExecuteModeType) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript { execute_mode } = &mut config.type_data
        {
            *execute_mode = match mode_type {
                ExecuteModeType::ScriptFile => ExecuteMode::ScriptFile {
                    script_path: String::new(),
                    script_options: String::new(),
                    interpreter_path: None,
                    interpreter_options: None,
                },
                ExecuteModeType::ScriptText => ExecuteMode::ScriptText {
                    script_text: String::new(),
                },
            };
        }

        Task::none()
    }

    fn handle_name_changed(&mut self, name: String) -> Task<Message> {
        if let Some(index) = self.selected_config_index
            && let Some(config) = self.configurations.get_mut(index)
        {
            config.name = name;
        }

        Task::none()
    }

    fn select_configuration(&mut self, index: usize) {
        self.selected_config_index = Some(index);

        let load_data = self.configurations.get(index).and_then(|config| {
            if let ConfigTypeData::Node {
                project_directory, ..
            } = &config.type_data
            {
                if project_directory.is_empty() {
                    None
                } else {
                    Some((
                        config.id,
                        project_directory.clone(),
                        config.working_directory.clone(),
                    ))
                }
            } else {
                None
            }
        });

        if let Some((config_id, project_dir, working_dir)) = load_data {
            self.load_node_package_jsons(config_id, &project_dir);
            if !working_dir.is_empty() {
                self.load_node_scripts(config_id, &working_dir);
            }
        }

        self.sync_editor_select_state_for_selected_config();
    }

    fn handle_add_configuration(&mut self) -> Task<Message> {
        self.configurations.push(RunConfiguration::default());
        self.selected_config_index = Some(self.configurations.len() - 1);
        self.sync_editor_select_state_for_selected_config();
        self.status_message = String::from("New configuration added");
        Task::none()
    }

    fn handle_delete_configuration(&mut self, index_opt: Option<usize>) -> Task<Message> {
        let index = index_opt.or(self.selected_config_index);

        if let Some(idx) = index
            && idx < self.configurations.len()
        {
            let config_id = self.configurations[idx].id;
            self.configurations.remove(idx);
            self.env_bulk_inputs.remove(&config_id);
            if let Some(modal) = &self.env_modal
                && modal.config_id == config_id
            {
                self.env_modal = None;
            }

            self.selected_config_index = if self.configurations.is_empty() {
                None
            } else if idx >= self.configurations.len() {
                Some(self.configurations.len() - 1)
            } else {
                Some(idx)
            };
            self.sync_editor_select_state_for_selected_config();
            self.status_message = String::from("Configuration deleted");
        }

        Task::none()
    }

    fn handle_run_configuration(&mut self, index_opt: Option<usize>) -> Task<Message> {
        let index = index_opt.or(self.selected_config_index);

        if let Some(idx) = index
            && let Some(config) = self.configurations.get(idx)
        {
            self.selected_config_index = Some(idx);
            let config = config.clone();

            let session = RunSession::new(config.name.clone());
            let session_id = session.id;
            let cancel_flag = session.cancel_flag.clone();

            self.sessions.push(session);
            self.selected_session_index = Some(self.sessions.len() - 1);

            self.ensure_workspace_tab();
            if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
                tab.open_session(session_id);
            }

            self.status_message = format!("Running: {}", config.name);
            self.current_view = ViewMode::Sessions;

            return Task::run(
                run_configuration_stream(config, session_id, cancel_flag),
                |msg| msg,
            );
        }

        Task::none()
    }

    fn handle_start_configuration_drag(&mut self, index: usize) -> Task<Message> {
        if index < self.configurations.len() {
            self.select_configuration(index);
            self.drag.pending_configuration_index = Some(index);
            self.drag.pending_configuration_origin = None;
            self.drag.hovered_configuration_target = None;
        }

        Task::none()
    }

    fn handle_configuration_drag_hovered(
        &mut self,
        index: usize,
        position: ConfigurationDropPosition,
    ) -> Task<Message> {
        if self.drag.dragging_configuration_index.is_some() && index <= self.configurations.len() {
            self.drag.hovered_configuration_target = Some((index, position));
        }

        Task::none()
    }

    fn handle_configuration_drag_exited(&mut self, index: usize) -> Task<Message> {
        if self
            .drag
            .hovered_configuration_target
            .is_some_and(|(hovered_index, _)| hovered_index == index)
        {
            self.drag.hovered_configuration_target = None;
        }

        Task::none()
    }

    fn move_configuration_to(&mut self, from_index: usize, to_index: usize) -> usize {
        if from_index >= self.configurations.len()
            || to_index > self.configurations.len()
            || from_index == to_index
        {
            return from_index;
        }

        let configuration = self.configurations.remove(from_index);
        let target_index = if from_index < to_index {
            to_index.saturating_sub(1)
        } else {
            to_index
        }
        .min(self.configurations.len());

        self.configurations.insert(target_index, configuration);

        if let Some(selected_index) = self.selected_config_index {
            self.selected_config_index = if selected_index == from_index {
                Some(target_index)
            } else if from_index < selected_index && selected_index <= target_index {
                Some(selected_index - 1)
            } else if target_index <= selected_index && selected_index < from_index {
                Some(selected_index + 1)
            } else {
                Some(selected_index)
            };
        }

        target_index
    }

    fn configuration_drop_index(
        from_index: usize,
        hovered_index: usize,
        position: ConfigurationDropPosition,
    ) -> usize {
        match position {
            ConfigurationDropPosition::Before
                if hovered_index == from_index || hovered_index == from_index + 1 =>
            {
                from_index
            }
            ConfigurationDropPosition::After if hovered_index == from_index => from_index,
            ConfigurationDropPosition::Before => hovered_index,
            ConfigurationDropPosition::After => hovered_index.saturating_add(1),
        }
    }

    fn handle_type_changed(&mut self, config_type: &ConfigurationType) -> Task<Message> {
        if let Some(index) = self.selected_config_index {
            let (config_id, working_directory, is_node) = self.configurations.get(index).map_or(
                (Uuid::new_v4(), String::from("."), false),
                |config| {
                    (
                        config.id,
                        config.working_directory.clone(),
                        matches!(config_type, ConfigurationType::Node),
                    )
                },
            );

            if let Some(config) = self.configurations.get_mut(index) {
                config.config_type = config_type.clone();
                config.type_data = match config_type {
                    ConfigurationType::Application => ConfigTypeData::Application {
                        command: String::new(),
                        arguments: String::new(),
                    },
                    ConfigurationType::ShellScript => ConfigTypeData::ShellScript {
                        execute_mode: ExecuteMode::default(),
                    },
                    ConfigurationType::Node => ConfigTypeData::Node {
                        project_directory: String::from("."),
                        package_manager: PackageManager::default(),
                        node_runtime_path: None,
                        command: crate::models::NodeCommand::default(),
                        script_name: None,
                        arguments: String::new(),
                        node_options: String::new(),
                    },
                };
            }

            if is_node {
                self.load_node_scripts(config_id, &working_directory);
            }

            self.sync_editor_select_state_for_selected_config();
        }

        Task::none()
    }

    fn handle_command_changed(&mut self, command: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Application { command: cmd, .. } = &mut config.type_data
        {
            *cmd = command;
        }

        Task::none()
    }

    fn handle_arguments_changed(&mut self, arguments: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Application {
                arguments: args, ..
            } = &mut config.type_data
        {
            *args = arguments;
        }

        Task::none()
    }

    fn handle_script_path_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript {
                execute_mode: ExecuteMode::ScriptFile { script_path, .. },
            } = &mut config.type_data
        {
            *script_path = value;
        }

        Task::none()
    }

    fn handle_browse_script_path(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_script_file = true;

        Task::perform(
            async {
                match AsyncFileDialog::new()
                    .set_title("Select Script File")
                    .add_filter(
                        "Script Files",
                        &["sh", "bash", "py", "js", "rb", "ps1", "bat", "cmd"],
                    )
                    .pick_file()
                    .await
                {
                    Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
                    None => Err(String::from(DIALOG_CANCELLED)),
                }
            },
            Message::ScriptPathSelected,
        )
    }

    fn handle_script_path_selected(&mut self, result: Result<String, String>) -> Task<Message> {
        self.file_dialog.is_loading_script_file = false;

        match result {
            Ok(path) => {
                if let Some(config) = self.get_selected_config_mut()
                    && let ConfigTypeData::ShellScript {
                        execute_mode: ExecuteMode::ScriptFile { script_path, .. },
                    } = &mut config.type_data
                {
                    script_path.clone_from(&path);
                }
                self.status_message = format!("Script file selected: {path}");
            }
            Err(error) => {
                self.status_message = if error == DIALOG_CANCELLED {
                    String::from("Script file selection cancelled")
                } else {
                    format!("Script file selection failed: {error}")
                };
            }
        }

        Task::none()
    }

    fn handle_script_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript {
                execute_mode: ExecuteMode::ScriptFile { script_options, .. },
            } = &mut config.type_data
        {
            *script_options = value;
        }

        Task::none()
    }

    fn handle_interpreter_path_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript {
                execute_mode:
                    ExecuteMode::ScriptFile {
                        interpreter_path, ..
                    },
            } = &mut config.type_data
        {
            *interpreter_path = if value.is_empty() { None } else { Some(value) };
        }

        Task::none()
    }

    fn handle_browse_interpreter_path(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_interpreter = true;

        Task::perform(
            async {
                match AsyncFileDialog::new()
                    .set_title("Select Interpreter")
                    .pick_file()
                    .await
                {
                    Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
                    None => Err(String::from(DIALOG_CANCELLED)),
                }
            },
            Message::InterpreterPathSelected,
        )
    }

    fn handle_interpreter_path_selected(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        self.file_dialog.is_loading_interpreter = false;

        match result {
            Ok(path) => {
                if let Some(config) = self.get_selected_config_mut()
                    && let ConfigTypeData::ShellScript {
                        execute_mode:
                            ExecuteMode::ScriptFile {
                                interpreter_path, ..
                            },
                    } = &mut config.type_data
                {
                    *interpreter_path = Some(path.clone());
                }
                self.status_message = format!("Interpreter selected: {path}");
            }
            Err(error) => {
                self.status_message = if error == DIALOG_CANCELLED {
                    String::from("Interpreter selection cancelled")
                } else {
                    format!("Interpreter selection failed: {error}")
                };
            }
        }

        Task::none()
    }

    fn handle_interpreter_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript {
                execute_mode:
                    ExecuteMode::ScriptFile {
                        interpreter_options,
                        ..
                    },
            } = &mut config.type_data
        {
            *interpreter_options = if value.is_empty() { None } else { Some(value) };
        }

        Task::none()
    }

    fn handle_script_text_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::ShellScript {
                execute_mode: ExecuteMode::ScriptText { script_text },
            } = &mut config.type_data
        {
            *script_text = value;
        }

        Task::none()
    }

    fn handle_browse_working_directory(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_folder = true;
        self.status_message = String::from("Opening folder dialog...");

        Task::perform(
            async {
                match AsyncFileDialog::new()
                    .set_title("Select Working Directory")
                    .pick_folder()
                    .await
                {
                    Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
                    None => Err(String::from(DIALOG_CANCELLED)),
                }
            },
            Message::WorkingDirectorySelected,
        )
    }

    fn handle_working_directory_selected(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        self.file_dialog.is_loading_folder = false;

        match result {
            Ok(path) => {
                let (config_id, is_node) = self
                    .selected_config_index
                    .and_then(|index| self.configurations.get(index))
                    .map_or((Uuid::new_v4(), false), |config| {
                        (
                            config.id,
                            matches!(config.config_type, ConfigurationType::Node),
                        )
                    });

                if let Some(index) = self.selected_config_index
                    && let Some(config) = self.configurations.get_mut(index)
                {
                    config.working_directory.clone_from(&path);
                }

                if is_node {
                    self.load_node_scripts(config_id, &path);
                }

                self.status_message = format!("Working directory set: {path}");
            }
            Err(error) => {
                self.status_message = if error == DIALOG_CANCELLED {
                    String::from("Directory selection cancelled")
                } else {
                    format!("Directory selection failed: {error}")
                };
            }
        }

        Task::none()
    }

    fn handle_browse_project_directory(&mut self) -> Task<Message> {
        self.node_ui.is_loading_project_directory = true;
        self.status_message = String::from("Opening project directory selection...");

        Task::perform(
            async {
                match AsyncFileDialog::new()
                    .set_title("Select Project Directory")
                    .pick_folder()
                    .await
                {
                    Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
                    None => Err(String::from(DIALOG_CANCELLED)),
                }
            },
            Message::ProjectDirectorySelected,
        )
    }

    fn handle_project_directory_selected(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        self.node_ui.is_loading_project_directory = false;

        match result {
            Ok(dir_path) => {
                let config_id = self
                    .selected_config_index
                    .and_then(|index| self.configurations.get(index))
                    .map_or_else(Uuid::new_v4, |config| config.id);

                if let Some(index) = self.selected_config_index
                    && let Some(config) = self.configurations.get_mut(index)
                    && let ConfigTypeData::Node {
                        project_directory, ..
                    } = &mut config.type_data
                {
                    project_directory.clone_from(&dir_path);
                }

                self.node_available_package_jsons.insert(config_id, vec![]);
                self.sync_editor_select_state_for_selected_config();
                self.status_message = String::from("Scanning package.json files...");

                return Task::perform(
                    async move {
                        use crate::utils::{find_all_package_jsons, to_relative_path};
                        use std::path::Path;

                        let project_dir = Path::new(&dir_path);
                        let package_json_paths = find_all_package_jsons(project_dir);
                        let relative_paths = package_json_paths
                            .iter()
                            .map(|path| to_relative_path(path, project_dir))
                            .collect();

                        (config_id, dir_path.clone(), relative_paths)
                    },
                    |(config_id, project_dir, paths)| {
                        Message::PackageJsonsScanned(config_id, project_dir, paths)
                    },
                );
            }
            Err(error) => {
                self.status_message = format!("Directory selection cancelled: {error}");
            }
        }

        Task::none()
    }

    fn handle_package_jsons_scanned(
        &mut self,
        config_id: Uuid,
        project_directory: &str,
        relative_paths: &[String],
    ) -> Task<Message> {
        use std::path::Path;

        self.node_available_package_jsons
            .insert(config_id, relative_paths.to_vec());
        self.sync_editor_select_state_for_selected_config();

        if let Some(first_pkg) = relative_paths.first() {
            let project_dir = Path::new(project_directory);
            let full_path = project_dir.join(first_pkg.trim_start_matches("./"));

            if let Some(parent) = full_path.parent() {
                let working_dir = parent.to_string_lossy().to_string();

                if let Some(index) = self.selected_config_index
                    && let Some(config) = self.configurations.get_mut(index)
                {
                    config.working_directory.clone_from(&working_dir);
                }

                self.load_node_scripts(config_id, &working_dir);
            }

            self.status_message = format!(
                "Found {} package.json file(s) in: {project_directory}",
                relative_paths.len()
            );
        } else {
            self.status_message = format!("No package.json found in: {project_directory}");
        }

        Task::none()
    }

    fn handle_package_json_dropdown_changed(&mut self, relative_path: &str) -> Task<Message> {
        use std::path::Path;

        let (config_id, project_directory) = self
            .selected_config_index
            .and_then(|index| self.configurations.get(index))
            .and_then(|config| {
                if let ConfigTypeData::Node {
                    project_directory, ..
                } = &config.type_data
                {
                    Some((config.id, project_directory.clone()))
                } else {
                    None
                }
            })
            .map_or((Uuid::new_v4(), String::from(".")), |value| value);

        let project_dir = Path::new(&project_directory);
        let full_path = project_dir.join(relative_path.trim_start_matches("./"));

        if let Some(parent) = full_path.parent() {
            let working_dir = parent.to_string_lossy().to_string();

            if let Some(index) = self.selected_config_index
                && let Some(config) = self.configurations.get_mut(index)
            {
                config.working_directory.clone_from(&working_dir);
            }

            self.load_node_scripts(config_id, &working_dir);
            self.status_message = format!("Selected package.json: {relative_path}");
        } else {
            self.status_message = format!("Invalid package.json path: {relative_path}");
        }

        Task::none()
    }

    fn handle_node_runtime_changed(&mut self, label: String) -> Task<Message> {
        let actual_path = self
            .available_node_runtimes
            .iter()
            .find(|(runtime_label, _)| runtime_label == &label)
            .map(|(_, path)| path.clone())
            .unwrap_or(label);

        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node {
                node_runtime_path, ..
            } = &mut config.type_data
        {
            *node_runtime_path = if actual_path == "node" {
                None
            } else {
                Some(actual_path)
            };
        }

        Task::none()
    }

    fn handle_node_runtimes_detected(&mut self, runtimes: Vec<(String, String)>) -> Task<Message> {
        self.available_node_runtimes = runtimes;
        self.editor_select_state
            .set_node_runtimes(&self.available_node_runtimes);
        Task::none()
    }

    fn handle_package_manager_changed(&mut self, package_manager: PackageManager) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node {
                package_manager: selected_package_manager,
                ..
            } = &mut config.type_data
        {
            *selected_package_manager = package_manager;
        }

        Task::none()
    }

    fn handle_node_command_changed(&mut self, cmd: crate::models::NodeCommand) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node {
                command,
                script_name,
                ..
            } = &mut config.type_data
        {
            *command = cmd;
            if !cmd.requires_script() {
                *script_name = None;
            }
        }

        Task::none()
    }

    fn handle_node_script_name_changed(&mut self, name: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node { script_name, .. } = &mut config.type_data
        {
            *script_name = Some(name);
        }

        Task::none()
    }

    fn handle_node_arguments_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node { arguments, .. } = &mut config.type_data
        {
            *arguments = value;
        }

        Task::none()
    }

    fn handle_node_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(config) = self.get_selected_config_mut()
            && let ConfigTypeData::Node { node_options, .. } = &mut config.type_data
        {
            *node_options = value;
        }

        Task::none()
    }

    fn handle_node_refresh_scripts(&mut self) -> Task<Message> {
        let (config_id, working_directory) = self
            .get_selected_config()
            .map_or((Uuid::new_v4(), String::from(".")), |config| {
                (config.id, config.working_directory.clone())
            });

        self.load_node_scripts(config_id, &working_directory);
        Task::none()
    }

    fn handle_working_directory_changed(&mut self, dir: &str) -> Task<Message> {
        let (config_id, is_node) = self
            .selected_config_index
            .and_then(|index| self.configurations.get(index))
            .map_or((Uuid::new_v4(), false), |config| {
                (
                    config.id,
                    matches!(config.config_type, ConfigurationType::Node),
                )
            });

        if let Some(index) = self.selected_config_index
            && let Some(config) = self.configurations.get_mut(index)
        {
            config.working_directory = dir.to_string();
        }

        if is_node {
            self.load_node_scripts(config_id, dir);
        }

        Task::none()
    }

    fn handle_configurations_loaded(
        &mut self,
        result: Result<Vec<RunConfiguration>, String>,
    ) -> Task<Message> {
        match result {
            Ok(configs) => {
                self.configurations = configs;
                self.env_bulk_inputs.clear();
                self.env_modal = None;
                self.status_message = if let Some(path) = &self.last_file_path {
                    format!("Loaded: {}", path.display())
                } else {
                    String::from("Configurations loaded")
                };

                if !self.configurations.is_empty() {
                    self.selected_config_index = Some(0);
                    self.load_node_metadata_for_current_configurations();
                }
            }
            Err(error) => {
                self.status_message = format!("Load failed: {error}");
            }
        }

        Task::none()
    }

    fn handle_save_configurations(&mut self) -> Task<Message> {
        let configs = self.configurations.clone();
        let current_path = self.last_file_path.clone();
        self.status_message = String::from("Saving configurations...");
        Task::perform(
            save_configurations(configs, current_path),
            Message::ConfigurationsSaved,
        )
    }

    fn handle_configurations_saved(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        match result {
            Ok(path) => {
                self.last_file_path = Some(path.clone());
                self.save_app_settings();
                self.status_message = format!("Saved: {}", path.display());
            }
            Err(error) => {
                self.status_message = if error == DIALOG_CANCELLED {
                    String::from("Save canceled")
                } else {
                    format!("Save failed: {error}")
                };
            }
        }

        Task::none()
    }

    fn handle_open_configurations(&mut self) -> Task<Message> {
        self.status_message = String::from("Opening configurations...");
        Task::perform(open_configurations(), Message::ConfigurationsOpened)
    }

    fn handle_configurations_opened(
        &mut self,
        result: Result<(Vec<RunConfiguration>, PathBuf), String>,
    ) -> Task<Message> {
        match result {
            Ok((configs, path)) => {
                self.configurations = configs;
                self.env_bulk_inputs.clear();
                self.env_modal = None;
                self.selected_config_index = (!self.configurations.is_empty()).then_some(0);
                self.load_node_metadata_for_current_configurations();

                self.last_file_path = Some(path.clone());
                self.save_app_settings();
                self.status_message = format!("Opened: {}", path.display());
            }
            Err(error) => {
                self.status_message = if error == DIALOG_CANCELLED {
                    String::from("Open canceled")
                } else {
                    format!("Open failed: {error}")
                };
            }
        }

        Task::none()
    }

    fn load_node_metadata_for_current_configurations(&mut self) {
        for (config_id, project_dir, working_dir) in self.collect_node_config_paths() {
            self.load_node_package_jsons(config_id, &project_dir);
            if !working_dir.is_empty() {
                self.load_node_scripts(config_id, &working_dir);
            }
        }
    }

    fn collect_node_config_paths(&self) -> Vec<(Uuid, String, String)> {
        self.configurations
            .iter()
            .filter_map(|config| {
                if let ConfigTypeData::Node {
                    project_directory, ..
                } = &config.type_data
                {
                    if project_directory.is_empty() {
                        None
                    } else {
                        Some((
                            config.id,
                            project_directory.clone(),
                            config.working_directory.clone(),
                        ))
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    fn selected_config_id(&self) -> Option<Uuid> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get(idx))
            .map(|config| config.id)
    }

    fn handle_env_bulk_input_changed(&mut self, text: String) -> Task<Message> {
        if let Some(config_id) = self.selected_config_id() {
            self.env_bulk_inputs.insert(config_id, text);
        }
        Task::none()
    }

    fn handle_env_bulk_input_submitted(&mut self) -> Task<Message> {
        let Some(index) = self.selected_config_index else {
            return Task::none();
        };
        let Some(config) = self.configurations.get_mut(index) else {
            return Task::none();
        };
        let config_id = config.id;
        let raw = self
            .env_bulk_inputs
            .get(&config_id)
            .cloned()
            .unwrap_or_default();
        let entries = crate::env_string::parse_env_string(&raw);
        config.environment_variables = entries.into_iter().collect();
        // raw text는 정렬된 형태로 normalize하여 표시 일관성 유지
        let normalized = crate::env_string::serialize_env_map(&config.environment_variables);
        self.env_bulk_inputs.insert(config_id, normalized);
        Task::none()
    }

    fn handle_open_env_modal(&mut self) -> Task<Message> {
        let Some(index) = self.selected_config_index else {
            return Task::none();
        };
        let Some(config) = self.configurations.get(index) else {
            return Task::none();
        };
        let config_id = config.id;
        // 사용자가 메인 input에 타이핑 후 Enter 누르지 않은 raw text가 남아 있으면
        // 그 결과를 staging으로 사용 (데이터 손실 방지). 일치하면 environment_variables 그대로.
        let serialized = crate::env_string::serialize_env_map(&config.environment_variables);
        let raw = self
            .env_bulk_inputs
            .get(&config_id)
            .cloned()
            .unwrap_or_default();
        let mut entries: Vec<(String, String)> = if raw == serialized {
            config
                .environment_variables
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            crate::env_string::parse_env_string(&raw)
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        self.env_modal = Some(EnvModalState {
            config_id,
            entries,
            focused_cell: ModalCell {
                row: 0,
                kind: ModalCellKind::Key,
            },
        });
        // 모달 열림과 동시에 첫 row(idx=0)의 key input에 focus.
        // entries가 비었어도 placeholder row가 idx=0이라 동일하게 동작.
        iced::widget::operation::focus(env_modal_key_id(0))
    }

    fn handle_confirm_env_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.env_modal.take() else {
            return Task::none();
        };
        let Some(config) = self
            .configurations
            .iter_mut()
            .find(|c| c.id == modal.config_id)
        else {
            return Task::none();
        };
        // 빈 키 row 제거 + HashMap 변환으로 중복 키 자동 dedupe (마지막 값 우선)
        config.environment_variables = modal
            .entries
            .into_iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v))
            .collect();
        let normalized = crate::env_string::serialize_env_map(&config.environment_variables);
        self.env_bulk_inputs.insert(modal.config_id, normalized);
        Task::none()
    }

    fn handle_cancel_env_modal(&mut self) -> Task<Message> {
        self.env_modal = None;
        Task::none()
    }

    fn handle_env_modal_row_key_changed(&mut self, index: usize, key: String) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        Self::set_modal_row(&mut modal.entries, index, Some(key), None);
        Task::none()
    }

    fn handle_env_modal_row_value_changed(&mut self, index: usize, value: String) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        Self::set_modal_row(&mut modal.entries, index, None, Some(value));
        Task::none()
    }

    fn handle_env_modal_duplicate_entry(&mut self, index: usize) -> Task<Message> {
        if let Some(modal) = self.env_modal.as_mut()
            && let Some((key, value)) = modal.entries.get(index)
        {
            // 동일 (key, value)를 원본 바로 다음 위치에 insert.
            // 같은 키가 둘이 되어 자연스럽게 중복 경고가 떠 사용자에게 키 변경을 유도.
            let cloned = (key.clone(), value.clone());
            modal.entries.insert(index + 1, cloned);
        }
        Task::none()
    }

    fn handle_env_modal_remove_entry(&mut self, index: usize) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        if index >= modal.entries.len() {
            return Task::none();
        }
        modal.entries.remove(index);
        // remove 후 focused_cell이 row 수보다 커지지 않도록 clamp + 명시적 focus 이동.
        // (위젯 id 재할당으로 stale focus가 다른 데이터에 머무는 것 방지)
        let last_row = modal.entries.len(); // placeholder row index
        if modal.focused_cell.row > last_row {
            modal.focused_cell.row = last_row;
        }
        modal.focused_cell.kind = ModalCellKind::Key;
        Self::focus_modal_cell_task(modal.focused_cell)
    }

    /// Tab/Shift+Tab 처리: focused_cell을 modal 내부에서만 cycle.
    /// `(entries.len() + 1) * 2` 개 cell을 모듈로 순회한다 (+1은 placeholder row).
    /// `total_cells`는 항상 ≥ 2 이므로 0-검사는 불필요.
    fn handle_env_modal_focus_shift(&mut self, backward: bool) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        let total_cells = (modal.entries.len() + 1) * 2;
        let current = modal.focused_cell.linear_index().min(total_cells - 1);
        let next = if backward {
            (current + total_cells - 1) % total_cells
        } else {
            (current + 1) % total_cells
        };
        modal.focused_cell = ModalCell::from_linear(next);
        Self::focus_modal_cell_task(modal.focused_cell)
    }

    fn focus_modal_cell_task(cell: ModalCell) -> Task<Message> {
        let id = match cell.kind {
            ModalCellKind::Key => env_modal_key_id(cell.row),
            ModalCellKind::Value => env_modal_value_id(cell.row),
        };
        iced::widget::operation::focus(id)
    }

    /// row index 위치에 key/value를 부분 업데이트. index가 entries 범위 밖이면
    /// 빈 entry를 push하여 auto-grow.
    fn set_modal_row(
        entries: &mut Vec<(String, String)>,
        index: usize,
        key: Option<String>,
        value: Option<String>,
    ) {
        while entries.len() <= index {
            entries.push((String::new(), String::new()));
        }
        if let Some(k) = key {
            entries[index].0 = k;
        }
        if let Some(v) = value {
            entries[index].1 = v;
        }
    }

    fn handle_move_editor_focus(backward: bool) -> Task<Message> {
        // iced's built-in focus traversal covers text inputs reliably, but not
        // selector widgets like combo_box. Keep Tab navigation scoped to editor
        // input fields instead of pretending selectors are keyboard-focusable.
        if backward {
            iced::widget::operation::focus_previous()
        } else {
            iced::widget::operation::focus_next()
        }
    }

    fn handle_editor_focus_area_changed(&mut self, is_active: bool) -> Task<Message> {
        self.configuration_ui.editor_focus_area_active = is_active;
        Task::none()
    }

    fn handle_process_started(&mut self, session_id: Uuid, pid: u32) -> Task<Message> {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.process_pid = Some(pid);
            eprintln!("[Process] Started {} (PID: {})", session.config_name, pid);
        }

        Task::none()
    }

    fn handle_output_received(&mut self, session_id: Uuid, output: &str) -> Task<Message> {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            for line in output.lines() {
                session.add_output_line(line);
            }

            if session.auto_scroll {
                session.scroll_progress = 1.0;
            }
        }

        Task::none()
    }

    fn handle_run_completed(
        &mut self,
        session_id: Uuid,
        result: Result<i32, String>,
    ) -> Task<Message> {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.is_running = false;
            match result {
                Ok(code) => {
                    session.exit_code = Some(code);
                    session.add_output_line(&format!("\nProcess exited with code: {code}"));
                    self.status_message = format!("Completed (exit code: {code})");
                }
                Err(error) => {
                    session.add_output_line(&format!("\nError: {error}"));
                    self.status_message = format!("Failed: {error}");
                }
            }
        }

        Task::none()
    }

    fn handle_open_url(&mut self, url: &str) -> Task<Message> {
        let _ = open::that(url);
        self.status_message = format!("Opening URL: {url}");
        Task::none()
    }

    fn handle_session_scroll_changed(&mut self, session_id: Uuid, progress: f32) -> Task<Message> {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            let clamped_progress = progress.clamp(0.0, 1.0);
            session.scroll_progress = clamped_progress;
            session.auto_scroll = clamped_progress >= 0.99;
        }

        Task::none()
    }

    fn handle_toggle_auto_scroll(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.auto_scroll = !session.auto_scroll;
            if session.auto_scroll {
                session.scroll_progress = 1.0;
            }
        }

        Task::none()
    }

    fn handle_pane_grid_resized(&mut self, event: pane_grid::ResizeEvent) -> Task<Message> {
        if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
            tab.pane_layout.resize(event.split, event.ratio);
            tab.sync_layout_tree_from_pane_grid();
        }

        Task::none()
    }

    fn handle_pane_grid_dragged(&mut self, event: pane_grid::DragEvent) -> Task<Message> {
        use pane_grid::DragEvent;

        match event {
            DragEvent::Picked { pane } => {
                self.drag.is_dragging_pane = true;
                self.drag.dragging_pane_id = Some(pane);
                self.status_message = format!("Dragging pane: {pane:?}");
            }
            DragEvent::Dropped { pane, target } => {
                self.drag.is_dragging_pane = false;
                self.drag.dragging_pane_id = None;

                if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
                    tab.pane_layout.drop(pane, target);
                    tab.sync_layout_tree_from_pane_grid();
                }

                self.status_message = String::from("Pane dropped");
            }
            DragEvent::Canceled { .. } => {
                self.drag.is_dragging_pane = false;
                self.drag.dragging_pane_id = None;
                self.status_message = String::from("Drag canceled");
            }
        }

        Task::none()
    }

    fn estimated_tab_drop_width(tab_name: &str) -> f32 {
        let char_count = u16::try_from(tab_name.chars().count()).unwrap_or(u16::MAX);
        let name_width = f32::from(char_count) * 8.0;
        name_width.max(60.0) + 40.0
    }

    fn handle_cursor_moved(&mut self, position: iced::Point) -> Task<Message> {
        self.drag.cursor_position = Some(position);

        if self.drag.dragging_configuration_index.is_none()
            && let Some(index) = self.drag.pending_configuration_index
        {
            const CONFIGURATION_DRAG_THRESHOLD: f32 = 6.0;

            if let Some(origin) = self.drag.pending_configuration_origin {
                let delta_x = position.x - origin.x;
                let delta_y = position.y - origin.y;
                let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();

                if distance >= CONFIGURATION_DRAG_THRESHOLD {
                    self.drag.pending_configuration_index = None;
                    self.drag.pending_configuration_origin = None;
                    self.drag.dragging_configuration_index = Some(index);
                    self.drag.hovered_configuration_target = None;
                    self.status_message = String::from("Dragging configuration...");
                }
            } else {
                self.drag.pending_configuration_origin = Some(position);
            }
        }

        Task::none()
    }

    fn handle_tab_bar_hovered(&mut self, tab_index: Option<usize>) -> Task<Message> {
        eprintln!(
            "[DnD] TabBarHovered: {:?}, tab_dragging={:?}",
            tab_index,
            self.drag.tab_dragging.is_some()
        );
        self.drag.hovered_tab_index = tab_index;
        Task::none()
    }

    fn handle_pane_hovered(&mut self, pane_id: pane_grid::Pane) -> Task<Message> {
        eprintln!("[DnD] PaneHovered: pane={pane_id:?}");
        self.drag.hovered_pane = Some(pane_id);

        if self.drag.tab_dragging.is_some() {
            self.drag.last_hovered_pane = Some(pane_id);
        }

        Task::none()
    }

    fn handle_pane_unhovered(&mut self, pane_id: pane_grid::Pane) -> Task<Message> {
        eprintln!("[DnD] PaneUnhovered: pane={pane_id:?}");
        if self.drag.hovered_pane == Some(pane_id) {
            self.drag.hovered_pane = None;
        }

        Task::none()
    }

    fn handle_drop_zone_hovered(
        &mut self,
        pane_id: pane_grid::Pane,
        zone: DropZone,
    ) -> Task<Message> {
        eprintln!(
            "[DnD] DropZoneHovered: pane={pane_id:?}, zone={zone:?}, outer_active={:?}",
            self.drag.hovered_outer_zone
        );

        if self.drag.hovered_outer_zone.is_none() {
            self.drag.hovered_drop_zone = Some((pane_id, zone));
            eprintln!("[DnD]   -> 내부 드랍존 설정됨");
        } else {
            eprintln!("[DnD]   -> 외부 활성화 상태, 내부 무시됨");
        }

        Task::none()
    }

    fn handle_drop_zone_unhovered(&mut self) -> Task<Message> {
        eprintln!("[DnD] DropZoneUnhovered");
        self.drag.hovered_drop_zone = None;
        Task::none()
    }

    fn handle_outer_drop_zone_hovered(&mut self, zone: DropZone) -> Task<Message> {
        eprintln!("[DnD] OuterDropZoneHovered: zone={zone:?}");
        self.drag.hovered_outer_zone = Some(zone);
        self.drag.hovered_drop_zone = None;
        Task::none()
    }

    fn handle_outer_drop_zone_unhovered(&mut self) -> Task<Message> {
        eprintln!("[DnD] OuterDropZoneUnhovered");
        self.drag.hovered_outer_zone = None;
        Task::none()
    }

    fn handle_mouse_released(&mut self) -> Task<Message> {
        if let Some(from_index) = self.drag.dragging_configuration_index.take() {
            self.drag.pending_configuration_index = None;
            self.drag.pending_configuration_origin = None;
            let to_index = self
                .drag
                .hovered_configuration_target
                .take()
                .map_or(from_index, |(index, position)| {
                    Self::configuration_drop_index(from_index, index, position)
                });

            if from_index == to_index {
                self.status_message = String::from("Configuration drag canceled");
            } else {
                let final_index = self.move_configuration_to(from_index, to_index);
                self.selected_config_index = Some(final_index);
                self.status_message = String::from("Configuration reordered");
            }

            return Task::none();
        }

        if self.drag.pending_configuration_index.take().is_some() {
            self.drag.pending_configuration_origin = None;
            self.drag.hovered_configuration_target = None;
            return Task::none();
        }

        self.drag.hovered_drop_zone = None;
        self.drag.hovered_outer_zone = None;
        self.drag.last_hovered_pane = None;
        self.drag.hovered_tab_index = None;
        self.drag.tab_dragging = None;

        Task::none()
    }

    fn handle_rerun_session(&mut self, session_index: usize) -> Task<Message> {
        if let Some(session) = self.sessions.get_mut(session_index) {
            if session.is_running {
                session.cancel_flag.store(true, Ordering::Relaxed);
                self.status_message = String::from("Stopping session for restart...");
            }

            let config_name = session.config_name.clone();
            let old_id = session.id;

            if let Some(config) = self.configurations.iter().find(|c| c.name == config_name) {
                let new_id = Uuid::new_v4();
                session.id = new_id;
                session.clear_output();
                session.is_running = true;
                session.exit_code = None;
                session.started_at = SystemTime::now();
                session.cancel_flag =
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let cancel_flag = session.cancel_flag.clone();

                for tab in &mut self.workspace_tabs {
                    for pane in tab.pane_layout.panes.values_mut() {
                        if pane.session_id == Some(old_id) {
                            pane.session_id = Some(new_id);
                        }
                    }

                    let layout_ids: Vec<LayoutId> = tab
                        .layout_tree
                        .collect_leaves()
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect();

                    for layout_id in layout_ids {
                        if let Some(tree_pane) = tab.layout_tree.get_pane_mut(layout_id)
                            && tree_pane.session_id == Some(old_id)
                        {
                            tree_pane.session_id = Some(new_id);
                        }
                    }
                }

                self.selected_session_index = Some(session_index);
                self.status_message = format!("Rerunning: {}", config.name);

                let config = config.clone();

                return Task::run(
                    run_configuration_stream(config, new_id, cancel_flag),
                    |msg| msg,
                );
            }

            self.status_message = format!("Configuration '{config_name}' not found");
        }

        Task::none()
    }

    fn handle_stop_session(&mut self, session_index: usize) -> Task<Message> {
        if let Some(session) = self.sessions.get_mut(session_index)
            && session.is_running
        {
            session.cancel_flag.store(true, Ordering::Relaxed);
            self.status_message = String::from("Stopping session...");
        }

        Task::none()
    }

    fn handle_remove_session(&mut self, session_index: usize) -> Task<Message> {
        let Some(session) = self.sessions.get(session_index) else {
            return Task::none();
        };

        let session_id = session.id;
        if session.is_running {
            session.cancel_flag.store(true, Ordering::Relaxed);
        }

        self.sessions.remove(session_index);
        for tab in &mut self.workspace_tabs {
            tab.remove_session(session_id);
        }

        self.selected_session_index = match self.selected_session_index {
            Some(_) if self.sessions.is_empty() => None,
            Some(selected) if selected == session_index => {
                Some(session_index.min(self.sessions.len().saturating_sub(1)))
            }
            Some(selected) if selected > session_index => Some(selected - 1),
            other => other,
        };
        self.hovered_session_index = match self.hovered_session_index {
            Some(_) if self.sessions.is_empty() => None,
            Some(hovered) if hovered == session_index => None,
            Some(hovered) if hovered > session_index => Some(hovered - 1),
            other => other,
        };
        self.status_message = String::from("Session removed");

        Task::none()
    }

    fn handle_open_session_in_workspace(&mut self, session_index: usize) -> Task<Message> {
        let Some(session_id) = self.sessions.get(session_index).map(|session| session.id) else {
            return Task::none();
        };

        self.selected_session_index = Some(session_index);
        self.ensure_workspace_tab();

        let opened = self
            .workspace_tabs
            .get_mut(self.selected_tab_index)
            .is_some_and(|tab| tab.open_session(session_id));

        self.status_message = if opened {
            String::from("Session opened in workspace")
        } else {
            String::from("Session already open in workspace")
        };

        Task::none()
    }

    fn handle_add_workspace_tab(&mut self) -> Task<Message> {
        self.workspace_tabs
            .push(WorkspaceTab::empty(String::from("Workspace")));
        self.selected_tab_index = self.workspace_tabs.len() - 1;
        self.tab_ui.editing_tab_name = None;
        self.status_message = String::from("Workspace added");
        Task::none()
    }

    fn handle_close_tab(&mut self, tab_index: usize) -> Task<Message> {
        if tab_index >= self.workspace_tabs.len() {
            return Task::none();
        }

        if self.workspace_tabs.len() == 1 {
            if let Some(tab) = self.workspace_tabs.get_mut(0) {
                tab.clear();
            }
            self.selected_tab_index = 0;
            self.tab_ui.editing_tab_name = None;
            self.status_message = String::from("Workspace cleared");
            return Task::none();
        }

        self.workspace_tabs.remove(tab_index);

        if self.workspace_tabs.is_empty() {
            self.selected_tab_index = 0;
        } else if self.selected_tab_index >= self.workspace_tabs.len() {
            self.selected_tab_index = self.workspace_tabs.len() - 1;
        }

        self.status_message = String::from("Tab closed");
        Task::none()
    }

    fn handle_close_pane(&mut self, pane_id: pane_grid::Pane) -> Task<Message> {
        let session_id_to_hide = self
            .workspace_tabs
            .get(self.selected_tab_index)
            .and_then(|tab| tab.pane_layout.get(pane_id))
            .and_then(|pane| pane.session_id);

        if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
            if let Some(session_id) = session_id_to_hide {
                tab.remove_session(session_id);
                self.status_message = String::from("Session hidden from workspace");
            } else {
                self.status_message = String::from("No session in pane");
            }
        }

        Task::none()
    }

    fn handle_toggle_pane_maximize(&mut self, pane_id: pane_grid::Pane) -> Task<Message> {
        if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
            if tab.pane_layout.panes.len() <= 1 || tab.pane_layout.get(pane_id).is_none() {
                return Task::none();
            }

            if tab.pane_layout.maximized() == Some(pane_id) {
                tab.pane_layout.restore();
                self.status_message = String::from("Pane restored");
            } else {
                tab.pane_layout.maximize(pane_id);
                self.status_message = String::from("Pane maximized");
            }
        }

        Task::none()
    }

    fn ensure_workspace_tab(&mut self) {
        if self.workspace_tabs.is_empty() {
            self.workspace_tabs
                .push(WorkspaceTab::empty(String::from("Workspace")));
            self.selected_tab_index = 0;
        } else if self.selected_tab_index >= self.workspace_tabs.len() {
            self.selected_tab_index = self.workspace_tabs.len() - 1;
        }
    }

    /// 현재 선택된 구성의 가변 참조 가져오기
    fn get_selected_config_mut(&mut self) -> Option<&mut RunConfiguration> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get_mut(idx))
    }

    /// 현재 선택된 구성의 불변 참조 가져오기
    fn get_selected_config(&self) -> Option<&RunConfiguration> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get(idx))
    }

    /// Node `package.json` 목록 로드 (`project_directory`에서)
    fn load_node_package_jsons(&mut self, config_id: Uuid, project_directory: &str) {
        use crate::utils::{find_all_package_jsons, to_relative_path};
        use std::path::Path;

        let project_dir = Path::new(project_directory);
        let package_json_paths = find_all_package_jsons(project_dir);
        let relative_paths: Vec<String> = package_json_paths
            .iter()
            .map(|p| to_relative_path(p, project_dir))
            .collect();

        self.node_available_package_jsons
            .insert(config_id, relative_paths);
        self.sync_editor_select_state_for_selected_config();
    }

    /// Node package scripts 로드 (package.json에서)
    fn load_node_scripts(&mut self, config_id: Uuid, working_dir: &str) {
        use crate::utils::{find_package_json, parse_scripts};
        use std::path::Path;

        let path = Path::new(working_dir);
        if let Some(pkg_json) = find_package_json(path) {
            match parse_scripts(&pkg_json) {
                Ok(scripts) => {
                    self.node_available_scripts.insert(config_id, scripts);
                }
                Err(e) => {
                    eprintln!("Failed to parse scripts: {e}");
                    self.node_available_scripts.insert(config_id, vec![]);
                }
            }
        } else {
            self.node_available_scripts.insert(config_id, vec![]);
        }

        self.sync_editor_select_state_for_selected_config();
    }

    /// 탭 바 드롭 오버레이 렌더링 (드래그 중에만 표시)
    /// 각 탭 위치에 투명한 드롭 존을 배치
    fn view_tab_bar_drop_overlay(&self) -> Element<'_, Message> {
        let tab_count = self.workspace_tabs.len();
        let hovered_tab = self.drag.hovered_tab_index;
        let highlight_color = Color::from_rgba(0.3, 0.6, 1.0, 0.4); // 파란색 하이라이트
        let normal_color = Color::TRANSPARENT;

        // 탭 바 높이 (workspace_tabs.rs의 탭 높이와 맞춤)
        let tab_bar_height: f32 = 40.0;

        // 각 탭에 대한 드롭 존 생성
        let mut tab_zones = row![].spacing(2).padding(4).align_y(Alignment::Center);

        for tab_index in 0..tab_count {
            let is_hovered = hovered_tab == Some(tab_index);
            let is_current = tab_index == self.selected_tab_index;

            // 현재 탭이 아닌 경우에만 하이라이트 표시
            let bg_color = if is_hovered && !is_current {
                highlight_color
            } else {
                normal_color
            };

            // 각 탭의 드롭 존 (탭 이름 길이에 따라 동적, 대략적인 너비 사용)
            let tab_name = &self.workspace_tabs[tab_index].name;
            let estimated_width = Self::estimated_tab_drop_width(tab_name);

            let zone = mouse_area(
                container(Space::new())
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(bg_color)),
                        border: Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .width(Length::Fixed(estimated_width))
                    .height(Length::Fixed(28.0)),
            )
            .on_enter(Message::TabBarHovered(Some(tab_index)))
            .on_exit(Message::TabBarHovered(None));

            tab_zones = tab_zones.push(zone);
        }

        // 새 탭 버튼 영역 (드롭하면 새 탭 생성)
        // (추후 구현 가능)

        // 탭 바 영역 전체를 감싸는 컨테이너 (디버그: 반투명 배경)
        container(tab_zones)
            .width(Length::Fill)
            .height(Length::Fixed(tab_bar_height))
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(
                    1.0, 0.0, 0.0, 0.2,
                ))),
                ..Default::default()
            })
            .into()
    }

    /// 환경변수 메인 input 텍스트 ref. `ensure_selected_env_bulk_input`이 사전에 채워뒀다고 가정.
    fn env_bulk_input_text(&self) -> &str {
        self.selected_config_index
            .and_then(|idx| self.configurations.get(idx))
            .and_then(|config| self.env_bulk_inputs.get(&config.id))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// `selected` 구성의 `env_bulk_inputs` 항목이 비어있다면 직렬화된 값으로 채움.
    /// `update` 끝에서 호출하여 view에서 항상 self의 String을 빌릴 수 있도록 보장.
    fn ensure_selected_env_bulk_input(&mut self) {
        let Some(idx) = self.selected_config_index else {
            return;
        };
        let Some(config) = self.configurations.get(idx) else {
            return;
        };
        let id = config.id;
        self.env_bulk_inputs
            .entry(id)
            .or_insert_with(|| crate::env_string::serialize_env_map(&config.environment_variables));
    }

    fn sync_editor_select_state_for_selected_config(&mut self) {
        let (scripts, package_jsons) = self
            .selected_config_index
            .and_then(|idx| self.configurations.get(idx))
            .map_or_else(
                || (Vec::new(), Vec::new()),
                |config| {
                    (
                        self.node_available_scripts
                            .get(&config.id)
                            .cloned()
                            .unwrap_or_default(),
                        self.node_available_package_jsons
                            .get(&config.id)
                            .cloned()
                            .unwrap_or_default(),
                    )
                },
            );

        self.editor_select_state.set_node_scripts(scripts);
        self.editor_select_state.set_package_jsons(package_jsons);
    }

    fn view_status_bar(
        &self,
        padding: impl Into<iced::Padding>,
    ) -> iced::widget::Container<'_, Message> {
        container(
            column![
                container(Space::new())
                    .width(Length::Fill)
                    .height(1)
                    .style(status_bar_top_border_style),
                container(text(&self.status_message).size(12))
                    .padding(padding)
                    .width(Length::Fill),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .style(status_bar_style)
    }

    fn view_configuration_panel_shell<'a>(
        title: &'a str,
        header_actions: Option<Element<'a, Message>>,
        body: Element<'a, Message>,
    ) -> Element<'a, Message> {
        container(
            column![
                row![
                    container(
                        text(title)
                            .size(16)
                            .color(Color::from_rgba8(255, 255, 255, 0.88)),
                    )
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                    Space::new().width(Length::Fill),
                    header_actions.unwrap_or_else(|| Space::new().into()),
                ]
                .height(28)
                .align_y(Alignment::Center)
                .width(Length::Fill),
                Space::new().height(10),
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(Background::Color(Color::from_rgba8(255, 255, 255, 0.06))),
                        ..container::Style::default()
                    }),
                Space::new().height(12),
                container(body).width(Length::Fill).height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding([8, 0])
        .style(Self::configuration_panel_style)
        .into()
    }

    fn configuration_panel_style(_theme: &Theme) -> container::Style {
        container::Style {
            ..container::Style::default()
        }
    }

    fn view_window_title_bar(&self) -> Element<'_, Message> {
        let main_tabs = view_main_tabs(self.current_view, !self.sessions.is_empty());
        let controls = row![
            Self::view_window_control_button(
                "✕",
                Message::CloseWindow,
                Color::from_rgb8(255, 95, 86),
            ),
            Self::view_window_control_button(
                "—",
                Message::MinimizeWindow,
                Color::from_rgb8(255, 189, 46),
            ),
            Self::view_window_control_button(
                if self.is_window_maximized {
                    "❐"
                } else {
                    "✢"
                },
                Message::ToggleWindowMaximize,
                Color::from_rgb8(39, 201, 63),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(22.0));

        mouse_area(
            container(
                column![
                    container(
                        row![
                            controls,
                            Space::new().width(Length::Fill),
                            text("Run/Debug Configuration Manager")
                                .size(12)
                                .color(Color::from_rgba8(255, 255, 255, 0.72)),
                            Space::new().width(Length::Fill),
                            main_tabs,
                        ]
                        .align_y(Alignment::Center)
                        .height(Length::Fixed(33.0)),
                    )
                    .padding([0, 12])
                    .width(Length::Fill),
                    container(Space::new())
                        .width(Length::Fill)
                        .height(1)
                        .style(status_bar_top_border_style),
                ]
                .spacing(0),
            )
            .width(Length::Fill)
            .height(Length::Fixed(34.0))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.base.color,
                )),
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                    radius: border::Radius::default().top(window_radius()),
                },
                ..container::Style::default()
            }),
        )
        .on_press(Message::StartWindowDrag)
        .on_double_click(Message::ToggleWindowMaximize)
        .interaction(mouse::Interaction::Pointer)
        .into()
    }

    fn view_window_control_button(
        label: &'static str,
        message: Message,
        color: Color,
    ) -> iced::widget::Button<'static, Message> {
        button(
            container(
                text(label)
                    .size(8)
                    .line_height(1.0)
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
            )
            .width(12)
            .height(12)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .padding(0)
        .width(14)
        .height(14)
        .on_press(message)
        .style(move |theme: &Theme, status| {
            let background = match status {
                button::Status::Pressed => theme.extended_palette().background.strong.color,
                _ => color,
            };
            let text_color = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Color::from_rgba8(0, 0, 0, 0.55)
                }
                _ => Color::TRANSPARENT,
            };

            button::Style {
                background: Some(Background::Color(background)),
                text_color,
                border: Border {
                    radius: 999.0.into(),
                    width: 1.0,
                    color: Color::from_rgba8(0, 0, 0, 0.12),
                },
                ..button::Style::default()
            }
        })
    }

    fn view_window_resize_grips(&self) -> Element<'static, Message> {
        const GRIP: f32 = 5.0;

        if self.is_window_maximized {
            return container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let top_row = row![
            Self::view_window_resize_grip(
                window::Direction::NorthWest,
                Length::Fixed(GRIP),
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingDiagonallyDown,
            ),
            Self::view_window_resize_grip(
                window::Direction::North,
                Length::Fill,
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingVertically,
            ),
            Self::view_window_resize_grip(
                window::Direction::NorthEast,
                Length::Fixed(GRIP),
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingDiagonallyUp,
            ),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(GRIP));

        let middle_row = row![
            Self::view_window_resize_grip(
                window::Direction::West,
                Length::Fixed(GRIP),
                Length::Fill,
                mouse::Interaction::ResizingHorizontally,
            ),
            Space::new().width(Length::Fill).height(Length::Fill),
            Self::view_window_resize_grip(
                window::Direction::East,
                Length::Fixed(GRIP),
                Length::Fill,
                mouse::Interaction::ResizingHorizontally,
            ),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let bottom_row = row![
            Self::view_window_resize_grip(
                window::Direction::SouthWest,
                Length::Fixed(GRIP),
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingDiagonallyUp,
            ),
            Self::view_window_resize_grip(
                window::Direction::South,
                Length::Fill,
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingVertically,
            ),
            Self::view_window_resize_grip(
                window::Direction::SouthEast,
                Length::Fixed(GRIP),
                Length::Fixed(GRIP),
                mouse::Interaction::ResizingDiagonallyDown,
            ),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(GRIP));

        container(
            column![top_row, middle_row, bottom_row]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_window_resize_grip(
        direction: window::Direction,
        width: Length,
        height: Length,
        interaction: mouse::Interaction,
    ) -> Element<'static, Message> {
        mouse_area(container(Space::new()).width(width).height(height))
            .on_press(Message::ResizeWindow(direction))
            .interaction(interaction)
            .into()
    }

    fn view_missing_tab_screen() -> Element<'static, Message> {
        container(text("No workspace tab available."))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn view_empty_workspace_content() -> Element<'static, Message> {
        container(
            column![
                text("No sessions in this workspace").size(15),
                text("Select a session from the left list to open it here.").size(12),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(empty_workspace_content_style)
        .into()
    }

    fn view_session_list_panel(&self) -> Element<'_, Message> {
        let current_tab = self.workspace_tabs.get(self.selected_tab_index);
        let header = row![
            text("Sessions").size(13),
            Space::new().width(Length::Fill),
            text(self.sessions.len().to_string()).size(11),
        ]
        .align_y(Alignment::Center)
        .padding([0, 8]);

        let mut list = column![].spacing(4).padding([8, 6]);

        if self.sessions.is_empty() {
            list = list.push(
                container(text("Run a configuration to create a session.").size(12))
                    .width(Length::Fill)
                    .padding([8, 10])
                    .style(session_empty_state_style),
            );
        } else {
            for (index, session) in self.sessions.iter().enumerate() {
                let is_open_in_workspace =
                    current_tab.is_some_and(|tab| tab.contains_session(session.id));
                let is_hovered = self.hovered_session_index == Some(index);

                list = list.push(Self::view_session_list_item(
                    index,
                    session,
                    is_open_in_workspace,
                    is_hovered,
                ));
            }
        }

        let list = scrollable(list)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(4.0)
                    .spacing(2.0)
                    .scroller_width(4),
            ))
            .height(Length::Fill);

        container(column![header, list].spacing(6))
            .width(Length::Fixed(260.0))
            .height(Length::Fill)
            .padding([8, 6])
            .style(session_list_panel_style)
            .into()
    }

    fn view_session_list_item(
        index: usize,
        session: &RunSession,
        is_open_in_workspace: bool,
        is_hovered: bool,
    ) -> Element<'_, Message> {
        let title = text(&session.config_name)
            .size(12)
            .wrapping(text::Wrapping::None);

        let item_content = row![
            container(Space::new())
                .width(7)
                .height(7)
                .style(move |theme: &Theme| session_list_status_dot_style(theme, session)),
            container(title)
                .width(Length::Fill)
                .center_y(Length::Shrink),
            row![
                Self::view_session_action_button(
                    "Stop session",
                    ICON_STOP,
                    session.is_running.then_some(Message::StopSession(index)),
                    SessionActionKind::Stop,
                    session.is_running,
                ),
                Self::view_session_action_button(
                    "Remove session",
                    ICON_DELETE,
                    Some(Message::RemoveSession(index)),
                    SessionActionKind::Remove,
                    true,
                ),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let item = container(item_content)
            .width(Length::Fill)
            .padding([8, 8])
            .style(move |theme: &Theme| {
                session_list_item_container_style(theme, is_open_in_workspace, is_hovered)
            });

        mouse_area(item)
            .interaction(mouse::Interaction::Pointer)
            .on_enter(Message::SessionListItemHovered(Some(index)))
            .on_exit(Message::SessionListItemHovered(None))
            .on_press(Message::OpenSessionInWorkspace(index))
            .into()
    }

    fn view_session_action_button(
        label: &'static str,
        icon: &'static [u8],
        on_press: Option<Message>,
        kind: SessionActionKind,
        is_active: bool,
    ) -> Element<'static, Message> {
        let icon = container(
            svg(svg::Handle::from_memory(icon))
                .width(12)
                .height(12)
                .style(move |theme: &Theme, _status| {
                    session_action_icon_style(theme, kind, is_active)
                }),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill);

        let button = button(icon)
            .width(24)
            .height(24)
            .padding(0)
            .style(move |theme: &Theme, status| {
                session_action_button_style(theme, status, kind, is_active)
            })
            .on_press_maybe(on_press);

        tooltip(button, icon_tooltip(label), tooltip::Position::Top).into()
    }

    fn view_configuration_screen<'a>(&'a self, env_bulk_text: &'a str) -> Element<'a, Message> {
        let split_ratio = configuration_split_ratio(&self.configuration_layout);
        let name_max_width = configuration_name_max_width(self.window_size, split_ratio);

        let configuration_split: Element<'a, Message> = pane_grid::PaneGrid::new(
            &self.configuration_layout,
            move |_pane_id, pane, _is_maximized| {
                let (title, body): (&str, Element<'a, Message>) = match pane {
                    ConfigurationScreenPane::Configurations => (
                        "Configurations",
                        mouse_area(
                            column![view_configuration_list(
                                &self.configurations,
                                self.selected_config_index,
                                name_max_width,
                                self.drag.dragging_configuration_index,
                                self.drag.hovered_configuration_target,
                            ),]
                            .width(Length::Fill)
                            .height(Length::Fill),
                        )
                        .on_enter(Message::EditorFocusAreaChanged(false))
                        .into(),
                    ),
                    ConfigurationScreenPane::Editor => (
                        "Editor",
                        mouse_area(
                            scrollable(
                                container(view_configuration_editor(
                                    &self.configurations,
                                    self.selected_config_index,
                                    env_bulk_text,
                                    EditorLoadingState {
                                        file_dialog: FileDialogLoadingState {
                                            folder: self.file_dialog.is_loading_folder,
                                            script_file: self.file_dialog.is_loading_script_file,
                                            interpreter: self.file_dialog.is_loading_interpreter,
                                        },
                                        node: NodeLoadingState {
                                            scripts: self.node_ui.is_loading_node_scripts,
                                            project_directory: self
                                                .node_ui
                                                .is_loading_project_directory,
                                        },
                                    },
                                    &self.editor_select_state,
                                    &self.available_node_runtimes,
                                ))
                                .width(Length::Fill)
                                .height(Length::Shrink),
                            )
                            .direction(scrollable::Direction::Vertical(
                                scrollable::Scrollbar::default()
                                    .width(4)
                                    .scroller_width(4.0)
                                    .spacing(2.0),
                            ))
                            .width(Length::Fill)
                            .height(Length::Fill),
                        )
                        .on_enter(Message::EditorFocusAreaChanged(true))
                        .on_exit(Message::EditorFocusAreaChanged(false))
                        .into(),
                    ),
                };

                let header_actions = match pane {
                    ConfigurationScreenPane::Configurations => Some(view_toolbar()),
                    ConfigurationScreenPane::Editor => None,
                };

                pane_grid::Content::new(Self::view_configuration_panel_shell(
                    title,
                    header_actions,
                    body,
                ))
                .style(move |_theme: &Theme| container::Style::default())
            },
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(12)
        .min_size(260)
        .on_resize(10, Message::ConfigurationPaneResized)
        .into();

        configuration_split
    }

    fn view_sessions_screen(&self) -> Element<'_, Message> {
        let Some(current_tab) = self.workspace_tabs.get(self.selected_tab_index) else {
            return Self::view_missing_tab_screen();
        };

        let workspace_tab_bar = view_workspace_tab_bar(
            &self.workspace_tabs,
            self.selected_tab_index,
            self.drag.hovered_tab_index,
            self.tab_ui.editing_tab_name.as_ref(),
        );

        let workspace_body: Element<'_, Message> = if current_tab.session_count() == 0 {
            Self::view_empty_workspace_content()
        } else {
            view_pane_layout(
                &current_tab.pane_layout,
                &self.sessions,
                current_tab.pane_layout.maximized(),
                self.drag.is_dragging_pane,
                self.drag.dragging_pane_id,
                self.drag.tab_dragging,
                self.drag.hovered_pane,
                self.drag.hovered_drop_zone,
                self.drag.hovered_outer_zone,
                self.workspace_tabs.len(),
            )
        };

        let tab_bar_island = container(workspace_tab_bar)
            .width(Length::Fill)
            .padding([6, 8])
            .style(workspace_tab_bar_island_style);

        let content_island = container(
            container(workspace_body)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(workspace_content_surface_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(0)
        .style(workspace_content_island_style);

        let tab_pane_area: Element<'_, Message> = column![
            tab_bar_island,
            content_island.width(Length::Fill).height(Length::Fill),
        ]
        .spacing(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

        let session_content: Element<'_, Message> = if self.drag.tab_dragging.is_some() {
            let tab_bar_overlay = self.view_tab_bar_drop_overlay();

            stack![tab_pane_area, tab_bar_overlay]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            tab_pane_area
        };

        let content = row![self.view_session_list_panel(), session_content]
            .spacing(10)
            .height(Length::Fill);

        content.into()
    }

    /// 마우스 이벤트 구독 (드래그 앤 드롭용)
    pub fn subscription(&self) -> Subscription<Message> {
        let cursor_subscription = if self.drag.is_dragging_pane
            || self.drag.pending_configuration_index.is_some()
            || self.drag.dragging_configuration_index.is_some()
        {
            event::listen_with(|event, _status, _id| match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorMoved(position))
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let release_subscription = if self.drag.tab_dragging.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::MouseReleased)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let configuration_release_subscription = if self.drag.pending_configuration_index.is_some()
            || self.drag.dragging_configuration_index.is_some()
        {
            event::listen_with(|event, _status, _id| match event {
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    Some(Message::MouseReleased)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // There is no stable app-level API here to ask whether the focused
        // widget belongs to the editor pane. We use the editor pane hover area
        // as a practical guard so Tab does not affect the whole screen.
        let editor_focus_subscription = if matches!(self.current_view, ViewMode::Configuration)
            && self.configuration_ui.editor_focus_area_active
            && self.env_modal.is_none()
        {
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if matches!(status, event::Status::Ignored) => {
                    Some(Message::MoveEditorFocus(modifiers.shift()))
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 모달이 열려 있을 때만 키보드 처리: Tab/Shift+Tab → 모달 내부 cycle, ESC → Cancel.
        // editor_focus_subscription는 모달 열림 시 비활성이라 Tab 메시지가 중복 발행되지 않는다.
        // 모달 trap: iced 글로벌 focus_next 대신 우리가 직접 cell index를 cycle해 외부로 빠지지 않게.
        let env_modal_keyboard_subscription = if self.env_modal.is_some() {
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if matches!(status, event::Status::Ignored) => {
                    if modifiers.shift() {
                        Some(Message::EnvModalFocusPrev)
                    } else {
                        Some(Message::EnvModalFocusNext)
                    }
                }
                // ESC: text_input이 unfocus capture해도 우리는 modal 닫기를 항상 발화.
                // status 가드 의도적으로 없음 — focus된 input 안에서도 ESC가 동작해야 한다.
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelEnvModal),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        let tab_name_edit_subscription = if self.tab_ui.editing_tab_name.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    Some(Message::CancelEditingTabNameOnOutsideClick)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        Subscription::batch([
            cursor_subscription,
            release_subscription,
            configuration_release_subscription,
            editor_focus_subscription,
            env_modal_keyboard_subscription,
            tab_name_edit_subscription,
            window::open_events().map(Message::WindowOpened),
            window::resize_events().map(|(id, size)| Message::WindowResized(id, size)),
        ])
    }

    /// 현재 상태를 UI로 렌더링
    /// Elm Architecture의 View 함수
    ///
    /// # Returns
    /// 렌더링할 Element 트리
    pub fn view(&self) -> Element<'_, Message> {
        let env_bulk_text = self.env_bulk_input_text();
        let content = match self.current_view {
            ViewMode::Configuration => self.view_configuration_screen(env_bulk_text),
            ViewMode::Sessions => self.view_sessions_screen(),
        };

        let chrome = column![
            self.view_window_title_bar(),
            container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(10),
            self.view_status_bar([4, 10]),
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

        let chrome = container(chrome)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(window_chrome_style);

        let mut layers = stack![
            chrome,
            self.view_window_resize_grips(),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(window_border_overlay_style),
        ];

        if let Some(modal) = self.env_modal.as_ref() {
            let props = EnvModalView {
                entries: &modal.entries,
            };
            layers = layers.push(view_env_modal(props));
        }

        layers.width(Length::Fill).height(Length::Fill).into()
    }
}

fn window_chrome_style(theme: &Theme) -> container::Style {
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

fn window_border_overlay_style(theme: &Theme) -> container::Style {
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

fn window_radius() -> f32 {
    if cfg!(target_os = "windows") {
        8.0
    } else {
        10.0
    }
}

fn status_bar_style(theme: &Theme) -> container::Style {
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

fn status_bar_top_border_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome_border_color(theme))),
        ..container::Style::default()
    }
}

fn session_list_panel_style(theme: &Theme) -> container::Style {
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

fn workspace_tab_bar_island_style(theme: &Theme) -> container::Style {
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

fn workspace_content_island_style(theme: &Theme) -> container::Style {
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

fn workspace_content_surface_style(theme: &Theme) -> container::Style {
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

fn empty_workspace_content_style(theme: &Theme) -> container::Style {
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

fn session_empty_state_style(theme: &Theme) -> container::Style {
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

fn session_list_item_container_style(
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

fn session_action_button_style(
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

fn session_action_icon_style(
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

fn session_list_status_dot_style(theme: &Theme, session: &RunSession) -> container::Style {
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

/// `RunConfigManager`의 `Drop` 구현 - 앱 종료 시 자동으로 모든 프로세스 정리
/// Tokio runtime이 종료되어도 OS 레벨에서 직접 프로세스를 kill
impl Drop for RunConfigManager {
    fn drop(&mut self) {
        eprintln!("[Shutdown] Cleaning up {} sessions...", self.sessions.len());
        self.prepare_running_sessions_for_shutdown();

        let running_sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|s| s.is_running && s.process_pid.is_some())
            .collect();

        if running_sessions.is_empty() {
            eprintln!("[Shutdown] No running processes to clean up.");
            return;
        }

        eprintln!(
            "[Shutdown] Terminating {} active processes...",
            running_sessions.len()
        );

        // Step 1: 먼저 SIGTERM으로 graceful shutdown 시도
        for session in &running_sessions {
            if let Some(pid) = session.process_pid {
                eprintln!(
                    "[Shutdown] Sending SIGTERM to PID {} ({})",
                    pid, session.config_name
                );

                #[cfg(unix)]
                unsafe {
                    // Unix: 프로세스 그룹 전체에 SIGTERM
                    libc::killpg(pid as i32, libc::SIGTERM);
                }

                #[cfg(windows)]
                {
                    // Windows: taskkill로 graceful 종료 시도 (without /F)
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T"])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                }
            }
        }

        // Step 2: 2초 대기 (graceful shutdown 기회)
        eprintln!("[Shutdown] Waiting 2 seconds for graceful termination...");
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Step 3: 아직 살아있는 프로세스는 SIGKILL로 강제 종료
        eprintln!("[Shutdown] Force killing remaining processes...");
        for session in &running_sessions {
            if let Some(pid) = session.process_pid {
                #[cfg(unix)]
                unsafe {
                    // Unix: SIGKILL로 강제 종료
                    libc::killpg(pid as i32, libc::SIGKILL);
                    eprintln!(
                        "[Shutdown] Sent SIGKILL to PID {} ({})",
                        pid, session.config_name
                    );
                }

                #[cfg(windows)]
                {
                    // Windows: /F 플래그로 강제 종료
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    let _ = std::process::Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output();
                    eprintln!(
                        "[Shutdown] Force killed PID {} ({})",
                        pid, session.config_name
                    );
                }
            }
        }

        eprintln!("[Shutdown] Cleanup completed.");
    }
}
