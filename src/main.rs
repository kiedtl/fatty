use std::sync::Mutex;
use std::process::{Command, Child};
use std::time::Duration;
use std::io::{Read, Write};

use vte;
use rustix::fd::OwnedFd;
use rustix::process::{kill_process, Pid, Signal};

use iced::window;
use iced::{Event, Element, Task, Subscription, Length};
use iced::keyboard::{self, key, Modifiers};
use iced::widget::{container, Column, row, text::{Rich, Span}, column, text, text_input};

mod term;
mod utils;
mod styles;
mod colors;

use styles::CS;

static STDERR: Mutex<Option<std::fs::File>> = Mutex::new(None);
fn set_panic_output(w: OwnedFd) {
    *STDERR.lock().unwrap() = Some(std::fs::File::from(w));
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut guard) = STDERR.lock() {
            if let Some(w) = guard.as_mut() {
                let _ = writeln!(w, "panic: {info}");
                return;
            }
        }
        eprintln!("panic: {info}");
    }));
}

pub type Elem<'a> = Element<'a, Message, styles::Theme, iced::Renderer>;

fn main() -> iced::Result {
    install_panic_hook();
    set_panic_output(rustix::io::dup(std::io::stderr()).unwrap());

    iced::daemon(|| App::new(), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .run()
}

struct Execution {
    command: String,
    child: Child,
    pid: Pid,
    term: term::Term,
    done: bool,
    exit_reason: Option<ExitReason>,
}

enum ExitReason {
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

struct App {
    master: OwnedFd,
    input: String,
    execs: Vec<Execution>,
    ansi: vte::ansi::Processor,
    theme: styles::Theme
}

#[derive(Debug, Clone)]
pub enum Message {
    None,
    Input(String),
    Run,
    Poll,
    Signal(Signal),
}

impl App {
    fn theme(&self, _: window::Id) -> styles::Theme {
        self.theme
    }

    fn new() -> (Self, Task<Message>) {
        let (_id, open) = window::open(window::Settings::default());

        let pty = rustix_openpty::openpty(None, None).unwrap();
        rustix::stdio::dup2_stdout(&pty.user).unwrap();
        rustix::stdio::dup2_stderr(&pty.user).unwrap();
        rustix::stdio::dup2_stdin(&pty.user).unwrap();

        let master_flags = rustix::fs::fcntl_getfl(&pty.controller).unwrap();
        rustix::fs::fcntl_setfl(&pty.controller, master_flags | rustix::fs::OFlags::NONBLOCK).unwrap();

        (Self {
            input: String::new(),
            execs: Vec::new(),
            master: pty.controller,
            ansi: vte::ansi::Processor::new(),
            theme: styles::Theme::gruvbox(),
        }, open.map(|_| Message::None))
    }

    fn title(&self, _: window::Id) -> String {
        "fatty".to_owned()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::None => { }
            Message::Input(s) => self.input = s,
            Message::Run => {
                let mut args = self.input.split(" ");
                let Some(command) = args.next() else { return Task::none(); };

                let child = Command::new(command)
                    .args(args)
                    .spawn()
                    .unwrap();
                let pid = Pid::from_child(&child);

                let command = std::mem::take(&mut self.input);
                self.execs.push(Execution {
                    command, child, pid,
                    term: term::Term::new(),
                    done: false,
                    exit_reason: None,
                });
            },
            Message::Poll => {
                self.poll_pty();
                self.wait_child();
            }
            Message::Signal(sig) => {
                if let Some(current) = self.execs.last() {
                    kill_process(current.pid, sig).unwrap();
                    self.wait_child();
                }
            },
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let poll = if let Some(last) = self.execs.last() && !last.done {
            iced::time::every(Duration::from_millis(30)).map(|_| Message::Poll)
        } else {
            Subscription::none()
        };

        let keys = iced::event::listen_with(|ev, status, _id| {
            if status != iced::event::Status::Ignored {
                return None;
            }

            match ev {
                Event::Keyboard(keyboard::Event::KeyReleased { key: key::Key::Character(k), modifiers: Modifiers::CTRL, .. })
                    if k == "c" => Some(Message::Signal(Signal::INT)),
                Event::Keyboard(keyboard::Event::KeyReleased { key: key::Key::Character(k), modifiers: Modifiers::CTRL, .. })
                    if k == "\\" => Some(Message::Signal(Signal::QUIT)),
                _ => None,
            }
        });

        Subscription::batch([poll, keys])
    }

    fn view(&self, _: window::Id) -> Elem<'_> {
        let mut execs = Column::new()
            .spacing(1);

        for exec in &self.execs {
            execs = execs.push(
                column![
                    container(
                        row![
                            container(
                                text(&exec.command)
                            )
                                .width(Length::Fill),
                            match exec.exit_reason {
                                None => text("Running"),
                                Some(ExitReason::Normal(code)) => text(code.to_string()),
                                Some(ExitReason::Signal { signal, .. }) => text(utils::signal_to_string(signal)),
                                Some(ExitReason::Unknown { sigval: Some(s), .. }) => text(format!("Signal({s})")),
                                Some(ExitReason::Unknown { sigval: None, .. }) => text("Exited (unknown)"),
                            }
                        ],
                    )
                        .class(CS::Box)
                        .padding(3),
                    container(
                        Column::with_children(
                            exec.term.cells.iter()
                                .map(|line|
                                    Rich::<'_, (), Message, styles::Theme>::with_spans(
                                        line.iter()
                                            .map(|c|
                                                Span::new(c.ch)
                                                    .color(exec.term.resolve(&self.theme, c.fg))
                                                    .background(iced::Background::Color(exec.term.resolve(&self.theme, c.bg)))
                                                    .font(c.iced_font())
                                            )
                                            .collect::<Vec<_>>()
                                    )
                                        .into()
                                )
                        )
                    )
                        .padding(1)
                        .width(Length::Fill),
                ],
            );
        }

        let mut input = text_input("rm -rf /", &self.input);

        if let Some(last) = self.execs.last() && !last.done {} else {
            input = input
                .on_input(Message::Input)
                .on_submit(Message::Run);
        }

        container(
            column![
                container(execs),
                input,
            ]
                .spacing(2)
        )
            .padding(5)
            .width(Length::Fill)
            .height(Length::Fill)
            .class(CS::Outer)
            .into()
    }

    fn poll_pty(&mut self) {
        if let Some(last) = self.execs.last_mut() {
            let mut buf = [0u8; 4096];
            match rustix::io::read(&self.master, &mut buf) {
                Ok(n) => self.ansi.advance(&mut last.term, &buf[0..n]),
                Err(rustix::io::Errno::AGAIN) => (),
                e => _ = e.unwrap(),
            }
        }
    }

    fn wait_child(&mut self) {
        if let Some(current) = self.execs.last_mut() {
            match rustix::process::waitpid(Some(current.pid), rustix::process::WaitOptions::NOHANG) {
                Ok(Some((_, status))) => {
                    current.done = status.exited() || status.signaled();

                    if status.signaled() && let Some(sigval) = status.terminating_signal() {
                        let raw = status.as_raw();
                        let cored = raw & 0x80 != 0;

                        if let Some(signal) = Signal::from_named_raw(sigval) {
                            current.exit_reason = Some(ExitReason::Signal { signal, cored });
                            if cored {
                                print!("{} (core dumped)", utils::signal_to_string(signal));
                            }
                        } else {
                            current.exit_reason = Some(ExitReason::Unknown { sigval: Some(sigval), cored });
                            if cored {
                                print!("Signal({sigval}) (core dumped)");
                            }
                        }
                    } else if let Some(exit) = status.exit_status() {
                        current.exit_reason = Some(ExitReason::Normal(exit));
                    }

                    self.poll_pty();
                },
                _ => (),
            }
        }
    }
}
