use iced::{Gradient, Background, Border, border::Radius, Color};
use iced::widget::{container, button, text_input, table};

use vte::ansi::Rgb;
use crate::colors::Hsv;
use crate::widgets::scrollable;
use crate::widgets::input;

const RAD_PX: f32 = 2.;
pub const RAD: Radius = Radius { top_left: RAD_PX, top_right: RAD_PX, bottom_right: RAD_PX, bottom_left: RAD_PX };

#[derive(Copy, Clone)]
pub struct Theme {
    pub palette: [Rgb; 16],

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
        Theme {
            palette: [
                Rgb { r: 0x3c, g: 0x38, b: 0x36 }, //  0 black
                Rgb { r: 0xcc, g: 0x24, b: 0x1d }, //  1 red
                Rgb { r: 0x98, g: 0x97, b: 0x1a }, //  2 green
                Rgb { r: 0xd7, g: 0x99, b: 0x21 }, //  3 yellow
                Rgb { r: 0x45, g: 0x85, b: 0x88 }, //  4 blue
                Rgb { r: 0xb1, g: 0x62, b: 0x86 }, //  5 magenta
                Rgb { r: 0x68, g: 0x9d, b: 0x6a }, //  6 cyan
                Rgb { r: 0xfe, g: 0xf9, b: 0xf3 }, //  7 white
                Rgb { r: 0xc2, g: 0xb3, b: 0xa4 }, //  8 bright black
                Rgb { r: 0x9d, g: 0x00, b: 0x06 }, //  9 bright red
                Rgb { r: 0x79, g: 0x74, b: 0x0e }, // 10 bright green
                Rgb { r: 0xb5, g: 0x76, b: 0x14 }, // 11 bright yellow
                Rgb { r: 0x07, g: 0x66, b: 0x78 }, // 12 bright blue
                Rgb { r: 0x8f, g: 0x3f, b: 0x71 }, // 13 bright magenta
                Rgb { r: 0x42, g: 0x7b, b: 0x58 }, // 14 bright cyan
                Rgb { r: 0x3c, g: 0x38, b: 0x36 }, // 15 bright white
            ],

            fg: Color::from_rgb8(0x28, 0x28, 0x28),
            white: Color::from_rgb8(0xfb, 0xfb, 0xe7),

            bg_hue: 120, //33,
            bg_sat: 0.03, //0.14, //0.153,

            ac_hue: 20, //19,
            ac_sat: 0.21, //0.61,
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

impl iced::widget::text::Catalog for Theme {
    type Class<'a> = TextClass;

    fn default<'a>() -> Self::Class<'a> {
        TextClass::Normal
    }

    fn style(&self, class: &Self::Class<'_>) -> iced::widget::text::Style {
        iced::widget::text::Style {
            color: class.color(self),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum TextClass {
    Normal,
    Custom(fn(&Theme) -> Color),
}

impl TextClass {
    pub fn color(self, t: &Theme) -> Option<Color> {
        match self {
            TextClass::Normal => None,
            TextClass::Custom(c) => Some((c)(t)),
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

    fn default<'a>() -> Self::Class<'a> {
        Box::new(myinput)
    }

    fn style(&self, class: &Self::Class<'_>, status: input::Status) -> input::Style {
        class(self, status)
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
            //background: background::color(t.ac(7)),
            color: t.ac(7),
            border: Border {
                color: t.bg(2),
                width: 1.,
                radius: RAD,
                ..Default::default()
            },
        },
    };

    match s {
        scrollable::Status::Active { .. } => {
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: Default::default(),
            }
        },
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered: ish,
            is_vertical_scrollbar_hovered: isv,
            ..
        } => {
            let h = scrollable::Rail {
                scroller: scrollable::Scroller {
                    //background: background::Color(t.ac(10)),
                    color: t.ac(9),
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
                gap: Default::default(),
            }
        },
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged: ish,
            is_vertical_scrollbar_dragged: isv,
            ..
        } => {
            let h = scrollable::Rail {
                scroller: scrollable::Scroller {
                    //background: background::Color(t.ac(10)),
                    color: t.ac(10),
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
                gap: Default::default(),
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
        icon: t.ac(4),
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
