use crate::models::{
    ConfigurationType, ExecuteModeType, KotlinLaunchModeType, NodeCommand, PackageManager,
    RunConfiguration,
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
    /// 구성 삭제 요청 — 즉시 지우지 않고 확인 모달을 연다 (undo 없는 파괴적 조작)
    RequestDeleteConfiguration(Option<usize>),
    /// 삭제 확인 모달의 Delete 확정 → 실제 삭제
    ConfirmDeleteConfiguration,
    /// 삭제 확인 모달 취소 (Cancel/X/Esc)
    CancelDeleteConfiguration,
    /// 지정된 구성 복제 (옵션을 그대로 복사한 새 구성 생성)
    CloneConfiguration(Option<usize>),
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
    /// Compound 구성에 멤버(다른 구성 id) 추가
    CompoundMemberAdded(Uuid),
    /// Compound 구성에서 멤버 제거
    CompoundMemberRemoved(Uuid),
    /// Compound 구성의 실행 대상 workspace 탭 이름 변경 (빈 문자열이면 현재 탭에서 실행)
    CompoundWorkspaceChanged(String),
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
    /// Node 메타데이터 비동기 로드 완료 — 파일 로드/열기 시 UI 스레드 블로킹을 피하기
    /// 위해 백그라운드 스캔 결과를 전달 (`config_id`, package.json 상대경로, scripts)
    NodeMetadataLoaded(Uuid, Vec<String>, Vec<String>),
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

    // Kotlin 구성 편집 메시지
    /// 실행 모드 선택 (Main class / JAR)
    KotlinLaunchModeChanged(KotlinLaunchModeType),
    /// main class 변경
    KotlinMainClassChanged(String),
    /// classpath 변경
    KotlinClasspathChanged(String),
    /// JAR 파일 경로 변경
    KotlinJarPathChanged(String),
    /// JAR 파일 선택 다이얼로그 열기
    BrowseKotlinJarPath,
    /// JAR 파일 선택 완료
    KotlinJarPathSelected(Result<String, String>),
    /// VM options 변경
    KotlinVmOptionsChanged(String),
    /// 프로그램 인자 변경
    KotlinProgramArgumentsChanged(String),
    /// JDK 선택 변경 (레이블 → 경로 해석)
    KotlinJdkChanged(String),
    /// JDK 수동 선택 다이얼로그 열기
    BrowseKotlinJdk,
    /// JDK 수동 선택 완료
    KotlinJdkPathSelected(Result<String, String>),
    /// JDK 감지 완료
    JdksDetected(Vec<(String, String)>),

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

    // 앱 설정 모달 lifecycle
    /// 설정 모달 열기 (현재 설정값을 staging으로 복사)
    OpenSettingsModal,
    /// 설정 모달 OK — staging 검증 후 설정에 반영하고 저장
    ConfirmSettingsModal,
    /// 설정 모달 Cancel — staging 폐기
    CancelSettingsModal,
    /// 설정: 실행 시 Environment 라인 표시 토글
    SettingsToggleEnvironment(bool),
    /// 설정: 세션 출력 최대 라인 수 입력 변경 (raw 문자열; Confirm 시 파싱·클램프)
    SettingsMaxLinesChanged(String),
    /// 설정: 새 세션 자동 스크롤 기본값 토글
    SettingsToggleAutoScroll(bool),
    /// 설정: 시작 시 업데이트 자동 확인 토글
    SettingsToggleAutoCheckUpdates(bool),

    /// Editor focus 이동 (`true`면 역방향)
    MoveEditorFocus(bool),
    /// Editor pane이 키보드 focus 이동 대상인지 갱신
    EditorFocusAreaChanged(bool),

    // Load/Save 메시지
    /// 앱 시작 시 자동 로드 완료 (성공/실패)
    ConfigurationsLoaded(Result<Vec<RunConfiguration>, String>),
    /// 현재 구성 파일 저장
    SaveConfigurations,
    /// 구성 파일 저장 완료 (저장된 파일 경로)
    ConfigurationsSaved(Result<PathBuf, String>),

    // 구성 가져오기(Import) 모달 lifecycle — 파일에서 선택 병합
    /// 가져올 파일 선택 다이얼로그 열기
    ImportConfigurations,
    /// 가져올 파일 읽기/파싱 완료 (파일의 구성 목록, 파일 경로) — 성공 시 모달 열림
    ImportFileLoaded(Result<(Vec<RunConfiguration>, PathBuf), String>),
    /// 가져오기 모달 Import — 선택된 구성을 현재 목록에 병합
    ConfirmImportModal,
    /// 가져오기 모달 Cancel — staging 폐기
    CancelImportModal,
    /// 가져오기 대상 구성 선택 토글 (`config_id`, 선택 여부)
    ImportModalToggleConfig(Uuid, bool),
    /// 가져오기 대상 전체 선택/해제 토글
    ImportModalToggleAll(bool),

    // 구성 내보내기(Export) 모달 lifecycle
    /// 내보내기 모달 열기 (전체 구성이 선택된 staging으로 시작)
    OpenExportModal,
    /// 내보내기 모달 Export — 선택된 구성만 파일로 저장
    ConfirmExportModal,
    /// 내보내기 모달 Cancel — staging 폐기
    CancelExportModal,
    /// 내보내기 대상 구성 선택 토글 (`config_id`, 선택 여부)
    ExportModalToggleConfig(Uuid, bool),
    /// 내보내기 대상 전체 선택/해제 토글
    ExportModalToggleAll(bool),
    /// 구성 내보내기 완료 (저장 경로 또는 에러/취소)
    ConfigurationsExported(Result<PathBuf, String>),

    // 실행 세션 메시지
    /// 프로세스 시작됨 (세션 ID, PID) - Drop cleanup을 위한 PID 추적
    ProcessStarted(Uuid, u32),
    /// 프로세스 출력 수신 (세션 ID, 출력 이벤트 배치 — Line=추가, Replace=마지막 라인 교체)
    OutputReceived(Uuid, Vec<crate::models::OutputEvent>),
    /// 프로세스 실행 완료 (세션 ID, 종료 코드 또는 에러 메시지)
    RunCompleted(Uuid, Result<i32, String>),
    /// 터미널 뷰포트 크기 변경 (세션 ID, cols, rows) — PTY resize로 전달 (변경 시에만 발행)
    SessionViewportResized(Uuid, u16, u16),

    // 세션 관리 메시지 (세션은 안정적인 `Uuid`로 식별 — stale 인덱스 방지)
    /// 세션 재실행 (`session_id`)
    RerunSession(Uuid),
    /// 실행 중인 세션 중지 (`session_id`)
    StopSession(Uuid),
    /// 세션 종료 및 제거 (`session_id`)
    RemoveSession(Uuid),
    /// 세션을 현재 워크스페이스에서 열기/focus (`session_id`)
    OpenSessionInWorkspace(Uuid),
    /// 세션 리스트 항목 hover 상태 변경 (표시용 인덱스)
    SessionListItemHovered(Option<usize>),
    /// 실행 중인 모든 세션 중지
    StopAllSessions,
    /// 모든 세션 재실행
    RerunAllSessions,
    /// 실패(0이 아닌 종료 코드)한 세션만 재실행
    RerunFailedSessions,

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
    /// 워크스페이스로 점프 (`index`, 0-based) — Sessions 화면으로 전환 후 해당 탭 선택.
    /// 키보드 단축키(Cmd+1~9) 전용. 범위 밖 인덱스는 핸들러에서 무시한다.
    JumpToWorkspace(usize),

    // Pane 관리 메시지 (현재 탭 내)
    /// `pane_grid` 드래그 이벤트 처리
    PaneGridDragged(pane_grid::DragEvent),
    /// `pane_grid` 리사이즈 이벤트 처리
    PaneGridResized(pane_grid::ResizeEvent),
    /// Pane 닫기 (`pane_id`)
    ClosePane(pane_grid::Pane),
    /// Pane 최대화/복원 토글 (`pane_id`)
    TogglePaneMaximize(pane_grid::Pane),
    /// Pane 클릭 — 그 pane의 세션을 활성 탭의 포커스로 기록 (검색 등 단축키 대상)
    PaneClicked(pane_grid::Pane),

    // 탭 간 드래그 앤 드롭
    /// 탭 바 위에 마우스 진입 (드래그 중 hover)
    TabBarHovered(Option<usize>),

    // 클립보드 메시지
    /// 텍스트를 클립보드에 복사
    CopyToClipboard(String),

    // 스크롤 위치 관리
    /// 세션의 스크롤 위치 변경 (세션 ID, 스크롤 진행률 0.0~1.0, 바닥 도달 여부)
    /// `at_bottom`은 terminal이 절대 거리로 판정한다. 비율 임계치는 대량 버퍼에서
    /// 바닥 근처 수백 줄을 모두 "바닥"으로 오판해 auto_scroll 해제가 안 됐다.
    SessionScrollChanged(Uuid, f32, bool),
    /// 자동 스크롤 토글 (세션 ID)
    ToggleAutoScroll(Uuid),

    // 출력 검색/필터 (세션 ID로 식별)
    /// 검색바 열기/포커스 (`session_id`)
    OpenSessionSearch(Uuid),
    /// 검색바 닫기 (`session_id`)
    CloseSessionSearch(Uuid),
    /// 검색어 변경 (`session_id`, 새 검색어)
    SessionSearchChanged(Uuid, String),
    /// 다음 매치로 이동 (`session_id`)
    SessionSearchNext(Uuid),
    /// 이전 매치로 이동 (`session_id`)
    SessionSearchPrev(Uuid),
    /// 필터 모드 토글(매치 라인만 표시) (`session_id`)
    ToggleSessionSearchFilter(Uuid),
    /// 정규식 모드 토글 (`session_id`)
    ToggleSessionSearchRegex(Uuid),
    /// 활성 pane의 세션에 검색바 열기 (Ctrl+F — 대상 세션은 핸들러가 해석)
    OpenSearchInActivePane,
    /// 활성 바(stdin 우선, 다음 검색) 하나 닫기 (ESC — 대상은 핸들러가 우선순위로 해석)
    CloseActiveBar,
    /// 활성 pane 검색의 다음 매치로 이동 (Enter — 대상 세션은 핸들러가 해석)
    SearchNextInActivePane,
    /// 활성 pane 검색의 이전 매치로 이동 (Shift+Enter — 대상 세션은 핸들러가 해석)
    SearchPrevInActivePane,

    // 세션 stdin 입력 (PTY/pipe stdin으로 한 줄 전송; search 클러스터와 동일 컨벤션)
    /// stdin 입력바 열기 (`session_id`)
    OpenSessionStdin(Uuid),
    /// stdin 입력바 닫기 (`session_id`)
    CloseSessionStdin(Uuid),
    /// stdin 드래프트 변경 (`session_id`, 입력 값)
    SessionStdinChanged(Uuid, String),
    /// stdin 드래프트 제출 — 프로세스 stdin으로 전송 (`session_id`)
    SessionStdinSubmitted(Uuid),
    /// stdin 쓰기 완료 (`session_id`, 제출 원문(실패 복원·폴백 pipe 로컬 에코용),
    /// 성공 시 로컬 에코 필요 여부(PTY/ConPTY=false, <1809 폴백 pipe=true) / 실패 사유)
    SessionStdinWriteCompleted(Uuid, String, Result<bool, crate::models::StdinWriteError>),
    /// 활성 pane 세션에 stdin 바 열기 (Cmd+I — 대상은 핸들러가 해석, Sessions 뷰 전용)
    OpenStdinInActivePane,
    /// 세션 출력 버퍼 비우기 (`session_id`) — 실행 중인 프로세스는 유지, 화면 로그만 클리어
    ClearSessionOutput(Uuid),
    /// 세션 출력을 파일로 내보내기 (`session_id`)
    ExportSessionOutput(Uuid),
    /// 출력 내보내기 완료 (저장 경로 또는 에러/취소)
    SessionOutputExported(Result<PathBuf, String>),

    // 컨트롤 오버플로 메뉴 (pane이 좁아 전체 버튼이 안 들어갈 때 ⋯로 펼치는 세션 액션 메뉴)
    /// 컨트롤 오버플로 메뉴(⋯) 토글 (`session_id`)
    ToggleSessionControlsMenu(Uuid),

    // URL 처리
    /// URL을 기본 브라우저로 열기
    OpenUrl(String),

    /// 마우스 릴리즈 (구성 리스트 드래그 중이면 드롭 완료)
    MouseReleased,
    /// 커서 이동 (`pane_grid` 드래그 중 위치 추적)
    CursorMoved(iced::Point),
    /// Configuration 화면 `pane_grid` 리사이즈 이벤트 처리
    ConfigurationPaneResized(pane_grid::ResizeEvent),

    // 업데이트 확인
    /// 최신 버전 확인 수동 트리거 (상태바 버전 라벨 클릭)
    CheckForUpdates,
    /// 최신 버전 확인 완료 (최신/업데이트 가능 또는 에러)
    UpdateCheckCompleted(Result<crate::services::UpdateOutcome, String>),
    /// 확인 중 로딩 스피너 프레임 진행 (타이머 tick)
    UpdateSpinnerTick,
    /// 실행 중 세션의 라이브 경과시간 갱신 tick (1초, 상태 변경 없음 — 재렌더 트리거)
    SessionTimerTick,
    /// Cmd+R: 컨텍스트 실행 — Sessions 뷰에서 포커스 세션이 있으면 재실행, 아니면 선택 구성 실행
    RunShortcut,
    /// Cmd+W: 포커스된 세션 pane을 현재 워크스페이스에서 숨김 (세션은 목록에 유지)
    CloseFocusedPane,
    /// 인앱 업데이트 요청 — 즉시 설치하지 않고 확인 모달을 연다 (설치 성공 시 재시작되므로)
    RequestInstallUpdate,
    /// 업데이트 확인 모달의 Update 확정 → 실제 설치 시작
    ConfirmInstallUpdate,
    /// 업데이트 확인 모달 취소 (Cancel/X/Esc)
    CancelInstallUpdate,
    /// 인앱 업데이트 다운로드·검증·적용 완료 (성공 시 재실행 계획 포함)
    UpdateInstallCompleted(Result<crate::services::RelaunchPlan, String>),

    // 윈도우 chrome 제어
    /// 메인 윈도우가 열림
    WindowOpened(window::Id),
    /// 메인 윈도우 리사이즈
    WindowResized(window::Id, iced::Size),
    /// 윈도우 최대화 상태 동기화
    WindowMaximized(bool),
    /// 윈도우 포커스 변경 (백그라운드 완료 알림 판단용)
    WindowFocusChanged(bool),
    /// 윈도우가 이동함 (창 좌상단 logical 좌표). 터치 드래그의 기준 위치 추적용.
    WindowMoved(iced::Point),
    /// 커스텀 타이틀바에서 창 드래그 시작 (마우스 — OS 드래그)
    StartWindowDrag,
    /// 커스텀 타이틀바 터치 드래그 시작 (손가락 창 기준 logical 좌표)
    TitleBarTouchDragStart(iced::Point),
    /// 커스텀 타이틀바 터치 드래그 이동 (손가락 창 기준 logical 좌표)
    TitleBarTouchDragMove(iced::Point),
    /// 커스텀 타이틀바 터치 드래그 종료
    TitleBarTouchDragEnd,
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
