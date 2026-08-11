use std::fmt;
use std::sync::Arc;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::hash::Hash;

use rustix::io::Errno;
use rustix::process::Signal;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};

use iced::{alignment, Length, Background, Border, Subscription};
use iced::widget::{Text, text, Row, row, column, container, responsive};
use iced::advanced::text::IntoFragment;
use iced::advanced::subscription::{Recipe, EventStream, Hasher};

use crate::colors;
use crate::styles::{self, CS, TextClass, Theme};
use crate::Elem;
use crate::helpers::*;

// Copied wholesale from Iced's source, after run_with_id() was removed.
pub struct Runner<I, F, S, T>
where
    F: FnOnce(EventStream) -> S,
    S: Stream<Item = T>,
{
    pub id: I,
    pub spawn: F,
}

impl<I, F, S, T> Recipe for Runner<I, F, S, T>
where
    I: Hash + 'static,
    F: FnOnce(EventStream) -> S,
    S: Stream<Item = T> + Send + 'static,
{
    type Output = T;

    fn hash(&self, state: &mut Hasher) {
        std::any::TypeId::of::<I>().hash(state);
        self.id.hash(state);
    }

    fn stream(self: Box<Self>, input: EventStream) -> BoxStream<'static, Self::Output> {
        futures::stream::StreamExt::boxed((self.spawn)(input))
    }
}

pub fn signal_to_string(signal: Signal) -> &'static str {
    match signal {
        Signal::ABORT => "Aborted",
        Signal::BUS => "Bus error",
        Signal::FPE => "Floating-point exception",
        Signal::HUP => "Hanged up",
        Signal::ILL => "Illegal instruction",
        Signal::INT => "Interrupted",
        Signal::KILL => "Murdered",
        Signal::PIPE => "Broken pipe",
        Signal::QUIT => "Quit",
        Signal::SEGV => "Segmentation fault",
        Signal::TERM => "Terminated",
        Signal::TRAP => "Trapped",
        _ => "Unknown",
    }
}

pub fn fmt_size(size: u64) -> String {
    let (prec, fac, suffix) = match size {
        0..1000 => (0, 1.0, ""),
        1000..1_000_000 => (0, 1000.0, "kB"),
        1_000_000..1_000_000_000 => (1, 1_000_000.0, "MB"),
        1_000_000_000..=u64::MAX => (2, 1_000_000_000.0, "GB"),
    };
    format!("{:.*}{suffix}", prec, (size as f32) / fac)
}

pub struct Mode(pub u32);

// impl fmt::Display for Mode {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
impl Mode {
    pub fn to_iced<'a>(&self) -> Elem<'a> {
        const NONE_CHAR: char = '⬥'; //'-'; //'·';
        const BASE: u32 = 0x777777;
        const R: u32 = 0xdd0000;
        const W: u32 = 0x227700;
        const X: u32 = 0x0000ff;

        let m = self.0;

        fn txt<'a>(f: impl IntoFragment<'a>) -> Text<'a, Theme> {
            mono(f)
                .line_height(0.75)
                .size(10.)
                .class(TextClass::Custom(|t: &Theme| t.white))
        }

        let bit = |mask: u32, ch: char| txt(if m & mask != 0 { ch } else { NONE_CHAR });
        let bitc = |mask: u32, mix: u32, c: u32| if m & mask != 0 { colors::mix(c, mix, 0.3) } else { c };

        let owner_x = match (m & 0o4000 != 0, m & 0o0100 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => NONE_CHAR,
        };

        let group_x = match (m & 0o2000 != 0, m & 0o0010 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => NONE_CHAR,
        };

        let other_x = match (m & 0o1000 != 0, m & 0o0001 != 0) {
            (true,  true)  => 't',
            (true,  false) => 'T',
            (false, true)  => 'x',
            (false, false) => NONE_CHAR,
        };

        let color_u = bitc(0o0100, X, bitc(0o0200, W, bitc(0o0400, R, BASE)));
        let color_g = bitc(0o0010, X, bitc(0o0020, W, bitc(0o0040, R, BASE)));
        let color_o = bitc(0o0001, X, bitc(0o0002, W, bitc(0o0004, R, BASE)));

        let set = [
            (color_u, bit(0o0400, 'r'), bit(0o0200, 'w'), txt(owner_x)),
            (color_g, bit(0o0040, 'r'), bit(0o0020, 'w'), txt(group_x)),
            (color_o, bit(0o0004, 'r'), bit(0o0002, 'w'), txt(other_x)),
        ];

        let mut thr = Row::new().spacing(1).width(Length::Shrink);

        for (bg, r, w, x) in set {
            thr = thr.push(
                container(
                    column![
                        r, row![w, x]
                    ].align_x(alignment::Horizontal::Center)
                )
                    .padding(1.)
                    .class(CS::Custom2(container::Style {
                        background: Some(Background::Color(colors::iced_color(bg))),
                        border: Border {
                            radius: styles::RAD,
                            ..Default::default()
                        },
                        ..Default::default()
                    }))
            );
        }

        thr.into()
    }
}

pub fn measure_text(
    content: &str,
    container_width: f32,
    text_size: f32,
    line_height: f32, // Relative
    font: iced::font::Font,
) -> (f32, f32) {
    use iced::alignment;
    use iced::advanced::graphics::text::Paragraph;
    use iced::advanced::text::Paragraph as _;

    let p = Paragraph::with_text(iced::advanced::Text {
        content,
        bounds: iced::Size::new(container_width, f32::INFINITY),
        size: iced::Pixels(text_size),
        line_height: iced::widget::text::LineHeight::Relative(line_height),
        font,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Top,
        shaping: iced::widget::text::Shaping::Basic,
        wrapping: iced::widget::text::Wrapping::Word,
        hint_factor: Some(1.0),
        ellipsis: Default::default(),
    });

    (p.min_width(), p.min_height())
}

// Wrapper for BorrowedFd<'_> that implements Read + Write with rustix
pub struct FdRw(pub Arc<OwnedFd>);

impl Read for FdRw {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match rustix::io::read(&self.0, &mut *buf) {
                Ok(n) => return Ok(n),
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Write for FdRw {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match rustix::io::write(&self.0, buf) {
                Ok(n) => return Ok(n),
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }

    // Raw fd writes are unbuffered -- no flushing
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
