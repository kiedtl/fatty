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
    content: impl Widget<Message, crate::styles::Theme, Renderer> + 'a,
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
    pub fn new(
        state: &'a ControlState,
        content: impl Widget<Message, crate::styles::Theme, Renderer> + 'a,
        control_message: Box<dyn Fn(ControlMessage) -> Message>,
    ) -> Self {
        let content = iced::Widget::boxed(content);

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

    pub fn boxed(self) -> Element<'a, Message, crate::styles::Theme, Renderer>
    where
        Message: 'a,
        Renderer: 'a + advanced::Renderer,
    {
        iced::Widget::boxed(self)
    }
}

impl<'a, Message, Renderer: advanced::Renderer> widget::Meta
    for Controller<'a, Message, Renderer>
{
}

impl<'a, Message, Renderer> Widget<Message, crate::styles::Theme, Renderer>
    for Controller<'a, Message, Renderer>
where
    Renderer: advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }

    fn state(&self) -> tree::State {
        self.content.state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
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
    ) {
        let size = self.size();
        layout::positioned(
            tree,
            limits,
            size.width,
            size.height,
            Padding::ZERO,
            |tree, limits| {
                self.content
                    .layout(tree, renderer, &limits.loose());

                tree.size
            },
            |content, container| {
                content.align(
                    container,
                    Alignment::from(self.align_x),
                    Alignment::from(self.align_y),
                )
            },
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let viewport = if self.clip {
            layout.bounds().intersection(viewport).unwrap_or_default()
        } else {
            *viewport
        };

        operation.container(None, layout.bounds(), &viewport);
        operation.traverse(
            &mut |operation| {
                let (layout, tree) = layout.iter_mut(&mut tree.children).next().unwrap();
                self.content.operate(
                    tree,
                    layout,
                    &viewport,
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
        layout: Layout,
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
                        (ControlMode::Normal, Modifiers::NONE, Some(';')) => {
                            self.send(shell, ControlMessage::ChangeMode(ControlMode::Leader));
                            return;
                        },
                        (ControlMode::Leader, Modifiers::NONE, Some('t')) => {
                            self.send(shell, ControlMessage::RotateListingMode);
                            return;
                        },
                        _ => (),
                    }
                }
                _ => (),
            }
        }

        let (layout, tree) = layout.iter_mut(&mut tree.children).next().unwrap();
        self.content.update(
            tree,
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let (layout, tree) = layout.iter(&tree.children).next().unwrap();
        self.content.mouse_interaction(
            tree,
            layout,
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
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        if let Some(clipped_viewport) = bounds.intersection(viewport) {
            let viewport = if self.clip { &clipped_viewport } else { viewport };
            let (layout, tree) = layout.iter(&tree.children).next().unwrap();
            self.content.draw(tree, renderer, theme, &renderer_style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, crate::styles::Theme, Renderer>> {
        let (layout, tree) = layout.iter_mut(&mut tree.children).next().unwrap();
        self.content.overlay(
            tree,
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}



pub fn update(app: &mut crate::App, message: ControlMessage) -> iced::Task<crate::Message> {
    use ControlMessage as CM;

    match message {
        CM::RotateListingMode => app.listing_mode = match app.listing_mode {
            None => Some(crate::ListingMode::Grid),
            Some(crate::ListingMode::Grid) => Some(crate::ListingMode::Table),
            Some(crate::ListingMode::Table) => None,
        },
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
