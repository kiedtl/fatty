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
