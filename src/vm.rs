use itertools::Itertools;

use crate::{ExitReason, Execution};
use crate::parser::*;
use rustix::process::Pid;

// pub enum RunCondition {
//     PreviousFailed,
//     PreviousSucceeded,
// }

#[derive(Debug, Clone)]
pub enum Instr {
    Run {
        command: Command,
        // cond: RunCondition,
    },
    RunSimplePipeline {
        commands: Vec<Command>,
        // cond: RunCondition,
    },
    DoneProgram,
}

#[derive(Debug, Clone)]
pub struct Block {
    contents: Vec<Instr>,
}

pub fn compile(ast: &[Ast]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut base_block = Block { contents: Vec::new() };
    for ast in ast {
        compile_ast(ast, &mut base_block.contents, &mut blocks);
    }

    base_block.contents.push(Instr::DoneProgram);
    blocks.insert(0, base_block);

    blocks
}

fn compile_ast(ast: &Ast, out: &mut Vec<Instr>, blocks: &mut Vec<Block>) {
    match ast {
        Ast::Stmt(Stmt::Command(command)) => out.push(Instr::Run { command: command.clone() }),
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
        _ => todo!(),
    }
}

pub struct VM {
    pub program: Vec<Block>,
    pub pc: (usize, Option<usize>),
    pub waiting_on: Option<Pid>,
    pub child_exit_stack: Vec<ExitReason>,
    pub done: bool,
}

impl VM {
    pub fn execute(&mut self) {
        assert!(!self.done);
        assert!(self.waiting_on.is_none());

        // Increment PC
        let instr_pc = match self.pc.1 {
            None => 0,
            Some(c) => c + 1,
        };
        self.pc.1 = Some(instr_pc);

        match &self.program[self.pc.0].contents[instr_pc] {
            Instr::Run { command: Command { argv } } => {
                let (cmd, args) = prepare_invocation(argv);
                let child = std::process::Command::new(cmd)
                    .args(args)
                    .spawn()
                    .unwrap();

                self.waiting_on = Some(Pid::from_child(&child));
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
                    }
                    if let Some(reader) = reader {
                        command.stdin(reader);
                    }

                    let child = command.spawn().unwrap();
                    last_pid = Some(Pid::from_child(&child));
                }

                self.waiting_on = last_pid;
            },
            Instr::DoneProgram => self.done = true,
        }
    }

    pub fn handle_exit(&mut self, pid: Pid, reason: ExitReason) {
        if Some(pid) == self.waiting_on {
            self.waiting_on = None;
            self.child_exit_stack.push(reason);
        } else {
            unreachable!();
        }
    }
}

fn prepare_invocation(argv: &[Token]) -> (String, impl Iterator<Item = String>) {
    let mut args = argv
        .iter()
        .map(|tok| {
            match tok {
                Token::Word(s) => expand_token(&s),
                _ => vec![tok.to_string()],
            }
        })
        .flatten();
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

    // bash behaviour: if no match, pass the literal token through
    if matches.is_empty() { vec![token.to_owned()] } else { matches }
}
