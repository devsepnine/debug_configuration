use serde_json::Value;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

/// SVG 아이콘을 바이너리에 embed
///
/// 컴파일 타임에 SVG 파일을 바이너리에 직접 포함시킵니다.
/// 이를 통해 단일 실행 파일만으로 배포할 수 있습니다.
// Configuration Editor
pub const ICON_FOLDER_OPEN: &[u8] = include_bytes!("../assets/mingcute--folder-open-line.svg");
pub const ICON_EDIT: &[u8] = include_bytes!("../assets/mingcute--edit-3-line.svg");
pub const ICON_DELETE: &[u8] = include_bytes!("../assets/mingcute--delete-2-fill.svg");
pub const ICON_SAVE: &[u8] = include_bytes!("../assets/mingcute--save-2-line.svg");
pub const ICON_SETTINGS: &[u8] = include_bytes!("../assets/app--settings.svg");
pub const ICON_CLOSE: &[u8] = include_bytes!("../assets/mingcute--close-fill.svg");
pub const ICON_ADD: &[u8] = include_bytes!("../assets/mingcute--add-square-line.svg");
pub const ICON_COPY: &[u8] = include_bytes!("../assets/mingcute--copy-2-line.svg");
pub const ICON_CONFIGURATIONS: &[u8] = include_bytes!("../assets/app--configurations.svg");
pub const ICON_SESSIONS: &[u8] = include_bytes!("../assets/app--sessions.svg");
pub const ICON_EXPORT: &[u8] = include_bytes!("../assets/ri--export-fill.svg");
pub const ICON_IMPORT: &[u8] = include_bytes!("../assets/ri--import-fill.svg");

// Shared sentinels
pub const DIALOG_CANCELLED: &str = "cancelled";

/// D2Coding 모노스페이스 폰트의 단일 글자(half-width) advance 너비(픽셀)를 계산.
///
/// D2Coding은 고정폭(라틴 half-width) 폰트이므로 라틴 글자 advance가 모두 동일하다.
/// 표준 너비로 'M' glyph의 advance/units_per_em 비율을 측정하며, 이 값은 폰트 고정이므로
/// 최초 1회만 파싱하여 `OnceLock`에 캐싱한다(리사이즈마다 재파싱 방지).
/// 폰트 파싱/측정 실패 시 `font_size * 0.6`로 fallback. 반환값은 항상 1.0 이상.
pub fn monospace_char_width(font_size: f32) -> f32 {
    use std::sync::OnceLock;

    /// 'M' advance / units_per_em — 폰트 불변 비율
    static ADVANCE_RATIO: OnceLock<f32> = OnceLock::new();

    let ratio = *ADVANCE_RATIO.get_or_init(|| {
        use ttf_parser::Face;

        const FALLBACK_RATIO: f32 = 0.6;

        let Ok(face) = Face::parse(crate::D2CODING_FONT, 0) else {
            return FALLBACK_RATIO;
        };
        let Some(glyph_id) = face.glyph_index('M') else {
            return FALLBACK_RATIO;
        };
        let advance_width = f32::from(face.glyph_hor_advance(glyph_id).unwrap_or(0));
        let units_per_em = f32::from(face.units_per_em());

        if units_per_em == 0.0 || advance_width == 0.0 {
            return FALLBACK_RATIO;
        }

        advance_width / units_per_em
    });

    (ratio * font_size).max(1.0)
}

/// 텍스트를 `max_width` 컬럼 이내로 자르고, 잘릴 경우 말줄임표("...")를 붙인다.
///
/// `max_width`는 half-width 컬럼 수 단위다. 한글 등 full-width 문자는 2컬럼을 소비하며,
/// 이는 D2Coding에서 full-width glyph가 half-width의 정확히 2배 너비인 것과 일치한다.
pub fn truncate_text(name: &str, max_width: usize) -> String {
    const ELLIPSIS_WIDTH: usize = 3;

    if name
        .chars()
        .map(|ch| ch.width().unwrap_or(1))
        .sum::<usize>()
        <= max_width
    {
        return name.to_owned();
    }

    let mut truncated = String::new();
    let mut current_width = 0;

    for ch in name.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if current_width + ch_width + ELLIPSIS_WIDTH > max_width {
            break;
        }
        truncated.push(ch);
        current_width += ch_width;
    }

    format!("{truncated}...")
}

// Configuration List
pub const ICON_PLAY: &[u8] = include_bytes!("../assets/mingcute--play-fill.svg");

// Configuration List - 구성 타입 뱃지 아이콘
pub const ICON_TYPE_APPLICATION: &[u8] = include_bytes!("../assets/app--type-application.svg");
pub const ICON_TYPE_SHELL: &[u8] = include_bytes!("../assets/app--type-shell.svg");
pub const ICON_TYPE_NODE: &[u8] = include_bytes!("../assets/app--type-node.svg");
pub const ICON_TYPE_KOTLIN: &[u8] = include_bytes!("../assets/app--type-kotlin.svg");
pub const ICON_TYPE_SPRING_BOOT: &[u8] = include_bytes!("../assets/app--type-springboot.svg");
pub const ICON_TYPE_COMPOUND: &[u8] = include_bytes!("../assets/app--type-compound.svg");

// Pane View
pub const ICON_REFRESH: &[u8] = include_bytes!("../assets/mingcute--refresh-1-fill.svg");
pub const ICON_STOP: &[u8] = include_bytes!("../assets/mingcute--stop-fill.svg");
pub const ICON_ARROW_DOWN_FILL: &[u8] =
    include_bytes!("../assets/mingcute--arrow-down-circle-fill.svg");
pub const ICON_ARROW_DOWN_LINE: &[u8] =
    include_bytes!("../assets/mingcute--arrow-down-circle-line.svg");
pub const ICON_PANE_MAXIMIZE: &[u8] = include_bytes!("../assets/app--pane-maximize.svg");
pub const ICON_PANE_RESTORE: &[u8] = include_bytes!("../assets/app--pane-restore.svg");
pub const ICON_SEARCH: &[u8] = include_bytes!("../assets/app--search.svg");
pub const ICON_TERMINAL_INPUT: &[u8] = include_bytes!("../assets/app--terminal-input.svg");
pub const ICON_MORE: &[u8] = include_bytes!("../assets/app--more.svg");
pub const ICON_ERASER: &[u8] = include_bytes!("../assets/mingcute--eraser-line.svg");

// ============================================================================
// Node package manager utility functions
// ============================================================================

/// `working_directory`에서 상위 디렉토리로 이동하며 `package.json` 파일 탐색
///
/// # Arguments
/// * `working_dir` - 탐색을 시작할 디렉토리
///
/// # Returns
/// * `Some(PathBuf)` - package.json 파일을 찾은 경우
/// * `None` - package.json을 찾지 못한 경우
pub fn find_package_json(working_dir: &Path) -> Option<PathBuf> {
    let mut current = working_dir;
    loop {
        let pkg = current.join("package.json");
        if pkg.exists() {
            return Some(pkg);
        }
        current = current.parent()?;
    }
}

/// package.json 파일에서 scripts 섹션 파싱
///
/// # Arguments
/// * `package_json_path` - package.json 파일 경로
///
/// # Returns
/// * `Ok(Vec<String>)` - 스크립트 이름 목록 (알파벳 순)
/// * `Err(String)` - 파싱 실패 시 에러 메시지
pub fn parse_scripts(package_json_path: &Path) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(package_json_path)
        .map_err(|e| format!("Failed to read package.json: {e}"))?;

    let json: Value =
        serde_json::from_str(&content).map_err(|e| format!("Invalid JSON in package.json: {e}"))?;

    let scripts = json
        .get("scripts")
        .and_then(|s| s.as_object())
        .map(|obj| {
            let mut keys: Vec<String> = obj.keys().cloned().collect();
            keys.sort();
            keys
        })
        .unwrap_or_default();

    Ok(scripts)
}

/// `working_directory`의 lockfile을 기반으로 패키지 매니저 감지
///
/// # Arguments
/// * `working_dir` - 확인할 디렉토리
///
/// # Returns
/// * "npm" - package-lock.json 존재
/// * "yarn" - yarn.lock 존재
/// * "pnpm" - pnpm-lock.yaml 존재
/// * "bun" - bun.lockb 또는 bun.lock 존재
/// * "npm" - lockfile이 없는 경우 기본값
pub fn detect_package_manager(working_dir: &Path) -> String {
    if working_dir.join("package-lock.json").exists() {
        return "npm".to_string();
    }
    if working_dir.join("yarn.lock").exists() {
        return "yarn".to_string();
    }
    if working_dir.join("pnpm-lock.yaml").exists() {
        return "pnpm".to_string();
    }
    if working_dir.join("bun.lockb").exists() || working_dir.join("bun.lock").exists() {
        return "bun".to_string();
    }
    "npm".to_string()
}

/// 재귀 탐색에서 제외할 디렉토리 (의존성·빌드 산출물·VCS·캐시). `package.json`과 Spring Boot
/// 모듈 스캔이 공유한다 — `build`/`target`이 빠지면 산출물 안의 사본을 모듈로 잘못 잡는다.
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    "coverage",
    ".nyc_output",
    "target",
    "vendor",
    ".cache",
    ".temp",
    ".tmp",
    "__pycache__",
    ".pytest_cache",
    ".venv",
    "venv",
];

/// `project_directory` 하위의 모든 `package.json` 파일 탐색 (재귀)
///
/// # Arguments
/// * `project_dir` - 탐색 시작 디렉토리
///
/// # Returns
/// * `Vec<PathBuf>` - 발견된 package.json 파일들의 절대 경로 목록
pub fn find_all_package_jsons(project_dir: &Path) -> Vec<PathBuf> {
    find_package_jsons_recursive(project_dir, 0, 5)
}

/// package.json 파일 재귀 탐색 (깊이 제한 포함)
fn find_package_jsons_recursive(dir: &Path, depth: usize, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();

    // 깊이 제한 초과 시 중단
    if depth > max_depth {
        return results;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // entry.file_type()는 심볼릭 링크를 따라가지 않는다(불필요한 stat도 절약).
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            // 심볼릭 링크 디렉터리는 따라가지 않는다: 순환 링크나 외부 대용량 트리(예: '/')로의
            // 링크가 느린/의도치 않은 스캔을 유발하는 것을 방지.
            if file_type.is_symlink() {
                continue;
            }

            let path = entry.path();

            // package.json 파일 발견
            if file_type.is_file() {
                if path.file_name().is_some_and(|n| n == "package.json") {
                    results.push(path);
                }
            }
            // 하위 디렉토리 재귀 탐색 (제외 목록 확인)
            else if file_type.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                // 숨김 폴더 또는 제외 목록에 있는 폴더는 스킵
                if !dir_name.starts_with('.') && !IGNORED_DIRS.contains(&dir_name) {
                    results.extend(find_package_jsons_recursive(&path, depth + 1, max_depth));
                }
            }
        }
    }

    if depth == 0 {
        results.sort();
    }
    results
}

/// Node.js runtime 경로 감지 (시스템 전체)
///
/// # Returns
/// * `Vec<(label, path)>` - 표시용 레이블과 실행 파일 경로 튜플 목록
///   예: [("Default (system)", "node"), ("v20.11.0", "C:\\...\\node.exe")]
pub fn detect_node_runtimes() -> Vec<(String, String)> {
    let mut runtimes = Vec::new();

    add_runtime(
        &mut runtimes,
        String::from("Default (system)"),
        String::from("node"),
    );

    #[cfg(target_os = "windows")]
    {
        collect_windows_node_runtimes(&mut runtimes);
    }

    #[cfg(not(target_os = "windows"))]
    {
        collect_unix_node_runtimes(&mut runtimes);
    }

    runtimes
}

fn add_runtime(runtimes: &mut Vec<(String, String)>, label: String, path: String) {
    if !runtimes
        .iter()
        .any(|(_, existing_path)| existing_path == &path)
    {
        runtimes.push((label, path));
    }
}

fn command_version(
    path: &std::path::Path,
    configure: impl FnOnce(&mut std::process::Command),
) -> Option<String> {
    let mut command = std::process::Command::new(path);
    command.arg("--version");
    configure(&mut command);

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_version_str(
    path: &str,
    configure: impl FnOnce(&mut std::process::Command),
) -> Option<String> {
    command_version(std::path::Path::new(path), configure)
}

#[cfg(target_os = "windows")]
fn collect_windows_node_runtimes(runtimes: &mut Vec<(String, String)>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/C", "where", "node"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        && output.status.success()
    {
        let paths = String::from_utf8_lossy(&output.stdout);
        for path in paths
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty() && *path != "node")
        {
            if let Some(version) = command_version_str(path, |command| {
                command.creation_flags(CREATE_NO_WINDOW);
            }) {
                add_runtime(runtimes, format!("{version} - {path}"), path.to_string());
            }
        }
    }

    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let nvm_path = PathBuf::from(user_profile).join("AppData\\Roaming\\nvm");
        collect_nvm_dir(runtimes, &nvm_path, "node.exe", "nvm", |command| {
            command.creation_flags(CREATE_NO_WINDOW);
        });
    }
}

#[cfg(not(target_os = "windows"))]
fn collect_unix_node_runtimes(runtimes: &mut Vec<(String, String)>) {
    if let Ok(output) = std::process::Command::new("sh")
        .args(["-c", "which -a node 2>/dev/null"])
        .output()
        && output.status.success()
    {
        let paths = String::from_utf8_lossy(&output.stdout);
        for path in paths
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty() && *path != "node")
        {
            if let Some(version) = command_version_str(path, |_| {}) {
                add_runtime(runtimes, format!("{version} - {path}"), path.to_string());
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let nvm_path = PathBuf::from(home).join(".nvm/versions/node");
        collect_nvm_dir(runtimes, &nvm_path, "bin/node", "nvm", |_| {});
    }
}

fn collect_nvm_dir(
    runtimes: &mut Vec<(String, String)>,
    root: &Path,
    binary_relative_path: &str,
    source: &str,
    configure: impl Copy + Fn(&mut std::process::Command),
) {
    if !root.exists() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let node_binary = path.join(binary_relative_path);
        if !node_binary.exists() {
            continue;
        }

        if let Some(version) = command_version(&node_binary, configure) {
            let path_str = node_binary.to_string_lossy().to_string();
            add_runtime(
                runtimes,
                format!("{version} ({source}) - {}", node_binary.to_string_lossy()),
                path_str,
            );
        }
    }
}

/// JDK(`java` 런타임) 경로 감지 (시스템 전체)
///
/// # Returns
/// * `Vec<(label, path)>` - 표시용 레이블과 `java` 실행 파일(또는 "java") 경로 튜플 목록
///   예: [("Default (system)", "java"), ("21.0.1 (sdkman) - /Users/.../bin/java", "/Users/.../bin/java")]
pub fn detect_jdks() -> Vec<(String, String)> {
    let mut jdks = Vec::new();

    add_runtime(
        &mut jdks,
        String::from("Default (system)"),
        String::from("java"),
    );

    #[cfg(target_os = "windows")]
    {
        collect_windows_jdks(&mut jdks);
    }

    #[cfg(target_os = "macos")]
    {
        collect_macos_jdks(&mut jdks);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        collect_linux_jdks(&mut jdks);
    }

    // 공통: JAVA_HOME + SDKMAN (해당 경로가 없으면 no-op)
    collect_env_and_sdkman_jdks(&mut jdks);

    jdks
}

/// 플랫폼별 `java` 실행 파일 이름. executor의 JDK 경로 해석에서도 재사용.
pub fn java_executable_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "java.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "java"
    }
}

/// `java -version`의 버전 문자열 추출. java는 버전을 **stderr**로 출력하므로
/// stdout만 읽는 `command_version`을 쓸 수 없다.
fn java_version(path: &Path) -> Option<String> {
    let mut command = std::process::Command::new(path);
    command.arg("-version");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stderr);
    let first_line = text.lines().next()?.trim();
    // 따옴표 안 버전 토큰 추출 (예: openjdk version "21.0.1" → 21.0.1), 없으면 첫 줄 전체
    let version = first_line
        .split('"')
        .nth(1)
        .map_or_else(|| first_line.to_string(), str::to_string);
    Some(version)
}

/// JDK home(`<home>/bin/java`)에서 `java`를 찾아 버전과 함께 목록에 추가.
fn add_jdk_from_home(jdks: &mut Vec<(String, String)>, home: &Path, source: &str) {
    let java_bin = home.join("bin").join(java_executable_name());
    if !java_bin.exists() {
        return;
    }
    if let Some(version) = java_version(&java_bin) {
        let path_str = java_bin.to_string_lossy().to_string();
        add_runtime(jdks, format!("{version} ({source}) - {path_str}"), path_str);
    }
}

/// `parent` 하위 각 디렉터리를 JDK home으로 간주하고 수집.
/// `home_suffix`는 자식에서 home까지의 상대 경로 (macOS는 "Contents/Home", 그 외는 "").
fn collect_jdks_in(
    jdks: &mut Vec<(String, String)>,
    parent: &Path,
    home_suffix: &str,
    source: &str,
) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let mut home = entry.path();
        if !home.is_dir() {
            continue;
        }
        if !home_suffix.is_empty() {
            home = home.join(home_suffix);
        }
        add_jdk_from_home(jdks, &home, source);
    }
}

/// `which -a java` (Unix PATH)로 JDK 수집.
#[cfg(unix)]
fn collect_path_jdks(jdks: &mut Vec<(String, String)>) {
    if let Ok(output) = std::process::Command::new("sh")
        .args(["-c", "which -a java 2>/dev/null"])
        .output()
        && output.status.success()
    {
        let paths = String::from_utf8_lossy(&output.stdout);
        for path in paths
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty() && *path != "java")
        {
            if let Some(version) = java_version(Path::new(path)) {
                add_runtime(jdks, format!("{version} - {path}"), path.to_string());
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn collect_macos_jdks(jdks: &mut Vec<(String, String)>) {
    collect_path_jdks(jdks);
    collect_jdks_in(
        jdks,
        Path::new("/Library/Java/JavaVirtualMachines"),
        "Contents/Home",
        "system",
    );
    if let Ok(home) = std::env::var("HOME") {
        collect_jdks_in(
            jdks,
            &PathBuf::from(home).join("Library/Java/JavaVirtualMachines"),
            "Contents/Home",
            "user",
        );
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn collect_linux_jdks(jdks: &mut Vec<(String, String)>) {
    collect_path_jdks(jdks);
    collect_jdks_in(jdks, Path::new("/usr/lib/jvm"), "", "system");
}

#[cfg(target_os = "windows")]
fn collect_windows_jdks(jdks: &mut Vec<(String, String)>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/C", "where", "java"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        && output.status.success()
    {
        let paths = String::from_utf8_lossy(&output.stdout);
        for path in paths
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty() && *path != "java")
        {
            if let Some(version) = java_version(Path::new(path)) {
                add_runtime(jdks, format!("{version} - {path}"), path.to_string());
            }
        }
    }

    for base in [
        "C:\\Program Files\\Java",
        "C:\\Program Files\\Eclipse Adoptium",
    ] {
        collect_jdks_in(jdks, Path::new(base), "", "system");
    }
}

/// `JAVA_HOME`과 SDKMAN(`~/.sdkman/candidates/java`)에서 JDK 수집 (전 플랫폼 공통).
fn collect_env_and_sdkman_jdks(jdks: &mut Vec<(String, String)>) {
    if let Ok(java_home) = std::env::var("JAVA_HOME")
        && !java_home.trim().is_empty()
    {
        add_jdk_from_home(jdks, &PathBuf::from(java_home), "JAVA_HOME");
    }

    if let Ok(home) = std::env::var("HOME") {
        let sdkman = PathBuf::from(home).join(".sdkman/candidates/java");
        collect_jdks_in(jdks, &sdkman, "", "sdkman");
    }
}

/// 절대 경로를 `project_directory` 기준 상대 경로로 변환
///
/// # Arguments
/// * `absolute_path` - 절대 경로
/// * `project_dir` - 기준 디렉토리
///
/// # Returns
/// * 상대 경로 문자열 (예: "./package.json", "packages/app1/package.json")
pub fn to_relative_path(absolute_path: &Path, project_dir: &Path) -> String {
    let relative = absolute_path
        .strip_prefix(project_dir)
        .ok()
        .and_then(|p| p.to_str());

    let raw = match relative {
        // project_dir == absolute_path (스스로의 package.json)
        Some("") => return String::from("./package.json"),
        Some(path) => path.to_string(),
        // strip_prefix 실패 (다른 드라이브 등): 절대 경로 그대로
        None => absolute_path.to_string_lossy().to_string(),
    };

    // 성공/실패 경로 모두 동일하게 구분자 정규화 (Windows 백슬래시 → 슬래시)
    raw.replace('\\', "/")
}

/// Spring Boot 모듈 경로 표기. Gradle은 프로젝트 경로(`:app:api`), Maven은 `-pl`에 줄 상대 경로
/// (`app/api`)다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpringModuleFlavor {
    Gradle,
    Maven,
}

/// 후보 스캔 결과(Spring 모듈·Kotlin main class·JAR 공용). `truncated`가 참이면 탐색 상한에
/// 걸려 일부를 놓쳤을 수 있다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateScan {
    pub items: Vec<String>,
    pub truncated: bool,
}

/// 모듈 디렉터리 탐색 깊이 상한 (`app/pspteller`류 중첩을 여유 있게 덮는다).
const SPRING_SCAN_MAX_MODULE_DEPTH: usize = 6;
/// 깊이 상한 밖에 모듈이 더 있는지 살필 때 추가로 내려가는 깊이.
const SCAN_MAX_PROBE_DEPTH: usize = 6;
/// `src/main/{kotlin,java}` 안 패키지 디렉터리 깊이 상한.
const SPRING_SCAN_MAX_SOURCE_DEPTH: usize = 12;
/// 한 번의 스캔에서 훑는 디렉터리 항목 수 상한 — 거대한 저장소에서 UI 작업이 길어지지 않게 한다.
const SCAN_MAX_ENTRIES: usize = 20_000;
/// 읽어볼 소스 파일 크기 상한. 생성 코드 같은 큰 파일 전체를 메모리에 올리지 않는다.
const SOURCE_MAX_BYTES: u64 = 256 * 1024;
/// 실행 가능한 앱의 표지. Boot 플러그인 적용 여부는 main class가 없는 모듈에도 붙으므로 신호가
/// 되지 못한다.
const SPRING_BOOT_MARKER: &str = "@SpringBootApplication";

/// `root` 아래에서 `@SpringBootApplication`이 있는 모듈을 찾는다.
///
/// 디렉터리에 `src/main/{kotlin,java}`가 있으면 그 부모를 모듈 후보로 보고 소스만 살핀다.
/// 루트 자체가 앱이면 빈 문자열이다. 디렉터리 이름이 모듈 값으로 쓸 수 없는 형태(공백 등)면
/// 실행 단계에서 거절될 값을 만들지 않도록 결과에서 뺀다. 심볼릭 링크는 따라가지 않는다.
pub fn scan_spring_boot_modules(root: &Path, flavor: SpringModuleFlavor) -> CandidateScan {
    let mut scan = SpringScan {
        flavor,
        budget: ScanBudget::new(),
        modules: Vec::new(),
    };
    scan.walk_modules(root, &mut Vec::new(), 0);
    scan.modules.sort();
    scan.modules.dedup();
    CandidateScan {
        items: scan.modules,
        truncated: scan.budget.truncated,
    }
}

/// 후보 스캔들이 공유하는 방문 예산과 "놓쳤을 수 있음" 표지.
struct ScanBudget {
    remaining: usize,
    truncated: bool,
}

impl ScanBudget {
    fn new() -> Self {
        Self {
            remaining: SCAN_MAX_ENTRIES,
            truncated: false,
        }
    }

    /// 항목 하나를 방문할 예산을 쓴다. 바닥나면 `truncated`를 세우고 거절한다.
    fn spend(&mut self) -> bool {
        if self.remaining == 0 {
            self.truncated = true;
            return false;
        }
        self.remaining -= 1;
        true
    }
}

struct SpringScan {
    flavor: SpringModuleFlavor,
    budget: ScanBudget,
    modules: Vec<String>,
}

impl SpringScan {
    fn spend(&mut self) -> bool {
        self.budget.spend()
    }

    fn walk_modules(&mut self, dir: &Path, relative: &mut Vec<String>, depth: usize) {
        if depth > SPRING_SCAN_MAX_MODULE_DEPTH {
            // 깊어서 건너뛴 곳 아래에 모듈처럼 보이는 디렉터리가 있을 때만 놓칠 수 있다고 알린다.
            // `docs/` 같은 비JVM 디렉터리가 깊다는 이유만으로 경고하면 모듈을 놓치지 않은
            // 프로젝트에도 뜬다.
            if probe_for_module(dir, 0, &mut self.budget) {
                self.budget.truncated = true;
            }
            return;
        }
        if self.has_spring_boot_application(&dir.join("src").join("main")) {
            self.push_module(relative);
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if !self.spend() {
                return;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            // `src`는 위에서 이미 소스로 살폈다. 그 안을 모듈 후보로 다시 재귀하지 않는다.
            if name.starts_with('.') || name == "src" || IGNORED_DIRS.contains(&name) {
                continue;
            }
            relative.push(name.to_string());
            self.walk_modules(&entry.path(), relative, depth + 1);
            relative.pop();
        }
    }

    fn push_module(&mut self, relative: &[String]) {
        let value = match self.flavor {
            SpringModuleFlavor::Gradle if relative.is_empty() => String::new(),
            SpringModuleFlavor::Gradle => format!(":{}", relative.join(":")),
            SpringModuleFlavor::Maven => relative.join("/"),
        };
        if crate::models::is_valid_spring_module(&value) {
            self.modules.push(value);
        }
    }

    fn has_spring_boot_application(&mut self, src_main: &Path) -> bool {
        // `src`·`main`·언어 디렉터리 자체가 링크일 수 있다. 자식 항목만 검사하면 링크된 `src`가
        // 트리 밖을 가리킬 때 그대로 따라간다.
        if !src_main.parent().is_some_and(is_real_dir) || !is_real_dir(src_main) {
            return false;
        }
        ["kotlin", "java"].iter().any(|language| {
            let dir = src_main.join(language);
            is_real_dir(&dir) && self.search_sources(&dir, 0)
        })
    }

    fn search_sources(&mut self, dir: &Path, depth: usize) -> bool {
        if depth > SPRING_SCAN_MAX_SOURCE_DEPTH {
            self.budget.truncated = true;
            return false;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            if !self.spend() {
                return false;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if self.search_sources(&path, depth + 1) {
                    return true;
                }
            } else if file_type.is_file() && is_jvm_source(&path) && contains_marker(&path) {
                return true;
            }
        }
        false
    }
}

/// `dir` 아래(자신 포함)에 모듈처럼 보이는 디렉터리가 있는지 예산 안에서 얕게 살핀다. 깊이 상한
/// 밖에 모듈이 더 있었는지 가늠하는 데 쓴다.
fn probe_for_module(dir: &Path, extra_depth: usize, budget: &mut ScanBudget) -> bool {
    if looks_like_module(dir) {
        return true;
    }
    if extra_depth >= SCAN_MAX_PROBE_DEPTH {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !budget.spend() {
            return false;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || IGNORED_DIRS.contains(&name) {
            continue;
        }
        if probe_for_module(&entry.path(), extra_depth + 1, budget) {
            return true;
        }
    }
    false
}

/// 심볼릭 링크가 아닌 실제 디렉터리인지.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_dir())
}

/// 모듈 루트로 보이는 디렉터리인지 (`src/main` 또는 빌드 파일). 깊이 상한으로 건너뛴 디렉터리에
/// 모듈이 있었을 가능성을 가늠하는 데 쓴다.
fn looks_like_module(dir: &Path) -> bool {
    dir.join("src").join("main").is_dir()
        || [
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "pom.xml",
        ]
        .iter()
        .any(|name| dir.join(name).is_file())
}

fn is_jvm_source(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "kt" || ext == "java")
}

fn contains_marker(path: &Path) -> bool {
    let small_enough = std::fs::metadata(path)
        .map(|meta| meta.len() <= SOURCE_MAX_BYTES)
        .unwrap_or(false);
    // 바이트로 읽는다: EUC-KR/CP949 주석이 든 소스도 `read_to_string`은 UTF-8 오류로 버려 앱을
    // 놓친다. 마커는 ASCII라 인코딩과 BOM에 상관없이 찾을 수 있다.
    small_enough && std::fs::read(path).is_ok_and(|bytes| has_spring_boot_annotation(&bytes))
}

/// 소스 바이트에 실제 `@SpringBootApplication` 사용이 있는지. 뒤에 식별자 문자가 이어지는
/// 이름(`@SpringBootApplicationX`)과 주석 줄(`//`, `*`, `/*`)의 언급은 제외한다. 문자열 안의
/// 언급까지는 가려내지 못하지만, 자동 채움은 후보가 정확히 하나일 때만 일어난다.
fn has_spring_boot_annotation(bytes: &[u8]) -> bool {
    let marker = SPRING_BOOT_MARKER.as_bytes();
    bytes
        .windows(marker.len())
        .enumerate()
        .filter(|(_, window)| *window == marker)
        .any(|(at, _)| {
            let after = bytes.get(at + marker.len());
            let continues_identifier =
                after.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
            let line_start = bytes[..at]
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |newline| newline + 1);
            let before_marker = &bytes[line_start..at];
            let trimmed = before_marker
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .map_or(&[][..], |first| &before_marker[first..]);
            let in_comment = trimmed.starts_with(b"//")
                || trimmed.starts_with(b"*")
                || trimmed.starts_with(b"/*");
            !continues_identifier && !in_comment
        })
}

/// 소스에서 main class를 찾을 때 내려가는 디렉터리 깊이 상한. 모듈 중첩(3~4단계)에 `src/main/kotlin`
/// 과 긴 패키지 이름(10단계 안팎)이 더해져도 닿지 않도록 넉넉히 잡는다 — 멀티모듈 Kotlin
/// 프로젝트에서 12는 패키지 디렉터리 도중에 끊겼다.
const KOTLIN_SCAN_MAX_DEPTH: usize = 24;

/// `src` 바로 아래의 디렉터리가 테스트 소스 세트인지. Gradle/멀티플랫폼은 `jvmTest`,
/// `integrationTest`, `testFixtures`처럼 이름 끝이나 전체가 test인 세트를 쓴다.
fn is_test_source_set(name: &str) -> bool {
    matches!(name, "test" | "tests" | "androidTest" | "testFixtures")
        || name.ends_with("Test")
        || name.ends_with("Tests")
}
/// JAR 산출물 탐색에서 모듈 디렉터리를 내려가는 깊이 상한.
const JAR_SCAN_MAX_DEPTH: usize = 6;
/// 배포용이 아닌 부산물 JAR의 접미사.
const AUXILIARY_JAR_SUFFIXES: &[&str] = &[
    "-plain.jar",
    "-sources.jar",
    "-javadoc.jar",
    "-tests.jar",
    "-test-fixtures.jar",
];

/// `root` 아래 `.kt`/`.java` 소스에서 `main` 진입점을 가진 클래스의 완전한 이름을 찾는다.
///
/// `src/main` 레이아웃에 묶지 않는다 — `kotlinc` 단일 파일이나 임의 폴더 구조도 쓰기 때문이다.
/// 테스트 소스(`src/test` 등)와 빌드 산출물·숨김 디렉터리는 건너뛰고 심볼릭 링크는 따라가지
/// 않는다.
pub fn scan_kotlin_main_classes(root: &Path) -> CandidateScan {
    let mut budget = ScanBudget::new();
    let mut found = Vec::new();
    walk_main_class_sources(root, 0, &mut budget, &mut found);
    found.sort();
    found.dedup();
    CandidateScan {
        items: found,
        truncated: budget.truncated,
    }
}

fn walk_main_class_sources(
    dir: &Path,
    depth: usize,
    budget: &mut ScanBudget,
    found: &mut Vec<String>,
) {
    if depth > KOTLIN_SCAN_MAX_DEPTH {
        budget.truncated = true;
        return;
    }
    let inside_src = dir.file_name().is_some_and(|name| name == "src");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !budget.spend() {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let is_test_source = inside_src && is_test_source_set(name);
            if name.starts_with('.') || IGNORED_DIRS.contains(&name) || is_test_source {
                continue;
            }
            walk_main_class_sources(&path, depth + 1, budget, found);
        } else if file_type.is_file() && is_jvm_source(&path) {
            read_main_class(&path, found);
        }
    }
}

fn read_main_class(path: &Path, found: &mut Vec<String>) {
    let (Some(stem), Some(extension)) = (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) else {
        return;
    };
    let small_enough = std::fs::metadata(path)
        .map(|meta| meta.len() <= SOURCE_MAX_BYTES)
        .unwrap_or(false);
    if !small_enough {
        return;
    }
    if let Some(class) = std::fs::read(path)
        .ok()
        .and_then(|bytes| parse_main_class(&bytes, stem, extension))
    {
        found.push(class);
    }
}

/// 소스 하나에서 `main` 진입점을 가진 클래스의 완전한 이름을 뽑는다.
///
/// Kotlin은 들여쓰기 없는 최상위 `fun main(`만 본다(`private`은 진입점이 아니다). 클래스 이름은
/// `@file:JvmName("X")`이 있으면 `X`, 없으면 파일 이름에서 만든 `<이름>Kt`다. Java는
/// `public static void main(`을 찾고 클래스 이름은 **파일 이름**으로 가정한다 — 중첩 클래스나 파일
/// 이름과 다른 클래스 안의 `main`은 잘못된 이름으로 나올 수 있어, 후보는 실행 전에 확인해야 한다.
/// `object` 안 `@JvmStatic`이나 companion object의 main은 찾지 못하고, 줄 처음에서 시작하는
/// 주석(`//`, `/* ... */`, `*`)의 언급은 무시하지만 문자열 리터럴 안의 언급은 가려내지 못한다.
/// Java 선언은 줄 처음에서 시작해야 하므로 `class A { public static void main(...) {`처럼 클래스
/// 선언과 같은 줄에 쓴 `main`이나 같은 줄의 어노테이션 뒤 선언은 찾지 못한다.
fn parse_main_class(bytes: &[u8], file_stem: &str, extension: &str) -> Option<String> {
    // 비ASCII 주석(CP949 등)이 있어도 ASCII인 선언 줄은 그대로 남는다.
    let text = String::from_utf8_lossy(bytes);
    let mut package = String::new();
    let mut jvm_name: Option<String> = None;
    let mut has_main = false;
    let mut in_block_comment = false;

    for (index, raw) in text.lines().enumerate() {
        let line = if index == 0 {
            raw.trim_start_matches('\u{feff}')
        } else {
            raw
        };
        let mut trimmed = line.trim();
        let mut line = line;
        // 줄 처음에서 열린 블록 주석은 닫는 표지까지 버리되, 같은 줄의 그 뒤 코드는 남긴다
        // (`/* c */ fun main() {}`). 주석 뒤 코드는 사실상 줄 처음의 선언이므로 들여쓰기 없는
        // 것으로 본다.
        if in_block_comment || trimmed.starts_with("/*") {
            match trimmed.find("*/") {
                Some(end) => {
                    in_block_comment = false;
                    trimmed = trimmed[end + 2..].trim();
                    line = trimmed;
                    if trimmed.is_empty() {
                        continue;
                    }
                }
                None => {
                    in_block_comment = true;
                    continue;
                }
            }
        }
        if trimmed.starts_with("//") || trimmed.starts_with('*') {
            continue;
        }
        if package.is_empty()
            && let Some(rest) = trimmed
                .strip_prefix("package")
                .filter(|rest| rest.starts_with(char::is_whitespace))
        {
            // 주석을 먼저 떼고 나서 `;`를 정리해야 `package a.b; // c`가 `a.b`가 된다.
            package = rest
                .split("//")
                .next()
                .unwrap_or("")
                .split("/*")
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_string();
        } else if let Some(rest) = trimmed.strip_prefix("@file:JvmName(") {
            jvm_name = rest
                .split('"')
                .nth(1)
                .filter(|name| !name.is_empty())
                .map(str::to_string);
        } else if !has_main {
            has_main = match extension {
                "kt" => is_kotlin_top_level_main(line),
                "java" => is_java_main(trimmed),
                _ => false,
            };
        }
    }
    if !has_main {
        return None;
    }

    let class = match extension {
        "kt" => jvm_name.unwrap_or_else(|| format!("{}Kt", kotlin_file_class_stem(file_stem))),
        _ => file_stem.to_string(),
    };
    Some(if package.is_empty() {
        class
    } else {
        format!("{package}.{class}")
    })
}

/// Java 진입점 선언인지: `public`과 `static`을 갖고(순서·`final` 등은 무관) `void main(`으로
/// 이어지는 줄. `void`와 `main` 사이, `main`과 `(` 사이 공백은 허용한다.
fn is_java_main(trimmed: &str) -> bool {
    let mut modifiers = Vec::new();
    let mut tokens = trimmed.split_whitespace();
    for token in tokens.by_ref() {
        if token == "void" {
            let is_entry = match tokens.next() {
                Some("main") => tokens.next().is_some_and(|next| next.starts_with('(')),
                Some(name) => name.starts_with("main("),
                None => false,
            };
            return is_entry && modifiers.contains(&"public") && modifiers.contains(&"static");
        }
        if !matches!(
            token,
            "public" | "static" | "final" | "synchronized" | "strictfp"
        ) {
            return false;
        }
        modifiers.push(token);
    }
    false
}

/// 들여쓰기 없는 최상위 `fun main(`인지. `private`은 JVM 진입점이 되지 못한다.
fn is_kotlin_top_level_main(line: &str) -> bool {
    if line.starts_with(char::is_whitespace) {
        return false;
    }
    let mut tokens = line.split_whitespace().peekable();
    while tokens
        .peek()
        .is_some_and(|token| matches!(*token, "public" | "internal" | "suspend"))
    {
        tokens.next();
    }
    if tokens.next() != Some("fun") {
        return false;
    }
    match tokens.next() {
        // `fun main (args: ...)`처럼 이름과 여는 괄호 사이에 공백이 있는 경우
        Some("main") => tokens.next().is_some_and(|token| token.starts_with('(')),
        Some(name) => name.starts_with("main("),
        None => false,
    }
}

/// Kotlin이 최상위 선언을 담는 파일 클래스의 이름 규칙: 첫 글자를 대문자로 하고 식별자로 쓸 수
/// 없는 문자는 `_`로 바꾼다 (`my-app.kt` -> `My_appKt`). 유니코드 글자·숫자는 그대로 두고
/// (`메인.kt` -> `메인Kt`), 숫자로 시작하면 `_`를 앞에 붙인다.
fn kotlin_file_class_stem(file_stem: &str) -> String {
    let mut class = String::new();
    for (index, c) in file_stem.chars().enumerate() {
        let c = if c.is_alphanumeric() || c == '_' {
            c
        } else {
            '_'
        };
        if index == 0 {
            if c.is_ascii_digit() {
                class.push('_');
            }
            // 컴파일러는 한 글자 대문자만 쓴다: `ß`처럼 여러 글자(`SS`)로 늘어나는 경우는 그대로 둔다.
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => class.push(single),
                _ => class.push(c),
            }
        } else {
            class.push(c);
        }
    }
    class
}

/// `root` 아래 빌드 산출물 폴더(`build/libs`, `target`)의 실행 가능한 JAR을 찾는다. 경로는 `root`
/// 기준 상대 경로(`/` 구분, `./` 없음)라 작업 디렉터리에서 실행할 때 그대로 유효하다.
///
/// `build`와 `target`은 일반 탐색에서 건너뛰는 디렉터리라 이름으로 직접 골라 그 안만 본다.
/// `-plain`·`-sources`·`-javadoc`·`-tests`와 `original-` 부산물은 제외한다.
pub fn scan_jar_artifacts(root: &Path) -> CandidateScan {
    let mut budget = ScanBudget::new();
    let mut found = Vec::new();
    walk_jar_artifacts(root, &mut Vec::new(), 0, &mut budget, &mut found);
    found.sort();
    found.dedup();
    CandidateScan {
        items: found,
        truncated: budget.truncated,
    }
}

fn walk_jar_artifacts(
    dir: &Path,
    relative: &mut Vec<String>,
    depth: usize,
    budget: &mut ScanBudget,
    found: &mut Vec<String>,
) {
    if depth > JAR_SCAN_MAX_DEPTH {
        // 산출물 폴더가 있을 법한 모듈 디렉터리를 놓쳤을 때만 알린다.
        if probe_for_module(dir, 0, budget) {
            budget.truncated = true;
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !budget.spend() {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        match name {
            "build" => collect_jars(
                &entry.path().join("libs"),
                relative,
                "build/libs",
                budget,
                found,
            ),
            "target" => collect_jars(&entry.path(), relative, "target", budget, found),
            // 소스 트리에는 산출물이 없고 패키지 디렉터리가 깊어 깊이 상한만 소모한다.
            _ if name == "src" || name.starts_with('.') || IGNORED_DIRS.contains(&name) => {}
            _ => {
                relative.push(name.to_string());
                walk_jar_artifacts(&entry.path(), relative, depth + 1, budget, found);
                relative.pop();
            }
        }
    }
}

fn collect_jars(
    dir: &Path,
    relative: &[String],
    output_dir: &str,
    budget: &mut ScanBudget,
    found: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !budget.spend() {
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let lowered = name.to_ascii_lowercase();
        let is_auxiliary = lowered.starts_with("original-")
            || AUXILIARY_JAR_SUFFIXES
                .iter()
                .any(|suffix| lowered.ends_with(suffix));
        if !lowered.ends_with(".jar") || is_auxiliary {
            continue;
        }
        let mut parts = relative.to_vec();
        parts.push(output_dir.to_string());
        parts.push(name.to_string());
        found.push(parts.join("/"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_jdks_always_includes_system_default() {
        let jdks = detect_jdks();
        assert!(
            jdks.iter()
                .any(|(label, path)| label == "Default (system)" && path == "java"),
            "detect_jdks must always include the system default entry"
        );
    }

    #[test]
    fn truncate_text_keeps_short_strings() {
        assert_eq!(truncate_text("", 10), "");
        assert_eq!(truncate_text("hello", 10), "hello");
        assert_eq!(truncate_text("hello", 5), "hello"); // 정확히 max_width면 말줄임 없음
    }

    #[test]
    fn truncate_text_appends_ellipsis_reserving_three_columns() {
        // "hello world"(11컬럼) > 8 → content는 max_width-3=5컬럼까지 → "hello..."
        assert_eq!(truncate_text("hello world", 8), "hello...");
    }

    #[test]
    fn truncate_text_accounts_for_full_width_columns() {
        // 한글은 2컬럼: "가나다" = 6컬럼
        assert_eq!(truncate_text("가나다", 10), "가나다");
        // max 5: "가"(2) 유지, "나" 추가 시 2+2+3=7>5 → "가..."
        assert_eq!(truncate_text("가나다", 5), "가...");
    }

    #[test]
    fn truncate_text_below_ellipsis_width_yields_ellipsis_only() {
        // max_width < ELLIPSIS_WIDTH(3): 첫 글자부터 break → "..."
        assert_eq!(truncate_text("hello", 2), "...");
    }

    #[test]
    fn to_relative_path_child_uses_forward_slashes() {
        let project = Path::new("/proj");
        let abs = project.join("packages").join("app").join("package.json");
        assert_eq!(to_relative_path(&abs, project), "packages/app/package.json");
    }

    #[test]
    fn to_relative_path_self_returns_dot_package_json() {
        let project = Path::new("/proj");
        assert_eq!(to_relative_path(project, project), "./package.json");
    }

    #[test]
    fn to_relative_path_outside_returns_normalized_absolute() {
        let project = Path::new("/proj");
        let abs = Path::new("/other/app/package.json");
        assert_eq!(to_relative_path(abs, project), "/other/app/package.json");
    }
    /// 임시 디렉터리에 격리된 프로젝트 트리. 사용자 홈이나 실제 프로젝트를 건드리지 않는다.
    struct SpringTree(PathBuf);

    impl SpringTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("rcm-scan-{tag}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("file");
        }

        fn app(&self, module_dir: &str) {
            let prefix = if module_dir.is_empty() {
                String::new()
            } else {
                format!("{module_dir}/")
            };
            self.write(
                &format!("{prefix}src/main/kotlin/com/example/App.kt"),
                "@SpringBootApplication\nclass App\n",
            );
        }

        fn scan(&self, flavor: SpringModuleFlavor) -> CandidateScan {
            scan_spring_boot_modules(&self.0, flavor)
        }
    }

    impl Drop for SpringTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn spring_scan_reports_a_root_app_as_the_empty_module() {
        let tree = SpringTree::new("root");
        tree.app("");

        assert_eq!(tree.scan(SpringModuleFlavor::Gradle).items, vec![""]);
        assert_eq!(tree.scan(SpringModuleFlavor::Maven).items, vec![""]);
    }

    #[test]
    fn spring_scan_finds_only_modules_with_a_boot_application() {
        let tree = SpringTree::new("multi");
        tree.app("app/pspteller");
        tree.app("app/schemashifter");
        // Boot 플러그인만 있고 main class가 없는 모듈은 후보가 아니다.
        tree.write(
            "adapter/build.gradle.kts",
            "plugins { id(\"org.springframework.boot\") }",
        );
        tree.write("adapter/src/main/kotlin/Lib.kt", "class Lib");
        tree.write("core/src/main/kotlin/Core.kt", "class Core");

        let gradle = tree.scan(SpringModuleFlavor::Gradle);
        let maven = tree.scan(SpringModuleFlavor::Maven);

        assert_eq!(gradle.items, vec![":app:pspteller", ":app:schemashifter"]);
        assert_eq!(maven.items, vec!["app/pspteller", "app/schemashifter"]);
        assert!(!gradle.truncated);
    }

    #[test]
    fn spring_scan_reads_java_sources_too() {
        let tree = SpringTree::new("java");
        tree.write(
            "api/src/main/java/com/example/Api.java",
            "@SpringBootApplication\npublic class Api {}\n",
        );

        assert_eq!(tree.scan(SpringModuleFlavor::Gradle).items, vec![":api"]);
    }

    #[test]
    fn spring_scan_ignores_test_sources_and_build_output() {
        let tree = SpringTree::new("ignored");
        tree.write(
            "app/src/test/kotlin/TestApp.kt",
            "@SpringBootApplication\nclass TestApp",
        );
        tree.write(
            "app/build/generated/src/main/kotlin/Gen.kt",
            "@SpringBootApplication\nclass Gen",
        );
        tree.write(
            "node_modules/x/src/main/kotlin/X.kt",
            "@SpringBootApplication\nclass X",
        );

        assert!(tree.scan(SpringModuleFlavor::Gradle).items.is_empty());
    }

    #[test]
    fn spring_scan_skips_oversized_sources_and_unusable_module_names() {
        let tree = SpringTree::new("limits");
        let huge = format!(
            "@SpringBootApplication\n{}",
            "x".repeat(usize::try_from(SOURCE_MAX_BYTES).unwrap() + 1)
        );
        tree.write("big/src/main/kotlin/Big.kt", &huge);
        // 공백이 든 디렉터리 이름은 실행 단계에서 거절될 값이라 결과에서 뺀다.
        tree.app("my app");

        assert!(tree.scan(SpringModuleFlavor::Gradle).items.is_empty());
    }

    #[test]
    fn spring_scan_reports_truncation_when_the_depth_cap_hides_modules() {
        let tree = SpringTree::new("deep");
        tree.app("a/b/c/d/e/f/g/h");

        let scan = tree.scan(SpringModuleFlavor::Gradle);

        assert!(scan.items.is_empty());
        assert!(scan.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn spring_scan_does_not_follow_symlinks() {
        let outside = SpringTree::new("outside");
        outside.app("");
        let tree = SpringTree::new("link");
        std::os::unix::fs::symlink(&outside.0, tree.0.join("linked")).expect("symlink");

        assert!(tree.scan(SpringModuleFlavor::Gradle).items.is_empty());
    }

    #[test]
    fn spring_scan_finds_an_app_in_a_non_utf8_source_with_a_bom() {
        let tree = SpringTree::new("encoding");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"// ");
        bytes.extend_from_slice(&[0xC7, 0xD1, 0xB1, 0xDB]); // CP949 "한글"
        bytes.extend_from_slice(b"\n@SpringBootApplication\nclass App\n");
        let path = tree.0.join("api/src/main/kotlin/App.kt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();

        assert_eq!(tree.scan(SpringModuleFlavor::Gradle).items, vec![":api"]);
    }

    #[test]
    fn spring_scan_ignores_mentions_in_comments_and_longer_names() {
        let tree = SpringTree::new("mentions");
        tree.write(
            "docs-only/src/main/kotlin/A.kt",
            "// @SpringBootApplication is documented elsewhere\n * @SpringBootApplication\n\
             /* @SpringBootApplication */\nclass A\n",
        );
        tree.write(
            "custom/src/main/kotlin/B.kt",
            "@SpringBootApplicationExtra\nclass B\n",
        );
        tree.write(
            "real/src/main/kotlin/C.kt",
            "  @SpringBootApplication(scanBasePackages = [\"x\"])\nclass C\n",
        );

        assert_eq!(tree.scan(SpringModuleFlavor::Gradle).items, vec![":real"]);
    }

    #[cfg(unix)]
    #[test]
    fn spring_scan_does_not_follow_a_symlinked_src_directory() {
        let outside = SpringTree::new("outside-src");
        outside.write(
            "src/main/kotlin/App.kt",
            "@SpringBootApplication\nclass App\n",
        );
        let tree = SpringTree::new("link-src");
        std::fs::create_dir_all(tree.0.join("app")).unwrap();
        std::os::unix::fs::symlink(outside.0.join("src"), tree.0.join("app/src")).unwrap();

        assert!(tree.scan(SpringModuleFlavor::Gradle).items.is_empty());
    }

    #[test]
    fn spring_scan_reports_truncation_when_the_entry_budget_runs_out() {
        let tree = SpringTree::new("budget");
        for index in 0..10 {
            tree.write(&format!("m{index}/README.md"), "x");
        }
        let mut scan = SpringScan {
            flavor: SpringModuleFlavor::Gradle,
            budget: ScanBudget {
                remaining: 3,
                truncated: false,
            },
            modules: Vec::new(),
        };

        scan.walk_modules(&tree.0, &mut Vec::new(), 0);

        assert!(scan.budget.truncated);
    }

    #[test]
    fn spring_scan_only_warns_about_depth_when_the_skipped_directory_looks_like_a_module() {
        let plain = SpringTree::new("deep-docs");
        plain.write("a/b/c/d/e/f/g/h/notes.md", "x");
        assert!(!plain.scan(SpringModuleFlavor::Gradle).truncated);

        let module = SpringTree::new("deep-module");
        module.write("a/b/c/d/e/f/g/h/build.gradle.kts", "");
        assert!(module.scan(SpringModuleFlavor::Gradle).truncated);
    }

    // ---- Kotlin main class / JAR 스캔 ----

    fn mains(tree: &SpringTree) -> Vec<String> {
        scan_kotlin_main_classes(&tree.0).items
    }

    #[test]
    fn main_class_of_a_top_level_kotlin_main_uses_the_file_class_name() {
        let tree = SpringTree::new("kt-main");
        tree.write(
            "src/main/kotlin/com/example/Main.kt",
            "package com.example\n\nfun main(args: Array<String>) {}\n",
        );

        assert_eq!(mains(&tree), vec!["com.example.MainKt"]);
    }

    #[test]
    fn main_class_without_a_package_is_the_bare_file_class() {
        let tree = SpringTree::new("kt-default");
        tree.write("Main.kt", "fun main() {}\n");
        tree.write("Spaced.kt", "suspend fun main (args: Array<String>) {}\n");

        assert_eq!(mains(&tree), vec!["MainKt", "SpacedKt"]);
    }

    #[test]
    fn main_class_honors_file_jvm_name_and_sanitizes_file_names() {
        let tree = SpringTree::new("kt-jvmname");
        tree.write(
            "a/App.kt",
            "@file:JvmName(\"Launcher\")\npackage com.x\nfun main() {}\n",
        );
        tree.write("b/my-app.kt", "package com.y\nfun main() {}\n");

        assert_eq!(mains(&tree), vec!["com.x.Launcher", "com.y.My_appKt"]);
    }

    #[test]
    fn main_class_skips_private_indented_and_lookalike_mains() {
        let tree = SpringTree::new("kt-negative");
        tree.write("A.kt", "private fun main() {}\n");
        tree.write("B.kt", "class B {\n    fun main() {}\n}\n");
        tree.write("C.kt", "fun mainly() {}\nfun main2() {}\n");
        tree.write("D.kt", "// fun main() {}\n/*\nfun main() {}\n*/\n");

        assert!(mains(&tree).is_empty());
    }

    #[test]
    fn main_class_reads_java_entry_points_and_ignores_other_methods() {
        let tree = SpringTree::new("java-main");
        tree.write(
            "src/main/java/com/example/App.java",
            "package com.example;\npublic class App {\n    public static void main(String[] args) {}\n}\n",
        );
        tree.write(
            "src/main/java/com/example/Util.java",
            "package com.example;\nclass Util { static void main2() {} }\n",
        );

        // Java의 main은 클래스 안이라 들여쓰기가 있어도 진입점이다.
        assert_eq!(mains(&tree), vec!["com.example.App"]);
    }

    #[test]
    fn main_class_scan_skips_tests_build_output_and_hidden_directories() {
        let tree = SpringTree::new("kt-skip");
        tree.write("src/test/kotlin/T.kt", "fun main() {}\n");
        tree.write("build/generated/G.kt", "fun main() {}\n");
        tree.write(".hidden/H.kt", "fun main() {}\n");
        // 패키지 이름이 `test`인 디렉터리는 `src` 바로 아래가 아니므로 건너뛰지 않는다.
        tree.write(
            "src/main/kotlin/com/test/Real.kt",
            "package com.test\nfun main() {}\n",
        );

        assert_eq!(mains(&tree), vec!["com.test.RealKt"]);
    }

    #[test]
    fn main_class_scan_reads_bom_and_non_utf8_sources() {
        let tree = SpringTree::new("kt-encoding");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"package com.k\n// ");
        bytes.extend_from_slice(&[0xC7, 0xD1, 0xB1, 0xDB]); // CP949 "한글"
        bytes.extend_from_slice(b"\nfun main() {}\n");
        let path = tree.0.join("Main.kt");
        std::fs::write(path, bytes).unwrap();

        assert_eq!(mains(&tree), vec!["com.k.MainKt"]);
    }

    #[test]
    fn main_class_scan_skips_oversized_sources() {
        let tree = SpringTree::new("kt-big");
        let body = "x".repeat(usize::try_from(SOURCE_MAX_BYTES).unwrap() + 1);
        tree.write("Big.kt", &format!("fun main() {{}}\n// {body}\n"));

        assert!(mains(&tree).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn main_class_scan_does_not_follow_symlinks() {
        let outside = SpringTree::new("kt-outside");
        outside.write("Main.kt", "fun main() {}\n");
        let tree = SpringTree::new("kt-link");
        std::os::unix::fs::symlink(&outside.0, tree.0.join("linked")).unwrap();

        assert!(mains(&tree).is_empty());
    }

    #[test]
    fn kotlin_file_class_names_follow_the_compiler_rules() {
        assert_eq!(kotlin_file_class_stem("main"), "Main");
        assert_eq!(kotlin_file_class_stem("my-app"), "My_app");
        assert_eq!(kotlin_file_class_stem("_x"), "_x");
    }

    fn jars(tree: &SpringTree) -> Vec<String> {
        scan_jar_artifacts(&tree.0).items
    }

    #[test]
    fn jar_scan_finds_gradle_and_maven_outputs_at_any_module_depth() {
        let tree = SpringTree::new("jars");
        tree.write("build/libs/app.jar", "");
        tree.write("api/build/libs/api-1.0.jar", "");
        tree.write("services/web/target/web.jar", "");

        assert_eq!(
            jars(&tree),
            vec![
                "api/build/libs/api-1.0.jar",
                "build/libs/app.jar",
                "services/web/target/web.jar",
            ]
        );
    }

    #[test]
    fn jar_scan_skips_auxiliary_artifacts_and_other_files() {
        let tree = SpringTree::new("jars-aux");
        for name in [
            "app-plain.jar",
            "app-sources.jar",
            "app-javadoc.jar",
            "app-tests.jar",
            "original-app.jar",
            "notes.txt",
            "app.war",
        ] {
            tree.write(&format!("build/libs/{name}"), "");
        }
        tree.write("build/libs/app.jar", "");

        assert_eq!(jars(&tree), vec!["build/libs/app.jar"]);
    }

    #[test]
    fn jar_scan_only_looks_in_output_directories_and_ignores_dependencies() {
        let tree = SpringTree::new("jars-scope");
        tree.write("libs/vendor.jar", "");
        tree.write("node_modules/x/build/libs/x.jar", "");
        tree.write(".gradle/build/libs/cache.jar", "");
        tree.write("build/classes/Other.jar", "");

        assert!(jars(&tree).is_empty());
    }

    #[test]
    fn jar_scan_of_a_project_without_outputs_is_empty_and_not_truncated() {
        let tree = SpringTree::new("jars-none");
        tree.write("src/main/kotlin/Main.kt", "fun main() {}\n");

        let scan = scan_jar_artifacts(&tree.0);

        assert!(scan.items.is_empty());
        assert!(!scan.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn jar_scan_does_not_follow_symlinks() {
        let outside = SpringTree::new("jars-outside");
        outside.write("build/libs/o.jar", "");
        let tree = SpringTree::new("jars-link");
        std::os::unix::fs::symlink(&outside.0, tree.0.join("linked")).unwrap();

        assert!(jars(&tree).is_empty());
    }

    #[test]
    fn main_class_scan_reaches_main_functions_under_long_package_paths() {
        let tree = SpringTree::new("kt-deep");
        let package = "a/b/c/d/e/f/g/h/i/j";
        tree.write(
            &format!("app/svc/src/main/kotlin/{package}/Main.kt"),
            "package a.b.c.d.e.f.g.h.i.j\nfun main() {}\n",
        );

        let scan = scan_kotlin_main_classes(&tree.0);

        assert_eq!(scan.items, vec!["a.b.c.d.e.f.g.h.i.j.MainKt"]);
        assert!(!scan.truncated);
    }

    #[test]
    fn jar_scan_does_not_warn_about_deep_source_packages() {
        let tree = SpringTree::new("jars-deep-src");
        tree.write(
            "app/src/main/kotlin/a/b/c/d/e/f/g/h/i/Main.kt",
            "fun main() {}\n",
        );
        tree.write("app/build/libs/app.jar", "");

        let scan = scan_jar_artifacts(&tree.0);

        assert_eq!(scan.items, vec!["app/build/libs/app.jar"]);
        assert!(!scan.truncated);
    }

    #[test]
    fn jar_scan_never_looks_inside_source_trees() {
        let tree = SpringTree::new("jars-src-skip");
        // `src` 안의 패키지 디렉터리 이름이 `build`/`libs`여도 산출물이 아니다.
        tree.write("app/src/main/kotlin/build/libs/inner.jar", "");
        tree.write("app/build/libs/app.jar", "");

        assert_eq!(jars(&tree), vec!["app/build/libs/app.jar"]);
    }

    #[test]
    fn jar_scan_filters_auxiliary_artifacts_regardless_of_case() {
        let tree = SpringTree::new("jars-aux-case");
        tree.write("build/libs/APP-SOURCES.JAR", "");
        tree.write("build/libs/Original-App.jar", "");
        tree.write("build/libs/app.jar", "");

        assert_eq!(jars(&tree), vec!["build/libs/app.jar"]);
    }

    #[test]
    fn jar_scan_warns_when_the_depth_cap_hides_a_module() {
        let tree = SpringTree::new("jars-deep-module");
        tree.write("a/b/c/d/e/f/g/h/build.gradle.kts", "");

        assert!(scan_jar_artifacts(&tree.0).truncated);
    }

    #[test]
    fn package_lines_with_semicolons_and_trailing_comments_are_parsed() {
        let tree = SpringTree::new("pkg-comment");
        tree.write(
            "a/A.java",
            "package com.a; // owner\npublic class A {\n    public static void main(String[] x) {}\n}\n",
        );
        tree.write(
            "b/B.java",
            "package com.b;// tight\npublic class B {\n    public static void main(String[] x) {}\n}\n",
        );
        tree.write(
            "c/C.java",
            "package com.c; /* c */\npublic class C {\n    public static void main(String[] x) {}\n}\n",
        );
        tree.write("d/D.kt", "package com.d // kotlin\nfun main() {}\n");

        assert_eq!(
            mains(&tree),
            vec!["com.a.A", "com.b.B", "com.c.C", "com.d.DKt"]
        );
    }

    #[test]
    fn java_entry_points_are_recognised_across_modifier_orders_and_spacing() {
        for declaration in [
            "public static void main(String[] args) {}",
            "public static void main (String[] args) {}",
            "static public void main(String... args) {}",
            "public final static void main(final String[] args) {}",
            "public static  void  main( String[] args) {}",
        ] {
            assert!(is_java_main(declaration), "{declaration}");
        }
        for declaration in [
            "public void main(String[] args) {}",
            "static void main(String[] args) {}",
            "public static void main2(String[] args) {}",
            "public static int main(String[] args) {}",
            "private static void main(String[] args) {}",
            "public static void mainly() {}",
        ] {
            assert!(!is_java_main(declaration), "{declaration}");
        }
    }

    #[test]
    fn java_main_uses_the_file_name_as_the_class_name() {
        // 문서화된 한계: 파일 이름과 다른 클래스의 main도 파일 이름 기준으로 보고된다.
        let bytes = b"package p;\nclass Other {\n    public static void main(String[] a) {}\n}\n";

        assert_eq!(
            parse_main_class(bytes, "File", "java"),
            Some(String::from("p.File"))
        );
    }

    #[test]
    fn block_comments_without_leading_stars_are_ignored() {
        let source = b"/*\nLicense header\npackage fake.pkg\nfun main() {}\n*/\npackage real.pkg\nfun main() {}\n";

        assert_eq!(
            parse_main_class(source, "App", "kt"),
            Some(String::from("real.pkg.AppKt"))
        );
    }

    #[test]
    fn code_after_a_closing_block_comment_marker_is_still_read() {
        for source in [
            "/* header */ fun main() {}\n",
            "/**\n * doc\n */ fun main() {}\n",
            "/* a */\nfun main() {}\n",
        ] {
            assert_eq!(
                parse_main_class(source.as_bytes(), "App", "kt"),
                Some(String::from("AppKt")),
                "{source:?}"
            );
        }
        // 전부 주석이면 진입점이 아니다.
        assert_eq!(
            parse_main_class(b"/* fun main() {} */\nclass A\n", "A", "kt"),
            None
        );
        // 닫히지 않은 주석은 파일 끝까지 주석이다(잘못된 입력이라 감수).
        assert_eq!(
            parse_main_class(b"/* never closed\nfun main() {}\n", "A", "kt"),
            None
        );
    }

    #[test]
    fn a_java_main_on_the_class_declaration_line_is_a_documented_limit() {
        let source = b"public class A { public static void main(String[] a) {\n}}\n";

        assert_eq!(parse_main_class(source, "A", "java"), None);
    }

    #[test]
    fn a_package_declaration_separated_by_a_tab_is_read() {
        assert_eq!(
            parse_main_class(b"package\tcom.tab\nfun main() {}\n", "M", "kt"),
            Some(String::from("com.tab.MKt"))
        );
    }

    #[test]
    fn kotlin_top_level_modifiers_are_accepted_in_any_supported_order() {
        for line in [
            "fun main() {}",
            "public fun main() {}",
            "internal fun main(args: Array<String>) {}",
            "suspend fun main() {}",
            "public suspend fun main() {}",
            "fun main (args: Array<String>) {}",
        ] {
            assert!(is_kotlin_top_level_main(line), "{line}");
        }
        for line in [
            "private fun main() {}",
            "    fun main() {}",
            "fun main2() {}",
            "fun mainly() {}",
            "val main = 1",
        ] {
            assert!(!is_kotlin_top_level_main(line), "{line}");
        }
    }

    #[test]
    fn kotlin_file_class_names_keep_unicode_and_guard_leading_digits() {
        assert_eq!(kotlin_file_class_stem("메인"), "메인");
        assert_eq!(kotlin_file_class_stem("1app"), "_1app");
        assert_eq!(kotlin_file_class_stem("app.v2"), "App_v2");
        // `ß`의 대문자는 두 글자(`SS`)지만 컴파일러는 한 글자 대문자만 쓰므로 그대로 둔다.
        assert_eq!(kotlin_file_class_stem("ßa"), "ßa");
    }

    #[test]
    fn test_source_sets_are_skipped_but_main_packages_named_test_are_not() {
        let tree = SpringTree::new("test-sets");
        for set in [
            "test",
            "commonTest",
            "jvmTest",
            "integrationTest",
            "testFixtures",
        ] {
            tree.write(&format!("src/{set}/kotlin/T.kt"), "fun main() {}\n");
        }
        tree.write("src/main/kotlin/M.kt", "fun main() {}\n");
        // 이름이 `test`로 끝나지 않는 세트는 테스트가 아니다.
        tree.write("src/latest/kotlin/L.kt", "fun main() {}\n");
        tree.write("src/contest/kotlin/C.kt", "fun main() {}\n");

        assert_eq!(mains(&tree), vec!["CKt", "LKt", "MKt"]);
    }

    #[test]
    fn jar_extension_matching_ignores_case() {
        let tree = SpringTree::new("jar-case");
        tree.write("build/libs/APP.JAR", "");

        assert_eq!(jars(&tree), vec!["build/libs/APP.JAR"]);
    }
}
