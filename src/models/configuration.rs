use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 실행 구성의 타입을 정의하는 열거형
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConfigurationType {
    /// 일반 애플리케이션 실행
    Application,
    /// 셸 스크립트 실행
    ShellScript,
    /// Node.js package manager 실행
    Node,
}

/// JavaScript package manager selection for Node configurations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PackageManager {
    /// Detect from lockfiles in the working directory.
    #[default]
    Auto,
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    pub const ALL: [Self; 5] = [Self::Auto, Self::Npm, Self::Pnpm, Self::Yarn, Self::Bun];

    pub fn executable_name(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Npm => Some("npm"),
            Self::Pnpm => Some("pnpm"),
            Self::Yarn => Some("yarn"),
            Self::Bun => Some("bun"),
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Auto => "Auto",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        };

        write!(f, "{label}")
    }
}

/// Node package manager 명령어 열거형
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum NodeCommand {
    // 자주 사용하는 명령어 (UI 상단)
    #[default]
    Run,
    Install,
    Start,
    Test,
    Build,

    // 나머지 명령어 (알파벳 순)
    Access,
    Adduser,
    Audit,
    Bin,
    Bugs,
    Cache,
    Ci,
    Completion,
    Config,
    Dedupe,
    Deprecate,
    DistTag,
    Docs,
    Doctor,
    Edit,
    Exec,
    Explore,
    FindDupes,
    Fund,
    Help,
    HelpSearch,
    Hook,
    Init,
    Link,
    Logout,
    Ls,
    Outdated,
    Owner,
    Pack,
    Ping,
    Prefix,
    Profile,
    Prune,
    Publish,
    Query,
    Rebuild,
    Repo,
    Restart,
    Root,
    RunScript,
    Search,
    SetScript,
    Shrinkwrap,
    Star,
    Stars,
    Stop,
    Team,
    Token,
    Uninstall,
    Unpublish,
    Unstar,
    Update,
    Version,
    View,
    Whoami,
}

impl NodeCommand {
    /// `command`가 `script_name`을 필요로 하는지 확인
    pub fn requires_script(self) -> bool {
        matches!(self, Self::Run)
    }

    /// 모든 Node package manager 명령어 목록 반환 (UI picklist용)
    pub fn all_commands() -> Vec<Self> {
        vec![
            // 자주 사용하는 명령어
            Self::Run,
            Self::Install,
            Self::Start,
            Self::Test,
            Self::Build,
            // 나머지 명령어
            Self::Access,
            Self::Adduser,
            Self::Audit,
            Self::Bin,
            Self::Bugs,
            Self::Cache,
            Self::Ci,
            Self::Completion,
            Self::Config,
            Self::Dedupe,
            Self::Deprecate,
            Self::DistTag,
            Self::Docs,
            Self::Doctor,
            Self::Edit,
            Self::Exec,
            Self::Explore,
            Self::FindDupes,
            Self::Fund,
            Self::Help,
            Self::HelpSearch,
            Self::Hook,
            Self::Init,
            Self::Link,
            Self::Logout,
            Self::Ls,
            Self::Outdated,
            Self::Owner,
            Self::Pack,
            Self::Ping,
            Self::Prefix,
            Self::Profile,
            Self::Prune,
            Self::Publish,
            Self::Query,
            Self::Rebuild,
            Self::Repo,
            Self::Restart,
            Self::Root,
            Self::RunScript,
            Self::Search,
            Self::SetScript,
            Self::Shrinkwrap,
            Self::Star,
            Self::Stars,
            Self::Stop,
            Self::Team,
            Self::Token,
            Self::Uninstall,
            Self::Unpublish,
            Self::Unstar,
            Self::Update,
            Self::Version,
            Self::View,
            Self::Whoami,
        ]
    }

    /// Node package manager 명령어를 문자열로 변환
    pub fn as_str(&self) -> &str {
        match self {
            Self::Run => "run",
            Self::Install => "install",
            Self::Start => "start",
            Self::Test => "test",
            Self::Build => "build",
            Self::Access => "access",
            Self::Adduser => "adduser",
            Self::Audit => "audit",
            Self::Bin => "bin",
            Self::Bugs => "bugs",
            Self::Cache => "cache",
            Self::Ci => "ci",
            Self::Completion => "completion",
            Self::Config => "config",
            Self::Dedupe => "dedupe",
            Self::Deprecate => "deprecate",
            Self::DistTag => "dist-tag",
            Self::Docs => "docs",
            Self::Doctor => "doctor",
            Self::Edit => "edit",
            Self::Exec => "exec",
            Self::Explore => "explore",
            Self::FindDupes => "find-dupes",
            Self::Fund => "fund",
            Self::Help => "help",
            Self::HelpSearch => "help-search",
            Self::Hook => "hook",
            Self::Init => "init",
            Self::Link => "link",
            Self::Logout => "logout",
            Self::Ls => "ls",
            Self::Outdated => "outdated",
            Self::Owner => "owner",
            Self::Pack => "pack",
            Self::Ping => "ping",
            Self::Prefix => "prefix",
            Self::Profile => "profile",
            Self::Prune => "prune",
            Self::Publish => "publish",
            Self::Query => "query",
            Self::Rebuild => "rebuild",
            Self::Repo => "repo",
            Self::Restart => "restart",
            Self::Root => "root",
            Self::RunScript => "run-script",
            Self::Search => "search",
            Self::SetScript => "set-script",
            Self::Shrinkwrap => "shrinkwrap",
            Self::Star => "star",
            Self::Stars => "stars",
            Self::Stop => "stop",
            Self::Team => "team",
            Self::Token => "token",
            Self::Uninstall => "uninstall",
            Self::Unpublish => "unpublish",
            Self::Unstar => "unstar",
            Self::Update => "update",
            Self::Version => "version",
            Self::View => "view",
            Self::Whoami => "whoami",
        }
    }
}

impl std::fmt::Display for NodeCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Shell Script 실행 모드 타입 (UI 선택용)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecuteModeType {
    ScriptFile,
    ScriptText,
}

impl ExecuteModeType {
    pub const ALL: [ExecuteModeType; 2] =
        [ExecuteModeType::ScriptFile, ExecuteModeType::ScriptText];
}

impl std::fmt::Display for ExecuteModeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ExecuteModeType::ScriptFile => "Script File",
                ExecuteModeType::ScriptText => "Script Text",
            }
        )
    }
}

/// Shell Script 실행 모드
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode")]
pub enum ExecuteMode {
    /// 스크립트 파일로 실행
    ScriptFile {
        /// 스크립트 파일 경로
        script_path: String,
        /// 스크립트에 전달할 옵션
        script_options: String,
        /// 인터프리터 경로 (선택적)
        interpreter_path: Option<String>,
        /// 인터프리터 옵션 (선택적)
        interpreter_options: Option<String>,
    },
    /// 스크립트 텍스트로 직접 실행
    ScriptText {
        /// 스크립트 텍스트
        script_text: String,
    },
}

impl Default for ExecuteMode {
    fn default() -> Self {
        ExecuteMode::ScriptFile {
            script_path: String::new(),
            script_options: String::new(),
            interpreter_path: None,
            interpreter_options: None,
        }
    }
}

/// 구성 타입별 데이터
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ConfigTypeData {
    /// Application 타입 데이터
    Application {
        /// 실행할 명령어
        command: String,
        /// 명령어에 전달할 인자들
        arguments: String,
    },
    /// `ShellScript` 타입 데이터
    ShellScript {
        /// 실행 모드
        execute_mode: ExecuteMode,
    },
    /// Node package manager 타입 데이터
    Node {
        /// 프로젝트 디렉토리 (package.json 탐색 루트)
        project_directory: String,
        /// JavaScript package manager.
        #[serde(default)]
        package_manager: PackageManager,
        /// Node Runtime 경로 (None이면 시스템 기본값 사용)
        node_runtime_path: Option<String>,
        /// package manager 명령어
        command: NodeCommand,
        /// 스크립트 이름 (command가 Run일 때만 사용)
        script_name: Option<String>,
        /// 추가 인자
        arguments: String,
        /// Node 옵션 (`NODE_OPTIONS` 환경 변수)
        node_options: String,
    },
}

impl ConfigurationType {
    pub const ALL: [ConfigurationType; 3] = [
        ConfigurationType::Application,
        ConfigurationType::ShellScript,
        ConfigurationType::Node,
    ];
}

impl std::fmt::Display for ConfigurationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ConfigurationType::Application => "Application",
                ConfigurationType::ShellScript => "Shell Script",
                ConfigurationType::Node => "Node",
            }
        )
    }
}

/// 실행 구성을 나타내는 구조체
/// 프로그램 실행에 필요한 모든 설정 정보를 포함
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfiguration {
    /// 고유 식별자
    pub id: Uuid,
    /// 구성 이름
    pub name: String,
    /// 구성 타입 (`Application`, `ShellScript` 등)
    pub config_type: ConfigurationType,
    /// 작업 디렉토리 경로
    pub working_directory: String,
    /// 환경 변수 맵 (key-value)
    pub environment_variables: HashMap<String, String>,
    /// 타입별 데이터
    pub type_data: ConfigTypeData,
}

impl Default for RunConfiguration {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::from("New Configuration"),
            config_type: ConfigurationType::Application,
            working_directory: String::from("."),
            environment_variables: HashMap::new(),
            type_data: ConfigTypeData::Application {
                command: String::new(),
                arguments: String::new(),
            },
        }
    }
}

impl RunConfiguration {}
