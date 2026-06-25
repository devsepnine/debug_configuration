//! 환경변수 편집 모달의 상태 타입과 업데이트 핸들러.
//!
//! `app.rs`(부모 모듈)의 `RunConfigManager`에 대한 `impl`을 이 자식 모듈에 둔다.
//! 자식 모듈은 부모의 비공개 필드(`env_modal`, `configurations`, `env_bulk_inputs` 등)에
//! 접근할 수 있으므로, 모달 관련 동작을 모델/뷰와 분리해 응집도를 높인다.

use super::RunConfigManager;
use crate::messages::Message;
use crate::views::{env_modal_key_id, env_modal_value_id};
use iced::Task;
use uuid::Uuid;

/// 환경변수 편집 모달의 staging 상태 (always-inline 패턴).
///
/// 모든 row가 항상 편집 가능. 모달 OK 시 entries를 `environment_variables`에 commit
/// (빈 키 row 제거 + HashMap 변환으로 자동 dedupe). Cancel 시 폐기.
pub struct EnvModalState {
    /// 어떤 구성의 모달인지
    pub config_id: Uuid,
    /// staging entries (입력 순서 유지). 빈 row는 view에서 자동 표시되며
    /// 사용자가 빈 row에 타이핑하기 시작하면 entries 끝에 push됨.
    pub entries: Vec<(String, String)>,
    /// Tab cycle 추적용 — 현재 focus 가진 cell. 마우스 클릭으로 외부 변경된 경우
    /// 다음 Tab까지는 stale일 수 있으나 cycle 자체는 항상 모달 내부에 머무름.
    pub focused_cell: ModalCell,
}

/// 모달 내부의 한 input cell 위치 (row index + key/value 구분).
#[derive(Debug, Clone, Copy)]
pub struct ModalCell {
    pub row: usize,
    pub kind: ModalCellKind,
}

#[derive(Debug, Clone, Copy)]
pub enum ModalCellKind {
    Key,
    Value,
}

impl ModalCell {
    /// row * 2 + (Key=0, Value=1). cycle 모듈로 연산용.
    pub fn linear_index(self) -> usize {
        self.row * 2
            + match self.kind {
                ModalCellKind::Key => 0,
                ModalCellKind::Value => 1,
            }
    }

    /// linear_index의 역변환.
    pub fn from_linear(linear: usize) -> Self {
        let row = linear / 2;
        let kind = if linear.is_multiple_of(2) {
            ModalCellKind::Key
        } else {
            ModalCellKind::Value
        };
        Self { row, kind }
    }
}

impl RunConfigManager {
    pub(super) fn handle_env_bulk_input_changed(&mut self, text: String) -> Task<Message> {
        let Some(config_id) = self.selected_config_id() else {
            return Task::none();
        };
        // 다른 편집 필드(command/arguments 등)와 동일하게 입력 즉시 모델에 반영한다.
        // raw text는 버퍼에 보존해 정규화(serialize)는 submit 시에만 수행 → 타이핑을 방해하지
        // 않으면서, Enter 없이 Run/Save해도 environment_variables가 항상 최신이도록 보장한다.
        if let Some(config) = self.configurations.iter_mut().find(|c| c.id == config_id) {
            config.environment_variables =
                crate::env_string::parse_env_string(&text).into_iter().collect();
        }
        self.env_bulk_inputs.insert(config_id, text);
        Task::none()
    }

    pub(super) fn handle_env_bulk_input_submitted(&mut self) -> Task<Message> {
        let Some(config_id) = self.selected_config_id() else {
            return Task::none();
        };
        // environment_variables는 입력 시점(changed)에 이미 반영됨. 여기서는 표시용 raw text를
        // 정렬된 형태로 normalize해 일관성만 유지한다.
        let normalized = self
            .configurations
            .iter()
            .find(|c| c.id == config_id)
            .map(|config| crate::env_string::serialize_env_map(&config.environment_variables));
        if let Some(normalized) = normalized {
            self.env_bulk_inputs.insert(config_id, normalized);
        }
        Task::none()
    }

    pub(super) fn handle_open_env_modal(&mut self) -> Task<Message> {
        let Some(index) = self.selected_config_index else {
            return Task::none();
        };
        let Some(config) = self.configurations.get(index) else {
            return Task::none();
        };
        let config_id = config.id;
        // environment_variables는 메인 input 입력 시점(`changed`)에 항상 최신화되므로,
        // 모달은 이를 직접 staging entries로 읽는다. 키 기준 정렬로 표시 일관성을 유지.
        let mut entries: Vec<(String, String)> = config
            .environment_variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        self.env_modal = Some(EnvModalState {
            config_id,
            entries,
            focused_cell: ModalCell {
                row: 0,
                kind: ModalCellKind::Key,
            },
        });
        // 모달 열림과 동시에 첫 row(idx=0)의 key input에 focus.
        // entries가 비었어도 placeholder row가 idx=0이라 동일하게 동작.
        iced::widget::operation::focus(env_modal_key_id(0))
    }

    pub(super) fn handle_confirm_env_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.env_modal.take() else {
            return Task::none();
        };
        let Some(config) = self
            .configurations
            .iter_mut()
            .find(|c| c.id == modal.config_id)
        else {
            return Task::none();
        };
        // 빈 키 row 제거 + HashMap 변환으로 중복 키 자동 dedupe (마지막 값 우선)
        config.environment_variables = modal
            .entries
            .into_iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, v)| (k.trim().to_string(), v))
            .collect();
        let normalized = crate::env_string::serialize_env_map(&config.environment_variables);
        self.env_bulk_inputs.insert(modal.config_id, normalized);
        Task::none()
    }

    pub(super) fn handle_cancel_env_modal(&mut self) -> Task<Message> {
        self.env_modal = None;
        Task::none()
    }

    pub(super) fn handle_env_modal_row_key_changed(
        &mut self,
        index: usize,
        key: String,
    ) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        Self::set_modal_row(&mut modal.entries, index, Some(key), None);
        Task::none()
    }

    pub(super) fn handle_env_modal_row_value_changed(
        &mut self,
        index: usize,
        value: String,
    ) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        Self::set_modal_row(&mut modal.entries, index, None, Some(value));
        Task::none()
    }

    pub(super) fn handle_env_modal_duplicate_entry(&mut self, index: usize) -> Task<Message> {
        if let Some(modal) = self.env_modal.as_mut()
            && let Some((key, value)) = modal.entries.get(index)
        {
            // 동일 (key, value)를 원본 바로 다음 위치에 insert.
            // 같은 키가 둘이 되어 자연스럽게 중복 경고가 떠 사용자에게 키 변경을 유도.
            let cloned = (key.clone(), value.clone());
            modal.entries.insert(index + 1, cloned);
        }
        Task::none()
    }

    pub(super) fn handle_env_modal_remove_entry(&mut self, index: usize) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        if index >= modal.entries.len() {
            return Task::none();
        }
        modal.entries.remove(index);
        // remove 후 focused_cell이 row 수보다 커지지 않도록 clamp + 명시적 focus 이동.
        // (위젯 id 재할당으로 stale focus가 다른 데이터에 머무는 것 방지)
        let last_row = modal.entries.len(); // placeholder row index
        if modal.focused_cell.row > last_row {
            modal.focused_cell.row = last_row;
        }
        modal.focused_cell.kind = ModalCellKind::Key;
        Self::focus_modal_cell_task(modal.focused_cell)
    }

    /// Tab/Shift+Tab 처리: focused_cell을 modal 내부에서만 cycle.
    /// `(entries.len() + 1) * 2` 개 cell을 모듈로 순회한다 (+1은 placeholder row).
    /// `total_cells`는 항상 ≥ 2 이므로 0-검사는 불필요.
    pub(super) fn handle_env_modal_focus_shift(&mut self, backward: bool) -> Task<Message> {
        let Some(modal) = self.env_modal.as_mut() else {
            return Task::none();
        };
        let total_cells = (modal.entries.len() + 1) * 2;
        let current = modal.focused_cell.linear_index().min(total_cells - 1);
        let next = if backward {
            (current + total_cells - 1) % total_cells
        } else {
            (current + 1) % total_cells
        };
        modal.focused_cell = ModalCell::from_linear(next);
        Self::focus_modal_cell_task(modal.focused_cell)
    }

    fn focus_modal_cell_task(cell: ModalCell) -> Task<Message> {
        let id = match cell.kind {
            ModalCellKind::Key => env_modal_key_id(cell.row),
            ModalCellKind::Value => env_modal_value_id(cell.row),
        };
        iced::widget::operation::focus(id)
    }

    /// row index 위치에 key/value를 부분 업데이트. index가 entries 범위 밖이면
    /// 빈 entry를 push하여 auto-grow.
    fn set_modal_row(
        entries: &mut Vec<(String, String)>,
        index: usize,
        key: Option<String>,
        value: Option<String>,
    ) {
        while entries.len() <= index {
            entries.push((String::new(), String::new()));
        }
        if let Some(k) = key {
            entries[index].0 = k;
        }
        if let Some(v) = value {
            entries[index].1 = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModalCell, ModalCellKind};
    use crate::app::RunConfigManager;
    use crate::models::RunConfiguration;

    /// 회귀: 메인 env 텍스트 입력은 Enter(submit) 없이도 즉시 `environment_variables`에
    /// 반영되어야 한다. 입력 직후 Run/Save 시 환경변수가 누락되던 버그를 방지한다.
    #[test]
    fn bulk_input_applied_immediately_without_submit() {
        let (mut app, _task) = RunConfigManager::new();
        assert!(
            app.configurations.is_empty(),
            "RunConfigManager::new() must start with no configurations"
        );
        app.configurations.push(RunConfiguration::default());
        app.selected_config_index = Some(0);

        let _ = app.handle_env_bulk_input_changed("FOO=bar;BAZ=qux".to_string());

        let env = &app.configurations[0].environment_variables;
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(env.get("BAZ").map(String::as_str), Some("qux"));
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn modal_cell_linear_index_round_trips() {
        for linear in 0..12 {
            assert_eq!(ModalCell::from_linear(linear).linear_index(), linear);
        }
        assert_eq!(
            ModalCell {
                row: 0,
                kind: ModalCellKind::Key
            }
            .linear_index(),
            0
        );
        assert_eq!(
            ModalCell {
                row: 0,
                kind: ModalCellKind::Value
            }
            .linear_index(),
            1
        );
        assert_eq!(
            ModalCell {
                row: 3,
                kind: ModalCellKind::Value
            }
            .linear_index(),
            7
        );
    }
}
