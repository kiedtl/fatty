use std::fs;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::{ExitReason, Execution};
use crate::parser::*;
use crate::{out, outln};
use crate::utils;

use itertools::Itertools;
use rustix::process::Pid;
use rustix::fd::{FromRawFd, AsFd, AsRawFd, OwnedFd, BorrowedFd};
use tokio::sync::{mpsc, watch};

// pub enum RunCondition {
//     PreviousFailed,
//     PreviousSucceeded,
// }

#[derive(Debug, Clone, PartialEq)]
pub struct Command2 {
    pub path: PathBuf,
    pub orig: String,
    pub args: Vec<Token>,
}

impl Command2 {
    pub fn to_string(&self) -> String {
        format!("{} {}", self.orig, self.args.iter().map(|t| t.to_string()).join(" "))
    }
}

#[derive(Debug, Clone)]
pub enum RunPipelineItem {
    Command(Command2),
    // Query(Query),
}

#[derive(Debug, Clone)]
pub enum Instr {
    ChangeDir { argv: Vec<Token> },
    Run {
        command: Command2,
        // cond: RunCondition,
    },
    RunPipeline {
        items: Vec<RunPipelineItem>,
        // cond: RunCondition,
    },
    CallAsync {
        block: usize,
    },
    DoneProgram,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub contents: Vec<Instr>,
}

#[derive(Clone, Debug)]
pub enum CompileError {
    CommandNotFound(LineCol, String),
}

pub fn compile(path: &[PathBuf], ast: &[Ast]) -> Result<Vec<Block>, CompileError> {
    let mut blocks = vec![Block { contents: Vec::new() }];

    let mut base_block = Vec::new();
    for ast in ast {
        compile_ast(path, ast, &mut base_block, &mut blocks)?;
    }

    base_block.push(Instr::DoneProgram);
    blocks[0].contents = base_block;

    Ok(blocks)
}

fn compile_ast(
    path: &[PathBuf],
    ast: &Ast,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>
) -> Result<(), CompileError>
{
    match ast {
        Ast::Stmt(_lc, Stmt::Command(command)) => {
            if let Some(command_str) = command.argv.get(0) {
                match command_str.as_str() {
                    "cd" => out.push(Instr::ChangeDir { argv: command.argv[1..].to_vec() }),
                    _ => {
                        let c = command_str.to_string();
                        let path = resolve(path, &c)
                            .ok_or_else(|| CompileError::CommandNotFound(command.lc, c.clone()))?;
                        let args = command.argv.iter().skip(1).cloned().collect::<Vec<_>>();
                        let orig = c.clone();
                        out.push(Instr::Run { command: Command2 { orig, path, args } });
                    },
                }
            }
        },
        Ast::Stmt(_lc, Stmt::Pipeline(Pipeline { items })) => {
            let is_simple = !items.iter().any(|c| matches!(c, PipelineItem::Sub(_)));

            if is_simple {
                out.push(Instr::RunPipeline {
                    items: items.into_iter()
                        .map(|c| match c {
                            PipelineItem::Command(c) => {
                                // FIXME: handle "cd" here
                                let cmd = c.argv[0].to_string();
                                let path = resolve(path, &cmd)
                                    .ok_or_else(|| CompileError::CommandNotFound(c.lc, cmd.clone()))?;
                                let args = c.argv.iter().skip(1).cloned().collect::<Vec<_>>();
                                let orig = cmd.clone();
                                Ok(RunPipelineItem::Command(Command2 { orig, args, path }))
                            },
                            // PipelineItem::Query(q) => {
                            //     Ok(RunPipelineItem::Query(q.clone()))
                            // },
                            PipelineItem::Sub(_) => todo!(),
                        })
                        .collect::<Result<Vec<_>, _>>()?
                });
            } else {
                todo!()
            }
        }
        Ast::Stmt(_lc, Stmt::Background(ast)) => {
            out.push(Instr::CallAsync { block: blocks.len() });

            let mut b = Block { contents: Vec::new() };
            compile_ast(path, ast, &mut b.contents, blocks)?;
            b.contents.push(Instr::DoneProgram);
            blocks.push(b);
        },
        _ => todo!(),
    }

    Ok(())
}

fn resolve(paths: &[PathBuf], cmd: &str) -> Option<PathBuf> {
    for path in paths {
        match fs::read_dir(path) {
            Ok(iter) => {
                for item in iter {
                    let Ok(item) = item else { continue };
                    if item.file_name() == OsStr::new(cmd)
                        && item.metadata()
                            .map(|m|
                                !m.file_type().is_dir() && m.permissions().mode() & 0o100 != 0
                            )
                            .unwrap_or(false)
                    {
                        return Some(item.path().to_owned());
                    }
                }
            },
            Err(_) => continue,
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct Job {
    pub status: watch::Receiver<VMStatus>,
}

impl Job {
    pub fn is_done(&self) -> bool {
        matches!(&*self.status.borrow(), VMStatus::Resolved { .. })
    }
}

#[derive(Debug, Clone, Default)]
pub enum VMStatus {
    #[default]
    None,
    Waiting {
        command: Command2,
        pid: Pid,
    },
    Done {
        command: Command2,
        reason: ExitReason,
    },
    Resolved {
        command: Option<Command2>,
        reason: Option<ExitReason>,
    }
}

#[derive(Clone, Debug)]
pub enum VMMessage {
    ChangedDir,
    Done(Option<ExitReason>),
    Waiting(Pid),
    Job(Job),
}

pub struct VM {
    pub fd3_slave: Option<OwnedFd>,
    pub slave: Option<OwnedFd>,
    pub program: Arc<Vec<Block>>,
    pub pc: (usize, Option<usize>),

    pub env: Arc<HashMap<OsString, OsString>>,
    pub waiting_on: Option<Pid>,
    pub child_exit_stack: Vec<ExitReason>,

    // For communicating between top-level shell and subshells. Rx is inside App.jobs.
    pub status: Option<watch::Sender<VMStatus>>,

    // For communicating between top-level shell and Fatty.
    pub msg_tx: Option<mpsc::Sender<VMMessage>>,

    pub done: bool,
}

impl VM {
    // Explicitely write out result type, and do the Box::pin thing because execute calls
    // execute_once which calls execute which calls execute_once... which makes rustc give up on
    // deciding whether execute_once() is Send or not, which makes tokio:spawn() very sad.
    pub fn execute(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            loop {
                self.execute_once().await;
                if self.done {
                    break;
                } else if let Some(pid) = self.waiting_on {
                    match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()) { //, rustix::process::WaitOptions::NOHANG) {
                        Ok(Some((_, status))) => {
                            let reason = ExitReason::from(status);

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

                            self.handle_exit(pid, reason);
                        },
                        Ok(None) | Err(_) => unreachable!(),
                        //Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
                    }
                }
            }
        })
    }

    pub async fn execute_once(&mut self) {
        assert!(!self.done);
        assert!(self.waiting_on.is_none());

        // Increment PC
        let instr_pc = match self.pc.1 {
            None => 0,
            Some(c) => c + 1,
        };
        self.pc.1 = Some(instr_pc);

        match &self.program[self.pc.0].contents[instr_pc] {
            Instr::ChangeDir { argv } => {
                let argv = prepare_args(&argv).collect::<Vec<_>>();

                if argv.len() != 1 {
                    outln!("Usage: cd <dir>");
                    return;
                }

                if let Err(e) = std::env::set_current_dir(&argv[0]) {
                    outln!("cd: {e:?}");
                }

                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::ChangedDir).await.unwrap();
                }
            }
            Instr::Run { command } => {
                let (cmd, args) = prepare_invocation(&command);
                let mut pcmd = std::process::Command::new(&cmd);
                pcmd.envs(&*self.env);
                pcmd.args(args);
                if let Some(slave) = &self.slave {
                    let slave = slave.as_fd();
                    pcmd.stdin(rustix::io::dup(slave).unwrap());
                    pcmd.stdout(rustix::io::dup(slave).unwrap());
                    pcmd.stderr(rustix::io::dup(slave).unwrap());
                    add_terminal_controller(&mut pcmd, slave);
                }
                if let Some(fd3_slave) = &self.fd3_slave {
                    add_stdobjout(&mut pcmd, fd3_slave.as_fd());
                }

                let child = pcmd.spawn().unwrap();
                let pid = Pid::from_child(&child);
                self.waiting_on = Some(pid);
                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::Waiting(pid)).await.unwrap();
                }
                if let Some(sender) = &self.status {
                    sender.send(VMStatus::Waiting { pid, command: command.clone() }).unwrap();
                }
            },
            Instr::RunPipeline { items } => {
                use std::process::Stdio;

                let mut reader;
                let mut next_reader = None;
                let mut writer = None;

                let mut reader_obj;
                let mut next_reader_obj = None;
                let mut writer_obj = None;

                let mut last_pid = None;

                for (i, item) in items.iter().enumerate() {
                    let RunPipelineItem::Command(command) = item else { unreachable!() };
                    let mut keep = Vec::<OwnedFd>::new();
                    reader = next_reader;
                    next_reader = None;

                    reader_obj = next_reader_obj;
                    next_reader_obj = None;

                    if i < items.len() - 1 {
                        let (fr, fw) = std::io::pipe().unwrap();
                        next_reader = Some(fr);
                        writer = Some(fw);

                        let (fr_obj, fw_obj) = std::io::pipe().unwrap();
                        next_reader_obj = Some(fr_obj);
                        writer_obj = Some(fw_obj);
                    }

                    let (cmd, args) = prepare_invocation(&command);
                    let mut command = std::process::Command::new(cmd);
                    command.envs(&*self.env);
                    command.args(args);

                    if let Some(writer) = writer.take() {
                        command.stdout(writer);
                    } else if let Some(slave) = &self.slave {
                        command.stdout(rustix::io::dup(slave.as_fd()).unwrap());
                    }

                    if let Some(writer_obj) = writer_obj.take() {
                        add_stdobjout(&mut command, writer_obj.as_fd());
                        keep.push(writer_obj.into());
                    } else if let Some(fd3_slave) = &self.fd3_slave {
                        add_stdobjout(&mut command, fd3_slave.as_fd());
                    }

                    if let Some(reader) = reader {
                        command.stdin(reader);
                    } else if let Some(slave) = &self.slave {
                        command.stdin(rustix::io::dup(slave.as_fd()).unwrap());
                    }

                    if let Some(reader_obj) = reader_obj {
                        add_stdobjin(&mut command, reader_obj.as_fd());
                        keep.push(reader_obj.into());
                    } else if let Some(fd3_slave) = &self.fd3_slave {
                        add_stdobjin(&mut command, fd3_slave.as_fd());
                    }

                    if let Some(slave) = &self.slave {
                        command.stderr(rustix::io::dup(slave.as_fd()).unwrap());
                    }

                    let child = command.spawn().unwrap();
                    last_pid = Some(Pid::from_child(&child));
                    std::mem::drop(keep);
                }

                let RunPipelineItem::Command(last) = items.last().unwrap() else { unreachable!() };
                self.waiting_on = last_pid;
                if let Some(sender) = &self.status {
                    sender.send(VMStatus::Waiting {
                        command: last.clone(),
                        pid: last_pid.unwrap(),
                    }).unwrap();
                }
            },
            Instr::CallAsync { block } => {
                let (tx, rx) = watch::channel(VMStatus::default());

                let env = self.env.clone();
                let program = self.program.clone();
                let block = *block;
                tokio::spawn(async move {
                    let mut vm = VM {
                        fd3_slave: None,
                        slave: None,
                        env,
                        program,
                        pc: (block, None),
                        waiting_on: None,
                        child_exit_stack: Vec::new(),
                        status: Some(tx),
                        msg_tx: None,
                        done: false,
                    };
                    vm.execute().await;
                });

                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::Job(Job { status: rx, })).await.unwrap();
                }
            },
            Instr::DoneProgram => {
                self.done = true;
                if let Some(sender) = &self.msg_tx {
                    sender.send(VMMessage::Done(self.child_exit_stack.pop())).await.unwrap();
                }
                if let Some(sender) = &self.status {
                    sender.send_modify(move |previous| {
                        match std::mem::take(previous) {
                            VMStatus::Done { command, reason } => *previous = VMStatus::Resolved { command: Some(command), reason: Some(reason) },
                            VMStatus::None => *previous = VMStatus::Resolved { command: None, reason: None },
                            _ => unreachable!(),
                        }
                    });
                }
            },
        }
    }

    pub fn handle_exit(&mut self, pid: Pid, reason: ExitReason) {
        if Some(pid) == self.waiting_on {
            self.waiting_on = None;
            self.child_exit_stack.push(reason);
            if let Some(sender) = &self.status {
                sender.send_modify(move |previous| {
                    let VMStatus::Waiting { command, .. } = std::mem::take(previous)
                        else { unreachable!() };
                    *previous = VMStatus::Done { command, reason };
                });
            }
        } else {
            unreachable!();
        }
    }
}

#[allow(dead_code)]
pub fn print_program(p: &[Block]) {
    for (blocki, block) in p.iter().enumerate() {
        println!("Block {blocki}:");
        for instr in &block.contents {
            match instr {
                Instr::ChangeDir { argv }
                    => println!("  - cd {}", argv.iter().map(|t| t.to_string()).join(" ")),
                Instr::Run { command: Command2 { path, args, .. } }
                    => println!("  - run {}: {}", path.display(), args.iter().map(|t| t.to_string()).join(" ")),
                Instr::RunPipeline { items }
                    => {
                        println!("  - create_pipe");
                        for item in items {
                            match item {
                                RunPipelineItem::Command(c) => println!("  - run {}: {}", c.path.display(), c.args.iter().map(|t| t.to_string()).join(" ")),
                                // RunPipelineItem::Query(q) => println!("  - query {}", q.items.iter().map(|t| t.to_string()).join(" ")),
                            }
                        }
                    },
                Instr::CallAsync { block } => println!("  - call_async {block}"),
                Instr::DoneProgram => println!("  - done"),
            }
        }
    }
}

unsafe fn safe_dup_nocloexec(fdno: i32, to: i32) -> std::io::Result<()> {
    unsafe {
        if fdno == to {
            // fd is already TO, clear CLOEXEC
            let flags = libc::fcntl(to, libc::F_GETFD);
            if flags == -1 || libc::fcntl(to, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        } else {
            // Silly rustix requires an OwnedFd, so use libc.
            if libc::dup2(fdno, to) == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

fn add_stdobjin(cmd: &mut std::process::Command, fd3_slave: BorrowedFd) {
    let fd3_raw = fd3_slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || safe_dup_nocloexec(fd3_raw, 4));
    }
}

fn add_stdobjout(cmd: &mut std::process::Command, fd3_slave: BorrowedFd) {
    let fd3_raw = fd3_slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || safe_dup_nocloexec(fd3_raw, 3));
    }
}

fn add_terminal_controller(cmd: &mut std::process::Command, slave: BorrowedFd) {
    let raw = slave.as_raw_fd();
    unsafe {
        cmd.pre_exec(move || {
            rustix::process::setsid()?;
            rustix::process::ioctl_tiocsctty(&BorrowedFd::borrow_raw(raw))?;
            Ok(())
        });
    }
}

fn prepare_args(argv: &[Token]) -> impl Iterator<Item = String> {
    argv.iter()
        .map(|tok| {
            match tok {
                Token::Word(s) => expand_token(&s),
                _ => vec![tok.to_string()],
            }
        })
        .flatten()
}

fn prepare_invocation(c: &Command2) -> (&Path, impl Iterator<Item = String>) {
    (&c.path, prepare_args(&c.args))
}

fn expand_token(token: &str) -> Vec<String> {
    let token = expand_tilde(token);
    expand_glob(&token)
}

fn expand_tilde(token: &str) -> String {
    if token == "~" || token.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{}", home.to_string_lossy(), &token[1..]);
        }
    }
    token.to_owned()
}

fn expand_glob(token: &str) -> Vec<String> {
    // Only bother if it looks like a glob
    if !token.contains(['*', '?', '[']) {
        return vec![token.to_owned()];
    }

    let matches: Vec<String> = glob::glob(token)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    println!("expanded {} into {:?}", token, matches);

    // bash behaviour: if no match, pass the literal token through
    if matches.is_empty() { vec![token.to_owned()] } else { matches }
}
