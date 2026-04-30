use super::{Draggable, TitleBar};
use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{self, Clipboard};
use iced::widget::container;
use iced::{Element, Event, Point, Rectangle, Size, Vector};

/// The content of a [`Pane`].
///
/// [`Pane`]: super::Pane
pub struct Content<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Theme: container::Catalog,
    Renderer: advanced::Renderer,
{
    title_bar: Option<TitleBar<'a, Message, Theme, Renderer>>,
    body: Element<'a, Message, Theme, Renderer>,
    class: Theme::Class<'a>,
}

impl<'a, Message, Theme, Renderer> Content<'a, Message, Theme, Renderer>
where
    Theme: container::Catalog,
    Renderer: advanced::Renderer,
{
    /// Creates a new [`Content`] with the provided body.
    pub fn new(body: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            title_bar: None,
            body: body.into(),
            class: Theme::default(),
        }
    }

    /// Sets the [`TitleBar`] of the [`Content`].
    pub fn title_bar(mut self, title_bar: TitleBar<'a, Message, Theme, Renderer>) -> Self {
        self.title_bar = Some(title_bar);
        self
    }

    /// Sets the style of the [`Content`].
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> container::Style + 'a) -> Self
    where
        Theme::Class<'a>: From<container::StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as container::StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the style class of the [`Content`].
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }
}

impl<Message, Theme, Renderer> Content<'_, Message, Theme, Renderer>
where
    Theme: container::Catalog,
    Renderer: advanced::Renderer,
{
    pub(super) fn state(&self) -> Tree {
        let children = if let Some(title_bar) = self.title_bar.as_ref() {
            vec![Tree::new(&self.body), title_bar.state()]
        } else {
            vec![Tree::new(&self.body), Tree::empty()]
        };

        Tree {
            children,
            ..Tree::empty()
        }
    }

    pub(super) fn diff(&self, tree: &mut Tree) {
        if tree.children.len() == 2 {
            if let Some(title_bar) = self.title_bar.as_ref() {
                title_bar.diff(&mut tree.children[1]);
            }

            tree.children[0].diff(&self.body);
        } else {
            *tree = self.state();
        }
    }

    /// Draws the [`Content`] with the provided [`Renderer`] and [`Layout`].
    ///
    /// [`Renderer`]: advanced::Renderer
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        {
            let style = theme.style(&self.class);

            container::draw_background(renderer, &style, bounds);
        }

        if let Some(title_bar) = &self.title_bar {
            let mut children = layout.children();
            let title_bar_layout = children.next().unwrap();
            let body_layout = children.next().unwrap();

            let show_controls = cursor.is_over(bounds);

            self.body.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                body_layout,
                cursor,
                viewport,
            );

            title_bar.draw(
                &tree.children[1],
                renderer,
                theme,
                style,
                title_bar_layout,
                cursor,
                viewport,
                show_controls,
            );
        } else {
            self.body.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }
    }

    pub(crate) fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if let Some(title_bar) = &mut self.title_bar {
            let max_size = limits.max();

            let title_bar_layout = title_bar.layout(
                &mut tree.children[1],
                renderer,
                &layout::Limits::new(Size::ZERO, max_size),
            );

            let title_bar_size = title_bar_layout.size();

            let body_layout = self.body.as_widget_mut().layout(
                &mut tree.children[0],
                renderer,
                &layout::Limits::new(
                    Size::ZERO,
                    Size::new(max_size.width, max_size.height - title_bar_size.height),
                ),
            );

            layout::Node::with_children(
                max_size,
                vec![
                    title_bar_layout,
                    body_layout.move_to(Point::new(0.0, title_bar_size.height)),
                ],
            )
        } else {
            self.body
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        }
    }

    pub(crate) fn operate(
        &mut self,
        tree: &mut Tree,
        layout: layout::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let body_layout = if let Some(title_bar) = &mut self.title_bar {
            let mut children = layout.children();

            title_bar.operate(
                &mut tree.children[1],
                children.next().unwrap(),
                renderer,
                operation,
            );

            children.next().unwrap()
        } else {
            layout
        };

        self.body
            .as_widget_mut()
            .operate(&mut tree.children[0], body_layout, renderer, operation);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut advanced::Shell<'_, Message>,
        viewport: &Rectangle,
        is_picked: bool,
    ) {
        let body_layout = if let Some(title_bar) = &mut self.title_bar {
            let mut children = layout.children();

            title_bar.update(
                &mut tree.children[1],
                event,
                children.next().unwrap(),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            children.next().unwrap()
        } else {
            layout
        };

        if !is_picked {
            self.body.as_widget_mut().update(
                &mut tree.children[0],
                event,
                body_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    pub(crate) fn grid_interaction(
        &self,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        drag_enabled: bool,
    ) -> Option<mouse::Interaction> {
        let title_bar = self.title_bar.as_ref()?;

        let mut children = layout.children();
        let title_bar_layout = children.next().unwrap();

        let is_over_pick_area = cursor.position().is_some_and(|cursor_position| {
            title_bar.is_over_pick_area(title_bar_layout, cursor_position)
        });

        if is_over_pick_area && drag_enabled {
            return Some(mouse::Interaction::Idle);
        }

        None
    }

    pub(crate) fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: layout::Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
        drag_enabled: bool,
    ) -> mouse::Interaction {
        let (body_layout, title_bar_interaction) = if let Some(title_bar) = &self.title_bar {
            let mut children = layout.children();
            let title_bar_layout = children.next().unwrap();

            let is_over_pick_area = cursor.position().is_some_and(|cursor_position| {
                title_bar.is_over_pick_area(title_bar_layout, cursor_position)
            });

            if is_over_pick_area && drag_enabled {
                return mouse::Interaction::Idle;
            }

            let mouse_interaction = title_bar.mouse_interaction(
                &tree.children[1],
                title_bar_layout,
                cursor,
                viewport,
                renderer,
            );

            (children.next().unwrap(), mouse_interaction)
        } else {
            (layout, mouse::Interaction::default())
        };

        self.body
            .as_widget()
            .mouse_interaction(&tree.children[0], body_layout, cursor, viewport, renderer)
            .max(title_bar_interaction)
    }

    pub(crate) fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: layout::Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if let Some(title_bar) = self.title_bar.as_mut() {
            let mut children = layout.children();
            let title_bar_layout = children.next()?;

            let mut states = tree.children.iter_mut();
            let body_state = states.next().unwrap();
            let title_bar_state = states.next().unwrap();

            match title_bar.overlay(
                title_bar_state,
                title_bar_layout,
                renderer,
                viewport,
                translation,
            ) {
                Some(overlay) => Some(overlay),
                None => self.body.as_widget_mut().overlay(
                    body_state,
                    children.next()?,
                    renderer,
                    viewport,
                    translation,
                ),
            }
        } else {
            self.body.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
    }
}

impl<Message, Theme, Renderer> Draggable for &Content<'_, Message, Theme, Renderer>
where
    Theme: container::Catalog,
    Renderer: advanced::Renderer,
{
    fn can_be_dragged_at(&self, layout: layout::Layout<'_>, cursor_position: Point) -> bool {
        if let Some(title_bar) = &self.title_bar {
            let mut children = layout.children();
            let title_bar_layout = children.next().unwrap();

            title_bar.is_over_pick_area(title_bar_layout, cursor_position)
        } else {
            false
        }
    }
}

impl<'a, T, Message, Theme, Renderer> From<T> for Content<'a, Message, Theme, Renderer>
where
    T: Into<Element<'a, Message, Theme, Renderer>>,
    Theme: container::Catalog + 'a,
    Renderer: advanced::Renderer,
{
    fn from(element: T) -> Self {
        Self::new(element)
    }
}
