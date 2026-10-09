use iced::{Gradient, Background, Border, border::Radius, Color, Shadow};
use iced::widget::{container, button, text_input, table, scrollable};

use vte::ansi::Rgb;
use crate::colors::{CIELAB, Hsv};
use crate::widgets::input;
use crate::widgets::spinnerbar;
use crate::widgets::menu;

const RAD_PX: f32 = 2.;
pub const RAD: Radius = Radius { top_left: RAD_PX, top_right: RAD_PX, bottom_right: RAD_PX, bottom_left: RAD_PX };

// https://gist.github.com/jake-stewart/0a8ea46159a7da2c808e5be2177e1783
fn full_palette(basepal: [Color; 16]) -> [Rgb; 256] {
    let base8_lab = [
        CIELAB::from_rgb(basepal[0]),
        CIELAB::from_rgb(basepal[1]),
        CIELAB::from_rgb(basepal[2]),
        CIELAB::from_rgb(basepal[3]),
        CIELAB::from_rgb(basepal[4]),
        CIELAB::from_rgb(basepal[5]),
        CIELAB::from_rgb(basepal[6]),
        CIELAB::from_rgb(basepal[7]),
    ];

    //let is_light = base8_lab[7].L < base8_lab[0].L;
    let mut pal = [basepal[0]; 256];
    for (pali, basei) in pal.iter_mut().zip(basepal.iter()) {
        *pali = *basei;
    }
    let mut i = 16;

    for r in 0..6 {
        let c0 = base8_lab[0].lerp(base8_lab[1], r as f64 / 5.);
        let c1 = base8_lab[2].lerp(base8_lab[3], r as f64 / 5.);
        let c2 = base8_lab[4].lerp(base8_lab[5], r as f64 / 5.);
        let c3 = base8_lab[6].lerp(base8_lab[7], r as f64 / 5.);
        for g in 0..6 {
            let c4 = c0.lerp(c1, g as f64 / 5.);
            let c5 = c2.lerp(c3, g as f64 / 5.);
            for b in 0..6 {
                let c6 = c4.lerp(c5, b as f64 / 5.);
                pal[i] = c6.to_rgb();
                i += 1;
            }
        }
    }

    for gr in 0..24 {
        let t = (gr as f64 + 1.) / 25.;
        let lab = base8_lab[0].lerp(base8_lab[7], t);
        pal[i] = lab.to_rgb();
        i += 1;
    }

    std::array::from_fn::<Rgb, 256, _>(|i| {
        let Color { r, g, b, a: _ } = pal[i];
        Rgb {
            r: (r * 255.) as u8,
            g: (g * 255.) as u8,
            b: (b * 255.) as u8,
        }
    })
}

#[derive(Copy, Clone)]
pub struct Theme {
    pub palette: [Rgb; 256],

    pub fg: Color,
    pub white: Color,

    pub bg_hue: u16,
    pub bg_sat: f32,

//     pub red_hue: u16,
//     pub red_sat: f32,

    pub ac_hue: u16,
    pub ac_sat: f32,
}

impl Theme {
    pub fn gruvbox() -> Theme {
        let base = [
            Color::from_rgb8(0x3c, 0x38, 0x36), //  0 black
            Color::from_rgb8(0xcc, 0x24, 0x1d), //  1 red
            Color::from_rgb8(0x98, 0x97, 0x1a), //  2 green
            Color::from_rgb8(0xd7, 0x99, 0x21), //  3 yellow
            Color::from_rgb8(0x45, 0x85, 0x88), //  4 blue
            Color::from_rgb8(0xb1, 0x62, 0x86), //  5 magenta
            Color::from_rgb8(0x68, 0x9d, 0x6a), //  6 cyan
            Color::from_rgb8(0xfe, 0xf9, 0xf3), //  7 white
            Color::from_rgb8(0xc2, 0xb3, 0xa4), //  8 bright black
            Color::from_rgb8(0x9d, 0x00, 0x06), //  9 bright red
            Color::from_rgb8(0x79, 0x74, 0x0e), // 10 bright green
            Color::from_rgb8(0xb5, 0x76, 0x14), // 11 bright yellow
            Color::from_rgb8(0x07, 0x66, 0x78), // 12 bright blue
            Color::from_rgb8(0x8f, 0x3f, 0x71), // 13 bright magenta
            Color::from_rgb8(0x42, 0x7b, 0x58), // 14 bright cyan
            Color::from_rgb8(0x3c, 0x38, 0x36), // 15 bright white
        ];
        Theme {
            palette: full_palette(base),

            fg: Color::from_rgb8(0x28, 0x28, 0x28),
            white: Color::from_rgb8(0xfb, 0xfb, 0xe7),

            bg_hue: 120, //33,
            bg_sat: 0.03, //0.14, //0.153,

            ac_hue: 20, //19,
            ac_sat: 0.38, //0.61,
        }
    }

    pub fn bg(&self, index: usize) -> Color {
        let v = (index & 15) as f32 * (1. / 15.);
        (Hsv { h: self.bg_hue, s: self.bg_sat, v }).to_color()
    }

    pub fn ac(&self, index: usize) -> Color {
        let v = (index & 15) as f32 * (1. / 15.);
        (Hsv { h: self.ac_hue, s: self.ac_sat, v }).to_color()
    }
}

impl iced::theme::Base for Theme {
    fn name(&self) -> &str {
        "fatty"
    }

    fn base(&self) -> iced::theme::Style {
        iced::theme::Style {
            background_color: self.bg(14),
            text_color: self.fg,
        }
    }

    fn default(preference: iced::theme::Mode) -> Self {
        match preference {
            iced::theme::Mode::None | iced::theme::Mode::Light => Theme::gruvbox(),
            iced::theme::Mode::Dark => todo!(),
        }
    }

    fn mode(&self) -> iced::theme::Mode {
        iced::theme::Mode::Light
    }

    fn seed(&self) -> Option<iced::theme::palette::Seed> {
        Some(iced::theme::palette::Seed {
            background: self.bg(14),
            text: self.fg,
            primary: self.fg,
            success: self.fg,
            warning: self.fg,
            danger: self.fg,
        })
    }
}

type MenuStyleFn<'a> = Box<dyn Fn(&Theme) -> menu::Style + 'a>;
impl menu::Catalog for Theme {
    type Class<'a> = MenuStyleFn<'a>;

    fn default<'a>() -> MenuStyleFn<'a> {
        Box::new(|t| {
            menu::Style {
                background: Background::Color(t.bg(13)),
                border: Border {
                    width: 1.0,
                    radius: RAD,
                    color: t.bg(1),
                },
                text_color: t.fg,
                selected_text_color: t.bg(14),
                selected_background: Background::Color(t.bg(4)),
                shadow: Default::default(),
            }
        })
    }

    fn style(&self, class: &MenuStyleFn<'_>) -> menu::Style {
        class(self)
    }
}

impl iced::widget::text::Catalog for Theme {
    type Class<'a> = TextClass;

    fn default<'a>() -> Self::Class<'a> {
        TextClass::Normal
    }

    fn style(&self, class: &Self::Class<'_>) -> iced::widget::text::Style {
        iced::widget::text::Style {
            color: class.color(self),
            selection: class.sel(self),
        }
    }

    fn selection(&self) -> iced::Color {
        self.bg(12)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum TextClass {
    Normal,
    Custom(fn(&Theme) -> Color, fn(&Theme) -> Color),
}

impl TextClass {
    pub fn color(self, t: &Theme) -> Option<Color> {
        match self {
            TextClass::Normal => None,
            TextClass::Custom(c, _) => Some((c)(t)),
        }
    }

    pub fn sel(self, t: &Theme) -> Option<Color> {
        match self {
            TextClass::Normal => None,
            TextClass::Custom(_, c) => Some((c)(t)),
        }
    }
}

impl container::Catalog for Theme {
    type Class<'a> = CS;

    fn default<'a>() -> Self::Class<'a> {
        CS::Base
    }

    fn style(&self, class: &Self::Class<'_>) -> container::Style {
        class.style(self)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum CS {
    Outer,
    Base,
    Box,
    WhiteBox,
    GrayBox,
    FadingHighlight(f32),
    Custom(fn(&Theme) -> container::Style),
    Custom2(container::Style),
}

type CSFunc = fn(&Theme) -> container::Style;
impl From<CSFunc> for CS {
    fn from(f: CSFunc) -> CS {
        CS::Custom(f)
    }
}

impl CS {
    pub fn style(self, t: &Theme) -> container::Style {
        match self {
            CS::Outer => container::Style {
                text_color: Some(t.fg),
                background: Some(Background::Color(t.bg(14))),
                snap: true,
                ..Default::default()
            },
            CS::Base => container::Style {
                text_color: Some(t.fg),
                background: None,
                snap: true,
                ..Default::default()
            },
            CS::Box => container::Style {
                text_color: Some(t.fg),
                background: Some(Background::Gradient(
                        Gradient::Linear(
                            iced::gradient::Linear::new(std::f32::consts::PI)
                                .add_stop(0.0, t.bg(15))
                                .add_stop(0.2, t.bg(14))
                                .add_stop(0.8, t.bg(12)),
                        )
                )),
                border: Border {
                    width: 1.,
                    radius: RAD,
                    color: t.bg(6),
                },
                shadow: Default::default(),
                snap: true,
            },
            CS::FadingHighlight(a) => container::Style {
                text_color: Some(t.fg),
                background: Some(Background::Color(t.ac(11).scale_alpha(a))),
                border: Border {
                    width: 1.,
                    radius: RAD,
                    color: t.ac(4).scale_alpha(a),
                },
                snap: true,
                ..Default::default()
            },
            CS::WhiteBox => container::Style {
                text_color: Some(t.fg),
                background: Some(Background::Color(t.white)),
                border: Border {
                    width: 1.,
                    radius: RAD,
                    color: t.bg(4),
                },
                shadow: Default::default(),
                snap: true,
            },
            CS::GrayBox => container::Style {
                text_color: Some(t.fg),
                background: Some(Background::Color(t.bg(14))),
                border: Border {
                    width: 1.,
                    radius: RAD,
                    color: t.bg(4),
                },
                shadow: Default::default(),
                snap: true,
            },
            CS::Custom(func) => (func)(t),
            CS::Custom2(s) => s,
        }
    }
}

impl scrollable::Catalog for Theme {
    type Class<'a> = Box<dyn Fn(&Theme, scrollable::Status) -> scrollable::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(scrollable_style)
    }

    fn style(&self, class: &Self::Class<'_>, status: scrollable::Status) -> scrollable::Style {
        class(self, status)
    }
}

impl text_input::Catalog for Theme {
    type Class<'a> = Box<dyn Fn(&Theme, text_input::Status) -> text_input::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(input)
    }

    fn style(&self, class: &Self::Class<'_>, status: text_input::Status) -> text_input::Style {
        class(self, status)
    }
}

impl input::Catalog for Theme {
    type Class<'a> = Box<dyn Fn(&Theme, input::Status) -> input::Style + 'a>;

    fn default<'a>() -> <Self as input::Catalog>::Class<'a> {
        Box::new(myinput)
    }

    fn style(&self, class: &<Self as input::Catalog>::Class<'_>, status: input::Status) -> input::Style {
        class(self, status)
    }
}

impl spinnerbar::Catalog for Theme {
    fn style(&self) -> spinnerbar::Style {
        spinnerbar::Style {
            border: self.bg(3),
            border_radius: RAD_PX,
            track: self.bg(8),
            fill: self.ac(8),
        }
    }
}

impl button::Catalog for Theme {
    type Class<'a> = Box<dyn Fn(&Theme, button::Status) -> button::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|_theme, _| iced::widget::button::Style::default())
    }

    fn style(&self, class: &Self::Class<'_>, status: button::Status) -> button::Style {
        class(self, status)
    }
}

impl table::Catalog for Theme {
    type Class<'a> = TableStyle;

    fn default<'a>() -> Self::Class<'a> {
        TableStyle::default()
    }

    fn style(&self, class: &Self::Class<'_>) -> table::Style {
        match class {
            TableStyle::Base => table::Style {
                separator_x: Background::Color(self.white),
                separator_y: Background::Color(self.ac(2)),
            },
        }
    }
}

#[derive(Default)]
pub enum TableStyle {
    #[default]
    Base,
}

pub fn scrollable_style(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: Some(Background::Color(t.bg(15))),
        border: Border {
            radius: RAD,
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(t.ac(7)),
            border: Border {
                color: t.bg(2),
                width: 1.,
                radius: RAD,
                ..Default::default()
            },
        },
    };

    let auto_scroll = scrollable::AutoScroll {
        background: Background::Color(t.bg(15)),
        border: Border {
            radius: RAD,
            ..Default::default()
        },
        shadow: Default::default(),
        icon: t.fg,
    };

    match s {
        scrollable::Status::Active { .. } => {
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                auto_scroll,
            }
        },
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered: ish,
            is_vertical_scrollbar_hovered: isv,
            ..
        } => {
            let h = scrollable::Rail {
                scroller: scrollable::Scroller {
                    background: Background::Color(t.ac(9)),
                    border: Border {
                        color: t.bg(2),
                        width: 1.,
                        radius: RAD,
                        ..Default::default()
                    },
                    ..rail.scroller
                },
                ..rail
            };
            scrollable::Style {
                vertical_rail: if isv { h } else { rail },
                horizontal_rail: if ish { h } else { rail },
                container: container::Style::default(),
                gap: None,
                auto_scroll,
            }
        },
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged: ish,
            is_vertical_scrollbar_dragged: isv,
            ..
        } => {
            let h = scrollable::Rail {
                scroller: scrollable::Scroller {
                    background: Background::Color(t.ac(10)),
                    border: Border {
                        color: t.bg(2),
                        width: 1.,
                        radius: RAD,
                        ..Default::default()
                    },
                    ..rail.scroller
                },
                ..rail
            };
            scrollable::Style {
                vertical_rail: if isv { h } else { rail },
                horizontal_rail: if ish { h } else { rail },
                container: container::Style::default(),
                gap: None,
                auto_scroll,
            }
        },
    }
}

pub fn input(t: &Theme, status: text_input::Status) -> text_input::Style {
    let active = text_input::Style {
        background: iced::Background::Color(t.white),
        border: Border {
            radius: RAD,
            width: 2.,
            color: t.bg(8),
        },
        placeholder: t.bg(8),
        value: t.fg,
        selection: t.ac(14),
    };

    match status {
        text_input::Status::Active => active,
        text_input::Status::Hovered => text_input::Style {
            border: Border {
                color: t.ac(12),
                ..active.border
            },
            ..active
        },
        text_input::Status::Focused { .. } => text_input::Style {
            border: Border {
                color: t.ac(8),
                ..active.border
            },
            ..active
        },
        text_input::Status::Disabled => text_input::Style {
            background: iced::Background::Color(t.bg(14)),
            value: active.placeholder,
            ..active
        },
    }
}

pub fn myinput(t: &Theme, status: input::Status) -> input::Style {
    let active = input::Style {
        background: iced::Background::Color(t.white),
        border: Border {
            radius: RAD,
            width: 2.,
            color: t.bg(8),
        },
        icon: t.ac(4),
        placeholder: t.bg(8),
        value: t.fg,
        cursor: t.bg(10),
        selection: t.ac(14),
    };

    match status {
        input::Status::Active => active,
        input::Status::Hovered => input::Style {
            border: Border {
                color: t.ac(12),
                ..active.border
            },
            ..active
        },
        input::Status::Focused { .. } => input::Style {
            border: Border {
                color: t.ac(8),
                ..active.border
            },
            ..active
        },
        input::Status::Disabled => input::Style {
            background: iced::Background::Color(t.bg(14)),
            value: active.placeholder,
            ..active
        },
    }
}
