//! 인앱 자기 업데이트: GitHub 릴리스 자산 다운로드 → 서명·체크섬 검증 → 플랫폼별 적용.
//!
//! 브라우저 다운로드에 붙는 quarantine/Mark-of-the-Web이 Gatekeeper·SmartScreen
//! 프롬프트의 방아쇠이므로, 앱이 직접 내려받아 교체하면 재설치 프롬프트가 사라진다.
//!
//! 보안 모델 (Tier B):
//! - 모든 URL은 `RELEASE_URL_PREFIX`(GitHub) 검증을 통과한 값만 사용 (update_check에서 강제)
//! - `checksums.txt`는 CI가 minisign 개인키로 서명하며, 앱은 내장 공개키로 서명을
//!   검증한 뒤에만 체크섬을 신뢰한다 — 릴리스/계정이 탈취돼도 개인키 없인 위조 불가
//! - 자산은 메모리로 수신(수신 바이트 기준 상한)한 뒤 **같은 버퍼를** 해시·추출한다 (TOCTOU 차단)
//! - tar 추출은 일반 파일/디렉터리만 허용하고 경로 이탈(zip-slip)·링크 엔트리를 거부한다
//!
//! 적용 전략:
//! - macOS: `.app` 번들을 같은 볼륨에 스테이징 후 rename 스왑 (2차 rename 실패 시 역스왑 복구)
//! - Windows: 검증된 MSI를 temp에 쓰고 `msiexec /i`(full UI)에 위임 — MotW가 없어
//!   SmartScreen이 뜨지 않고, wix ExitDialog 체크박스가 재실행을 담당 (UAC 1회는 잔존)
//! - Linux: 아카이브에서 바이너리만 추출해 temp+rename으로 교체 — rename(2)은 실행 중
//!   바이너리 경로에도 원자적으로 동작하므로(inode 유지) 별도 크레이트가 필요 없다

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::futures::StreamExt;
use sha2::{Digest, Sha256};

use super::update_check::{RELEASE_URL_PREFIX, UpdateDownload};

/// 릴리스 서명 검증용 minisign 공개키. 개인키는 CI Secret(`MINISIGN_SECRET_KEY`)에만
/// 존재하며, 이 키로 검증되지 않는 `checksums.txt`는 무조건 거부한다 (무서명 폴백 없음).
const MINISIGN_PUBLIC_KEY: &str = "RWT2wzOU76oZkAj3gSARYLsNlKLqJQgdF71r7WLKvKmuG5DuWSrdngr4";

/// 자산 다운로드 상한 (수신 누적 바이트 기준 — Content-Length는 신뢰하지 않는다).
const MAX_ASSET_BYTES: u64 = 300 * 1024 * 1024;

/// checksums.txt / 서명 파일 다운로드 상한.
const MAX_TEXT_BYTES: u64 = 1024 * 1024;

/// tar 추출 시 허용하는 총 해제 크기 (압축 폭탄 방어; 헤더 선언 크기 합산).
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// 연결 수립 타임아웃.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 읽기 stall 타임아웃. update_check의 10초 **총** 타임아웃은 수십 MB 다운로드에서
/// 반드시 실패하므로 재사용하지 않는다 — 총 타임아웃 없이 stall만 감지한다.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// 업데이트 적용 후 "무엇을 실행하고 종료할지"만 담는 계획.
/// 실제 spawn + exit는 app.rs 핸들러가 수행한다 (이 모듈은 프로세스를 종료하지 않는다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelaunchPlan {
    /// 새 번들 설치 완료 — `open -n <app_path>`로 재실행.
    MacOs { app_path: PathBuf },
    /// 검증된 MSI 준비 완료 — `msiexec /i <msi_path>`(full UI)로 위임.
    Windows { msi_path: PathBuf },
    /// 바이너리 교체 완료 — `<exe_path>`를 재실행.
    Linux { exe_path: PathBuf },
}

/// 플랫폼별 적용 대상 (네트워크 작업 전에 미리 해석해 조기 실패시킨다).
/// 각 variant는 해당 OS의 `resolve_apply_target`(cfg-gated)에서만 생성되므로,
/// 컴파일 대상 OS 기준으로는 나머지 variant가 "미생성"으로 보인다.
#[allow(dead_code)]
enum ApplyTarget {
    /// 교체할 기존 `.app` 번들 경로.
    MacOsBundle(PathBuf),
    /// MSI는 temp에 쓰므로 사전 대상이 없다.
    WindowsMsi,
    /// 교체할 현재 실행 파일 경로.
    LinuxBinary(PathBuf),
}

/// 업데이트 자산을 내려받아 검증하고 현재 설치에 적용한다.
///
/// 순서: 적용 대상 사전 해석(조기 실패) → checksums+서명 다운로드 → **서명 검증** →
/// 자산 다운로드(상한) → sha256 대조 → 플랫폼별 적용(blocking 작업은 spawn_blocking).
/// 어떤 단계든 실패하면 기존 설치는 그대로 남는다.
pub async fn download_and_apply(dl: UpdateDownload) -> Result<RelaunchPlan, String> {
    let target = resolve_apply_target()?;

    // GitHub가 보고한 크기가 상한을 넘으면 다운로드 자체를 시작하지 않는다.
    if dl.asset_size > MAX_ASSET_BYTES {
        return Err(format!(
            "update asset too large: {} bytes (limit {})",
            dl.asset_size, MAX_ASSET_BYTES
        ));
    }

    let client = download_client()?;

    let checksums_raw = fetch_capped(&client, &dl.checksums_url, MAX_TEXT_BYTES).await?;
    let signature_raw = fetch_capped(&client, &dl.checksums_sig_url, MAX_TEXT_BYTES).await?;
    verify_signature_with_key(MINISIGN_PUBLIC_KEY, &checksums_raw, &signature_raw)?;

    let checksums_text = std::str::from_utf8(&checksums_raw)
        .map_err(|_| "checksums.txt is not valid UTF-8".to_string())?;
    let checksums = parse_checksums(checksums_text);
    let expected = checksums
        .get(dl.asset_name.as_str())
        .ok_or_else(|| format!("no checksum entry for {}", dl.asset_name))?
        .clone();

    let payload = fetch_capped(&client, &dl.asset_url, MAX_ASSET_BYTES).await?;
    let actual = sha256_hex(&payload);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "checksum mismatch for {} — aborting update",
            dl.asset_name
        ));
    }

    // 압축 해제·rename 등 blocking 파일 작업은 tokio 워커를 막지 않도록 분리한다.
    tokio::task::spawn_blocking(move || apply(target, payload))
        .await
        .map_err(|e| format!("update task panicked: {e}"))?
}

/// 현재 빌드/설치 형태에서 업데이트를 적용할 수 있는지 미리 확인한다.
/// 불가(예: 번들 밖 dev 빌드)면 다운로드를 시작하기 전에 실패시킨다.
fn resolve_apply_target() -> Result<ApplyTarget, String> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().map_err(|e| format!("cannot locate self: {e}"))?;
        let bundle = bundle_root_from_exe(&exe).ok_or_else(|| {
            "in-app update requires running from the installed .app bundle".to_string()
        })?;
        Ok(ApplyTarget::MacOsBundle(bundle))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(ApplyTarget::WindowsMsi)
    }
    #[cfg(target_os = "linux")]
    {
        let exe = std::env::current_exe().map_err(|e| format!("cannot locate self: {e}"))?;
        Ok(ApplyTarget::LinuxBinary(exe))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("in-app update is not supported on this platform".to_string())
    }
}

/// 다운로드 전용 HTTP 클라이언트. 총 타임아웃 없이 connect/stall 타임아웃만 둔다.
fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!(
            "run_config_manager/{}",
            super::update_check::CURRENT_VERSION
        ))
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

/// URL prefix를 재확인하고, 수신 누적 바이트 기준 상한을 강제하며 스트리밍 수신한다.
async fn fetch_capped(client: &reqwest::Client, url: &str, cap: u64) -> Result<Vec<u8>, String> {
    // update_check에서 이미 검증된 값이지만, 심층 방어로 이 경계에서도 강제한다.
    if !url.starts_with(RELEASE_URL_PREFIX) {
        return Err(format!("refusing non-GitHub download URL: {url}"));
    }
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("unexpected download response: {e}"))?;

    let mut buf: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download interrupted: {e}"))?;
        accumulate_capped(&mut buf, &chunk, cap)?;
    }
    Ok(buf)
}

/// `buf`에 `chunk`를 추가하되 누적 크기가 `cap`을 넘으면 거부한다.
fn accumulate_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: u64) -> Result<(), String> {
    let next = buf.len() as u64 + chunk.len() as u64;
    if next > cap {
        return Err(format!("download exceeds limit of {cap} bytes — aborting"));
    }
    buf.extend_from_slice(chunk);
    Ok(())
}

/// minisign 서명 검증. `pk_b64` 공개키로 `data`에 대한 서명 파일 내용(`sig_text`)을
/// 검증한다. 실패는 무조건 에러 — 서명 없는 업데이트 경로는 존재하지 않는다.
fn verify_signature_with_key(pk_b64: &str, data: &[u8], sig_raw: &[u8]) -> Result<(), String> {
    let pk = minisign_verify::PublicKey::from_base64(pk_b64)
        .map_err(|e| format!("invalid update public key: {e}"))?;
    let sig_text = std::str::from_utf8(sig_raw)
        .map_err(|_| "signature file is not valid UTF-8".to_string())?;
    let signature = minisign_verify::Signature::decode(sig_text)
        .map_err(|e| format!("malformed signature file: {e}"))?;
    pk.verify(data, &signature, false)
        .map_err(|e| format!("signature verification FAILED — refusing update: {e}"))
}

/// `sha256sum` 출력 형식(`<hex><space><space|*><name>`)을 파싱해 `name → hex` 맵을 만든다.
/// 형식에 맞지 않는 줄은 무시한다 (누락 항목은 조회 단계에서 에러가 된다).
fn parse_checksums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((hash, rest)) = line.split_once(' ') else {
            continue;
        };
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        // 두 번째 구분 문자: 텍스트 모드는 공백, 바이너리 모드는 '*'.
        let name = rest.strip_prefix(['*', ' ']).unwrap_or(rest).trim();
        if name.is_empty() {
            continue;
        }
        map.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    map
}

/// 버퍼의 SHA-256을 소문자 hex 문자열로 반환.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 검증 완료된 payload를 플랫폼별로 적용한다 (blocking — spawn_blocking 안에서 호출).
fn apply(target: ApplyTarget, payload: Vec<u8>) -> Result<RelaunchPlan, String> {
    match target {
        ApplyTarget::MacOsBundle(bundle) => apply_macos(&bundle, &payload),
        ApplyTarget::WindowsMsi => apply_windows(&payload),
        ApplyTarget::LinuxBinary(exe) => apply_linux(&exe, &payload),
    }
}

// ---- macOS: .app 번들 스왑 -------------------------------------------------
// 헬퍼들은 순수 파일시스템 연산이라 플랫폼 게이트 없이 두어 어느 CI에서든 테스트한다.

/// `current_exe()` 경로에서 `.app` 번들 루트를 역산한다.
/// 정확히 `<Bundle>.app/Contents/MacOS/<binary>` 형태일 때만 `Some` — 형태가 다르면
/// (dev 빌드 등) `None`을 반환해, 상위 디렉터리를 번들로 오인해 rename하는 사고를 막는다.
/// 프로덕션 호출부는 macOS cfg 블록 안에만 있고, 타 플랫폼에서는 테스트로만 쓰인다.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bundle_root_from_exe(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let root = contents.parent()?;
    if root.extension()? != "app" {
        return None;
    }
    Some(root.to_path_buf())
}

/// tar.gz를 `dest` 아래로 해제한다. 일반 파일/디렉터리 엔트리만 허용하며
/// 링크(symlink/hardlink) 엔트리·경로 이탈·총 해제 크기 초과를 거부한다.
fn extract_targz_to(bytes: &[u8], dest: &Path) -> Result<(), String> {
    extract_targz_to_with_cap(bytes, dest, MAX_UNPACKED_BYTES)
}

fn extract_targz_to_with_cap(bytes: &[u8], dest: &Path, cap: u64) -> Result<(), String> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    let mut total: u64 = 0;
    for entry in archive
        .entries()
        .map_err(|e| format!("invalid archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("invalid archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("invalid entry path: {e}"))?
            .into_owned();
        sanitize_tar_entry_path(&path)?;

        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Directory => {}
            // pax 확장 헤더(bsdtar 기본 포맷)는 메타데이터 전용이라 파일로 풀지 않고
            // 건너뛴다. CI가 COPYFILE_DISABLE로 생성하더라도 방어적으로 허용한다.
            tar::EntryType::XHeader | tar::EntryType::XGlobalHeader => continue,
            other => {
                return Err(format!(
                    "refusing archive entry type {other:?} at {}",
                    path.display()
                ));
            }
        }

        // bsdtar가 기본으로 파일마다 끼워 넣는 macOS AppleDouble 사이드카(`._*`,
        // 리소스포크/Finder 메타데이터). Regular 타입이라 위 필터를 통과하므로 별도로
        // 걸러야 한다 — 번들 안에 섞이면 코드서명 리소스 실(seal)이 깨진다
        // (codesign --verify --deep --strict 실측 확인).
        if path
            .file_name()
            .is_some_and(|n| n.as_encoded_bytes().starts_with(b"._"))
        {
            continue;
        }

        total = total.saturating_add(entry.header().size().unwrap_or(0));
        if total > cap {
            return Err(format!("archive expands beyond {cap} bytes — aborting"));
        }

        // unpack_in은 자체적으로도 경로 이탈을 방어한다 (false = 안전상 건너뜀 → 에러 승격).
        let unpacked = entry
            .unpack_in(dest)
            .map_err(|e| format!("failed to extract {}: {e}", path.display()))?;
        if !unpacked {
            return Err(format!("unsafe archive entry skipped: {}", path.display()));
        }
    }
    Ok(())
}

/// tar 엔트리 경로가 상대 경로이고 `..`/루트 성분이 없는지 검증한다 (zip-slip 방어).
fn sanitize_tar_entry_path(path: &Path) -> Result<(), String> {
    use std::path::Component;
    if path.as_os_str().is_empty() {
        return Err("empty archive entry path".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "refusing archive entry escaping destination: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// 스테이징 디렉터리에서 최상위 `.app` 디렉터리를 정확히 하나 찾는다.
fn find_single_app_dir(staging: &Path) -> Result<PathBuf, String> {
    let mut found: Option<PathBuf> = None;
    let entries =
        std::fs::read_dir(staging).map_err(|e| format!("cannot read staging dir: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.extension().is_some_and(|ext| ext == "app") {
            if found.is_some() {
                return Err("archive contains more than one .app bundle".to_string());
            }
            found = Some(path);
        }
    }
    found.ok_or_else(|| "archive does not contain an .app bundle".to_string())
}

/// 번들 스왑: 기존 → `backup` rename 후 새 번들을 제자리로 rename.
/// 2차 rename이 실패하면 역스왑으로 복구한다 (제자리에 앱이 없는 상태를 만들지 않는다).
fn swap_bundle_at(bundle: &Path, new_bundle: &Path, backup: &Path) -> Result<(), String> {
    std::fs::rename(bundle, backup)
        .map_err(|e| format!("cannot move current bundle aside: {e}"))?;
    if let Err(install_err) = std::fs::rename(new_bundle, bundle) {
        return Err(match std::fs::rename(backup, bundle) {
            Ok(()) => format!("failed to install new bundle (rolled back): {install_err}"),
            Err(rollback_err) => format!(
                "failed to install new bundle: {install_err}; ROLLBACK ALSO FAILED \
                 ({rollback_err}) — reinstall from {}",
                backup.display()
            ),
        });
    }
    Ok(())
}

/// macOS 적용: 같은 볼륨(번들의 부모)에 스테이징 → 추출 → 검사 → 스왑.
/// 스테이징 디렉터리는 성공/실패와 무관하게 정리한다. 이전 번들은 `.old-<pid>`로
/// 남겨두고(즉시 롤백용), 다음 실행의 `cleanup_stale_update_artifacts`가 제거한다.
fn apply_macos(bundle: &Path, payload: &[u8]) -> Result<RelaunchPlan, String> {
    let parent = bundle
        .parent()
        .ok_or_else(|| "bundle has no parent directory".to_string())?;
    let bundle_name = bundle
        .file_name()
        .ok_or_else(|| "bundle has no name".to_string())?
        .to_string_lossy()
        .into_owned();

    let staging = parent.join(format!(".rcm-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    // 부모(대개 /Applications)에 쓸 수 없으면 여기서 조기 실패한다 (표준 사용자 등).
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("cannot write to {}: {e}", parent.display()))?;

    let result = (|| {
        extract_targz_to(payload, &staging)?;
        let new_app = find_single_app_dir(&staging)?;
        // 최소 구조 검사: 실행 불가능한 껍데기로 기존 앱을 대체하는 사고 방지.
        if !new_app.join("Contents").join("Info.plist").is_file() {
            return Err("new bundle is missing Contents/Info.plist".to_string());
        }
        let backup = parent.join(format!("{bundle_name}.old-{}", std::process::id()));
        swap_bundle_at(bundle, &new_app, &backup)
    })();

    // 성공(빈 껍데기)·실패(부분 추출물) 모두 스테이징을 정리한다.
    let _ = std::fs::remove_dir_all(&staging);
    result?;

    Ok(RelaunchPlan::MacOs {
        app_path: bundle.to_path_buf(),
    })
}

/// 새 파일을 O_EXCL(`create_new`)로 생성해 기록한다. 경로에 이미 무엇(파일이든
/// 심볼릭 링크든)이 존재하면 따라가/덮어쓰지 않고 즉시 실패한다 (선점 공격 방어).
fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

// ---- Windows: MSI 위임 ------------------------------------------------------

/// 검증된 MSI를 temp에 기록한다. 실행(msiexec)은 RelaunchPlan을 받은 핸들러가 수행.
fn apply_windows(payload: &[u8]) -> Result<RelaunchPlan, String> {
    let msi_path = std::env::temp_dir().join(format!("rcm-update-{}.msi", std::process::id()));
    // 같은 pid의 이전 잔여물이 있으면 제거 후 O_EXCL로 새로 만든다.
    let _ = std::fs::remove_file(&msi_path);
    write_new_file(&msi_path, payload).map_err(|e| format!("cannot write installer: {e}"))?;
    Ok(RelaunchPlan::Windows { msi_path })
}

// ---- Linux: 단일 바이너리 교체 ----------------------------------------------

/// tar.gz에서 경로가 `suffix`로 끝나는 일반 파일 엔트리 하나를 추출한다.
fn extract_entry_from_targz(bytes: &[u8], suffix: &Path) -> Result<Vec<u8>, String> {
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| format!("invalid archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("invalid archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("invalid entry path: {e}"))?
            .into_owned();
        if entry.header().entry_type() == tar::EntryType::Regular && path.ends_with(suffix) {
            let declared = entry.header().size().unwrap_or(0);
            if declared > MAX_UNPACKED_BYTES {
                return Err("archive entry too large".to_string());
            }
            let mut buf = Vec::with_capacity(declared as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("failed to read archive entry: {e}"))?;
            return Ok(buf);
        }
    }
    Err(format!("archive does not contain {}", suffix.display()))
}

/// `target` 경로의 바이너리를 새 내용으로 교체한다. 같은 디렉터리의 임시 파일에 쓴 뒤
/// rename하므로 원자적이며, Unix에서는 실행 중인 바이너리 경로에도 안전하다
/// (기존 inode는 프로세스가 쥔 채 유지되고 경로만 새 파일을 가리킨다).
fn replace_binary_at(target: &Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = target
        .parent()
        .ok_or_else(|| "binary has no parent directory".to_string())?;
    let tmp = dir.join(format!(".rcm-new-{}", std::process::id()));
    // 같은 pid의 이전 잔여물이 있으면 제거 후 O_EXCL로 새로 만든다 (선점 공격 방어).
    let _ = std::fs::remove_file(&tmp);
    write_new_file(&tmp, new_bytes).map_err(|e| format!("cannot write new binary: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("cannot mark new binary executable: {e}"));
        }
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace binary at {}: {e}", target.display())
    })
}

/// Linux 적용: 아카이브에서 바이너리를 꺼내 현재 실행 파일을 교체한다.
fn apply_linux(exe: &Path, payload: &[u8]) -> Result<RelaunchPlan, String> {
    let binary = extract_entry_from_targz(payload, Path::new("bin/run_config_manager"))?;
    replace_binary_at(exe, &binary)?;
    Ok(RelaunchPlan::Linux {
        exe_path: exe.to_path_buf(),
    })
}

// ---- 잔여물 정리 --------------------------------------------------------------

/// 이전 업데이트가 남긴 잔여물을 정리한다 (best-effort, startup Task에서 호출).
/// macOS: 번들 부모의 `<Bundle>.app.old-*` / `.rcm-update-*` — 현재 번들은 제외.
/// Windows: temp의 `rcm-update-*.msi` (설치 중 잠긴 파일은 삭제가 실패하고 무시된다).
pub fn cleanup_stale_update_artifacts() {
    #[cfg(target_os = "macos")]
    {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let Some(bundle) = bundle_root_from_exe(&exe) else {
            return;
        };
        let Some(parent) = bundle.parent() else {
            return;
        };
        cleanup_update_artifacts_in(parent, &bundle);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("rcm-update-") && name.ends_with(".msi") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // 교체 도중 크래시가 남긴 임시 새 바이너리(.rcm-new-*)를 정리한다.
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let Some(dir) = exe.parent() else {
            return;
        };
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".rcm-new-") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// `dir` 안에서 이 앱의 업데이트 잔여물만 제거한다. `current`(실행 중 번들)는 절대
/// 건드리지 않으며, 우리가 실제로 만드는 이름 형태(`<현재 번들명>.old-<pid>`,
/// `.rcm-update-<pid>`)와 **정확히** 일치할 때만 삭제한다 — 부분 문자열 매칭은
/// 사용자가 만든 무관한 백업 폴더(예: `SomeApp.app.old-2024`)까지 지울 수 있다.
/// 프로덕션 호출부는 macOS cfg 블록 안에만 있고, 타 플랫폼에서는 테스트로만 쓰인다.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn cleanup_update_artifacts_in(dir: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let Some(current_name) = current
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return;
    };
    let backup_prefix = format!("{current_name}.old-");
    let is_numeric_suffix = |name: &str, prefix: &str| -> bool {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_backup = is_numeric_suffix(&name, &backup_prefix);
        let is_staging = is_numeric_suffix(&name, ".rcm-update-");
        if is_backup || is_staging {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 테스트 전용 minisign 키쌍으로 서명한 fixture (프로덕션 키와 무관).
    const TEST_PUBLIC_KEY: &str = "RWQYx33D/1qQuMaP7BwL2zKy4vj0755zf4LvSNAaSW3mBgZoM8C3xuAT";
    const TEST_CHECKSUMS: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  empty.bin\n5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  hello.txt\n";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQYx33D/1qQuPaFPL1g94X9Q9pKjdyTbC6cq8HRECLgKUZfFQyuDEGF0SwuHNj4J++QfsJroqJrb+CplwjNRxEsKAP1GvI9jww=\ntrusted comment: timestamp:1783335966\tfile:checksums.txt\thashed\nVF+j1ReILfonRGEE54c9eIJlvCHRWuVbTgAR2qEJu87BDky7yW4xfDfTVf3O6NxLeQEUIULGlP0W0Q9yiFXeBg==\n";

    static TEST_DIR_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    fn unique_temp_dir(tag: &str) -> PathBuf {
        let n = TEST_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rcm_update_{}_{}_{}", std::process::id(), n, tag));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// (path, bytes, mode) 목록으로 tar.gz 버퍼 생성.
    fn build_targz(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, bytes, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    // ---- 서명 검증 ----

    #[test]
    fn signature_verifies_with_correct_key() {
        assert!(
            verify_signature_with_key(
                TEST_PUBLIC_KEY,
                TEST_CHECKSUMS.as_bytes(),
                TEST_SIGNATURE.as_bytes()
            )
            .is_ok()
        );
    }

    #[test]
    fn signature_rejects_tampered_data() {
        let mut tampered = TEST_CHECKSUMS.as_bytes().to_vec();
        tampered[0] ^= 0x01; // 해시 첫 글자 변조
        let err = verify_signature_with_key(TEST_PUBLIC_KEY, &tampered, TEST_SIGNATURE.as_bytes())
            .unwrap_err();
        assert!(err.contains("FAILED"), "unexpected error: {err}");
    }

    #[test]
    fn signature_rejects_wrong_key() {
        // 프로덕션 공개키로는 테스트 서명이 검증되지 않아야 한다 (키 분리 확인).
        assert!(
            verify_signature_with_key(
                MINISIGN_PUBLIC_KEY,
                TEST_CHECKSUMS.as_bytes(),
                TEST_SIGNATURE.as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn signature_rejects_garbage() {
        assert!(verify_signature_with_key(TEST_PUBLIC_KEY, b"data", b"not a signature").is_err());
        assert!(
            verify_signature_with_key("not-a-key", b"data", TEST_SIGNATURE.as_bytes()).is_err()
        );
    }

    // ---- 체크섬 ----

    #[test]
    fn parses_sha256sum_output() {
        let sums = parse_checksums(TEST_CHECKSUMS);
        assert_eq!(
            sums.get("hello.txt").map(String::as_str),
            Some("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
        );
        assert_eq!(sums.len(), 2);
    }

    #[test]
    fn parses_binary_mode_marker_and_skips_malformed() {
        let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 *starred.bin\nnot a checksum line\nzz91b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  bad-hex.txt\n";
        let sums = parse_checksums(text);
        assert!(sums.contains_key("starred.bin"));
        assert_eq!(sums.len(), 1);
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"hello\n"),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    /// 서명 → 체크섬 파싱 → payload 해시 대조까지 다운로드 이후의 검증 체인 전체.
    #[test]
    fn full_verification_chain_accepts_good_and_rejects_bad_payload() {
        verify_signature_with_key(
            TEST_PUBLIC_KEY,
            TEST_CHECKSUMS.as_bytes(),
            TEST_SIGNATURE.as_bytes(),
        )
        .unwrap();
        let sums = parse_checksums(TEST_CHECKSUMS);
        let expected = sums.get("hello.txt").unwrap();
        assert!(sha256_hex(b"hello\n").eq_ignore_ascii_case(expected));
        assert!(!sha256_hex(b"HELLO\n").eq_ignore_ascii_case(expected));
    }

    // ---- 다운로드 상한 ----

    #[test]
    fn accumulate_rejects_beyond_cap() {
        let mut buf = Vec::new();
        assert!(accumulate_capped(&mut buf, &[0u8; 60], 100).is_ok());
        assert!(accumulate_capped(&mut buf, &[0u8; 40], 100).is_ok());
        assert!(accumulate_capped(&mut buf, &[0u8; 1], 100).is_err());
        assert_eq!(buf.len(), 100);
    }

    // ---- tar 추출/새니타이즈 ----

    #[test]
    fn extracts_regular_files_and_dirs() {
        let dir = unique_temp_dir("extract_ok");
        let tgz = build_targz(&[
            ("My.app/Contents/Info.plist", b"plist".as_slice(), 0o644),
            ("My.app/Contents/MacOS/bin", b"\x7fELF".as_slice(), 0o755),
        ]);
        extract_targz_to(&tgz, &dir).unwrap();
        assert!(dir.join("My.app/Contents/Info.plist").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("My.app/Contents/MacOS/bin"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "exec bits must survive extraction");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_path_traversal_entries() {
        // tar::Builder는 `..` 경로 생성을 스스로 거부하므로, 공격자가 만들 법한
        // 악성 아카이브는 raw 헤더 조작으로 구성해 추출기의 방어를 검증한다.
        let dir = unique_temp_dir("extract_evil");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        {
            let raw = header.as_mut_bytes();
            let evil = b"../evil.txt";
            raw[..evil.len()].copy_from_slice(evil); // name 필드(offset 0, NUL 패딩)
        }
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &b"x"[..]).unwrap();
        let tgz = builder.into_inner().unwrap().finish().unwrap();

        assert!(extract_targz_to(&tgz, &dir).is_err());
        assert!(!dir.parent().unwrap().join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_appledouble_sidecar_files() {
        // bsdtar는 기본으로 파일마다 ._<name> AppleDouble 사이드카(Regular 타입)를
        // 끼워 넣는다. 그대로 추출하면 번들 코드서명 실이 깨지므로 반드시 걸러야 한다.
        let dir = unique_temp_dir("extract_appledouble");
        let tgz = build_targz(&[
            ("._My.app", b"apple-double".as_slice(), 0o644),
            ("My.app/Contents/._Info.plist", b"junk".as_slice(), 0o644),
            ("My.app/Contents/Info.plist", b"plist".as_slice(), 0o644),
            (
                "My.app/Contents/MacOS/._run_config_manager",
                b"junk".as_slice(),
                0o644,
            ),
            (
                "My.app/Contents/MacOS/run_config_manager",
                b"bin".as_slice(),
                0o755,
            ),
        ]);
        extract_targz_to(&tgz, &dir).unwrap();
        assert!(dir.join("My.app/Contents/Info.plist").is_file());
        assert!(
            dir.join("My.app/Contents/MacOS/run_config_manager")
                .is_file()
        );
        assert!(!dir.join("._My.app").exists());
        assert!(!dir.join("My.app/Contents/._Info.plist").exists());
        assert!(
            !dir.join("My.app/Contents/MacOS/._run_config_manager")
                .exists()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_pax_extended_header_entries() {
        // macOS bsdtar 기본(pax) 포맷은 각 파일 앞에 'x'(XHeader) 메타데이터 엔트리를
        // 끼워넣는다 (실측: PaxHeader/<name>). 추출기는 이를 파일로 풀지 않고 건너뛰되
        // 실제 파일은 정상 추출해야 한다. CI가 ustar를 쓰더라도 방어선으로 검증한다.
        let dir = unique_temp_dir("extract_pax");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        // 'x' 확장 헤더 엔트리 (bsdtar가 만드는 형태를 raw로 재현)
        let pax_payload = b"27 mtime=1783335966.000000\n";
        let mut pax_header = tar::Header::new_ustar();
        pax_header.set_entry_type(tar::EntryType::XHeader);
        pax_header.set_size(pax_payload.len() as u64);
        pax_header.set_path("PaxHeader/real.txt").unwrap();
        pax_header.set_cksum();
        builder.append(&pax_header, &pax_payload[..]).unwrap();
        // 뒤따르는 실제 파일
        let mut file_header = tar::Header::new_ustar();
        file_header.set_size(4);
        file_header.set_mode(0o644);
        file_header.set_path("real.txt").unwrap();
        file_header.set_cksum();
        builder.append(&file_header, &b"data"[..]).unwrap();
        let tgz = builder.into_inner().unwrap().finish().unwrap();

        extract_targz_to(&tgz, &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("real.txt")).unwrap(), b"data");
        assert!(
            !dir.join("PaxHeader").exists(),
            "pax metadata must not be extracted as files"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_symlink_entries() {
        let dir = unique_temp_dir("extract_link");
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_cksum();
        builder
            .append_link(&mut header, "inner/link", "/etc/passwd")
            .unwrap();
        let tgz = builder.into_inner().unwrap().finish().unwrap();
        let err = extract_targz_to(&tgz, &dir).unwrap_err();
        assert!(err.contains("entry type"), "unexpected error: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_archives_expanding_beyond_cap() {
        let dir = unique_temp_dir("extract_bomb");
        let tgz = build_targz(&[
            ("a.bin", [0u8; 100].as_slice(), 0o644),
            ("b.bin", [0u8; 100].as_slice(), 0o644),
        ]);
        assert!(extract_targz_to_with_cap(&tgz, &dir, 150).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_rejects_absolute_and_parent_paths() {
        assert!(sanitize_tar_entry_path(Path::new("ok/relative")).is_ok());
        assert!(sanitize_tar_entry_path(Path::new("../up")).is_err());
        assert!(sanitize_tar_entry_path(Path::new("a/../../b")).is_err());
        assert!(sanitize_tar_entry_path(Path::new("/absolute")).is_err());
        assert!(sanitize_tar_entry_path(Path::new("")).is_err());
    }

    // ---- 번들 루트 역산 ----

    #[test]
    fn bundle_root_resolves_only_real_bundle_layout() {
        assert_eq!(
            bundle_root_from_exe(Path::new(
                "/Applications/RunConfigManager.app/Contents/MacOS/run_config_manager"
            )),
            Some(PathBuf::from("/Applications/RunConfigManager.app"))
        );
        // dev 빌드/임의 경로 — 절대 Some을 반환해선 안 된다 (/Applications 오인 참사 방지).
        assert_eq!(
            bundle_root_from_exe(Path::new("/Users/x/target/release/run_config_manager")),
            None
        );
        assert_eq!(bundle_root_from_exe(Path::new("/usr/bin/foo")), None);
        assert_eq!(
            bundle_root_from_exe(Path::new("/Applications/run_config_manager")),
            None
        );
        assert_eq!(
            bundle_root_from_exe(Path::new(
                "/Applications/Fake/Contents/MacOS/run_config_manager"
            )),
            None
        );
    }

    // ---- 바이너리 교체 (Linux 경로) ----

    #[test]
    fn replaces_binary_content_and_exec_bit() {
        let dir = unique_temp_dir("replace_bin");
        let target = dir.join("run_config_manager");
        std::fs::write(&target, b"old").unwrap();
        replace_binary_at(&target, b"new-binary").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new-binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_linux_binary_entry_by_suffix() {
        let tgz = build_targz(&[
            (
                "run_config_manager-linux-x64/share/doc/README.md",
                b"doc".as_slice(),
                0o644,
            ),
            (
                "run_config_manager-linux-x64/bin/run_config_manager",
                b"\x7fELF-binary".as_slice(),
                0o755,
            ),
        ]);
        let bytes = extract_entry_from_targz(&tgz, Path::new("bin/run_config_manager")).unwrap();
        assert_eq!(bytes, b"\x7fELF-binary");
        assert!(extract_entry_from_targz(&tgz, Path::new("bin/missing")).is_err());
    }

    // ---- 번들 스왑 (macOS 경로) ----

    /// 가짜 `.app` 번들 디렉터리 생성 (마커 파일로 신·구 구분).
    fn make_fake_bundle(dir: &Path, name: &str, marker: &str) -> PathBuf {
        let bundle = dir.join(name);
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), marker).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/run_config_manager"), marker).unwrap();
        bundle
    }

    #[test]
    fn swap_installs_new_and_keeps_backup() {
        let dir = unique_temp_dir("swap_ok");
        let bundle = make_fake_bundle(&dir, "App.app", "old");
        let new_bundle = make_fake_bundle(&dir, "New.app", "new");
        let backup = dir.join("App.app.old-test");
        swap_bundle_at(&bundle, &new_bundle, &backup).unwrap();
        let installed = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert_eq!(installed, "new");
        let backed_up = std::fs::read_to_string(backup.join("Contents/Info.plist")).unwrap();
        assert_eq!(backed_up, "old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_rolls_back_when_install_fails() {
        let dir = unique_temp_dir("swap_rollback");
        let bundle = make_fake_bundle(&dir, "App.app", "old");
        let missing_new = dir.join("DoesNotExist.app"); // 2차 rename 실패 유도
        let backup = dir.join("App.app.old-test");
        let err = swap_bundle_at(&bundle, &missing_new, &backup).unwrap_err();
        assert!(err.contains("rolled back"), "unexpected error: {err}");
        // 역스왑으로 원본이 제자리에 복구되어야 한다.
        let restored = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert_eq!(restored, "old");
        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// macOS 적용 전체 통합: tar.gz → 스테이징 추출 → 구조 검사 → 스왑 → 스테이징 정리.
    #[test]
    fn apply_macos_end_to_end_with_fake_bundle() {
        let dir = unique_temp_dir("apply_e2e");
        let bundle = make_fake_bundle(&dir, "RunConfigManager.app", "old");
        let tgz = build_targz(&[
            (
                "RunConfigManager.app/Contents/Info.plist",
                b"new".as_slice(),
                0o644,
            ),
            (
                "RunConfigManager.app/Contents/MacOS/run_config_manager",
                b"new-binary".as_slice(),
                0o755,
            ),
        ]);
        let plan = apply_macos(&bundle, &tgz).unwrap();
        assert_eq!(
            plan,
            RelaunchPlan::MacOs {
                app_path: bundle.clone()
            }
        );
        // 새 번들이 제자리에 설치됨
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap(),
            "new"
        );
        // 백업이 남아 있음
        let backup = dir.join(format!("RunConfigManager.app.old-{}", std::process::id()));
        assert_eq!(
            std::fs::read_to_string(backup.join("Contents/Info.plist")).unwrap(),
            "old"
        );
        // 스테이징은 정리됨
        assert!(
            !dir.join(format!(".rcm-update-{}", std::process::id()))
                .exists()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_macos_rejects_bundle_without_plist() {
        let dir = unique_temp_dir("apply_noplist");
        let bundle = make_fake_bundle(&dir, "RunConfigManager.app", "old");
        let tgz = build_targz(&[(
            "RunConfigManager.app/Contents/MacOS/run_config_manager",
            b"new".as_slice(),
            0o755,
        )]);
        assert!(apply_macos(&bundle, &tgz).is_err());
        // 실패 시 기존 번들은 그대로
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 실제 릴리스 파이프라인 레시피로 만든 아카이브(진짜 .app)를 추출→스왑까지
    /// 통과시키는 수동 e2e. 기본 실행에선 건너뛴다:
    /// `RCM_E2E_ARCHIVE=<path to -app.tar.gz> cargo test -- --ignored real_archive`
    #[test]
    #[ignore = "requires RCM_E2E_ARCHIVE pointing at a CI-recipe .app tar.gz"]
    fn real_archive_applies_to_fake_bundle() {
        let archive = std::env::var("RCM_E2E_ARCHIVE").expect("set RCM_E2E_ARCHIVE");
        let payload = std::fs::read(&archive).expect("cannot read archive");
        let dir = unique_temp_dir("real_e2e");
        let bundle = make_fake_bundle(&dir, "RunConfigManager.app", "old");

        let plan = apply_macos(&bundle, &payload).unwrap();
        assert_eq!(
            plan,
            RelaunchPlan::MacOs {
                app_path: bundle.clone()
            }
        );
        // 설치된 번들은 가짜("old")가 아닌 실제 번들이어야 한다.
        let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert_ne!(plist, "old");
        assert!(plist.contains("CFBundleIdentifier"));
        let binary = bundle.join("Contents/MacOS/run_config_manager");
        assert!(binary.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "binary must stay executable");
        }
        // 설치된 번들의 코드서명 리소스 실(seal)이 tar 왕복에서 살아남았는지 검증.
        // AppleDouble(._*) 오염 회귀를 잡는 핵심 검사다 (macOS에서만 의미 있음).
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&bundle)
                .output()
                .expect("codesign must be runnable on macOS");
            assert!(
                output.status.success(),
                "codesign verification failed — bundle seal broken by extraction:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 잔여물 정리 ----

    #[test]
    fn cleanup_removes_backups_and_staging_but_not_current() {
        let dir = unique_temp_dir("cleanup");
        let current = make_fake_bundle(&dir, "RunConfigManager.app", "current");
        let backup = make_fake_bundle(&dir, "RunConfigManager.app.old-123", "old");
        let staging = dir.join(".rcm-update-123");
        std::fs::create_dir_all(&staging).unwrap();
        let unrelated = make_fake_bundle(&dir, "Other.app", "other");

        cleanup_update_artifacts_in(&dir, &current);

        assert!(current.exists());
        assert!(unrelated.exists());
        assert!(!backup.exists());
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_never_touches_lookalike_names() {
        // 우리가 만들지 않은 이름(다른 번들명, 비숫자 접미사, 유사 스테이징)은 절대
        // 삭제하지 않는다 — 부분 문자열 매칭 회귀 방지.
        let dir = unique_temp_dir("cleanup_lookalike");
        let current = make_fake_bundle(&dir, "RunConfigManager.app", "current");
        let other_backup = make_fake_bundle(&dir, "SomeApp.app.old-2024", "user backup");
        let non_numeric = make_fake_bundle(&dir, "RunConfigManager.app.old-backup", "manual");
        let staging_like = dir.join(".rcm-update-notes");
        std::fs::create_dir_all(&staging_like).unwrap();

        cleanup_update_artifacts_in(&dir, &current);

        assert!(other_backup.exists(), "unrelated app backup must survive");
        assert!(non_numeric.exists(), "non-numeric suffix must survive");
        assert!(staging_like.exists(), "lookalike staging must survive");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
