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
    /// Kotlin 앱 실행 (JVM `java` 경유)
    Kotlin,
    /// 여러 구성을 묶어 한 번에 실행 (복합 구성)
    Compound,
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

/// Kotlin 실행 모드 타입 (UI 선택용)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KotlinLaunchModeType {
    MainClass,
    Jar,
}

impl KotlinLaunchModeType {
    pub const ALL: [KotlinLaunchModeType; 2] =
        [KotlinLaunchModeType::MainClass, KotlinLaunchModeType::Jar];
}

impl std::fmt::Display for KotlinLaunchModeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                KotlinLaunchModeType::MainClass => "Main class",
                KotlinLaunchModeType::Jar => "JAR",
            }
        )
    }
}

/// Kotlin 실행 모드
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode")]
pub enum KotlinLaunchMode {
    /// main class 실행 (`java -cp <classpath> <main_class>`)
    MainClass {
        /// 실행할 main class (예: `com.example.MainKt`)
        main_class: String,
        /// classpath (`-cp` 값). 플랫폼 구분자(`:`/`;`)는 사용자 책임
        classpath: String,
    },
    /// JAR 실행 (`java -jar <jar_path>`)
    Jar {
        /// 실행할 JAR 파일 경로
        jar_path: String,
    },
}

impl Default for KotlinLaunchMode {
    fn default() -> Self {
        KotlinLaunchMode::MainClass {
            main_class: String::new(),
            classpath: String::new(),
        }
    }
}

impl KotlinLaunchMode {
    /// 현재 모드의 타입(판별자)을 반환 (UI 셀렉터의 선택값용).
    pub fn mode_type(&self) -> KotlinLaunchModeType {
        match self {
            KotlinLaunchMode::MainClass { .. } => KotlinLaunchModeType::MainClass,
            KotlinLaunchMode::Jar { .. } => KotlinLaunchModeType::Jar,
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
    /// Kotlin 타입 데이터 — JVM(`java`)으로 Kotlin 앱 실행
    Kotlin {
        /// JDK 경로 (None이면 시스템 `java`). 디렉터리(JDK home) 또는 `java` 실행 파일 경로
        jdk_path: Option<String>,
        /// 실행 모드 (Main class / JAR)
        #[serde(default)]
        launch_mode: KotlinLaunchMode,
        /// VM options (`-Xmx2g` 등 JVM 인자)
        vm_options: String,
        /// 프로그램 인자
        program_arguments: String,
    },
    /// 복합 구성 데이터 — 함께 실행할 다른 구성들의 id 목록.
    /// 실행 시 각 멤버가 자신의 세션/페인으로 동시에 펼쳐진다.
    Compound {
        /// 함께 실행할 멤버 구성들의 id (실행 순서는 무의미 — 동시 실행)
        #[serde(default)]
        members: Vec<Uuid>,
        /// 실행 대상 workspace 탭의 base 이름. `None`/빈 문자열이면 현재 활성 탭에서 실행하고,
        /// 지정하면 그 이름으로 새 탭을 만들어(동명 충돌 시 넘버링) 거기서 실행한다.
        #[serde(default)]
        workspace: Option<String>,
    },
}

/// `ConfigTypeData::Application` 변형의 가변 필드 묶음.
pub struct ApplicationFieldsMut<'a> {
    pub command: &'a mut String,
    pub arguments: &'a mut String,
}

/// `ExecuteMode::ScriptFile` 변형의 가변 필드 묶음.
pub struct ScriptFileFieldsMut<'a> {
    pub script_path: &'a mut String,
    pub script_options: &'a mut String,
    pub interpreter_path: &'a mut Option<String>,
    pub interpreter_options: &'a mut Option<String>,
}

/// `ConfigTypeData::Node` 변형의 가변 필드 묶음.
pub struct NodeFieldsMut<'a> {
    pub package_manager: &'a mut PackageManager,
    pub node_runtime_path: &'a mut Option<String>,
    pub command: &'a mut NodeCommand,
    pub script_name: &'a mut Option<String>,
    pub arguments: &'a mut String,
    pub node_options: &'a mut String,
}

/// `ConfigTypeData::Kotlin` 변형의 공통 가변 필드 묶음 (모드 무관).
pub struct KotlinFieldsMut<'a> {
    pub jdk_path: &'a mut Option<String>,
    pub launch_mode: &'a mut KotlinLaunchMode,
    pub vm_options: &'a mut String,
    pub program_arguments: &'a mut String,
}

/// `KotlinLaunchMode::MainClass` 변형의 가변 필드 묶음.
pub struct KotlinMainClassFieldsMut<'a> {
    pub main_class: &'a mut String,
    pub classpath: &'a mut String,
}

impl ConfigTypeData {
    /// 이 데이터에 대응하는 구성 타입(판별자)을 반환.
    pub fn config_type(&self) -> ConfigurationType {
        match self {
            ConfigTypeData::Application { .. } => ConfigurationType::Application,
            ConfigTypeData::ShellScript { .. } => ConfigurationType::ShellScript,
            ConfigTypeData::Node { .. } => ConfigurationType::Node,
            ConfigTypeData::Kotlin { .. } => ConfigurationType::Kotlin,
            ConfigTypeData::Compound { .. } => ConfigurationType::Compound,
        }
    }

    /// Application 변형이면 가변 필드 묶음 반환.
    pub fn application_mut(&mut self) -> Option<ApplicationFieldsMut<'_>> {
        if let ConfigTypeData::Application { command, arguments } = self {
            Some(ApplicationFieldsMut { command, arguments })
        } else {
            None
        }
    }

    /// `ShellScript` + `ScriptFile` 변형이면 가변 필드 묶음 반환.
    pub fn script_file_mut(&mut self) -> Option<ScriptFileFieldsMut<'_>> {
        if let ConfigTypeData::ShellScript {
            execute_mode:
                ExecuteMode::ScriptFile {
                    script_path,
                    script_options,
                    interpreter_path,
                    interpreter_options,
                },
        } = self
        {
            Some(ScriptFileFieldsMut {
                script_path,
                script_options,
                interpreter_path,
                interpreter_options,
            })
        } else {
            None
        }
    }

    /// `ShellScript` + `ScriptText` 변형이면 스크립트 텍스트의 가변 참조 반환.
    pub fn script_text_mut(&mut self) -> Option<&mut String> {
        if let ConfigTypeData::ShellScript {
            execute_mode: ExecuteMode::ScriptText { script_text },
        } = self
        {
            Some(script_text)
        } else {
            None
        }
    }

    /// Node 변형이면 가변 필드 묶음 반환.
    pub fn node_mut(&mut self) -> Option<NodeFieldsMut<'_>> {
        if let ConfigTypeData::Node {
            package_manager,
            node_runtime_path,
            command,
            script_name,
            arguments,
            node_options,
            ..
        } = self
        {
            Some(NodeFieldsMut {
                package_manager,
                node_runtime_path,
                command,
                script_name,
                arguments,
                node_options,
            })
        } else {
            None
        }
    }

    /// Kotlin 변형이면 공통 가변 필드 묶음 반환.
    pub fn kotlin_mut(&mut self) -> Option<KotlinFieldsMut<'_>> {
        if let ConfigTypeData::Kotlin {
            jdk_path,
            launch_mode,
            vm_options,
            program_arguments,
        } = self
        {
            Some(KotlinFieldsMut {
                jdk_path,
                launch_mode,
                vm_options,
                program_arguments,
            })
        } else {
            None
        }
    }

    /// Kotlin + `MainClass` 변형이면 가변 필드 묶음 반환.
    pub fn kotlin_main_class_mut(&mut self) -> Option<KotlinMainClassFieldsMut<'_>> {
        if let ConfigTypeData::Kotlin {
            launch_mode:
                KotlinLaunchMode::MainClass {
                    main_class,
                    classpath,
                },
            ..
        } = self
        {
            Some(KotlinMainClassFieldsMut {
                main_class,
                classpath,
            })
        } else {
            None
        }
    }

    /// Kotlin + `Jar` 변형이면 `jar_path`의 가변 참조 반환.
    pub fn kotlin_jar_path_mut(&mut self) -> Option<&mut String> {
        if let ConfigTypeData::Kotlin {
            launch_mode: KotlinLaunchMode::Jar { jar_path },
            ..
        } = self
        {
            Some(jar_path)
        } else {
            None
        }
    }

    /// Compound 변형이면 멤버 id 목록의 가변 참조 반환.
    pub fn compound_members_mut(&mut self) -> Option<&mut Vec<Uuid>> {
        if let ConfigTypeData::Compound { members, .. } = self {
            Some(members)
        } else {
            None
        }
    }

    /// Compound 변형이면 실행 대상 workspace 탭 이름(Option)의 가변 참조 반환.
    pub fn compound_workspace_mut(&mut self) -> Option<&mut Option<String>> {
        if let ConfigTypeData::Compound { workspace, .. } = self {
            Some(workspace)
        } else {
            None
        }
    }
}

impl ConfigurationType {
    pub const ALL: [ConfigurationType; 5] = [
        ConfigurationType::Application,
        ConfigurationType::ShellScript,
        ConfigurationType::Node,
        ConfigurationType::Kotlin,
        ConfigurationType::Compound,
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
                ConfigurationType::Kotlin => "Kotlin",
                ConfigurationType::Compound => "Compound",
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
            working_directory: String::from("."),
            environment_variables: HashMap::new(),
            type_data: ConfigTypeData::Application {
                command: String::new(),
                arguments: String::new(),
            },
        }
    }
}

impl RunConfiguration {
    /// 구성 타입(판별자)을 `type_data`에서 파생. 별도 필드로 중복 저장하지 않으므로
    /// 두 표현이 어긋나는 불법 상태가 발생할 수 없다.
    pub fn config_type(&self) -> ConfigurationType {
        self.type_data.config_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotlin_config_type_data_round_trips() {
        let original = ConfigTypeData::Kotlin {
            jdk_path: Some(String::from("/opt/jdk")),
            launch_mode: KotlinLaunchMode::MainClass {
                main_class: String::from("com.example.MainKt"),
                classpath: String::from("build/libs/*"),
            },
            vm_options: String::from("-Xmx2g"),
            program_arguments: String::from("--debug"),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let restored: ConfigTypeData = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(original, restored);
        assert_eq!(restored.config_type(), ConfigurationType::Kotlin);
    }

    #[test]
    fn kotlin_jar_launch_mode_round_trips() {
        let original = KotlinLaunchMode::Jar {
            jar_path: String::from("build/libs/app.jar"),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let restored: KotlinLaunchMode = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(original, restored);
    }

    #[test]
    fn kotlin_launch_mode_defaults_to_main_class() {
        assert_eq!(
            KotlinLaunchMode::default().mode_type(),
            KotlinLaunchModeType::MainClass
        );
    }

    #[test]
    fn configuration_type_all_includes_kotlin() {
        assert!(ConfigurationType::ALL.contains(&ConfigurationType::Kotlin));
    }
}
