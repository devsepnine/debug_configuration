use crate::models::RunConfiguration;
use crate::utils::DIALOG_CANCELLED;
use rfd::AsyncFileDialog;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 앱 설정 (마지막 파일 경로 등)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    /// 마지막으로 사용한 구성 파일 경로
    pub last_file_path: Option<PathBuf>,
    /// Configuration 화면 좌우 분할 비율
    pub configuration_split_ratio: Option<f32>,
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

/// 특정 경로에서 구성 목록 로드
pub async fn load_from_path(path: PathBuf) -> Result<Vec<RunConfiguration>, String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| format!("File read error: {e}"))?;

    parse_config_file(&content)
}

/// 구성 목록을 현재 파일 또는 사용자가 선택한 파일로 저장
///
/// # Returns
/// * `Ok(PathBuf)` - 저장된 파일 경로
/// * `Err(String)` - 저장 실패 시 에러 메시지
pub async fn save_configurations(
    configs: Vec<RunConfiguration>,
    current_path: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let file_path = match current_path {
        Some(path) => path,
        // AsyncFileDialog는 내부적으로 메인 스레드로 마샬링하므로, Task::perform가
        // 워커 스레드에서 폴링해도 안전하다 (macOS AppKit runModal()은 메인 스레드 전용).
        None => AsyncFileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("configurations.json")
            .save_file()
            .await
            .map(|handle| handle.path().to_path_buf())
            .ok_or_else(|| DIALOG_CANCELLED.to_string())?,
    };

    let json = serialize_config_file(configs)?;

    std::fs::write(&file_path, json).map_err(|e| format!("File write error: {e}"))?;

    Ok(file_path)
}

/// 임의 텍스트(세션 출력 등)를 사용자가 선택한 파일로 저장.
///
/// off-main 안전성을 위해 `AsyncFileDialog` 사용 (save_configurations 참고).
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

/// 사용자가 선택한 파일에서 구성 목록 열기
///
/// # Returns
/// * `Ok((Vec<RunConfiguration>, PathBuf))` - 열린 구성 목록과 파일 경로
/// * `Err(String)` - 열기 실패 시 에러 메시지
pub async fn open_configurations() -> Result<(Vec<RunConfiguration>, PathBuf), String> {
    // off-main 안전성을 위해 AsyncFileDialog 사용 (save_configurations 참고).
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
}
