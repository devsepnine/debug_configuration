//! 커스텀 타이틀바 드래그 영역 위젯.
//!
//! `mouse_area`는 마우스 좌클릭과 터치(FingerPressed)를 같은 `on_press`로 묶고 좌표를
//! 버리기 때문에, 터치 드래그를 마우스와 다르게 처리할 수 없다. 그래서 직접 만든다.
//!
//! 배경: Windows 터치 환경에서 OS 창 드래그(`window::drag` → winit `drag_window` →
//! `WM_NCLBUTTONDOWN` 모달 이동 루프)는 종료 신호로 **마우스 뗌**을 기다리는데, 터치는
//! 그 신호를 `WM_POINTER*`로만 보내므로 루프가 끝나지 않아 메시지 펌프가 멈춘다(앱 프리징).
//! 그래서 마우스는 기존 OS 드래그를 그대로 쓰고, 터치는 손가락 좌표를 호출부에 넘겨
//! 호출부가 `window::move_to`로 직접 창을 옮긴다(OS 모달 루프를 아예 타지 않음).
//!
//! - 마우스 좌클릭 press → `on_mouse_press` (호출부가 OS 드래그 시작)
//! - 더블 클릭 / 더블 탭 → `on_double`
//! - 터치 FingerPressed/Moved/Lifted → `on_touch_start/move/end` (좌표는 창 기준 logical)
//!
//! 자식(창 컨트롤 버튼·탭)이 이벤트를 캡처하면 `mouse_area`와 동일하게 조기 반환한다.
//! 터치 드래그가 시작되면 이후 FingerMoved/Lifted는 이 위젯이 선점(grab)해, 손가락이
//! 자식 위젯 위로 지나가도 드래그가 끊기지 않는다.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::Operation;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::touch;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

/// 타이틀바 드래그 영역. 하나의 콘텐츠를 감싸고 마우스/터치 이벤트를 각각 다른
/// 메시지로 발행한다.
pub struct TitleBarDragArea<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_mouse_press: Option<Message>,
    on_double: Option<Message>,
    on_touch_start: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    on_touch_move: Option<Box<dyn Fn(Point) -> Message + 'a>>,
    on_touch_end: Option<Message>,
    interaction: mouse::Interaction,
}

/// 위젯의 지속 상태. 더블클릭 판정과 터치 드래그 grab 여부를 tree에 보관한다.
#[derive(Default)]
struct State {
    previous_click: Option<mouse::Click>,
    is_touch_dragging: bool,
}

impl<'a, Message, Theme, Renderer> TitleBarDragArea<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_mouse_press: None,
            on_double: None,
            on_touch_start: None,
            on_touch_move: None,
            on_touch_end: None,
            interaction: mouse::Interaction::None,
        }
    }

    /// 마우스 좌클릭 press 시 발행 (호출부가 OS 창 드래그를 시작).
    #[must_use]
    pub fn on_mouse_press(mut self, message: Message) -> Self {
        self.on_mouse_press = Some(message);
        self
    }

    /// 더블 클릭 / 더블 탭 시 발행 (최대화 토글 등).
    #[must_use]
    pub fn on_double(mut self, message: Message) -> Self {
        self.on_double = Some(message);
        self
    }

    /// 터치 시작 시 발행. 인자는 창 기준 logical 좌표의 손가락 위치.
    #[must_use]
    pub fn on_touch_start(mut self, f: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_touch_start = Some(Box::new(f));
        self
    }

    /// 터치 이동 시 발행 (드래그 중일 때만). 인자는 창 기준 logical 좌표.
    #[must_use]
    pub fn on_touch_move(mut self, f: impl Fn(Point) -> Message + 'a) -> Self {
        self.on_touch_move = Some(Box::new(f));
        self
    }

    /// 터치 뗌/유실 시 발행 (드래그 종료).
    #[must_use]
    pub fn on_touch_end(mut self, message: Message) -> Self {
        self.on_touch_end = Some(message);
        self
    }

    /// 영역 위에서 표시할 마우스 커서 모양.
    #[must_use]
    pub fn interaction(mut self, interaction: mouse::Interaction) -> Self {
        self.interaction = interaction;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TitleBarDragArea<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
    Message: Clone,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // 드래그가 이미 진행 중이면 손가락 이동/뗌을 이 위젯이 선점한다(grab). 자식에
        // 먼저 넘기지 않으므로, 손가락이 스크롤 영역 등 자식 위로 지나가도 드래그가
        // 그쪽으로 새지 않는다.
        if tree.state.downcast_ref::<State>().is_touch_dragging {
            match event {
                Event::Touch(touch::Event::FingerMoved { position, .. }) => {
                    if let Some(f) = self.on_touch_move.as_ref() {
                        shell.publish(f(*position));
                    }
                    shell.capture_event();
                    return;
                }
                Event::Touch(
                    touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. },
                ) => {
                    tree.state.downcast_mut::<State>().is_touch_dragging = false;
                    if let Some(message) = self.on_touch_end.clone() {
                        shell.publish(message);
                    }
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }

        // 자식(버튼·탭)이 먼저 이벤트를 처리·캡처하도록 위임 (mouse_area와 동일).
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() {
            return;
        }

        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if !cursor.is_over(bounds) {
                    return;
                }
                if let Some(message) = self.on_mouse_press.clone() {
                    shell.publish(message);
                    shell.capture_event();
                }
                if self.on_double.is_some()
                    && let Some(position) = cursor.position()
                {
                    publish_double_click(&self.on_double, state, position, shell);
                    shell.capture_event();
                }
            }
            // 터치 좌표(창 기준 logical)로 직접 히트 테스트한다. 터치 환경에서 마우스
            // 커서 위치는 없거나 부정확할 수 있어 `cursor.is_over`에 의존하지 않는다.
            Event::Touch(touch::Event::FingerPressed { position, .. }) => {
                if !bounds.contains(*position) {
                    return;
                }
                state.is_touch_dragging = true;
                if let Some(f) = self.on_touch_start.as_ref() {
                    shell.publish(f(*position));
                }
                if self.on_double.is_some() {
                    publish_double_click(&self.on_double, state, *position, shell);
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let content_interaction = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );

        match content_interaction {
            mouse::Interaction::None if cursor.is_over(layout.bounds()) => self.interaction,
            _ => content_interaction,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// 연속 press 간격으로 더블클릭을 판정해, 더블이면 `on_double`을 발행한다
/// (mouse_area와 동일한 `mouse::Click` 누적 방식).
fn publish_double_click<Message: Clone>(
    on_double: &Option<Message>,
    state: &mut State,
    position: Point,
    shell: &mut Shell<'_, Message>,
) {
    let new_click = mouse::Click::new(position, mouse::Button::Left, state.previous_click);
    if new_click.kind() == mouse::click::Kind::Double
        && let Some(message) = on_double.clone()
    {
        shell.publish(message);
    }
    state.previous_click = Some(new_click);
}

impl<'a, Message, Theme, Renderer> From<TitleBarDragArea<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a + Clone,
    Theme: 'a,
    Renderer: 'a + renderer::Renderer,
{
    fn from(area: TitleBarDragArea<'a, Message, Theme, Renderer>) -> Self {
        Element::new(area)
    }
}
