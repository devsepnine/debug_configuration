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
pub const ICON_CLOSE: &[u8] = include_bytes!("../assets/mingcute--close-fill.svg");
pub const ICON_ADD: &[u8] = include_bytes!("../assets/mingcute--add-square-line.svg");
pub const ICON_COPY: &[u8] = include_bytes!("../assets/mingcute--copy-2-line.svg");
pub const ICON_CONFIGURATIONS: &[u8] = include_bytes!("../assets/app--configurations.svg");
pub const ICON_SESSIONS: &[u8] = include_bytes!("../assets/app--sessions.svg");

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
    // 제외할 디렉토리 목록
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
