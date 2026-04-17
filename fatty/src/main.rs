#![allow(unused_imports)]

use std::cell;
use std::ffi::OsString;
use std::fs::DirEntry;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};

use futures::channel::mpsc::Sender;
use futures::{StreamExt, SinkExt};
use inotify;
use itertools::Itertools;
use rustix::fd::{AsFd, OwnedFd, RawFd};
use rustix::process::{kill_process, Pid, Signal};
use rustix::fs::FileType;
use tokio::sync::watch;
use vte;

use iced::futures::stream;
use iced::window;
use iced::{Event, Element, Task, Subscription, Padding, Length};
use iced::keyboard::{self, key, Modifiers};
use iced::widget::{container, Row, table::{self, Table}, Column, row, text::{Rich, Span}, column, text, text_input, responsive, space};
use iced::advanced::text::Ellipsis;

mod bolger;
mod colors;
mod helpers;
mod parser;
mod styles;
mod term;
mod utils;
mod vm;
mod widgets;

use helpers::*;
use styles::CS;
use vm::VMStatus;
use widgets::scrollable::scrollable;

const FONT_SIZE: f32 = 15.0;

static PTY_MASTER: Mutex<Option<std::fs::File>> = Mutex::new(None);
fn set_pty_output(w: &OwnedFd) {
    *PTY_MASTER.lock().unwrap() = Some(std::fs::File::from(rustix::io::dup(w).unwrap()));
}

#[macro_export]
macro_rules! outln {
    ($fmt:literal $(, $e:expr)*) => {
        if let Ok(mut guard) = crate::PTY_MASTER.lock() {
            use std::io::Write;
            if let Some(w) = guard.as_mut() {
                let _ = writeln!(w, $fmt, $($e,)*);
            }
        }
    }
}

#[macro_export]
macro_rules! out {
    ($fmt:literal $(, $e:expr)*) => {
        if let Ok(mut guard) = crate::PTY_MASTER.lock() {
            use std::io::Write;
            if let Some(w) = guard.as_mut() {
                let _ = write!(w, $fmt, $($e,)*);
            }
        }
    }
}

pub type Elem<'a> = Element<'a, Message, styles::Theme, iced::Renderer>;

fn main() -> iced::Result {
    iced::application(move || App::new(), App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .run()
}

pub struct Execution {
    string: String,
    vm: vm::VM,
    term: term::Term,
    output: String,
    document: bolger::ui::Document,
    exit_reason: Option<ExitReason>,

    b_err: bool, // Is the output corrupted permanently
    p_stack: usize,
    q_flag: bool,
}

impl Execution {
    pub fn new(string: String, vm: vm::VM, width: usize) -> Execution {
        Execution {
            string, vm,
            term: term::Term::new(width),
            output: "".to_owned(),
            document: bolger::ui::Document::new(),
            exit_reason: None,
            b_err: false,
            p_stack: 0,
            q_flag: false,
        }
    }
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

#[derive(Clone, Debug)]
pub enum Message {
    None,
    Animate,
    Input(String),
    Run,
    ContinueProgram,
    ChildExited(Pid, ExitReason),
    JobResolved(usize, usize),
    Poll,
    Signal(Signal),
    Inotify(OsString),
}

struct App {
    master: OwnedFd,
    slave: OwnedFd,
    input: String,
    execs: Vec<Execution>,
    ansi: vte::ansi::Processor,
    theme: styles::Theme,

    listing: Vec<MyDirEntry>,
    listing_last_changed: Option<(OsString, Instant)>,

    vwidth: cell::Cell<Option<f32>>,
}

impl Drop for App {
    fn drop(&mut self) {
        // Explicitely drop slave, so that master drops only after slave
        let slave = std::mem::replace(&mut self.slave, unsafe { rustix::stdio::take_stdin() }); // hack
        std::mem::drop(slave);
    }
}

impl App {
    fn theme(&self) -> styles::Theme {
        self.theme
    }

    fn new() -> (Self, Task<Message>) {
        //let (_id, open) = window::open(window::Settings::default());

        // let shell = tokio::task::block_in_place(|| {
        //     tokio::runtime::Handle::current().block_on(async {
        //         Shell::new(Default::default()).await.unwrap()
        //     })
        // });

        let pty = rustix_openpty::openpty(None, None).unwrap();
        let master_flags = rustix::fs::fcntl_getfl(&pty.controller).unwrap();
        rustix::fs::fcntl_setfl(
            &pty.controller,
            master_flags | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC
        ).unwrap();
        set_pty_output(&pty.controller);

        (
            Self {
                input: String::new(),
                listing: listing(),
                listing_last_changed: None,
                execs: Vec::new(),
                master: pty.controller,
                slave: pty.user,
                ansi: vte::ansi::Processor::new(),
                theme: styles::Theme::gruvbox(),
                //shell: Arc::new(TokioMutex::new(shell)),
                vwidth: cell::Cell::new(None),
            },
            iced::font::set_defaults(iced::Font::new("Atkinson Hyperlegible Next"), 16.),
            //open.map(|_| Message::None)
        )
    }

    fn title(&self) -> String {
        "fatty".to_owned()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::None => { }
            Message::Animate => { }
            Message::Input(s) => self.input = s,
            Message::Run => {
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        let program = vm::compile(&parsed);
                        //vm::print_program(&program);

                        let (font_width, font_height) = utils::measure_text(
                            "m", f32::INFINITY, FONT_SIZE, 1., term::Cell::default().iced_font()
                        );

                        // Sometimes there's an extra column that causes ugly wrapping
                        let font_width = font_width * 1.01;

                        let width = self.vwidth.get()
                            .map(|width| (width / font_width).floor() as u16)
                            .unwrap_or(70);

                        rustix::termios::tcsetwinsize(
                            &self.master,
                            rustix::termios::Winsize {
                                ws_row: 100,
                                ws_col: width,
                                ws_xpixel: font_width as u16,
                                ws_ypixel: font_height as u16,
                            }
                        ).unwrap();

                        let string = std::mem::take(&mut self.input);
                        self.execs.push(Execution::new(
                            string,
                            vm::VM {
                                program: Arc::new(program),
                                pc: (0, None),
                                waiting_on: None,
                                child_exit_stack: Vec::new(),
                                jobs: Vec::new(),
                                status: None,
                                done: false,
                            },
                            width as usize,
                        ));

                        return self.update(Message::ContinueProgram);
                    },
                    Err(err) => {
                        outln!("{err}");
                        self.poll_pty();
                    }
                }
            },
            Message::ContinueProgram => {
                if let Some(current) = self.execs.last_mut() {
                    assert!(current.vm.waiting_on == None);
                    loop {
                        current.vm.execute(Some(self.slave.as_fd()));

                        if current.vm.done {
                            current.exit_reason = current.vm.child_exit_stack.pop();
                            break;
                        } else if current.vm.waiting_on.is_some() {
                            break;
                        }
                    }
                    self.poll_pty();
                }
            },
            Message::ChildExited(pid, reason) => {
                match reason {
                    ExitReason::Normal(_) => (),
                    ExitReason::Signal { signal, .. } => out!("{}", utils::signal_to_string(signal)),
                    ExitReason::Unknown { sigval: Some(s), .. } => out!("Signal({s})"),
                    ExitReason::Unknown { sigval: None, .. } => out!("Exited (unknown)"),
                }

                match reason {
                    ExitReason::Signal { cored: true, .. }
                    | ExitReason::Unknown { cored: true, .. } => out!(" (core dumped)"),
                    _ => (),
                }

                match reason {
                    ExitReason::Normal(_) => (),
                    _ => outln!(""), // Newline
                }

                if let Some(current) = self.execs.last_mut() {
                    current.vm.handle_exit(pid, reason);
                }

                return self.update(Message::ContinueProgram);
            },
            Message::JobResolved(_exec_ind, _job_ind) => {
                // todo
            },
            Message::Poll => self.poll_pty(),
            Message::Signal(sig) => {
                if let Some(current) = self.execs.last() && let Some(pid) = current.vm.waiting_on {
                    kill_process(pid, sig).unwrap();
                }
            },
            Message::Inotify(item) => {
                self.listing_last_changed = Some((item, Instant::now()));
                self.listing = listing();
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

        let jobs = watch_jobs(self);

        let fswatch = if let Ok(path) = std::env::current_dir() {
            fswatch(path)
                .map(Message::Inotify)
        } else {
            Subscription::none()
        };

        let is_animating = match &self.listing_last_changed {
            Some((_, at)) if at.elapsed().as_millis() < 300 => true,
            _ => false,
        };

        let animation = if is_animating {
            window::frames().map(|_| Message::Animate)
        } else {
            Subscription::none()
        };

        Subscription::batch([poll, keys, wait, jobs, fswatch, animation])
    }

    fn view(&self) -> Elem<'_> {
        let exit_reason = |reason| {
            match reason {
                None => text("Running"),
                Some(ExitReason::Normal(code)) => text(code.to_string()),
                Some(ExitReason::Signal { signal, .. }) => text(utils::signal_to_string(signal)),
                Some(ExitReason::Unknown { sigval: Some(s), .. }) => text(format!("Signal({s})")),
                Some(ExitReason::Unknown { sigval: None, .. }) => text("Exited (unknown)"),
            }
        };

        let listing = Table::new(
            [
                table::column(thead("mode"), |d: &MyDirEntry| {
                    utils::Mode(d.mode).to_iced()
                }),
                // table::column(thead("user"), |d: &DirEntry| {
                //     text(d.uname.unwrap_or("?".to_string()))
                //         .ellipsis(Ellipsis::End)
                // }),
                table::column(thead("size"), |d: &MyDirEntry| {
                    let e: Elem<'_> = if d.kind == FileType::Directory {
                        space().into()
                    } else {
                        mono(utils::fmt_size(d.size))
                            .into()
                    };
                    e
                }),
                table::column(thead("name"), |d: &MyDirEntry| {
                    let class = match &self.listing_last_changed {
                        Some((name, at)) if *name == d.raw_name && at.elapsed().as_millis() < 300 => {
                            let f = (at.elapsed().as_millis() as f32 / 300.).powf(4.);
                            CS::FadingHighlight(1. - f)
                        },
                        _ => CS::Base,
                    };
                    container(
                        text(d.name.clone())
                            .width(Length::Fill)
                    )
                        .padding(Padding { left: 2., right: 1., ..Default::default() })
                        .class(class)
                })
                    .width(Length::Fill)
            ],
            &self.listing,
        )
            .width(Length::Fill)
            .padding_y(1);

        let mut jobs = Column::new()
            .spacing(1);

        for exec in &self.execs {
            for job in &exec.vm.jobs {
                jobs = jobs.push(
                    container({
                        let e: Elem<'_> = match &*job.status.borrow() {
                            VMStatus::Waiting { command, pid }
                                => text(format!("{} ({})", command.to_string(), pid)).into(),
                            VMStatus::Done { command, reason }
                            | VMStatus::Resolved { command: Some(command), reason: Some(reason) }
                                => row![
                                    text(command.to_string())
                                        .width(Length::Fill),
                                    exit_reason(Some(*reason)),
                                ].into(),
                            VMStatus::None
                            | VMStatus::Resolved { .. }
                                => text("...").into(),
                        };
                        e
                    })
                        .class(CS::Box)
                        .padding(3)
                        .width(Length::Fill)
                );
            }
        }

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
                            exit_reason(exec.exit_reason),
                        ],
                    )
                        .class(CS::Box)
                        .padding(3),
                    if !exec.document.elements.is_empty() {
                        let e: Elem<'_> = space().into();
                        e
                    } else {
                        container(
                            widgets::tty::Tty::new(
                                &exec.term,
                                &self.theme,
                                FONT_SIZE,
                                |term, theme, color| term.resolve(theme, color),
                            )
                        )
                            .padding(1)
                            .width(Length::Fill)
                            .into()
                    },
                    scrollable(
                        if !exec.document.elements.is_empty() {
                            let mut uis = Column::new()
                                .spacing(0);
                            let mut spans = Vec::new();

                            for uielem in &exec.document.elements {
                                if uielem.is_block() {
                                    uis = uis.push(
                                        Rich::with_spans(std::mem::take(&mut spans))
                                            .on_link_click(iced::never)
                                    );
                                    uis = uis.push(uielem.to_iced(Default::default(), &exec.document.ids));
                                } else {
                                    spans.push(uielem.to_iced_span(Default::default(), &exec.document.ids));
                                }
                            }

                            let e: Elem<'_> = uis.into();
                            e
                        } else {
                            space().into()
                        }
                    ),
                    text({
                        let mut s = format!("p_stack: {}; ", exec.p_stack);
                        if exec.q_flag {
                            s = format!("{s}waiting for quote; ");
                        }
                        if exec.b_err {
                            s = format!("{s}corrupted output");
                        }
                        s
                    }),
                ],
            );
        }

        let mut input = text_input("rm -rf /", &self.input);

        if let Some(last) = self.execs.last() && !last.vm.done {
            // Input disabled.
        } else {
            input = input
                .on_input(Message::Input)
                .on_submit(Message::Run);
        }

        container(
            row![
                column![
                    responsive(move |size| {
                        self.vwidth.set(Some(size.width));
                        space()
                            .height(1.)
                            .into()
                    })
                        .height(Length::Shrink)
                        .width(Length::Fill),
                    column![
                        scrollable(
                            container(execs)
                                .padding(Padding {
                                    right: 15.,
                                    ..Default::default()
                                })
                        )
                            .anchor_bottom()
                            .height(Length::Fill),
                        input,
                    ]
                        .spacing(4.)
                ]
                    .width(Length::FillPortion(2)),
                column![
                    scrollable(
                        container(listing)
                            .padding(Padding {
                                bottom: 5.,
                                top: 5.,
                                right: 15.,
                                left: 5.,
                            })
                            .width(Length::Fill)
                            .class(CS::WhiteBox)
                    )
                        .height(Length::FillPortion(2))
                        .width(Length::Fill),
                    scrollable(
                        container(jobs)
                            .padding(Padding {
                                bottom: 5.,
                                top: 5.,
                                right: 15.,
                                left: 5.,
                            })
                            .width(Length::Fill)
                            .class(CS::WhiteBox)
                    )
                        .height(Length::FillPortion(1))
                        .width(Length::Fill)
                        .anchor_bottom()
                ]
                    .spacing(4)
                    .width(Length::FillPortion(1)),
            ]
                .spacing(4)
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
            loop {
                match rustix::io::read(&self.master, &mut buf) {
                    Ok(n) => {
                        self.ansi.advance(&mut last.term, &buf[0..n]);

                        let mut buf_last = 0;

                        if !last.b_err {
                            for ind in memchr::memchr3_iter(b'(', b')', b'"', &buf[0..n]) {
                                match buf[ind] {
                                    b'(' => last.p_stack += 1,
                                    b')' if last.p_stack == 0 => last.b_err = true,
                                    b')' => last.p_stack -= 1,
                                    b'"' => last.q_flag = !last.q_flag,
                                    _ => unreachable!(),
                                }

                                if last.p_stack == 0 && !last.q_flag {
                                    last.output.push_str(&String::from_utf8_lossy(&buf[buf_last..ind + 1]));
                                    buf_last = ind + 1;

                                    match bolger::parser::parse(&last.output) {
                                        Ok(ast) => {
                                            last.output.clear();
                                            last.document.consume_nodes(&ast).unwrap();
                                        },
                                        Err(e) => println!("{e:?}"),
                                    }
                                }
                            }
                        }

                        last.output.push_str(&String::from_utf8_lossy(&buf[buf_last..n]));
                    },
                    Err(rustix::io::Errno::AGAIN) => return,
                    e => _ = e.unwrap(),
                }
            }
        }
    }
}

pub fn fswatch(path: PathBuf) -> Subscription<OsString> {
    use inotify::WatchMask as W;

    Subscription::run_with(path, |path| {
        let path = path.clone();
        stream::unfold(Some(path), |path| async move {
            let fl = W::ATTRIB | W::CLOSE_WRITE | W::CREATE | W::DELETE | W::EXCL_UNLINK;

            let path: PathBuf = path?;
            let inot = inotify::Inotify::init().unwrap();
            let wd = inot.watches().add(&path, fl).unwrap();

            let mut buf = [0; 2048];
            let mut stream = inot.into_event_stream(&mut buf)
                .expect("Error converting to stream");

            while let Some(event_or_err) = stream.next().await {
                if let Ok(ev) = event_or_err {
                    stream.watches().remove(wd).unwrap();
                    return Some((ev.name.unwrap(), Some(path)));
                }
            }
            stream.watches().remove(wd).unwrap();
            None
        })
    })
}

pub fn wait_on(pid: Pid) -> Subscription<ExitReason> {
    Subscription::run_with(pid, |pid| stream::unfold(Some(*pid), |state| async move {
        let pid = state?;

        loop {
            let result = tokio::task::spawn_blocking(move || {
                rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG)
            })
            .await
            .expect("spawn_blocking panicked");

            match result {
                Ok(Some((_, status))) => return Some((ExitReason::from(status), None)),
                Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(_) => unreachable!(),
            }
        }
    }))
}

fn watch_jobs(app: &App) -> Subscription<Message> {
    #[derive(Clone)]
    struct Watching {
        exec_ind: usize,
        job_ind: usize,
        receiver: watch::Receiver<VMStatus>,
    }

    impl Hash for Watching {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.exec_ind.hash(state);
            self.job_ind.hash(state);
        }
    }

    // Collect all (exec_index, job_index, receiver) for jobs that are NOT yet resolved
    let receivers: Vec<Watching> = app
        .execs
        .iter()
        .enumerate()
        .flat_map(|(exec_ind, exec)| {
            exec.vm
                .jobs
                .iter()
                .enumerate()
                .filter(|(_, job)| !job.is_done())
                .map(move |(job_ind, job)| Watching { exec_ind, job_ind, receiver: job.status.clone() })
                .collect::<Vec<_>>()
        })
        .collect();

    if receivers.is_empty() {
        return Subscription::none();
    }

    Subscription::run_with(
        receivers,
        |receivers| {
            let receivers = receivers.clone();
            iced::stream::channel(16, move |mut output: Sender<Message>| async move {
                loop {
                    let futures: Vec<_> = receivers
                        .iter()
                        .map(|Watching { exec_ind, job_ind, receiver: rx }| {
                            let mut rx = rx.clone();
                            let exec_idx = *exec_ind;
                            let job_idx = *job_ind;
                            Box::pin(async move {
                                // Loop until the job resolves
                                loop {
                                    rx.changed().await.ok();
                                    if matches!(&*rx.borrow(), VMStatus::Resolved { .. }) {
                                        return (exec_idx, job_idx);
                                    }
                                }
                            })
                        })
                        .collect();

                    let ((exec_index, job_index), _, _) = futures::future::select_all(futures).await;

                    output
                        .send(Message::JobResolved(exec_index, job_index))
                        .await
                        .ok();
                }
            })
        }
    )
}

struct MyDirEntry {
    kind: FileType,
    mode: u32,
    size: u64,
    raw_name: OsString,
    name: String,
    uname: Option<String>,
}

fn listing() -> Vec<MyDirEntry> {
    let mut entries = std::fs::read_dir(".")
        .unwrap()
        .map(|res| {
            let d = res.unwrap();
            let met = d.metadata().unwrap();

            let mode = met.permissions().mode();
            let kind = FileType::from_raw_mode(mode);
            let uname = unsafe {
                let r = libc::getpwuid(met.uid());
                (r.is_null()).then(||
                    std::ffi::CStr::from_ptr((*r).pw_name)
                        .to_string_lossy()
                        .to_string()
                )
            };
            let size = met.len();

            let raw_name = d.file_name();
            let mut name = raw_name.to_string_lossy().to_string();
            if met.is_dir() {
                name.push('/');
            }

            MyDirEntry { mode, kind, uname, size, raw_name, name }
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|i| i.name.clone());
    entries
}
