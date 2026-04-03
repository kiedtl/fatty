use std::sync::Mutex;
use std::process::{Command, Child};
use std::time::Duration;
use std::io::{Read, Write};

use vte;
use rustix::fd::OwnedFd;

use iced::window;
use iced::{Element, Task, Subscription, Length};
use iced::widget::{container, Column, row, text::{Rich, Span}, column, text, text_input};

mod term;
use term::Cell;

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
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (_id, open) = window::open(window::Settings::default());

        let pty = rustix_openpty::openpty(None, None).unwrap();
        rustix::stdio::dup2_stdout(&pty.user).unwrap();
        rustix::stdio::dup2_stderr(&pty.user).unwrap();
        rustix::stdio::dup2_stdin(&pty.user).unwrap();

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
            Message::Poll => {
                if let Some(last) = self.execs.last_mut() {
                    let mut buf = [0u8; 4096];
                    let n = rustix::io::read(&self.master, &mut buf).unwrap();
                    self.ansi.advance(&mut last.term, &buf[0..n]);

                    if let Ok(Some(status)) = last.child.try_wait() {
                        last.done = true;
                        last.exit = status.code();
                    }
                }
            },
            Message::Run => {
                let mut args = self.input.split(" ");
                let Some(command) = args.next() else { return Task::none(); };

                let child = Command::new(command)
                    .args(args)
                    .spawn()
                    .unwrap();

                let command = std::mem::take(&mut self.input);
                self.execs.push(Execution {
                    command,
                    child,
                    term: term::Term::new(),
                    done: false,
                    exit: None,
                });
            },
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        if let Some(last) = self.execs.last() && !last.done {
            iced::time::every(Duration::from_millis(30)).map(|_| Message::Poll)
        } else {
            Subscription::none()
        }
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

        container(
            column![
                container(execs),
                text_input("...", &self.input)
                    .on_input(Message::Input)
                    .on_submit(Message::Run),
            ]
                .spacing(2)
        )
            .padding(3)
            .into()
    }
}
