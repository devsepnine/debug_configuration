use std::path::Path;
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
