use crate::messages::Message;
use crate::models::{DropZone, Pane, RunSession};
use crate::utils::{
    ICON_ARROW_DOWN_FILL, ICON_ARROW_DOWN_LINE, ICON_CLOSE, ICON_PANE_MAXIMIZE, ICON_PANE_RESTORE,
    ICON_REFRESH, ICON_STOP,
};
use crate::views::shared::{IconButtonState, icon_button_foreground, icon_button_style};
use crate::views::terminal::view_terminal_for_session;
use crate::widgets::pane_grid;
use iced::widget::{Space, stack};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Theme, border,
    widget::{button, column, container, mouse_area, row, svg, text},
};
use uuid::Uuid;

const CONTROL_ICON_SIZE: f32 = 14.0;
const CONTROL_BUTTON_SIZE: f32 = 24.0;
const DRAG_OPACITY: f32 = 0.5;

fn with_drag_opacity(color: Color, is_dragging: bool) -> Color {
    if is_dragging {
        Color {
            a: DRAG_OPACITY,
            ..color
        }
    } else {
        color
    }
}

fn pane_container_style_with_radius(
    background: Color,
    border_color: Color,
    border_width: f32,
    radius: border::Radius,
) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(background)),
        border: Border {
            width: border_width,
            color: border_color,
            radius,
        },
        ..Default::default()
    }
}

fn control_icon<'a>(handle: svg::Handle, state: IconButtonState) -> Element<'a, Message> {
    let icon = svg(handle)
        .width(CONTROL_ICON_SIZE)
        .height(CONTROL_ICON_SIZE)
        .style(move |theme: &Theme, _status| svg::Style {
            color: Some(icon_button_foreground(theme, state)),
        });

    container(icon)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn control_button_style(
    theme: &Theme,
    status: button::Status,
    state: IconButtonState,
    is_dragging: bool,
) -> button::Style {
    let mut style = icon_button_style(theme, status, state, 6.0);

    if let Some(Background::Color(background)) = style.background {
        style.background = Some(Background::Color(with_drag_opacity(
            background,
            is_dragging,
        )));
    }

    style
}

fn control_button(
    icon: Element<'_, Message>,
    message: Option<Message>,
    state: IconButtonState,
    is_dragging: bool,
) -> iced::widget::Button<'_, Message> {
    let button = button(icon)
        .padding(0)
        .width(CONTROL_BUTTON_SIZE)
        .height(CONTROL_BUTTON_SIZE)
        .style(move |theme: &Theme, status| {
            control_button_style(theme, status, state, is_dragging)
        });

    if let Some(message) = message {
        button.on_press(message)
    } else {
        button
    }
}

/// `pane_grid`를 사용한 Pane 레이아웃 렌더링
///
/// # Arguments
/// * `state` - `pane_grid::State<Pane>`
/// * `sessions` - 모든 세션 데이터
/// * `is_dragging_pane` - Pane 드래그 중 여부 (드롭 존 표시를 위해 content 투명화)
/// * `dragging_pane_id` - 드래그 중인 Pane ID (`pane_grid` 네이티브 드래그)
/// * `tab_dragging` - 탭 드래그 상태 (`session_id`, `from_pane_id`) - 드래그 앤 드롭
/// * `hovered_pane` - 현재 마우스가 hover 중인 Pane (드롭 타겟)
/// * `hovered_drop_zone` - 현재 마우스가 hover 중인 드롭 존 (`pane_id`, zone)
/// * `hovered_outer_zone` - 현재 마우스가 hover 중인 외부 드롭 존
/// * `total_tabs` - 전체 워크스페이스 탭 개수 (드래그 핸들 표시용)
#[allow(clippy::too_many_arguments)]
pub fn view_pane_layout<'a>(
    state: &'a pane_grid::State<Pane>,
    sessions: &'a [RunSession],
    maximized_pane: Option<pane_grid::Pane>,
    is_dragging_pane: bool,
    dragging_pane_id: Option<pane_grid::Pane>,
    tab_dragging: Option<(Uuid, pane_grid::Pane)>,
    hovered_pane: Option<pane_grid::Pane>,
    hovered_drop_zone: Option<(pane_grid::Pane, DropZone)>,
    hovered_outer_zone: Option<DropZone>,
    total_tabs: usize,
) -> Element<'a, Message> {
    // 전체 Pane 개수 계산
    let pane_count = state.panes.len();
    let pane_grid_widget = pane_grid::PaneGrid::new(state, |pane_id, pane, _is_focused| {
        view_pane_content(
            pane_id,
            pane,
            sessions,
            pane_count,
            maximized_pane == Some(pane_id),
            is_dragging_pane,
            dragging_pane_id,
            tab_dragging,
            hovered_pane,
            hovered_drop_zone,
            hovered_outer_zone,
            total_tabs,
        )
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .on_drag(Message::PaneGridDragged)
    .on_resize(10, Message::PaneGridResized)
    .spacing(4);

    // 탭 드래그 중이면 외부 드롭 존 표시
    if tab_dragging.is_some() {
        view_with_outer_drop_zones(pane_grid_widget.into(), hovered_outer_zone)
    } else {
        // container 없이 직접 반환 (이벤트 전달 보장)
        pane_grid_widget.into()
    }
}

/// 외부 드롭 존을 포함한 레이아웃 렌더링 (오버레이 방식)
fn view_with_outer_drop_zones(
    content: Element<'_, Message>,
    hovered_outer_zone: Option<DropZone>,
) -> Element<'_, Message> {
    let edge_size: f32 = 16.0;

    // 상단 드롭 존 (오버레이)
    let top_zone = mouse_area(
        container(Space::new())
            .style(move |theme: &Theme| {
                let bg = if hovered_outer_zone == Some(DropZone::Top) {
                    outer_drop_zone_color(theme)
                } else {
                    Color::TRANSPARENT
                };
                container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                }
            })
            .width(Length::Fill)
            .height(Length::Fixed(edge_size)),
    )
    .on_enter(Message::OuterDropZoneHovered(DropZone::Top))
    .on_exit(Message::OuterDropZoneUnhovered);

    // 하단 드롭 존 (오버레이)
    let bottom_zone = mouse_area(
        container(Space::new())
            .style(move |theme: &Theme| {
                let bg = if hovered_outer_zone == Some(DropZone::Bottom) {
                    outer_drop_zone_color(theme)
                } else {
                    Color::TRANSPARENT
                };
                container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                }
            })
            .width(Length::Fill)
            .height(Length::Fixed(edge_size)),
    )
    .on_enter(Message::OuterDropZoneHovered(DropZone::Bottom))
    .on_exit(Message::OuterDropZoneUnhovered);

    // 좌측 드롭 존 (오버레이)
    let left_zone = mouse_area(
        container(Space::new())
            .style(move |theme: &Theme| {
                let bg = if hovered_outer_zone == Some(DropZone::Left) {
                    outer_drop_zone_color(theme)
                } else {
                    Color::TRANSPARENT
                };
                container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                }
            })
            .width(Length::Fixed(edge_size))
            .height(Length::Fill),
    )
    .on_enter(Message::OuterDropZoneHovered(DropZone::Left))
    .on_exit(Message::OuterDropZoneUnhovered);

    // 우측 드롭 존 (오버레이)
    let right_zone = mouse_area(
        container(Space::new())
            .style(move |theme: &Theme| {
                let bg = if hovered_outer_zone == Some(DropZone::Right) {
                    outer_drop_zone_color(theme)
                } else {
                    Color::TRANSPARENT
                };
                container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                }
            })
            .width(Length::Fixed(edge_size))
            .height(Length::Fill),
    )
    .on_enter(Message::OuterDropZoneHovered(DropZone::Right))
    .on_exit(Message::OuterDropZoneUnhovered);

    // 오버레이 레이어: 상단, 하단은 column으로, 좌우는 row로
    let overlay = column![
        top_zone,
        row![
            left_zone,
            Space::new().width(Length::Fill).height(Length::Fill),
            right_zone,
        ]
        .height(Length::Fill),
        bottom_zone,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // stack으로 콘텐츠 위에 오버레이
    stack![content, overlay]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn outer_drop_zone_color(theme: &Theme) -> Color {
    Color {
        a: 0.42,
        ..theme.extended_palette().success.base.color
    }
}

/// 개별 Pane 내용 렌더링 (단일 세션 모델)
///
/// # Arguments
/// * `pane_id` - `pane_grid::Pane` ID
/// * `pane` - 렌더링할 Pane 데이터 (단일 세션)
/// * `sessions` - 모든 세션 데이터
/// * `pane_count` - 현재 워크스페이스의 Pane 개수
/// * `is_maximized` - 현재 Pane 최대화 여부
/// * `_is_dragging_pane` - Pane 드래그 중 여부 (현재 미사용)
/// * `dragging_pane_id` - 드래그 중인 Pane ID (`pane_grid` 네이티브 드래그)
/// * `tab_dragging` - 탭 드래그 상태 (`session_id`, `from_pane_id`) - 드래그 앤 드롭
/// * `hovered_pane` - 현재 마우스가 hover 중인 Pane (드롭 타겟)
/// * `hovered_drop_zone` - 현재 마우스가 hover 중인 드롭 존 (`pane_id`, zone)
/// * `hovered_outer_zone` - 현재 마우스가 hover 중인 외부 드롭 존
/// * `total_tabs` - 전체 워크스페이스 탭 개수 (드래그 핸들 표시용)
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn view_pane_content<'a>(
    pane_id: pane_grid::Pane,
    pane: &'a Pane,
    sessions: &'a [RunSession],
    pane_count: usize,
    is_maximized: bool,
    _is_dragging_pane: bool,
    dragging_pane_id: Option<pane_grid::Pane>,
    tab_dragging: Option<(Uuid, pane_grid::Pane)>,
    hovered_pane: Option<pane_grid::Pane>,
    hovered_drop_zone: Option<(pane_grid::Pane, DropZone)>,
    hovered_outer_zone: Option<DropZone>,
    total_tabs: usize,
) -> pane_grid::Content<'a, Message> {
    // 세션 이름 가져오기
    let current_session = pane
        .session_id
        .and_then(|id| sessions.iter().find(|s| s.id == id));
    let session_name =
        current_session.map_or_else(|| String::from("Empty"), |s| s.config_name.clone());
    let is_session_running = current_session.is_some_and(|session| session.is_running);

    let _ = (tab_dragging, total_tabs); // 미사용 경고 방지

    // 드래그 중이고 이 Pane이 hover된 경우 하이라이트
    let is_drop_target = if let Some((_, from_pane)) = tab_dragging {
        hovered_pane == Some(pane_id) && from_pane != pane_id
    } else {
        false
    };

    let mut title_controls: Option<Element<'a, Message>> = None;
    let content: Element<'a, Message> = if let Some(session_id) = pane.session_id {
        // 세션이 있는 경우
        if let Some(session) = sessions.iter().find(|s| s.id == session_id) {
            // 세션 인덱스 찾기
            let session_index = sessions
                .iter()
                .position(|s| s.id == session_id)
                .unwrap_or(0);

            // Pane 드래그 중 여부 (반투명 효과 적용용)
            let is_dragging = dragging_pane_id == Some(pane_id);

            let terminal_output = view_terminal_for_session(session, is_dragging);
            title_controls = Some(view_session_controls(
                session,
                session_index,
                pane_id,
                pane_count,
                is_maximized,
                is_dragging,
            ));

            container(terminal_output)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            container(text("Session not found").size(14))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        }
    } else {
        container(
            text("Empty pane")
                .size(14)
                .align_x(iced::alignment::Horizontal::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    };

    // 탭 드래그 중일 때 mouse_area로 감싸서 hover 추적 및 split drop zone 표시
    let content_with_hover: Element<'a, Message> = if let Some((_, from_pane)) = tab_dragging {
        let is_source_pane = from_pane == pane_id;
        let is_hovered = hovered_pane == Some(pane_id);

        let content_with_mouse_area: Element<'a, Message> = mouse_area(content)
            .on_enter(Message::PaneHovered(pane_id))
            .on_exit(Message::PaneUnhovered(pane_id))
            .into();

        // Split drop zone 표시 조건:
        // 1. 외부 드롭 존이 활성화되어 있으면 내부 드롭 존 표시 안함
        // 2. 소스 Pane이 아니고 hover 중일 때 표시
        let show_split_zones = if hovered_outer_zone.is_some() {
            // 외부 드롭 존 활성화 시 내부 드롭 존 비활성화
            false
        } else if is_source_pane {
            // 소스 Pane: 단일 세션이므로 분리 불가
            false
        } else {
            // 다른 Pane에서는 hover 중일 때 표시
            is_hovered
        };

        if show_split_zones {
            // 현재 이 Pane에서 hover 중인 드롭 존 확인
            let current_hovered_zone = hovered_drop_zone
                .filter(|(p, _)| *p == pane_id)
                .map(|(_, z)| z);

            // Split drop zone 오버레이 추가
            let split_overlay = view_split_drop_zone(pane_id, is_source_pane, current_hovered_zone);
            stack![content_with_mouse_area, split_overlay].into()
        } else {
            content_with_mouse_area
        }
    } else {
        content
    };

    // TitleBar 복구 - session_name 표시
    let mut title_bar = pane_grid::TitleBar::new(session_title(session_name, is_session_running))
        .padding([6, 9])
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            pane_container_style_with_radius(
                with_drag_opacity(
                    Color {
                        a: 0.58,
                        ..palette.background.strong.color
                    },
                    dragging_pane_id == Some(pane_id),
                ),
                Color {
                    a: 0.0,
                    ..palette.background.base.text
                },
                0.0,
                border::Radius::default().top(4.0),
            )
        });
    if let Some(controls) = title_controls {
        title_bar = title_bar
            .controls(pane_grid::Controls::new(controls))
            .always_show_controls();
    }

    pane_grid::Content::new(content_with_hover)
        .title_bar(title_bar)
        .style(move |theme: &Theme| {
            if is_drop_target {
                return container::Style {
                    background: None,
                    border: Border {
                        width: 3.0,
                        color: theme.extended_palette().success.strong.color,
                        radius: 4.0.into(),
                    },
                    ..container::Style::default()
                };
            }

            container::Style {
                background: None,
                border: Border {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                    radius: 0.0.into(),
                },
                ..container::Style::default()
            }
        })
}

/// 세션 컨트롤 버튼 영역 렌더링 (터미널 위에 표시)
///
/// # Arguments
/// * `session` - 현재 선택된 세션
/// * `session_index` - 세션의 전역 인덱스
/// * `pane_id` - Pane ID (닫기 버튼용)
/// * `pane_count` - 현재 워크스페이스의 Pane 개수
/// * `is_maximized` - 현재 Pane 최대화 여부
/// * `is_dragging` - Pane 드래그 중 여부 (반투명 효과)
fn view_session_controls(
    session: &RunSession,
    session_index: usize,
    pane_id: pane_grid::Pane,
    pane_count: usize,
    is_maximized: bool,
    is_dragging: bool,
) -> Element<'_, Message> {
    let session_id = session.id;

    let rerun_button = control_button(
        control_icon(
            svg::Handle::from_memory(ICON_REFRESH),
            IconButtonState::Active,
        ),
        Some(Message::RerunSession(session_index)),
        IconButtonState::Active,
        is_dragging,
    );

    let stop_state = if session.is_running {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let stop_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_STOP), stop_state),
        session
            .is_running
            .then_some(Message::StopSession(session_index)),
        stop_state,
        is_dragging,
    );

    // 자동 스크롤 버튼
    let auto_scroll = session.auto_scroll;

    // SVG 아이콘 - 활성화 상태에 따라 다른 아이콘 사용
    let auto_scroll_svg = if auto_scroll {
        svg::Handle::from_memory(ICON_ARROW_DOWN_FILL)
    } else {
        svg::Handle::from_memory(ICON_ARROW_DOWN_LINE)
    };

    let auto_scroll_button = control_button(
        control_icon(auto_scroll_svg, IconButtonState::Active),
        Some(Message::ToggleAutoScroll(session_id)),
        IconButtonState::Active,
        is_dragging,
    );

    let maximize_button = control_button(
        control_icon(
            svg::Handle::from_memory(if is_maximized {
                ICON_PANE_RESTORE
            } else {
                ICON_PANE_MAXIMIZE
            }),
            IconButtonState::Active,
        ),
        Some(Message::TogglePaneMaximize(pane_id)),
        IconButtonState::Active,
        is_dragging,
    );

    let close_button = control_button(
        control_icon(
            svg::Handle::from_memory(ICON_CLOSE),
            IconButtonState::Active,
        ),
        Some(Message::ClosePane(pane_id)),
        IconButtonState::Active,
        is_dragging,
    );

    let mut controls_row = row![
        rerun_button,
        Space::new().width(4),
        stop_button,
        Space::new().width(4),
        auto_scroll_button,
    ]
    .align_y(Alignment::Center)
    .spacing(1.0);

    if pane_count > 1 {
        controls_row = controls_row.push(Space::new().width(4));
        controls_row = controls_row.push(maximize_button);
    }

    controls_row = controls_row.push(Space::new().width(6));
    controls_row = controls_row.push(close_button);

    container(controls_row)
        .padding(0)
        .style(move |_theme: &Theme| container::Style {
            background: None,
            border: Border {
                width: 0.0,
                color: Color::TRANSPARENT,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn session_title(session_name: String, is_running: bool) -> Element<'static, Message> {
    row![
        container(Space::new())
            .width(7)
            .height(7)
            .style(move |theme: &Theme| session_status_dot_style(theme, is_running)),
        text(session_name).size(12)
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn session_status_dot_style(theme: &Theme, is_running: bool) -> container::Style {
    let palette = theme.extended_palette();
    let color = if is_running {
        palette.success.base.color
    } else {
        palette.background.base.text
    };

    container::Style {
        background: Some(Background::Color(Color {
            a: if is_running { 0.92 } else { 0.32 },
            ..color
        })),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Split drop zone 오버레이 렌더링
/// 탭을 드래그할 때 마우스 위치에 따라 분할 방향을 자동 결정
///
/// # Arguments
/// * `pane_id` - 대상 Pane ID
/// * `is_source_pane` - 드래그가 시작된 소스 Pane인지 여부
/// * `current_hovered_zone` - 현재 hover 중인 드롭 존 (하이라이트 용)
fn view_split_drop_zone(
    pane_id: pane_grid::Pane,
    is_source_pane: bool,
    current_hovered_zone: Option<DropZone>,
) -> Element<'static, Message> {
    // 드롭 존 영역 생성 헬퍼
    let create_zone = |zone: DropZone| -> Element<'static, Message> {
        let is_hovered = current_hovered_zone == Some(zone);

        let zone_container = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(move |theme: &Theme| {
                split_drop_zone_style(theme, zone, is_hovered, is_source_pane)
            });

        // mouse_area로 감싸서 hover 이벤트 감지
        mouse_area(zone_container)
            .on_enter(Message::DropZoneHovered(pane_id, zone))
            .on_exit(Message::DropZoneUnhovered)
            .into()
    };

    // 상단 존
    let top_zone = container(create_zone(DropZone::Top))
        .width(Length::Fill)
        .height(Length::FillPortion(1));

    // 중앙 행: 좌측 | 중앙 | 우측
    let left_zone = container(create_zone(DropZone::Left))
        .width(Length::FillPortion(1))
        .height(Length::Fill);

    let center_zone = container(create_zone(DropZone::Center))
        .width(Length::FillPortion(2))
        .height(Length::Fill);

    let right_zone = container(create_zone(DropZone::Right))
        .width(Length::FillPortion(1))
        .height(Length::Fill);

    let middle_row = row![left_zone, center_zone, right_zone]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::FillPortion(2));

    // 하단 존
    let bottom_zone = container(create_zone(DropZone::Bottom))
        .width(Length::Fill)
        .height(Length::FillPortion(1));

    // 전체 레이아웃: 상단 / 중간(좌|중|우) / 하단
    let layout = column![top_zone, middle_row, bottom_zone]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill);

    // 반투명 배경 오버레이로 감싸기
    container(layout)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.28,
                ..theme.extended_palette().background.base.color
            })),
            ..Default::default()
        })
        .into()
}

fn split_drop_zone_style(
    theme: &Theme,
    zone: DropZone,
    is_hovered: bool,
    is_source_pane: bool,
) -> container::Style {
    let palette = theme.extended_palette();
    let base_color = match zone {
        DropZone::Center if is_source_pane => palette.danger.base.color,
        DropZone::Center => palette.primary.base.color,
        _ => palette.success.base.color,
    };
    let border_color = palette.background.base.text;

    container::Style {
        background: Some(iced::Background::Color(if is_hovered {
            Color {
                a: 0.42,
                ..base_color
            }
        } else {
            Color {
                a: 0.10,
                ..palette.background.base.text
            }
        })),
        border: Border {
            width: if is_hovered { 2.0 } else { 1.0 },
            color: Color {
                a: if is_hovered { 0.58 } else { 0.16 },
                ..border_color
            },
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}
