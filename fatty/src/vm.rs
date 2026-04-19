use std::sync::Arc;
use std::time::Duration;

use crate::{ExitReason, Execution};
use crate::parser::*;
use crate::{out, outln};

use itertools::Itertools;
use rustix::process::Pid;
use rustix::fd::{AsRawFd, OwnedFd, BorrowedFd};
use tokio::sync::watch;

// pub enum RunCondition {
//     PreviousFailed,
//     PreviousSucceeded,
// }

#[derive(Debug, Clone)]
pub enum Instr {
    ChangeDir { argv: Vec<Token> },
    Run {
        command: Command,
        // cond: RunCondition,
    },
    RunSimplePipeline {
        commands: Vec<Command>,
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

pub fn compile(ast: &[Ast]) -> Vec<Block> {
    let mut blocks = vec![Block { contents: Vec::new() }];

    let mut base_block = Vec::new();
    for ast in ast {
        compile_ast(ast, &mut base_block, &mut blocks);
    }

    base_block.push(Instr::DoneProgram);
    blocks[0].contents = base_block;

    blocks
}

fn compile_ast(ast: &Ast, out: &mut Vec<Instr>, blocks: &mut Vec<Block>) {
    match ast {
        Ast::Stmt(Stmt::Command(command)) => {
            if let Some(command_str) = command.argv.get(0) {
                match command_str.as_str() {
                    "cd" => out.push(Instr::ChangeDir { argv: command.argv[1..].to_vec() }),
                    _ => out.push(Instr::Run { command: command.clone() }),
                }
            }
        },
        Ast::Stmt(Stmt::Pipeline(Pipeline { items })) => {
            let is_simple = !items.iter().any(|c| matches!(c, SubOrCommand::Sub(_)));

            if is_simple {
                out.push(Instr::RunSimplePipeline {
                    commands: items.into_iter()
                        .map(|c| match c {
                            SubOrCommand::Command(c) => c.clone(),
                            _ => unreachable!(),
                        })
                        .collect()
                });
            } else {
                todo!()
            }
        }
        Ast::Stmt(Stmt::Background(ast)) => {
            out.push(Instr::CallAsync { block: blocks.len() });

            let mut b = Block { contents: Vec::new() };
            compile_ast(ast, &mut b.contents, blocks);
            b.contents.push(Instr::DoneProgram);
            blocks.push(b);
        },
        _ => todo!(),
    }
}

pub struct Job {
    pub status: watch::Receiver<VMStatus>,
}

impl Job {
    pub fn is_done(&self) -> bool {
        matches!(&*self.status.borrow(), VMStatus::Resolved { .. })
    }
}

#[derive(Default)]
pub enum VMStatus {
    #[default]
    None,
    Waiting {
        command: Command,
        pid: Pid,
    },
    Done {
        command: Command,
        reason: ExitReason,
    },
    Resolved {
        command: Option<Command>,
        reason: Option<ExitReason>,
    }
}

pub struct VM {
    pub program: Arc<Vec<Block>>,
    pub pc: (usize, Option<usize>),
    pub waiting_on: Option<Pid>,
    pub child_exit_stack: Vec<ExitReason>,
    pub status: Option<watch::Sender<VMStatus>>,
    pub jobs: Vec<Job>,
    pub done: bool,
}

impl VM {
    pub fn execute(&mut self, slave: Option<BorrowedFd<'_>>) {
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
            }
            Instr::Run { command } => {
                let (cmd, args) = prepare_invocation(&command.argv);
                let mut pcmd = std::process::Command::new(&cmd);
                pcmd.args(args);
                if let Some(slave) = slave {
                    pcmd.stdin(rustix::io::dup(slave).unwrap());
                    pcmd.stdout(rustix::io::dup(slave).unwrap());
                    pcmd.stderr(rustix::io::dup(slave).unwrap());
                }

                let child = pcmd.spawn().unwrap();
                let pid = Pid::from_child(&child);
                self.waiting_on = Some(pid);
                if let Some(sender) = &self.status {
                    sender.send(VMStatus::Waiting { pid, command: command.clone() }).unwrap();
                }
            },
            Instr::RunSimplePipeline { commands } => {
                use std::process::Stdio;

                let mut reader;
                let mut next_reader = None;
                let mut writer = None;
                let mut last_pid = None;

                for (i, command) in commands.iter().enumerate() {
                    reader = next_reader;
                    next_reader = None;
                    if i < commands.len() - 1 {
                        let (fr, fw) = std::io::pipe().unwrap();
                        next_reader = Some(fr);
                        writer = Some(fw);
                    }

                    let (cmd, args) = prepare_invocation(&command.argv);
                    let mut command = std::process::Command::new(cmd);
                    command.args(args);

                    if let Some(writer) = writer.take() {
                        command.stdout(writer);
                    } else if let Some(slave) = slave {
                        command.stdout(rustix::io::dup(slave).unwrap());
                    }

                    if let Some(reader) = reader {
                        command.stdin(reader);
                    } else if let Some(slave) = slave {
                        command.stdin(rustix::io::dup(slave).unwrap());
                    }

                    if let Some(slave) = slave {
                        command.stderr(rustix::io::dup(slave).unwrap());
                    }

                    let child = command.spawn().unwrap();
                    last_pid = Some(Pid::from_child(&child));
                }

                self.waiting_on = last_pid;
                if let Some(sender) = &self.status {
                    sender.send(VMStatus::Waiting {
                        command: commands.last().unwrap().clone(),
                        pid: last_pid.unwrap(),
                    }).unwrap();
                }
            },
            Instr::CallAsync { block } => {
                let (tx, rx) = watch::channel(VMStatus::default());

                let mut vm = VM {
                    program: self.program.clone(),
                    pc: (*block, None),
                    waiting_on: None,
                    child_exit_stack: Vec::new(),
                    jobs: Vec::new(),
                    status: Some(tx),
                    done: false,
                };

                tokio::spawn(async move {
                    loop {
                        if vm.done {
                            break;
                        } else if let Some(pid) = vm.waiting_on {
                            match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG) {
                                Ok(Some((_, status))) => vm.handle_exit(pid, ExitReason::from(status)),
                                Ok(None) => tokio::time::sleep(Duration::from_millis(20)).await,
                                Err(_) => unreachable!(),
                            }
                        } else {
                            vm.execute(None);
                        }
                    }
                });

                self.jobs.push(Job {
                    status: rx,
                });
            },
            Instr::DoneProgram => {
                self.done = true;
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

pub fn print_program(p: &[Block]) {
    for (blocki, block) in p.iter().enumerate() {
        println!("Block {blocki}:");
        for instr in &block.contents {
            match instr {
                Instr::ChangeDir { argv }
                    => println!("  - cd {}", argv.iter().map(|t| t.to_string()).join(" ")),
                Instr::Run { command: Command { argv } }
                    => println!("  - run {}", argv.iter().map(|t| t.to_string()).join(" ")),
                Instr::RunSimplePipeline { commands }
                    => {
                        println!("  - create_pipe");
                        for command in commands {
                            println!("  - run {}", command.argv.iter().map(|t| t.to_string()).join(" "));
                        }
                    },
                Instr::CallAsync { block } => println!("  - call_async {block}"),
                Instr::DoneProgram => println!("  - done"),
            }
        }
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

fn prepare_invocation(argv: &[Token]) -> (String, impl Iterator<Item = String>) {
    let mut args = prepare_args(argv);
    let command = args.next().unwrap();
    (command, args)
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
