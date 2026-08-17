pub mod compiler;
pub mod parser;
pub mod vm;
mod utils;

use rustix::process::Signal;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExitReason {
    Normal(i32),
    Signal {
        signal: Signal,
        cored: bool,
    },
    Unknown {
        sigval: Option<i32>,
        cored: bool,
    },
}

impl From<rustix::process::WaitStatus> for ExitReason {
    fn from(status: rustix::process::WaitStatus) -> ExitReason {
        let raw = status.as_raw();
        let cored = raw & 0x80 != 0;

        if status.signaled() && let Some(sigval) = status.terminating_signal() {
            if let Some(signal) = Signal::from_named_raw(sigval) {
                ExitReason::Signal { signal, cored }
            } else {
                ExitReason::Unknown { sigval: Some(sigval), cored }
            }
        } else if let Some(exit) = status.exit_status() {
            ExitReason::Normal(exit)
        } else {
            ExitReason::Unknown { sigval: None, cored }
        }
    }
}
