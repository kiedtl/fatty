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
