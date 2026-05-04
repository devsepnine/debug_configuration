use crate::models::{
    ConfigurationType, DropZone, ExecuteModeType, NodeCommand, PackageManager, RunConfiguration,
};
use crate::widgets::pane_grid;
use iced::window;
use std::path::PathBuf;
use uuid::Uuid;

/// 메인 뷰 모드 - 구성 관리와 실행 세션 간 전환
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// 구성 관리 탭
    Configuration,
    /// 실행 세션 탭
    Sessions,
}

/// 애플리케이션의 모든 이벤트와 액션을 정의하는 메시지 타입
/// Elm Architecture의 Message 패턴을 따름
#[derive(Debug, Clone)]
pub enum Message {
    // 뷰 전환 메시지
    /// 메인 탭 전환
    SwitchView(ViewMode),

    // 구성 관리 메시지
    /// 새 구성 추가
    AddConfiguration,
    /// 지정된 구성 삭제
    DeleteConfiguration(Option<usize>),
    /// 지정된 구성 실행
    RunConfiguration(Option<usize>),
    /// 구성 리스트 드래그 시작
    StartConfigurationDrag(usize),
    /// 드래그 중 구성 드롭 위치 갱신
    ConfigurationDragHovered(usize, ConfigurationDropPosition),
    /// 드래그 중 구성 항목 이탈
    ConfigurationDragExited(usize),

    // 구성 편집 메시지
    /// 구성 이름 변경
    NameChanged(String),
    /// 구성 타입 변경
    TypeChanged(ConfigurationType),
    /// 명령어 변경
    CommandChanged(String),
    /// 인자 변경
    ArgumentsChanged(String),

    // Shell Script 구성 편집 메시지
    /// Execute Mode 선택 (ScriptFile/ScriptText)
    ExecuteModeChanged(ExecuteModeType),
    /// 스크립트 파일 경로 변경
    ScriptPathChanged(String),
    /// 스크립트 파일 선택 다이얼로그 열기
    BrowseScriptPath,
    /// 스크립트 파일 선택 완료
    ScriptPathSelected(Result<String, String>),
    /// 스크립트 옵션 변경
    ScriptOptionsChanged(String),
    /// 인터프리터 경로 변경
    InterpreterPathChanged(String),
    /// 인터프리터 선택 다이얼로그 열기
    BrowseInterpreterPath,
    /// 인터프리터 선택 완료
    InterpreterPathSelected(Result<String, String>),
    /// 인터프리터 옵션 변경
    InterpreterOptionsChanged(String),
    /// 스크립트 텍스트 변경
    ScriptTextChanged(String),

    // Node 구성 편집 메시지
    /// Node 프로젝트 디렉토리 선택 다이얼로그 열기
    BrowseProjectDirectory,
    /// Node 프로젝트 디렉토리 선택 완료
    ProjectDirectorySelected(Result<String, String>),
    /// Node `package.json` 스캔 완료 (`config_id`, `project_directory`, 상대 경로 목록)
    PackageJsonsScanned(Uuid, String, Vec<String>),
    /// Node package.json 드롭다운 선택 변경 (상대 경로)
    PackageJsonDropdownChanged(String),
    /// Node Runtime 감지 완료
    NodeRuntimesDetected(Vec<(String, String)>),
    /// Node Runtime 경로 변경
    NodeRuntimeChanged(String),
    /// Node package manager 변경
    PackageManagerChanged(PackageManager),
    /// Node package manager 명령어 변경
    NodeCommandChanged(NodeCommand),
    /// Node package script 이름 변경
    NodeScriptNameChanged(String),
    /// Node package manager 인자 변경
    NodeArgumentsChanged(String),
    /// Node 옵션 변경
    NodeOptionsChanged(String),
    /// Node package script 목록 새로고침
    NodeRefreshScripts,

    /// 작업 디렉토리 변경
    WorkingDirectoryChanged(String),
    /// 폴더 선택 다이얼로그 열기
    BrowseWorkingDirectory,
    /// 폴더 선택 완료
    WorkingDirectorySelected(Result<String, String>),

    // 환경 변수 관리 메시지 (메인 단일 input)
    /// 메인 환경변수 input 텍스트 변경 (`KEY=value;...` raw text)
    EnvBulkInputChanged(String),
    /// 메인 환경변수 input Submit (Enter) - 파싱하여 저장
    EnvBulkInputSubmitted,

    // 환경 변수 모달 lifecycle
    /// 환경변수 모달 열기 (현재 environment_variables를 staging으로 복사)
    OpenEnvModal,
    /// 환경변수 모달 OK - staging을 environment_variables에 반영
    ConfirmEnvModal,
    /// 환경변수 모달 Cancel - staging 폐기
    CancelEnvModal,

    // 환경 변수 모달 내부 (always-inline staging)
    /// 모달 row의 키 입력 변경 (인덱스, 새 값). 인덱스가 범위 밖이면 새 row 추가.
    EnvModalRowKeyChanged(usize, String),
    /// 모달 row의 값 입력 변경 (인덱스, 새 값). 인덱스가 범위 밖이면 새 row 추가.
    EnvModalRowValueChanged(usize, String),
    /// 모달 row 복제 — 동일 (key, value)을 entries 끝에 새로 push (인덱스)
    EnvModalDuplicateEntry(usize),
    /// 모달 row 제거 (인덱스)
    EnvModalRemoveEntry(usize),
    /// 모달 내부 다음 cell로 focus (Tab) — modal trap cycle
    EnvModalFocusNext,
    /// 모달 내부 이전 cell로 focus (Shift+Tab) — modal trap cycle
    EnvModalFocusPrev,
    /// Editor focus 이동 (`true`면 역방향)
    MoveEditorFocus(bool),
    /// Editor pane이 키보드 focus 이동 대상인지 갱신
    EditorFocusAreaChanged(bool),

    // Open/Save 메시지
    /// 앱 시작 시 자동 로드 완료 (성공/실패)
    ConfigurationsLoaded(Result<Vec<RunConfiguration>, String>),
    /// 현재 구성 파일 열기 (기존 구성 대체)
    OpenConfigurations,
    /// 구성 파일 열기 완료 (구성 목록, 파일 경로)
    ConfigurationsOpened(Result<(Vec<RunConfiguration>, PathBuf), String>),
    /// 현재 구성 파일 저장
    SaveConfigurations,
    /// 구성 파일 저장 완료 (저장된 파일 경로)
    ConfigurationsSaved(Result<PathBuf, String>),

    // 실행 세션 메시지
    /// 프로세스 시작됨 (세션 ID, PID) - Drop cleanup을 위한 PID 추적
    ProcessStarted(Uuid, u32),
    /// 프로세스 출력 수신 (세션 ID, 출력 텍스트)
    OutputReceived(Uuid, String),
    /// 프로세스 실행 완료 (세션 ID, 종료 코드 또는 에러 메시지)
    RunCompleted(Uuid, Result<i32, String>),

    // 세션 관리 메시지
    /// 세션 재실행 (`session_index`)
    RerunSession(usize),
    /// 실행 중인 세션 중지 (`session_index`)
    StopSession(usize),
    /// 세션 종료 및 제거 (`session_index`)
    RemoveSession(usize),
    /// 세션을 현재 워크스페이스에서 열기/focus (`session_index`)
    OpenSessionInWorkspace(usize),
    /// 세션 리스트 항목 hover 상태 변경
    SessionListItemHovered(Option<usize>),

    // 워크스페이스 탭 관리 메시지
    /// 새 워크스페이스 탭 추가
    AddWorkspaceTab,
    /// 워크스페이스 탭 닫기 (`tab_index`)
    CloseTab(usize),
    /// 탭 이름 클릭 (더블클릭 시 편집 모드 진입)
    TabNameClicked(usize),
    /// 탭 이름 입력 변경 (`new_value`)
    TabNameInputChanged(String),
    /// 탭 이름 편집 완료 (저장)
    FinishEditingTabName,
    /// 탭 이름 편집 취소
    CancelEditingTabName,
    /// 편집 중인 탭 바깥 클릭으로 이름 편집 취소
    CancelEditingTabNameOnOutsideClick,

    // Pane 관리 메시지 (현재 탭 내)
    /// `pane_grid` 드래그 이벤트 처리
    PaneGridDragged(pane_grid::DragEvent),
    /// `pane_grid` 리사이즈 이벤트 처리
    PaneGridResized(pane_grid::ResizeEvent),
    /// Pane 닫기 (`pane_id`)
    ClosePane(pane_grid::Pane),
    /// Pane 최대화/복원 토글 (`pane_id`)
    TogglePaneMaximize(pane_grid::Pane),

    // 탭 간 드래그 앤 드롭
    /// 탭 바 위에 마우스 진입 (드래그 중 hover)
    TabBarHovered(Option<usize>),

    // 클립보드 메시지
    /// 텍스트를 클립보드에 복사
    CopyToClipboard(String),

    // 스크롤 위치 관리
    /// 세션의 스크롤 위치 변경 (세션 ID, 스크롤 진행률 0.0~1.0)
    SessionScrollChanged(Uuid, f32),
    /// 자동 스크롤 토글 (세션 ID)
    ToggleAutoScroll(Uuid),

    // URL 처리
    /// URL을 기본 브라우저로 열기
    OpenUrl(String),

    // 탭 드래그 앤 드롭
    /// Pane 위에 마우스 진입 (드래그 중 hover 추적용)
    PaneHovered(pane_grid::Pane),
    /// Pane에서 마우스 이탈
    PaneUnhovered(pane_grid::Pane),
    /// 드롭 존 위에 마우스 진입 (`pane_id`, `zone`)
    DropZoneHovered(pane_grid::Pane, DropZone),
    /// 드롭 존에서 마우스 이탈
    DropZoneUnhovered,
    /// 외부 드롭 존 위에 마우스 진입 (`pane_grid` 바깥 가장자리)
    OuterDropZoneHovered(DropZone),
    /// 외부 드롭 존에서 마우스 이탈
    OuterDropZoneUnhovered,
    /// 마우스 릴리즈 (드래그 중이면 드롭 완료)
    MouseReleased,
    /// 커서 이동 (`pane_grid` 드래그 중 위치 추적)
    CursorMoved(iced::Point),
    /// Configuration 화면 `pane_grid` 리사이즈 이벤트 처리
    ConfigurationPaneResized(pane_grid::ResizeEvent),

    // 윈도우 chrome 제어
    /// 메인 윈도우가 열림
    WindowOpened(window::Id),
    /// 메인 윈도우 리사이즈
    WindowResized(window::Id, iced::Size),
    /// 윈도우 최대화 상태 동기화
    WindowMaximized(bool),
    /// 커스텀 타이틀바에서 창 드래그 시작
    StartWindowDrag,
    /// 커스텀 프레임에서 창 리사이즈 시작
    ResizeWindow(window::Direction),
    /// 커스텀 타이틀바에서 창 최소화
    MinimizeWindow,
    /// 커스텀 타이틀바에서 창 최대화 토글
    ToggleWindowMaximize,
    /// 커스텀 타이틀바에서 창 닫기
    CloseWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationDropPosition {
    Before,
    After,
}
