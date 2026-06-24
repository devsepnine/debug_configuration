use crate::widgets::pane_grid;
use std::collections::HashMap;
use uuid::Uuid;

/// 레이아웃 트리의 고유 ID
pub type LayoutId = u64;

/// 레이아웃 트리 - `pane_grid`의 내부 구조를 미러링
#[derive(Clone, Debug)]
pub enum LayoutTree {
    /// 단일 Pane (leaf 노드)
    Leaf { id: LayoutId, pane: Pane },
    /// 분할된 영역 (내부 노드)
    Split {
        axis: pane_grid::Axis,
        ratio: f32,
        first: Box<LayoutTree>,  // Left 또는 Top
        second: Box<LayoutTree>, // Right 또는 Bottom
    },
}

impl LayoutTree {
    /// 새로운 Leaf 노드 생성
    pub fn new_leaf(id: LayoutId, pane: Pane) -> Self {
        LayoutTree::Leaf { id, pane }
    }

    /// 다음 사용 가능한 ID 찾기
    pub fn next_id(&self) -> LayoutId {
        self.max_id() + 1
    }

    /// 트리에서 최대 ID 찾기
    fn max_id(&self) -> LayoutId {
        match self {
            LayoutTree::Leaf { id, .. } => *id,
            LayoutTree::Split { first, second, .. } => first.max_id().max(second.max_id()),
        }
    }

    /// ID로 Pane 찾기 (가변 참조)
    pub fn get_pane_mut(&mut self, target_id: LayoutId) -> Option<&mut Pane> {
        match self {
            LayoutTree::Leaf { id, pane } if *id == target_id => Some(pane),
            LayoutTree::Leaf { .. } => None,
            LayoutTree::Split { first, second, .. } => {
                if let Some(p) = first.get_pane_mut(target_id) {
                    Some(p)
                } else {
                    second.get_pane_mut(target_id)
                }
            }
        }
    }

    /// 모든 leaf의 (id, pane) 수집
    pub fn collect_leaves(&self) -> Vec<(LayoutId, &Pane)> {
        match self {
            LayoutTree::Leaf { id, pane } => vec![(*id, pane)],
            LayoutTree::Split { first, second, .. } => {
                let mut leaves = first.collect_leaves();
                leaves.extend(second.collect_leaves());
                leaves
            }
        }
    }

    /// 새 패인을 추가할 때 분할할 leaf와 분할 축을 선택한다.
    ///
    /// 가장 큰 패인을 골라 더 긴 변 방향으로 쪼개 균형 잡힌 격자를 유지한다
    /// (넓으면 좌우=Vertical, 높으면 상하=Horizontal). `aspect`는 표시 영역의
    /// 가로/세로 비율로, 넓은 창은 열을, 높은 창은 행을 더 만들게 한다.
    pub fn best_split_target(&self, aspect: f32) -> Option<(LayoutId, pane_grid::Axis)> {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };

        let mut leaves = Vec::new();
        self.collect_leaf_sizes(1.0, 1.0, &mut leaves);

        leaves
            .into_iter()
            .map(|(id, width_frac, height_frac)| {
                let width = width_frac * aspect;
                let height = height_frac;
                let axis = if width >= height {
                    pane_grid::Axis::Vertical // 좌우 분할 (폭을 나눔)
                } else {
                    pane_grid::Axis::Horizontal // 상하 분할 (높이를 나눔)
                };
                (id, axis, width * height)
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, axis, _)| (id, axis))
    }

    /// 각 leaf의 상대 (가로, 세로) 비율을 트리에서 누적 계산.
    fn collect_leaf_sizes(
        &self,
        width_frac: f32,
        height_frac: f32,
        out: &mut Vec<(LayoutId, f32, f32)>,
    ) {
        match self {
            LayoutTree::Leaf { id, .. } => out.push((*id, width_frac, height_frac)),
            LayoutTree::Split {
                axis,
                ratio,
                first,
                second,
            } => match *axis {
                pane_grid::Axis::Vertical => {
                    first.collect_leaf_sizes(width_frac * *ratio, height_frac, out);
                    second.collect_leaf_sizes(width_frac * (1.0 - *ratio), height_frac, out);
                }
                pane_grid::Axis::Horizontal => {
                    first.collect_leaf_sizes(width_frac, height_frac * *ratio, out);
                    second.collect_leaf_sizes(width_frac, height_frac * (1.0 - *ratio), out);
                }
            },
        }
    }

    /// 특정 ID의 leaf를 분할
    pub fn split_leaf(
        &mut self,
        target_id: LayoutId,
        axis: pane_grid::Axis,
        new_pane: Pane,
        new_first: bool, // true면 새 pane이 first(Left/Top), false면 second(Right/Bottom)
    ) -> Option<LayoutId> {
        let new_id = self.next_id();
        self.split_leaf_internal(target_id, axis, new_pane, new_first, new_id)
    }

    fn split_leaf_internal(
        &mut self,
        target_id: LayoutId,
        axis: pane_grid::Axis,
        new_pane: Pane,
        new_first: bool,
        new_id: LayoutId,
    ) -> Option<LayoutId> {
        match self {
            LayoutTree::Leaf { id, pane } if *id == target_id => {
                let old_leaf = LayoutTree::Leaf {
                    id: *id,
                    pane: pane.clone(),
                };
                let new_leaf = LayoutTree::Leaf {
                    id: new_id,
                    pane: new_pane,
                };

                let (first, second) = if new_first {
                    (new_leaf, old_leaf)
                } else {
                    (old_leaf, new_leaf)
                };

                *self = LayoutTree::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                Some(new_id)
            }
            LayoutTree::Leaf { .. } => None,
            LayoutTree::Split { first, second, .. } => first
                .split_leaf_internal(target_id, axis, new_pane.clone(), new_first, new_id)
                .or_else(|| {
                    second.split_leaf_internal(target_id, axis, new_pane, new_first, new_id)
                }),
        }
    }

    /// 빈 leaf 제거 (세션이 없는 pane)
    pub fn remove_empty_leaves(&mut self) -> bool {
        match self {
            LayoutTree::Leaf { pane, .. } => pane.is_empty(),
            LayoutTree::Split { first, second, .. } => {
                let first_empty = first.remove_empty_leaves();
                let second_empty = second.remove_empty_leaves();

                if first_empty && !second_empty {
                    *self = *second.clone();
                    false
                } else if !first_empty && second_empty {
                    *self = *first.clone();
                    false
                } else {
                    first_empty && second_empty
                }
            }
        }
    }

    /// `pane_grid::Configuration`으로 변환
    pub fn to_configuration(&self) -> pane_grid::Configuration<Pane> {
        match self {
            LayoutTree::Leaf { pane, .. } => pane_grid::Configuration::Pane(pane.clone()),
            LayoutTree::Split {
                axis,
                ratio,
                first,
                second,
            } => pane_grid::Configuration::Split {
                axis: *axis,
                ratio: *ratio,
                a: Box::new(first.to_configuration()),
                b: Box::new(second.to_configuration()),
            },
        }
    }

    /// `pane_grid::State` 생성 후 ID 매핑 반환
    pub fn create_pane_grid(&self) -> (pane_grid::State<Pane>, Vec<(LayoutId, pane_grid::Pane)>) {
        let config = self.to_configuration();
        let state = pane_grid::State::with_configuration(config);

        // pane_grid의 pane들과 LayoutTree의 leaf들을 순서대로 매핑
        let leaves: Vec<LayoutId> = self.collect_leaf_ids();
        let pane_ids: Vec<pane_grid::Pane> = state.panes.keys().copied().collect();

        // leaves와 pane_ids의 순서가 일치한다고 가정
        // (Configuration 생성 순서와 State 내부 순서가 일치)
        let mapping: Vec<(LayoutId, pane_grid::Pane)> = leaves.into_iter().zip(pane_ids).collect();

        (state, mapping)
    }

    /// `pane_grid::State`에서 현재 레이아웃 트리 재구성
    pub fn from_pane_grid(
        state: &pane_grid::State<Pane>,
    ) -> (Self, Vec<(LayoutId, pane_grid::Pane)>) {
        let mut next_id = 0;
        let mut mapping = Vec::new();
        let tree = Self::from_node(state.layout(), state, &mut next_id, &mut mapping);

        (tree, mapping)
    }

    fn from_node(
        node: &pane_grid::Node,
        state: &pane_grid::State<Pane>,
        next_id: &mut LayoutId,
        mapping: &mut Vec<(LayoutId, pane_grid::Pane)>,
    ) -> Self {
        match node {
            pane_grid::Node::Pane(pane_id) => {
                let id = *next_id;
                *next_id = next_id.saturating_add(1);
                mapping.push((id, *pane_id));

                Self::Leaf {
                    id,
                    pane: state.get(*pane_id).cloned().unwrap_or_else(Pane::empty),
                }
            }
            pane_grid::Node::Split {
                axis, ratio, a, b, ..
            } => Self::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(Self::from_node(a, state, next_id, mapping)),
                second: Box::new(Self::from_node(b, state, next_id, mapping)),
            },
        }
    }

    /// leaf ID들만 수집 (순서 보장)
    fn collect_leaf_ids(&self) -> Vec<LayoutId> {
        match self {
            LayoutTree::Leaf { id, .. } => vec![*id],
            LayoutTree::Split { first, second, .. } => {
                let mut ids = first.collect_leaf_ids();
                ids.extend(second.collect_leaf_ids());
                ids
            }
        }
    }
}

/// Pane - 단일 세션을 표시하는 단위 (Termius 스타일)
#[derive(Clone, Debug)]
pub struct Pane {
    /// 이 Pane에 연결된 세션 ID (None이면 빈 Pane)
    pub session_id: Option<Uuid>,
}

impl Pane {
    /// 빈 Pane 생성
    pub fn empty() -> Self {
        Self { session_id: None }
    }

    /// 세션을 가진 Pane 생성
    pub fn with_session(session_id: Uuid) -> Self {
        Self {
            session_id: Some(session_id),
        }
    }

    /// Pane이 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.session_id.is_none()
    }

    /// 세션 설정
    pub fn set_session(&mut self, session_id: Uuid) {
        self.session_id = Some(session_id);
    }

    /// 세션 제거
    pub fn clear(&mut self) {
        self.session_id = None;
    }
}

/// 워크스페이스 탭 - 독립적인 `pane_grid` 레이아웃을 가진 탭
#[derive(Debug)]
pub struct WorkspaceTab {
    /// 탭 이름 (표시용)
    pub name: String,
    /// 이 탭의 Pane 레이아웃
    pub pane_layout: pane_grid::State<Pane>,
    /// 레이아웃 트리 (구조 보존용)
    pub layout_tree: LayoutTree,
    /// `LayoutId` <-> `pane_grid::Pane` 매핑
    pub id_mapping: HashMap<LayoutId, pane_grid::Pane>,
}

impl WorkspaceTab {
    /// 빈 워크스페이스 탭 생성
    pub fn empty(name: String) -> Self {
        let layout_tree = LayoutTree::new_leaf(0, Pane::empty());
        let (pane_layout, mapping) = layout_tree.create_pane_grid();

        Self {
            name,
            pane_layout,
            layout_tree,
            id_mapping: mapping.into_iter().collect(),
        }
    }

    /// 탭 내 세션 개수
    pub fn session_count(&self) -> usize {
        self.pane_layout
            .panes
            .values()
            .filter(|p| p.session_id.is_some())
            .count()
    }

    /// 현재 워크스페이스에 세션이 이미 열려 있는지 확인
    pub fn contains_session(&self, session_id: Uuid) -> bool {
        self.pane_layout
            .panes
            .values()
            .any(|pane| pane.session_id == Some(session_id))
    }

    /// 현재 워크스페이스에 세션 열기.
    ///
    /// 빈 패인이 있으면 거기에 채우고, 없으면 가장 큰 패인을 긴 변 방향으로 분할해
    /// 균형 잡힌 격자를 유지한다 (`aspect` = 표시 영역의 가로/세로 비율).
    pub fn open_session(&mut self, session_id: Uuid, aspect: f32) -> bool {
        if self.contains_session(session_id) {
            return false;
        }

        if let Some((layout_id, _)) = self
            .layout_tree
            .collect_leaves()
            .into_iter()
            .find(|(_, pane)| pane.is_empty())
        {
            if let Some(pane) = self.layout_tree.get_pane_mut(layout_id) {
                pane.set_session(session_id);
            }
        } else if let Some((layout_id, axis)) = self.layout_tree.best_split_target(aspect) {
            let _ =
                self.layout_tree
                    .split_leaf(layout_id, axis, Pane::with_session(session_id), false);
        }

        self.rebuild_from_layout_tree();
        true
    }

    /// 현재 워크스페이스에서 세션만 제거하고 실행 세션은 유지
    pub fn remove_session(&mut self, session_id: Uuid) -> bool {
        let mut removed = false;

        let layout_ids: Vec<LayoutId> = self
            .layout_tree
            .collect_leaves()
            .into_iter()
            .filter_map(|(layout_id, pane)| {
                (pane.session_id == Some(session_id)).then_some(layout_id)
            })
            .collect();

        for layout_id in layout_ids {
            if let Some(pane) = self.layout_tree.get_pane_mut(layout_id) {
                pane.clear();
                removed = true;
            }
        }

        if removed {
            self.layout_tree.remove_empty_leaves();
            self.rebuild_from_layout_tree();
        }

        removed
    }

    /// 워크스페이스 내용을 비우고 탭은 유지
    pub fn clear(&mut self) {
        *self = Self::empty(String::from("Workspace"));
    }

    fn rebuild_from_layout_tree(&mut self) {
        let (pane_layout, mapping) = self.layout_tree.create_pane_grid();
        self.pane_layout = pane_layout;
        self.id_mapping = mapping.into_iter().collect();
    }

    /// 현재 `pane_grid::State`를 기준으로 `layout_tree`를 동기화
    pub fn sync_layout_tree_from_pane_grid(&mut self) {
        let (layout_tree, mapping) = LayoutTree::from_pane_grid(&self.pane_layout);
        self.layout_tree = layout_tree;
        self.id_mapping = mapping.into_iter().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_areas(tree: &LayoutTree, width: f32, height: f32, out: &mut Vec<f32>) {
        match tree {
            LayoutTree::Leaf { .. } => out.push(width * height),
            LayoutTree::Split {
                axis,
                ratio,
                first,
                second,
            } => match *axis {
                pane_grid::Axis::Vertical => {
                    leaf_areas(first, width * *ratio, height, out);
                    leaf_areas(second, width * (1.0 - *ratio), height, out);
                }
                pane_grid::Axis::Horizontal => {
                    leaf_areas(first, width, height * *ratio, out);
                    leaf_areas(second, width, height * (1.0 - *ratio), out);
                }
            },
        }
    }

    #[test]
    fn best_split_target_splits_largest_along_longer_side() {
        // 단일 leaf: 넓은 화면이면 좌우(Vertical) 분할
        let mut tree = LayoutTree::new_leaf(0, Pane::empty());
        let (id, axis) = tree.best_split_target(1.6).unwrap();
        assert_eq!(id, 0);
        assert!(matches!(axis, pane_grid::Axis::Vertical));

        // 좌우로 한 번 나누면 각 칸이 세로로 길어져 상하(Horizontal) 분할을 선호
        tree.split_leaf(0, pane_grid::Axis::Vertical, Pane::empty(), false);
        let (_, axis) = tree.best_split_target(1.6).unwrap();
        assert!(matches!(axis, pane_grid::Axis::Horizontal));
    }

    #[test]
    fn open_session_keeps_layout_balanced() {
        let mut tab = WorkspaceTab::empty(String::from("W"));
        for _ in 0..4 {
            tab.open_session(Uuid::new_v4(), 1.6);
        }
        assert_eq!(tab.session_count(), 4);

        let mut areas = Vec::new();
        leaf_areas(&tab.layout_tree, 1.0, 1.0, &mut areas);
        assert_eq!(areas.len(), 4);

        // 균형 격자면 각 칸 ≈0.25, 기존 계단형이면 최대 0.5였다.
        let max_area = areas.iter().copied().fold(0.0_f32, f32::max);
        assert!(max_area <= 0.3, "layout not balanced: areas = {areas:?}");
    }

    #[test]
    fn open_session_fills_empty_leaf_first() {
        let mut tab = WorkspaceTab::empty(String::from("W"));
        let id = Uuid::new_v4();
        tab.open_session(id, 1.6);
        // 첫 세션은 분할 없이 빈 leaf를 채운다 (단일 패인).
        assert_eq!(tab.pane_layout.panes.len(), 1);
        assert!(tab.contains_session(id));
    }
}
