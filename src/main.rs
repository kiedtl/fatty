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

pub type Elem<'a> = Element<'a, Message>;

fn main() -> iced::Result {
    install_panic_hook();
    set_panic_output(rustix::io::dup(std::io::stderr()).unwrap());

    iced::daemon(|| App::new(), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .run()
}

struct Execution {
    command: String,
    child: Child,
    pid: Pid,
    term: term::Term,
    done: bool,
    exit: Option<i32>,
}

struct App {
    master: OwnedFd,
    input: String,
    execs: Vec<Execution>,
    ansi: vte::ansi::Processor,
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
                    exit: None,
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
                        text(&exec.command)
                    ),
                    container(
                        Column::with_children(
                            exec.term.cells.iter()
                                .map(|line|
                                    Rich::<'_, (), Message>::with_spans(
                                        line.iter()
                                            .map(|c|
                                                Span::new(c.ch)
                                                    .color(exec.term.resolve(c.fg))
                                                    .background(iced::Background::Color(exec.term.resolve(c.bg)))
                                                    .font(c.iced_font())
                                            )
                                            .collect::<Vec<_>>()
                                    )
                                        .into()
                                )
                        )
                    )
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
            .padding(3)
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
                        if let Some(signal) = Signal::from_named_raw(sigval) {
                            match signal {
                                Signal::ABORT => print!("Aborted"),
                                Signal::BUS => print!("Bus error"),
                                Signal::FPE => print!("Floating-point exception"),
                                Signal::HUP => print!("Hanged up"),
                                Signal::ILL => print!("Illegal instruction"),
                                Signal::INT => print!("Interrupted"),
                                Signal::KILL => print!("Murdered"),
                                Signal::PIPE => print!("Broken pipe"),
                                Signal::QUIT => print!("Quit"),
                                Signal::SEGV => print!("Segmentation fault"),
                                Signal::TERM => print!("Terminated"),
                                Signal::TRAP => print!("Trapped"),
                                _ => print!("{signal:?}"),
                            }
                        } else {
                            print!("Signal({sigval})");
                        }

                        let raw = status.as_raw();
                        let coredumped = raw & 0x80 != 0;

                        if coredumped {
                            println!(" (core dumped)");
                        } else {
                            println!("");
                        }

                    }

                    if current.done {
                        current.exit = status.exit_status();
                    }

                    self.poll_pty();
                },
                _ => (),
            }
        }
    }
}
