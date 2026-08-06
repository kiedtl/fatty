// TODO: remove container attributes (eg max_height) and put in sensible defaults for simplification.
use crate::{ControlState, ControlMode, ControlMessage};

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
    state: &'a ControlState,
    control_message: impl Fn(ControlMessage) -> Message + 'static,
    content: impl Into<Element<'a, Message, crate::styles::Theme, Renderer>>,
) -> Controller<'a, Message, Renderer> {
    Controller::new(state, content, Box::new(control_message))
}

#[allow(missing_debug_implementations)]
pub struct Controller<'a, Message, Renderer = iced::Renderer>
where
    Renderer: advanced::Renderer,
{
    state: &'a ControlState,
    width: Option<Length>,
    height: Option<Length>,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
    clip: bool,
    content: Element<'a, Message, crate::styles::Theme, Renderer>,
    control_message: Box<dyn Fn(ControlMessage) -> Message>,
}

impl<'a, Message, Renderer: advanced::Renderer> Controller<'a, Message, Renderer> {
    pub fn new<T>(
        state: &'a ControlState,
        content: T,
        control_message: Box<dyn Fn(ControlMessage) -> Message>,
    ) -> Self
    where
        T: Into<Element<'a, Message, crate::styles::Theme, Renderer>>,
    {
        let content = content.into();

        Controller {
            state,
            width: None,
            height: None,
            align_x: alignment::Horizontal::Left,
            align_y: alignment::Vertical::Top,
            clip: false,
            content,
            control_message,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = Some(height.into());
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

    fn send(&self, shell: &mut Shell<'_, Message>, m: ControlMessage) {
        shell.publish((self.control_message)(m));
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

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width.unwrap_or(Length::Shrink),
            height: self.height.unwrap_or(Length::Shrink),
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = self.size();
        layout::positioned(
            limits,
            size.width,
            size.height,
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
        operation.container(None, layout.bounds());
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
        if self.state.mode != ControlMode::Term {
            match &event {
                Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(key), modifiers, .. }) => {
                    match (self.state, *modifiers, key) {
                        (_, Modifiers::NONE, Named::Escape) => {
                            self.send(shell, ControlMessage::ChangeMode(ControlMode::Normal));
                            return;
                        },
                        _ => (),
                    }
                },
                Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Character(key), modifiers, physical_key, .. }) => {
                    let lkey = Key::Character(key.clone()).to_latin(*physical_key);
                    match (self.state.mode, *modifiers, lkey) {
                        (ControlMode::Normal, Modifiers::NONE, Some('k')) => {
                            self.send(shell, ControlMessage::HistoryUp);
                            return;
                        },
                        (ControlMode::Normal, Modifiers::NONE, Some('j')) => {
                            self.send(shell, ControlMessage::HistoryDown);
                            return;
                        },
                        (ControlMode::Normal, Modifiers::NONE, Some('i')) => {
                            self.send(shell, ControlMessage::ChangeMode(ControlMode::Insert));
                            return;
                        },
                        _ => (),
                    }
                }
                _ => (),
            }
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

pub fn update(app: &mut crate::App, message: ControlMessage) -> iced::Task<crate::Message> {
    use ControlMessage as CM;

    match message {
        CM::ChangeMode(s) => app.control.mode = s,
        CM::HistoryUp => if app.execs.len() >= 1 {
            let hc = app.control.history_cursor.unwrap_or(app.execs.len());
            let hc = hc.saturating_sub(1);
            if Some(hc) != app.control.history_cursor {
                app.control.history_cursor = Some(hc);
                app.input = app.execs[hc].cmdline.clone();
            }
        },
        CM::HistoryDown => if app.execs.len() >= 1 {
            let hc = app.control.history_cursor.unwrap_or(app.execs.len() - 1);
            let hc = if hc + 1 == app.execs.len() { None } else { Some(hc + 1) };
            if hc != app.control.history_cursor {
                app.control.history_cursor = hc;
                if let Some(hc) = hc {
                    app.input = app.execs[hc].cmdline.clone();
                } else {
                    app.input.clear();
                }
            }
        },
    }

    iced::Task::none()
}
