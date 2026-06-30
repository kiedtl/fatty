use std::borrow::Cow;
use crate::term::Term;

use iced::{
    keyboard::{self, key, Modifiers},
    advanced::{
        layout, renderer,
        widget::{self, Widget, Tree},
        Layout, Shell,
    },
    mouse,
    Event,
    Color,
    Element, Length, Rectangle, Size,
};

pub struct Tty<'a, Theme, Message> {
    term: &'a Term,
    theme: &'a Theme,
    font_size: f32,
    resolver: fn(&Term, &Theme, vte::ansi::Color) -> Color,
    on_input: Box<dyn Fn(Cow<'static, [u8]>) -> Message>,
    take_input: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct CellSize {
    width: f32,
    height: f32,
}

impl<'a, Theme, Message> Tty<'a, Theme, Message> {
    pub fn new(
        term: &'a Term,
        theme: &'a Theme,
        font_size: f32,
        resolver: fn(&Term, &Theme, vte::ansi::Color) -> Color,
        on_input: impl Fn(Cow<'static, [u8]>) -> Message + 'static,
        take_input: bool,
    ) -> Self {
        Self { term, theme, font_size, resolver, on_input: Box::new(on_input), take_input }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Tty<'a, Theme, Message>
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<CellSize>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(CellSize::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if !self.take_input {
            return;
        }

        match &event {
            Event::Keyboard(keyboard::Event::KeyPressed { modified_key, text, .. }) => {
                // TODO: application cursor mode, DECCKM, when arrows/home/end switch
                // from \x1b[ to \x1bO
                let spc: Option<&'static [u8]> = match modified_key.as_ref() {
                    keyboard::Key::Named(key::Named::Enter) => Some(b"\r"),
                    keyboard::Key::Named(key::Named::Tab) => Some(b"\t"),
                    keyboard::Key::Named(key::Named::Backspace) => Some(b"\x7f"),
                    keyboard::Key::Named(key::Named::Escape) => Some(b"\x1b"),
                    keyboard::Key::Named(key::Named::ArrowUp) => Some(b"\x1b[A"),
                    keyboard::Key::Named(key::Named::ArrowDown) => Some(b"\x1b[B"),
                    keyboard::Key::Named(key::Named::ArrowRight) => Some(b"\x1b[C"),
                    keyboard::Key::Named(key::Named::ArrowLeft) => Some(b"\x1b[D"),
                    keyboard::Key::Named(key::Named::Home) => Some(b"\x1b[H"),
                    keyboard::Key::Named(key::Named::End) => Some(b"\x1b[F"),
                    keyboard::Key::Named(key::Named::Insert) => Some(b"\x1b[2~"),
                    keyboard::Key::Named(key::Named::Delete) => Some(b"\x1b[3~"),
                    keyboard::Key::Named(key::Named::PageUp) => Some(b"\x1b[5~"),
                    keyboard::Key::Named(key::Named::PageDown) => Some(b"\x1b[6~"),
                    keyboard::Key::Named(key::Named::F1) => Some(b"\x1bOP"),
                    keyboard::Key::Named(key::Named::F2) => Some(b"\x1bOQ"),
                    keyboard::Key::Named(key::Named::F3) => Some(b"\x1bOR"),
                    keyboard::Key::Named(key::Named::F4) => Some(b"\x1bOS"),
                    keyboard::Key::Named(key::Named::F5) => Some(b"\x1b[15~"),
                    keyboard::Key::Named(key::Named::F6) => Some(b"\x1b[17~"),
                    keyboard::Key::Named(key::Named::F7) => Some(b"\x1b[18~"),
                    keyboard::Key::Named(key::Named::F8) => Some(b"\x1b[19~"),
                    keyboard::Key::Named(key::Named::F9) => Some(b"\x1b[20~"),
                    keyboard::Key::Named(key::Named::F10) => Some(b"\x1b[21~"),
                    keyboard::Key::Named(key::Named::F11) => Some(b"\x1b[23~"),
                    keyboard::Key::Named(key::Named::F12) => Some(b"\x1b[24~"),
                    _ => None,
                };

                if let Some(spc) = spc {
                    shell.publish((self.on_input)(spc.into()));
                    shell.capture_event();
                    return;
                }

                if let Some(text) = text {
                    shell.publish((self.on_input)(text.as_bytes().to_owned().into()));
                    shell.capture_event();
                    return;
                }
            }
            _ => (),
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        use iced::advanced::text::{self, Renderer as _};

        let cell_size = tree.state.downcast_mut::<CellSize>();

        // Measure once; re-use on subsequent layouts.
        if cell_size.width == 0.0 {
            let (w, h) = crate::utils::measure_text(
                "M", f32::INFINITY, self.font_size, 1.2,
                iced::Font {
                    family: iced::font::Family::Name("Drafting* Mono"),
                    ..Default::default()
                },
            );
            cell_size.width = w;
            cell_size.height = h;
        }

        let rows = self.term.cells.len() as f32;
        let cols = self.term.width as f32;
        layout::Node::new(Size {
            width: cols * cell_size.width,
            height: rows * cell_size.height,
        })
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::text::{self, Paragraph, Renderer as _};

        let origin = layout.bounds().position();
        let &CellSize { width: cw, height: ch } = tree.state.downcast_ref::<CellSize>();

        for (row, line) in self.term.cells.iter().enumerate() {
            for (col, cell) in line.iter().enumerate() {
                let x = origin.x + col as f32 * cw;
                let y = origin.y + row as f32 * ch;
                let bounds = Rectangle { x, y, width: cw, height: ch };

                let mut bg = cell.bg;
                let mut fg = cell.fg;
                if row == self.term.cursor_y && col == self.term.cursor_x {
                    bg = cell.fg;
                    fg = cell.bg;
                }

                // Background quad
                let bg = (self.resolver)(self.term, self.theme, bg);
                renderer.fill_quad(
                    renderer::Quad { bounds, ..Default::default() },
                    iced::Background::Color(bg),
                );

                // Glyph
                if cell.ch != ' ' && cell.ch != '\t' {
                    renderer.fill_text(
                        text::Text {
                            content: cell.ch.to_string(),
                            bounds: bounds.size(),
                            size: iced::Pixels(self.font_size),
                            font: cell.iced_font(),
                            align_x: iced::alignment::Horizontal::Left.into(),
                            align_y: iced::alignment::Vertical::Top.into(),
                            line_height: text::LineHeight::Absolute(iced::Pixels(ch)),
                            shaping: text::Shaping::Basic,
                            wrapping: text::Wrapping::None,
                            ellipsis: Default::default(),
                            hint_factor: Default::default(),
                        },
                        iced::Point { x, y },
                        (self.resolver)(self.term, self.theme, fg),
                        bounds,
                    );
                }
            }
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Tty<'a, Theme, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>,
    Theme: 'a,
    Message: 'a,
{
    fn from(view: Tty<'a, Theme, Message>) -> Self {
        Element::new(view)
    }
}
