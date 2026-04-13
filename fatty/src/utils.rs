use std::fmt;
use rustix::process::Signal;

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

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.0;

        let file_type = match m & 0o170000 {
            0o100000 => '-', // file
            0o040000 => 'd', // dir
            0o120000 => 'l', // symlink
            0o020000 => 'c', // char device (??)
            0o060000 => 'b', // block device
            0o010000 => 'p', // fifo/pipe
            0o140000 => 's', // socket
            _        => '?',
        };

        let bit = |mask: u32, ch: char| if m & mask != 0 { ch } else { '-' };

        let owner_x = match (m & 0o4000 != 0, m & 0o0100 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        let group_x = match (m & 0o2000 != 0, m & 0o0010 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        let other_x = match (m & 0o1000 != 0, m & 0o0001 != 0) {
            (true,  true)  => 't',
            (true,  false) => 'T',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        write!(f, "{}{}{}{}{}{}{}{}{}{}",
            file_type,
            bit(0o0400, 'r'), bit(0o0200, 'w'), owner_x,
            bit(0o0040, 'r'), bit(0o0020, 'w'), group_x,
            bit(0o0004, 'r'), bit(0o0002, 'w'), other_x,
        )
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
