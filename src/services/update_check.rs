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

/// 응답 본문 크기 상한. latest-release JSON은 assets 배열(릴리스 노트 포함)을 담아도
/// 수십 KB 수준이므로 256 KiB면 충분하며, 비정상/악의적 응답으로 인한 메모리 소진을 방어한다.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// 전체 요청 타임아웃 — 무응답 연결에서 Task가 무기한 pending되는 것을 막는다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 연결 수립 타임아웃.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 릴리스 페이지/자산 다운로드로 허용하는 URL prefix. 응답이 변조되어도
/// `javascript:`/`file:` 등 위험한 스킴이나 외부 호스트로의 유도를 막는다.
pub(crate) const RELEASE_URL_PREFIX: &str = "https://github.com/";

/// 릴리스 자산 중 서명된 체크섬 목록/서명 파일 이름.
const CHECKSUMS_ASSET: &str = "checksums.txt";
const CHECKSUMS_SIG_ASSET: &str = "checksums.txt.minisig";

/// 인앱 자기 업데이트에 필요한 다운로드 정보 (플랫폼 자산 + 서명된 체크섬).
/// 모든 URL은 `RELEASE_URL_PREFIX` 검증을 통과한 값만 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateDownload {
    /// 플랫폼 자산 파일 이름 (checksums.txt의 항목 키와 동일).
    pub asset_name: String,
    /// 플랫폼 자산 다운로드 URL.
    pub asset_url: String,
    /// GitHub가 보고한 자산 크기 (bytes) — 다운로드 상한 산정 참고용.
    pub asset_size: u64,
    /// `checksums.txt` 다운로드 URL.
    pub checksums_url: String,
    /// `checksums.txt.minisig` (체크섬 목록의 minisign 서명) 다운로드 URL.
    pub checksums_sig_url: String,
}

/// 업데이트 확인 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// 현재 버전이 최신.
    UpToDate { current: String },
    /// 더 높은 버전이 존재.
    Available {
        current: String,
        latest: String,
        /// 릴리스 페이지 URL (브라우저로 열기 — 인앱 설치 불가 시 폴백).
        url: String,
        /// 인앱 설치용 다운로드 정보. 구 릴리스처럼 플랫폼 자산/서명이 없으면 `None`
        /// (이 경우 상태바 버튼은 기존처럼 릴리스 페이지를 연다).
        download: Option<UpdateDownload>,
    },
}

/// GitHub Releases API 응답에서 필요한 필드만 추출.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

/// 릴리스 자산 메타데이터 (다운로드 URL 매칭용).
#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
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
        let download = select_download(&release.assets);
        Ok(UpdateOutcome::Available {
            current,
            latest: normalize_version(latest_raw).to_string(),
            url: release.html_url,
            download,
        })
    } else {
        Ok(UpdateOutcome::UpToDate { current })
    }
}

/// 현재 빌드 플랫폼에 맞는 릴리스 자산 이름. 릴리스 파이프라인의 자산 명명 규칙과
/// 일치해야 한다 (`release.yml` 참고). 지원하지 않는 플랫폼/아키텍처 조합은 `None`.
fn platform_asset_name() -> Option<&'static str> {
    // `std::env::consts::ARCH`는 "x86_64"/"aarch64"를 주므로 자산명("x64"/"arm64")과
    // 직접 보간하면 불일치한다 — 명시 매핑만 사용한다.
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Some("run_config_manager-macos-x64-app.tar.gz"),
        ("macos", "aarch64") => Some("run_config_manager-macos-arm64-app.tar.gz"),
        ("windows", "x86_64") => Some("run_config_manager-windows-x64.msi"),
        ("linux", "x86_64") => Some("run_config_manager-linux-x64.tar.gz"),
        _ => None,
    }
}

/// 자산 목록에서 (플랫폼 자산, checksums.txt, checksums.txt.minisig)를 찾아
/// `UpdateDownload`를 구성한다. 셋 중 하나라도 없거나 URL prefix 검증에 실패하면
/// `None` — 호출 측은 릴리스 페이지 열기로 폴백한다 (구 릴리스 호환).
fn select_download(assets: &[GithubAsset]) -> Option<UpdateDownload> {
    let wanted = platform_asset_name()?;
    let find = |name: &str| -> Option<&GithubAsset> {
        assets
            .iter()
            .find(|a| a.name == name && a.browser_download_url.starts_with(RELEASE_URL_PREFIX))
    };
    let asset = find(wanted)?;
    let checksums = find(CHECKSUMS_ASSET)?;
    let signature = find(CHECKSUMS_SIG_ASSET)?;
    Some(UpdateDownload {
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        asset_size: asset.size,
        checksums_url: checksums.browser_download_url.clone(),
        checksums_sig_url: signature.browser_download_url.clone(),
    })
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

    // ---- 인앱 업데이트 자산 선택 ----

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!(
                "https://github.com/devsepnine/debug_configuration/releases/download/v9.9.9/{name}"
            ),
            size: 1024,
        }
    }

    /// 현재 테스트 실행 플랫폼의 기대 자산 이름 (platform_asset_name과 독립 산출).
    fn expected_platform_asset() -> Option<&'static str> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "x86_64") => Some("run_config_manager-macos-x64-app.tar.gz"),
            ("macos", "aarch64") => Some("run_config_manager-macos-arm64-app.tar.gz"),
            ("windows", "x86_64") => Some("run_config_manager-windows-x64.msi"),
            ("linux", "x86_64") => Some("run_config_manager-linux-x64.tar.gz"),
            _ => None,
        }
    }

    #[test]
    fn selects_download_when_all_assets_present() {
        let Some(wanted) = expected_platform_asset() else {
            return; // 미지원 플랫폼에서는 select_download가 None이어야 한다 (아래 테스트가 검증)
        };
        let assets = vec![
            asset(wanted),
            asset("checksums.txt"),
            asset("checksums.txt.minisig"),
            asset("run_config_manager-unrelated.zip"),
        ];
        let dl = select_download(&assets).expect("expected a download");
        assert_eq!(dl.asset_name, wanted);
        assert!(dl.asset_url.starts_with(RELEASE_URL_PREFIX));
        assert!(dl.checksums_url.ends_with("checksums.txt"));
        assert!(dl.checksums_sig_url.ends_with("checksums.txt.minisig"));
    }

    #[test]
    fn missing_signature_or_checksums_disables_in_app_update() {
        let Some(wanted) = expected_platform_asset() else {
            assert!(select_download(&[asset("checksums.txt")]).is_none());
            return;
        };
        // 서명 없음 (구 릴리스) → None
        assert!(select_download(&[asset(wanted), asset("checksums.txt")]).is_none());
        // 체크섬 없음 → None
        assert!(select_download(&[asset(wanted), asset("checksums.txt.minisig")]).is_none());
        // 플랫폼 자산 없음 → None
        assert!(
            select_download(&[asset("checksums.txt"), asset("checksums.txt.minisig")]).is_none()
        );
    }

    #[test]
    fn rejects_asset_urls_outside_github() {
        let Some(wanted) = expected_platform_asset() else {
            return;
        };
        let mut evil = asset(wanted);
        evil.browser_download_url = format!("https://evil.example.com/{wanted}");
        // 플랫폼 자산 URL이 GitHub 밖 → 자산 매칭 실패로 전체 None
        assert!(
            select_download(&[evil, asset("checksums.txt"), asset("checksums.txt.minisig")])
                .is_none()
        );
    }

    #[test]
    fn platform_asset_name_maps_arch_aliases() {
        // 이 바이너리가 지원 플랫폼에서 돌면 반드시 매핑이 존재해야 하고,
        // 자산명은 x86_64/aarch64가 아니라 x64/arm64 표기를 써야 한다.
        if let Some(name) = platform_asset_name() {
            assert!(!name.contains("x86_64"));
            assert!(!name.contains("aarch64"));
            assert_eq!(Some(name), expected_platform_asset());
        }
    }
}
