#![allow(unused_imports)]

use std::sync::{Arc, Mutex};
use std::process::{Command, Child};
use std::time::Duration;
use std::io::{Read, Write};
use std::cell;

//use brush_core::{self as bc, Shell};
use vte;
use rustix::fd::OwnedFd;
use rustix::process::{kill_process, Pid, Signal};
//use tokio::sync::Mutex as TokioMutex;

use iced::futures::stream;
use iced::window;
use iced::{Event, Element, Task, Subscription, Length};
use iced::keyboard::{self, key, Modifiers};
use iced::widget::{container, Column, row, text::{Rich, Span}, column, text, text_input, scrollable, responsive, space};

mod colors;
mod parser;
mod styles;
mod term;
mod utils;
mod vm;

use styles::CS;

const FONT_SIZE: f32 = 15.0;

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

#[macro_export]
macro_rules! log {
    ($fmt:literal $(, $e:expr)*) => {
        if let Ok(mut guard) = crate::STDERR.lock() {
            use std::io::Write;
            if let Some(w) = guard.as_mut() {
                let _ = writeln!(w, $fmt, $($e,)*);
            }
        }
    }
}

pub type Elem<'a> = Element<'a, Message, styles::Theme, iced::Renderer>;

fn main() -> iced::Result {
    install_panic_hook();
    set_panic_output(rustix::io::dup(std::io::stderr()).unwrap());

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .run()
}

pub struct Execution {
    string: String,
    vm: vm::VM,
    term: term::Term,
    exit_reason: Option<ExitReason>,
}

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

struct App {
    master: OwnedFd,
    input: String,
    execs: Vec<Execution>,
    ansi: vte::ansi::Processor,
    theme: styles::Theme,
    //shell: Arc<TokioMutex<Shell>>,

    vwidth: cell::Cell<Option<f32>>,
}

#[derive(Clone)]
pub enum Message {
    None,
    Input(String),
    Run,
    ContinueProgram,
    ChildExited(Pid, ExitReason),
    Poll,
    Signal(Signal),
}

impl App {
    fn theme(&self) -> styles::Theme {
        self.theme
    }

    fn new() -> Self { //(Self, Task<Message>) {
        //let (_id, open) = window::open(window::Settings::default());

        let pty = rustix_openpty::openpty(None, None).unwrap();
        rustix::stdio::dup2_stdout(&pty.user).unwrap();
        rustix::stdio::dup2_stderr(&pty.user).unwrap();
        rustix::stdio::dup2_stdin(&pty.user).unwrap();

        let master_flags = rustix::fs::fcntl_getfl(&pty.controller).unwrap();
        rustix::fs::fcntl_setfl(&pty.controller, master_flags | rustix::fs::OFlags::NONBLOCK).unwrap();

        // let shell = tokio::task::block_in_place(|| {
        //     tokio::runtime::Handle::current().block_on(async {
        //         Shell::new(Default::default()).await.unwrap()
        //     })
        // });

        //(
            Self {
            input: String::new(),
            execs: Vec::new(),
            master: pty.controller,
            ansi: vte::ansi::Processor::new(),
            theme: styles::Theme::gruvbox(),
            //shell: Arc::new(TokioMutex::new(shell)),
            vwidth: cell::Cell::new(None),
        }//, open.map(|_| Message::None))
    }

    fn title(&self) -> String {
        "fatty".to_owned()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::None => { }
            Message::Input(s) => self.input = s,
            Message::Run => {
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        let program = vm::compile(&parsed);

                        let font_width = utils::measure_text(
                            "m", f32::INFINITY, FONT_SIZE, 1., term::Cell::default().iced_font()
                        ).0
                            // Sometimes there's an extra column that causes ugly wrapping
                            * 1.01;

                        let width = self.vwidth.get()
                            .map(|width| (width / font_width).floor() as usize)
                            .unwrap_or(70);

                        let string = std::mem::take(&mut self.input);
                        self.execs.push(Execution {
                            string,
                            vm: vm::VM {
                                program,
                                pc: (0, None),
                                waiting_on: None,
                                child_exit_stack: Vec::new(),
                                done: false,
                            },
                            term: term::Term::new(width),
                            exit_reason: None,
                        });

                        return self.update(Message::ContinueProgram);
                    },
                    Err(err) => {
                        println!("{err}");
                        self.poll_pty();
                    }
                }
            },
            Message::ContinueProgram => {
                if let Some(current) = self.execs.last_mut() {
                    assert!(current.vm.waiting_on == None);
                    current.vm.execute();

                    if current.vm.done {
                        current.exit_reason = current.vm.child_exit_stack.pop();
                    }

                    self.poll_pty();
                }
            },
            Message::ChildExited(pid, reason) => {
                match reason {
                    ExitReason::Normal(_) => (),
                    ExitReason::Signal { signal, .. } => print!("{}", utils::signal_to_string(signal)),
                    ExitReason::Unknown { sigval: Some(s), .. } => print!("Signal({s})"),
                    ExitReason::Unknown { sigval: None, .. } => print!("Exited (unknown)"),
                }

                match reason {
                    ExitReason::Signal { cored: true, .. }
                    | ExitReason::Unknown { cored: true, .. } => print!(" (core dumped"),
                    _ => (),
                }

                println!(""); // Newline

                if let Some(current) = self.execs.last_mut() {
                    current.vm.handle_exit(pid, reason);
                }

                return self.update(Message::ContinueProgram);
            },
            Message::Poll => {
                self.poll_pty();
            }
            Message::Signal(sig) => {
                if let Some(current) = self.execs.last() && let Some(pid) = current.vm.waiting_on {
                    kill_process(pid, sig).unwrap();
                }
            },
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let poll = if let Some(last) = self.execs.last() && !last.vm.done {
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

        let wait = match self.execs.last() {
            Some(Execution {
                vm: vm::VM {
                    waiting_on: Some(pid),
                    ..
                },
                ..
            }) => wait_on(*pid).with(*pid).map(|(pid, exited)| Message::ChildExited(pid, exited)),
            _ => Subscription::none(),
        };

        Subscription::batch([poll, keys, wait])
    }

    fn view(&self) -> Elem<'_> {
        let mut execs = Column::new()
            .spacing(1);

        for exec in &self.execs {
            execs = execs.push(
                column![
                    container(
                        row![
                            container(
                                text(&exec.string)
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
                                            .map(|c| {
                                                let ch = if c.ch == '\t' { ' ' } else { c.ch };
                                                Span::new(ch)
                                                    .color(exec.term.resolve(&self.theme, c.fg))
                                                    .background(iced::Background::Color(exec.term.resolve(&self.theme, c.bg)))
                                                    .font(c.iced_font())
                                                    .size(FONT_SIZE)
                                            })
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

        if let Some(last) = self.execs.last() && !last.vm.done {} else {
            input = input
                .on_input(Message::Input)
                .on_submit(Message::Run);
        }

        container(
            column![
                scrollable(
                    container(execs)
                        .padding(iced::Padding {
                            right: 15.,
                            ..Default::default()
                        })
                )
                    .anchor_bottom()
                    .height(Length::Fill),
                responsive(move |size| {
                    self.vwidth.set(Some(size.width));
                    space()
                        .height(1.)
                        .into()
                })
                    .height(Length::Shrink)
                    .width(Length::Fill),
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
        let i = std::time::Instant::now();
        if let Some(last) = self.execs.last_mut() {
            let mut buf = [0u8; 65535];
            match rustix::io::read(&self.master, &mut buf) {
                Ok(n) => self.ansi.advance(&mut last.term, &buf[0..n]),
                Err(rustix::io::Errno::AGAIN) => (),
                e => _ = e.unwrap(),
            }
        }
    }
}

pub fn wait_on(pid: Pid) -> Subscription<ExitReason> {
    Subscription::run_with(pid, |pid| stream::unfold(Some(*pid), |state| async move {
        let pid = state?;

        loop {
            let result = tokio::task::spawn_blocking(move || {
                rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty())
            })
            .await
            .expect("spawn_blocking panicked");

            match result {
                Ok(Some((_, status))) => {
                    let raw = status.as_raw();
                    let cored = raw & 0x80 != 0;

                    return
                        if status.signaled() && let Some(sigval) = status.terminating_signal() {
                            if let Some(signal) = Signal::from_named_raw(sigval) {
                                Some((ExitReason::Signal { signal, cored }, None))
                            } else {
                                Some((ExitReason::Unknown { sigval: Some(sigval), cored }, None))
                            }
                        } else if let Some(exit) = status.exit_status() {
                            Some((ExitReason::Normal(exit), None))
                        } else {
                            Some((ExitReason::Unknown { sigval: None, cored }, None))
                        };
                },
                Ok(None) => unreachable!(),
                Err(_) => unreachable!(),
            }
        }
    }))
}
