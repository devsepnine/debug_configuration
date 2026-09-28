//! MCP Bearer 토큰의 생성·영속화·비교.
//!
//! 토큰은 앱 설정 JSON이 아니라 별도 파일(`~/.run_config_mcp_token`)에 둔다. 설정 파일은
//! 사용자가 백업·공유·이슈에 첨부하는 대상이므로, 같은 파일에 두면 토큰이 함께 새어나간다.

use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 토큰 hex 문자열 길이 (32바이트).
const TOKEN_HEX_LEN: usize = 64;

/// 토큰 파일 경로. 홈 디렉터리를 못 찾으면 `None` — 설정 파일과 같은 fail-closed 정책이다
/// (CWD 폴백 금지: 공유 디렉터리에 비밀이 남는다).
pub fn token_path() -> Option<PathBuf> {
    dirs::home_dir().map(|mut path| {
        path.push(".run_config_mcp_token");
        path
    })
}

/// 새 토큰 생성. `Uuid::new_v4`는 OS CSPRNG(`getrandom`)에서 각 16바이트를 얻으므로,
/// 두 개를 이어 32바이트 = hex 64자를 만든다. 난수 crate를 새로 들이지 않기 위한 선택이다.
pub fn generate() -> String {
    let mut hex = String::with_capacity(TOKEN_HEX_LEN);
    for _ in 0..2 {
        for byte in Uuid::new_v4().as_bytes() {
            hex.push_str(&format!("{byte:02x}"));
        }
    }
    hex
}

/// 저장된 토큰을 읽고, 없거나 형식이 깨졌으면 새로 만들어 저장한다.
pub fn load_or_create() -> Result<String, String> {
    let path =
        token_path().ok_or_else(|| "No home directory for the MCP token file".to_string())?;
    load_or_create_at(&path)
}

/// 새 토큰으로 교체한다. 이전 토큰을 쓰던 클라이언트는 즉시 `401`을 받는다.
pub fn regenerate() -> Result<String, String> {
    let path =
        token_path().ok_or_else(|| "No home directory for the MCP token file".to_string())?;
    let token = generate();
    write_at(&path, &token)?;
    Ok(token)
}

/// 경로를 명시한 로드/생성. 테스트가 사용자 홈을 건드리지 않도록 분리해 둔다.
fn load_or_create_at(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(content) if is_well_formed(content.trim()) => {
            let token = content.trim().to_string();
            tighten_permissions(path);
            Ok(token)
        }
        // 내용이 깨졌으면 인증 불가 상태로 남기지 않고 재발급한다. 이 파일의 유일한
        // 소비자가 우리 자신이므로 버릴 수 있는 데이터가 없다.
        Ok(_) => {
            let token = generate();
            write_at(path, &token)?;
            Ok(token)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let token = generate();
            write_at(path, &token)?;
            Ok(token)
        }
        Err(e) => Err(format!("Failed to read {}: {e}", path.display())),
    }
}

/// 토큰 파일 쓰기. unix에서는 생성 시점부터 `0600`이어야 한다 — 먼저 만들고 나중에
/// 퍼미션을 조이면 그 사이에 다른 사용자가 읽을 수 있는 창이 생긴다.
fn write_at(path: &Path, token: &str) -> Result<(), String> {
    use std::io::Write;

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(path)
        .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;

    // `OpenOptions::mode`는 파일을 새로 만들 때만 적용된다. 이미 존재하던 파일이 느슨한
    // 퍼미션(재발급 경로)이었다면 **내용을 쓰기 전에** 조여야 한다 — 순서를 바꾸면 새 토큰이
    // 잠깐 세계 읽기 가능한 상태로 디스크에 놓인다. 이 시점 파일은 truncate로 비어 있다.
    tighten_permissions(path);
    file.write_all(token.as_bytes())
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(())
}

/// unix 퍼미션을 `0600`으로 맞춘다. 실패는 치명적이지 않으나 조용히 넘기지 않는다.
/// Windows는 사용자 프로필 디렉터리의 ACL에 의존한다.
fn tighten_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        if metadata.permissions().mode() & 0o777 == 0o600 {
            return;
        }
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            eprintln!("[MCP] Failed to restrict {} to 0600: {e}", path.display());
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// 저장된 값이 우리가 발급한 형식인지.
fn is_well_formed(token: &str) -> bool {
    token.len() == TOKEN_HEX_LEN && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 상수 시간 비교. 길이는 비밀이 아니므로(항상 64자) 먼저 비교하고, 내용은 조기 종료 없이
/// 전체를 XOR 누적한다 — 바이트 단위 조기 종료는 타이밍으로 접두사를 흘린다.
pub fn matches(expected: &str, provided: &str) -> bool {
    let (expected, provided) = (expected.as_bytes(), provided.as_bytes());
    if expected.len() != provided.len() || expected.is_empty() {
        return false;
    }

    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(provided) {
        diff |= a ^ b;
    }
    std::hint::black_box(diff) == 0
}

/// UI 표시용 마스킹. 사용자가 "어느 토큰인지" 식별할 수 있을 만큼만 남긴다.
pub fn mask(token: &str) -> String {
    if token.len() <= 12 {
        return "•".repeat(token.len().max(8));
    }
    format!(
        "{}{}{}",
        &token[..6],
        "•".repeat(12),
        &token[token.len() - 4..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 임시 디렉터리에 격리된 토큰 파일 경로. 사용자 홈을 건드리지 않는다.
    struct TempTokenFile {
        dir: PathBuf,
    }

    impl TempTokenFile {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("rcm-mcp-token-{}", Uuid::new_v4()));
            fs::create_dir_all(&dir).expect("temp dir");
            Self { dir }
        }

        fn path(&self) -> PathBuf {
            self.dir.join("token")
        }
    }

    impl Drop for TempTokenFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn generated_token_is_64_hex_chars() {
        let token = generate();
        assert_eq!(token.len(), TOKEN_HEX_LEN);
        assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_tokens_differ() {
        assert_ne!(generate(), generate());
    }

    #[test]
    fn load_creates_then_reuses_the_same_token() {
        let temp = TempTokenFile::new();
        let first = load_or_create_at(&temp.path()).expect("created");
        let second = load_or_create_at(&temp.path()).expect("reloaded");
        assert_eq!(first, second);
    }

    #[test]
    fn corrupted_token_file_is_replaced() {
        let temp = TempTokenFile::new();
        fs::write(temp.path(), "not-a-token").expect("seed");

        let token = load_or_create_at(&temp.path()).expect("regenerated");
        assert!(is_well_formed(&token));
        let persisted = fs::read_to_string(temp.path()).expect("read back");
        assert_eq!(persisted.trim(), token);
    }

    #[test]
    fn trailing_whitespace_is_tolerated() {
        let temp = TempTokenFile::new();
        let token = generate();
        fs::write(temp.path(), format!("{token}\n")).expect("seed");
        assert_eq!(load_or_create_at(&temp.path()).expect("loaded"), token);
    }

    #[cfg(unix)]
    #[test]
    fn created_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempTokenFile::new();
        load_or_create_at(&temp.path()).expect("created");
        let mode = fs::metadata(temp.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "token file must not be group/world readable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn loose_permissions_are_tightened_on_load() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempTokenFile::new();
        let token = generate();
        fs::write(temp.path(), &token).expect("seed");
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o644)).expect("loosen");

        load_or_create_at(&temp.path()).expect("loaded");
        let mode = fs::metadata(temp.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_a_loose_file_never_leaves_it_readable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempTokenFile::new();
        fs::write(temp.path(), "stale").expect("seed");
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o644)).expect("loosen");

        let token = generate();
        write_at(&temp.path(), &token).expect("written");

        let mode = fs::metadata(temp.path())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(fs::read_to_string(temp.path()).expect("read"), token);
    }

    #[test]
    fn matches_accepts_the_same_token() {
        let token = generate();
        assert!(matches(&token, &token));
    }

    #[test]
    fn matches_rejects_wrong_and_empty_tokens() {
        let token = generate();
        let mut wrong = token.clone();
        wrong.replace_range(0..1, if token.starts_with('a') { "b" } else { "a" });

        assert!(!matches(&token, &wrong));
        assert!(!matches(&token, ""));
        assert!(!matches("", ""));
        assert!(!matches(&token, &token[..token.len() - 1]));
    }

    #[test]
    fn mask_hides_the_middle_of_the_token() {
        let token = generate();
        let masked = mask(&token);
        assert!(masked.starts_with(&token[..6]));
        assert!(masked.ends_with(&token[token.len() - 4..]));
        assert!(!masked.contains(&token[6..token.len() - 4]));
    }

    #[test]
    fn mask_of_a_short_value_reveals_nothing() {
        assert_eq!(mask("abc"), "••••••••");
    }
}
