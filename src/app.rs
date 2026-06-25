use crate::messages::{ConfigurationDropPosition, Message, ViewMode};
use crate::models::{
    ConfigTypeData, ConfigurationType, ExecuteMode, ExecuteModeType, LayoutId, PackageManager,
    RunConfiguration, RunSession, SearchState, SessionStatusKind, WorkspaceTab,
};
use crate::services::{
    AppSettings, UpdateOutcome, check_latest_release, export_text, load_from_path, load_settings,
    open_configurations, register_running_pid, run_configuration_stream, save_configurations,
    save_settings, unregister_running_pid,
};
use crate::utils::{DIALOG_CANCELLED, ICON_DELETE, ICON_PLAY, ICON_REFRESH, ICON_STOP};
use crate::views::shared::icon_tooltip;
use crate::views::{
    EditorLoadingState, EditorSelectState, EnvModalView, FileDialogLoadingState, NodeLoadingState,
    view_configuration_editor, view_configuration_list, view_env_modal, view_main_tabs,
    view_pane_layout, view_toolbar, view_workspace_tab_bar,
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
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

mod chrome;
mod env_modal;

use env_modal::EnvModalState;

use chrome::{
    SessionActionKind, empty_workspace_content_style, session_action_button_style,
    session_action_icon_style, session_empty_state_style, session_list_item_container_style,
    session_list_panel_style, session_list_status_dot_style, status_bar_style,
    status_bar_top_border_style, status_bar_version_style, window_border_overlay_style,
    window_chrome_style, window_radius, workspace_content_island_style,
    workspace_content_surface_style, workspace_tab_bar_island_style,
};

/// 세션 리스트의 상태 배지 고정 폭 — 레이아웃 계산과 실제 렌더링이 공유한다.
const SESSION_STATUS_BADGE_WIDTH: f32 = 52.0;

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
    /// 탭 바 hover 추적 (탭 이름 편집 외부클릭 판정에 사용)
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

#[derive(Debug, Clone, Copy)]
enum ConfigurationScreenPane {
    Configurations,
    Editor,
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
    // 이름 텍스트 폰트 크기 (themed_name_text와 일치해야 함)
    const NAME_FONT_SIZE: f32 = 14.0;

    // 리스트 아이템에서 이름 텍스트를 제외한 고정 요소들의 실제 너비 합.
    // 값은 widgets::configuration_list의 실제 위젯 레이아웃과 일치한다.
    const ITEM_AREA_PADDING: f32 = 8.0; // item container padding [2, 4] 좌우
    const CONTENT_ROW_SPACING: f32 = 16.0; // content_row spacing(4) × 갭 4개
    const SELECTION_BAR_WIDTH: f32 = 2.0; // selection_bar 고정 폭
    const ACTION_BUTTONS_WIDTH: f32 = 72.0; // action button 24px × 3개 (Run/Clone/Delete)
    const SUMMARY_HORIZONTAL_PADDING: f32 = 16.0; // summary container padding [2, 8] 좌우
    const TYPE_BADGE_WIDTH: f32 = 44.0; // 타입 배지 최대 폭 (보수적)
    const BADGE_NAME_GAP: f32 = 8.0; // 배지-이름 사이 Space
    const SCROLLBAR_WIDTH: f32 = 4.0; // 세로 스크롤바 폭

    let list_reserved_width = ITEM_AREA_PADDING
        + CONTENT_ROW_SPACING
        + SELECTION_BAR_WIDTH
        + ACTION_BUTTONS_WIDTH
        + SUMMARY_HORIZONTAL_PADDING
        + TYPE_BADGE_WIDTH
        + BADGE_NAME_GAP
        + SCROLLBAR_WIDTH;

    // 이름 텍스트는 D2Coding 모노스페이스로 렌더되므로 측정 폭 = 실제 렌더 폭.
    // 가변폭 평균 추정과 달리 우측 공백 없이 정확히 잘린다.
    let char_width = crate::utils::monospace_char_width(NAME_FONT_SIZE);
    let content_width = (window_size.width - ROOT_HORIZONTAL_PADDING).max(600.0);
    let left_pane_width = ((content_width - PANE_SPACING) * split_ratio).max(260.0);
    let title_width = (left_pane_width - list_reserved_width).max(72.0);

    // title_width(>=72)와 char_width(>=1)가 모두 양수이므로 결과는 항상 양수.
    // 넓은 화면에서는 긴 이름을 더 많이 표시하도록 상한을 두지 않는다.
    (title_width / char_width).floor() as usize
}

/// 세션 리스트 항목에서 이름이 차지할 수 있는 최대 너비(half-width 컬럼 수).
///
/// 세션 패널은 고정 폭(260px)이므로 윈도우 크기와 무관하다.
/// 값은 view_session_list_panel / view_session_list_item의 실제 위젯 레이아웃과 일치한다.
fn session_name_max_width() -> usize {
    const PANEL_WIDTH: f32 = 260.0;
    // 이름 텍스트 폰트 크기 (view_session_list_item과 일치해야 함)
    const NAME_FONT_SIZE: f32 = 12.0;

    const PANEL_PADDING: f32 = 12.0; // panel container padding [8, 6] 좌우
    const LIST_PADDING: f32 = 12.0; // list column padding [8, 6] 좌우
    const ITEM_PADDING: f32 = 16.0; // item container padding [8, 8] 좌우
    const CONTENT_ROW_SPACING: f32 = 24.0; // item_content row spacing(8) × 갭 3개 (점|이름|배지|버튼)
    const STATUS_DOT_WIDTH: f32 = 7.0; // 실행 상태 점
    const ACTION_BUTTONS_WIDTH: f32 = 51.0; // action button 24px × 2개 + spacing(3)
    const SCROLLBAR_WIDTH: f32 = 6.0; // 세로 스크롤바 width 4 + spacing 2

    let reserved = PANEL_PADDING
        + LIST_PADDING
        + ITEM_PADDING
        + CONTENT_ROW_SPACING
        + STATUS_DOT_WIDTH
        + SESSION_STATUS_BADGE_WIDTH
        + ACTION_BUTTONS_WIDTH
        + SCROLLBAR_WIDTH;

    let char_width = crate::utils::monospace_char_width(NAME_FONT_SIZE);
    let title_width = (PANEL_WIDTH - reserved).max(48.0);

    (title_width / char_width).floor() as usize
}

/// 복제 구성의 유니크한 이름 생성. `{base} (copy)` → `{base} (copy 2)` → ... 순으로
/// `existing_names`와 충돌하지 않는 첫 이름을 반환한다.
fn unique_clone_name(base_name: &str, existing_names: &[&str]) -> String {
    let mut candidate = format!("{base_name} (copy)");
    let mut suffix = 2;
    while existing_names.iter().any(|name| *name == candidate) {
        candidate = format!("{base_name} (copy {suffix})");
        suffix += 1;
    }
    candidate
}

/// 네이티브 파일/폴더 선택 다이얼로그를 여는 Task 생성 (4개 browse 핸들러 공통).
/// 취소 시 `DIALOG_CANCELLED` 센티넬을 Err로 반환한다.
fn pick_path_task(
    title: &'static str,
    filter: Option<(&'static str, &'static [&'static str])>,
    folder: bool,
    on_done: fn(Result<String, String>) -> Message,
) -> Task<Message> {
    Task::perform(
        async move {
            let mut dialog = AsyncFileDialog::new().set_title(title);
            if let Some((name, extensions)) = filter {
                dialog = dialog.add_filter(name, extensions);
            }
            let handle = if folder {
                dialog.pick_folder().await
            } else {
                dialog.pick_file().await
            };
            match handle {
                Some(handle) => Ok(handle.path().to_string_lossy().to_string()),
                None => Err(String::from(DIALOG_CANCELLED)),
            }
        },
        on_done,
    )
}

/// 다이얼로그 결과 에러를 일관된 상태 메시지로 변환. 취소는 `{noun} cancelled`,
/// 그 외는 `{noun} failed: {error}`.
fn cancellable_status(error: &str, noun: &str) -> String {
    if error == DIALOG_CANCELLED {
        format!("{noun} cancelled")
    } else {
        format!("{noun} failed: {error}")
    }
}

/// 백그라운드(비포커스) 상태에서 실행이 끝났을 때 OS 데스크톱 알림을 표시.
/// 정상 종료/비정상 종료/스폰 실패를 모두 구분해 알리며, UI 스레드를 막지
/// 않도록 별도 스레드에서 발행하고 발행 실패는 조용히 무시한다.
fn notify_run_finished(name: &str, result: Result<i32, &str>) {
    let (summary, body) = match result {
        Ok(0) => (
            String::from("Run finished"),
            format!("{name} completed successfully"),
        ),
        Ok(code) => (
            String::from("Run failed"),
            format!("{name} exited with code {code}"),
        ),
        Err(error) => (
            String::from("Run failed"),
            format!("{name} failed: {error}"),
        ),
    };

    std::thread::spawn(move || {
        // app_id를 지정하지 않으면 Windows에서 PowerShell AppUserModelID로
        // 폴백한다. 커스텀 AUMID는 Start Menu 등록이 선행되어야 토스트가 뜨므로,
        // 미패키징 앱에서는 폴백을 그대로 사용하는 편이 안전하다.
        let _ = notify_rust::Notification::new()
            .summary(&summary)
            .body(&body)
            .show();
    });
}

/// 새 버전 발견 시 OS 데스크톱 알림을 표시한다. 토스트 클릭으로 링크를 여는
/// 동작은 플랫폼별 편차가 커 신뢰할 수 없으므로, 알림은 안내용으로만 쓰고
/// 실제 다운로드 진입점은 상태바의 클릭 가능한 링크가 영속적으로 제공한다.
/// UI 스레드를 막지 않도록 별도 스레드에서 발행하고 실패는 조용히 무시한다.
fn notify_update_available(latest: &str, url: &str) {
    let summary = String::from("Update available");
    let body = format!("Version {latest} is available.\n{url}");

    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .summary(&summary)
            .body(&body)
            .show();
    });
}

/// Node 구성에서 (`config_id`, `project_directory`, `working_directory`)를 추출.
/// Node 타입이 아니거나 `project_directory`가 비어 있으면 `None`.
fn node_paths(config: &RunConfiguration) -> Option<(Uuid, String, String)> {
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
}

/// Node 메타데이터(package.json 목록 + scripts)를 파일시스템에서 스캔한다.
/// 재귀 디렉터리 워크가 무거울 수 있어 `spawn_blocking`으로 전용 블로킹 스레드에서
/// 실행하므로, iced/tokio 워커도 UI 스레드도 막지 않는다.
async fn scan_node_metadata(
    config_id: Uuid,
    project_dir: String,
    working_dir: String,
) -> (Uuid, Vec<String>, Vec<String>) {
    use crate::utils::{
        find_all_package_jsons, find_package_json, parse_scripts, to_relative_path,
    };
    use std::path::Path;

    tokio::task::spawn_blocking(move || {
        let project_path = Path::new(&project_dir);
        let package_jsons: Vec<String> = find_all_package_jsons(project_path)
            .iter()
            .map(|p| to_relative_path(p, project_path))
            .collect();
        let scripts = if working_dir.is_empty() {
            Vec::new()
        } else {
            find_package_json(Path::new(&working_dir))
                .and_then(|pkg| parse_scripts(&pkg).ok())
                .unwrap_or_default()
        };
        (config_id, package_jsons, scripts)
    })
    .await
    .unwrap_or_else(|err| {
        // 블로킹 스캔 태스크 패닉/취소 — 조용히 빈 결과로 폴백하되 진단 가능하게 로그.
        eprintln!("[Node] metadata scan task failed: {err}");
        (config_id, Vec::new(), Vec::new())
    })
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
    /// 세션 리스트에서 hover 중인 항목 (표시용 인덱스)
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
    /// 윈도우 포커스 여부 (백그라운드에서 실행이 끝났을 때만 알림)
    is_window_focused: bool,
    /// 마지막으로 사용한 구성 파일 경로 (Open/Save)
    last_file_path: Option<PathBuf>,
    /// 사용 가능한 새 버전 `(latest, release_url)`. 없으면 `None`.
    /// 상태바의 업데이트 링크 표시에 사용한다.
    update_available: Option<(String, String)>,
    /// 업데이트 확인이 진행 중인지. 상태바에 로딩 스피너를 표시하고
    /// 스피너 타이머 subscription을 활성화하는 데 쓴다.
    is_checking_update: bool,
    /// 로딩 스피너 프레임 인덱스 (확인 중 타이머 tick마다 증가).
    update_spinner_frame: usize,
}

/// 상태바 업데이트 확인 로딩 스피너 프레임. D2Coding(모노스페이스)에서 항상
/// 렌더되도록 ASCII만 사용한다.
const UPDATE_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

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
            hovered_session_index: None,
            workspace_tabs: vec![WorkspaceTab::empty(String::from("Workspace 1"))],
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
            is_window_focused: true,
            last_file_path: last_file_path.clone(),
            update_available: None,
            // 아래에서 시작 시 자동 체크 Task를 큐잉하므로 스피너를 켠 상태로 시작.
            is_checking_update: true,
            update_spinner_frame: 0,
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

        // 3. 최신 버전 확인 (백그라운드, 실패는 비치명적)
        tasks.push(Task::perform(
            check_latest_release(),
            Message::UpdateCheckCompleted,
        ));

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
            | Message::CloneConfiguration(_)
            | Message::RunConfiguration(_)
            | Message::StartConfigurationDrag(_)
            | Message::ConfigurationDragHovered(_, _)
            | Message::ConfigurationDragExited(_)
            | Message::NameChanged(_)
            | Message::TypeChanged(_)
            | Message::CompoundMemberAdded(_)
            | Message::CompoundMemberRemoved(_)
            | Message::CompoundWorkspaceChanged(_)
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
            | Message::NodeMetadataLoaded(_, _, _)
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
            | Message::StopAllSessions
            | Message::RerunAllSessions
            | Message::RerunFailedSessions
            | Message::CopyToClipboard(_)
            | Message::OpenUrl(_)
            | Message::SessionScrollChanged(_, _, _)
            | Message::ToggleAutoScroll(_)
            | Message::OpenSessionSearch(_)
            | Message::CloseSessionSearch(_)
            | Message::SessionSearchChanged(_, _)
            | Message::SessionSearchNext(_)
            | Message::SessionSearchPrev(_)
            | Message::ToggleSessionSearchFilter(_)
            | Message::ToggleSessionSearchRegex(_)
            | Message::OpenSearchInActivePane
            | Message::CloseActiveSearch
            | Message::SearchNextInActivePane
            | Message::SearchPrevInActivePane
            | Message::ExportSessionOutput(_)
            | Message::SessionOutputExported(_)
            | Message::CheckForUpdates
            | Message::UpdateCheckCompleted(_)
            | Message::UpdateSpinnerTick => self.handle_session_messages(message),
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
            | Message::MouseReleased
            | Message::WindowOpened(_)
            | Message::WindowResized(_, _)
            | Message::WindowMaximized(_)
            | Message::WindowFocusChanged(_)
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
            Message::CloneConfiguration(index_opt) => self.handle_clone_configuration(index_opt),
            Message::RunConfiguration(index_opt) => self.handle_run_configuration(index_opt),
            Message::StartConfigurationDrag(index) => self.handle_start_configuration_drag(index),
            Message::ConfigurationDragHovered(index, position) => {
                self.handle_configuration_drag_hovered(index, position)
            }
            Message::ConfigurationDragExited(index) => self.handle_configuration_drag_exited(index),
            Message::NameChanged(name) => self.handle_name_changed(name),
            Message::TypeChanged(config_type) => self.handle_type_changed(&config_type),
            Message::CompoundMemberAdded(member_id) => self.handle_compound_member_added(member_id),
            Message::CompoundMemberRemoved(member_id) => {
                self.handle_compound_member_removed(member_id)
            }
            Message::CompoundWorkspaceChanged(value) => {
                self.handle_compound_workspace_changed(value)
            }
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
            Message::NodeMetadataLoaded(config_id, package_jsons, scripts) => {
                self.handle_node_metadata_loaded(config_id, package_jsons, scripts)
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
            Message::RerunSession(session_id) => self.handle_rerun_session(session_id),
            Message::StopSession(session_id) => self.handle_stop_session(session_id),
            Message::RemoveSession(session_id) => self.handle_remove_session(session_id),
            Message::OpenSessionInWorkspace(session_id) => {
                self.handle_open_session_in_workspace(session_id)
            }
            Message::SessionListItemHovered(session_index) => {
                self.hovered_session_index = session_index;
                Task::none()
            }
            Message::StopAllSessions => self.handle_stop_all_sessions(),
            Message::RerunAllSessions => self.handle_rerun_all_sessions(),
            Message::RerunFailedSessions => self.handle_rerun_failed_sessions(),
            Message::CopyToClipboard(text) => iced::clipboard::write(text),
            Message::OpenUrl(url) => self.handle_open_url(&url),
            Message::CheckForUpdates => self.handle_check_for_updates(),
            Message::UpdateCheckCompleted(result) => self.handle_update_check_completed(result),
            Message::UpdateSpinnerTick => self.handle_update_spinner_tick(),
            Message::SessionScrollChanged(session_id, progress, at_bottom) => {
                self.handle_session_scroll_changed(session_id, progress, at_bottom)
            }
            Message::ToggleAutoScroll(session_id) => self.handle_toggle_auto_scroll(session_id),
            Message::OpenSessionSearch(session_id) => self.handle_open_session_search(session_id),
            Message::CloseSessionSearch(session_id) => self.handle_close_session_search(session_id),
            Message::SessionSearchChanged(session_id, query) => {
                self.handle_session_search_changed(session_id, query)
            }
            Message::SessionSearchNext(session_id) => {
                self.handle_session_search_step(session_id, 1)
            }
            Message::SessionSearchPrev(session_id) => {
                self.handle_session_search_step(session_id, -1)
            }
            Message::ToggleSessionSearchFilter(session_id) => {
                self.handle_toggle_session_search_filter(session_id)
            }
            Message::ToggleSessionSearchRegex(session_id) => {
                self.handle_toggle_session_search_regex(session_id)
            }
            Message::OpenSearchInActivePane => match self.focused_search_session_id() {
                Some(session_id) => self.handle_open_session_search(session_id),
                None => Task::none(),
            },
            Message::CloseActiveSearch => match self.focused_search_session_id() {
                Some(session_id) => self.handle_close_session_search(session_id),
                None => Task::none(),
            },
            Message::SearchNextInActivePane => match self.focused_search_session_id() {
                Some(session_id) => self.handle_session_search_step(session_id, 1),
                None => Task::none(),
            },
            Message::SearchPrevInActivePane => match self.focused_search_session_id() {
                Some(session_id) => self.handle_session_search_step(session_id, -1),
                None => Task::none(),
            },
            Message::ExportSessionOutput(session_id) => {
                self.handle_export_session_output(session_id)
            }
            Message::SessionOutputExported(result) => self.handle_session_output_exported(result),
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
            Message::MouseReleased => self.handle_mouse_released(),
            Message::WindowOpened(id) => self.handle_window_opened(id),
            Message::WindowResized(id, size) => self.handle_window_resized(id, size),
            Message::WindowMaximized(is_maximized) => self.handle_window_maximized(is_maximized),
            Message::WindowFocusChanged(focused) => {
                self.is_window_focused = focused;
                Task::none()
            }
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
        if let Some((tab_index, new_name)) = self.tab_ui.editing_tab_name.take() {
            // 빈 이름은 충돌하지 않는 번호 이름으로 대체 (자기 탭은 제외해 원래 번호 재사용)
            let resolved = if new_name.trim().is_empty() {
                self.next_workspace_name(Some(tab_index))
            } else {
                new_name
            };
            if let Some(tab) = self.workspace_tabs.get_mut(tab_index) {
                tab.name = resolved;
            }
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

        if let Some((config_id, project_dir, working_dir)) =
            self.configurations.get(index).and_then(node_paths)
        {
            self.load_node_metadata(config_id, &project_dir, &working_dir);
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
            // 구성별 Node 캐시도 함께 제거 (UUID는 재사용되지 않으므로 누수 방지).
            self.node_available_scripts.remove(&config_id);
            self.node_available_package_jsons.remove(&config_id);
            // 삭제된 구성을 참조하던 Compound 멤버에서도 제거 (댕글링 참조 방지).
            for other in &mut self.configurations {
                if let Some(members) = other.type_data.compound_members_mut() {
                    members.retain(|id| *id != config_id);
                }
            }
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

    fn handle_clone_configuration(&mut self, index_opt: Option<usize>) -> Task<Message> {
        let index = index_opt.or(self.selected_config_index);

        if let Some(idx) = index
            && idx < self.configurations.len()
        {
            let source_id = self.configurations[idx].id;
            let base_name = self.configurations[idx].name.clone();

            let mut clone = self.configurations[idx].clone();
            clone.id = Uuid::new_v4();

            // 동일 이름 중복을 피해 유니크한 이름 생성 (무한 " (copy)" 누적 방지)
            let existing_names: Vec<&str> = self
                .configurations
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            clone.name = unique_clone_name(&base_name, &existing_names);

            // 원본에 스캔되어 있던 Node 스크립트/package.json 캐시를 그대로 전파
            let new_id = clone.id;
            if let Some(scripts) = self.node_available_scripts.get(&source_id).cloned() {
                self.node_available_scripts.insert(new_id, scripts);
            }
            if let Some(pkgs) = self.node_available_package_jsons.get(&source_id).cloned() {
                self.node_available_package_jsons.insert(new_id, pkgs);
            }

            let new_idx = idx + 1;
            self.configurations.insert(new_idx, clone);
            self.selected_config_index = Some(new_idx);
            self.sync_editor_select_state_for_selected_config();
            self.status_message = String::from("Configuration cloned");
        }

        Task::none()
    }

    fn handle_run_configuration(&mut self, index_opt: Option<usize>) -> Task<Message> {
        let index = index_opt.or(self.selected_config_index);

        if let Some(idx) = index
            && let Some(config) = self.configurations.get(idx)
        {
            self.selected_config_index = Some(idx);

            // Compound 구성: 멤버들을 각자의 세션/페인으로 동시 실행
            if let ConfigTypeData::Compound { members, workspace } = &config.type_data {
                let compound_name = config.name.clone();
                let members = members.clone();
                let workspace = workspace.clone();
                return self.run_compound(&compound_name, &members, workspace.as_deref());
            }

            let config = config.clone();

            let session = RunSession::new(config.name.clone());
            let session_id = session.id;
            let cancel_flag = session.cancel_flag.clone();

            self.sessions.push(session);

            self.ensure_workspace_tab();
            let aspect = self.workspace_content_aspect();
            if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
                tab.open_session(session_id, aspect);
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

    /// Compound 구성 실행: 각 멤버를 자신의 세션/페인으로 펼쳐 동시 실행한다.
    /// 누락된(삭제된) 멤버와 중첩 Compound 멤버는 건너뛴다.
    fn run_compound(
        &mut self,
        compound_name: &str,
        members: &[Uuid],
        workspace: Option<&str>,
    ) -> Task<Message> {
        // 먼저 실행 가능한 멤버를 수집한다(삭제/중첩 멤버는 스킵). 탭 생성·전환은 실행할
        // 멤버가 있을 때만 하여, 멤버 0개 compound가 빈 탭을 남기지 않게 한다(None 경로의
        // ensure_workspace_tab과 동일한 "빈 탭 안 만듦" 동작).
        let mut runnable = Vec::new();
        let mut skipped = 0usize;
        for &member_id in members {
            let Some(member) = self.configurations.iter().find(|c| c.id == member_id) else {
                skipped += 1; // 삭제된 멤버
                continue;
            };
            if matches!(member.type_data, ConfigTypeData::Compound { .. }) {
                skipped += 1; // 중첩 방지
                continue;
            }
            runnable.push(member.clone());
        }

        if runnable.is_empty() {
            self.status_message = format!("Compound '{compound_name}' has no runnable members");
            return Task::none();
        }

        // 실행할 멤버가 있으니 대상 탭을 정한다. workspace 이름이 지정되면 그 이름으로 새 탭을
        // 만들어(동명 충돌 시 넘버링) 전환하고, 비어 있으면 현재 활성 탭에서 실행한다.
        match workspace.map(str::trim).filter(|s| !s.is_empty()) {
            Some(base) => {
                let name = self.unique_workspace_name(base);
                self.workspace_tabs.push(WorkspaceTab::empty(name));
                self.selected_tab_index = self.workspace_tabs.len() - 1;
            }
            None => self.ensure_workspace_tab(),
        }
        self.current_view = ViewMode::Sessions;

        let aspect = self.workspace_content_aspect();
        let mut tasks = Vec::new();
        for config in runnable {
            let session = RunSession::new(config.name.clone());
            let session_id = session.id;
            let cancel_flag = session.cancel_flag.clone();

            self.sessions.push(session);
            if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
                tab.open_session(session_id, aspect);
            }

            tasks.push(Task::run(
                run_configuration_stream(config, session_id, cancel_flag),
                |msg| msg,
            ));
        }

        let launched = tasks.len();
        self.status_message = if skipped > 0 {
            format!("Running compound '{compound_name}': {launched} task(s), {skipped} skipped")
        } else {
            format!("Running compound '{compound_name}': {launched} task(s)")
        };

        Task::batch(tasks)
    }

    fn handle_compound_member_added(&mut self, member_id: Uuid) -> Task<Message> {
        // 자기 자신은 멤버로 추가 불가 (무한 중첩 방지)
        if self.selected_config_id() == Some(member_id) {
            return Task::none();
        }
        if let Some(members) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::compound_members_mut)
            && !members.contains(&member_id)
        {
            members.push(member_id);
        }
        Task::none()
    }

    fn handle_compound_member_removed(&mut self, member_id: Uuid) -> Task<Message> {
        if let Some(members) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::compound_members_mut)
        {
            members.retain(|id| *id != member_id);
        }
        Task::none()
    }

    fn handle_compound_workspace_changed(&mut self, value: String) -> Task<Message> {
        if let Some(workspace) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::compound_workspace_mut)
        {
            // 빈/공백 입력은 None으로 정규화 — "현재 탭에서 실행"과 같은 의미.
            *workspace = Some(value).filter(|v| !v.trim().is_empty());
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
                    ConfigurationType::Compound => ConfigTypeData::Compound {
                        members: Vec::new(),
                        workspace: None,
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
        if let Some(app) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::application_mut)
        {
            *app.command = command;
        }

        Task::none()
    }

    fn handle_arguments_changed(&mut self, arguments: String) -> Task<Message> {
        if let Some(app) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::application_mut)
        {
            *app.arguments = arguments;
        }

        Task::none()
    }

    fn handle_script_path_changed(&mut self, value: String) -> Task<Message> {
        if let Some(sf) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::script_file_mut)
        {
            *sf.script_path = value;
        }

        Task::none()
    }

    fn handle_browse_script_path(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_script_file = true;
        pick_path_task(
            "Select Script File",
            Some((
                "Script Files",
                &["sh", "bash", "py", "js", "rb", "ps1", "bat", "cmd"],
            )),
            false,
            Message::ScriptPathSelected,
        )
    }

    fn handle_script_path_selected(&mut self, result: Result<String, String>) -> Task<Message> {
        self.file_dialog.is_loading_script_file = false;

        match result {
            Ok(path) => {
                if let Some(sf) = self
                    .selected_type_data_mut()
                    .and_then(ConfigTypeData::script_file_mut)
                {
                    sf.script_path.clone_from(&path);
                }
                self.status_message = format!("Script file selected: {path}");
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Script file selection");
            }
        }

        Task::none()
    }

    fn handle_script_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(sf) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::script_file_mut)
        {
            *sf.script_options = value;
        }

        Task::none()
    }

    fn handle_interpreter_path_changed(&mut self, value: String) -> Task<Message> {
        if let Some(sf) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::script_file_mut)
        {
            *sf.interpreter_path = if value.is_empty() { None } else { Some(value) };
        }

        Task::none()
    }

    fn handle_browse_interpreter_path(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_interpreter = true;
        pick_path_task(
            "Select Interpreter",
            None,
            false,
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
                if let Some(sf) = self
                    .selected_type_data_mut()
                    .and_then(ConfigTypeData::script_file_mut)
                {
                    *sf.interpreter_path = Some(path.clone());
                }
                self.status_message = format!("Interpreter selected: {path}");
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Interpreter selection");
            }
        }

        Task::none()
    }

    fn handle_interpreter_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(sf) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::script_file_mut)
        {
            *sf.interpreter_options = if value.is_empty() { None } else { Some(value) };
        }

        Task::none()
    }

    fn handle_script_text_changed(&mut self, value: String) -> Task<Message> {
        if let Some(script_text) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::script_text_mut)
        {
            *script_text = value;
        }

        Task::none()
    }

    fn handle_browse_working_directory(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_folder = true;
        self.status_message = String::from("Opening folder dialog...");
        pick_path_task(
            "Select Working Directory",
            None,
            true,
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
                            matches!(config.config_type(), ConfigurationType::Node),
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
                self.status_message = cancellable_status(&error, "Directory selection");
            }
        }

        Task::none()
    }

    fn handle_browse_project_directory(&mut self) -> Task<Message> {
        self.node_ui.is_loading_project_directory = true;
        self.status_message = String::from("Opening project directory selection...");
        pick_path_task(
            "Select Project Directory",
            None,
            true,
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
                self.status_message = cancellable_status(&error, "Project directory selection");
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

        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            *node.node_runtime_path = if actual_path == "node" {
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
        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            *node.package_manager = package_manager;
        }

        Task::none()
    }

    fn handle_node_command_changed(&mut self, cmd: crate::models::NodeCommand) -> Task<Message> {
        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            *node.command = cmd;
            if !cmd.requires_script() {
                *node.script_name = None;
            }
        }

        Task::none()
    }

    fn handle_node_script_name_changed(&mut self, name: String) -> Task<Message> {
        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            // 빈 문자열은 None으로 정규화해 `npm run ""` 같은 빈 인자 생성을 방지.
            *node.script_name = if name.trim().is_empty() {
                None
            } else {
                Some(name)
            };
        }

        Task::none()
    }

    fn handle_node_arguments_changed(&mut self, value: String) -> Task<Message> {
        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            *node.arguments = value;
        }

        Task::none()
    }

    fn handle_node_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(node) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::node_mut)
        {
            *node.node_options = value;
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
                    matches!(config.config_type(), ConfigurationType::Node),
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
                self.node_available_scripts.clear();
                self.node_available_package_jsons.clear();
                self.env_modal = None;
                self.status_message = if let Some(path) = &self.last_file_path {
                    format!("Loaded: {}", path.display())
                } else {
                    String::from("Configurations loaded")
                };

                if !self.configurations.is_empty() {
                    self.selected_config_index = Some(0);
                    self.load_node_metadata_for_current_configurations()
                } else {
                    Task::none()
                }
            }
            Err(error) => {
                self.status_message = format!("Load failed: {error}");
                Task::none()
            }
        }
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
                self.status_message = cancellable_status(&error, "Save");
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
                self.node_available_scripts.clear();
                self.node_available_package_jsons.clear();
                self.env_modal = None;
                self.selected_config_index = (!self.configurations.is_empty()).then_some(0);
                let metadata_task = self.load_node_metadata_for_current_configurations();

                self.last_file_path = Some(path.clone());
                self.save_app_settings();
                self.status_message = format!("Opened: {}", path.display());
                metadata_task
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Open");
                Task::none()
            }
        }
    }

    /// 현재 구성들의 Node 메타데이터(package.json 목록 + scripts)를 백그라운드에서 로드.
    /// 재귀 디렉터리 스캔이 무거울 수 있어 UI 스레드(update)가 아닌 워커에서 수행하고,
    /// 결과는 NodeMetadataLoaded로 받아 적용한다 (이전엔 로드/열기 시 모든 Node 구성을
    /// 동기 스캔해 창 전체가 그동안 멈췄다 — blocking-on-UI-thread 버그).
    fn load_node_metadata_for_current_configurations(&self) -> Task<Message> {
        let tasks: Vec<Task<Message>> = self
            .collect_node_config_paths()
            .into_iter()
            .map(|(config_id, project_dir, working_dir)| {
                Task::perform(
                    scan_node_metadata(config_id, project_dir, working_dir),
                    |(id, package_jsons, scripts)| {
                        Message::NodeMetadataLoaded(id, package_jsons, scripts)
                    },
                )
            })
            .collect();
        Task::batch(tasks)
    }

    fn handle_node_metadata_loaded(
        &mut self,
        config_id: Uuid,
        package_jsons: Vec<String>,
        scripts: Vec<String>,
    ) -> Task<Message> {
        self.node_available_package_jsons
            .insert(config_id, package_jsons);
        self.node_available_scripts.insert(config_id, scripts);
        self.sync_editor_select_state_for_selected_config();
        Task::none()
    }

    fn collect_node_config_paths(&self) -> Vec<(Uuid, String, String)> {
        self.configurations.iter().filter_map(node_paths).collect()
    }

    fn selected_config_id(&self) -> Option<Uuid> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get(idx))
            .map(|config| config.id)
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
        // 세션이 이미 제거/완료된 뒤 늦게 도착한 ProcessStarted라면 등록하지 않는다.
        // 등록만 되고 완료 시 unregister가 매칭되지 않으면 PID가 레지스트리에 영구
        // 누수되어, 시그널 핸들러가 재사용된 무관한 PID를 kill할 수 있다.
        if let Some(session) = self.session_by_id_mut(session_id)
            && session.is_running
        {
            // 시그널 핸들러(Ctrl+C)가 정리할 수 있도록 전역 레지스트리에 등록.
            register_running_pid(pid);
            session.process_pid = Some(pid);
            eprintln!("[Process] Started {} (PID: {})", session.config_name, pid);
        }

        Task::none()
    }

    fn handle_output_received(&mut self, session_id: Uuid, output: &str) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            // `output`은 단일 줄이 아니라 executor가 묶어 보낸 멀티라인 배치일 수 있다
            // (process_output_loop의 출력 coalescing — UI 메시지 폭주 방지). `lines()`로
            // 분할해 줄 단위로 누적한다(add_output_line이 내부에서 '\n' 재분할). 배치는
            // 항상 trailing '\n'으로 끝나며 `lines()`가 이를 무시하므로 빈 줄은 생기지 않는다.
            for line in output.lines() {
                session.add_output_line(line);
            }

            if session.auto_scroll {
                session.scroll_progress = 1.0;
            }

            // 출력이 바뀌었으니 검색 매치 캐시 갱신 (검색바 열려 있을 때만).
            if session.search.is_some() {
                session.refresh_search_matches();
            }
        }

        Task::none()
    }

    fn handle_run_completed(
        &mut self,
        session_id: Uuid,
        result: Result<i32, String>,
    ) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.is_running = false;
            session.finished_at = Some(SystemTime::now());
            // PID 추적 해제 + 세션에서도 제거 (Windows PID 재사용으로 인한 오인 kill 방지).
            if let Some(pid) = session.process_pid.take() {
                unregister_running_pid(pid);
            }
            match result {
                Ok(code) => {
                    let config_name = session.config_name.clone();
                    session.exit_code = Some(code);
                    session.add_output_line(&format!("\nProcess exited with code: {code}"));
                    self.status_message = format!("Completed (exit code: {code})");
                    // 창이 백그라운드일 때만 데스크톱 알림 (보고 있는 실행은 알리지 않음)
                    if !self.is_window_focused {
                        notify_run_finished(&config_name, Ok(code));
                    }
                }
                Err(error) => {
                    let config_name = session.config_name.clone();
                    session.add_output_line(&format!("\nError: {error}"));
                    self.status_message = format!("Failed: {error}");
                    // 스폰/실행 실패도 백그라운드면 알림 (가장 중요한 실패 케이스)
                    if !self.is_window_focused {
                        notify_run_finished(&config_name, Err(&error));
                    }
                }
            }
        }

        // 완료 라인("exited with code")이 추가됐으니 검색 매치 캐시 갱신.
        // (status_message 대입과 borrow가 겹치지 않도록 별도 lookup으로 수행)
        if let Some(session) = self.session_by_id_mut(session_id)
            && session.search.is_some()
        {
            session.refresh_search_matches();
        }

        Task::none()
    }

    fn handle_open_url(&mut self, url: &str) -> Task<Message> {
        // 결과를 반영한다. Linux에 xdg-open(xdg-utils)이 없으면 Err가 나는데,
        // 무시하면 "Opening URL"만 표시되고 아무 일도 안 일어나 혼란을 준다.
        self.status_message = match open::that(url) {
            Ok(()) => format!("Opening URL: {url}"),
            Err(error) => format!("Failed to open URL: {error}"),
        };
        Task::none()
    }

    /// 최신 버전 수동 확인 트리거 (상태바 버전 라벨 클릭).
    /// 이미 확인 중이면 중복 요청을 무시한다.
    fn handle_check_for_updates(&mut self) -> Task<Message> {
        if self.is_checking_update {
            return Task::none();
        }
        self.status_message = String::from("Checking for updates…");
        self.is_checking_update = true;
        self.update_spinner_frame = 0;
        Task::perform(check_latest_release(), Message::UpdateCheckCompleted)
    }

    /// 로딩 스피너 프레임 진행 (확인 중 타이머 tick).
    fn handle_update_spinner_tick(&mut self) -> Task<Message> {
        self.update_spinner_frame = self.update_spinner_frame.wrapping_add(1);
        Task::none()
    }

    /// 최신 버전 확인 결과 처리. 새 버전이면 상태를 저장하고 OS 알림을 발행한다.
    /// 시작 시 자동 체크의 실패는 상태바 메시지로만 조용히 알린다.
    fn handle_update_check_completed(
        &mut self,
        result: Result<UpdateOutcome, String>,
    ) -> Task<Message> {
        self.is_checking_update = false;
        match result {
            Ok(UpdateOutcome::Available { latest, url, .. }) => {
                self.status_message = format!("Update available: v{latest}");
                self.update_available = Some((latest.clone(), url.clone()));
                notify_update_available(&latest, &url);
            }
            Ok(UpdateOutcome::UpToDate { current }) => {
                self.status_message = format!("You're on the latest version (v{current})");
                self.update_available = None;
            }
            Err(error) => {
                self.status_message = format!("Update check failed: {error}");
            }
        }
        Task::none()
    }

    fn handle_session_scroll_changed(
        &mut self,
        session_id: Uuid,
        progress: f32,
        at_bottom: bool,
    ) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.scroll_progress = progress.clamp(0.0, 1.0);
            // 검색 점프(scroll_target)가 아직 소비되지 않은 publish라면 auto_scroll을 건드리지
            // 않는다. 마지막 매치가 바닥이면 at_bottom=true로 와서 자동 추적이 의도와 달리
            // 켜지기 때문(점프는 search_step에서 auto_scroll=false로 둔다 — 이슈 1).
            if session.scroll_target.is_none() {
                // auto_scroll은 절대 거리 기반 at_bottom으로 판정한다(이슈 2). 사용자가 바닥에서
                // 몇 줄만 위로 올려도 자동 추적이 풀려야 로그를 읽을 수 있다.
                session.auto_scroll = at_bottom;
            }
            // 1회성 점프 목표 소비 완료 처리(이슈 1).
            session.scroll_target = None;
        }

        Task::none()
    }

    fn handle_toggle_auto_scroll(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.auto_scroll = !session.auto_scroll;
            if session.auto_scroll {
                session.scroll_progress = 1.0;
            }
        }

        Task::none()
    }

    fn handle_open_session_search(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            if session.search.is_none() {
                session.search = Some(SearchState::default());
            }
            session.refresh_search_matches();
        }
        // 검색바 입력에 포커스 (다음 view에서 위젯이 존재)
        iced::widget::operation::focus(crate::views::shared::session_search_input_id(session_id))
    }

    fn handle_close_session_search(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.search = None;
        }
        Task::none()
    }

    fn handle_session_search_changed(&mut self, session_id: Uuid, query: String) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            if let Some(search) = session.search.as_mut() {
                search.query = query;
                search.current = 0; // 검색어 변경 시 첫 매치부터
            }
            session.refresh_search_matches();
        }
        Task::none()
    }

    fn handle_toggle_session_search_filter(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id)
            && let Some(search) = session.search.as_mut()
        {
            // 필터 토글은 매치 집합을 바꾸지 않으므로 refresh_search_matches 불필요;
            // 표시만 바뀌므로 current만 첫 매치로 리셋한다.
            search.filter = !search.filter;
            search.current = 0;
        }
        Task::none()
    }

    fn handle_toggle_session_search_regex(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            if let Some(search) = session.search.as_mut() {
                search.regex = !search.regex;
                search.current = 0;
            }
            // 정규식 모드 변경은 매치 집합을 바꾸므로 캐시 재계산.
            session.refresh_search_matches();
        }
        Task::none()
    }

    /// 세션 출력 전체를 평문으로 펼쳐 파일로 저장(다이얼로그). RENDER_LINE_LIMIT와
    /// 무관하게 버퍼에 남은 모든 라인을 내보낸다.
    fn handle_export_session_output(&mut self, session_id: Uuid) -> Task<Message> {
        let Some(session) = self.sessions.iter().find(|s| s.id == session_id) else {
            return Task::none();
        };
        let content: String = session
            .output_lines
            .iter()
            .map(|(_, segments)| {
                segments
                    .iter()
                    .map(|seg| seg.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        // 파일명에 부적합한 문자(경로 구분/예약문자/제어문자)를 치환한 기본 이름
        // (다이얼로그에서 수정 가능).
        let safe_name: String = session
            .config_name
            .chars()
            .map(|c| {
                if c.is_ascii_control()
                    || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let suggested = format!("{safe_name}-output.log");
        self.status_message = String::from("Exporting output...");
        Task::perform(
            export_text(content, suggested),
            Message::SessionOutputExported,
        )
    }

    fn handle_session_output_exported(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        self.status_message = match result {
            Ok(path) => format!("Output saved: {}", path.display()),
            Err(error) => cancellable_status(&error, "Export"),
        };
        Task::none()
    }

    /// 다음/이전 매치로 이동(`direction`: +1 다음, -1 이전)하고 해당 라인으로 스크롤.
    /// 매치는 캐시(`search.matches`)를 사용한다 (키 입력 시점엔 출력 변화가 없어 신선).
    fn handle_session_search_step(&mut self, session_id: Uuid, direction: i32) -> Task<Message> {
        let Some(session) = self.session_by_id_mut(session_id) else {
            return Task::none();
        };
        let len = session.search.as_ref().map_or(0, |s| s.matches.len());
        if len == 0 {
            return Task::none();
        }
        let new_current = {
            let Some(search) = session.search.as_mut() else {
                return Task::none();
            };
            let base = search.current.min(len - 1);
            search.current = if direction >= 0 {
                (base + 1) % len
            } else {
                (base + len - 1) % len
            };
            search.current
        };
        // 매치 라인으로 점프한다. 논리줄 인덱스를 scroll_target에 실어 보내면 터미널 뷰가
        // wrapped offset으로 변환해 스크롤한다(이슈 1). 비율 기반은 wrapping과 어긋날뿐더러
        // 같은 세션에서는 offset에 반영조차 되지 않았다. 자동 추적은 해제.
        let Some(line) = session
            .search
            .as_ref()
            .and_then(|s| s.matches.get(new_current).copied())
        else {
            return Task::none();
        };
        session.scroll_target = Some(line);
        session.auto_scroll = false;
        Task::none()
    }

    /// 검색 네비게이션 키 → 메시지. Enter = 다음 매치, Shift+Enter = 이전 매치.
    /// 대상 세션은 핸들러가 active pane에서 해석한다(`event::listen_with`는 fn 포인터만
    /// 받아 세션 id를 캡처할 수 없다). subscription과 단위 테스트가 공유한다.
    fn search_nav_message(shift: bool) -> Message {
        if shift {
            Message::SearchPrevInActivePane
        } else {
            Message::SearchNextInActivePane
        }
    }

    /// Ctrl+F/ESC가 대상으로 삼을 세션. 활성 탭의 leaf pane 중 이미 검색바가 열린
    /// 세션을 우선, 없으면 첫 세션 pane (다중 pane에서 정확한 대상은 pane의 검색 버튼 사용).
    fn focused_search_session_id(&self) -> Option<Uuid> {
        let tab = self.workspace_tabs.get(self.selected_tab_index)?;
        let leaves = tab.layout_tree.collect_leaves();
        leaves
            .iter()
            .filter_map(|(_, pane)| pane.session_id)
            .find(|id| {
                self.sessions
                    .iter()
                    .any(|s| s.id == *id && s.search.is_some())
            })
            .or_else(|| leaves.iter().find_map(|(_, pane)| pane.session_id))
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
        self.drag.hovered_tab_index = tab_index;
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
        }

        Task::none()
    }

    fn handle_rerun_session(&mut self, session_id: Uuid) -> Task<Message> {
        // 동시성 설계: 실행 중 재실행 시 이전 스트림과 새 스트림이 잠시 공존하지만 격리된다.
        // (1) 이전 cancel_flag.store(true)는 이전 스트림이 보유한 같은 Arc를 가리켜 이전
        //     실행을 중단시키고, 이후 cancel_flag를 새 Arc로 교체해 새 스트림과 분리한다.
        // (2) session.id를 new_id로 바꾸므로 이전 스트림이 보내는 모든 메시지(OutputReceived/
        //     RunCompleted, old_id 키)는 더 이상 어떤 세션과도 매칭되지 않아 무시된다 —
        //     교차 배선(cross-wiring) 없음. 이전 프로세스는 이전 스트림의 terminate_and_reap가
        //     강제 종료한다. (잠깐 is_running=true인데 새 프로세스 시작 전인 transient는 무해)
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id) {
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
                // 출력이 비워졌으니 검색 매치 캐시도 비운다 (stale 인덱스 방지).
                session.refresh_search_matches();
                session.is_running = true;
                session.exit_code = None;
                session.finished_at = None;
                // 이전 실행의 PID는 더 이상 이 세션에 속하지 않는다. 새 프로세스가 PID를
                // 보고하기 전 stale PID가 kill되지 않도록 추적을 해제하고 슬롯을 비운다.
                if let Some(old_pid) = session.process_pid.take() {
                    unregister_running_pid(old_pid);
                }
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

    fn handle_stop_session(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == session_id)
            && session.is_running
        {
            session.cancel_flag.store(true, Ordering::Relaxed);
            self.status_message = String::from("Stopping session...");
        }

        Task::none()
    }

    fn handle_remove_session(&mut self, session_id: Uuid) -> Task<Message> {
        let Some(removed_pos) = self.sessions.iter().position(|s| s.id == session_id) else {
            return Task::none();
        };

        let session = &self.sessions[removed_pos];
        let running_pid = if session.is_running {
            session.cancel_flag.store(true, Ordering::Relaxed);
            session.process_pid
        } else {
            None
        };
        // 세션 제거 후에는 RunCompleted가 매칭되지 않아 PID가 해제되지 않으므로 여기서 해제.
        // 실제 종료는 cancel_flag → terminate_and_reap(SIGKILL 에스컬레이션)가 처리한다.
        if let Some(pid) = running_pid {
            unregister_running_pid(pid);
        }

        self.sessions.remove(removed_pos);
        for tab in &mut self.workspace_tabs {
            tab.remove_session(session_id);
        }

        // hovered_session_index는 표시용 인덱스(뷰에서 사용)이므로 제거 위치에 맞춰 보정.
        self.hovered_session_index = match self.hovered_session_index {
            Some(_) if self.sessions.is_empty() => None,
            Some(hovered) if hovered == removed_pos => None,
            Some(hovered) if hovered > removed_pos => Some(hovered - 1),
            other => other,
        };
        self.status_message = String::from("Session removed");

        Task::none()
    }

    fn handle_open_session_in_workspace(&mut self, session_id: Uuid) -> Task<Message> {
        if !self.sessions.iter().any(|session| session.id == session_id) {
            return Task::none();
        }

        self.ensure_workspace_tab();

        let aspect = self.workspace_content_aspect();
        let opened = self
            .workspace_tabs
            .get_mut(self.selected_tab_index)
            .is_some_and(|tab| tab.open_session(session_id, aspect));

        self.status_message = if opened {
            String::from("Session opened in workspace")
        } else {
            String::from("Session already open in workspace")
        };

        Task::none()
    }

    /// 실행 중인 모든 세션 중지 (각 세션의 기존 stop 경로 재사용).
    fn handle_stop_all_sessions(&mut self) -> Task<Message> {
        let running: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|session| session.is_running)
            .map(|session| session.id)
            .collect();

        let count = running.len();
        for session_id in running {
            let _ = self.handle_stop_session(session_id);
        }

        self.status_message = if count == 0 {
            String::from("No running sessions to stop")
        } else {
            format!("Stopping {count} session(s)...")
        };
        Task::none()
    }

    /// 모든 세션 재실행.
    fn handle_rerun_all_sessions(&mut self) -> Task<Message> {
        let ids: Vec<Uuid> = self.sessions.iter().map(|session| session.id).collect();
        self.rerun_sessions(&ids, "session(s)")
    }

    /// 실패(0이 아닌 종료 코드)한 세션만 재실행.
    fn handle_rerun_failed_sessions(&mut self) -> Task<Message> {
        let ids: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|session| {
                !session.is_running && session.exit_code.is_some_and(|code| code != 0)
            })
            .map(|session| session.id)
            .collect();
        self.rerun_sessions(&ids, "failed session(s)")
    }

    /// 주어진 세션 id들을 각각 재실행하고 Task들을 배치로 묶는다.
    fn rerun_sessions(&mut self, ids: &[Uuid], noun: &str) -> Task<Message> {
        if ids.is_empty() {
            self.status_message = format!("No {noun} to rerun");
            return Task::none();
        }

        let mut tasks = Vec::with_capacity(ids.len());
        for &session_id in ids {
            tasks.push(self.handle_rerun_session(session_id));
        }
        self.status_message = format!("Rerunning {} {noun}...", ids.len());
        Task::batch(tasks)
    }

    /// 기존 탭과 충돌하지 않는 `Workspace N` 형식의 이름 생성 (가장 작은 빈 번호).
    /// `exclude`로 지정한 인덱스의 탭은 점유 검사에서 제외한다 — 이름을 비워 확정할 때
    /// 그 탭이 원래 쓰던 번호를 그대로 되찾도록 하기 위함.
    fn next_workspace_name(&self, exclude: Option<usize>) -> String {
        let mut n = 1;
        loop {
            let candidate = format!("Workspace {n}");
            let taken = self
                .workspace_tabs
                .iter()
                .enumerate()
                .any(|(i, tab)| Some(i) != exclude && tab.name == candidate);
            if !taken {
                return candidate;
            }
            n += 1;
        }
    }

    /// base 이름으로 고유한 workspace 탭 이름을 만든다. 미사용이면 base 그대로, 충돌하면
    /// "base 2", "base 3"... 으로 빈 번호를 찾는다(`next_workspace_name`과 같은 gap-fill 방식).
    /// base 자체가 "1번"이므로 번호는 2부터 시작한다("base 1"은 어색하므로).
    fn unique_workspace_name(&self, base: &str) -> String {
        let exists = |name: &str| self.workspace_tabs.iter().any(|t| t.name == name);
        if !exists(base) {
            return base.to_string();
        }
        let mut n: usize = 2;
        loop {
            let candidate = format!("{base} {n}");
            if !exists(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    fn handle_add_workspace_tab(&mut self) -> Task<Message> {
        let name = self.next_workspace_name(None);
        self.workspace_tabs.push(WorkspaceTab::empty(name));
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
                tab.name = String::from("Workspace 1");
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
            let name = self.next_workspace_name(None);
            self.workspace_tabs.push(WorkspaceTab::empty(name));
            self.selected_tab_index = 0;
        } else if self.selected_tab_index >= self.workspace_tabs.len() {
            self.selected_tab_index = self.workspace_tabs.len() - 1;
        }
    }

    /// 워크스페이스 페인 영역의 대략적인 가로/세로 비율 (균형 분할 방향 결정용).
    ///
    /// 세션 리스트 패널과 윈도우 chrome을 대략 제외한 추정치다. 정밀할 필요는 없고
    /// (넓다 vs 높다)만 구분하면 충분하다.
    fn workspace_content_aspect(&self) -> f32 {
        const SESSION_LIST_WIDTH: f32 = 260.0;
        const HORIZONTAL_MARGIN: f32 = 40.0;
        const VERTICAL_CHROME: f32 = 96.0;

        let width = (self.window_size.width - SESSION_LIST_WIDTH - HORIZONTAL_MARGIN).max(120.0);
        let height = (self.window_size.height - VERTICAL_CHROME).max(120.0);
        width / height
    }

    /// 현재 선택된 구성의 가변 참조 가져오기
    fn get_selected_config_mut(&mut self) -> Option<&mut RunConfiguration> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get_mut(idx))
    }

    /// 현재 선택된 구성의 `type_data` 가변 참조. 타입별 필드 세터의 공통 진입점.
    fn selected_type_data_mut(&mut self) -> Option<&mut ConfigTypeData> {
        self.get_selected_config_mut()
            .map(|config| &mut config.type_data)
    }

    /// 현재 선택된 구성의 불변 참조 가져오기
    fn get_selected_config(&self) -> Option<&RunConfiguration> {
        self.selected_config_index
            .and_then(|idx| self.configurations.get(idx))
    }

    /// 세션 ID로 가변 세션 조회 (id 기반 핸들러 공통).
    fn session_by_id_mut(&mut self, session_id: Uuid) -> Option<&mut RunSession> {
        self.sessions
            .iter_mut()
            .find(|session| session.id == session_id)
    }

    /// Node 구성의 package.json 목록과 스크립트를 함께 로드.
    fn load_node_metadata(&mut self, config_id: Uuid, project_dir: &str, working_dir: &str) {
        self.load_node_package_jsons(config_id, project_dir);
        if !working_dir.is_empty() {
            self.load_node_scripts(config_id, working_dir);
        }
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
                container(
                    row![
                        text(&self.status_message).size(12),
                        Space::new().width(Length::Fill),
                        self.view_status_bar_version(),
                    ]
                    .align_y(Alignment::Center)
                    .spacing(8),
                )
                .padding(padding)
                .width(Length::Fill),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .style(status_bar_style)
    }

    /// 상태바 우측의 버전 라벨/업데이트 링크.
    /// 새 버전이 있으면 `v{현재} → v{최신} ⬆`를 눌러 릴리스 페이지를 열고,
    /// 없으면 `v{현재}`를 눌러 수동 업데이트 확인을 트리거한다.
    fn view_status_bar_version(&self) -> Element<'_, Message> {
        let current = crate::services::CURRENT_VERSION;

        // 확인 중에는 회전 스피너를 표시하고 버튼을 비활성화(on_press 생략)한다.
        if self.is_checking_update {
            let frame =
                UPDATE_SPINNER_FRAMES[self.update_spinner_frame % UPDATE_SPINNER_FRAMES.len()];
            return button(text(format!("{frame} checking")).size(12))
                .padding([1, 6])
                .style(move |theme: &Theme, status| status_bar_version_style(theme, status, false))
                .into();
        }

        let (label, on_press) = match &self.update_available {
            Some((latest, url)) => (
                format!("v{current} → v{latest} ⬆"),
                Message::OpenUrl(url.clone()),
            ),
            None => (format!("v{current}"), Message::CheckForUpdates),
        };
        let has_update = self.update_available.is_some();

        button(text(label).size(12))
            .padding([1, 6])
            .on_press(on_press)
            .style(move |theme: &Theme, status| status_bar_version_style(theme, status, has_update))
            .into()
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

        let has_running = self.sessions.iter().any(|session| session.is_running);
        let has_failed = self
            .sessions
            .iter()
            .any(|session| !session.is_running && session.exit_code.is_some_and(|code| code != 0));

        let mut header = row![text("Sessions").size(13), Space::new().width(Length::Fill)]
            .spacing(3)
            .align_y(Alignment::Center)
            .padding([0, 8]);

        if !self.sessions.is_empty() {
            header = header
                .push(Self::view_session_action_button(
                    "Stop all running",
                    ICON_STOP,
                    has_running.then_some(Message::StopAllSessions),
                    SessionActionKind::Stop,
                    has_running,
                ))
                .push(Self::view_session_action_button(
                    "Rerun all",
                    ICON_REFRESH,
                    Some(Message::RerunAllSessions),
                    SessionActionKind::Remove,
                    true,
                ))
                .push(Self::view_session_action_button(
                    "Rerun failed",
                    ICON_PLAY,
                    has_failed.then_some(Message::RerunFailedSessions),
                    SessionActionKind::Remove,
                    has_failed,
                ))
                .push(Space::new().width(4));
        }

        let header = header.push(text(self.sessions.len().to_string()).size(11));

        let mut list = column![].spacing(4).padding([8, 6]);

        if self.sessions.is_empty() {
            list = list.push(
                container(text("Run a configuration to create a session.").size(12))
                    .width(Length::Fill)
                    .padding([8, 10])
                    .style(session_empty_state_style),
            );
        } else {
            let name_max_width = session_name_max_width();
            for (index, session) in self.sessions.iter().enumerate() {
                let is_open_in_workspace =
                    current_tab.is_some_and(|tab| tab.contains_session(session.id));
                let is_hovered = self.hovered_session_index == Some(index);

                list = list.push(Self::view_session_list_item(
                    index,
                    session,
                    is_open_in_workspace,
                    is_hovered,
                    name_max_width,
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
        name_max_width: usize,
    ) -> Element<'_, Message> {
        let display_name = crate::utils::truncate_text(&session.config_name, name_max_width);

        // 툴팁 박스 폭을 이름 영역 폭과 맞춰 좌측 가장자리를 이름 시작점에 정렬한다.
        // title 텍스트의 size(12)와 동일한 폰트 크기로 폭을 계산하고 툴팁도 같은 크기로 렌더한다.
        const NAME_FONT_SIZE: f32 = 12.0;
        let tooltip_width =
            name_max_width as f32 * crate::utils::monospace_char_width(NAME_FONT_SIZE);

        let title = text(display_name)
            .font(crate::D2CODING)
            .size(NAME_FONT_SIZE)
            .wrapping(text::Wrapping::None);

        let title_area: Element<'_, Message> = container(title)
            .width(Length::Fill)
            .center_y(Length::Shrink)
            .into();

        let title_with_tooltip = tooltip(
            title_area,
            crate::widgets::configuration_list::view_name_tooltip(
                &session.config_name,
                tooltip_width,
                NAME_FONT_SIZE,
            ),
            tooltip::Position::Top,
        )
        .gap(4);

        // 상태 배지 (고정 폭, 완료된 세션만 텍스트 표시 — 실행 중은 점으로 충분)
        let is_failed = matches!(session.status_kind(), SessionStatusKind::Failed(_));
        let badge_text = if session.is_running {
            String::new()
        } else {
            session.status_badge_label()
        };
        let badge = text(badge_text)
            .size(10)
            .width(Length::Fixed(SESSION_STATUS_BADGE_WIDTH))
            .align_x(iced::alignment::Horizontal::Right)
            .wrapping(text::Wrapping::None)
            .style(move |theme: &Theme| text::Style {
                color: Some(if is_failed {
                    theme.extended_palette().danger.base.color
                } else {
                    Color {
                        a: 0.5,
                        ..theme.extended_palette().background.base.text
                    }
                }),
            });

        let item_content = row![
            container(Space::new())
                .width(7)
                .height(7)
                .style(move |theme: &Theme| session_list_status_dot_style(theme, session)),
            title_with_tooltip,
            badge,
            row![
                Self::view_session_action_button(
                    "Stop session",
                    ICON_STOP,
                    session
                        .is_running
                        .then_some(Message::StopSession(session.id)),
                    SessionActionKind::Stop,
                    session.is_running,
                ),
                Self::view_session_action_button(
                    "Remove session",
                    ICON_DELETE,
                    Some(Message::RemoveSession(session.id)),
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
            .on_press(Message::OpenSessionInWorkspace(session.id))
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

        let content = row![self.view_session_list_panel(), tab_pane_area]
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

        let window_focus_subscription = event::listen_with(|event, _status, _id| match event {
            Event::Window(window::Event::Focused) => Some(Message::WindowFocusChanged(true)),
            Event::Window(window::Event::Unfocused) => Some(Message::WindowFocusChanged(false)),
            _ => None,
        });

        // 세션 화면에서 Ctrl/Cmd+F = 검색 열기. 모달이 열려 있으면 비활성화. 대상
        // 세션은 핸들러가 해석한다(클로저는 fn 포인터라 self 접근 불가).
        let sessions_active =
            matches!(self.current_view, ViewMode::Sessions) && self.env_modal.is_none();
        let search_open_subscription = if sessions_active {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Character(ref c),
                    modifiers,
                    ..
                }) if modifiers.command() && c.as_str() == "f" => {
                    Some(Message::OpenSearchInActivePane)
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };
        // ESC = 검색 닫기. 실제로 검색바가 열려 있을 때만 활성화해 다른 ESC 용도와
        // 충돌하지 않게 한다 (검색이 없으면 ESC를 가로채지 않음).
        let search_close_subscription =
            if sessions_active && self.sessions.iter().any(|s| s.search.is_some()) {
                event::listen_with(|event, _status, _id| match event {
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    }) => Some(Message::CloseActiveSearch),
                    _ => None,
                })
            } else {
                Subscription::none()
            };
        // 검색바가 열려 있을 때 Enter = 다음 매치, Shift+Enter = 이전 매치. text_input의
        // on_submit은 modifiers를 몰라 Shift를 구분하지 못하므로 여기서 처리하고 on_submit은
        // 제거했다. 대상은 active pane의 "검색이 열린" 세션. tab 이름 편집 중에는 Enter가
        // 이름 제출과 겹치지 않도록 비활성화한다.
        // 대상 세션을 캡처하지 않는다(listen_with는 fn 포인터만 받음 — 위 search_open/close와
        // 동일하게 핸들러가 active pane에서 해석). tab 이름 편집 중에는 Enter가 이름 제출과
        // 겹치지 않도록 비활성화한다.
        let search_nav_active = sessions_active
            && self.tab_ui.editing_tab_name.is_none()
            && self.sessions.iter().any(|s| s.search.is_some());
        let search_nav_subscription = if search_nav_active {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Enter),
                    modifiers,
                    ..
                }) => Some(Self::search_nav_message(modifiers.shift())),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 업데이트 확인 중에만 스피너 애니메이션 tick을 발행한다.
        let update_spinner_subscription = if self.is_checking_update {
            iced::time::every(Duration::from_millis(120)).map(|_| Message::UpdateSpinnerTick)
        } else {
            Subscription::none()
        };

        Subscription::batch([
            cursor_subscription,
            configuration_release_subscription,
            editor_focus_subscription,
            env_modal_keyboard_subscription,
            tab_name_edit_subscription,
            window_focus_subscription,
            search_open_subscription,
            search_close_subscription,
            search_nav_subscription,
            update_spinner_subscription,
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

        // 종료 경로이므로 graceful 유예(2초 sleep) 없이 즉시 강제 종료한다. 이전 구현은
        // Drop이 실행되는 UI/메인 스레드를 2초간 std::thread::sleep으로 블로킹해 종료 시
        // 창이 멈췄다. graceful 종료(SIGTERM 유예)는 정상 Stop 경로의 비동기
        // terminate_and_reap가 담당하고, 여기서는 종료 시 자식 누수 방지가 목적이다.
        for session in &running_sessions {
            if let Some(pid) = session.process_pid {
                #[cfg(unix)]
                unsafe {
                    // Unix: 프로세스 그룹 전체에 SIGKILL
                    libc::killpg(pid as i32, libc::SIGKILL);
                    eprintln!(
                        "[Shutdown] Sent SIGKILL to PID {} ({})",
                        pid, session.config_name
                    );
                }

                #[cfg(windows)]
                {
                    // Windows: /T /F로 프로세스 트리 강제 종료
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RunConfiguration;

    fn manager_with_configs(names: &[&str]) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.configurations = names
            .iter()
            .map(|name| RunConfiguration {
                name: (*name).to_string(),
                ..RunConfiguration::default()
            })
            .collect();
        app.selected_config_index = None;
        app
    }

    fn names(app: &RunConfigManager) -> Vec<String> {
        app.configurations.iter().map(|c| c.name.clone()).collect()
    }

    #[test]
    fn unique_clone_name_appends_copy_then_escalates() {
        assert_eq!(unique_clone_name("App", &["App"]), "App (copy)");
        assert_eq!(
            unique_clone_name("App", &["App", "App (copy)"]),
            "App (copy 2)"
        );
        assert_eq!(
            unique_clone_name("App", &["App", "App (copy)", "App (copy 2)"]),
            "App (copy 3)"
        );
    }

    #[test]
    fn unique_clone_name_of_a_copy_does_not_collide() {
        assert_eq!(
            unique_clone_name("X (copy)", &["X (copy)"]),
            "X (copy) (copy)"
        );
    }

    #[test]
    fn configuration_drop_index_self_and_adjacent_are_noops() {
        use ConfigurationDropPosition::{After, Before};
        // 자기 자신/인접 위치에 드롭하면 from 그대로 (no-op)
        assert_eq!(RunConfigManager::configuration_drop_index(2, 2, Before), 2);
        assert_eq!(RunConfigManager::configuration_drop_index(2, 3, Before), 2);
        assert_eq!(RunConfigManager::configuration_drop_index(2, 2, After), 2);
    }

    #[test]
    fn configuration_drop_index_maps_position_to_insertion() {
        use ConfigurationDropPosition::{After, Before};
        assert_eq!(RunConfigManager::configuration_drop_index(2, 5, Before), 5);
        assert_eq!(RunConfigManager::configuration_drop_index(2, 5, After), 6);
        assert_eq!(RunConfigManager::configuration_drop_index(2, 0, Before), 0);
    }

    #[test]
    fn move_configuration_down_reorders_and_tracks_selection() {
        let mut app = manager_with_configs(&["A", "B", "C", "D"]);
        app.selected_config_index = Some(0); // 이동되는 항목 선택
        let final_index = app.move_configuration_to(0, 2);
        assert_eq!(names(&app), vec!["B", "A", "C", "D"]);
        assert_eq!(final_index, 1);
        assert_eq!(app.selected_config_index, Some(1));
    }

    #[test]
    fn move_configuration_up_reorders() {
        let mut app = manager_with_configs(&["A", "B", "C", "D"]);
        let final_index = app.move_configuration_to(3, 1);
        assert_eq!(names(&app), vec!["A", "D", "B", "C"]);
        assert_eq!(final_index, 1);
    }

    #[test]
    fn move_configuration_noop_when_from_equals_to() {
        let mut app = manager_with_configs(&["A", "B", "C"]);
        let final_index = app.move_configuration_to(1, 1);
        assert_eq!(names(&app), vec!["A", "B", "C"]);
        assert_eq!(final_index, 1);
    }

    #[test]
    fn move_configuration_shifts_selection_when_crossing() {
        // selected가 이동 구간을 가로지를 때 인덱스 보정
        let mut app = manager_with_configs(&["A", "B", "C", "D"]);
        app.selected_config_index = Some(1); // B 선택
        app.move_configuration_to(0, 3); // A를 뒤로 이동: from<selected<=target → -1
        assert_eq!(app.selected_config_index, Some(0));
    }

    #[test]
    fn delete_middle_keeps_index_pointing_to_next() {
        let mut app = manager_with_configs(&["A", "B", "C"]);
        app.selected_config_index = Some(1);
        let _ = app.handle_delete_configuration(Some(1));
        assert_eq!(names(&app), vec!["A", "C"]);
        assert_eq!(app.selected_config_index, Some(1)); // 이제 C를 가리킴
    }

    #[test]
    fn delete_last_clamps_selection() {
        let mut app = manager_with_configs(&["A", "B"]);
        app.selected_config_index = Some(1);
        let _ = app.handle_delete_configuration(Some(1));
        assert_eq!(names(&app), vec!["A"]);
        assert_eq!(app.selected_config_index, Some(0));
    }

    #[test]
    fn delete_only_config_clears_selection() {
        let mut app = manager_with_configs(&["A"]);
        app.selected_config_index = Some(0);
        let _ = app.handle_delete_configuration(Some(0));
        assert!(app.configurations.is_empty());
        assert_eq!(app.selected_config_index, None);
    }

    fn manager_with_sessions(names: &[&str]) -> (RunConfigManager, Vec<Uuid>) {
        let (mut app, _task) = RunConfigManager::new();
        app.sessions = names
            .iter()
            .map(|n| RunSession::new((*n).to_string()))
            .collect();
        let ids = app.sessions.iter().map(|s| s.id).collect();
        (app, ids)
    }

    #[test]
    fn remove_session_by_id_removes_the_correct_session() {
        let (mut app, ids) = manager_with_sessions(&["a", "b", "c"]);
        let _ = app.handle_remove_session(ids[1]);
        assert_eq!(app.sessions.len(), 2);
        assert_eq!(app.sessions[0].id, ids[0]);
        assert_eq!(app.sessions[1].id, ids[2]);
        assert!(app.sessions.iter().all(|s| s.id != ids[1]));
    }

    #[test]
    fn remove_session_unknown_id_is_noop() {
        let (mut app, _ids) = manager_with_sessions(&["a"]);
        let _ = app.handle_remove_session(Uuid::new_v4());
        assert_eq!(app.sessions.len(), 1);
    }

    #[test]
    fn remove_session_shifts_hovered_index() {
        let (mut app, ids) = manager_with_sessions(&["a", "b", "c"]);
        app.hovered_session_index = Some(2); // "c" hover 중
        let _ = app.handle_remove_session(ids[0]); // 앞 항목 제거 → hover 인덱스 -1
        assert_eq!(app.hovered_session_index, Some(1));
    }

    #[test]
    fn stop_session_by_id_sets_cancel_flag() {
        let (mut app, ids) = manager_with_sessions(&["a", "b"]);
        assert!(!app.sessions[1].cancel_flag.load(Ordering::Relaxed));
        let _ = app.handle_stop_session(ids[1]);
        assert!(app.sessions[1].cancel_flag.load(Ordering::Relaxed));
        // 다른 세션은 영향 없음
        assert!(!app.sessions[0].cancel_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn next_workspace_name_numbers_and_fills_gaps() {
        let (mut app, _task) = RunConfigManager::new(); // 초기 탭 "Workspace 1"
        assert_eq!(app.next_workspace_name(None), "Workspace 2");

        app.workspace_tabs
            .push(WorkspaceTab::empty(String::from("Workspace 2")));
        assert_eq!(app.next_workspace_name(None), "Workspace 3");

        // 중간 번호를 비우면 가장 작은 빈 번호를 재사용
        app.workspace_tabs[0].name = String::from("Renamed");
        assert_eq!(app.next_workspace_name(None), "Workspace 1");
    }

    #[test]
    fn next_workspace_name_excludes_self_so_renamed_tab_reclaims_its_number() {
        let (mut app, _task) = RunConfigManager::new(); // ["Workspace 1"]
        app.workspace_tabs
            .push(WorkspaceTab::empty(String::from("Workspace 2")));
        // 인덱스 1("Workspace 2")을 제외하면 그 번호가 비어 보여 자기 번호를 재사용
        assert_eq!(app.next_workspace_name(Some(1)), "Workspace 2");
        // 제외하지 않으면 다음 빈 번호
        assert_eq!(app.next_workspace_name(None), "Workspace 3");
    }

    #[test]
    fn run_compound_fans_out_to_member_sessions() {
        let mut app = manager_with_configs(&["A", "B", "Bundle"]);
        let a_id = app.configurations[0].id;
        let b_id = app.configurations[1].id;
        app.configurations[2].type_data = ConfigTypeData::Compound {
            members: vec![a_id, b_id],
            workspace: None,
        };
        let _ = app.handle_run_configuration(Some(2));
        assert_eq!(app.sessions.len(), 2);
        let names: Vec<&str> = app
            .sessions
            .iter()
            .map(|s| s.config_name.as_str())
            .collect();
        assert!(names.contains(&"A") && names.contains(&"B"));
    }

    #[test]
    fn run_compound_skips_missing_and_nested_members() {
        let mut app = manager_with_configs(&["A", "Inner", "Bundle"]);
        let a_id = app.configurations[0].id;
        app.configurations[1].type_data = ConfigTypeData::Compound {
            members: vec![],
            workspace: None,
        };
        let inner_id = app.configurations[1].id;
        app.configurations[2].type_data = ConfigTypeData::Compound {
            members: vec![a_id, inner_id, Uuid::new_v4()],
            workspace: None,
        };
        let _ = app.handle_run_configuration(Some(2));
        assert_eq!(app.sessions.len(), 1); // 중첩 Compound와 누락 멤버는 건너뜀
        assert_eq!(app.sessions[0].config_name, "A");
    }

    #[test]
    fn compound_member_add_dedups_and_excludes_self() {
        let mut app = manager_with_configs(&["A", "Bundle"]);
        let a_id = app.configurations[0].id;
        let bundle_id = app.configurations[1].id;
        app.configurations[1].type_data = ConfigTypeData::Compound {
            members: vec![],
            workspace: None,
        };
        app.selected_config_index = Some(1);
        let _ = app.handle_compound_member_added(a_id);
        let _ = app.handle_compound_member_added(a_id); // 중복 무시
        let _ = app.handle_compound_member_added(bundle_id); // 자기 자신 무시
        let ConfigTypeData::Compound { members, .. } = &app.configurations[1].type_data else {
            panic!("expected compound");
        };
        assert_eq!(members, &vec![a_id]);
    }

    #[test]
    fn delete_strips_member_from_compound() {
        let mut app = manager_with_configs(&["A", "B", "Bundle"]);
        let a_id = app.configurations[0].id;
        let b_id = app.configurations[1].id;
        app.configurations[2].type_data = ConfigTypeData::Compound {
            members: vec![a_id, b_id],
            workspace: None,
        };
        app.selected_config_index = Some(2);
        let _ = app.handle_compound_member_removed(b_id);
        let _ = app.handle_delete_configuration(Some(0)); // "A" 삭제
        let compound = app
            .configurations
            .iter()
            .find(|c| matches!(c.type_data, ConfigTypeData::Compound { .. }))
            .expect("compound still present");
        let ConfigTypeData::Compound { members, .. } = &compound.type_data else {
            unreachable!()
        };
        assert!(
            members.is_empty(),
            "deleted ids must be stripped from members"
        );
    }

    #[test]
    fn unique_workspace_name_keeps_base_then_numbers_on_collision() {
        let mut app = manager_with_configs(&["A"]);
        // 기본 탭은 "Workspace 1" → "Backend"는 미사용이므로 그대로.
        assert_eq!(app.unique_workspace_name("Backend"), "Backend");
        app.workspace_tabs
            .push(WorkspaceTab::empty("Backend".to_string()));
        assert_eq!(app.unique_workspace_name("Backend"), "Backend 2");
        app.workspace_tabs
            .push(WorkspaceTab::empty("Backend 2".to_string()));
        assert_eq!(app.unique_workspace_name("Backend"), "Backend 3");
    }

    #[test]
    fn run_compound_with_workspace_opens_new_named_tab() {
        let mut app = manager_with_configs(&["A", "B", "Bundle"]);
        let a_id = app.configurations[0].id;
        let b_id = app.configurations[1].id;
        app.configurations[2].type_data = ConfigTypeData::Compound {
            members: vec![a_id, b_id],
            workspace: Some("Backend".to_string()),
        };
        let tabs_before = app.workspace_tabs.len();

        let _ = app.handle_run_configuration(Some(2));

        assert_eq!(
            app.workspace_tabs.len(),
            tabs_before + 1,
            "workspace 지정 시 새 탭 생성"
        );
        assert_eq!(
            app.workspace_tabs[app.selected_tab_index].name, "Backend",
            "지정한 이름의 탭으로 전환"
        );
        assert_eq!(app.sessions.len(), 2, "멤버 2개가 실행됨");
    }

    #[test]
    fn search_step_cycles_matches_and_scrolls_to_match() {
        let (mut app, ids) = manager_with_sessions(&["s"]);
        let sid = ids[0];
        {
            let session = &mut app.sessions[0];
            session.add_output_line("alpha"); // 0
            session.add_output_line("beta error"); // 1 (match)
            session.add_output_line("gamma"); // 2
            session.add_output_line("delta error"); // 3 (match)
        }
        let _ = app.handle_open_session_search(sid);
        let _ = app.handle_session_search_changed(sid, "error".to_string());

        let search = app.sessions[0].search.as_ref().unwrap();
        assert_eq!(search.matches, vec![1, 3]);
        assert_eq!(search.current, 0);

        // next: current 0->1, 매치 라인 3을 scroll_target으로 설정, auto_scroll off
        let _ = app.handle_session_search_step(sid, 1);
        assert_eq!(app.sessions[0].search.as_ref().unwrap().current, 1);
        assert_eq!(app.sessions[0].scroll_target, Some(3));
        assert!(!app.sessions[0].auto_scroll);

        // next wraps 1->0, prev wraps 0->1
        let _ = app.handle_session_search_step(sid, 1);
        assert_eq!(app.sessions[0].search.as_ref().unwrap().current, 0);
        let _ = app.handle_session_search_step(sid, -1);
        assert_eq!(app.sessions[0].search.as_ref().unwrap().current, 1);
    }

    #[test]
    fn search_nav_enter_next_shift_enter_prev() {
        assert!(
            matches!(
                RunConfigManager::search_nav_message(false),
                Message::SearchNextInActivePane
            ),
            "Enter → 다음 매치"
        );
        assert!(
            matches!(
                RunConfigManager::search_nav_message(true),
                Message::SearchPrevInActivePane
            ),
            "Shift+Enter → 이전 매치"
        );
    }

    #[test]
    fn search_current_clamps_when_matches_shrink() {
        let (mut app, ids) = manager_with_sessions(&["s"]);
        let sid = ids[0];
        {
            let session = &mut app.sessions[0];
            session.add_output_line("e1 error");
            session.add_output_line("e2 error");
            session.add_output_line("e3 error");
        }
        let _ = app.handle_open_session_search(sid);
        let _ = app.handle_session_search_changed(sid, "error".to_string());
        let _ = app.handle_session_search_step(sid, 1); // ->1
        let _ = app.handle_session_search_step(sid, 1); // ->2 (last)
        assert_eq!(app.sessions[0].search.as_ref().unwrap().current, 2);

        // 매치가 줄면 current가 범위로 클램프되어야 한다.
        {
            let session = &mut app.sessions[0];
            session.clear_output();
            session.add_output_line("only error");
            session.refresh_search_matches();
        }
        let search = app.sessions[0].search.as_ref().unwrap();
        assert_eq!(search.matches, vec![0]);
        assert_eq!(search.current, 0);
    }

    fn manager_with_named_sessions(names: &[&str]) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        // 재실행이 이름으로 구성을 찾으므로 동일 이름의 구성도 함께 만든다.
        app.configurations = names
            .iter()
            .map(|n| RunConfiguration {
                name: (*n).to_string(),
                ..RunConfiguration::default()
            })
            .collect();
        app.sessions = names
            .iter()
            .map(|n| RunSession::new((*n).to_string()))
            .collect();
        app
    }

    #[test]
    fn stop_all_cancels_only_running_sessions() {
        let (mut app, _ids) = manager_with_sessions(&["a", "b", "c"]);
        app.sessions[1].is_running = false; // b는 종료됨
        let _ = app.handle_stop_all_sessions();
        assert!(app.sessions[0].cancel_flag.load(Ordering::Relaxed));
        assert!(!app.sessions[1].cancel_flag.load(Ordering::Relaxed));
        assert!(app.sessions[2].cancel_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn rerun_failed_reruns_only_nonzero_exit_sessions() {
        let mut app = manager_with_named_sessions(&["ok", "fail"]);
        app.sessions[0].is_running = false;
        app.sessions[0].exit_code = Some(0);
        app.sessions[1].is_running = false;
        app.sessions[1].exit_code = Some(1);

        let _ = app.handle_rerun_failed_sessions();

        let ok = app.sessions.iter().find(|s| s.config_name == "ok").unwrap();
        assert!(!ok.is_running, "exit-0 session must not be rerun");
        assert_eq!(ok.exit_code, Some(0));

        let fail = app
            .sessions
            .iter()
            .find(|s| s.config_name == "fail")
            .unwrap();
        assert!(fail.is_running, "failed session must be rerun");
        assert_eq!(fail.exit_code, None);
    }

    #[test]
    fn rerun_all_reruns_every_session() {
        let mut app = manager_with_named_sessions(&["a", "b"]);
        app.sessions[0].is_running = false;
        app.sessions[0].exit_code = Some(0);
        app.sessions[1].is_running = false;
        app.sessions[1].exit_code = Some(1);

        let _ = app.handle_rerun_all_sessions();

        assert!(app.sessions.iter().all(|s| s.is_running));
    }

    #[test]
    fn rerun_failed_with_no_failures_is_noop() {
        let mut app = manager_with_named_sessions(&["a"]);
        // 실행 중 세션만 있음 (실패 없음)
        let _ = app.handle_rerun_failed_sessions();
        assert!(app.sessions[0].is_running);
        assert!(app.status_message.contains("No failed"));
    }
}
