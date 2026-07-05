//! 구성 가져오기(Import) 모달의 상태 타입과 업데이트 핸들러.
//!
//! Open(전체 교체)을 대체하는 기능: 파일에서 읽은 구성 중 선택한 것만 현재 목록에
//! **병합**한다. 같은 id가 이미 있으면 제자리 교체(같은 UUID = 같은 구성의 다른 버전),
//! 새 id는 목록 끝에 추가한다. 현재 작업 파일 경로(`last_file_path`)는 건드리지 않는다.

use super::{RunConfigManager, cancellable_status};
use crate::messages::Message;
use crate::models::{ConfigTypeData, RunConfiguration};
use crate::services::import_configurations;
use iced::Task;
use std::collections::HashSet;
use std::path::PathBuf;
use uuid::Uuid;

/// 가져오기 모달의 staging 상태. 파일 내용은 메모리에만 존재하며 Import/Cancel 시 폐기된다.
pub struct ImportModalState {
    /// 가져올 파일 경로 (모달 헤더 표시용)
    pub source_path: PathBuf,
    /// 파일에서 읽은 구성 목록 (파일 내 순서 유지, id 중복은 첫 항목만 남김)
    pub configs: Vec<RunConfiguration>,
    /// 가져오기로 선택된 구성 id 집합 (`configs`에 존재하는 id의 부분집합)
    pub selected: HashSet<Uuid>,
}

impl RunConfigManager {
    pub(super) fn handle_import_configurations(&mut self) -> Task<Message> {
        self.status_message = String::from("Importing configurations...");
        Task::perform(import_configurations(), Message::ImportFileLoaded)
    }

    pub(super) fn handle_import_file_loaded(
        &mut self,
        result: Result<(Vec<RunConfiguration>, PathBuf), String>,
    ) -> Task<Message> {
        match result {
            Ok((configs, path)) => {
                // 수기 편집 파일 방어: 파일 안에서 id가 중복되면 첫 항목만 남긴다
                // (목록의 id 유일성 불변식을 병합 시점에 지키기 위함).
                let mut seen = HashSet::new();
                let configs: Vec<RunConfiguration> =
                    configs.into_iter().filter(|c| seen.insert(c.id)).collect();
                if configs.is_empty() {
                    self.status_message = format!("No configurations found in {}", path.display());
                    return Task::none();
                }
                // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
                self.close_all_modals();
                // 기본 선택: 새 구성만. 이미 존재하는 id(교체 대상)는 로컬 수정본을
                // 실수로 덮어쓰지 않도록 명시적으로 체크해야 가져와진다.
                let existing: HashSet<Uuid> = self.configurations.iter().map(|c| c.id).collect();
                let selected = configs
                    .iter()
                    .map(|c| c.id)
                    .filter(|id| !existing.contains(id))
                    .collect();
                self.status_message = format!("Import: {}", path.display());
                self.import_modal = Some(ImportModalState {
                    source_path: path,
                    configs,
                    selected,
                });
                Task::none()
            }
            Err(error) => {
                self.status_message = cancellable_status(&error, "Import");
                Task::none()
            }
        }
    }

    pub(super) fn handle_cancel_import_modal(&mut self) -> Task<Message> {
        if self.import_modal.take().is_some() {
            self.status_message = String::from("Import cancelled");
        }
        Task::none()
    }

    pub(super) fn handle_import_modal_toggle_config(
        &mut self,
        config_id: Uuid,
        checked: bool,
    ) -> Task<Message> {
        let Some(modal) = self.import_modal.as_mut() else {
            return Task::none();
        };
        if checked {
            modal.selected.insert(config_id);
            // Compound를 선택하면 파일 안의 멤버도 함께 선택해 준다(1회성 편의).
            // 단, 이미 현재 목록에 있는 멤버는 가져오지 않아도 참조가 살아 있고,
            // 자동 체크하면 의도치 않은 "교체"가 되므로 제외한다.
            if let Some(ConfigTypeData::Compound { members, .. }) = modal
                .configs
                .iter()
                .find(|c| c.id == config_id)
                .map(|c| &c.type_data)
            {
                let in_file: HashSet<Uuid> = modal.configs.iter().map(|c| c.id).collect();
                let existing: HashSet<Uuid> = self.configurations.iter().map(|c| c.id).collect();
                let auto: Vec<Uuid> = members
                    .iter()
                    .filter(|id| in_file.contains(id) && !existing.contains(id))
                    .copied()
                    .collect();
                modal.selected.extend(auto);
            }
        } else {
            modal.selected.remove(&config_id);
        }
        Task::none()
    }

    pub(super) fn handle_import_modal_toggle_all(&mut self, checked: bool) -> Task<Message> {
        if let Some(modal) = self.import_modal.as_mut() {
            modal.selected = if checked {
                modal.configs.iter().map(|c| c.id).collect()
            } else {
                HashSet::new()
            };
        }
        Task::none()
    }

    pub(super) fn handle_confirm_import_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.import_modal.take() else {
            return Task::none();
        };
        let ImportModalState {
            configs, selected, ..
        } = modal;
        let to_import: Vec<RunConfiguration> = configs
            .into_iter()
            .filter(|c| selected.contains(&c.id))
            .collect();
        if to_import.is_empty() {
            // Import 버튼이 비활성화되므로 정상 경로에선 도달하지 않는다(방어적).
            return Task::none();
        }

        let mut added = 0usize;
        let mut replaced = 0usize;
        for config in to_import {
            // 교체 시 환경변수 메인 input 캐시가 stale해지므로 무효화 (신규 id는 no-op).
            self.env_bulk_inputs.remove(&config.id);
            if let Some(existing) = self.configurations.iter_mut().find(|c| c.id == config.id) {
                *existing = config;
                replaced += 1;
            } else {
                self.configurations.push(config);
                added += 1;
            }
        }

        if self.selected_config_index.is_none() && !self.configurations.is_empty() {
            self.selected_config_index = Some(0);
        }
        self.status_message = if replaced > 0 {
            format!(
                "Imported {} configuration(s) ({replaced} replaced)",
                added + replaced
            )
        } else {
            format!("Imported {added} configuration(s)")
        };
        // 가져온 Node 구성의 package.json/scripts 메타데이터를 백그라운드에서 로드.
        self.load_node_metadata_for_current_configurations()
    }
}

#[cfg(test)]
mod tests {
    use super::RunConfigManager;
    use crate::models::{ConfigTypeData, RunConfiguration};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn named(name: &str) -> RunConfiguration {
        RunConfiguration {
            name: name.to_string(),
            ..RunConfiguration::default()
        }
    }

    fn manager_with_configs(names: &[&str]) -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.configurations = names.iter().map(|name| named(name)).collect();
        app
    }

    fn load_file(app: &mut RunConfigManager, configs: Vec<RunConfiguration>) {
        let _ = app.handle_import_file_loaded(Ok((configs, PathBuf::from("/tmp/import.json"))));
    }

    #[test]
    fn file_loaded_selects_only_new_ids_by_default() {
        let mut app = manager_with_configs(&["Mine"]);
        let colliding = app.configurations[0].clone();
        let fresh = named("Fresh");
        let fresh_id = fresh.id;
        load_file(&mut app, vec![colliding.clone(), fresh]);

        let modal = app.import_modal.as_ref().expect("modal open");
        assert_eq!(modal.configs.len(), 2);
        assert!(modal.selected.contains(&fresh_id));
        // 교체 대상(동일 id)은 기본 미선택 — 로컬 수정 보호
        assert!(!modal.selected.contains(&colliding.id));
    }

    #[test]
    fn file_loaded_dedupes_duplicate_ids_keeping_first() {
        let mut app = manager_with_configs(&[]);
        let a = named("A");
        let mut a_dup = named("A-dup");
        a_dup.id = a.id;
        load_file(&mut app, vec![a, a_dup]);

        let modal = app.import_modal.as_ref().expect("modal open");
        assert_eq!(modal.configs.len(), 1);
        assert_eq!(modal.configs[0].name, "A");
    }

    #[test]
    fn empty_file_shows_status_without_modal() {
        let mut app = manager_with_configs(&["Mine"]);
        load_file(&mut app, vec![]);
        assert!(app.import_modal.is_none());
        assert!(app.status_message.starts_with("No configurations found"));
    }

    #[test]
    fn load_error_sets_status() {
        let mut app = manager_with_configs(&[]);
        let _ = app.handle_import_file_loaded(Err(String::from("cancelled")));
        assert_eq!(app.status_message, "Import cancelled");

        let _ = app.handle_import_file_loaded(Err(String::from("bad json")));
        assert_eq!(app.status_message, "Import failed: bad json");
    }

    #[test]
    fn checking_compound_auto_selects_file_members_except_already_existing() {
        let mut app = manager_with_configs(&["Existing"]);
        let existing_id = app.configurations[0].id;

        let fresh = named("FreshMember");
        let fresh_id = fresh.id;
        let existing_copy = app.configurations[0].clone();
        let mut bundle = named("Bundle");
        let bundle_id = bundle.id;
        bundle.type_data = ConfigTypeData::Compound {
            members: vec![fresh_id, existing_id, Uuid::new_v4()],
            workspace: None,
        };
        load_file(&mut app, vec![fresh, existing_copy, bundle]);

        let _ = app.handle_import_modal_toggle_all(false);
        let _ = app.handle_import_modal_toggle_config(bundle_id, true);

        let selected = &app.import_modal.as_ref().unwrap().selected;
        assert!(selected.contains(&bundle_id));
        assert!(selected.contains(&fresh_id));
        // 이미 현재 목록에 있는 멤버는 자동 체크하지 않는다 (의도치 않은 교체 방지)
        assert!(!selected.contains(&existing_id));
    }

    #[test]
    fn confirm_appends_new_and_replaces_existing_in_place() {
        let mut app = manager_with_configs(&["Keep", "Target"]);
        let target_id = app.configurations[1].id;

        let mut updated = named("Target v2");
        updated.id = target_id;
        let fresh = named("Fresh");
        let fresh_id = fresh.id;
        load_file(&mut app, vec![updated, fresh]);
        // 교체 대상까지 전부 선택
        let _ = app.handle_import_modal_toggle_all(true);
        let _ = app.handle_confirm_import_modal();

        assert!(app.import_modal.is_none());
        let names: Vec<&str> = app.configurations.iter().map(|c| c.name.as_str()).collect();
        // 교체는 제자리(인덱스 1), 신규는 끝에 추가
        assert_eq!(names, vec!["Keep", "Target v2", "Fresh"]);
        assert_eq!(app.configurations[1].id, target_id);
        assert_eq!(app.configurations[2].id, fresh_id);
        assert_eq!(
            app.status_message,
            "Imported 2 configuration(s) (1 replaced)"
        );
    }

    #[test]
    fn confirm_invalidates_env_cache_for_replaced_config() {
        let mut app = manager_with_configs(&["Target"]);
        let target_id = app.configurations[0].id;
        app.env_bulk_inputs.insert(target_id, String::from("OLD=1"));

        let mut updated = named("Target v2");
        updated.id = target_id;
        load_file(&mut app, vec![updated]);
        let _ = app.handle_import_modal_toggle_all(true);
        let _ = app.handle_confirm_import_modal();

        assert!(!app.env_bulk_inputs.contains_key(&target_id));
    }

    #[test]
    fn confirm_selects_first_config_when_nothing_was_selected() {
        let mut app = manager_with_configs(&[]);
        app.selected_config_index = None;
        load_file(&mut app, vec![named("A")]);
        let _ = app.handle_confirm_import_modal();
        assert_eq!(app.selected_config_index, Some(0));
        assert_eq!(app.status_message, "Imported 1 configuration(s)");
    }

    #[test]
    fn confirm_with_empty_selection_closes_without_merge() {
        let mut app = manager_with_configs(&["Mine"]);
        let colliding = app.configurations[0].clone();
        load_file(&mut app, vec![colliding]);
        // 기본 선택이 비어 있음 (전부 교체 대상) — 그대로 확정
        let _ = app.handle_confirm_import_modal();
        assert!(app.import_modal.is_none());
        assert_eq!(app.configurations.len(), 1);
        assert!(!app.status_message.starts_with("Imported"));
    }

    #[test]
    fn cancel_discards_staging_and_sets_status() {
        let mut app = manager_with_configs(&[]);
        load_file(&mut app, vec![named("A")]);
        assert!(app.import_modal.is_some());
        let _ = app.handle_cancel_import_modal();
        assert!(app.import_modal.is_none());
        assert_eq!(app.status_message, "Import cancelled");
        assert!(app.configurations.is_empty());
    }

    #[test]
    fn file_loaded_closes_other_open_modals() {
        // 파일 다이얼로그가 떠 있는 동안 앱은 조작 불가(runModal)지만, 결과 도착
        // 시점에 다른 모달이 열려 있어도 "새로 열리는 모달이 이긴다" 계약을 지킨다.
        let mut app = manager_with_configs(&["A"]);
        let _ = app.handle_open_export_modal();
        assert!(app.export_modal.is_some());

        load_file(&mut app, vec![named("B")]);
        assert!(app.export_modal.is_none());
        assert!(app.import_modal.is_some());
    }

    #[test]
    fn configurations_reload_closes_open_import_modal() {
        // 시작 자동 로드의 비동기 완료가 모달이 열린 뒤 도착하면 병합 대상이
        // 사용자가 봤던 목록과 달라진다 — 모달을 닫아 명시적 재시도를 유도한다.
        let mut app = manager_with_configs(&["Old"]);
        load_file(&mut app, vec![named("A")]);
        assert!(app.import_modal.is_some());

        let _ = app.handle_configurations_loaded(Ok(vec![named("Fresh")]));
        assert!(app.import_modal.is_none());
    }
}
