//! 구성 삭제 확인 모달의 상태 타입과 업데이트 핸들러.
//!
//! 목록의 Delete 버튼은 즉시 삭제하는 대신 이 모달을 띄운다 — 삭제는 undo가 없는
//! 파괴적 조작이라 원클릭 오조작을 막는다. 대상은 index가 아니라 id로 보관해,
//! 모달이 떠 있는 동안의 목록 변동(불가능에 가깝지만 방어적)에도 엉뚱한 항목을
//! 지우지 않는다. 확정 시 기존 삭제 경로(`handle_delete_configuration`)를 재사용한다.

use super::RunConfigManager;
use crate::messages::Message;
use iced::Task;
use uuid::Uuid;

/// 삭제 확인 모달의 상태.
pub struct ConfirmDeleteModalState {
    /// 삭제 대상 구성 id.
    pub config_id: Uuid,
    /// 삭제 대상 구성 이름 (모달 본문 표시용).
    pub name: String,
    /// 이 구성을 멤버로 참조 중인 Compound 구성 이름들. 비어 있지 않으면
    /// "참조가 함께 제거된다" 경고를 표시한다.
    pub referencing_compounds: Vec<String>,
}

impl RunConfigManager {
    /// Delete 버튼 → 확인 모달 열기. 대상이 없으면 no-op.
    pub(super) fn handle_request_delete_configuration(
        &mut self,
        index_opt: Option<usize>,
    ) -> Task<Message> {
        let index = index_opt.or(self.selected_config_index);
        let Some(config) = index.and_then(|idx| self.configurations.get(idx)) else {
            return Task::none();
        };
        let config_id = config.id;
        let name = config.name.clone();
        let referencing_compounds = self
            .configurations
            .iter()
            .filter(|other| {
                other
                    .type_data
                    .compound_members()
                    .is_some_and(|members| members.contains(&config_id))
            })
            .map(|other| other.name.clone())
            .collect();
        // 한 번에 하나의 모달만 (계약과 근거는 close_all_modals 참고).
        self.close_all_modals();
        self.confirm_delete_modal = Some(ConfirmDeleteModalState {
            config_id,
            name,
            referencing_compounds,
        });
        Task::none()
    }

    /// 모달의 Delete 확정 → 실제 삭제 (기존 삭제 경로 재사용).
    pub(super) fn handle_confirm_delete_configuration(&mut self) -> Task<Message> {
        let Some(state) = self.confirm_delete_modal.take() else {
            return Task::none();
        };
        let Some(idx) = self
            .configurations
            .iter()
            .position(|c| c.id == state.config_id)
        else {
            // 모달이 떠 있는 동안 대상이 사라진 극단 케이스 — 조용히 닫는다.
            return Task::none();
        };
        self.handle_delete_configuration(Some(idx))
    }

    /// 모달 취소 (Cancel/X/Esc).
    pub(super) fn handle_cancel_delete_configuration(&mut self) -> Task<Message> {
        self.confirm_delete_modal = None;
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use crate::app::RunConfigManager;
    use crate::models::{ConfigTypeData, RunConfiguration};

    fn manager_with_bundle() -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        let a = RunConfiguration {
            name: String::from("A"),
            ..RunConfiguration::default()
        };
        let a_id = a.id;
        let mut bundle = RunConfiguration {
            name: String::from("Bundle"),
            ..RunConfiguration::default()
        };
        bundle.type_data = ConfigTypeData::Compound {
            members: vec![a_id],
            workspace: None,
        };
        app.configurations = vec![a, bundle];
        app
    }

    #[test]
    fn request_opens_modal_and_does_not_delete() {
        let mut app = manager_with_bundle();
        let _ = app.handle_request_delete_configuration(Some(0));
        // 아직 삭제되지 않고 모달만 열린다.
        assert_eq!(app.configurations.len(), 2);
        let modal = app.confirm_delete_modal.as_ref().expect("modal open");
        assert_eq!(modal.name, "A");
        // A는 Bundle이 참조 중 → 경고 목록에 표시.
        assert_eq!(modal.referencing_compounds, vec![String::from("Bundle")]);
    }

    #[test]
    fn confirm_deletes_and_cancel_keeps() {
        let mut app = manager_with_bundle();
        let _ = app.handle_request_delete_configuration(Some(0));
        let _ = app.handle_confirm_delete_configuration();
        assert_eq!(app.configurations.len(), 1);
        assert!(app.confirm_delete_modal.is_none());
        // Compound 멤버 참조도 기존 삭제 경로가 정리한다.
        assert_eq!(
            app.configurations[0].type_data.compound_members(),
            Some(&[][..])
        );

        // cancel 경로: 남은 Bundle에 대해 열었다 취소 — 삭제되지 않는다.
        let _ = app.handle_request_delete_configuration(Some(0));
        let _ = app.handle_cancel_delete_configuration();
        assert_eq!(app.configurations.len(), 1);
        assert!(app.confirm_delete_modal.is_none());
    }

    #[test]
    fn request_without_target_is_noop() {
        let (mut app, _task) = RunConfigManager::new();
        app.selected_config_index = None;
        let _ = app.handle_request_delete_configuration(None);
        assert!(app.confirm_delete_modal.is_none());
    }
}
