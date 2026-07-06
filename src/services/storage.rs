use crate::models::RunConfiguration;
use crate::utils::DIALOG_CANCELLED;
use rfd::AsyncFileDialog;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 앱 설정 (마지막 파일 경로, 사용자 환경설정 등).
///
/// 신규 필드는 모두 `#[serde(default = ...)]`을 지정해, 이 필드가 없는 구버전 설정
/// 파일도 파싱 실패 없이 로드되며 누락분은 기본값으로 채워진다(하위호환).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// legacy: 파일 기반 시절의 마지막 작업 파일 경로. 구성 영속성은 앱 저장소로 전환되어,
    /// 이 값은 저장소 최초 시드(migration) 용도로만 읽는다. 구버전 설정 파일 호환을 위해 유지.
    pub last_file_path: Option<PathBuf>,
    /// Configuration 화면 좌우 분할 비율
    pub configuration_split_ratio: Option<f32>,
    /// 구성 실행 시 출력 헤더에 `Environment: ...` 라인을 표시할지 여부
    #[serde(default = "default_true")]
    pub show_environment_on_run: bool,
    /// 세션 터미널 출력 버퍼의 최대 보관 라인 수 (새 세션부터 적용)
    #[serde(default = "default_max_output_lines")]
    pub max_output_lines: usize,
    /// 새 세션의 자동 스크롤 초기 상태
    #[serde(default = "default_true")]
    pub default_auto_scroll: bool,
    /// 앱 시작 시 업데이트를 자동으로 확인할지 여부
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
}

/// serde 기본값 헬퍼: bool 필드의 기본은 `true` (켜짐). `#[derive(Default)]`의
/// bool 기본은 `false`라 직접 지정해야 한다.
fn default_true() -> bool {
    true
}

/// serde 기본값 헬퍼: 출력 라인 한도 기본값 (세션 버퍼 상한과 단일 출처 공유).
fn default_max_output_lines() -> usize {
    crate::models::DEFAULT_MAX_OUTPUT_LINES
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_file_path: None,
            configuration_split_ratio: None,
            show_environment_on_run: true,
            max_output_lines: default_max_output_lines(),
            default_auto_scroll: true,
            auto_check_updates: true,
        }
    }
}

/// 앱 설정 파일 경로. 홈 디렉터리를 찾을 수 없으면 `None`을 반환해 fail-closed한다.
/// (CWD에 설정을 쓰면 공유/공격자 쓰기 가능 위치에 남을 수 있으므로 현재 디렉터리 폴백 금지.)
fn settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|mut path| {
        path.push(".run_config_settings.json");
        path
    })
}

/// 앱 설정 로드. 파일 부재는 조용히 기본값, 읽기/파싱 실패는 로그 후 기본값
/// (손상된 설정을 조용히 삼키지 않도록 진단 출력).
pub fn load_settings() -> AppSettings {
    let Some(path) = settings_path() else {
        return AppSettings::default();
    };
    if !path.exists() {
        return AppSettings::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!(
                    "[Settings] Failed to parse {}: {e}; using defaults",
                    path.display()
                );
                AppSettings::default()
            }
        },
        Err(e) => {
            eprintln!(
                "[Settings] Failed to read {}: {e}; using defaults",
                path.display()
            );
            AppSettings::default()
        }
    }
}

/// 앱 설정 저장. 직렬화/쓰기 실패는 조용히 무시하지 않고 로그로 남긴다.
pub fn save_settings(settings: &AppSettings) {
    let Some(path) = settings_path() else {
        eprintln!("[Settings] No home directory; skipping settings persistence");
        return;
    };
    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("[Settings] Failed to write {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!("[Settings] Failed to serialize settings: {e}"),
    }
}

/// 현재 구성 파일 스키마 버전. 디스크 포맷이 호환 불가하게 바뀔 때마다 +1 하고
/// `migrate_config_file`에 변환 단계를 추가한다.
const CURRENT_CONFIG_VERSION: u32 = 1;

/// 디스크에 저장되는 구성 파일 형식 (버전 envelope).
///
/// 구버전 파일은 envelope 없이 베어 배열(`[...]`)로 저장되었으며,
/// `parse_config_file`이 두 형식을 모두 읽는다.
#[derive(Debug, Serialize, Deserialize)]
struct ConfigFile {
    version: u32,
    #[serde(default)]
    configurations: Vec<RunConfiguration>,
}

/// 버전 envelope을 현재 스키마로 마이그레이션.
///
/// 현재까지의 변경은 구조 수준에서 forward-compatible(미지 필드 무시, 누락 옵션 기본값)
/// 하므로 데이터 변환은 없다. 예: `Compound.workspace`(Option, serde default) 추가는 구·신
/// 버전 양방향 호환된다. 향후 호환 불가 변경 시 버전별 변환을 여기에 추가한다.
/// 앱이 지원하는 버전보다 새 파일은 손상시키지 않도록 명확한 에러로 거부한다.
fn migrate_config_file(file: ConfigFile) -> Result<Vec<RunConfiguration>, String> {
    if file.version > CURRENT_CONFIG_VERSION {
        return Err(format!(
            "Configuration file version {} is newer than this app supports ({}). Please update the app.",
            file.version, CURRENT_CONFIG_VERSION
        ));
    }
    Ok(file.configurations)
}

/// 구성 파일 내용을 파싱. 신버전 envelope(`{...}`)과 구버전 베어 배열(`[...]`)을 모두 지원.
///
/// serde_json은 선행 BOM을 허용하지 않으므로(수기 편집 파일 대비) 먼저 BOM을 제거한다.
fn parse_config_file(content: &str) -> Result<Vec<RunConfiguration>, String> {
    let trimmed = content.trim_start_matches('\u{FEFF}').trim_start();
    if trimmed.starts_with('[') {
        // 구버전: 버전 없는 베어 배열 (제거된 config_type 등 미지 필드는 serde가 무시)
        serde_json::from_str(trimmed).map_err(|e| format!("Deserialization error: {e}"))
    } else {
        let file: ConfigFile =
            serde_json::from_str(trimmed).map_err(|e| format!("Deserialization error: {e}"))?;
        migrate_config_file(file)
    }
}

/// 구성 목록을 현재 스키마 버전 envelope으로 직렬화.
fn serialize_config_file(configurations: Vec<RunConfiguration>) -> Result<String, String> {
    let file = ConfigFile {
        version: CURRENT_CONFIG_VERSION,
        configurations,
    };
    serde_json::to_string_pretty(&file).map_err(|e| format!("Serialization error: {e}"))
}

/// 앱 전용 구성 저장소 경로 (`data_dir/run_config_manager/configs.json`).
///
/// import/export 파일과 무관하게 구성이 항상 이 고정 위치에서 로드/저장된다.
/// `data_dir`을 찾을 수 없으면 `None`으로 fail-closed한다 (CWD 폴백 금지).
fn configs_store_path() -> Option<PathBuf> {
    dirs::data_dir().map(|mut path| {
        path.push("run_config_manager");
        path.push("configs.json");
        path
    })
}

/// 지정 경로에서 구성을 읽는다. 파일 부재는 빈 목록(`Ok`), 읽기/파싱 실패는 `Err`.
/// 설정 로드와 달리 파싱 실패를 조용히 삼키지 않고 호출 측이 상태로 표면화하도록 전달한다
/// (사용자가 "왜 구성이 안 보이나"를 알 수 있어야 하므로).
fn read_configs_at(path: &Path) -> Result<Vec<RunConfiguration>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path).map_err(|e| format!("File read error: {e}"))?;
    parse_config_file(&content)
}

/// 원자적 저장용 임시 파일 시퀀스 — 동시 저장 시 temp 경로 충돌을 막는다.
static STORE_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 지정 경로에 구성을 버전 envelope으로 쓴다. 상위 디렉터리를 생성한 뒤 임시 파일에 쓰고
/// rename으로 원자적 교체한다 — 쓰기 도중 크래시/전원차단으로 인한 torn write(잘린 파일)를
/// 막는다. 저장소가 유일한 진실의 원천이 되었으므로 원자성이 중요하다.
fn write_configs_at(path: &Path, configs: Vec<RunConfiguration>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "store path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create store directory: {e}"))?;
    let json = serialize_config_file(configs)?;
    let seq = STORE_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = parent.join(format!(".configs.{}.{}.tmp", std::process::id(), seq));
    std::fs::write(&tmp, json).map_err(|e| format!("File write error: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("File rename error: {e}"))
}

/// 저장소가 있으면 로드하고, 없으면 `legacy` 파일에서 1회 마이그레이션(시드) 후 반환한다.
/// - 저장소 존재: 저장소를 읽는다 (손상 시 `Err` 표면화).
/// - 저장소 없음 + legacy 유효(비어있지 않음): 저장소에 시드한 뒤 반환.
/// - 저장소 없음 + legacy 손상: `Err`로 표면화 (조용히 삼키지 않음 — 이동/삭제와 손상을 구분).
/// - 저장소 없음 + legacy 부재/빈 목록: 빈 목록 (신규 설치).
///
/// 경로를 명시로 받아 실제 `data_dir` 없이 종단 테스트할 수 있다.
fn load_or_migrate_at(
    store: &Path,
    legacy: Option<&Path>,
) -> Result<Vec<RunConfiguration>, String> {
    if store.exists() {
        return read_configs_at(store);
    }
    if let Some(legacy) = legacy
        && legacy.exists()
    {
        let configs = read_configs_at(legacy)?; // 손상 시 Err로 표면화
        if !configs.is_empty() {
            write_configs_at(store, configs.clone())?;
        }
        return Ok(configs);
    }
    Ok(Vec::new())
}

/// startup 구성 로드 진입점. 앱 저장소에서 로드하되, 저장소가 없으면 legacy 작업 파일
/// (`last_file_path`)에서 1회 마이그레이션한다. `data_dir`을 못 찾으면 `Err`로 fail-closed
/// 한다 (신규 설치로 오인해 조용히 빈 목록을 보이지 않도록 — save_to_store와 대칭).
///
/// 모든 파일 I/O가 이 async 본문에서 일어나므로, 반환된 Task를 폴링하지 않는 단위 테스트는
/// 실제 데이터 디렉터리를 건드리지 않는다.
pub async fn load_or_migrate_store(
    legacy: Option<PathBuf>,
) -> Result<Vec<RunConfiguration>, String> {
    let store = configs_store_path()
        .ok_or_else(|| "No data directory available for the configuration store".to_string())?;
    load_or_migrate_at(&store, legacy.as_deref())
}

/// 구성 목록을 앱 저장소에 저장한다 (Save). 저장 위치는 항상 고정 저장소이며
/// 다이얼로그를 띄우지 않는다 (import/export와 분리).
pub async fn save_to_store(configs: Vec<RunConfiguration>) -> Result<PathBuf, String> {
    let path = configs_store_path()
        .ok_or_else(|| "No data directory available for the configuration store".to_string())?;
    write_configs_at(&path, configs)?;
    Ok(path)
}

/// 구성 목록을 버전 envelope으로 직렬화해 `target_path`(없으면 저장 다이얼로그로
/// 선택한 경로)에 쓴다. save/export의 공통 경로.
///
/// # Returns
/// * `Ok(PathBuf)` - 저장된 파일 경로
/// * `Err(String)` - 취소(`DIALOG_CANCELLED`) 또는 직렬화/쓰기 실패
async fn save_configs_to(
    configs: Vec<RunConfiguration>,
    target_path: Option<PathBuf>,
    suggested_name: &str,
) -> Result<PathBuf, String> {
    let file_path = match target_path {
        Some(path) => path,
        // AsyncFileDialog는 내부적으로 메인 스레드로 마샬링하므로, Task::perform가
        // 워커 스레드에서 폴링해도 안전하다 (macOS AppKit runModal()은 메인 스레드 전용).
        None => AsyncFileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(suggested_name)
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf())
            .ok_or_else(|| DIALOG_CANCELLED.to_string())?,
    };

    let json = serialize_config_file(configs)?;

    std::fs::write(&file_path, json).map_err(|e| format!("File write error: {e}"))?;

    Ok(file_path)
}

/// 선택된 구성들을 사용자가 지정한 파일로 내보내기.
///
/// 저장(save)과 달리 현재 파일 경로를 건드리지 않고 항상 저장 다이얼로그를 띄운다.
/// 파일 포맷은 저장과 동일한 버전 envelope이므로 Open으로 그대로 다시 열 수 있다.
///
/// # Returns
/// * `Ok(PathBuf)` - 내보낸 파일 경로
/// * `Err(String)` - 취소(`DIALOG_CANCELLED`) 또는 직렬화/쓰기 실패
pub async fn export_configurations(configs: Vec<RunConfiguration>) -> Result<PathBuf, String> {
    save_configs_to(configs, None, "configurations-export.json").await
}

/// 임의 텍스트(세션 출력 등)를 사용자가 선택한 파일로 저장.
///
/// off-main 안전성을 위해 `AsyncFileDialog` 사용 (`save_configs_to` 참고).
///
/// # Returns
/// * `Ok(PathBuf)` - 저장된 파일 경로
/// * `Err(String)` - 취소(`DIALOG_CANCELLED`) 또는 쓰기 실패
pub async fn export_text(content: String, suggested_name: String) -> Result<PathBuf, String> {
    let file_path = AsyncFileDialog::new()
        .add_filter("Text", &["txt", "log"])
        .set_file_name(suggested_name)
        .save_file()
        .await
        .map(|handle| handle.path().to_path_buf())
        .ok_or_else(|| DIALOG_CANCELLED.to_string())?;

    std::fs::write(&file_path, content).map_err(|e| format!("File write error: {e}"))?;

    Ok(file_path)
}

/// 가져오기(Import)용으로 사용자가 선택한 파일에서 구성 목록 읽기.
/// 파일을 파싱만 하며 현재 구성/작업 파일 경로에는 영향을 주지 않는다
/// (병합 대상 선택과 반영은 호출 측 import 모달이 담당).
///
/// # Returns
/// * `Ok((Vec<RunConfiguration>, PathBuf))` - 파일의 구성 목록과 파일 경로
/// * `Err(String)` - 취소(`DIALOG_CANCELLED`) 또는 읽기/파싱 실패
pub async fn import_configurations() -> Result<(Vec<RunConfiguration>, PathBuf), String> {
    // off-main 안전성을 위해 AsyncFileDialog 사용 (save_configs_to 참고).
    let file_path = AsyncFileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
        .ok_or_else(|| DIALOG_CANCELLED.to_string())?;

    let content =
        std::fs::read_to_string(&file_path).map_err(|e| format!("File read error: {e}"))?;

    let configs = parse_config_file(&content)?;

    Ok((configs, file_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<RunConfiguration> {
        vec![RunConfiguration {
            name: String::from("A"),
            ..RunConfiguration::default()
        }]
    }

    #[test]
    fn round_trips_through_versioned_envelope() {
        let json = serialize_config_file(sample()).unwrap();
        assert!(json.contains("\"version\""));
        let parsed = parse_config_file(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "A");
    }

    #[test]
    fn serialized_file_declares_current_version() {
        let json = serialize_config_file(vec![]).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["version"].as_u64(),
            Some(u64::from(CURRENT_CONFIG_VERSION))
        );
    }

    #[test]
    fn loads_legacy_bare_array_with_removed_config_type_field() {
        // 구버전 포맷: 버전 envelope 없는 베어 배열 + 제거된 config_type 필드 포함
        let legacy = r#"[
          {"id":"00000000-0000-0000-0000-000000000001","name":"Legacy",
           "config_type":"Application","working_directory":".",
           "environment_variables":{},
           "type_data":{"type":"Application","command":"echo","arguments":"hi"}}
        ]"#;
        let parsed = parse_config_file(legacy).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Legacy");
        assert_eq!(
            parsed[0].config_type(),
            crate::models::ConfigurationType::Application
        );
    }

    #[test]
    fn rejects_newer_version_gracefully() {
        let future = r#"{"version":9999,"configurations":[]}"#;
        let err = parse_config_file(future).unwrap_err();
        assert!(
            err.contains("newer"),
            "expected a version error, got: {err}"
        );
    }

    #[test]
    fn empty_configurations_field_defaults_to_empty() {
        let parsed = parse_config_file(r#"{"version":1}"#).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn parses_content_with_leading_bom() {
        // 메모장 등으로 저장된 BOM 선행 파일도 로드 (envelope/배열 양쪽)
        let bom = "\u{FEFF}";
        assert!(
            parse_config_file(&format!("{bom}{{\"version\":1,\"configurations\":[]}}")).is_ok()
        );
        assert!(parse_config_file(&format!("{bom}[]")).is_ok());
    }

    #[test]
    fn app_settings_legacy_json_defaults_new_fields() {
        // 신규 필드가 없는 구버전 설정 파일도 파싱되며 누락분은 기본값으로 채워진다(하위호환).
        let legacy = r#"{"last_file_path":null,"configuration_split_ratio":0.3}"#;
        let settings: AppSettings = serde_json::from_str(legacy).unwrap();
        assert!(settings.show_environment_on_run);
        assert_eq!(
            settings.max_output_lines,
            crate::models::DEFAULT_MAX_OUTPUT_LINES
        );
        assert!(settings.default_auto_scroll);
        assert!(settings.auto_check_updates);
    }

    #[test]
    fn app_settings_round_trip_preserves_fields() {
        let settings = AppSettings {
            show_environment_on_run: false,
            max_output_lines: 12_345,
            default_auto_scroll: false,
            auto_check_updates: false,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!back.show_environment_on_run);
        assert_eq!(back.max_output_lines, 12_345);
        assert!(!back.default_auto_scroll);
        assert!(!back.auto_check_updates);
    }

    // ---- 앱 전용 저장소 (configs) ----

    static STORE_TEST_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// 테스트 격리용 고유 임시 경로 (외부 crate 없이 pid+카운터로 충돌 회피).
    fn unique_temp_dir(tag: &str) -> PathBuf {
        let n = STORE_TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("rcm_store_{}_{}_{}", std::process::id(), n, tag))
    }

    #[test]
    fn store_write_then_read_round_trips() {
        let dir = unique_temp_dir("rt");
        let file = dir.join("configs.json");
        write_configs_at(&file, sample()).unwrap();
        let loaded = read_configs_at(&file).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "A");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_read_missing_is_empty() {
        // 저장소 파일이 없으면 빈 목록(신규 설치) — 에러가 아님.
        let file = unique_temp_dir("missing").join("configs.json");
        assert!(read_configs_at(&file).unwrap().is_empty());
    }

    #[test]
    fn store_read_corrupt_surfaces_error() {
        // 손상된 저장소는 조용히 빈 목록이 아니라 Err로 표면화된다.
        let dir = unique_temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("configs.json");
        std::fs::write(&file, "{ this is not valid json").unwrap();
        assert!(read_configs_at(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_seeds_store_from_legacy_when_store_missing() {
        let base = unique_temp_dir("mig");
        let store = base.join("store/configs.json");
        let legacy = base.join("legacy.json");
        write_configs_at(&legacy, sample()).unwrap();
        // 저장소 없음 + legacy 유효 → 반환 + 저장소 시드
        let loaded = load_or_migrate_at(&store, Some(&legacy)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(store.exists());
        assert_eq!(read_configs_at(&store).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_prefers_existing_store_over_legacy() {
        // 저장소가 있으면 legacy를 무시하고 저장소를 읽는다 (덮어쓰지 않음).
        let base = unique_temp_dir("mig_store");
        let store = base.join("store/configs.json");
        let legacy = base.join("legacy.json");
        write_configs_at(&store, Vec::new()).unwrap(); // 저장소는 비어 있음
        write_configs_at(&legacy, sample()).unwrap(); // legacy엔 1개
        let loaded = load_or_migrate_at(&store, Some(&legacy)).unwrap();
        assert!(loaded.is_empty()); // 저장소(빈 것)를 따른다
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_surfaces_corrupt_legacy_as_error() {
        // 저장소 없음 + legacy 손상 → 조용히 빈 목록이 아니라 Err (이동/삭제와 손상을 구분).
        let base = unique_temp_dir("mig_corrupt");
        std::fs::create_dir_all(&base).unwrap();
        let store = base.join("store/configs.json");
        let legacy = base.join("legacy.json");
        std::fs::write(&legacy, "{ not valid json").unwrap();
        assert!(load_or_migrate_at(&store, Some(&legacy)).is_err());
        assert!(!store.exists()); // 손상 시 저장소를 만들지 않는다
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_empty_when_no_store_and_no_or_empty_legacy() {
        let base = unique_temp_dir("mig_fresh");
        let store = base.join("store/configs.json");
        // legacy 없음 → 빈 목록, 저장소 미생성 (신규 설치)
        assert!(load_or_migrate_at(&store, None).unwrap().is_empty());
        assert!(!store.exists());
        // legacy가 빈 목록 → 빈 목록, 저장소 미생성
        let legacy = base.join("legacy.json");
        write_configs_at(&legacy, Vec::new()).unwrap();
        assert!(
            load_or_migrate_at(&store, Some(&legacy))
                .unwrap()
                .is_empty()
        );
        assert!(!store.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migrate_surfaces_corrupt_store_as_error() {
        // 저장소 자체가 손상이면 Err로 표면화 (조용히 빈 목록이 아님).
        let base = unique_temp_dir("mig_store_corrupt");
        std::fs::create_dir_all(&base).unwrap();
        let store = base.join("configs.json");
        std::fs::write(&store, "{ not valid json").unwrap();
        assert!(load_or_migrate_at(&store, None).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
