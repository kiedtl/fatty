use crate::term::Term;

use iced::{
    advanced::{
        layout, renderer,
        widget::{self, Widget},
        Layout, Shell,
    },
    Color,
    Element, Length, Rectangle, Size,
};

pub struct Tty<'a, Theme> {
    term: &'a Term,
    theme: &'a Theme,
    font_size: f32,
    resolver: fn(&Term, &Theme, vte::ansi::Color) -> Color,
}

#[derive(Debug, Clone, Copy, Default)]
struct CellSize {
    width: f32,
    height: f32,
}

impl<'a, Theme> Tty<'a, Theme> {
    pub fn new(
        term: &'a Term,
        theme: &'a Theme,
        font_size: f32,
        resolver: fn(&Term, &Theme, vte::ansi::Color) -> Color,
    ) -> Self {
        Self { term, theme, font_size, resolver }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Tty<'a, Theme>
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

                // Background quad
                let bg = (self.resolver)(self.term, self.theme, cell.bg);
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
                        (self.resolver)(self.term, self.theme, cell.fg),
                        bounds,
                    );
                }
            }
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Tty<'a, Theme>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>,
    Theme: 'a,
{
    fn from(view: Tty<'a, Theme>) -> Self {
        Element::new(view)
    }
}
