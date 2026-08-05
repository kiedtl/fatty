#![allow(unused_imports)]

use std::borrow::Cow;
use std::cell;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::DirEntry;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};

use futures::stream::BoxStream;
use futures::channel::mpsc::Sender;
use futures::{StreamExt, SinkExt};
use inotify;
use itertools::Itertools;
use rustix::fd::{AsFd, OwnedFd, RawFd, AsRawFd};
use rustix::process::{kill_process, Pid, Signal};
use rustix::fs::FileType;
use tokio::sync::watch;
use vte;

use iced::futures::stream;
use iced::window;
use iced::{Event, Element, Task, Subscription, Padding, Length};
use iced::keyboard::{self, key, Modifiers};
use iced::widget::{
    column,
    container,
    grid,
    responsive,
    row,
    space,
    table::{self, Table},
    text,
    text_input,
    text::{Rich, Span},
    Column,
    Row,
};
use iced::advanced::text::Ellipsis;

mod bolger;
mod colors;
mod bwine_ui;
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
use parser::LineCol;
use widgets::scrollable::scrollable;
use widgets::input::input;
use widgets::controller;

const FONT_SIZE: f32 = 15.0;

static PTY_MASTER: Mutex<Option<std::fs::File>> = Mutex::new(None);
fn set_pty_output(w: &OwnedFd) {
    *PTY_MASTER.lock().unwrap() = Some(std::fs::File::from(rustix::io::dup(w).unwrap()));
}

#[macro_export]
macro_rules! outln {
    ($fmt:literal $(, $e:expr)*) => { {
        if let Ok(mut guard) = crate::PTY_MASTER.lock() {
            use std::io::Write;
            if let Some(w) = guard.as_mut() {
                let _ = writeln!(w, $fmt, $($e,)*);
            }
        }
    } }
}

#[macro_export]
macro_rules! out {
    ($fmt:literal $(, $e:expr)*) => { {
        if let Ok(mut guard) = crate::PTY_MASTER.lock() {
            use std::io::Write;
            if let Some(w) = guard.as_mut() {
                let _ = write!(w, $fmt, $($e,)*);
            }
        }
    } }
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
    fd3_master: Arc<OwnedFd>,
    fd3_slave: OwnedFd,

    cmdline: String,
    vm: vm::VM,
    exit_reason: Option<ExitReason>,
    is_drained: bool,
    fd3_is_drained: bool,

    term: term::Term,
    ansi: vte::ansi::Processor,

    output: String,
    outputb: Vec<u8>,

    document: bolger::ui::Document,

    bwine_ast: Vec<bwine::Token<'static>>,
    bwine_sr: bwine::StreamingReader,
    bwine_object: Option<bwine::Value<'static>>,
    bwine_extra: bool,
    bwine_error: bool,
    bwine_consumed: usize,

    b_err: bool, // Is the output corrupted permanently
    p_stack: usize,
    q_flag: bool,
}

impl Execution {
    pub fn new(cmdline: String, vm: vm::VM, width: usize) -> Execution {
        //let (fd3_master, fd3_slave) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::NONBLOCK).unwrap();
        // Unix socket for bidi communication.. bad idea?
        let (fd3_master, fd3_slave) = rustix::net::socketpair(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            rustix::net::SocketFlags::NONBLOCK,
            None,
        ).unwrap();
        Execution {
            fd3_master: Arc::new(fd3_master),
            fd3_slave,
            cmdline, vm,
            term: term::Term::new(width),
            ansi: vte::ansi::Processor::new(),
            output: "".to_owned(),
            outputb: Vec::new(),
            bwine_ast: Vec::new(),
            bwine_sr: bwine::StreamingReader::new(),
            bwine_object: None,
            bwine_extra: false,
            bwine_error: false,
            bwine_consumed: 0,
            document: bolger::ui::Document::new(),
            exit_reason: None,
            is_drained: false,
            fd3_is_drained: false,
            b_err: false,
            p_stack: 0,
            q_flag: false,
        }
    }

    pub fn cleanup(&mut self) {
        // unsafe {
        //     rustix::io::close(self.fd3_master.as_raw_fd());
        //     rustix::io::close(self.fd3_slave.as_raw_fd());
        // }
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControlState {
    mode: ControlMode,
    history_cursor: Option<usize>,
}

impl ControlState {
    pub fn new() -> Self {
        Self {
            mode: ControlMode::Normal,
            history_cursor: None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ControlMode {
    Normal,
    Insert,
    Term,
}

#[derive(Clone, Debug)]
pub enum ControlMessage {
    ChangeMode(ControlMode),
    HistoryUp,
    HistoryDown,
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
    Pty(usize, bool, Box<[u8]>),
    PtyInput(usize, Cow<'static, [u8]>),
    PtyDrained(usize, bool),
    Signal(Signal),
    Inotify(OsString),
    Controller(ControlMessage),
}

struct App {
    master: Arc<OwnedFd>,
    slave: OwnedFd,
    control: ControlState,
    input: String,
    input_compile_error: Option<LineCol>,
    execs: Vec<Execution>,
    theme: styles::Theme,
    env: HashMap<OsString, OsString>,

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
    fn get_env<'a>(&'a self, s: &str, fallback: &'a str) -> &'a OsStr {
        self.env.get(OsStr::new(s)).map_or(OsStr::new(fallback), |v| v)
    }

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

        let mut env: HashMap<_, _> = std::env::vars_os().collect();
        env.insert("FATTY".into(), "normal0".into());

        (
            Self {
                control: ControlState::new(),
                input: String::new(),
                input_compile_error: None,
                listing: listing(),
                listing_last_changed: None,
                execs: Vec::new(),
                master: Arc::new(pty.controller),
                slave: pty.user,
                theme: styles::Theme::gruvbox(),
                //shell: Arc::new(TokioMutex::new(shell)),
                vwidth: cell::Cell::new(None),
                env,
            },
            iced::font::set_defaults(iced::Font::new("Atkinson Hyperlegible Next"), 16.),
        )
    }

    fn title(&self) -> String {
        "fatty".to_owned()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::None => { }
            Message::Animate => { }
            Message::Input(s) => {
                self.input = s;
                self.input_compile_error = None;
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        let path_var = self.get_env("PATH", "");
                        match vm::compile(path_var, &parsed) {
                            Ok(_) => (),
                            Err(vm::CompileError::CommandNotFound(lc, s)) => {
                                self.input_compile_error = Some(lc);
                            }
                        }
                    }
                    Err(_) => (),
                }
            },
            Message::Run => {
                self.control.history_cursor = None;
                self.control.mode = ControlMode::Term;
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        let path_var = self.get_env("PATH", "");
                        let program = match vm::compile(path_var, &parsed) {
                            Ok(p) => p,
                            Err(_) => return Task::none(),
                        };
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

                        let cmdline = std::mem::take(&mut self.input);
                        self.execs.push(Execution::new(
                            cmdline,
                            vm::VM {
                                env: Arc::new(self.env.clone()),
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
                    }
                }
            },
            Message::ContinueProgram => {
                if let Some(current) = self.execs.last_mut() {
                    assert!(current.vm.waiting_on == None);
                    loop {
                        current.vm.execute(Some(self.slave.as_fd()), Some(current.fd3_slave.as_fd()));

                        if current.vm.done {
                            current.exit_reason = current.vm.child_exit_stack.pop();
                            current.cleanup();
                            if self.control.mode == ControlMode::Term {
                                if !self.execs.iter().any(|e| !e.vm.done) {
                                    self.control.mode = ControlMode::Insert;
                                }
                            }
                            break;
                        } else if current.vm.waiting_on.is_some() {
                            break;
                        }
                    }
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
            Message::Pty(exec_ind, is_fd3, buf) => {
                let Some(exec) = self.execs.get_mut(exec_ind) else { return Task::none() };
                if !is_fd3 {
                    exec.ansi.advance(&mut exec.term, &buf);

                    if !exec.b_err {
                        let mut buf_last = 0;
                        for ind in 0..buf.len() {
                            match buf[ind] {
                                b'(' => exec.p_stack += 1,
                                b')' if exec.p_stack == 0 => exec.b_err = true,
                                b')' => exec.p_stack -= 1,
                                b'"' => exec.q_flag = !exec.q_flag,
                                _ => continue,
                            }

                            if !exec.b_err && exec.p_stack == 0 && !exec.q_flag {
                                exec.output.push_str(&String::from_utf8_lossy(&buf[buf_last..ind + 1]));
                                buf_last = ind + 1;

                                match bolger::parser::parse(&exec.output) {
                                    Ok(ast) => {
                                        exec.output.clear();
                                        exec.document.consume_nodes(&ast).unwrap();
                                    },
                                    Err(e) => {
                                        println!("bolger: {e:?}");
                                    }
                                }
                            }
                        }

                        exec.output.push_str(&String::from_utf8_lossy(&buf[buf_last..]));
                    }
                } else {
                    exec.outputb.extend(&buf);

                    if !exec.bwine_error && !exec.bwine_sr.is_done() {
                        while !exec.bwine_sr.is_done() {
                            match exec.bwine_sr.read_once(&exec.outputb[exec.bwine_consumed..], &mut exec.bwine_ast) {
                                Ok(nn) => exec.bwine_consumed += nn,
                                Err(e) if e.is_end_of_input() => break,
                                Err(e) => {
                                    exec.bwine_object = None;
                                    exec.bwine_error = true;
                                    println!("bwine: {e:?}");
                                },
                            }

                            if let Some((v, object)) = bwine::Token::collect(&exec.bwine_ast) {
                                assert!(exec.bwine_sr.is_done());
                                exec.bwine_object = Some(object);
                                exec.bwine_extra = v < exec.bwine_ast.len();
                            }

                            exec.outputb.drain(..exec.bwine_consumed);
                            exec.bwine_consumed = 0;
                        }
                    } else {
                        exec.bwine_extra = true;
                    }
                }
            },
            Message::PtyInput(_exec_ind, bytes) => {
                let mut consumed = 0;
                while consumed < bytes.len() {
                    match rustix::io::write(&self.master, &bytes[consumed..]) {
                        Ok(n) => consumed += n,
                        Err(rustix::io::Errno::AGAIN) => (),
                        Err(e) => Err(e).unwrap(),
                    }
                }
            },
            Message::PtyDrained(exec_ind, is_fd3) => {
                let Some(exec) = self.execs.get_mut(exec_ind) else { return Task::none() };
                if exec.vm.done {
                    if is_fd3 {
                        exec.fd3_is_drained = true;
                    } else {
                        exec.is_drained = true;
                    }
                }
            },
            Message::Signal(sig) => {
                if let Some(current) = self.execs.last() && let Some(pid) = current.vm.waiting_on {
                    kill_process(pid, sig).unwrap();
                }
            },
            Message::Inotify(item) => {
                self.listing_last_changed = Some((item, Instant::now()));
                self.listing = listing();
            },
            Message::Controller(message) => {
                return controller::update(self, message);
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut polling_subs = self.execs.iter()
            .enumerate()
            .map(|(exec_ind, exec)| [
                (self.master.clone(), exec_ind, false, exec.is_drained),
                (exec.fd3_master.clone(), exec_ind, true, exec.fd3_is_drained),
            ])
            .flatten()
            .filter(|(_, _, _, is_drained)| !is_drained)
            .map(|(fd, exec_ind, is_fd3, _)| readpty(fd, exec_ind, is_fd3))
            .collect::<Vec<_>>();
        let polls = if polling_subs.len() == 1 {
            polling_subs.remove(0)
        } else {
            Subscription::batch(polling_subs)
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

        Subscription::batch([polls, keys, wait, jobs, fswatch, animation])
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

        fn listing_item_class(app: &App, d: &MyDirEntry) -> CS {
            match &app.listing_last_changed {
                Some((name, at)) if *name == d.raw_name && at.elapsed().as_millis() < 300 => {
                    let f = (at.elapsed().as_millis() as f32 / 300.).powf(4.);
                    CS::FadingHighlight(1. - f)
                },
                _ => CS::Base,
            }
        }

        fn listing_table<'a>(app: &'a App) -> Elem<'a> {
            Table::new(
                [
                    table::column(/* thead("mode") */space(), |d: &MyDirEntry| {
                        utils::Mode(d.mode).to_iced()
                    }),
                    table::column(/* thead("user") */space(), |d: &MyDirEntry| {
                        text(d.uname.as_ref().map(|s| s.as_str()).unwrap_or("?"))
                            .ellipsis(Ellipsis::End)
                    }),
                    table::column(/* thead("size") */space(), |d: &MyDirEntry| {
                        let e: Elem<'_> = if d.kind == FileType::Directory {
                            space().into()
                        } else {
                            mono(utils::fmt_size(d.size))
                                .into()
                        };
                        e
                    }),
                    table::column(/* thead("name") */space(), |d: &MyDirEntry| {
                        container(
                            text(d.name.clone())
                                .width(Length::Fill)
                        )
                            .padding(Padding { left: 2., right: 1., ..Default::default() })
                            .class(listing_item_class(app, d))
                    })
                        .width(Length::Fill)
                ],
                &app.listing,
            )
                .width(Length::Fill)
                .separator_x(0)
                .separator_y(0)
                .padding_y(0)
                .padding_x(5)
                .into()
        }

        fn listing_grid<'a>(app: &'a App) -> Elem<'a> {
            responsive(move |sz| {
                let widths = app.listing.iter()
                    .map(|d| utils::measure_text(
                            &d.name, std::f32::INFINITY,
                            14., 1., iced::font::Font::default()
                    ).0)
                    .collect::<Vec<_>>();
                let max = widths.iter().copied().fold(0., f32::max) * 1.2;
                let cell_width = if max > sz.width * 0.12 {
                    let mut widths = widths;
                    widths.sort_by(|a, b| a.total_cmp(b));
                    widths[widths.len() / 3 * 2]
                } else {
                    max
                };

                let all_same_uname = app.listing.len() > 0
                    && app.listing.iter().skip(1).all(|it| app.listing[0].uname == it.uname);

                grid(app.listing.iter().map(|d: &MyDirEntry| {
                    container(mycolumn![
                        container(
                            text(d.name.clone())
                                .ellipsis(Ellipsis::End)
                        )
                            .width(Length::Shrink)
                            .class(listing_item_class(app, d)),
                        mono(
                            if d.kind == FileType::Directory {
                                "".to_string()
                            } else {
                                utils::fmt_size(d.size)
                            }
                        ),
                        if !all_same_uname =>
                            text(d.uname.as_ref().map(|s| s.as_str()).unwrap_or("?"))
                                .ellipsis(Ellipsis::End),
                        utils::Mode(d.mode).to_iced()
                    ])
                        .clip(true)
                        .class(CS::GrayBox)
                        .padding(2)
                        .into()
                }))
                    .spacing(2)
                    .height(Length::Shrink)
                    .fluid(cell_width)
                    .into()
            })
                .width(Length::Fill)
                .height(Length::Shrink)
                .into()
        }

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

        for (i, exec) in self.execs.iter().enumerate() {
            execs = execs.push(
                column![
                    container(
                        row![
                            container(
                                text(&exec.cmdline)
                            )
                                .width(Length::Fill),
                            exit_reason(exec.exit_reason),
                        ],
                    )
                        .class(CS::Box)
                        .padding(3),
                    if !exec.document.elements.is_empty() {
                        let e: Elem<'_> = scrollable({
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
                        }).into();
                        e
                    } else if let Some(object) = &exec.bwine_object {
                        scrollable(bwine_ui::to_iced(object)).into()
                    } else {
                        container(
                            widgets::tty::Tty::new(
                                &exec.term,
                                &self.theme,
                                FONT_SIZE,
                                |term, theme, color| term.resolve(theme, color),
                                move |c| Message::PtyInput(i, c),
                                i == self.execs.len() - 1 && !exec.vm.done
                            )
                        )
                            .padding(1)
                            .width(Length::Fill)
                            .into()
                    },
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

        let mut input = input(self.control.mode, "rm -rf /", &self.input);

        if let Some(LineCol(_, s, e)) = self.input_compile_error {
            input.add_annotation(s, e);
        }

        if let Some(last) = self.execs.last() && !last.vm.done {
            // Input disabled.
        } else {
            input = input
                .on_input(Message::Input)
                .on_submit(Message::Run);
        }

        controller::controller(
            &self.control,
            Message::Controller,
            container(
                column![
                    column![
                        scrollable(
                            container(listing_grid(self))
                                .padding(Padding {
                                    bottom: 5.,
                                    top: 5.,
                                    right: 15.,
                                    left: 5.,
                                })
                                .width(Length::Fill)
                                .class(CS::WhiteBox)
                        )
                            .height(Length::FillPortion(3))
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
                        .height(Length::FillPortion(1)),
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
                        .height(Length::FillPortion(3)),
                    container(
                        row![
                            mono(self.get_env("USER", "?").to_string_lossy()),
                            text("@"),
                            mono(rustix::system::uname().nodename().to_string_lossy().to_string()),
                            text(" on "),
                            mono(
                                if let Ok(cwd) = std::env::current_dir() {
                                    let mut p = cwd.display().to_string();
                                    if let Some(home) = self.env.get(OsStr::new("HOME")) {
                                        let home = home.to_string_lossy().to_string();
                                        p = p.replace(&home, "~");
                                    }
                                    p
                                } else {
                                    "No known CWD!".to_string()
                                }
                            )
                        ]
                    )
                ]
                    .spacing(4)
            )
                .padding(5)
                .width(Length::Fill)
                .height(Length::Fill)
                .class(CS::Outer)
        )
            .into()
    }
}

fn readpty(master: Arc<OwnedFd>, exec_ind: usize, is_fd3: bool) -> Subscription<Message> {
    iced::advanced::subscription::from_recipe(utils::Runner {
        id: (exec_ind, is_fd3, master.as_raw_fd()),
        spawn: move |_| -> BoxStream<'static, Message> {
            Box::pin(stream::unfold((master, exec_ind, is_fd3), |(master, exec_ind, is_fd3)| async move {
                let mut buf = [0u8; 8192];
                let mut tries = 0;
                loop {
                    match rustix::io::read(&*master, &mut buf) {
                        Err(rustix::io::Errno::AGAIN) => {
                            tries += 1;
                            if tries > 10 {
                                return Some((
                                    Message::PtyDrained(exec_ind, is_fd3),
                                    (master, exec_ind, is_fd3)
                                ));
                            } else {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        }
                        Ok(n) => {
                            return Some((
                                Message::Pty(exec_ind, is_fd3, Box::from(&buf[..n])),
                                (master, exec_ind, is_fd3)
                            ));
                        }
                        Err(_) => return None,
                    }
                }
            }))
        },
    })
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
                (!r.is_null()).then(||
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
