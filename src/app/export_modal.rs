//! 구성 내보내기(Export) 모달의 상태 타입과 업데이트 핸들러.
//!
//! `settings_modal`과 동일하게 부모 모듈(`app.rs`)의 `RunConfigManager`에 대한 `impl`을
//! 자식 모듈에 둔다. 선택 상태는 staging(`selected`)으로만 존재하며 Export/Cancel 시 폐기된다.

use super::{RunConfigManager, cancellable_status};
use crate::messages::Message;
use crate::models::{ConfigTypeData, RunConfiguration};
use crate::services::export_configurations;
use iced::Task;
use std::collections::HashSet;
use std::path::PathBuf;
use uuid::Uuid;

/// 내보내기 모달의 staging 상태.
///
/// 불변식: `selected`는 항상 현재 `configurations`에 존재하는 id의 부분집합이다.
/// (전체 선택 체크박스가 `selected.len() == configurations.len()`으로 판정하므로
/// dangling id가 섞이면 표시가 어긋난다.)
pub struct ExportModalState {
    /// 내보내기로 선택된 구성 id 집합
    pub selected: HashSet<Uuid>,
}

impl RunConfigManager {
    pub(super) fn handle_open_export_modal(&mut self) -> Task<Message> {
        if self.configurations.is_empty() {
            self.status_message = String::from("No configurations to export");
            return Task::none();
        }
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        // 기본은 전체 선택 — "현재 구성 내보내기"가 주 사용례이고 해제가 예외 조작이다.
        self.export_modal = Some(ExportModalState {
            selected: self.configurations.iter().map(|c| c.id).collect(),
        });
        Task::none()
    }

    pub(super) fn handle_cancel_export_modal(&mut self) -> Task<Message> {
        self.export_modal = None;
        Task::none()
    }

    pub(super) fn handle_export_modal_toggle_config(
        &mut self,
        config_id: Uuid,
        checked: bool,
    ) -> Task<Message> {
        let Some(modal) = self.export_modal.as_mut() else {
            return Task::none();
        };
        if checked {
            modal.selected.insert(config_id);
            // Compound를 선택하면 멤버도 함께 선택해 준다(1회성 편의 — 이후 개별 해제 가능).
            // 멤버 없이 내보내면 대상 파일에서 dangling id가 되므로 기본은 동반 선택.
            if let Some(ConfigTypeData::Compound { members, .. }) = self
                .configurations
                .iter()
                .find(|c| c.id == config_id)
                .map(|c| &c.type_data)
            {
                let existing: HashSet<Uuid> = self.configurations.iter().map(|c| c.id).collect();
                // 불변식 유지: 이미 dangling인 멤버 id는 selected에 넣지 않는다.
                modal
                    .selected
                    .extend(members.iter().filter(|id| existing.contains(id)));
            }
        } else {
            modal.selected.remove(&config_id);
        }
        Task::none()
    }

    pub(super) fn handle_export_modal_toggle_all(&mut self, checked: bool) -> Task<Message> {
        if let Some(modal) = self.export_modal.as_mut() {
            modal.selected = if checked {
                self.configurations.iter().map(|c| c.id).collect()
            } else {
                HashSet::new()
            };
        }
        Task::none()
    }

    pub(super) fn handle_confirm_export_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.export_modal.take() else {
            return Task::none();
        };
        // 리스트 순서를 보존해 내보낸다 (선택 순서와 무관하게 결과가 결정적).
        let selected: Vec<RunConfiguration> = self
            .configurations
            .iter()
            .filter(|c| modal.selected.contains(&c.id))
            .cloned()
            .collect();
        if selected.is_empty() {
            // Export 버튼이 비활성화되므로 정상 경로에선 도달하지 않는다(방어적).
            return Task::none();
        }
        self.status_message = format!("Exporting {} configuration(s)...", selected.len());
        Task::perform(
            export_configurations(selected),
            Message::ConfigurationsExported,
        )
    }

    pub(super) fn handle_configurations_exported(
        &mut self,
        result: Result<PathBuf, String>,
    ) -> Task<Message> {
        match result {
            Ok(path) => {
                self.status_message = format!("Exported: {}", path.display());
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Export");
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::RunConfigManager;
    use crate::models::{ConfigTypeData, RunConfiguration};
    use uuid::Uuid;

    fn manager_with_configs(names: &[&str]) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.configurations = names
            .iter()
            .map(|name| RunConfiguration {
                name: (*name).to_string(),
                ..RunConfiguration::default()
            })
            .collect();
        app
    }

    #[test]
    fn open_export_modal_selects_all_by_default() {
        let mut app = manager_with_configs(&["A", "B"]);
        let _ = app.handle_open_export_modal();
        let modal = app.export_modal.as_ref().expect("modal open");
        assert_eq!(modal.selected.len(), 2);
        assert!(
            app.configurations
                .iter()
                .all(|c| modal.selected.contains(&c.id))
        );
    }

    #[test]
    fn open_export_modal_without_configs_shows_status_and_stays_closed() {
        let mut app = manager_with_configs(&[]);
        let _ = app.handle_open_export_modal();
        assert!(app.export_modal.is_none());
        assert_eq!(app.status_message, "No configurations to export");
    }

    #[test]
    fn toggle_config_unchecks_and_rechecks() {
        let mut app = manager_with_configs(&["A", "B"]);
        let a_id = app.configurations[0].id;
        let _ = app.handle_open_export_modal();

        let _ = app.handle_export_modal_toggle_config(a_id, false);
        assert!(!app.export_modal.as_ref().unwrap().selected.contains(&a_id));

        let _ = app.handle_export_modal_toggle_config(a_id, true);
        assert!(app.export_modal.as_ref().unwrap().selected.contains(&a_id));
    }

    #[test]
    fn checking_compound_auto_selects_existing_members_only() {
        let mut app = manager_with_configs(&["A", "B", "Bundle"]);
        let a_id = app.configurations[0].id;
        let b_id = app.configurations[1].id;
        let bundle_id = app.configurations[2].id;
        let dangling = Uuid::new_v4();
        app.configurations[2].type_data = ConfigTypeData::Compound {
            members: vec![a_id, b_id, dangling],
            workspace: None,
        };

        let _ = app.handle_open_export_modal();
        let _ = app.handle_export_modal_toggle_all(false);
        let _ = app.handle_export_modal_toggle_config(bundle_id, true);

        let selected = &app.export_modal.as_ref().unwrap().selected;
        assert!(selected.contains(&bundle_id));
        assert!(selected.contains(&a_id) && selected.contains(&b_id));
        // dangling 멤버는 selected 불변식(현재 구성의 부분집합)을 위해 제외
        assert!(!selected.contains(&dangling));
    }

    #[test]
    fn toggle_all_fills_and_clears_selection() {
        let mut app = manager_with_configs(&["A", "B", "C"]);
        let _ = app.handle_open_export_modal();

        let _ = app.handle_export_modal_toggle_all(false);
        assert!(app.export_modal.as_ref().unwrap().selected.is_empty());

        let _ = app.handle_export_modal_toggle_all(true);
        assert_eq!(app.export_modal.as_ref().unwrap().selected.len(), 3);
    }

    #[test]
    fn confirm_with_empty_selection_closes_without_export_task() {
        let mut app = manager_with_configs(&["A"]);
        let _ = app.handle_open_export_modal();
        let _ = app.handle_export_modal_toggle_all(false);
        let _ = app.handle_confirm_export_modal();
        assert!(app.export_modal.is_none());
        // 상태 메시지가 "Exporting..."으로 바뀌지 않아야 한다 (export 미실행)
        assert!(!app.status_message.starts_with("Exporting"));
    }

    #[test]
    fn confirm_closes_modal_and_sets_exporting_status() {
        let mut app = manager_with_configs(&["A", "B"]);
        let b_id = app.configurations[1].id;
        let _ = app.handle_open_export_modal();
        let _ = app.handle_export_modal_toggle_config(b_id, false);
        let _ = app.handle_confirm_export_modal();
        assert!(app.export_modal.is_none());
        assert_eq!(app.status_message, "Exporting 1 configuration(s)...");
    }

    #[test]
    fn configurations_reload_closes_open_export_modal() {
        // Open 버튼/시작 자동 로드의 비동기 완료가 모달이 열린 뒤 도착하면
        // 선택 집합이 교체 전 구성의 id로 stale해진다 — 모달을 닫아야 한다.
        let mut app = manager_with_configs(&["Old"]);
        let _ = app.handle_open_export_modal();
        assert!(app.export_modal.is_some());

        let fresh = vec![RunConfiguration {
            name: String::from("Fresh"),
            ..RunConfiguration::default()
        }];
        let _ = app.handle_configurations_loaded(Ok(fresh));
        assert!(app.export_modal.is_none());
    }

    #[test]
    fn exported_result_updates_status() {
        let mut app = manager_with_configs(&["A"]);
        let _ = app.handle_configurations_exported(Ok(std::path::PathBuf::from("/tmp/x.json")));
        assert_eq!(app.status_message, "Exported: /tmp/x.json");

        let _ = app.handle_configurations_exported(Err(String::from("cancelled")));
        assert_eq!(app.status_message, "Export cancelled");

        let _ = app.handle_configurations_exported(Err(String::from("disk full")));
        assert_eq!(app.status_message, "Export failed: disk full");
    }
}
