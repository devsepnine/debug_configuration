use crate::models::RunConfiguration;
use crate::utils::DIALOG_CANCELLED;
use rfd::FileDialog;
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

/// 앱 설정 파일 경로
fn get_settings_path() -> PathBuf {
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(".run_config_settings.json");
    path
}

/// 앱 설정 로드
pub fn load_settings() -> AppSettings {
    let path = get_settings_path();
    if !path.exists() {
        return AppSettings::default();
    }

    std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// 앱 설정 저장
pub fn save_settings(settings: &AppSettings) {
    let path = get_settings_path();
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

/// 특정 경로에서 구성 목록 로드
pub async fn load_from_path(path: PathBuf) -> Result<Vec<RunConfiguration>, String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| format!("File read error: {e}"))?;

    serde_json::from_str(&content).map_err(|e| format!("Deserialization error: {e}"))
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
        None => FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("configurations.json")
            .save_file()
            .ok_or_else(|| DIALOG_CANCELLED.to_string())?,
    };

    let json =
        serde_json::to_string_pretty(&configs).map_err(|e| format!("Serialization error: {e}"))?;

    std::fs::write(&file_path, json).map_err(|e| format!("File write error: {e}"))?;

    Ok(file_path)
}

/// 사용자가 선택한 파일에서 구성 목록 열기
///
/// # Returns
/// * `Ok((Vec<RunConfiguration>, PathBuf))` - 열린 구성 목록과 파일 경로
/// * `Err(String)` - 열기 실패 시 에러 메시지
pub async fn open_configurations() -> Result<(Vec<RunConfiguration>, PathBuf), String> {
    let file_path = FileDialog::new()
        .add_filter("JSON", &["json"])
        .pick_file()
        .ok_or_else(|| DIALOG_CANCELLED.to_string())?;

    let content =
        std::fs::read_to_string(&file_path).map_err(|e| format!("File read error: {e}"))?;

    let configs: Vec<RunConfiguration> =
        serde_json::from_str(&content).map_err(|e| format!("Deserialization error: {e}"))?;

    Ok((configs, file_path))
}
