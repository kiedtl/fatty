use std::time::{Duration, Instant};

use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{layout, mouse, renderer, text};
use iced::advanced::{Layout, Widget};
use iced::event::Event;
use iced::{
    window, Background, Border, Color, Element, Gradient,
    Length, Rectangle, Size,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Shuffle,
}

pub struct SpinnerBar<Theme = crate::styles::Theme>
where
    Theme: Catalog,
{
    tick: u64,
    width: Length,
    height: f32,
    bkind: Kind,
    _theme: std::marker::PhantomData<Theme>,
}

impl<Theme> SpinnerBar<Theme>
where
    Theme: Catalog,
{
    pub fn new(tick: u64, bkind: Kind) -> Self {
        Self {
            tick, bkind,
            width: Length::Fill,
            height: 20.,
            _theme: std::marker::PhantomData,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

/// Colors and rendering variant for a [`SpinnerBar`].
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub border: Color,
    pub border_radius: f32,
    pub track: Color, // Empty track color
    pub fill: Color, // Filled portion color
}

pub trait Catalog {
    fn style(&self) -> Style;
}

struct State {}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for SpinnerBar<Theme>
where
    Theme: Catalog,
    Renderer: renderer::Renderer + text::Renderer<Font = iced::Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {})
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.height))
    }

    fn layout(&mut self, _: &mut Tree, _: &Renderer, lim: &layout::Limits) -> layout::Node {
        let size = lim.resolve(self.width, Length::Fixed(self.height), Size::new(200.0, self.height));
        layout::Node::new(size)
    }

    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let style = theme.style();
        let bounds = layout.bounds();

        match self.bkind {
            Kind::Shuffle => draw_shuffle(renderer, bounds, self.tick, &style),
        }
    }
}

// https://codeberg.org/toothrot/atim/src/branch/main/atim_classic/src/scroll_bar.rs#L221
fn draw_shuffle<R: renderer::Renderer>(
    renderer: &mut R,
    bounds: Rectangle,
    tick: u64,
    style: &Style,
) {
    let Rectangle { x, y, width, height } = bounds;
    let l1_w = width * 0.34;
    let l2_w = width * 0.18;
    let l3_w = width * 0.07;

    let wt = tick as f32 * width * 0.02;

    let mut l3_s_w = wt % ((width - l3_w) * 2.);
    let mut l2_s_w = (l3_s_w + l3_w) - l2_w;
    let mut l1_s_w = (l3_s_w + l3_w) - l1_w;
    if l3_s_w + l3_w > width {
        l3_s_w = (width * 2.) - l3_s_w - (l3_w * 2.);
        l2_s_w = l3_s_w;
        l1_s_w = l3_s_w;
    }
    l2_s_w = l2_s_w.clamp(0., width - l2_w);
    l1_s_w = l1_s_w.clamp(0., width - l1_w);

    {
        for x1 in (0..(width as usize)).step_by(3) {
            for y1 in (0..(height as usize)).skip(x1 % 2).step_by(3) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: x1 as f32 + x,
                            y: y1 as f32 + y,
                            width: 1.,
                            height: 1.,
                        },
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    style.track,
                )
            }
        }
    }

    {
        for x1 in ((l1_s_w as usize)..(l1_s_w as usize + l1_w as usize)).step_by(2) {
            for y1 in (0..(height as usize)).skip(x1 % 2).step_by(2) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: x1 as f32 + x,
                            y: y1 as f32 + y,
                            width: 1.,
                            height: 1.,
                        },
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    style.fill,
                )
            }
        }
    }

    {
        for x1 in (l2_s_w as usize)..(l2_s_w as usize + l2_w as usize) {
            for y1 in (0..(height as usize)).skip(x1 % 3).step_by(3) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: x1 as f32 + x,
                            y: y1 as f32 + y,
                            width: 1.,
                            height: 1.,
                        },
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    style.fill,
                )
            }
        }
    }

    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: x + l3_s_w,
                y,
                width: l3_w,
                height,
            },
            snap: true,
            ..renderer::Quad::default()
        },
        style.fill,
    )
}

impl<'a, Message, Theme, Renderer> From<SpinnerBar<Theme>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: 'a + Catalog,
    Renderer: 'a + renderer::Renderer + text::Renderer<Font = iced::Font>,
{
    fn from(w: SpinnerBar<Theme>) -> Self {
        Self::new(w)
    }
}
