//! GitHub Releases 기반 최신 버전 확인.
//!
//! 시작 시 자동 1회 + 사용자가 상태바에서 수동으로 트리거할 수 있으며,
//! 새 버전이 있으면 호출 측에서 OS 알림과 상태바 링크로 안내한다.
//! 네트워크 실패는 비치명적으로 다루도록 `Result<_, String>`을 반환한다.

use serde::Deserialize;
use std::time::Duration;

/// 릴리스를 조회할 GitHub 저장소 (`owner/repo`).
const REPO: &str = "devsepnine/debug_configuration";

/// 현재 빌드 버전 (`Cargo.toml`의 `version`). 상태바 표시와 비교 모두 이 상수를 SSOT로 쓴다.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 응답 본문 크기 상한. 정상 latest-release JSON은 수 KB이므로 64 KiB면 충분하며,
/// 비정상/악의적 응답으로 인한 메모리 소진을 방어한다.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// 전체 요청 타임아웃 — 무응답 연결에서 Task가 무기한 pending되는 것을 막는다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 연결 수립 타임아웃.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 릴리스 페이지로 허용하는 URL prefix. 응답이 변조되어도 `javascript:`/`file:` 등
/// 위험한 스킴을 브라우저로 여는 것을 막는다.
const RELEASE_URL_PREFIX: &str = "https://github.com/";

/// 업데이트 확인 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// 현재 버전이 최신.
    UpToDate { current: String },
    /// 더 높은 버전이 존재.
    Available {
        current: String,
        latest: String,
        /// 릴리스 페이지 URL (브라우저로 열기).
        url: String,
    },
}

/// GitHub Releases API 응답에서 필요한 필드만 추출.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

/// GitHub의 최신 릴리스를 조회해 현재 버전과 비교한다.
///
/// GitHub API는 `User-Agent` 헤더가 없으면 403을 반환하므로 반드시 지정한다.
/// 인증 토큰 없이 공개 저장소를 조회하며(IP당 시간당 60회로 충분), 어떤 실패도
/// `Err(String)`으로 변환해 호출 측이 조용히 상태만 갱신할 수 있게 한다.
pub async fn check_latest_release() -> Result<UpdateOutcome, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let user_agent = format!("run_config_manager/{CURRENT_VERSION}");

    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    let bytes = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("unexpected response: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;

    // 역직렬화 전에 크기를 확인해 비정상 대용량 응답을 방어한다.
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!("response too large: {} bytes", bytes.len()));
    }

    let release: GithubRelease =
        serde_json::from_slice(&bytes).map_err(|e| format!("failed to parse response: {e}"))?;

    let latest_raw = release.tag_name.trim();
    let current = CURRENT_VERSION.to_string();

    if is_newer(latest_raw, CURRENT_VERSION) {
        // 변조 방어: GitHub 릴리스 URL prefix가 아니면 거부한다.
        if !release.html_url.starts_with(RELEASE_URL_PREFIX) {
            return Err(format!("unexpected release URL: {}", release.html_url));
        }
        Ok(UpdateOutcome::Available {
            current,
            latest: normalize_version(latest_raw).to_string(),
            url: release.html_url,
        })
    } else {
        Ok(UpdateOutcome::UpToDate { current })
    }
}

/// 앞의 `v`/`V` 접두사를 제거한 버전 문자열을 반환.
fn normalize_version(raw: &str) -> &str {
    raw.strip_prefix(['v', 'V']).unwrap_or(raw)
}

/// `major.minor.patch` 앞 3요소를 숫자 튜플로 파싱. 형식이 맞지 않으면 `None`.
/// pre-release 접미사(`-rc.1` 등)는 패치 토큰에서 잘려 무시된다.
fn parse_version(raw: &str) -> Option<(u32, u32, u32)> {
    let mut parts = normalize_version(raw).split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    // 패치에 pre-release/build 메타데이터가 붙을 수 있어 숫자 prefix만 취한다.
    let patch_token = parts.next()?;
    let patch_digits: String = patch_token
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let patch = patch_digits.parse().ok()?;
    Some((major, minor, patch))
}

/// `latest`가 `current`보다 높은 버전이면 `true`.
/// 어느 쪽이든 파싱에 실패하면 보수적으로 `false`(업데이트 없음)를 반환한다.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_and_without_prefix() {
        assert_eq!(parse_version("v0.2.4"), Some((0, 2, 4)));
        assert_eq!(parse_version("0.2.4"), Some((0, 2, 4)));
        assert_eq!(parse_version("V1.10.0"), Some((1, 10, 0)));
    }

    #[test]
    fn parses_prerelease_patch() {
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3)));
    }

    #[test]
    fn non_numeric_patch_returns_none() {
        // patch가 숫자로 시작하지 않으면 파싱 실패 → is_newer는 보수적으로 false.
        assert_eq!(parse_version("1.2.alpha1"), None);
        // 숫자 접두사가 있으면 그 부분만 취한다.
        assert_eq!(parse_version("1.2.0-0"), Some((1, 2, 0)));
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn detects_newer_version() {
        assert!(is_newer("0.3.0", "0.2.4"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.2.10", "0.2.4"));
    }

    #[test]
    fn ignores_same_or_older() {
        assert!(!is_newer("0.2.4", "0.2.4"));
        assert!(!is_newer("0.2.3", "0.2.4"));
        assert!(!is_newer("v0.1.0", "0.2.4"));
    }

    #[test]
    fn malformed_is_not_newer() {
        assert!(!is_newer("garbage", "0.2.4"));
        assert!(!is_newer("0.3.0", "garbage"));
    }
}
