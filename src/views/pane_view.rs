use crate::messages::Message;
use crate::models::{Pane, RunSession, SessionStatusKind};
use crate::utils::{
    ICON_ARROW_DOWN_FILL, ICON_ARROW_DOWN_LINE, ICON_CLOSE, ICON_ERASER, ICON_MORE,
    ICON_PANE_MAXIMIZE, ICON_PANE_RESTORE, ICON_REFRESH, ICON_SAVE, ICON_SEARCH, ICON_STOP,
    ICON_TERMINAL_INPUT,
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

/// pane 타이틀바 포커스 강조 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneFocus {
    /// 강조 없음 — pane이 하나뿐이거나 포커스가 설정되지 않음.
    Inactive,
    /// 현재 포커스된 pane (accent 틴트 + 밝은 타이틀).
    Focused,
    /// 다른 pane이 포커스됨 — 타이틀을 살짝 흐리게.
    Unfocused,
}

/// pane 개수·세션·포커스 대상으로 타이틀바 강조 상태를 정한다.
/// pane이 2개 이상이고 포커스가 설정됐을 때만 Focused/Unfocused로 갈린다(단일 pane은 강조 불필요).
fn pane_focus(pane_count: usize, session_id: Option<Uuid>, focused: Option<Uuid>) -> PaneFocus {
    match focused {
        Some(focused) if pane_count > 1 && session_id.is_some() => {
            if session_id == Some(focused) {
                PaneFocus::Focused
            } else {
                PaneFocus::Unfocused
            }
        }
        _ => PaneFocus::Inactive,
    }
}

/// `pane_grid`를 사용한 Pane 레이아웃 렌더링
///
/// # Arguments
/// * `state` - `pane_grid::State<Pane>`
/// * `sessions` - 모든 세션 데이터
/// * `focused_session_id` - 활성 탭에서 포커스된 세션 (타이틀바 강조 대상)
/// * `is_dragging_pane` - Pane 드래그 중 여부 (드롭 존 표시를 위해 content 투명화)
/// * `dragging_pane_id` - 드래그 중인 Pane ID (`pane_grid` 네이티브 드래그)
pub fn view_pane_layout<'a>(
    state: &'a pane_grid::State<Pane>,
    sessions: &'a [RunSession],
    maximized_pane: Option<pane_grid::Pane>,
    focused_session_id: Option<Uuid>,
    is_dragging_pane: bool,
    dragging_pane_id: Option<pane_grid::Pane>,
) -> Element<'a, Message> {
    let pane_count = state.panes.len();
    pane_grid::PaneGrid::new(state, move |pane_id, pane, _is_focused| {
        view_pane_content(
            pane_id,
            pane,
            sessions,
            pane_count,
            pane_focus(pane_count, pane.session_id, focused_session_id),
            maximized_pane == Some(pane_id),
            is_dragging_pane,
            dragging_pane_id,
        )
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .on_click(Message::PaneClicked)
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
/// * `focus` - 타이틀바 포커스 강조 상태
/// * `is_maximized` - 현재 Pane 최대화 여부
/// * `_is_dragging_pane` - Pane 드래그 중 여부 (현재 미사용)
/// * `dragging_pane_id` - 드래그 중인 Pane ID (`pane_grid` 네이티브 드래그)
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn view_pane_content<'a>(
    pane_id: pane_grid::Pane,
    pane: &'a Pane,
    sessions: &'a [RunSession],
    pane_count: usize,
    focus: PaneFocus,
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
    // 상태 배지: 실행 중엔 라이브 경과시간("Running 12.3s" — 1초 tick으로 갱신),
    // 종료 후엔 결과와 소요 시간을 표시한다.
    let session_badge = current_session.map(RunSession::status_badge_label);

    let mut title_controls: Option<(Element<'a, Message>, Element<'a, Message>)> = None;
    let content: Element<'a, Message> = if let Some(session_id) = pane.session_id {
        // 세션이 있는 경우
        if let Some(session) = sessions.iter().find(|s| s.id == session_id) {
            // Pane 드래그 중 여부 (반투명 효과 적용용)
            let is_dragging = dragging_pane_id == Some(pane_id);

            let terminal_output = view_terminal_for_session(session, is_dragging);
            // 넓으면 전체 버튼(full), 좁아 안 들어가면 ⋯+닫기(compact).
            // title_bar가 너비를 보고 자동 전환한다(Controls::dynamic).
            title_controls = Some((
                view_session_controls(session, pane_id, pane_count, is_maximized, is_dragging),
                view_session_controls_compact(session, pane_id, is_dragging),
            ));

            // 터미널 위에 오버플로 메뉴(열려 있으면) → 검색바(열려 있으면) 순으로 쌓는다.
            let mut stacked = column![].spacing(0);
            if session.controls_menu_open {
                stacked = stacked.push(view_session_controls_menu(
                    session,
                    pane_id,
                    pane_count,
                    is_maximized,
                ));
            }
            if session.search.is_some() {
                stacked = stacked.push(view_session_search_bar(session));
            }
            // stdin 입력바는 터미널 **아래** — 프롬프트가 나타나는 위치(맨 아래) 옆에서
            // 바로 응답하도록 (터미널이 Fill이라 바는 pane 바닥에 붙는다).
            let mut stacked = stacked.push(terminal_output);
            if session.stdin_input.is_some() {
                stacked = stacked.push(view_session_stdin_bar(session));
            }
            let body: Element<'a, Message> = stacked.into();

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

    // TitleBar 복구 - session_name 표시. 포커스된 pane은 타이틀바를 accent로 틴트한다.
    let mut title_bar = pane_grid::TitleBar::new(session_title(
        session_name,
        session_status,
        session_badge,
        focus,
    ))
    .padding([6, 9])
    .style(move |theme: &Theme| {
        let palette = theme.extended_palette();
        let background = if focus == PaneFocus::Focused {
            Color {
                a: 0.42,
                ..palette.primary.base.color
            }
        } else {
            Color {
                a: 0.58,
                ..palette.background.strong.color
            }
        };
        pane_container_style_with_radius(
            with_drag_opacity(background, dragging_pane_id == Some(pane_id)),
            Color {
                a: 0.0,
                ..palette.background.base.text
            },
            0.0,
            border::Radius::default().top(4.0),
        )
    });
    if let Some((full, compact)) = title_controls {
        title_bar = title_bar
            .controls(pane_grid::Controls::dynamic(full, compact))
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

    // stdin 입력바 버튼 — 열려 있으면 active 강조 + 토글로 닫기. 실행 여부와 무관하게
    // 항상 누를 수 있다(비활성 상태는 바 자체가 표시 — 드래프트 확인/작성 가능).
    let stdin_open = session.stdin_input.is_some();
    let stdin_state = if stdin_open {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let stdin_message = if stdin_open {
        Message::CloseSessionStdin(session_id)
    } else {
        Message::OpenSessionStdin(session_id)
    };
    let stdin_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_TERMINAL_INPUT), stdin_state),
        Some(stdin_message),
        stdin_state,
        is_dragging,
    );

    // export(파일 저장)·clear(로그 지우기) 공용 상태 — 출력이 있을 때만 활성화
    // (export는 빈 0바이트 파일 방지, clear는 빈 버퍼 no-op 방지)
    let has_output = !session.output_lines.is_empty();
    let output_action_state = if has_output {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let export_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_SAVE), output_action_state),
        has_output.then_some(Message::ExportSessionOutput(session_id)),
        output_action_state,
        is_dragging,
    );

    // 로그 지우기 (실행 중인 프로세스는 유지, 화면 버퍼만 클리어)
    let clear_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_ERASER), output_action_state),
        has_output.then_some(Message::ClearSessionOutput(session_id)),
        output_action_state,
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
        Space::new().width(4),
        stdin_button,
        Space::new().width(4),
        export_button,
        Space::new().width(4),
        clear_button,
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

/// 좁은 pane용 축약 컨트롤: `⋯`(오버플로 메뉴 토글) + 닫기.
///
/// 닫기는 가장 자주 쓰므로 좁아도 항상 노출하고, 나머지 액션은 `⋯` 메뉴로 모은다.
/// `title_bar`가 full이 안 들어간다고 판단하면 이 compact으로 자동 전환한다.
fn view_session_controls_compact(
    session: &RunSession,
    pane_id: pane_grid::Pane,
    is_dragging: bool,
) -> Element<'_, Message> {
    let session_id = session.id;

    let menu_state = if session.controls_menu_open {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let menu_button = control_button(
        control_icon(svg::Handle::from_memory(ICON_MORE), menu_state),
        Some(Message::ToggleSessionControlsMenu(session_id)),
        menu_state,
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

    let controls_row = row![menu_button, Space::new().width(6), close_button]
        .align_y(Alignment::Center)
        .spacing(1.0);

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

/// 오버플로 메뉴: `⋯`로 펼치는 세션 액션 아이콘 행 (터미널 위에 표시).
///
/// full 컨트롤과 동일한 아이콘 버튼들을 라벨 없이 가로로 나열한다. body 전체 너비를
/// 쓰므로 세션명이 자리를 차지하는 title_bar보다 여유가 있어 아이콘이 모두 들어간다.
/// 닫기는 compact에 항상 있으므로 제외하고, 항목 선택 시 각 액션 핸들러가 메뉴를 닫는다.
fn view_session_controls_menu(
    session: &RunSession,
    pane_id: pane_grid::Pane,
    pane_count: usize,
    is_maximized: bool,
) -> Element<'_, Message> {
    let session_id = session.id;

    let rerun = control_button(
        control_icon(
            svg::Handle::from_memory(ICON_REFRESH),
            IconButtonState::Active,
        ),
        Some(Message::RerunSession(session_id)),
        IconButtonState::Active,
        false,
    );

    let stop_state = if session.is_running {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let stop = control_button(
        control_icon(svg::Handle::from_memory(ICON_STOP), stop_state),
        session
            .is_running
            .then_some(Message::StopSession(session_id)),
        stop_state,
        false,
    );

    let auto_scroll_svg = if session.auto_scroll {
        ICON_ARROW_DOWN_FILL
    } else {
        ICON_ARROW_DOWN_LINE
    };
    let auto_scroll = control_button(
        control_icon(
            svg::Handle::from_memory(auto_scroll_svg),
            IconButtonState::Active,
        ),
        Some(Message::ToggleAutoScroll(session_id)),
        IconButtonState::Active,
        false,
    );

    // search/stdin은 full 컨트롤과 동일한 상태 토글 — 메뉴가 지속 표면이 되면서
    // "열기 전용" 항목은 두 번째 클릭이 무동작으로 보이는 함정이 된다.
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
    let search = control_button(
        control_icon(svg::Handle::from_memory(ICON_SEARCH), search_state),
        Some(search_message),
        search_state,
        false,
    );

    let stdin_open = session.stdin_input.is_some();
    let stdin_state = if stdin_open {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let stdin_message = if stdin_open {
        Message::CloseSessionStdin(session_id)
    } else {
        Message::OpenSessionStdin(session_id)
    };
    let stdin = control_button(
        control_icon(svg::Handle::from_memory(ICON_TERMINAL_INPUT), stdin_state),
        Some(stdin_message),
        stdin_state,
        false,
    );

    let has_output = !session.output_lines.is_empty();
    let output_action_state = if has_output {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    let export = control_button(
        control_icon(svg::Handle::from_memory(ICON_SAVE), output_action_state),
        has_output.then_some(Message::ExportSessionOutput(session_id)),
        output_action_state,
        false,
    );

    let clear = control_button(
        control_icon(svg::Handle::from_memory(ICON_ERASER), output_action_state),
        has_output.then_some(Message::ClearSessionOutput(session_id)),
        output_action_state,
        false,
    );

    let mut controls_row = row![
        rerun,
        Space::new().width(4),
        stop,
        Space::new().width(4),
        auto_scroll,
        Space::new().width(4),
        search,
        Space::new().width(4),
        stdin,
        Space::new().width(4),
        export,
        Space::new().width(4),
        clear,
    ]
    .align_y(Alignment::Center)
    .spacing(1.0);

    if pane_count > 1 {
        let maximize = control_button(
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
            false,
        );
        controls_row = controls_row.push(Space::new().width(4)).push(maximize);
    }

    container(controls_row)
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

/// stdin 입력바 (터미널 아래에 표시). Enter/⏎ 제출, x/Esc 닫기.
/// 프로세스가 실행 중이 아니면 입력·제출이 비활성화되고 placeholder로 상태를 알린다
/// (드래프트는 유지 — 종료/rerun에도 타이핑 내용을 잃지 않는다).
fn view_session_stdin_bar(session: &RunSession) -> Element<'_, Message> {
    let draft = session
        .stdin_input
        .as_deref()
        .expect("view_session_stdin_bar requires stdin state");
    let session_id = session.id;
    let running = session.is_running;

    let placeholder = if running {
        "Send input to process (Enter)"
    } else {
        "Process not running"
    };
    let mut input = text_input(placeholder, draft)
        .id(crate::views::shared::session_stdin_input_id(session_id))
        .size(12)
        .padding([2, 6])
        .width(Length::Fill);
    if running {
        input = input
            .on_input(move |value| Message::SessionStdinChanged(session_id, value))
            // on_submit이 있어 Enter를 capture한다 — 검색 nav(Enter, Ignored 게이트)와
            // 이중 발화하지 않는 근거. 빈 제출 허용(bare \n — "press enter" 프롬프트).
            .on_submit(Message::SessionStdinSubmitted(session_id));
    }

    let send_state = if running {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };
    // ^C/^D를 x(닫기)와 비인접하게 앞쪽에 둔다 — 파괴적 액션/닫기 오클릭이 최악의 쌍.
    let controls = row![
        glyph_button(
            "^C",
            "Interrupt (Ctrl+C)",
            Message::SessionInterruptRequested(session_id),
            send_state
        ),
        glyph_button(
            "^D",
            "Send EOF (Ctrl+D)",
            Message::SessionEofRequested(session_id),
            send_state
        ),
        glyph_button(
            "⏎",
            "Send (Enter)",
            Message::SessionStdinSubmitted(session_id),
            send_state
        ),
        glyph_button(
            "x",
            "Close input (Esc)",
            Message::CloseSessionStdin(session_id),
            IconButtonState::Active
        ),
    ]
    .spacing(2)
    .align_y(Alignment::Center);

    container(row![input, controls].spacing(8).align_y(Alignment::Center))
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

/// 출력 검색바 (터미널 위에 표시). 매치 개수, 이전/다음, 필터 토글, 닫기.
fn view_session_search_bar(session: &RunSession) -> Element<'_, Message> {
    let search = session
        .search
        .as_ref()
        .expect("view_session_search_bar requires search state");
    let session_id = session.id;

    // 캐시된 매치 수 사용 (매 프레임 재스캔 방지; app의 update에서 갱신됨).
    let total = search.matches.len();
    // 정규식 모드에서 패턴이 잘못됐는지 가벼운 유효성 검사(표시용; 컴파일 1회).
    // 매칭 경로와 동일한 빌더(case_insensitive)를 써 유효성 판정을 일치시킨다.
    let invalid_regex = search.regex
        && !search.query.is_empty()
        && regex::RegexBuilder::new(&search.query)
            .case_insensitive(true)
            .build()
            .is_err();
    let count_text = if search.query.is_empty() {
        String::new()
    } else if invalid_regex {
        String::from("bad regex")
    } else if total == 0 {
        String::from("0/0")
    } else {
        format!("{}/{}", (search.current % total) + 1, total)
    };

    let input = text_input("Find in output...", &search.query)
        .id(session_search_input_id(session_id))
        .on_input(move |value| Message::SessionSearchChanged(session_id, value))
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
    let regex_state = if search.regex {
        IconButtonState::Active
    } else {
        IconButtonState::Inactive
    };

    let controls = row![
        glyph_button(
            "<",
            "Previous match (Shift+Enter)",
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
            ".*",
            "Regex (case-insensitive)",
            Message::ToggleSessionSearchRegex(session_id),
            regex_state
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
    focus: PaneFocus,
) -> Element<'static, Message> {
    // 포커스된 pane은 타이틀을 또렷하게, 비포커스는 살짝 흐리게. 단일 pane(Inactive)은
    // 색 override 없이 기본 텍스트색을 유지한다.
    let name_alpha = match focus {
        PaneFocus::Focused => Some(0.98),
        PaneFocus::Unfocused => Some(0.72),
        PaneFocus::Inactive => None,
    };
    let mut name = text(session_name).size(12).wrapping(text::Wrapping::None);
    if let Some(alpha) = name_alpha {
        name = name.style(move |theme: &Theme| text::Style {
            color: Some(Color {
                a: alpha,
                ..theme.extended_palette().background.base.text
            }),
        });
    }

    let mut title = row![
        container(Space::new())
            .width(7)
            .height(7)
            .style(move |theme: &Theme| session_status_dot_style(theme, status)),
        name,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_focus_inactive_for_single_pane() {
        // pane이 하나뿐이면 포커스 대상이어도 강조하지 않는다.
        let id = Uuid::new_v4();
        assert_eq!(pane_focus(1, Some(id), Some(id)), PaneFocus::Inactive);
    }

    #[test]
    fn pane_focus_splits_focused_and_unfocused_with_multiple_panes() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_eq!(pane_focus(2, Some(a), Some(a)), PaneFocus::Focused);
        assert_eq!(pane_focus(2, Some(b), Some(a)), PaneFocus::Unfocused);
    }

    #[test]
    fn pane_focus_inactive_without_focus_or_for_empty_pane() {
        let a = Uuid::new_v4();
        // 포커스가 설정되지 않음
        assert_eq!(pane_focus(2, Some(a), None), PaneFocus::Inactive);
        // 빈 pane(세션 없음)은 강조 대상이 아니다
        assert_eq!(pane_focus(2, None, Some(a)), PaneFocus::Inactive);
    }
}
