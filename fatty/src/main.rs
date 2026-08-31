#![allow(irrefutable_let_patterns)]
#![allow(dead_code)]
#![allow(unused_imports)]

use std::borrow::Cow;
use std::cell;
use std::collections::{HashSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::DirEntry;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};

use dirs;
use serde::{Serialize, Deserialize};
use futures::stream::BoxStream;
use futures::channel::mpsc as futures_mpsc; // TODO: convert all to tokio's mpsc
use futures::{StreamExt, SinkExt};
use inotify;
use itertools::Itertools;
use rustix::fd::{AsFd, OwnedFd, RawFd, AsRawFd};
use rustix::process::{kill_process, Pid, Signal};
use rustix::fs::FileType;
use tokio::sync::{mpsc, watch, Mutex as TokioMutex};
use vte;
use marish::{
    ExitReason,
    compiler,
    parser::{self, LineCol},
    vm::{self, VMStatus},
};

use iced::futures::stream;
use iced::window;
use iced::{Size, Event, Element, Task, Subscription, Padding, Length};
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
use iced::advanced::text::{Wrapping, Ellipsis};

mod bolger;
mod colors;
mod bwine_ui;
mod helpers;
mod styles;
mod term;
mod utils;
mod widgets;
mod completer;

use helpers::*;
use styles::CS;
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

#[derive(Serialize, Deserialize)]
pub struct Completions {
    registry: HashMap<PathBuf, PathBuf>,
}

pub struct Execution {
    fd3_master: Arc<OwnedFd>,

    cmdline: String,
    jobs: Vec<vm::Job>,
    waiting_on: Option<vm::WaitingOn2>,
    done: bool,
    rx: Option<Arc<TokioMutex<mpsc::Receiver<vm::VMMessage>>>>,
    stack: Vec<vm::StackValue>,
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
    pub fn new(cmdline: String, fd3_master: OwnedFd, rx: mpsc::Receiver<vm::VMMessage>, width: usize) -> Execution {
        Execution {
            fd3_master: Arc::new(fd3_master),
            cmdline,
            jobs: Vec::new(),
            waiting_on: None,
            done: false,
            rx: Some(Arc::new(TokioMutex::new(rx))),
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
            stack: Vec::new(),
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

#[derive(Clone)]
pub enum Message {
    None,
    Animate,
    Input(String, widgets::input::Cursor),
    Run,
    JobResolved(usize, usize),
    VMMessage(usize, vm::VMMessage),
    VMMessageClosed(usize),
    Pty(usize, bool, Box<[u8]>),
    PtyInput(usize, Cow<'static, [u8]>),
    PtyDrained(usize, bool),
    Signal(Signal),
    DirectoryChanged,
    Inotify(OsString),
    Controller(ControlMessage),
}

struct App {
    completions: Completions,
    master: Arc<OwnedFd>,
    slave: OwnedFd,
    control: ControlState,
    input: String,
    input_compile_error: Option<LineCol>,
    input_completions: Vec<String>,
    execs: Vec<Execution>,
    vars: HashMap<String, bwine::Value<'static>>,
    theme: styles::Theme,
    env: HashMap<OsString, OsString>,
    path: Vec<PathBuf>,

    listing: Vec<MyDirEntry>,
    listing_last_changed: Option<(OsString, Instant)>,

    vsize: cell::Cell<Option<Size>>,
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
        let config_dir = dirs::config_dir().unwrap().join("fatty");
        let completions: Completions = serde_json::from_str(
            &std::fs::read_to_string(config_dir.join("completions.json")).unwrap()
        ).unwrap();

        let pty = rustix_openpty::openpty(None, None).unwrap();
        let master_flags = rustix::fs::fcntl_getfl(&pty.controller).unwrap();
        rustix::fs::fcntl_setfl(
            &pty.controller,
            master_flags | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC
        ).unwrap();
        set_pty_output(&pty.controller);

        let mut env: HashMap<_, _> = std::env::vars_os().collect();
        env.insert("FATTY".into(), "normal0".into());

        let mut exe_path = PathBuf::from(std::env::current_exe().unwrap());
        exe_path.pop();

        let mut path = compiler::path_from_env(&env);
        path.insert(0, exe_path.join("fatty_bin/tools"));
        path.insert(0, exe_path.join("fatty_bin/crickhollow"));

        (
            Self {
                completions,
                control: ControlState::new(),
                input: String::new(),
                input_compile_error: None,
                input_completions: Vec::new(),
                listing: listing(),
                listing_last_changed: None,
                execs: Vec::new(),
                vars: HashMap::new(),
                master: Arc::new(pty.controller),
                slave: pty.user,
                theme: styles::Theme::gruvbox(),
                //shell: Arc::new(TokioMutex::new(shell)),
                vsize: cell::Cell::new(None),
                env,
                path,
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
            Message::Input(s, cursor) => {
                self.input = s;
                self.input_compile_error = None;
                self.input_completions.clear();
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        match compiler::compile(&self.path, &parsed) {
                            Ok((warnings, blocks)) => {
                                for warning in warnings {
                                    match warning {
                                        compiler::Warning::CommandNotFound(lc, _) => {
                                            self.input_compile_error = Some(lc);
                                        }
                                    }
                                }

                                let value = widgets::input::Value::new(&self.input);
                                let cursor_start = cursor.start(&value);
                                let mut editing_command = None;
                                for block in blocks {
                                    for instr in &block.contents {
                                        match instr {
                                            vm::Instr::Run { command, .. } => {
                                                if command.lc.1 <= cursor_start && command.lc.2 >= cursor_start {
                                                    editing_command = Some(command.clone());
                                                }
                                            },
                                            vm::Instr::RunPipeline { items, .. } => {
                                                for item in items {
                                                    match item {
                                                        vm::RunPipelineItem::Command(command) => {
                                                            if command.lc.1 <= cursor_start && command.lc.2 >= cursor_start {
                                                                editing_command = Some(command.clone());
                                                            }
                                                        },
                                                        _ => (),
                                                    }
                                                }
                                            },
                                            _ => (),
                                        }
                                    }
                                }

                                if let Some(c) = editing_command {
                                    if cursor_start <= c.lc.1 + c.orig.len() && !c.orig.as_bytes().contains(&b'/') {
                                        let mut completions = HashSet::new();
                                        for path in &self.path {
                                            match std::fs::read_dir(path) {
                                                Ok(iter) => {
                                                    for item in iter {
                                                        let Ok(item) = item else { continue };
                                                        if !compiler::is_valid_executable(item.metadata().ok()) {
                                                            continue;
                                                        }
                                                        let fname = item.file_name().to_string_lossy().into_owned();
                                                        if fname.starts_with(&c.orig) {
                                                            completions.insert(fname);
                                                        }
                                                    }
                                                },
                                                Err(_) => continue,
                                            }
                                        }
                                        self.input_completions = completions.into_iter().collect();
                                        self.input_completions.sort();
                                    } else if let Some(p) = &c.path && let Some(completer) = self.completions.registry.get(p) {
                                        let full_inp = self.input[c.lc.1..c.lc.2].to_owned();
                                        let cmd = full_inp[..c.orig.len()].to_owned();
                                        let inp = full_inp[c.orig.len()..].to_owned();
                                        let out = std::process::Command::new(completer)
                                            .args([cmd, inp, cursor_start.to_string()])
                                            .output();
                                        match out {
                                            Ok(output) => {
                                                let s = String::from_utf8_lossy(&output.stdout);
                                                for line in s.split("\n") {
                                                    if line.trim().is_empty() {
                                                        continue;
                                                    }
                                                    self.input_completions.push(line.to_owned());
                                                }
                                            },
                                            Err(_) => {
                                                println!("Completion fails");
                                            },
                                        }
                                    } else {
                                        let full_inp = self.input[c.lc.1..c.lc.2].to_owned();
                                        let cmd = &full_inp[..c.orig.len()];
                                        let inp = &full_inp[c.orig.len()..];
                                        self.input_completions = completer::dumb(cmd, inp, cursor_start);
                                    }
                                }
                            }
                            Err(_) => unreachable!(),
                        }
                    }
                    Err(_) => (),
                }
            },
            Message::Run => {
                self.input_completions.clear();
                match parser::parse_str(&self.input) {
                    Ok(parsed) => {
                        let program = match compiler::compile(&self.path, &parsed) {
                            Ok((_warn, p)) => p,
                            Err(_) => return Task::none(),
                        };
                        // println!("{:#?}", parsed);
                        // vm::print_program(&program);

                        self.control.history_cursor = None;
                        self.control.mode = ControlMode::Term;

                        let (font_width, font_height) = utils::measure_text(
                            "m", f32::INFINITY, FONT_SIZE, 1., term::Cell::default().iced_font()
                        );

                        // Sometimes there's an extra column that causes ugly wrapping
                        let font_width = font_width * 1.01;

                        let width = self.vsize.get()
                            .map(|s| (s.width / font_width).floor() as u16)
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

                        // Unix socket for bidi communication.. bad idea?
                        let (fd3_master, fd3_slave) = rustix::net::socketpair(
                            rustix::net::AddressFamily::UNIX,
                            rustix::net::SocketType::STREAM,
                            rustix::net::SocketFlags::NONBLOCK,
                            None,
                        ).unwrap();

                        let cmdline = std::mem::take(&mut self.input);
                        let (tx, rx) = mpsc::channel(999);
                        let ex = Execution::new(cmdline, fd3_master, rx, width as usize);
                        self.execs.push(ex);

                        let mut vm = vm::VM {
                            test_ctx: Default::default(),
                            fd3_slave: Some(Arc::new(fd3_slave)),
                            slave: Some(Arc::new(self.slave.try_clone().unwrap())),
                            env: Arc::new(self.env.clone()),
                            program: Arc::new(program),
                            pc: (0, None),
                            stack: Vec::new(),
                            scope: vec![vm::Scope {
                                is_inherited: false,
                                vars: self.vars.clone(),
                            }],
                            rstack: Vec::new(),
                            waiting_on: None,
                            child_exit_stack: Vec::new(),
                            msg_tx: Some(tx),
                            status: None,
                            done: false,
                        };

                        return Task::perform(async move {
                            vm.execute().await;
                        }, |_| Message::None);
                    },
                    Err(err) => {
                        outln!("{err}");
                        println!("{err}");
                    }
                }
            },
            Message::JobResolved(_exec_ind, _job_ind) => {
                // todo
            },
            Message::VMMessageClosed(exec_ind) => {
                if let Some(exec) = self.execs.get_mut(exec_ind) {
                    exec.rx = None;
                }
            }
            Message::VMMessage(exec_ind, message) => {
                if let Some(exec) = self.execs.get_mut(exec_ind) {
                    match message {
                        vm::VMMessage::ChangedDir => return self.update(Message::DirectoryChanged),
                        vm::VMMessage::Job(job) => {
                            exec.jobs.push(job);
                        }
                        vm::VMMessage::Waiting(waiting_on) => {
                            exec.waiting_on = Some(waiting_on);
                        }
                        vm::VMMessage::Done { last_exit_reason, stack, vars } => {
                            self.vars = vars;
                            exec.done = true;
                            exec.exit_reason = last_exit_reason;
                            exec.stack = stack;
                            exec.cleanup();
                            if self.control.mode == ControlMode::Term
                                && !self.execs.iter().any(|e| !e.done)
                            {
                                self.control.mode = ControlMode::Insert;
                            }
                        }
                    }
                }
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
                if exec.done {
                    if is_fd3 {
                        exec.fd3_is_drained = true;
                    } else {
                        exec.is_drained = true;
                    }
                }
            },
            Message::Signal(sig) => {
                if let Some(current) = self.execs.last() && let Some(waiting_on) = &current.waiting_on {
                    match waiting_on {
                        vm::WaitingOn2::Pid(pid) => kill_process(*pid, sig).unwrap(),
                        vm::WaitingOn2::Builtin(abort_h) => abort_h.abort(),
                    }
                }
            },
            Message::DirectoryChanged => {
                self.listing_last_changed = None;
                self.listing = listing();
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

        let jobs = watch_jobs(self);
        let vmwatch = watch_vms(self);

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

        Subscription::batch([polls, keys, jobs, vmwatch, fswatch, animation])
    }

    fn view(&self) -> Elem<'_> {
        let exit_reason: fn(_) -> _ = |reason| {
            let cored = match reason {
                Some(ExitReason::Signal { cored: true, .. })
                | Some(ExitReason::Unknown { cored: true, .. }) => true,
                _ => false,
            };
            let reason = match reason {
                None => "Running".to_string(),
                Some(ExitReason::Normal(code)) => code.to_string(),
                Some(ExitReason::Builtin) => "done".to_string(),
                Some(ExitReason::Signal { signal, .. }) => utils::signal_to_string(signal).to_string(),
                Some(ExitReason::Unknown { sigval: Some(s), .. }) => format!("Signal({s})"),
                Some(ExitReason::Unknown { sigval: None, .. }) => "Exited (unknown)".to_string(),
            };
            text(format!("{}{}", reason, if cored { " (cored)" } else { "" }))
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
                    .map(|d| {
                        utils::measure_text(
                            &d.name, std::f32::INFINITY,
                            16., 1., iced::font::Font::default()
                        ).0
                    })
                    .collect::<Vec<_>>();
                let max = widths.iter().copied().fold(0., f32::max);
                let cell_width = max.min(sz.width * 0.12).max(50.) * 1.1;

                let all_same_uname = app.listing.len() > 0
                    && app.listing.iter().skip(1).all(|it| app.listing[0].uname == it.uname);

                grid(app.listing.iter().map(|d: &MyDirEntry| {
                    container(
                        container(mycolumn![
                            container(
                                text(d.name.clone())
                                    .wrapping(Wrapping::WordOrGlyph)
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
                        ]).clip(true)
                    )
                        .class(CS::GrayBox)
                        .padding(3)
                        .into()
                }))
                    .spacing(4)
                    .height(Length::Shrink)
                    .fluid(cell_width)
            })
                .width(Length::Fill)
                .height(Length::Shrink)
                .into()
        }

        let mut jobs = Column::new()
            .spacing(1);

        for exec in &self.execs {
            for job in &exec.jobs {
                jobs = jobs.push(
                    container({
                        let e: Elem<'_> = match &*job.status.borrow() {
                            VMStatus::Waiting {
                                item: vm::RunPipelineItem::Command(c),
                                on: vm::WaitingOn2::Pid(pid)
                            } => text(format!("{} ({})", c.orig.clone(), pid)).into(),
                            VMStatus::Waiting {
                                item: vm::RunPipelineItem::Where { lc, .. },
                                ..
                            } => text(lc.get(&exec.cmdline)).into(),
                            VMStatus::Waiting { .. } => unreachable!(),
                            VMStatus::Done { command: Some(command), reason }
                            | VMStatus::Resolved { command: Some(command), reason: Some(reason) }
                                => row![
                                    text(command.orig.clone())
                                        .width(Length::Fill),
                                    exit_reason(Some(*reason)),
                                ].into(),
                            VMStatus::None | VMStatus::Resolved { .. } | VMStatus::Done { .. }
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

        let execs_view = move || {
            let mut col = Column::new()
                .spacing(1);

            for (i, exec) in self.execs.iter().enumerate() {
                col = col.push(
                    column![
                        container(row![
                            container(
                                text(&exec.cmdline)
                            )
                                .width(Length::Fill),
                            exit_reason(exec.exit_reason),
                        ])
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
                            scrollable(bwine_ui::to_iced(object, self.vsize.get())).into()
                        } else {
                            container(
                                widgets::tty::Tty::new(
                                    &exec.term,
                                    &self.theme,
                                    FONT_SIZE,
                                    |term, theme, color| term.resolve(theme, color),
                                    move |c| Message::PtyInput(i, c),
                                    i == self.execs.len() - 1 && !exec.done
                                )
                            )
                                .padding(1)
                                .width(Length::Fill)
                                .into()
                        },
                        Column::with_children(
                            exec.stack.iter().map(|item| {
                                let vm::StackValue::Value(v) = item else { unreachable!() };
                                bwine_ui::to_iced(v, self.vsize.get())
                            })
                        ),
                        // text({
                        //     let mut s = format!("p_stack: {}; ", exec.p_stack);
                        //     if exec.q_flag {
                        //         s = format!("{s}waiting for quote; ");
                        //     }
                        //     if exec.b_err {
                        //         s = format!("{s}corrupted output");
                        //     }
                        //     s
                        // }),
                    ],
                );
            }

            col
        };

        let mut input = input(self.control.mode, "rm -rf /", &self.input)
            .completions(self.input_completions.clone());

        if let Some(LineCol(_, s, e)) = self.input_compile_error {
            input.add_annotation(s, e);
        }

        if let Some(last) = self.execs.last() && !last.done {
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
                            self.vsize.set(Some(size));
                            scrollable(
                                container(execs_view())
                                    .padding(Padding {
                                        right: 15.,
                                        ..Default::default()
                                    })
                            )
                                .anchor_bottom()
                                .height(Length::Fill)
                        }),
                        input,
                    ]
                        .spacing(4.)
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
                                    let home = self.get_env("HOME", "").to_string_lossy().to_string();
                                    p = p.replace(&home, "~");
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
            .width(Length::Fill)
            .height(Length::Fill)
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
            exec.jobs
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
            iced::stream::channel(16, move |mut output: futures_mpsc::Sender<Message>| async move {
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

fn watch_vms(app: &App) -> Subscription<Message> {
    #[derive(Clone)]
    struct Watching {
        exec_ind: usize,
        rx: Arc<TokioMutex<mpsc::Receiver<vm::VMMessage>>>,
    }

    impl Hash for Watching {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.exec_ind.hash(state);
        }
    }

    let receivers: Vec<Watching> = app.execs.iter()
        .enumerate()
        .filter(|(_, exec)| exec.rx.is_some())
        .map(|(exec_ind, exec)| Watching { exec_ind, rx: exec.rx.as_ref().unwrap().clone() })
        .collect();

    if receivers.is_empty() {
        return Subscription::none();
    }

    Subscription::run_with(
        receivers,
        |receivers| {
            let receivers = receivers.clone();
            iced::stream::channel(32, move |mut output: futures_mpsc::Sender<Message>| async move {
                loop {
                    let futures: Vec<_> = receivers
                        .iter()
                        .map(|Watching { exec_ind, rx }| {
                            let rx = rx.clone();
                            let exec_idx = *exec_ind;
                            Box::pin(async move {
                                match rx.lock().await.recv().await {
                                    Some(val) => Message::VMMessage(exec_idx, val),
                                    None => Message::VMMessageClosed(exec_idx),
                                }
                            })
                        })
                        .collect();
                    let (val, _, _) = futures::future::select_all(futures).await;
                    output.send(val).await.unwrap();
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
