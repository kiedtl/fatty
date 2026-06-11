// TODO: remove container attributes (eg max_height) and put in sensible defaults for simplification.
use crate::ControlState;

use iced::advanced::widget::operation::Operation;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, layout, renderer, widget};
use iced::advanced::{Layout, Shell, Widget};
use iced::alignment::{self, Alignment};
use iced::event::Event;
use iced::{mouse, overlay, keyboard::{self, Key, Modifiers, key::Named}};
use iced::{
    Background, Border, Color, Element, Length, Padding, Pixels, Point, Rectangle, Shadow, Size,
    Vector,
};

pub fn controller<'a, Message, Renderer: advanced::Renderer>(
    state: ControlState,
    on_control_state_change: impl Fn(ControlState) -> Message + 'static,
    content: impl Into<Element<'a, Message, crate::styles::Theme, Renderer>>,
) -> Controller<'a, Message, Renderer> {
    Controller::new(state, content, Box::new(on_control_state_change))
}

#[allow(missing_debug_implementations)]
pub struct Controller<'a, Message, Renderer = iced::Renderer>
where
    Renderer: advanced::Renderer,
{
    state: ControlState,
    id: Option<Id>,
    width: Length,
    height: Length,
    max_width: f32,
    max_height: f32,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
    clip: bool,
    content: Element<'a, Message, crate::styles::Theme, Renderer>,
    on_control_state_change: Box<dyn Fn(ControlState) -> Message>,
}

impl<'a, Message, Renderer: advanced::Renderer> Controller<'a, Message, Renderer> {
    pub fn new<T>(
        state: ControlState,
        content: T,
        on_control_state_change: Box<dyn Fn(ControlState) -> Message>,
    ) -> Self
    where
        T: Into<Element<'a, Message, crate::styles::Theme, Renderer>>,
    {
        let content = content.into();
        let size = content.as_widget().size_hint();

        Controller {
            state,
            id: None,
            width: size.width.fluid(),
            height: size.height.fluid(),
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
            align_x: alignment::Horizontal::Left,
            align_y: alignment::Vertical::Top,
            clip: false,
            content,
            on_control_state_change,
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn max_width(mut self, max_width: impl Into<Pixels>) -> Self {
        self.max_width = max_width.into().0;
        self
    }

    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = max_height.into().0;
        self
    }

    pub fn align_x(mut self, alignment: alignment::Horizontal) -> Self {
        self.align_x = alignment;
        self
    }

    pub fn align_y(mut self, alignment: alignment::Vertical) -> Self {
        self.align_y = alignment;
        self
    }

    pub fn center_x(mut self) -> Self {
        self.align_x = alignment::Horizontal::Center;
        self
    }

    pub fn center_y(mut self) -> Self {
        self.align_y = alignment::Vertical::Center;
        self
    }

    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }
}

impl<'a, Message, Renderer> Widget<Message, crate::styles::Theme, Renderer>
    for Controller<'a, Message, Renderer>
where
    Renderer: advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::positioned(
            &limits.max_width(self.max_width).max_height(self.max_height),
            self.width,
            self.height,
            Padding::ZERO,
            |limits| {
                self.content.as_widget_mut().layout(tree, renderer, &limits.loose())
            },
            |content, size| content.align(Alignment::from(self.align_x), Alignment::from(self.align_y), size),
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(self.id.as_ref().map(|id| &id.0), layout.bounds());
        operation.traverse(
            &mut |operation| {
                self.content.as_widget_mut().operate(
                    tree,
                    layout.children().next().unwrap(),
                    renderer,
                    operation,
                );
            },
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        match &event {
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(key), modifiers, .. }) => {
                match (self.state, *modifiers, key) {
                    (_, Modifiers::NONE, Named::Escape) => {
                        shell.publish((self.on_control_state_change)(ControlState::Normal));
                        return;
                    },
                    _ => (),
                }
            },
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Character(key), modifiers, physical_key, .. }) => {
                let lkey = Key::Character(key.clone()).to_latin(*physical_key);
                match (self.state, *modifiers, lkey) {
                    (ControlState::Normal, Modifiers::NONE, Some('i')) => {
                        shell.publish((self.on_control_state_change)(ControlState::Insert));
                        return;
                    },
                    _ => (),
                }
            }
            _ => (),
        }

        self.content.as_widget_mut().update(
            tree,
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            tree,
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &crate::styles::Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        if let Some(clipped_viewport) = bounds.intersection(viewport) {
            let viewport = if self.clip { &clipped_viewport } else { viewport };
            let child = layout.children().next().unwrap();
            self.content.as_widget().draw(tree, renderer, theme, &renderer_style, child, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, crate::styles::Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            tree,
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Renderer> From<Controller<'a, Message, Renderer>>
    for Element<'a, Message, crate::styles::Theme, Renderer>
where
    Message: 'a,
    Renderer: 'a + advanced::Renderer,
{
    fn from(column: Controller<'a, Message, Renderer>) -> Element<'a, Message, crate::styles::Theme, Renderer> {
        Element::new(column)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Id(widget::Id);

impl Id {
    pub fn new(id: impl Into<String>) -> Self {
        Self(widget::Id::from(id.into()))
    }

    pub fn unique() -> Self {
        Self(widget::Id::unique())
    }
}

impl From<Id> for widget::Id {
    fn from(id: Id) -> Self {
        id.0
    }
}

fn _quad(x: f32, y: f32, w: f32, h: f32) -> renderer::Quad {
    renderer::Quad {
        bounds: Rectangle::new(Point::new(x, y), Size::new(w, h)),
        border: Default::default(),
        shadow: Default::default(),
        snap: true,
    }
}
