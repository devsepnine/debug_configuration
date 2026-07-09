use crate::messages::{ConfigurationDropPosition, Message, StdinBarKey, ViewMode};
use crate::models::{
    ConfigTypeData, ConfigurationType, ExecuteMode, ExecuteModeType, KotlinLaunchMode,
    KotlinLaunchModeType, LayoutId, PackageManager, RunConfiguration, RunSession, SearchState,
    SessionStatusKind, WorkspaceTab,
};
use crate::services::{
    AppSettings, UpdateOutcome, check_latest_release, export_text, force_kill_process_tree,
    load_or_migrate_store, load_settings, register_running_pid, run_configuration_stream,
    save_settings, save_to_store, terminate_session_process, unregister_running_pid,
};
use crate::utils::{DIALOG_CANCELLED, ICON_DELETE, ICON_PLAY, ICON_REFRESH, ICON_STOP};
use crate::views::shared::icon_tooltip;
use crate::views::{
    ConfirmDeleteModalView, ConfirmUpdateModalView, EditorLoadingState, EditorSelectState,
    EnvModalView, ExportModalView, FileDialogLoadingState, ImportModalView, NodeLoadingState,
    SettingsModalView, view_configuration_editor, view_configuration_list,
    view_confirm_delete_modal, view_confirm_update_modal, view_env_modal, view_export_modal,
    view_import_modal, view_main_tabs, view_pane_layout, view_settings_modal, view_toolbar,
    view_workspace_tab_bar,
};
use crate::widgets::pane_grid;
use crate::widgets::title_bar_drag::TitleBarDragArea;
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Point, Size, Subscription, Task,
    Theme, border, event, keyboard, mouse,
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
mod confirm_delete_modal;
mod confirm_update_modal;
mod env_modal;
mod export_modal;
mod import_modal;
mod settings_modal;

use confirm_delete_modal::ConfirmDeleteModalState;
use confirm_update_modal::ConfirmUpdateModalState;
use env_modal::EnvModalState;
use export_modal::ExportModalState;
use import_modal::ImportModalState;
use settings_modal::SettingsModalState;

use chrome::{
    SessionActionKind, empty_workspace_content_style, session_action_button_style,
    session_action_icon_style, session_empty_state_style, session_list_item_container_style,
    session_list_panel_style, session_list_status_dot_style, status_bar_style,
    status_bar_top_border_style, status_bar_version_style, window_border_overlay_style,
    window_chrome_style, window_radius, workspace_content_island_style,
    workspace_content_surface_style, workspace_tab_bar_island_style,
};

/// 세션 리스트의 상태 배지 고정 폭 — 레이아웃 계산과 실제 렌더링이 공유한다.
/// 압축 라벨의 최장 형태("✕ exit 130", "12m 05s")가 10px 폰트에서 들어가는 크기
/// (기존 52px는 세 자리 exit code에서 이미 넘쳤다).
const SESSION_STATUS_BADGE_WIDTH: f32 = 64.0;

/// 커스텀 타이틀바 터치 드래그의 진행 상태.
///
/// 좌표는 모두 logical(iced가 스케일 팩터를 반영해 변환). 손가락이 물리적으로 움직일
/// 때만 터치 이벤트가 오고, 프로그램적 `move_to`는 터치 이벤트를 만들지 않으므로
/// `window_pos`를 이동마다 낙관적으로 갱신해도 피드백 루프가 생기지 않는다.
struct TouchDragState {
    /// press 시점에 손가락이 창에서 잡은 지점 (창 좌상단 기준 offset). 이동 내내 고정.
    grab: Point,
    /// 현재 창 좌상단의 화면 절대 위치. 이동마다 새 위치로 갱신한다.
    window_pos: Point,
}

#[derive(Default)]
struct FileDialogState {
    is_loading_folder: bool,
    is_loading_script_file: bool,
    is_loading_interpreter: bool,
    is_loading_kotlin_jar: bool,
    is_loading_kotlin_jdk: bool,
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
    const TYPE_ICON_WIDTH: f32 = 14.0; // 타입 아이콘 박스 폭 (type_icon BOX_SIZE)
    const ICON_NAME_GAP: f32 = 6.0; // 아이콘-이름 사이 Space
    const SCROLLBAR_WIDTH: f32 = 4.0; // 세로 스크롤바 폭

    let list_reserved_width = ITEM_AREA_PADDING
        + CONTENT_ROW_SPACING
        + SELECTION_BAR_WIDTH
        + ACTION_BUTTONS_WIDTH
        + SUMMARY_HORIZONTAL_PADDING
        + TYPE_ICON_WIDTH
        + ICON_NAME_GAP
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

/// 인앱 업데이트 적용 후 재실행 계획을 실행한다. spawn 성공 여부만 확인하며,
/// 프로세스 종료는 호출 측(`handle_update_install_completed`)이 담당한다.
/// - macOS: 새 번들을 `open -n`으로 실행 (이미 제자리에 설치된 상태)
/// - Windows: 검증된 MSI를 `msiexec /i`(full UI)로 실행 — 설치·재실행은 설치관리자가 담당
/// - Linux: 교체된 바이너리를 그대로 재실행
fn execute_relaunch(plan: &crate::services::RelaunchPlan) -> Result<(), String> {
    use crate::services::RelaunchPlan;
    match plan {
        RelaunchPlan::MacOs { app_path } => std::process::Command::new("open")
            .arg("-n")
            .arg(app_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch updated app: {e}")),
        RelaunchPlan::Windows { msi_path } => std::process::Command::new("msiexec")
            .arg("/i")
            .arg(msi_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch installer: {e}")),
        RelaunchPlan::Linux { exe_path } => std::process::Command::new(exe_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to relaunch: {e}")),
    }
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
    /// 앱 설정 모달 상태 (`Some`이면 모달 열림)
    settings_modal: Option<SettingsModalState>,
    /// 구성 내보내기 모달 상태 (`Some`이면 모달 열림)
    export_modal: Option<ExportModalState>,
    /// 구성 가져오기 모달 상태 (`Some`이면 모달 열림)
    import_modal: Option<ImportModalState>,
    /// 구성 삭제 확인 모달 상태 (`Some`이면 모달 열림)
    confirm_delete_modal: Option<ConfirmDeleteModalState>,
    /// 인앱 업데이트 확인 모달 상태 (`Some`이면 모달 열림)
    confirm_update_modal: Option<ConfirmUpdateModalState>,
    /// 설정: 구성 실행 시 Environment 라인 표시 여부
    show_environment_on_run: bool,
    /// 설정: 새 세션의 출력 버퍼 최대 라인 수
    max_output_lines: usize,
    /// 설정: 새 세션의 자동 스크롤 초기값
    default_auto_scroll: bool,
    /// 설정: 앱 시작 시 업데이트 자동 확인
    auto_check_updates: bool,
    file_dialog: FileDialogState,

    /// Node package script 캐시 (`config_id` 기준)
    node_available_scripts: HashMap<Uuid, Vec<String>>,
    node_ui: NodeUiState,
    editor_select_state: EditorSelectState,
    /// Node 프로젝트에서 발견된 `package.json` 목록 (`config_id` -> 상대 경로 리스트)
    node_available_package_jsons: HashMap<Uuid, Vec<String>>,
    /// 시스템에서 감지된 Node.js Runtime 목록 (label, path)
    available_node_runtimes: Vec<(String, String)>,
    /// 시스템에서 감지된 JDK 목록 (label, path)
    available_jdks: Vec<(String, String)>,

    /// 실행 중인 세션 목록
    sessions: Vec<RunSession>,
    /// 세션 리스트에서 hover 중인 항목 (표시용 인덱스)
    hovered_session_index: Option<usize>,
    /// 가장 최근 관측된 터미널 pane 뷰포트 (cols, rows) — 신규 세션 PTY의 초기
    /// 크기로 사용해 시작 직후 리사이즈(ConPTY 전체 리페인트)를 피한다.
    last_seen_viewport: Option<(u16, u16)>,

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
    /// 윈도우 좌상단의 logical 좌표. `WindowMoved`로 갱신하며, 터치 드래그가 창을
    /// 옮길 절대 위치를 계산하는 기준점이다.
    window_pos: Point,
    is_window_maximized: bool,
    /// 커스텀 타이틀바 터치 드래그 상태 (`Some`이면 드래그 중).
    title_bar_touch_drag: Option<TouchDragState>,
    /// 윈도우 포커스 여부 (백그라운드에서 실행이 끝났을 때만 알림)
    is_window_focused: bool,
    /// legacy: 파일 기반 시절의 마지막 작업 파일 경로. 구성 영속성은 앱 저장소
    /// (`data_dir/run_config_manager/configs.json`)로 전환되어, 이 값은 startup 시
    /// 저장소 최초 시드(migration) 용도로만 읽는다.
    last_file_path: Option<PathBuf>,
    /// 초기 구성 로드(`ConfigurationsLoaded`)가 처리되었는지 여부. 로드가 끝나기 전에는
    /// Save를 막아, 빈/부실 목록이 저장소를 덮어써 데이터를 잃는 것을 방지한다.
    configs_ready: bool,
    /// 사용 가능한 새 버전 정보. 없으면 `None`. 상태바 업데이트 버튼 표시에 사용한다.
    update_available: Option<AvailableUpdate>,
    /// 업데이트 확인이 진행 중인지. 상태바에 로딩 스피너를 표시하고
    /// 스피너 타이머 subscription을 활성화하는 데 쓴다.
    is_checking_update: bool,
    /// 인앱 업데이트(다운로드·검증·적용)가 진행 중인지. 중복 시작을 막고
    /// 상태바 버튼을 비활성 표시한다.
    is_updating: bool,
    /// 인앱 업데이트가 실패했는지. 실패 후 상태바 버튼은 릴리스 페이지 열기로
    /// 폴백해 사용자가 수동으로 내려받을 수 있게 한다.
    update_install_failed: bool,
    /// 로딩 스피너 프레임 인덱스 (확인 중 타이머 tick마다 증가).
    update_spinner_frame: usize,
}

/// 상태바에 표시할 사용 가능한 업데이트.
/// (crate 가시성: 자식 모듈 `confirm_update_modal`의 핸들러/테스트가 사용한다.)
#[derive(Debug, Clone)]
pub(crate) struct AvailableUpdate {
    /// 새 버전 (v 접두사 없는 정규화 형태).
    pub(crate) latest: String,
    /// 릴리스 페이지 URL (인앱 설치 불가/실패 시 폴백 진입점).
    pub(crate) url: String,
    /// 인앱 설치용 다운로드 정보. 구 릴리스처럼 자산이 없으면 `None`.
    pub(crate) download: Option<crate::services::UpdateDownload>,
}

/// 상태바 업데이트 확인 로딩 스피너 프레임. D2Coding(모노스페이스)에서 항상
/// 렌더되도록 ASCII만 사용한다.
const UPDATE_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

impl RunConfigManager {
    /// 새로운 애플리케이션 인스턴스 생성 및 초기화
    ///
    /// # Returns
    /// (애플리케이션 인스턴스, 초기 Task)
    /// 구성은 앱 저장소에서 로드하며, 저장소가 없으면 legacy 작업 파일에서 1회 마이그레이션한다.
    pub fn new() -> (Self, Task<Message>) {
        // 앱 설정에서 마지막 파일 경로 확인
        let settings = load_settings();
        let last_file_path = settings.last_file_path.clone();
        let configuration_split_ratio = settings.configuration_split_ratio.unwrap_or(0.30);
        let auto_check_updates = settings.auto_check_updates;

        let app = Self {
            current_view: ViewMode::Configuration,
            configurations: vec![],
            selected_config_index: None,
            status_message: String::from("Ready"),
            env_bulk_inputs: HashMap::new(),
            env_modal: None,
            settings_modal: None,
            export_modal: None,
            import_modal: None,
            confirm_delete_modal: None,
            confirm_update_modal: None,
            show_environment_on_run: settings.show_environment_on_run,
            max_output_lines: settings.max_output_lines,
            default_auto_scroll: settings.default_auto_scroll,
            auto_check_updates,
            file_dialog: FileDialogState::default(),
            node_available_scripts: HashMap::new(),
            node_ui: NodeUiState::default(),
            editor_select_state: EditorSelectState::new(),
            node_available_package_jsons: HashMap::new(),
            available_node_runtimes: vec![("Default (system)".to_string(), "node".to_string())],
            available_jdks: vec![("Default (system)".to_string(), "java".to_string())],
            sessions: vec![],
            hovered_session_index: None,
            last_seen_viewport: None,
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
            window_pos: Point::ORIGIN,
            is_window_maximized: false,
            title_bar_touch_drag: None,
            is_window_focused: true,
            last_file_path: last_file_path.clone(),
            configs_ready: false,
            update_available: None,
            // 설정이 켜진 경우에만 아래에서 자동 체크 Task를 큐잉하므로 그에 맞춰 스피너 시작.
            is_checking_update: auto_check_updates,
            is_updating: false,
            update_install_failed: false,
            update_spinner_frame: 0,
        };

        // 초기화 Task들
        let mut tasks = vec![];

        // 1. Node Runtime 감지 (백그라운드)
        tasks.push(Task::perform(
            async { crate::utils::detect_node_runtimes() },
            Message::NodeRuntimesDetected,
        ));

        // 1-1. JDK 감지 (백그라운드)
        tasks.push(Task::perform(
            async { crate::utils::detect_jdks() },
            Message::JdksDetected,
        ));

        // 2. 앱 저장소에서 구성 로드 (저장소가 없으면 legacy 작업 파일에서 1회 마이그레이션).
        //    모든 파일 I/O가 이 Task 안에서 일어나므로, Task를 폴링하지 않는 단위 테스트는
        //    실제 데이터 디렉터리를 건드리지 않는다. 손상은 ConfigurationsLoaded(Err)로 표면화.
        tasks.push(Task::perform(
            load_or_migrate_store(last_file_path),
            Message::ConfigurationsLoaded,
        ));

        // 3. 최신 버전 확인 (설정이 켜진 경우만; 백그라운드, 실패는 비치명적)
        if auto_check_updates {
            tasks.push(Task::perform(
                check_latest_release(),
                Message::UpdateCheckCompleted,
            ));
        }

        // 4. 이전 인앱 업데이트가 남긴 잔여물(.app.old-* 백업, 스테이징) 정리.
        //    파일 I/O이므로 Task 안에서 수행한다 (결과 통지는 불필요 — discard).
        tasks.push(
            Task::future(async {
                crate::services::cleanup_stale_update_artifacts();
            })
            .discard(),
        );

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
        // 오버플로 메뉴 항목이 세션 액션을 실행하면 해당 세션의 메뉴를 닫는다.
        self.close_controls_menu_on_action(&message);

        match message {
            Message::SwitchView(view_mode) => {
                self.current_view = view_mode;
                Task::none()
            }
            Message::OpenSettingsModal => self.handle_open_settings_modal(),
            Message::ConfirmSettingsModal => self.handle_confirm_settings_modal(),
            Message::CancelSettingsModal => self.handle_cancel_settings_modal(),
            Message::SettingsToggleEnvironment(value) => {
                self.handle_settings_toggle_environment(value)
            }
            Message::SettingsMaxLinesChanged(text) => self.handle_settings_max_lines_changed(text),
            Message::SettingsToggleAutoScroll(value) => {
                self.handle_settings_toggle_auto_scroll(value)
            }
            Message::SettingsToggleAutoCheckUpdates(value) => {
                self.handle_settings_toggle_auto_check_updates(value)
            }
            Message::OpenExportModal => self.handle_open_export_modal(),
            Message::ConfirmExportModal => self.handle_confirm_export_modal(),
            Message::CancelExportModal => self.handle_cancel_export_modal(),
            Message::ExportModalToggleConfig(config_id, checked) => {
                self.handle_export_modal_toggle_config(config_id, checked)
            }
            Message::ExportModalToggleAll(checked) => self.handle_export_modal_toggle_all(checked),
            Message::ConfigurationsExported(result) => self.handle_configurations_exported(result),
            Message::ImportConfigurations => self.handle_import_configurations(),
            Message::ImportFileLoaded(result) => self.handle_import_file_loaded(result),
            Message::ConfirmImportModal => self.handle_confirm_import_modal(),
            Message::CancelImportModal => self.handle_cancel_import_modal(),
            Message::ImportModalToggleConfig(config_id, checked) => {
                self.handle_import_modal_toggle_config(config_id, checked)
            }
            Message::ImportModalToggleAll(checked) => self.handle_import_modal_toggle_all(checked),
            Message::AddConfiguration
            | Message::RequestDeleteConfiguration(_)
            | Message::ConfirmDeleteConfiguration
            | Message::CancelDeleteConfiguration
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
            | Message::KotlinLaunchModeChanged(_)
            | Message::KotlinMainClassChanged(_)
            | Message::KotlinClasspathChanged(_)
            | Message::KotlinJarPathChanged(_)
            | Message::BrowseKotlinJarPath
            | Message::KotlinJarPathSelected(_)
            | Message::KotlinVmOptionsChanged(_)
            | Message::KotlinProgramArgumentsChanged(_)
            | Message::KotlinJdkChanged(_)
            | Message::BrowseKotlinJdk
            | Message::KotlinJdkPathSelected(_)
            | Message::JdksDetected(_)
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
            | Message::SaveConfigurations
            | Message::ConfigurationsSaved(_) => self.handle_configuration_messages(message),
            Message::ProcessStarted(_, _)
            | Message::OutputReceived(_, _)
            | Message::RunCompleted(_, _)
            | Message::SessionViewportResized(_, _, _)
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
            | Message::CloseActiveBar
            | Message::OpenSessionStdin(_)
            | Message::CloseSessionStdin(_)
            | Message::SessionStdinChanged(_, _)
            | Message::SessionStdinSubmitted(_)
            | Message::SessionStdinWriteCompleted(_, _, _)
            | Message::OpenStdinInActivePane
            | Message::StdinBarKeyPressed(_)
            | Message::StdinBarKeyResolved(_, _)
            | Message::SessionInterruptRequested(_)
            | Message::SessionInterruptCompleted(_, _)
            | Message::SearchNextInActivePane
            | Message::SearchPrevInActivePane
            | Message::ClearSessionOutput(_)
            | Message::ExportSessionOutput(_)
            | Message::SessionOutputExported(_)
            | Message::ToggleSessionControlsMenu(_)
            | Message::CheckForUpdates
            | Message::UpdateCheckCompleted(_)
            | Message::UpdateSpinnerTick
            | Message::SessionTimerTick
            | Message::RequestInstallUpdate
            | Message::ConfirmInstallUpdate
            | Message::CancelInstallUpdate
            | Message::UpdateInstallCompleted(_) => self.handle_session_messages(message),
            Message::AddWorkspaceTab
            | Message::JumpToWorkspace(_)
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
            | Message::CloseFocusedPane
            | Message::RunShortcut
            | Message::TogglePaneMaximize(_)
            | Message::PaneClicked(_)
            | Message::TabBarHovered(_)
            | Message::MouseReleased
            | Message::WindowOpened(_)
            | Message::WindowResized(_, _)
            | Message::WindowMaximized(_)
            | Message::WindowFocusChanged(_)
            | Message::WindowMoved(_)
            | Message::StartWindowDrag
            | Message::TitleBarTouchDragStart(_)
            | Message::TitleBarTouchDragMove(_)
            | Message::TitleBarTouchDragEnd
            | Message::ResizeWindow(_)
            | Message::MinimizeWindow
            | Message::ToggleWindowMaximize
            | Message::CloseWindow => self.handle_workspace_messages(message),
        }
    }

    fn handle_configuration_messages(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddConfiguration => self.handle_add_configuration(),
            Message::RequestDeleteConfiguration(index_opt) => {
                self.handle_request_delete_configuration(index_opt)
            }
            Message::ConfirmDeleteConfiguration => self.handle_confirm_delete_configuration(),
            Message::CancelDeleteConfiguration => self.handle_cancel_delete_configuration(),
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
            Message::KotlinLaunchModeChanged(mode_type) => {
                self.handle_kotlin_launch_mode_changed(mode_type)
            }
            Message::KotlinMainClassChanged(value) => self.handle_kotlin_main_class_changed(value),
            Message::KotlinClasspathChanged(value) => self.handle_kotlin_classpath_changed(value),
            Message::KotlinJarPathChanged(value) => self.handle_kotlin_jar_path_changed(value),
            Message::BrowseKotlinJarPath => self.handle_browse_kotlin_jar_path(),
            Message::KotlinJarPathSelected(result) => self.handle_kotlin_jar_path_selected(result),
            Message::KotlinVmOptionsChanged(value) => self.handle_kotlin_vm_options_changed(value),
            Message::KotlinProgramArgumentsChanged(value) => {
                self.handle_kotlin_program_arguments_changed(value)
            }
            Message::KotlinJdkChanged(label) => self.handle_kotlin_jdk_changed(label),
            Message::BrowseKotlinJdk => self.handle_browse_kotlin_jdk(),
            Message::KotlinJdkPathSelected(result) => self.handle_kotlin_jdk_path_selected(result),
            Message::JdksDetected(jdks) => self.handle_jdks_detected(jdks),
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
            Message::SessionViewportResized(session_id, cols, rows) => {
                self.handle_session_viewport_resized(session_id, cols, rows)
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
            // 상태 변경 없음 — 메시지 수신 자체가 재렌더를 유발해 라이브 경과시간이 갱신된다.
            Message::SessionTimerTick => Task::none(),
            Message::RequestInstallUpdate => self.handle_request_install_update(),
            Message::ConfirmInstallUpdate => self.handle_confirm_install_update(),
            Message::CancelInstallUpdate => self.handle_cancel_install_update(),
            Message::UpdateInstallCompleted(result) => self.handle_update_install_completed(result),
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
            Message::OpenSearchInActivePane => match self.search_open_target() {
                Some(session_id) => self.handle_open_session_search(session_id),
                None => Task::none(),
            },
            Message::CloseActiveBar => self.handle_close_active_bar(),
            Message::OpenSessionStdin(session_id) => self.handle_open_session_stdin(session_id),
            Message::CloseSessionStdin(session_id) => self.handle_close_session_stdin(session_id),
            Message::SessionStdinChanged(session_id, value) => {
                self.handle_session_stdin_changed(session_id, value)
            }
            Message::SessionStdinSubmitted(session_id) => {
                self.handle_session_stdin_submitted(session_id)
            }
            Message::SessionStdinWriteCompleted(session_id, line, result) => {
                self.handle_session_stdin_write_completed(session_id, line, result)
            }
            Message::OpenStdinInActivePane => match self.stdin_open_target() {
                Some(session_id) => self.handle_open_session_stdin(session_id),
                None => Task::none(),
            },
            Message::StdinBarKeyPressed(key) => self.handle_stdin_bar_key_pressed(key),
            Message::StdinBarKeyResolved(key, focused) => {
                self.handle_stdin_bar_key_resolved(key, focused)
            }
            Message::SessionInterruptRequested(session_id) => {
                self.handle_session_interrupt_requested(session_id)
            }
            Message::SessionInterruptCompleted(session_id, result) => {
                self.handle_session_interrupt_completed(session_id, result)
            }
            Message::SearchNextInActivePane => match self.search_nav_target() {
                Some(session_id) => self.handle_session_search_step(session_id, 1),
                None => Task::none(),
            },
            Message::SearchPrevInActivePane => match self.search_nav_target() {
                Some(session_id) => self.handle_session_search_step(session_id, -1),
                None => Task::none(),
            },
            Message::ClearSessionOutput(session_id) => self.handle_clear_session_output(session_id),
            Message::ExportSessionOutput(session_id) => {
                self.handle_export_session_output(session_id)
            }
            Message::SessionOutputExported(result) => self.handle_session_output_exported(result),
            Message::ToggleSessionControlsMenu(session_id) => {
                self.handle_toggle_session_controls_menu(session_id)
            }
            _ => unreachable!("non-session message routed to handle_session_messages"),
        }
    }

    fn handle_workspace_messages(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AddWorkspaceTab => self.handle_add_workspace_tab(),
            Message::JumpToWorkspace(index) => self.handle_jump_to_workspace(index),
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
            Message::CloseFocusedPane => self.handle_close_focused_pane(),
            Message::RunShortcut => self.handle_run_shortcut(),
            Message::TogglePaneMaximize(pane_id) => self.handle_toggle_pane_maximize(pane_id),
            Message::PaneClicked(pane_id) => self.handle_pane_clicked(pane_id),
            Message::TabBarHovered(tab_index) => self.handle_tab_bar_hovered(tab_index),
            Message::MouseReleased => self.handle_mouse_released(),
            Message::WindowOpened(id) => self.handle_window_opened(id),
            Message::WindowResized(id, size) => self.handle_window_resized(id, size),
            Message::WindowMaximized(is_maximized) => self.handle_window_maximized(is_maximized),
            Message::WindowFocusChanged(focused) => {
                self.is_window_focused = focused;
                // 포커스를 잃으면(시스템 다이얼로그·Alt-Tab 등) winit이 터치 취소/뗌
                // 이벤트를 만들지 않아 드래그가 걸린 채 멈출 수 있다. 그러면 이후 모든
                // WindowMoved가 무시되어 window_pos가 stale해지므로 여기서 드래그를 끝낸다.
                if !focused {
                    self.title_bar_touch_drag = None;
                    // 창 배치(크기/위치)를 blur 시점에 영속화한다. 리사이즈/이동 이벤트마다
                    // 쓰면 드래그 중 초당 수십 회 디스크 쓰기가 생기므로, "정리하고 다른 곳을
                    // 본 순간"에 저장하는 편이 싸고 충분하다 (닫기 버튼 경로도 별도 저장).
                    self.save_app_settings();
                }
                Task::none()
            }
            Message::WindowMoved(position) => self.handle_window_moved(position),
            Message::StartWindowDrag => self.handle_start_window_drag(),
            Message::TitleBarTouchDragStart(finger) => {
                self.handle_title_bar_touch_drag_start(finger)
            }
            Message::TitleBarTouchDragMove(finger) => self.handle_title_bar_touch_drag_move(finger),
            Message::TitleBarTouchDragEnd => {
                self.title_bar_touch_drag = None;
                Task::none()
            }
            Message::ResizeWindow(direction) => self.handle_resize_window(direction),
            Message::MinimizeWindow => self.handle_minimize_window(),
            Message::ToggleWindowMaximize => self.handle_toggle_window_maximize(),
            Message::CloseWindow => self.handle_close_window(),
            _ => unreachable!("non-workspace message routed to handle_workspace_messages"),
        }
    }

    fn handle_window_opened(&mut self, id: window::Id) -> Task<Message> {
        self.window_id = Some(id);
        // 최대화 상태와 함께 초기 창 위치도 확보한다 (터치 드래그 기준점). 이후엔
        // WindowMoved로 계속 갱신되지만, 첫 드래그 전에 정확한 값이 필요하다.
        let initial_position =
            window::position(id).and_then(|p| Task::done(Message::WindowMoved(p)));
        Task::batch([
            window::is_maximized(id).map(Message::WindowMaximized),
            initial_position,
        ])
    }

    /// 창 이동 반영. 터치 드래그 중에는 우리가 방금 요청한 이동이 되돌아오는 것이므로
    /// 기준 위치를 덮어쓰지 않는다(낙관적으로 이미 갱신됨). 그 외(마우스 OS 드래그,
    /// 외부 요인)에는 최신 위치로 동기화한다.
    fn handle_window_moved(&mut self, position: Point) -> Task<Message> {
        if self.title_bar_touch_drag.is_none() {
            self.window_pos = position;
        }
        Task::none()
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

    /// 터치 드래그 시작: 손가락이 잡은 지점과 현재 창 위치를 기록한다. OS 모달 이동
    /// 루프(`window::drag`)를 타지 않으므로 터치 종료 신호 부재로 인한 프리징이 없다.
    fn handle_title_bar_touch_drag_start(&mut self, finger: Point) -> Task<Message> {
        // 최대화 상태에서는 이동하지 않는다 (리사이즈와 동일 정책).
        if self.is_window_maximized {
            return Task::none();
        }
        self.title_bar_touch_drag = Some(TouchDragState {
            grab: finger,
            window_pos: self.window_pos,
        });
        Task::none()
    }

    /// 터치 드래그 이동: 손가락(창 기준)이 잡은 지점을 계속 물도록 창을 옮긴다.
    ///
    /// `finger`는 현재 창 위치(`state.window_pos`) 기준의 좌표다. 잡은 지점 `grab`이
    /// 손가락 아래에 오려면 새 창 위치 = 현재 창 위치 + (finger - grab). 손가락이 물리적
    /// 이동할 때만 이벤트가 오므로(프로그램적 move_to는 터치 이벤트를 만들지 않음) 이
    /// 점화식은 안정적이며, 창 위치를 낙관적으로 갱신해 다음 이벤트의 기준으로 삼는다.
    fn handle_title_bar_touch_drag_move(&mut self, finger: Point) -> Task<Message> {
        // 드래그 도중 최대화되면(예: 더블탭이 같은 press에서 최대화를 함께 발행) 기준
        // 위치가 무의미해진다. move_to는 창을 강제로 unmaximize하므로 여기서 중단한다.
        if self.is_window_maximized {
            self.title_bar_touch_drag = None;
            return Task::none();
        }
        let Some(drag) = self.title_bar_touch_drag.as_mut() else {
            return Task::none();
        };
        let new_pos = Point::new(
            drag.window_pos.x + (finger.x - drag.grab.x),
            drag.window_pos.y + (finger.y - drag.grab.y),
        );
        drag.window_pos = new_pos;
        self.window_pos = new_pos;
        // 위치 상태는 window_id와 무관하게 갱신하고, 실제 이동 효과만 창이 있을 때 낸다.
        self.window_id
            .map_or_else(Task::none, |id| window::move_to(id, new_pos))
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
        // 진행 중인 터치 드래그가 있으면 끝낸다 (더블탭이 드래그 시작과 최대화를
        // 같은 press에서 발행하는 경우 등 — 최대화된 창을 move_to하지 않도록).
        self.title_bar_touch_drag = None;
        self.window_id.map_or_else(Task::none, |id| {
            self.is_window_maximized = !self.is_window_maximized;
            Task::batch([
                window::toggle_maximize(id),
                window::is_maximized(id).map(Message::WindowMaximized),
            ])
        })
    }

    fn handle_close_window(&mut self) -> Task<Message> {
        // 종료 직전 창 배치를 영속화한다 (다음 실행에서 크기/위치 복원).
        self.save_app_settings();
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
        // 단위 테스트가 핸들러(설정 확인, pane 리사이즈, blur 등)를 거쳐 여기 도달하면
        // 실제 사용자 설정 파일(~/.run_config_settings.json)이 테스트 기본값으로 덮인다 —
        // 저장소 테스트 격리와 같은 원칙으로 테스트에서는 디스크에 쓰지 않는다.
        if cfg!(test) {
            return;
        }
        save_settings(&AppSettings {
            last_file_path: self.last_file_path.clone(),
            configuration_split_ratio: Some(configuration_split_ratio(&self.configuration_layout)),
            show_environment_on_run: self.show_environment_on_run,
            max_output_lines: self.max_output_lines,
            default_auto_scroll: self.default_auto_scroll,
            auto_check_updates: self.auto_check_updates,
            window_size: Some((self.window_size.width, self.window_size.height)),
            window_position: Some((self.window_pos.x, self.window_pos.y)),
        });
    }

    /// 키보드 단축키(Cmd+1~9)로 워크스페이스 탭으로 점프한다.
    ///
    /// 워크스페이스 탭은 Sessions 화면에만 존재하므로 화면 전환을 함께 수행한다.
    /// 존재하지 않는 인덱스(탭 개수보다 큰 번호)는 무시한다. 마우스 클릭 경로
    /// (`handle_tab_name_clicked`)와 달리 더블클릭 타이머·이름 편집 상태를 건드리지 않는다.
    fn handle_jump_to_workspace(&mut self, index: usize) -> Task<Message> {
        if index < self.workspace_tabs.len() {
            self.current_view = ViewMode::Sessions;
            self.selected_tab_index = index;
        }
        Task::none()
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

    /// 새 세션을 생성하며 현재 설정값(출력 라인 한도·자동 스크롤 기본)을 주입한다.
    /// 설정은 생성 시점에 고정되므로 이미 실행 중인 세션에는 소급되지 않는다.
    fn new_session(&self, config_name: String) -> RunSession {
        let mut session = RunSession::new(config_name);
        session.max_output_lines = self.max_output_lines;
        session.auto_scroll = self.default_auto_scroll;
        session
    }

    fn handle_run_configuration(&mut self, index_opt: Option<usize>) -> Task<Message> {
        // 인앱 업데이트 진행 중에는 새 실행을 막는다 — 설치 성공 시 앱이 재시작되므로
        // 그 사이 시작된 세션이 경고 없이 종료되는 것을 방지한다 (RequestInstallUpdate 가드의 짝).
        if self.is_updating {
            self.status_message = String::from("Update in progress — wait before running");
            return Task::none();
        }
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

            let session = self.new_session(config.name.clone());
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
                run_configuration_stream(
                    config,
                    session_id,
                    cancel_flag,
                    self.show_environment_on_run,
                    // 신규 세션: 최근 관측된 pane 뷰포트로 스폰해 시작 직후의 리사이즈를
                    // 없앤다 — Windows ConPTY는 리사이즈마다 전체 리페인트(빈 행 무더기)를
                    // 내보내 첫 출력이 화면 아래로 밀린다. 실측과 다르면 첫 프레임의
                    // SessionViewportResized가 보정한다.
                    self.last_seen_viewport,
                ),
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
            let session = self.new_session(config.name.clone());
            let session_id = session.id;
            let cancel_flag = session.cancel_flag.clone();

            self.sessions.push(session);
            if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index) {
                tab.open_session(session_id, aspect);
            }

            tasks.push(Task::run(
                run_configuration_stream(
                    config,
                    session_id,
                    cancel_flag,
                    self.show_environment_on_run,
                    // compound 멤버도 최근 pane 뷰포트로 스폰 (위 run 경로와 동일 근거).
                    self.last_seen_viewport,
                ),
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
                    ConfigurationType::Kotlin => ConfigTypeData::Kotlin {
                        jdk_path: None,
                        launch_mode: KotlinLaunchMode::default(),
                        vm_options: String::new(),
                        program_arguments: String::new(),
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

    fn handle_kotlin_launch_mode_changed(
        &mut self,
        mode_type: KotlinLaunchModeType,
    ) -> Task<Message> {
        // 동일 모드 재선택 시 입력값이 사라지지 않도록 변경이 있을 때만 교체.
        if let Some(kotlin) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_mut)
            && kotlin.launch_mode.mode_type() != mode_type
        {
            *kotlin.launch_mode = match mode_type {
                KotlinLaunchModeType::MainClass => KotlinLaunchMode::MainClass {
                    main_class: String::new(),
                    classpath: String::new(),
                },
                KotlinLaunchModeType::Jar => KotlinLaunchMode::Jar {
                    jar_path: String::new(),
                },
            };
        }

        Task::none()
    }

    fn handle_kotlin_main_class_changed(&mut self, value: String) -> Task<Message> {
        if let Some(main_class) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_main_class_mut)
        {
            *main_class.main_class = value;
        }

        Task::none()
    }

    fn handle_kotlin_classpath_changed(&mut self, value: String) -> Task<Message> {
        if let Some(main_class) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_main_class_mut)
        {
            *main_class.classpath = value;
        }

        Task::none()
    }

    fn handle_kotlin_jar_path_changed(&mut self, value: String) -> Task<Message> {
        if let Some(jar_path) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_jar_path_mut)
        {
            *jar_path = value;
        }

        Task::none()
    }

    fn handle_browse_kotlin_jar_path(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_kotlin_jar = true;
        pick_path_task(
            "Select JAR File",
            Some(("JAR Files", &["jar"])),
            false,
            Message::KotlinJarPathSelected,
        )
    }

    fn handle_kotlin_jar_path_selected(&mut self, result: Result<String, String>) -> Task<Message> {
        self.file_dialog.is_loading_kotlin_jar = false;

        match result {
            Ok(path) => {
                if let Some(jar_path) = self
                    .selected_type_data_mut()
                    .and_then(ConfigTypeData::kotlin_jar_path_mut)
                {
                    jar_path.clone_from(&path);
                }
                self.status_message = format!("JAR file selected: {path}");
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "JAR file selection");
            }
        }

        Task::none()
    }

    fn handle_kotlin_vm_options_changed(&mut self, value: String) -> Task<Message> {
        if let Some(kotlin) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_mut)
        {
            *kotlin.vm_options = value;
        }

        Task::none()
    }

    fn handle_kotlin_program_arguments_changed(&mut self, value: String) -> Task<Message> {
        if let Some(kotlin) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_mut)
        {
            *kotlin.program_arguments = value;
        }

        Task::none()
    }

    fn handle_kotlin_jdk_changed(&mut self, label: String) -> Task<Message> {
        let actual_path = self
            .available_jdks
            .iter()
            .find(|(jdk_label, _)| jdk_label == &label)
            .map(|(_, path)| path.clone())
            .unwrap_or(label);

        if let Some(kotlin) = self
            .selected_type_data_mut()
            .and_then(ConfigTypeData::kotlin_mut)
        {
            *kotlin.jdk_path = if actual_path == "java" {
                None
            } else {
                Some(actual_path)
            };
        }

        Task::none()
    }

    fn handle_browse_kotlin_jdk(&mut self) -> Task<Message> {
        self.file_dialog.is_loading_kotlin_jdk = true;
        // JDK는 home 디렉터리를 고른다 (executor가 bin/java를 해석).
        pick_path_task(
            "Select JDK Home",
            None,
            true,
            Message::KotlinJdkPathSelected,
        )
    }

    fn handle_kotlin_jdk_path_selected(&mut self, result: Result<String, String>) -> Task<Message> {
        self.file_dialog.is_loading_kotlin_jdk = false;

        match result {
            Ok(path) => {
                if let Some(kotlin) = self
                    .selected_type_data_mut()
                    .and_then(ConfigTypeData::kotlin_mut)
                {
                    *kotlin.jdk_path = Some(path.clone());
                }
                self.status_message = format!("JDK selected: {path}");
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "JDK selection");
            }
        }

        Task::none()
    }

    fn handle_jdks_detected(&mut self, jdks: Vec<(String, String)>) -> Task<Message> {
        self.available_jdks = jdks;
        self.editor_select_state.set_jdks(&self.available_jdks);
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
        // 로드가 (성공/실패 무관하게) 끝났음을 표시 — 이 시점부터 Save가 허용된다.
        self.configs_ready = true;
        match result {
            Ok(configs) => {
                self.configurations = configs;
                self.env_bulk_inputs.clear();
                self.node_available_scripts.clear();
                self.node_available_package_jsons.clear();
                self.env_modal = None;
                // 내보내기 모달의 선택 집합은 교체 전 구성의 id라 stale — 닫아서
                // "선택했다고 믿은 것과 다른 것을 내보내는" 사고를 막는다.
                self.export_modal = None;
                // 가져오기 모달도 닫는다 — 병합 대상이 사용자가 봤던 목록과 달라지므로.
                self.import_modal = None;
                self.status_message =
                    format!("Loaded {} configuration(s)", self.configurations.len());

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
        // 초기 로드가 끝나기 전(또는 로드 실패로 빈 상태일 때) Save를 허용하면 빈/부실
        // 목록이 저장소를 덮어써 데이터를 잃을 수 있다. 로드 완료 전에는 저장을 막는다.
        if !self.configs_ready {
            self.status_message =
                String::from("Still loading configurations; please wait before saving");
            return Task::none();
        }
        let configs = self.configurations.clone();
        self.status_message = String::from("Saving configurations...");
        Task::perform(save_to_store(configs), Message::ConfigurationsSaved)
    }

    fn handle_configurations_saved(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        match result {
            Ok(path) => {
                self.status_message = format!("Saved: {}", path.display());
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Save");
            }
        }

        Task::none()
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
            // rerun/재사용 pane: 관측된 뷰포트를 스폰 직후 재푸시해 초기 크기를 실측값으로
            // 맞춘다 (신규 pane은 아직 None — 첫 프레임 publish가 담당).
            if let Some((cols, rows)) = session.pty_viewport {
                crate::services::resize_session_pty(session_id, cols, rows);
            }
        }

        Task::none()
    }

    /// 터미널 뷰포트 변경 통지 → 세션에 기록하고 PTY에 전달 (pipe 세션은 no-op).
    fn handle_session_viewport_resized(
        &mut self,
        session_id: Uuid,
        cols: u16,
        rows: u16,
    ) -> Task<Message> {
        // 신규 세션 스폰의 초기 크기 후보 — pane들은 대개 비슷한 크기라, 이 값으로
        // 스폰하면 시작 직후의 리사이즈(ConPTY 전체 리페인트 유발)가 사라진다.
        // 세션 존재 여부와 무관하게 기록한다(어느 pane이든 유효한 실측값).
        self.last_seen_viewport = Some((cols, rows));
        if let Some(session) = self.session_by_id_mut(session_id) {
            if session.pty_viewport == Some((cols, rows)) {
                return Task::none();
            }
            session.pty_viewport = Some((cols, rows));
            crate::services::resize_session_pty(session_id, cols, rows);
        }
        Task::none()
    }

    fn handle_output_received(
        &mut self,
        session_id: Uuid,
        events: &[crate::models::OutputEvent],
    ) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            // Stop 요청 후 채널에 이미 쌓여 있던 잔여 배치는 폐기한다 — Stop의 의미
            // (즉시 중단)와 일치하고, 생산자도 cancel 시 버퍼 청크를 버린다(대칭).
            // 이게 없으면 출력 폭주 중 Stop이 수 초 늦어 보인다: 정지 표시를 만드는
            // RunCompleted가 큐에 쌓인 대형 배치들 **뒤에서** 도착하기 때문. 폐기로
            // 큐가 즉시 비면 생산자의 blocked send도 풀려 cancel 감지도 빨라진다.
            // rerun은 세션 id 스왑 + cancel_flag 새 Arc 교체라 새 실행과 무관하다.
            if session.cancel_flag.load(Ordering::Relaxed) {
                return Task::none();
            }
            // executor가 묶어 보낸 이벤트 배치 (process_output_loop의 coalescing —
            // UI 메시지 폭주 방지). Line=추가, Replace=마지막 라인 교체(라이브 진행바).
            for event in events {
                session.apply_output_event(event);
            }

            if session.auto_scroll {
                session.scroll_progress = 1.0;
            }

            // 출력이 바뀌었으니 검색 매치 캐시 갱신 — 배치당 1회 (이벤트당 금지: 라이브
            // 진행바는 초당 수십 배치라 O(버퍼) 스캔이 곱해진다). search 닫힘 시 no-op.
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
            // Stop이 이미 확정한 종료 시각은 보존한다 (지속시간 뱃지가 Stop 클릭 기준).
            if session.finished_at.is_none() {
                session.finished_at = Some(SystemTime::now());
            }
            // PID 추적 해제 + 세션에서도 제거 (Windows PID 재사용으로 인한 오인 kill 방지).
            // Stop 경로는 이미 take+해제했으므로 여기선 None이라 no-op.
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
            Ok(UpdateOutcome::Available {
                latest,
                url,
                download,
                ..
            }) => {
                self.status_message = format!("Update available: v{latest}");
                notify_update_available(&latest, &url);
                self.update_available = Some(AvailableUpdate {
                    latest,
                    url,
                    download,
                });
                // 새 업데이트를 발견하면 이전 설치 실패 상태는 리셋한다.
                self.update_install_failed = false;
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

    /// 인앱 업데이트 시작 (상태바 업데이트 버튼 클릭).
    /// 다운로드 정보가 없거나(구 릴리스) 이전 설치가 실패했으면 릴리스 페이지로 폴백한다.
    fn handle_install_update(&mut self) -> Task<Message> {
        if self.is_updating {
            return Task::none();
        }
        let Some(update) = &self.update_available else {
            return Task::none();
        };
        // 인앱 설치 불가/실패 → 브라우저 폴백 (기존 수동 설치 경로).
        if update.download.is_none() || self.update_install_failed {
            let url = update.url.clone();
            return self.handle_open_url(&url);
        }
        // 실행 중인 세션이 있으면 거부 — 업데이트 재시작이 자식 프로세스를 조용히
        // 죽이는 것을 막는다 (사용자가 직접 정리한 뒤 다시 시도).
        if self.sessions.iter().any(|session| session.is_running) {
            self.status_message = String::from("Stop running sessions before updating");
            return Task::none();
        }
        let download = update
            .download
            .clone()
            .expect("checked download.is_some() above");
        self.is_updating = true;
        self.status_message = String::from("Downloading and verifying update…");
        Task::perform(
            crate::services::download_and_apply(download),
            Message::UpdateInstallCompleted,
        )
    }

    /// 인앱 업데이트 적용 결과 처리. 성공 시 새 버전을 실행하고 이 프로세스를 종료한다.
    /// 실패하면 상태만 남기고 릴리스 페이지 폴백으로 전환한다 (기존 설치는 무손상).
    fn handle_update_install_completed(
        &mut self,
        result: Result<crate::services::RelaunchPlan, String>,
    ) -> Task<Message> {
        use crate::services::RelaunchPlan;
        self.is_updating = false;
        match result {
            Ok(plan) => {
                // 방어 재검사: 다운로드가 도는 동안 세션이 시작됐다면(가드 우회 경로 대비)
                // kill+exit로 조용히 죽이지 않는다. macOS/Linux는 새 버전이 이미 제자리에
                // 설치된 상태이므로 재클릭 유도 대신 수동 재시작을 안내하고, Windows는
                // 아직 미설치라 재시도(재다운로드)가 안전하므로 버튼을 유지한다.
                if self.sessions.iter().any(|session| session.is_running) {
                    self.status_message = match &plan {
                        RelaunchPlan::Windows { msi_path } => format!(
                            "Update downloaded — stop sessions, then run the installer: {}",
                            msi_path.display()
                        ),
                        RelaunchPlan::MacOs { .. } | RelaunchPlan::Linux { .. } => {
                            self.update_available = None;
                            String::from(
                                "Update installed — stop sessions and restart the app to finish",
                            )
                        }
                    };
                    return Task::none();
                }
                match execute_relaunch(&plan) {
                    Ok(()) => {
                        // 시그널 핸들러(main.rs)와 동일한 종료 경로: 자식 정리 후 즉시 종료.
                        // 위 가드로 실행 중 세션은 없지만, 방어적으로 정리한다.
                        crate::services::kill_all_running_processes();
                        std::process::exit(0);
                    }
                    Err(error) => {
                        // 재실행만 실패한 상태. macOS/Linux는 새 버전이 이미 설치돼 있어
                        // 재클릭 시 혼란스러운 에러(개명된 .old 번들에서 resolve 실패)만
                        // 나므로 버튼을 내리고 수동 재시작을 안내한다. Windows는 설치가
                        // 시작되지 않았으므로 MSI 위치를 안내하고 재시도를 허용한다.
                        self.status_message = match &plan {
                            RelaunchPlan::Windows { msi_path } => format!(
                                "Relaunch failed ({error}); run the installer manually: {}",
                                msi_path.display()
                            ),
                            RelaunchPlan::MacOs { .. } | RelaunchPlan::Linux { .. } => {
                                self.update_available = None;
                                format!(
                                    "Update installed, but relaunch failed ({error}) — restart the app manually"
                                )
                            }
                        };
                    }
                }
            }
            Err(error) => {
                self.status_message = format!("Update failed: {error}");
                self.update_install_failed = true;
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

    /// stdin 입력바 열기 (+입력에 포커스). 이미 열려 있으면 re-focus만.
    fn handle_open_session_stdin(&mut self, session_id: Uuid) -> Task<Message> {
        let Some(session) = self.session_by_id_mut(session_id) else {
            return Task::none();
        };
        if session.stdin_input.is_none() {
            session.stdin_input = Some(String::new());
        }
        iced::widget::operation::focus(crate::views::shared::session_stdin_input_id(session_id))
    }

    fn handle_close_session_stdin(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.stdin_input = None;
            // 드래프트가 죽었으므로 보관 드래프트가 다음 열기에서 부활하지 않게 리셋.
            session.reset_stdin_history_navigation();
        }
        Task::none()
    }

    fn handle_session_stdin_changed(&mut self, session_id: Uuid, value: String) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id)
            && session.stdin_input.is_some()
        {
            session.stdin_input = Some(value);
            // 수동 편집은 탐색을 끝낸다 — 편집된 텍스트가 새 라이브 드래프트가 되고,
            // 다음 ↑가 그것을 보관한다. (히스토리 탐색의 드래프트 교체는 on_input을
            // 거치지 않으므로 여기로 오지 않는다.)
            session.reset_stdin_history_navigation();
        }
        Task::none()
    }

    /// stdin 제출: 드래프트를 낙관적으로 비우고 전송 Task를 띄운다.
    /// 원문은 완료 메시지에 실어 보낸다 — 하드 실패 시 복원, 폴백 pipe(<1809) 성공
    /// 시 로컬 에코.
    fn handle_session_stdin_submitted(&mut self, session_id: Uuid) -> Task<Message> {
        let Some(session) = self.session_by_id_mut(session_id) else {
            return Task::none();
        };
        if !session.is_running {
            return Task::none();
        }
        let Some(draft) = session.stdin_input.as_mut() else {
            return Task::none();
        };
        let line = std::mem::take(draft);
        // 이력은 제출 시점에 기록한다 — Timeout이어도 쓰기는 결국 전달되고(순서 보존),
        // Broken 복원→재제출은 연속 중복 제거가 이중 등록을 막는다.
        session.push_stdin_history(&line);
        let sent = line.clone();
        Task::perform(
            crate::services::write_session_stdin(session_id, line),
            move |result| Message::SessionStdinWriteCompleted(session_id, sent.clone(), result),
        )
    }

    /// stdin 쓰기 결과 처리.
    /// - `Ok(true)`: 폴백 pipe(Windows <1809) 성공 — 에코가 없으므로 세션 로그에 로컬 에코.
    /// - `Ok(false)`: PTY(unix)/ConPTY(Windows) 성공 — 터미널 에코가 출력으로 돌아오므로
    ///   아무것도 안 함.
    /// - `Timeout`: 조기 신호일 뿐(쓰기는 백그라운드에서 완료됨) — 복원 금지(복원 후
    ///   재제출 = 중복 입력), 상태 메시지만.
    /// - `Broken`: 하드 실패 — 사용자가 그 사이 새로 타이핑하지 않았다면 드래프트 복원.
    fn handle_session_stdin_write_completed(
        &mut self,
        session_id: Uuid,
        line: String,
        result: Result<bool, crate::models::StdinWriteError>,
    ) -> Task<Message> {
        use crate::models::StdinWriteError;
        match result {
            Ok(needs_local_echo) => {
                if needs_local_echo && let Some(session) = self.session_by_id_mut(session_id) {
                    session.add_output_line(&line);
                    if session.search.is_some() {
                        session.refresh_search_matches();
                    }
                }
            }
            Err(StdinWriteError::Timeout) => {
                self.status_message =
                    String::from("Input pending — the process is not reading stdin yet");
            }
            Err(StdinWriteError::Broken(error)) => {
                self.status_message = format!("Input failed: {error}");
                if let Some(session) = self.session_by_id_mut(session_id)
                    && session.stdin_input.as_deref() == Some("")
                {
                    session.stdin_input = Some(line);
                    // 방어적 리셋 — 현재는 제출 시점의 push가 탐색을 이미 끝냈지만,
                    // 그 불변식에 기대지 않고 직접 대입엔 항상 리셋을 동반시킨다.
                    session.reset_stdin_history_navigation();
                }
            }
        }
        Task::none()
    }

    /// stdin 바 키(↑/↓)의 공통 진입점: 실제 포커스된 위젯 Id를 조회해 해석 메시지로
    /// 되돌린다. 구독 클로저는 self를 캡처할 수 없고, pane 포커스는 위젯 포커스와
    /// 다를 수 있어(검색 입력 포커스 등) find_focused가 권위 판정이다. 포커스된
    /// 위젯이 없으면 오퍼레이션이 Outcome::None으로 끝나 메시지가 발화되지 않는다.
    fn handle_stdin_bar_key_pressed(&mut self, key: StdinBarKey) -> Task<Message> {
        if !self.sessions.iter().any(|s| s.stdin_input.is_some()) {
            return Task::none();
        }
        iced::advanced::widget::operate(
            iced::advanced::widget::operation::focusable::find_focused(),
        )
        .map(move |focused| Message::StdinBarKeyResolved(key, focused))
    }

    /// find_focused 결과를 stdin 바로 라우팅한다. 포커스 Id가 stdin 바가 열린 어떤
    /// 세션의 입력과도 일치하지 않으면(검색창/탭 이름 편집 등) no-op — 히스토리/
    /// 인터럽트 키가 다른 입력의 복사·캐럿 조작과 충돌하지 않는 안전핀.
    fn handle_stdin_bar_key_resolved(
        &mut self,
        key: StdinBarKey,
        focused: iced::advanced::widget::Id,
    ) -> Task<Message> {
        let Some(session_id) = self
            .sessions
            .iter()
            .filter(|s| s.stdin_input.is_some())
            .map(|s| s.id)
            .find(|id| crate::views::shared::session_stdin_input_id(*id) == focused)
        else {
            return Task::none();
        };
        match key {
            StdinBarKey::HistoryOlder | StdinBarKey::HistoryNewer => {
                let older = matches!(key, StdinBarKey::HistoryOlder);
                let Some(session) = self.session_by_id_mut(session_id) else {
                    return Task::none();
                };
                if session.navigate_stdin_history(older) {
                    // 드래프트 교체 후 캐럿은 이전 오프셋에 남으므로 끝으로 보낸다.
                    iced::widget::operation::move_cursor_to_end(
                        crate::views::shared::session_stdin_input_id(session_id),
                    )
                } else {
                    Task::none()
                }
            }
            StdinBarKey::Interrupt => self.handle_session_interrupt_requested(session_id),
        }
    }

    /// ^C 버튼/Ctrl+C 키 공통 진입점 — 실행 중이 아니면 no-op (버튼은 Inactive
    /// 상태에서도 on_press가 발화하므로 여기서 가드한다).
    fn handle_session_interrupt_requested(&mut self, session_id: Uuid) -> Task<Message> {
        let running = self
            .session_by_id_mut(session_id)
            .is_some_and(|s| s.is_running);
        if !running {
            return Task::none();
        }
        Task::perform(
            crate::services::interrupt_session(session_id),
            move |result| Message::SessionInterruptCompleted(session_id, result),
        )
    }

    /// 인터럽트 결과 처리. 성공은 침묵 — ^C 에코/KeyboardInterrupt 표시는 터미널과
    /// 자식의 몫이다(에코는 전송의 책임). 실패 양쪽 모두 pid 그룹 SIGINT 폴백을
    /// 시도한다:
    /// - `Timeout`: 쓰기 경로가 막힌 상태(자식이 stdin을 안 읽어 이전 쓰기가 writer
    ///   mutex를 문 채 블록 등) — 인터럽트가 가장 필요한 순간이므로 에스컬레이션한다.
    ///   블록됐던 ETX가 뒤늦게 전달돼도 실제 터미널에서 ^C를 두 번 누른 것과
    ///   동일(SIGINT 2회)이라 무해하다.
    /// - `Broken`(PTY 경로 부재/파손 — unix pipe 폴백 세션 등).
    ///
    /// 폴백도 불가하면(windows <1809 폴백, pid 부재) 상태 메시지로 안내한다. 단,
    /// 그 사이 Stop/Remove/자연 종료가 선행된 세션엔 침묵한다 — 인터럽트 결과가
    /// 이미 무의미하고, 방금 표시된 "Session stopped" 류 메시지를 덮어쓰지 않는다.
    fn handle_session_interrupt_completed(
        &mut self,
        session_id: Uuid,
        result: Result<(), crate::models::StdinWriteError>,
    ) -> Task<Message> {
        use crate::models::StdinWriteError;
        let Err(error) = result else {
            return Task::none();
        };
        if !self
            .sessions
            .iter()
            .any(|s| s.id == session_id && s.is_running)
        {
            return Task::none();
        }
        if !self.try_signal_interrupt(session_id) {
            self.status_message = String::from(match error {
                StdinWriteError::Timeout => {
                    "Interrupt pending — the process is not reading stdin yet"
                }
                StdinWriteError::Broken(_) => "Interrupt is not available for this session",
            });
        }
        Task::none()
    }

    /// pid 그룹 SIGINT 폴백 — 실제로 신호를 보냈으면 true. Stop과 달리 pid를
    /// take하지 않는다(인터럽트 후에도 세션은 계속 산다). `is_running` 재확인이
    /// 종료 직후의 낡은 pid로 신호를 쏘는 것을 막는다.
    fn try_signal_interrupt(&self, session_id: Uuid) -> bool {
        self.sessions
            .iter()
            .find(|s| s.id == session_id && s.is_running)
            .and_then(|s| s.process_pid)
            .is_some_and(crate::services::interrupt_session_process)
    }

    /// Esc: 활성 바 하나만 닫는다. 우선순위 = 포커스 pane stdin → 포커스 pane 검색 →
    /// 임의 stdin-open 세션 → 임의 search-open 세션 (LIFO 근사 — 나중에 연 표면 먼저).
    fn handle_close_active_bar(&mut self) -> Task<Message> {
        if let Some(session_id) = self.stdin_nav_target() {
            return self.handle_close_session_stdin(session_id);
        }
        if let Some(session_id) = self.search_nav_target() {
            return self.handle_close_session_search(session_id);
        }
        Task::none()
    }

    /// 오버플로 메뉴(⋯) 토글. pane이 좁을 때 compact의 `⋯` 버튼이 보낸다.
    /// 한 번에 하나의 메뉴만 열리도록, 대상 세션을 토글하고 나머지는 모두 닫는다.
    fn handle_toggle_session_controls_menu(&mut self, session_id: Uuid) -> Task<Message> {
        let opening = !self
            .session_by_id_mut(session_id)
            .map(|session| session.controls_menu_open)
            .unwrap_or(false);
        for session in &mut self.sessions {
            session.controls_menu_open = opening && session.id == session_id;
        }
        Task::none()
    }

    /// 오버플로 메뉴를 닫아야 하는 상황을 한곳에서 처리한다. 메뉴는 `⋯` 토글로만
    /// 여닫는 지속 표면이다 — 항목 클릭(rerun/stop/search/stdin/...)은 메뉴를 닫지
    /// 않아 연속 조작이 가능하다(stop→rerun 등). 닫는 경우는 "`⋯`가 사라져 토글로
    /// 닫을 수 없게 되는" 레이아웃 변화뿐:
    /// - 리사이즈/드래그/pane·탭 닫기(Cmd+W 포함) → 모든 메뉴 닫기 (pane이
    ///   넓어지면 full 컨트롤로 전환돼 `⋯` 자체가 사라지고, 탭/pane 닫기는 세션
    ///   객체에 남은 열림 플래그가 재열기 시 고아 메뉴로 나타나는 것을 막는다)
    /// - `TogglePaneMaximize` → 해당 pane 세션의 메뉴 닫기 (maximize로 넓어진 pane도
    ///   full 컨트롤로 전환된다; 메뉴에 maximize 항목이 있어 pane→세션으로 해석)
    fn close_controls_menu_on_action(&mut self, message: &Message) {
        if matches!(
            message,
            Message::PaneGridResized(_)
                | Message::WindowResized(_, _)
                | Message::ClosePane(_)
                | Message::CloseTab(_)
                | Message::CloseFocusedPane
                | Message::PaneGridDragged(_)
        ) {
            for session in &mut self.sessions {
                session.controls_menu_open = false;
            }
            return;
        }

        if let Message::TogglePaneMaximize(pane_id) = message {
            let Some(session_id) = self
                .workspace_tabs
                .get(self.selected_tab_index)
                .and_then(|tab| tab.pane_layout.get(*pane_id))
                .and_then(|pane| pane.session_id)
            else {
                return;
            };
            if let Some(session) = self.session_by_id_mut(session_id) {
                session.controls_menu_open = false;
            }
        }
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

    /// 세션 출력 버퍼를 비운다. 실행 중인 프로세스와 이후 출력에는 영향이 없고, 화면에
    /// 쌓인 로그만 클리어한다(재현 직전 초기화 등). 검색 매치 캐시의 라인 인덱스가
    /// stale해지므로 함께 갱신한다 — `handle_rerun_session`의 clear 직후 처리와 동일.
    fn handle_clear_session_output(&mut self, session_id: Uuid) -> Task<Message> {
        if let Some(session) = self.session_by_id_mut(session_id) {
            session.clear_output();
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
        let Some(line_idx) = session
            .search
            .as_ref()
            .and_then(|s| s.matches.get(new_current).map(|m| m.line_idx))
        else {
            return Task::none();
        };
        // 인덱스가 아니라 안정 line_id를 목표로 저장한다 — 스트리밍 중 앞쪽 줄이
        // evict돼 인덱스가 밀려도 점프가 어긋나지 않는다 (뷰 빌드 시점에 행으로 해석).
        let Some(target_id) = session.output_lines.get(line_idx).map(|(id, _)| *id) else {
            return Task::none();
        };
        session.scroll_target = Some(target_id);
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

    /// 네비게이션 단축키 키 이벤트를 메시지로 매핑한다.
    ///
    /// F1 = Configurations, F2 = Sessions, 순수 Cmd+1~9 = 워크스페이스 점프.
    /// `event::listen_with`(fn 포인터라 self 캡처 불가) 클로저와 단위 테스트가 공유한다.
    /// 매핑되지 않는 키는 `None`을 반환해 다른 단축키와 충돌하지 않는다.
    fn nav_shortcut_message(
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<Message> {
        match key {
            // F1/F2는 단독 입력만 받는다(Cmd+F1=디스플레이 미러링 등 시스템 조합 제외).
            keyboard::Key::Named(keyboard::key::Named::F1) if modifiers.is_empty() => {
                Some(Message::SwitchView(ViewMode::Configuration))
            }
            keyboard::Key::Named(keyboard::key::Named::F2) if modifiers.is_empty() => {
                Some(Message::SwitchView(ViewMode::Sessions))
            }
            // 순수 Cmd+숫자만 받는다(Shift/Alt 조합은 macOS 스크린샷 Cmd+Shift+3~5 등과
            // 겹칠 수 있어 제외). 문자 범위 '1'..='9'는 Cmd+F(search_open)와 겹치지 않는다.
            keyboard::Key::Character(c)
                if modifiers.command() && !modifiers.shift() && !modifiers.alt() =>
            {
                match c.as_str().chars().next()? {
                    ch @ '1'..='9' => Some(Message::JumpToWorkspace((ch as u8 - b'1') as usize)),
                    's' => Some(Message::SaveConfigurations),
                    'r' => Some(Message::RunShortcut),
                    'w' => Some(Message::CloseFocusedPane),
                    'i' => Some(Message::OpenStdinInActivePane),
                    ',' => Some(Message::OpenSettingsModal),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// stdin 바 전용 키 매칭 (구독 클로저는 fn 포인터 — 대상 해석은
    /// `handle_stdin_bar_key_pressed`의 find_focused가 담당).
    /// - Ctrl+C: **status 게이트 없음** — win/linux의 text_input은 포커스 중 Ctrl+C를
    ///   선택 유무와 무관하게 캡처하므로(복사 arm의 capture가 무조건) Ignored 게이트를
    ///   두면 키가 영원히 도달하지 않는다. 판정은 **물리 KeyC 또는 logical "c"의
    ///   합집합** — 물리키는 한글 등 비라틴 레이아웃(logical이 "ㅊ"; text_input의
    ///   복사 판정 to_latin도 비라틴에서만 물리키를 참조)을, logical은 Dvorak처럼
    ///   라틴 문자가 물리적으로 이동한 레이아웃(to_latin이 logical을 신뢰)을
    ///   커버한다. macOS의 복사는 Cmd+C(logo)라 이 arm(control 전용, logo 배제)과
    ///   겹치지 않는다.
    /// - ↑/↓: 수식키 없음 + `Status::Ignored`만 — iced text_input은 수직 방향키를
    ///   캡처하지 않아 포커스 중에도 Ignored로 도달하고, 게이트는 미래에 방향키를
    ///   캡처하는 위젯이 생겨도 이중 발화를 막는 위생 장치다.
    fn stdin_bar_key_message(
        key: &keyboard::Key,
        physical_key: keyboard::key::Physical,
        modifiers: keyboard::Modifiers,
        status: event::Status,
    ) -> Option<Message> {
        let is_key_c = matches!(
            physical_key,
            keyboard::key::Physical::Code(keyboard::key::Code::KeyC)
        ) || matches!(key, keyboard::Key::Character(c) if c.as_str().eq_ignore_ascii_case("c"));
        if is_key_c
            && modifiers.control()
            && !modifiers.shift()
            && !modifiers.alt()
            && !modifiers.logo()
        {
            return Some(Message::StdinBarKeyPressed(StdinBarKey::Interrupt));
        }
        let arrow_eligible = modifiers.is_empty() && matches!(status, event::Status::Ignored);
        match key {
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) if arrow_eligible => {
                Some(Message::StdinBarKeyPressed(StdinBarKey::HistoryOlder))
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) if arrow_eligible => {
                Some(Message::StdinBarKeyPressed(StdinBarKey::HistoryNewer))
            }
            _ => None,
        }
    }

    /// Cmd+F가 검색을 "열" 세션. 포커스한(마지막으로 클릭/연) pane을 최우선으로 —
    /// 이미 검색이 열려 있으면 `handle_open_session_search`가 입력에 re-focus만 한다.
    /// 포커스가 없으면 이미 검색바가 열린 세션, 그래도 없으면 탭의 첫 세션 pane.
    /// "열기"는 보고 있는 pane을 대상으로 해야 하므로 포커스가 검색 열림 여부보다 우선한다.
    fn search_open_target(&self) -> Option<Uuid> {
        let tab = self.workspace_tabs.get(self.selected_tab_index)?;
        if let Some(focused) = tab.focused_session() {
            return Some(focused);
        }
        self.first_session_with_search(tab).or_else(|| {
            tab.layout_tree
                .collect_leaves()
                .into_iter()
                .find_map(|(_, pane)| pane.session_id)
        })
    }

    /// ESC/다음/이전이 대상으로 삼을 세션. 이 단축키들은 검색이 열린 pane에서만 의미가
    /// 있다(구독 자체가 "검색 열림"을 조건으로 활성화). 따라서 포커스 pane이 검색 중이면
    /// 그것을, 아니면 검색이 열린 다른 pane을 고른다 — 포커스를 다른 pane으로 옮겨도
    /// 이미 열린 검색을 계속 조작/닫을 수 있게 하기 위함(포커스만 우선하면 회귀).
    fn search_nav_target(&self) -> Option<Uuid> {
        let tab = self.workspace_tabs.get(self.selected_tab_index)?;
        if let Some(focused) = tab.focused_session()
            && self.session_is_searching(focused)
        {
            return Some(focused);
        }
        self.first_session_with_search(tab)
    }

    /// 활성 탭의 leaf 순서로 검색바가 열린 첫 세션.
    fn first_session_with_search(&self, tab: &WorkspaceTab) -> Option<Uuid> {
        tab.layout_tree
            .collect_leaves()
            .into_iter()
            .filter_map(|(_, pane)| pane.session_id)
            .find(|&id| self.session_is_searching(id))
    }

    /// 해당 세션에 검색바가 열려 있는지.
    fn session_is_searching(&self, session_id: Uuid) -> bool {
        self.sessions
            .iter()
            .any(|s| s.id == session_id && s.search.is_some())
    }

    /// Cmd+I가 stdin 바를 "열" 세션 — 포커스 pane 최우선(search_open_target 미러).
    /// Sessions 뷰가 아니면 None (Configuration 뷰에서 발화 방지).
    fn stdin_open_target(&self) -> Option<Uuid> {
        if !matches!(self.current_view, ViewMode::Sessions) {
            return None;
        }
        let tab = self.workspace_tabs.get(self.selected_tab_index)?;
        if let Some(focused) = tab.focused_session() {
            return Some(focused);
        }
        self.first_session_with_stdin(tab).or_else(|| {
            tab.layout_tree
                .collect_leaves()
                .into_iter()
                .find_map(|(_, pane)| pane.session_id)
        })
    }

    /// Esc가 닫기 대상으로 삼을 stdin 세션 — 포커스 pane이 stdin 열림이면 그것,
    /// 아니면 stdin이 열린 다른 pane (search_nav_target 미러).
    fn stdin_nav_target(&self) -> Option<Uuid> {
        let tab = self.workspace_tabs.get(self.selected_tab_index)?;
        if let Some(focused) = tab.focused_session()
            && self.session_has_stdin_open(focused)
        {
            return Some(focused);
        }
        self.first_session_with_stdin(tab)
    }

    /// 활성 탭의 leaf 순서로 stdin 바가 열린 첫 세션.
    fn first_session_with_stdin(&self, tab: &WorkspaceTab) -> Option<Uuid> {
        tab.layout_tree
            .collect_leaves()
            .into_iter()
            .filter_map(|(_, pane)| pane.session_id)
            .find(|&id| self.session_has_stdin_open(id))
    }

    /// 해당 세션에 stdin 입력바가 열려 있는지.
    fn session_has_stdin_open(&self, session_id: Uuid) -> bool {
        self.sessions
            .iter()
            .any(|s| s.id == session_id && s.stdin_input.is_some())
    }

    /// Pane 클릭 → 그 pane의 세션을 활성 탭의 포커스로 기록.
    /// 빈 pane 클릭은 무시(기존 포커스 유지)해, 빈 영역을 눌렀다고 검색 대상을 잃지 않게 한다.
    /// 같은 프레임에서 ClosePane이 먼저 처리됐다면 `session_at`이 무효화된 핸들에 대해
    /// `None`을 돌려주므로(레이아웃 rebuild → 새 State → HashMap miss) 자연히 no-op이 된다.
    fn handle_pane_clicked(&mut self, pane: pane_grid::Pane) -> Task<Message> {
        if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index)
            && let Some(session_id) = tab.session_at(pane)
        {
            tab.focus_session(session_id);
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
        // 재실행은 기존 세션 객체를 재사용하므로 max_output_lines/auto_scroll은 생성 시점 값을
        // 유지한다(설정 변경 후 재실행해도 소급 적용 안 됨 — "새 세션부터" 정책과 일치).
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
                // rerun은 pane을 재사용하므로 마지막 관측 뷰포트를 초기 PTY 크기로 넘긴다.
                let initial_viewport = session.pty_viewport;

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
                    run_configuration_stream(
                        config,
                        new_id,
                        cancel_flag,
                        self.show_environment_on_run,
                        initial_viewport,
                    ),
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
            // Stop은 파이프라인 왕복(취소 감지→SIGTERM→exit→RunCompleted)을 기다리지
            // 않는다: 신호를 지금 직접 보내고 UI 상태도 즉시 확정한다 — 출력 폭주 중에도
            // rerun과 같은 즉각성. 협조 경로는 그대로 뒤따라와 SIGKILL 에스컬레이션을
            // 보장하고(중복 신호는 무해), 늦게 도착하는 RunCompleted가 exit code를 채운다.
            if let Some(pid) = session.process_pid.take() {
                terminate_session_process(pid);
                unregister_running_pid(pid);
            }
            session.is_running = false;
            session.finished_at = Some(SystemTime::now());
            self.status_message = String::from("Session stopped");
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

    /// Cmd+W: 포커스된 세션 pane을 워크스페이스에서 숨긴다 (`ClosePane`과 동일 효과 —
    /// 세션 자체는 좌측 목록에 남고 프로세스도 유지된다). Sessions 뷰에서만 동작.
    fn handle_close_focused_pane(&mut self) -> Task<Message> {
        if !matches!(self.current_view, ViewMode::Sessions) {
            return Task::none();
        }
        if let Some(tab) = self.workspace_tabs.get_mut(self.selected_tab_index)
            && let Some(focused) = tab.focused_session()
        {
            tab.remove_session(focused);
            self.status_message = String::from("Session hidden from workspace");
        }
        Task::none()
    }

    /// Cmd+R: 컨텍스트 실행. Sessions 뷰에서 포커스된 세션이 있으면 그 세션을 재실행하고,
    /// 그 외에는 Configurations 뷰의 선택된 구성을 실행한다 (Run 버튼과 동일 경로).
    fn handle_run_shortcut(&mut self) -> Task<Message> {
        if matches!(self.current_view, ViewMode::Sessions)
            && let Some(focused) = self
                .workspace_tabs
                .get(self.selected_tab_index)
                .and_then(|tab| tab.focused_session())
        {
            return self.handle_rerun_session(focused);
        }
        self.handle_run_configuration(None)
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

        // 인앱 업데이트 진행 중에는 버튼을 비활성(on_press 없음)으로 표시해
        // 중복 클릭을 view 단계에서도 차단한다 (핸들러 가드와 이중 방어).
        if self.is_updating {
            return button(text("updating…").size(12))
                .padding([1, 6])
                .style(move |theme: &Theme, status| status_bar_version_style(theme, status, true))
                .into();
        }

        let (label, on_press) = match &self.update_available {
            // 인앱 설치 가능(자산+서명 존재, 미실패) → 클릭 시 확인 모달을 거쳐 설치.
            // 불가/실패 시에는 RequestInstallUpdate가 릴리스 페이지 열기로 폴백한다.
            Some(update) => (
                format!("v{current} → v{latest} ⬆", latest = update.latest),
                Message::RequestInstallUpdate,
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

        // 마우스는 기존 OS 드래그(window::drag), 터치는 수동 이동(window::move_to)으로
        // 분기한다. Windows 터치에서 OS 모달 이동 루프가 프리징하는 문제를 피하기 위함.
        TitleBarDragArea::new(
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
        .on_mouse_press(Message::StartWindowDrag)
        .on_double(Message::ToggleWindowMaximize)
        .on_touch_start(Message::TitleBarTouchDragStart)
        .on_touch_move(Message::TitleBarTouchDragMove)
        .on_touch_end(Message::TitleBarTouchDragEnd)
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

        // macOS는 grip을 깔지 않는다. winit의 `drag_resize_window`이 macOS에서
        // `NotSupported`라 grip을 잡아도 리사이즈가 시작되지 않고, resizable NSWindow의
        // 네이티브 가장자리 리사이즈가 이미 동작한다. grip을 두면 커서만 바뀌고
        // 동작하지 않는 5px 죽은 영역 + 네이티브 리사이즈 방해만 된다.
        // Windows/Linux는 `drag_resize_window`이 동작하므로 grip을 유지한다.
        if cfg!(target_os = "macos") || self.is_window_maximized {
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

        // 상태 배지 (고정 폭 — 압축 라벨 사용: 실행 중엔 라이브 경과시간, 종료 후엔
        // 짧은 결과. 소요시간까지 담은 풀 라벨은 pane 타이틀이 표시한다.)
        // Windows 크래시 코드처럼 거대한 exit code("✕ exit -1073741819")도 고정 폭을
        // 넘지 않도록 표시 문자 수를 배지 폭에 맞춰 자른다 (no-wrap이라 넘치면 이름을 침범).
        let is_failed = matches!(session.status_kind(), SessionStatusKind::Failed(_));
        let badge_text = crate::utils::truncate_text(&session.status_badge_label_compact(), 11);
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
                                            kotlin_jar: self.file_dialog.is_loading_kotlin_jar,
                                            kotlin_jdk: self.file_dialog.is_loading_kotlin_jdk,
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
                                    &self.available_jdks,
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
                current_tab.focused_session(),
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

    /// 오버레이 모달(env/settings/export/import/삭제확인/업데이트확인) 중 하나라도 열려 있는지.
    /// 전역 키보드 단축키를 비활성화해 backdrop 뒤 UI로 입력이 새는 것을 막는 가드.
    fn any_modal_open(&self) -> bool {
        self.env_modal.is_some()
            || self.settings_modal.is_some()
            || self.export_modal.is_some()
            || self.import_modal.is_some()
            || self.confirm_delete_modal.is_some()
            || self.confirm_update_modal.is_some()
    }

    /// 모든 오버레이 모달을 닫는다. 각 모달의 open 핸들러가 "한 번에 하나의 모달만"
    /// 계약을 지키기 위해 자신을 열기 직전에 호출한다(보통 opaque backdrop이 막지만
    /// 방어적). 새 모달 추가 시 `any_modal_open`과 이 함수만 갱신하면 된다.
    fn close_all_modals(&mut self) {
        self.env_modal = None;
        self.settings_modal = None;
        self.export_modal = None;
        self.import_modal = None;
        self.confirm_delete_modal = None;
        self.confirm_update_modal = None;
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
        // 모달이 하나라도 열려 있으면 비활성 — Tab이 backdrop 뒤 에디터 input으로
        // focus를 옮겨 이후 타이핑이 보이지 않는 input에 들어가는 것을 막는다.
        let editor_focus_subscription = if matches!(self.current_view, ViewMode::Configuration)
            && self.configuration_ui.editor_focus_area_active
            && !self.any_modal_open()
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

        // 설정 모달: Esc로 닫기 (필드가 적어 Tab 트랩은 불필요).
        let settings_modal_keyboard_subscription = if self.settings_modal.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelSettingsModal),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 내보내기 모달: Esc로 닫기 (settings 모달과 동일 — 체크박스뿐이라 Tab 트랩 불필요).
        let export_modal_keyboard_subscription = if self.export_modal.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelExportModal),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 가져오기 모달: Esc로 닫기 (export 모달과 동일).
        let import_modal_keyboard_subscription = if self.import_modal.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelImportModal),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 삭제 확인 모달: Esc로 취소 (다른 모달과 동일 — 버튼 2개뿐이라 Tab 트랩 불필요).
        let confirm_delete_keyboard_subscription = if self.confirm_delete_modal.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelDeleteConfiguration),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // 업데이트 확인 모달: Esc로 취소 (삭제 확인 모달과 동일).
        let confirm_update_keyboard_subscription = if self.confirm_update_modal.is_some() {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CancelInstallUpdate),
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
            // 창 위치 추적 (터치 드래그가 이동할 절대 위치의 기준). 마우스 OS 드래그·
            // 프로그램적 move_to 모두 WM_WINDOWPOSCHANGED → Moved로 도착한다.
            Event::Window(window::Event::Moved(position)) => Some(Message::WindowMoved(position)),
            _ => None,
        });

        // 세션 화면에서 Ctrl/Cmd+F = 검색 열기. 모달이 열려 있으면 비활성화. 대상
        // 세션은 핸들러가 해석한다(클로저는 fn 포인터라 self 접근 불가).
        let sessions_active =
            matches!(self.current_view, ViewMode::Sessions) && !self.any_modal_open();
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
        // Esc = 활성 바(stdin 우선 → 검색) 하나 닫기. 단일 구독·단일 메시지로 두고
        // 우선순위는 핸들러(handle_close_active_bar)가 정한다 — 바 종류별 Esc 구독을
        // 병렬로 두면 한 번의 Esc에 둘 다 닫히는 이중 발화가 생긴다.
        // (의도적으로 status 게이트 없음 — 포커스된 input 안에서도 Esc가 동작해야 한다)
        let bar_close_subscription = if sessions_active
            && self
                .sessions
                .iter()
                .any(|s| s.search.is_some() || s.stdin_input.is_some())
        {
            event::listen_with(|event, _status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::CloseActiveBar),
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
        // 동일하게 핸들러가 active pane에서 해석).
        // Status::Ignored 게이트: stdin 입력은 on_submit이 있어 Enter를 **capture**하고,
        // 검색 입력은 on_submit이 없어 Ignored로 흐른다 — 양쪽 바가 동시에 열려 있어도
        // stdin 타이핑 중 Enter가 검색 nav로 이중 발화하지 않는다.
        let search_nav_active = sessions_active
            && self.tab_ui.editing_tab_name.is_none()
            && self.sessions.iter().any(|s| s.search.is_some());
        let search_nav_subscription = if search_nav_active {
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Enter),
                    modifiers,
                    ..
                }) if matches!(status, event::Status::Ignored) => {
                    Some(Self::search_nav_message(modifiers.shift()))
                }
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // stdin 바 키(↑/↓ 히스토리, Ctrl+C 인터럽트): 어떤 세션이든 stdin 바가 열려
        // 있을 때만 활성. 어느 바가 대상인지는 핸들러의 find_focused 조회가 판정한다.
        let stdin_bar_keys_active =
            sessions_active && self.sessions.iter().any(|s| s.stdin_input.is_some());
        let stdin_bar_key_subscription = if stdin_bar_keys_active {
            event::listen_with(|event, status, _id| match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    physical_key,
                    modifiers,
                    ..
                }) => Self::stdin_bar_key_message(&key, physical_key, modifiers, status),
                _ => None,
            })
        } else {
            Subscription::none()
        };

        // F1/F2 = 화면 전환(Configurations/Sessions), Cmd+1~9 = 해당 워크스페이스로 점프.
        // 모달이 열려 있거나 탭 이름 편집 중이면 비활성화해 입력/모달 작업을 보호한다.
        // 키→메시지 매핑은 nav_shortcut_message로 추출해 단위 테스트와 공유한다.
        let nav_shortcut_subscription =
            if !self.any_modal_open() && self.tab_ui.editing_tab_name.is_none() {
                event::listen_with(|event, _status, _id| match event {
                    Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                        Self::nav_shortcut_message(&key, modifiers)
                    }
                    _ => None,
                })
            } else {
                Subscription::none()
            };

        // 실행 중 세션이 있을 때만 라이브 경과시간 tick(1초)을 발행한다.
        let session_timer_subscription = if self.sessions.iter().any(|s| s.is_running) {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::SessionTimerTick)
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
            settings_modal_keyboard_subscription,
            export_modal_keyboard_subscription,
            import_modal_keyboard_subscription,
            confirm_delete_keyboard_subscription,
            confirm_update_keyboard_subscription,
            tab_name_edit_subscription,
            window_focus_subscription,
            search_open_subscription,
            bar_close_subscription,
            search_nav_subscription,
            stdin_bar_key_subscription,
            nav_shortcut_subscription,
            session_timer_subscription,
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

        if let Some(modal) = self.settings_modal.as_ref() {
            let props = SettingsModalView {
                show_environment: modal.show_environment,
                max_output_lines_text: &modal.max_output_lines_text,
                default_auto_scroll: modal.default_auto_scroll,
                auto_check_updates: modal.auto_check_updates,
            };
            layers = layers.push(view_settings_modal(props));
        }

        if let Some(modal) = self.export_modal.as_ref() {
            let props = ExportModalView {
                configurations: &self.configurations,
                selected: &modal.selected,
            };
            layers = layers.push(view_export_modal(props));
        }

        if let Some(modal) = self.import_modal.as_ref() {
            let props = ImportModalView {
                source: &modal.source_path,
                configs: &modal.configs,
                selected: &modal.selected,
                existing: &self.configurations,
            };
            layers = layers.push(view_import_modal(props));
        }

        if let Some(modal) = self.confirm_delete_modal.as_ref() {
            let props = ConfirmDeleteModalView {
                name: &modal.name,
                referencing_compounds: &modal.referencing_compounds,
            };
            layers = layers.push(view_confirm_delete_modal(props));
        }

        if let Some(modal) = self.confirm_update_modal.as_ref() {
            let props = ConfirmUpdateModalView {
                current: crate::services::CURRENT_VERSION,
                latest: &modal.latest,
                // 열림 이후의 세션 변화도 반영되도록 매 프레임 현재 상태로 계산한다
                // (열림 시점 스냅샷은 세션을 멈춰도 버튼이 계속 죽어 있게 된다).
                running_sessions: self.sessions.iter().filter(|s| s.is_running).count(),
            };
            layers = layers.push(view_confirm_update_modal(props));
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
                // executor의 공용 트리 킬로 위임 — 과거 인라인 killpg는 자기그룹
                // 사살 가드(signal_process_group의 getpgid 검사)가 없어, pid==pgid
                // 전제가 깨지면 종료 시 앱을 띄운 터미널 그룹까지 죽일 수 있었다.
                force_kill_process_tree(pid);
                eprintln!(
                    "[Shutdown] Force killed PID {} ({})",
                    pid, session.config_name
                );
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
    fn save_is_blocked_until_initial_load_completes() {
        // 초기 로드 전에는 Save가 no-op이어야 한다 (빈 목록이 저장소를 덮어쓰는 것 방지).
        // new()의 로드 Task는 테스트에서 폴링되지 않으므로 configs_ready=false로 시작한다.
        let (mut app, _task) = RunConfigManager::new();
        assert!(!app.configs_ready);
        let _ = app.handle_save_configurations();
        assert!(
            app.status_message.contains("loading"),
            "expected a still-loading message, got: {}",
            app.status_message
        );

        // 로드가 (성공/실패 무관하게) 처리되면 ready가 되어 Save가 열린다.
        let _ = app.handle_configurations_loaded(Err(String::from("boom")));
        assert!(app.configs_ready);
        let _ = app.handle_save_configurations();
        assert!(app.status_message.contains("Saving"));
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

    fn manager_with_workspaces(count: usize) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.workspace_tabs = (1..=count)
            .map(|n| WorkspaceTab::empty(format!("Workspace {n}")))
            .collect();
        app.selected_tab_index = 0;
        app
    }

    #[test]
    fn jump_to_workspace_switches_view_and_selects_tab() {
        // Cmd+3 상당: Configs 화면에 있어도 Sessions로 전환하며 3번째 탭(index 2)을 선택
        let mut app = manager_with_workspaces(3);
        app.current_view = ViewMode::Configuration;

        let _ = app.handle_jump_to_workspace(2);

        assert_eq!(app.current_view, ViewMode::Sessions);
        assert_eq!(app.selected_tab_index, 2);
    }

    #[test]
    fn jump_to_workspace_out_of_range_is_noop() {
        // 존재하지 않는 번호(탭 2개인데 index 5)는 화면·선택을 바꾸지 않는다
        let mut app = manager_with_workspaces(2);
        app.current_view = ViewMode::Configuration;
        app.selected_tab_index = 1;

        let _ = app.handle_jump_to_workspace(5);

        assert_eq!(app.current_view, ViewMode::Configuration);
        assert_eq!(app.selected_tab_index, 1);
    }

    #[test]
    fn jump_to_workspace_from_sessions_only_changes_tab() {
        // 이미 Sessions 화면이면 화면은 유지하고 탭만 바뀐다
        let mut app = manager_with_workspaces(3);
        app.current_view = ViewMode::Sessions;
        app.selected_tab_index = 0;

        let _ = app.handle_jump_to_workspace(2);

        assert_eq!(app.current_view, ViewMode::Sessions);
        assert_eq!(app.selected_tab_index, 2);
    }

    #[test]
    fn nav_shortcut_f1_f2_switch_views() {
        use keyboard::key::Named;
        assert!(matches!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Named(Named::F1),
                keyboard::Modifiers::empty()
            ),
            Some(Message::SwitchView(ViewMode::Configuration))
        ));
        assert!(matches!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Named(Named::F2),
                keyboard::Modifiers::empty()
            ),
            Some(Message::SwitchView(ViewMode::Sessions))
        ));
    }

    #[test]
    fn nav_shortcut_cmd_digits_jump_to_workspace() {
        // Cmd+1 → index 0, Cmd+9 → index 8
        assert!(matches!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("1".into()),
                keyboard::Modifiers::COMMAND
            ),
            Some(Message::JumpToWorkspace(0))
        ));
        assert!(matches!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("9".into()),
                keyboard::Modifiers::COMMAND
            ),
            Some(Message::JumpToWorkspace(8))
        ));
    }

    #[test]
    fn nav_shortcut_rejects_cmd_zero_and_modifier_combos() {
        // Cmd+0: 대응 워크스페이스 없음
        assert!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("0".into()),
                keyboard::Modifiers::COMMAND
            )
            .is_none()
        );
        // Cmd 없이 "1": 텍스트 입력과 충돌하지 않도록 무시
        assert!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("1".into()),
                keyboard::Modifiers::empty()
            )
            .is_none()
        );
        // Cmd+Shift+1: macOS 스크린샷 등 시스템 단축키와 충돌 방지로 무시
        assert!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("1".into()),
                keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT
            )
            .is_none()
        );
        // Cmd+Alt+1: 마찬가지로 순수 Cmd+숫자가 아니므로 무시
        assert!(
            RunConfigManager::nav_shortcut_message(
                &keyboard::Key::Character("1".into()),
                keyboard::Modifiers::COMMAND | keyboard::Modifiers::ALT
            )
            .is_none()
        );
    }

    /// Cmd+<글자> 매핑 헬퍼 — 순수 Cmd일 때의 메시지를 얻는다.
    fn cmd_shortcut(ch: &str, modifiers: keyboard::Modifiers) -> Option<Message> {
        RunConfigManager::nav_shortcut_message(&keyboard::Key::Character(ch.into()), modifiers)
    }

    #[test]
    fn nav_shortcut_cmd_letters_map_to_actions() {
        use keyboard::Modifiers;
        assert!(matches!(
            cmd_shortcut("s", Modifiers::COMMAND),
            Some(Message::SaveConfigurations)
        ));
        assert!(matches!(
            cmd_shortcut("r", Modifiers::COMMAND),
            Some(Message::RunShortcut)
        ));
        assert!(matches!(
            cmd_shortcut("w", Modifiers::COMMAND),
            Some(Message::CloseFocusedPane)
        ));
        assert!(matches!(
            cmd_shortcut(",", Modifiers::COMMAND),
            Some(Message::OpenSettingsModal)
        ));
        assert!(matches!(
            cmd_shortcut("i", Modifiers::COMMAND),
            Some(Message::OpenStdinInActivePane)
        ));
        // Shift/Alt 조합과 수식키 없는 입력은 무시 (기존 정책과 동일).
        for ch in ["s", "r", "w", ",", "i"] {
            assert!(cmd_shortcut(ch, Modifiers::COMMAND | Modifiers::SHIFT).is_none());
            assert!(cmd_shortcut(ch, Modifiers::COMMAND | Modifiers::ALT).is_none());
            assert!(cmd_shortcut(ch, Modifiers::empty()).is_none());
        }
    }

    // ---- stdin 입력바 ----

    #[test]
    fn stdin_open_close_and_draft_lifecycle() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];

        let _ = app.handle_open_session_stdin(sid);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("")
        );
        let _ = app.handle_session_stdin_changed(sid, String::from("hello"));
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("hello")
        );
        // 재열기(이미 열림)는 드래프트를 보존한다 (re-focus만).
        let _ = app.handle_open_session_stdin(sid);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("hello")
        );
        let _ = app.handle_close_session_stdin(sid);
        assert!(app.session_by_id_mut(sid).unwrap().stdin_input.is_none());
    }

    #[test]
    fn stdin_draft_survives_rerun() {
        // rerun은 세션 객체를 재사용하며 stdin_input을 건드리지 않는다 (회귀 방지).
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        let _ = app.handle_open_session_stdin(sid);
        let _ = app.handle_session_stdin_changed(sid, String::from("typed"));

        let _ = app.handle_rerun_session(sid);

        // rerun이 세션 id를 스왑하므로 새 id로 조회한다.
        let session = app
            .sessions
            .iter()
            .find(|s| s.config_name == "a")
            .expect("session survives rerun");
        assert_eq!(session.stdin_input.as_deref(), Some("typed"));
    }

    #[test]
    fn stdin_submit_pushes_history() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);

        let _ = app.handle_session_stdin_changed(sid, String::from("foo"));
        let _ = app.handle_session_stdin_submitted(sid);
        // 빈 Enter는 전송되지만 이력에는 남지 않는다.
        let _ = app.handle_session_stdin_submitted(sid);

        let session = app.session_by_id_mut(sid).unwrap();
        assert_eq!(session.stdin_history, vec!["foo"]);
        assert_eq!(session.stdin_input.as_deref(), Some(""));
    }

    #[test]
    fn stdin_broken_restore_then_resubmit_does_not_duplicate_history() {
        use crate::models::StdinWriteError;
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);

        let _ = app.handle_session_stdin_changed(sid, String::from("x"));
        let _ = app.handle_session_stdin_submitted(sid);
        let _ = app.handle_session_stdin_write_completed(
            sid,
            String::from("x"),
            Err(StdinWriteError::Broken(String::from("EPIPE"))),
        );
        // 복원된 드래프트를 그대로 재제출 → 연속 중복 제거로 이력은 한 번만.
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("x")
        );
        let _ = app.handle_session_stdin_submitted(sid);
        assert_eq!(app.session_by_id_mut(sid).unwrap().stdin_history, vec!["x"]);
    }

    #[test]
    fn stdin_history_survives_rerun() {
        // rerun은 세션 객체를 재사용하며 stdin_history를 건드리지 않는다
        // (`stdin_draft_survives_rerun`과 동형의 회귀 방지).
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);
        let _ = app.handle_session_stdin_changed(sid, String::from("cmd"));
        let _ = app.handle_session_stdin_submitted(sid);

        let _ = app.handle_rerun_session(sid);

        let session = app
            .sessions
            .iter()
            .find(|s| s.config_name == "a")
            .expect("session survives rerun");
        assert_eq!(session.stdin_history, vec!["cmd"]);
    }

    #[test]
    fn stdin_bar_key_message_maps_arrows_only_when_ignored() {
        use keyboard::key::{Code, Named, Physical};
        let up = keyboard::Key::Named(Named::ArrowUp);
        let down = keyboard::Key::Named(Named::ArrowDown);
        let phys_up = Physical::Code(Code::ArrowUp);
        let phys_down = Physical::Code(Code::ArrowDown);
        assert!(matches!(
            RunConfigManager::stdin_bar_key_message(
                &up,
                phys_up,
                keyboard::Modifiers::empty(),
                event::Status::Ignored
            ),
            Some(Message::StdinBarKeyPressed(StdinBarKey::HistoryOlder))
        ));
        assert!(matches!(
            RunConfigManager::stdin_bar_key_message(
                &down,
                phys_down,
                keyboard::Modifiers::empty(),
                event::Status::Ignored
            ),
            Some(Message::StdinBarKeyPressed(StdinBarKey::HistoryNewer))
        ));
        // 위젯이 캡처한 방향키와 수식키 조합은 무시.
        assert!(
            RunConfigManager::stdin_bar_key_message(
                &up,
                phys_up,
                keyboard::Modifiers::empty(),
                event::Status::Captured
            )
            .is_none()
        );
        assert!(
            RunConfigManager::stdin_bar_key_message(
                &up,
                phys_up,
                keyboard::Modifiers::SHIFT,
                event::Status::Ignored
            )
            .is_none()
        );
    }

    #[test]
    fn stdin_bar_key_message_maps_ctrl_c_regardless_of_status() {
        use keyboard::key::{Code, Physical};
        let ctrl = keyboard::Modifiers::CTRL;
        let phys_c = Physical::Code(Code::KeyC);
        let key_c = keyboard::Key::Character("c".into());
        // 캡처 상태에서도 발화해야 한다 — win/linux text_input은 포커스 중 Ctrl+C를
        // 무조건 캡처하므로 Ignored 게이트가 있으면 이 키는 영원히 도달하지 않는다.
        assert!(matches!(
            RunConfigManager::stdin_bar_key_message(&key_c, phys_c, ctrl, event::Status::Captured),
            Some(Message::StdinBarKeyPressed(StdinBarKey::Interrupt))
        ));
        // 한글 레이아웃: logical key가 "ㅊ"이어도 물리 KeyC로 판정한다.
        let key_hangul = keyboard::Key::Character("ㅊ".into());
        assert!(matches!(
            RunConfigManager::stdin_bar_key_message(
                &key_hangul,
                phys_c,
                ctrl,
                event::Status::Ignored
            ),
            Some(Message::StdinBarKeyPressed(StdinBarKey::Interrupt))
        ));
        // Dvorak류: 라틴 문자가 물리적으로 이동한 레이아웃은 logical "c"로 판정한다
        // (text_input의 복사도 같은 키에서 발화 — 물리 위치가 아닌 문자 기준).
        let phys_j = Physical::Code(Code::KeyJ);
        assert!(matches!(
            RunConfigManager::stdin_bar_key_message(&key_c, phys_j, ctrl, event::Status::Ignored),
            Some(Message::StdinBarKeyPressed(StdinBarKey::Interrupt))
        ));
        // 수식키 배제: Ctrl 없음 / +Shift / +Alt / +Logo(macOS Cmd+C 복사 보호).
        for mods in [
            keyboard::Modifiers::empty(),
            ctrl | keyboard::Modifiers::SHIFT,
            ctrl | keyboard::Modifiers::ALT,
            ctrl | keyboard::Modifiers::LOGO,
        ] {
            assert!(
                RunConfigManager::stdin_bar_key_message(
                    &key_c,
                    phys_c,
                    mods,
                    event::Status::Ignored
                )
                .is_none()
            );
        }
    }

    #[test]
    fn stdin_history_resolved_walks_and_restores_draft() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);
        for line in ["a", "b"] {
            let _ = app.handle_session_stdin_changed(sid, String::from(line));
            let _ = app.handle_session_stdin_submitted(sid);
        }
        let _ = app.handle_session_stdin_changed(sid, String::from("typing"));

        let input_id = crate::views::shared::session_stdin_input_id(sid);
        let draft = |app: &mut RunConfigManager| {
            app.session_by_id_mut(sid)
                .unwrap()
                .stdin_input
                .clone()
                .unwrap()
        };

        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, input_id.clone());
        assert_eq!(draft(&mut app), "b");
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, input_id.clone());
        assert_eq!(draft(&mut app), "a");
        // 가장 오래된 항목에서 정지.
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, input_id.clone());
        assert_eq!(draft(&mut app), "a");
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryNewer, input_id.clone());
        assert_eq!(draft(&mut app), "b");
        // 최신을 지나면 타이핑 중이던 드래프트 복원.
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryNewer, input_id);
        assert_eq!(draft(&mut app), "typing");
    }

    #[test]
    fn stdin_history_edit_resets_cursor_midway() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);
        for line in ["a", "b"] {
            let _ = app.handle_session_stdin_changed(sid, String::from(line));
            let _ = app.handle_session_stdin_submitted(sid);
        }

        let input_id = crate::views::shared::session_stdin_input_id(sid);
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, input_id.clone());
        // 열람 중 수동 편집 → 탐색이 끝나고 편집본이 새 라이브 드래프트가 된다.
        let _ = app.handle_session_stdin_changed(sid, String::from("bx"));
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, input_id.clone());
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("b")
        );
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryNewer, input_id);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("bx")
        );
    }

    #[test]
    fn controls_menu_persists_across_item_actions_but_closes_on_layout_change() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        let _ = app.handle_toggle_session_controls_menu(sid);
        assert!(app.session_by_id_mut(sid).unwrap().controls_menu_open);

        // 항목 액션은 메뉴를 닫지 않는다 — 지속 표면(stop→rerun 연속 조작 등).
        app.close_controls_menu_on_action(&Message::RerunSession(sid));
        app.close_controls_menu_on_action(&Message::OpenSessionStdin(sid));
        app.close_controls_menu_on_action(&Message::StopSession(sid));
        assert!(
            app.session_by_id_mut(sid).unwrap().controls_menu_open,
            "menu must stay open across item actions"
        );

        // maximize는 pane이 넓어져 full 컨트롤로 전환되므로(⋯ 소멸) 닫는다.
        let pane_id = pane_for_session(&app.workspace_tabs[app.selected_tab_index], sid);
        app.close_controls_menu_on_action(&Message::TogglePaneMaximize(pane_id));
        assert!(!app.session_by_id_mut(sid).unwrap().controls_menu_open);

        // 레이아웃 변화(pane 닫기/리사이즈류)는 모든 메뉴를 닫는다.
        let _ = app.handle_toggle_session_controls_menu(sid);
        app.close_controls_menu_on_action(&Message::ClosePane(pane_id));
        assert!(!app.session_by_id_mut(sid).unwrap().controls_menu_open);

        // 탭 닫기/Cmd+W도 pane 소멸 경로 — 세션 객체에 열림 플래그가 남아 재열기 시
        // 고아 메뉴(이중 컨트롤)로 나타나지 않아야 한다.
        let _ = app.handle_toggle_session_controls_menu(sid);
        app.close_controls_menu_on_action(&Message::CloseTab(0));
        assert!(!app.session_by_id_mut(sid).unwrap().controls_menu_open);
        let _ = app.handle_toggle_session_controls_menu(sid);
        app.close_controls_menu_on_action(&Message::CloseFocusedPane);
        assert!(!app.session_by_id_mut(sid).unwrap().controls_menu_open);
    }

    #[test]
    fn interrupt_completed_broken_without_pid_sets_status() {
        use crate::models::StdinWriteError;
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        {
            let session = app.session_by_id_mut(sid).unwrap();
            session.is_running = true;
            session.process_pid = None; // 시그널 폴백 불가 케이스
        }
        let _ = app.handle_session_interrupt_completed(
            sid,
            Err(StdinWriteError::Broken(String::from("no writer"))),
        );
        assert!(
            app.status_message.contains("Interrupt"),
            "user must be told the interrupt could not be delivered: {:?}",
            app.status_message
        );

        // Timeout도 폴백 불가면 안내한다 (막힌 쓰기 경로의 에스컬레이션 실패 케이스).
        app.status_message.clear();
        let _ = app.handle_session_interrupt_completed(sid, Err(StdinWriteError::Timeout));
        assert!(
            app.status_message.contains("Interrupt pending"),
            "timeout without a fallback pid must surface: {:?}",
            app.status_message
        );
    }

    #[test]
    fn stdin_history_resolved_ignores_foreign_widget_id() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_open_session_stdin(sid);
        let _ = app.handle_session_stdin_changed(sid, String::from("x"));
        let _ = app.handle_session_stdin_submitted(sid);
        let _ = app.handle_session_stdin_changed(sid, String::from("typing"));

        // 검색 입력 등 다른 위젯이 포커스면 히스토리 키는 no-op.
        let foreign = crate::views::shared::session_search_input_id(sid);
        let _ = app.handle_stdin_bar_key_resolved(StdinBarKey::HistoryOlder, foreign);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("typing")
        );
    }

    #[test]
    fn stop_discards_output_queued_behind_cancel() {
        // Stop(cancel) 후 채널에 남아 있던 출력 배치는 폐기되어야 한다 — 아니면 폭주 중
        // Stop 시 RunCompleted가 대형 배치들 뒤로 밀려 정지가 수 초 늦어 보인다.
        use crate::models::OutputEvent;
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.session_by_id_mut(sid).unwrap().is_running = true;

        let _ = app.handle_output_received(sid, &[OutputEvent::Line(String::from("before"))]);
        let before = app.session_by_id_mut(sid).unwrap().output_lines.len();
        assert!(before > 0, "테스트 전제: cancel 전 출력은 적용");

        let _ = app.handle_stop_session(sid);
        {
            // Stop은 RunCompleted 왕복을 기다리지 않고 UI 상태를 즉시 확정한다.
            let s = app.session_by_id_mut(sid).unwrap();
            assert!(!s.is_running, "stop 즉시 is_running=false");
            assert!(s.finished_at.is_some(), "stop 즉시 finished_at 확정");
        }
        let _ = app.handle_output_received(sid, &[OutputEvent::Line(String::from("late"))]);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().output_lines.len(),
            before,
            "cancel 후 도착한 배치는 폐기"
        );

        // 늦게 도착한 RunCompleted는 exit code를 채우되 Stop의 종료 시각을 보존한다.
        let stopped_at = app.session_by_id_mut(sid).unwrap().finished_at;
        let _ = app.handle_run_completed(sid, Ok(143));
        let s = app.session_by_id_mut(sid).unwrap();
        assert_eq!(s.exit_code, Some(143));
        assert_eq!(
            s.finished_at, stopped_at,
            "finished_at은 Stop 클릭 기준 유지"
        );

        // rerun은 cancel_flag를 새 Arc로 교체하므로 새 실행의 출력은 다시 적용된다.
        let _ = app.handle_rerun_session(sid);
        let new_sid = app
            .sessions
            .iter()
            .find(|s| s.config_name == "a")
            .expect("session survives rerun")
            .id;
        let _ = app.handle_output_received(new_sid, &[OutputEvent::Line(String::from("fresh"))]);
        assert!(
            !app.session_by_id_mut(new_sid)
                .unwrap()
                .output_lines
                .is_empty(),
            "rerun 후 새 실행 출력은 적용"
        );
    }

    #[test]
    fn stdin_submit_takes_draft_only_while_running() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        let _ = app.handle_open_session_stdin(sid);
        let _ = app.handle_session_stdin_changed(sid, String::from("hi"));

        // 종료된 세션 → 제출 no-op(드래프트 유지).
        app.session_by_id_mut(sid).unwrap().is_running = false;
        let _ = app.handle_session_stdin_submitted(sid);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("hi")
        );

        // 실행 중 → 제출이 드래프트를 낙관적으로 비운다 (전송 Task는 테스트에서 미폴링).
        app.session_by_id_mut(sid).unwrap().is_running = true;
        let _ = app.handle_session_stdin_submitted(sid);
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("")
        );
    }

    #[test]
    fn stdin_write_completed_restore_and_echo_rules() {
        use crate::models::StdinWriteError;
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        let _ = app.handle_open_session_stdin(sid);

        // Broken + 드래프트 미변경("") → 원문 복원.
        let _ = app.handle_session_stdin_write_completed(
            sid,
            String::from("lost"),
            Err(StdinWriteError::Broken(String::from("EPIPE"))),
        );
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("lost")
        );

        // Broken + 그 사이 새로 타이핑 → 덮어쓰지 않는다.
        let _ = app.handle_session_stdin_changed(sid, String::from("newer"));
        let _ = app.handle_session_stdin_write_completed(
            sid,
            String::from("old"),
            Err(StdinWriteError::Broken(String::from("EPIPE"))),
        );
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("newer")
        );

        // Timeout → 복원 금지 (백그라운드에서 결국 쓰이므로 재제출=중복).
        let _ = app.handle_session_stdin_changed(sid, String::new());
        let _ = app.handle_session_stdin_write_completed(
            sid,
            String::from("pending"),
            Err(StdinWriteError::Timeout),
        );
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().stdin_input.as_deref(),
            Some("")
        );
        assert!(app.status_message.contains("Input pending"));

        // Ok(true) = 폴백 pipe(Windows <1809) 성공 → 로컬 에코 한 줄.
        let before = app.session_by_id_mut(sid).unwrap().output_lines.len();
        let _ = app.handle_session_stdin_write_completed(sid, String::from("echoed"), Ok(true));
        let session = app.session_by_id_mut(sid).unwrap();
        assert_eq!(session.output_lines.len(), before + 1);
        // Ok(false) = PTY/ConPTY 성공 → 에코 없음 (전송이 담당).
        let before = session.output_lines.len();
        let _ = app.handle_session_stdin_write_completed(sid, String::from("quiet"), Ok(false));
        assert_eq!(
            app.session_by_id_mut(sid).unwrap().output_lines.len(),
            before
        );
    }

    #[test]
    fn close_active_bar_prefers_stdin_then_search() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        let sid = ids[0];
        app.current_view = ViewMode::Sessions;
        let _ = app.handle_open_session_search(sid);
        let _ = app.handle_open_session_stdin(sid);

        // 1차 Esc: stdin만 닫힘 (검색 유지).
        let _ = app.handle_close_active_bar();
        let session = app.session_by_id_mut(sid).unwrap();
        assert!(session.stdin_input.is_none());
        assert!(session.search.is_some());

        // 2차 Esc: 검색 닫힘.
        let _ = app.handle_close_active_bar();
        assert!(app.session_by_id_mut(sid).unwrap().search.is_none());
    }

    #[test]
    fn stdin_open_target_requires_sessions_view() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        app.current_view = ViewMode::Configuration;
        assert_eq!(app.stdin_open_target(), None, "Configuration 뷰에선 무시");
        app.current_view = ViewMode::Sessions;
        assert_eq!(app.stdin_open_target(), Some(ids[0]));
    }

    #[test]
    fn close_focused_pane_hides_session_but_keeps_it_in_list() {
        let (mut app, ids) = manager_with_open_panes(&["a", "b"]);
        app.current_view = ViewMode::Sessions;
        // 마지막으로 연 세션(b)이 포커스 상태.
        let focused = app.workspace_tabs[0].focused_session().unwrap();
        assert_eq!(focused, ids[1]);

        let _ = app.handle_close_focused_pane();

        // 워크스페이스에서는 사라지고, 세션 목록에는 남는다.
        assert!(!app.workspace_tabs[0].contains_session(ids[1]));
        assert!(app.sessions.iter().any(|s| s.id == ids[1]));
    }

    #[test]
    fn close_focused_pane_is_noop_outside_sessions_view() {
        let (mut app, ids) = manager_with_open_panes(&["a"]);
        app.current_view = ViewMode::Configuration;
        let _ = app.handle_close_focused_pane();
        assert!(app.workspace_tabs[0].contains_session(ids[0]));
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
    fn title_bar_touch_drag_moves_window_by_finger_delta() {
        let (mut app, _task) = RunConfigManager::new();
        app.window_pos = Point::new(100.0, 50.0);

        // 창의 (30,10) 지점을 잡음
        let _ = app.handle_title_bar_touch_drag_start(Point::new(30.0, 10.0));
        // 손가락이 (50,10)으로 이동 → 창은 (20,0)만큼 이동
        let _ = app.handle_title_bar_touch_drag_move(Point::new(50.0, 10.0));
        assert_eq!(app.window_pos, Point::new(120.0, 50.0));
    }

    #[test]
    fn title_bar_touch_drag_recurrence_is_stable_across_steps() {
        // 각 이동 이벤트의 손가락 좌표는 "현재 창 위치" 기준이다. 손가락을 창 기준
        // grab 지점에 유지하면 delta 0이라 창은 (어디에 있든) 제자리를 지킨다.
        let (mut app, _task) = RunConfigManager::new();
        app.window_pos = Point::new(200.0, 100.0);
        let _ = app.handle_title_bar_touch_drag_start(Point::new(40.0, 12.0));

        // grab 지점 그대로면 delta 0 → 창 불변 (반복해도 안정)
        let _ = app.handle_title_bar_touch_drag_move(Point::new(40.0, 12.0));
        let _ = app.handle_title_bar_touch_drag_move(Point::new(40.0, 12.0));
        assert_eq!(app.window_pos, Point::new(200.0, 100.0));

        // 창 기준 (60,12)로 이동 → +20,0 → (220,100)
        let _ = app.handle_title_bar_touch_drag_move(Point::new(60.0, 12.0));
        assert_eq!(app.window_pos, Point::new(220.0, 100.0));

        // 창은 이제 220. 손가락이 물리적으로 제자리면 다음 이벤트의 창 기준 좌표는
        // 다시 grab(40) → delta 0 → 창은 220 유지 (200으로 되돌아가지 않음).
        let _ = app.handle_title_bar_touch_drag_move(Point::new(40.0, 12.0));
        assert_eq!(app.window_pos, Point::new(220.0, 100.0));
    }

    #[test]
    fn title_bar_touch_drag_ignored_when_maximized() {
        let (mut app, _task) = RunConfigManager::new();
        app.is_window_maximized = true;
        let _ = app.handle_title_bar_touch_drag_start(Point::new(10.0, 10.0));
        assert!(app.title_bar_touch_drag.is_none());
    }

    #[test]
    fn title_bar_touch_drag_move_bails_if_maximized_mid_drag() {
        // 드래그 시작 뒤 (더블탭 등으로) 최대화가 발생하고 이어서 move 이벤트가 오면,
        // move_to로 창을 강제 복원하지 않고 드래그를 취소해야 한다.
        let (mut app, _task) = RunConfigManager::new();
        app.window_pos = Point::new(100.0, 100.0);
        let _ = app.handle_title_bar_touch_drag_start(Point::new(10.0, 10.0));
        assert!(app.title_bar_touch_drag.is_some());

        app.is_window_maximized = true; // 최대화가 드래그 도중 발생
        let _ = app.handle_title_bar_touch_drag_move(Point::new(40.0, 10.0));
        assert!(app.title_bar_touch_drag.is_none());
        assert_eq!(app.window_pos, Point::new(100.0, 100.0)); // 이동 없음
    }

    #[test]
    fn losing_focus_ends_stuck_touch_drag() {
        // 포커스 상실 시 터치 종료 이벤트가 유실될 수 있으므로 드래그를 끝내,
        // 이후 WindowMoved가 계속 무시되어 window_pos가 stale해지는 것을 막는다.
        let (mut app, _task) = RunConfigManager::new();
        let _ = app.handle_title_bar_touch_drag_start(Point::new(5.0, 5.0));
        assert!(app.title_bar_touch_drag.is_some());

        let _ = app.dispatch_message(Message::WindowFocusChanged(false));
        assert!(app.title_bar_touch_drag.is_none());

        // 드래그가 풀렸으니 이후 이동이 정상 반영된다
        let _ = app.handle_window_moved(Point::new(300.0, 300.0));
        assert_eq!(app.window_pos, Point::new(300.0, 300.0));
    }

    #[test]
    fn window_moved_ignored_while_touch_dragging() {
        // 드래그 중에는 우리가 낸 move_to가 Moved로 되돌아오므로 기준 위치를
        // 덮어쓰지 않아야 한다(외부 Moved만 반영).
        let (mut app, _task) = RunConfigManager::new();
        app.window_pos = Point::new(100.0, 100.0);
        let _ = app.handle_title_bar_touch_drag_start(Point::new(5.0, 5.0));
        let _ = app.handle_window_moved(Point::new(999.0, 999.0));
        assert_eq!(app.window_pos, Point::new(100.0, 100.0));

        // 드래그가 끝나면 다시 외부 이동을 반영한다
        app.title_bar_touch_drag = None;
        let _ = app.handle_window_moved(Point::new(300.0, 300.0));
        assert_eq!(app.window_pos, Point::new(300.0, 300.0));
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
        assert_eq!(
            search
                .matches
                .iter()
                .map(|m| m.line_idx)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
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
        assert_eq!(
            search
                .matches
                .iter()
                .map(|m| m.line_idx)
                .collect::<Vec<_>>(),
            vec![0]
        );
        assert_eq!(search.current, 0);
    }

    #[test]
    fn clear_session_output_empties_buffer_and_search_cache() {
        let (mut app, ids) = manager_with_sessions(&["s"]);
        let sid = ids[0];
        {
            let session = &mut app.sessions[0];
            session.add_output_line("line 1");
            session.add_output_line("error here");
        }
        let _ = app.handle_open_session_search(sid);
        let _ = app.handle_session_search_changed(sid, "error".to_string());
        assert_eq!(app.sessions[0].search.as_ref().unwrap().matches.len(), 1);

        // 로그 지우기: 출력 버퍼와 검색 매치 캐시가 모두 비워져야 한다.
        let _ = app.handle_clear_session_output(sid);

        assert!(app.sessions[0].output_lines.is_empty());
        assert!(app.sessions[0].search.as_ref().unwrap().matches.is_empty());
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

    /// 주어진 세션을 담은 `pane_grid::Pane` 핸들을 찾는다 (클릭 시뮬레이션용).
    fn pane_for_session(tab: &WorkspaceTab, session_id: Uuid) -> pane_grid::Pane {
        tab.pane_layout
            .panes
            .iter()
            .find(|(_, pane)| pane.session_id == Some(session_id))
            .map(|(pane, _)| *pane)
            .expect("session must occupy a pane")
    }

    /// `names`의 모든 세션을 현재 탭의 pane으로 연 앱을 만든다.
    fn manager_with_open_panes(names: &[&str]) -> (RunConfigManager, Vec<Uuid>) {
        let (mut app, ids) = manager_with_sessions(names);
        for id in &ids {
            app.workspace_tabs[0].open_session(*id, 1.0);
        }
        (app, ids)
    }

    #[test]
    fn search_open_targets_clicked_session_not_first() {
        let (mut app, ids) = manager_with_open_panes(&["a", "b", "c"]);

        // 마지막으로 연 세션(c)이 포커스 — Cmd+F는 "첫 세션"이 아닌 이걸 대상으로.
        assert_eq!(app.search_open_target(), Some(ids[2]));

        // 두 번째 세션 pane을 클릭하면 그 세션이 Cmd+F 대상이 된다.
        let pane_b = pane_for_session(&app.workspace_tabs[0], ids[1]);
        let _ = app.handle_pane_clicked(pane_b);
        assert_eq!(app.search_open_target(), Some(ids[1]));
    }

    /// 회귀 가드: A에서 검색을 연 뒤 다른 pane으로 포커스를 옮겨도 Enter/ESC는
    /// "열린 검색"(A)을, Cmd+F는 "포커스 pane"(B)을 대상으로 해야 한다.
    #[test]
    fn search_nav_follows_open_search_even_after_focus_moves() {
        let (mut app, ids) = manager_with_open_panes(&["a", "b", "c"]);

        let _ = app.handle_open_session_search(ids[0]); // a에서 검색 열기
        let pane_b = pane_for_session(&app.workspace_tabs[0], ids[1]);
        let _ = app.handle_pane_clicked(pane_b); // 포커스를 b로 이동

        assert_eq!(
            app.search_open_target(),
            Some(ids[1]),
            "Cmd+F는 포커스 pane을 연다"
        );
        assert_eq!(
            app.search_nav_target(),
            Some(ids[0]),
            "Enter/ESC는 이미 열린 검색을 계속 조작한다"
        );
    }

    #[test]
    fn search_nav_prefers_focused_pane_when_it_is_searching() {
        let (mut app, ids) = manager_with_open_panes(&["a", "b"]);

        // 두 pane 모두 검색이 열린 상태에서 a를 포커스하면 네비게이션 대상은 a.
        let _ = app.handle_open_session_search(ids[0]);
        let _ = app.handle_open_session_search(ids[1]);
        let pane_a = pane_for_session(&app.workspace_tabs[0], ids[0]);
        let _ = app.handle_pane_clicked(pane_a);

        assert_eq!(app.search_nav_target(), Some(ids[0]));
    }

    #[test]
    fn search_open_falls_back_when_focused_session_closed() {
        let (mut app, ids) = manager_with_open_panes(&["a", "b", "c"]);

        // b를 포커스한 뒤 그 세션을 워크스페이스에서 닫으면 포커스가 사라지고,
        // 대상은 살아 있는 세션으로 폴백된다 (stale 포커스를 가리키지 않는다).
        let pane_b = pane_for_session(&app.workspace_tabs[0], ids[1]);
        let _ = app.handle_pane_clicked(pane_b);
        app.workspace_tabs[0].remove_session(ids[1]);

        // 포커스가 사라지고 검색도 열려 있지 않으니 search_open_target의 마지막 폴백
        // (탭의 첫 세션 pane)을 탄다. 어느 leaf가 첫째인지는 레이아웃 내부 구현이므로,
        // 닫힌 세션이 아닌 살아 있는 세션이라는 것만 단언한다.
        let target = app.search_open_target().expect("a live session remains");
        assert!(
            target == ids[0] || target == ids[2],
            "닫힌 세션이 아닌 살아 있는 세션을 반환해야 한다"
        );
    }
}
