use crate::messages::Message;
use crate::models::{Pane, RunSession, SessionStatusKind};
use crate::utils::{
    ICON_ARROW_DOWN_FILL, ICON_ARROW_DOWN_LINE, ICON_CLOSE, ICON_PANE_MAXIMIZE, ICON_PANE_RESTORE,
    ICON_REFRESH, ICON_SEARCH, ICON_STOP,
};
use crate::views::shared::{
    IconButtonState, icon_button_foreground, icon_button_style, icon_tooltip,
    session_search_input_id,
};
use crate::views::terminal::view_terminal_for_session;
use crate::widgets::pane_grid;
use iced::widget::Space;
use iced::{
    Alignment, Background, Border, Color, Element, Length, Theme, border,
    widget::{button, column, container, row, svg, text, text_input, tooltip},
};

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
pub fn view_pane_layout<'a>(
    state: &'a pane_grid::State<Pane>,
    sessions: &'a [RunSession],
    maximized_pane: Option<pane_grid::Pane>,
    is_dragging_pane: bool,
    dragging_pane_id: Option<pane_grid::Pane>,
) -> Element<'a, Message> {
    let pane_count = state.panes.len();
    pane_grid::PaneGrid::new(state, |pane_id, pane, _is_focused| {
        view_pane_content(
            pane_id,
            pane,
            sessions,
            pane_count,
            maximized_pane == Some(pane_id),
            is_dragging_pane,
            dragging_pane_id,
        )
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .on_drag(Message::PaneGridDragged)
    .on_resize(10, Message::PaneGridResized)
    .spacing(4)
    .into()
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
#[allow(clippy::too_many_lines)]
fn view_pane_content<'a>(
    pane_id: pane_grid::Pane,
    pane: &'a Pane,
    sessions: &'a [RunSession],
    pane_count: usize,
    is_maximized: bool,
    _is_dragging_pane: bool,
    dragging_pane_id: Option<pane_grid::Pane>,
) -> pane_grid::Content<'a, Message> {
    // 세션 이름 가져오기
    let current_session = pane
        .session_id
        .and_then(|id| sessions.iter().find(|s| s.id == id));
    let session_name =
        current_session.map_or_else(|| String::from("Empty"), |s| s.config_name.clone());
    let session_status = current_session.map(RunSession::status_kind);
    // 상태 배지는 완료된 세션에만 표시 (실행 중은 점으로 충분).
    let session_badge = current_session
        .filter(|session| !session.is_running)
        .map(RunSession::status_badge_label);

    let mut title_controls: Option<Element<'a, Message>> = None;
    let content: Element<'a, Message> = if let Some(session_id) = pane.session_id {
        // 세션이 있는 경우
        if let Some(session) = sessions.iter().find(|s| s.id == session_id) {
            // Pane 드래그 중 여부 (반투명 효과 적용용)
            let is_dragging = dragging_pane_id == Some(pane_id);

            let terminal_output = view_terminal_for_session(session, is_dragging);
            title_controls = Some(view_session_controls(
                session,
                pane_id,
                pane_count,
                is_maximized,
                is_dragging,
            ));

            // 검색바가 열려 있으면 터미널 위에 표시.
            let body: Element<'a, Message> = if session.search.is_some() {
                column![view_session_search_bar(session), terminal_output].into()
            } else {
                terminal_output
            };

            container(body)
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

    // TitleBar 복구 - session_name 표시
    let mut title_bar =
        pane_grid::TitleBar::new(session_title(session_name, session_status, session_badge))
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

    pane_grid::Content::new(content)
        .title_bar(title_bar)
        .style(move |_theme: &Theme| container::Style {
            background: None,
            border: Border {
                width: 0.0,
                color: Color::TRANSPARENT,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
}

/// 세션 컨트롤 버튼 영역 렌더링 (터미널 위에 표시)
///
/// # Arguments
/// * `session` - 현재 선택된 세션
/// * `pane_id` - Pane ID (닫기 버튼용)
/// * `pane_count` - 현재 워크스페이스의 Pane 개수
/// * `is_maximized` - 현재 Pane 최대화 여부
/// * `is_dragging` - Pane 드래그 중 여부 (반투명 효과)
fn view_session_controls(
    session: &RunSession,
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
        Some(Message::RerunSession(session_id)),
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
            .then_some(Message::StopSession(session_id)),
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

    // 검색 버튼 — 검색바가 열려 있으면 active 강조 + 토글로 닫기.
    let search_open = session.search.is_some();
    let search_state = if search_open {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let search_message = if search_open {
        Message::CloseSessionSearch(session_id)
    } else {
        Message::OpenSessionSearch(session_id)
    };
    let search_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_SEARCH), search_state),
        Some(search_message),
        search_state,
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
        Space::new().width(4),
        search_button,
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

/// 출력 검색바 (터미널 위에 표시). 매치 개수, 이전/다음, 필터 토글, 닫기.
fn view_session_search_bar(session: &RunSession) -> Element<'_, Message> {
    let search = session
        .search
        .as_ref()
        .expect("view_session_search_bar requires search state");
    let session_id = session.id;

    // 캐시된 매치 수 사용 (매 프레임 재스캔 방지; app의 update에서 갱신됨).
    let total = search.matches.len();
    let count_text = if search.query.is_empty() {
        String::new()
    } else if total == 0 {
        String::from("0/0")
    } else {
        format!("{}/{}", (search.current % total) + 1, total)
    };

    let input = text_input("Find in output...", &search.query)
        .id(session_search_input_id(session_id))
        .on_input(move |value| Message::SessionSearchChanged(session_id, value))
        .on_submit(Message::SessionSearchNext(session_id))
        .size(12)
        .padding([2, 6])
        .width(Length::Fill);

    let count = text(count_text)
        .size(11)
        .style(|theme: &Theme| text::Style {
            color: Some(Color {
                a: 0.6,
                ..theme.extended_palette().background.base.text
            }),
        });

    let filter_state = if search.filter {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };

    let controls = row![
        glyph_button(
            "<",
            "Previous match",
            Message::SessionSearchPrev(session_id),
            IconButtonState::Active
        ),
        glyph_button(
            ">",
            "Next match (Enter)",
            Message::SessionSearchNext(session_id),
            IconButtonState::Active
        ),
        glyph_button(
            "filter",
            "Show only matching lines",
            Message::ToggleSessionSearchFilter(session_id),
            filter_state
        ),
        glyph_button(
            "x",
            "Close search (Esc)",
            Message::CloseSessionSearch(session_id),
            IconButtonState::Active
        ),
    ]
    .spacing(2)
    .align_y(Alignment::Center);

    container(
        row![input, count, controls]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .width(Length::Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weak.color,
        )),
        ..container::Style::default()
    })
    .into()
}

/// 검색바용 작은 텍스트/글리프 버튼 (컨트롤과 동일한 토글 스타일 + 툴팁).
fn glyph_button(
    label: &str,
    tip: &'static str,
    message: Message,
    state: IconButtonState,
) -> Element<'static, Message> {
    let content = container(text(label.to_string()).size(12))
        .center_x(Length::Shrink)
        .center_y(Length::Fill);
    let btn = button(content)
        .padding([2, 7])
        .height(CONTROL_BUTTON_SIZE)
        .style(move |theme: &Theme, status| icon_button_style(theme, status, state, 4.0))
        .on_press(message);
    tooltip(btn, icon_tooltip(tip), tooltip::Position::Top)
        .gap(4)
        .into()
}

fn session_title(
    session_name: String,
    status: Option<SessionStatusKind>,
    badge: Option<String>,
) -> Element<'static, Message> {
    let mut title = row![
        container(Space::new())
            .width(7)
            .height(7)
            .style(move |theme: &Theme| session_status_dot_style(theme, status)),
        text(session_name).size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if let Some(badge) = badge {
        let is_failed = matches!(status, Some(SessionStatusKind::Failed(_)));
        title = title
            .push(Space::new().width(8))
            .push(
                text(badge)
                    .size(11)
                    .style(move |theme: &Theme| text::Style {
                        color: Some(if is_failed {
                            theme.extended_palette().danger.base.color
                        } else {
                            Color {
                                a: 0.55,
                                ..theme.extended_palette().background.base.text
                            }
                        }),
                    }),
            );
    }

    title.into()
}

fn session_status_dot_style(theme: &Theme, status: Option<SessionStatusKind>) -> container::Style {
    let palette = theme.extended_palette();
    let (color, alpha) = match status {
        Some(SessionStatusKind::Running) => (palette.success.base.color, 0.92),
        Some(SessionStatusKind::Succeeded) => (palette.background.base.text, 0.42),
        Some(SessionStatusKind::Failed(_)) => (palette.danger.base.color, 0.88),
        Some(SessionStatusKind::Stopped) | None => (palette.background.base.text, 0.30),
    };

    container::Style {
        background: Some(Background::Color(Color { a: alpha, ..color })),
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}
