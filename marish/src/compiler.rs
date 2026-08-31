use std::ffi::{OsString, OsStr};
use std::fs;
use std::path::{Path, PathBuf};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::ffi::OsStrExt;
use std::collections::HashMap;

use crate::parser::*;
use crate::vm::*;

#[derive(Clone, Debug)]
pub enum Warning {
    CommandNotFound(LineCol, String),
}

#[derive(Clone, Debug)]
pub enum CompileError { }

/// Compiles AST into assembly blocks. First block is the entry one; last block is always an
/// artificial block that call all the test blocks
pub fn compile(path: &[PathBuf], ast: &[Ast]) -> Result<(Vec<Warning>, Vec<Block>), CompileError> {
    let flags = Flags {
        muffle: false,
    };
    let mut compiler = Compiler {
        warnings: Vec::new(),
        blocks: vec![Block { id: BlockId::Entry, contents: Vec::new() }],
    };

    let mut base_block = Vec::new();
    for ast in ast {
        compiler.compile_ast(flags, path, ast, &mut base_block)?;
    }

    base_block.push(Instr::Return);
    compiler.blocks[0].contents = base_block;

    // Create test block
    let mut test_block = Vec::new();
    for (i, b) in compiler.blocks.iter().enumerate() {
        if matches!(b.id, BlockId::Test { .. }) {
            test_block.push(Instr::Call { block: i });
        }
    }
    test_block.push(Instr::Return);
    compiler.blocks.push(Block { id: BlockId::TestsEntry, contents: test_block });

    Ok((compiler.warnings, compiler.blocks))
}

#[derive(Copy, Clone)]
struct Flags {
    muffle: bool,
}

impl Flags {
    fn muffle(mut self) -> Self {
        self.muffle = true;
        self
    }
}

struct Compiler {
    warnings: Vec<Warning>,
    blocks: Vec<Block>,
}

impl Compiler {
    fn compile_token(
        &mut self,
        flags: Flags,
        path: &[PathBuf],
        tok: &Token,
        out: &mut Vec<Instr>,
    ) -> Result<(), CompileError> {
        match tok {
            Token::Var(v) => {
                out.push(Instr::VarRef(v.name.clone()));
                for field in &v.fields {
                    match field {
                        FieldExpr::Column(operand) => {
                            self.compile_single(flags, path, operand, out)?;
                            out.push(Instr::GetColumn);
                        }
                        FieldExpr::Index(operand) => {
                            self.compile_single(flags, path, operand, out)?;
                            out.push(Instr::GetIndex);
                        }
                    }
                }
                out.push(Instr::Deref);
            }
            Token::Array(Array { items }) => {
                for item in items.iter().rev() {
                    self.compile_single(flags, path, item, out)?;
                }
                out.push(Instr::CollectArray(items.len()));
            }
            Token::Table(Table { header, rows }) => {
                for item in header.iter().rev() {
                    self.compile_single(flags, path, item, out)?;
                }
                out.push(Instr::CollectArray(header.len()));
                for row in rows.iter().rev() {
                    for item in row.iter().rev() {
                        self.compile_single(flags, path, item, out)?;
                    }
                    out.push(Instr::CollectArray(row.len()));
                }
                out.push(Instr::CollectTable(rows.len()));
            }
            _ => out.push(Instr::Load(tok.clone())),
        }
        Ok(())
    }

    fn compile_single(
        &mut self,
        flags: Flags,
        path: &[PathBuf],
        operand: &Single,
        out: &mut Vec<Instr>,
    ) -> Result<(), CompileError> {
        match operand {
            Single::Token(tok) => self.compile_token(flags, path, &tok, out)?,
            Single::Sub(body) => {
                let mut b = Block::new_internal();
                self.compile_ast(flags, path, body, &mut b.contents)?;
                b.contents.push(Instr::Return);
                self.blocks.push(b);
                out.push(Instr::Call { block: self.blocks.len() - 1 });
            },
        }
        Ok(())
    }

    fn compile_ast(
        &mut self,
        flags: Flags,
        path: &[PathBuf],
        ast: &Ast,
        out: &mut Vec<Instr>,
    ) -> Result<(), CompileError> {
        match ast {
            Ast::Stmt(_lc, Stmt::Token(token)) => {
                self.compile_token(flags, path, &token, out)?;
            },
            Ast::Stmt(_lc, Stmt::Assert(Assert { lc, body })) => {
                let mut b = Block::new_internal();
                self.compile_ast(flags, path, body, &mut b.contents)?;
                b.contents.push(Instr::Return);
                self.blocks.push(b);
                out.push(Instr::Call { block: self.blocks.len() - 1 });
                out.push(Instr::Assert(*lc));
            }
            Ast::Stmt(_lc, Stmt::Assignment(Assignment { lhs, rhs })) => {
                self.compile_single(flags.muffle(), path, rhs, out)?;
                out.push(Instr::VarRef(lhs.name.clone()));
                for field in &lhs.fields {
                    match field {
                        FieldExpr::Column(operand) => {
                            self.compile_single(flags, path, &operand, out)?;
                            out.push(Instr::GetColumn);
                        }
                        FieldExpr::Index(operand) => {
                            self.compile_single(flags, path, &operand, out)?;
                            out.push(Instr::GetIndex);
                        }
                    }
                }
                out.push(Instr::SetVar);
            }
            Ast::Stmt(_lc, Stmt::TestDecl(TestDecl { name, body })) => {
                let mut b = Block { id: BlockId::Test { name: name.clone() }, contents: Vec::new() };
                b.contents.push(Instr::EnterTest(name.clone()));
                for item in body {
                    self.compile_ast(flags, path, item, &mut b.contents)?;
                }
                b.contents.push(Instr::RecordTestSuccess);
                b.contents.push(Instr::Return);
                self.blocks.push(b);
            }
            Ast::Stmt(_lc, Stmt::Sub(body)) => {
                let mut b = Block::new_internal();
                self.compile_ast(flags, path, body, &mut b.contents)?;
                b.contents.push(Instr::Return);
                self.blocks.push(b);
                out.push(Instr::Call { block: self.blocks.len() - 1 });
            }
            Ast::Stmt(_lc, Stmt::BoolExpr(BoolExpr { lhs, rhs, op })) => {
                self.compile_single(flags, path, lhs, out)?;
                self.compile_single(flags, path, rhs, out)?;
                out.push(Instr::Op(*op));
            }
            Ast::Stmt(_lc, Stmt::BoolNegate(inner)) => {
                self.compile_single(flags, path, inner, out)?;
                out.push(Instr::Negate);
            },
            Ast::Stmt(_lc, Stmt::Where(_)) => todo!(),
            Ast::Stmt(_lc, Stmt::Command(command)) => {
                match command.cmd.as_str() {
                    "cd" => {
                        if command.argv.len() != 1 {
                            panic!("TODO: handle cd getting wrong number of args");
                        }
                        self.compile_single(flags, path, &command.argv[0], out)?;
                        out.push(Instr::ChangeDir);
                    }
                    command_str => {
                        let command_path = resolve(path, &command_str);
                        if command_path.is_none() {
                            self.warnings.push(Warning::CommandNotFound(command.lc, command_str.to_owned()));
                        }
                        for item in command.argv.iter().rev() {
                            self.compile_single(flags.muffle(), path, item, out)?;
                        }
                        let argc = command.argv.len();
                        let orig = command_str.to_string();
                        out.push(Instr::Run {
                            muffle: flags.muffle,
                            command: Command2 { lc: command.lc, orig, path: command_path, argc }
                        });
                    },
                }
            },
            Ast::Stmt(_lc, Stmt::Pipeline(Pipeline { initial_value, items })) => {
                let is_simple = !items.iter().any(|c| matches!(c, PipelineItem::Sub(_)));

                let is_there_initial_value = initial_value.is_some();
                if let Some(token) = initial_value.clone() {
                    self.compile_token(flags, path, &token, out)?;
                }

                if is_simple {
                    let instr = Instr::RunPipeline {
                        muffle: flags.muffle,
                        is_there_initial_value,
                        items: items.into_iter()
                            .map(|c| match c {
                                PipelineItem::Command(c) => {
                                    // FIXME: handle "cd" here
                                    let orig = c.cmd.clone();
                                    let command_path = resolve(path, &orig);
                                    if command_path.is_none() {
                                        self.warnings.push(Warning::CommandNotFound(c.lc, orig.clone()));
                                    }
                                    for item in c.argv.iter().rev() {
                                        self.compile_single(flags.muffle(), path, item, out)?;
                                    }
                                    Ok(RunPipelineItem::Command(Command2 {
                                        lc: c.lc, orig, argc: c.argv.len(),
                                        path: command_path
                                    }))
                                },
                                PipelineItem::Where(func, lc) => {
                                    let mut b = Block::new_internal();
                                    self.compile_ast(flags, path, func, &mut b.contents)?;
                                    b.contents.push(Instr::Return);
                                    self.blocks.push(b);
                                    Ok(RunPipelineItem::Where { block: self.blocks.len() - 1, lc: *lc })
                                },
                                PipelineItem::Sub(_) => todo!(),
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    };
                    out.push(instr);
                } else {
                    todo!()
                }
            }
            Ast::Stmt(_lc, Stmt::Background(ast)) => {
                let mut b = Block::new_internal();
                self.compile_ast(flags, path, ast, &mut b.contents)?;
                b.contents.push(Instr::Return);
                self.blocks.push(b);
                out.push(Instr::CallAsync { block: self.blocks.len() - 1 });
            },
            _ => todo!(),
        }

        Ok(())
    }
}

pub fn is_valid_executable(met: Option<fs::Metadata>) -> bool {
    met
        .map(|m| !m.file_type().is_dir() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn path_from_env(env: &HashMap<OsString, OsString>) -> Vec<PathBuf> {
    env
        .get(OsStr::new("PATH"))
        .map_or(OsStr::new(""), |v| v)
        .as_encoded_bytes()
        .split(|n| *n == b':')
        .map(|seg| Path::new(OsStr::from_bytes(seg)).to_owned())
        .collect::<Vec<_>>()
}

pub fn resolve(paths: &[PathBuf], cmd: &str) -> Option<PathBuf> {
    if cmd.as_bytes().contains(&b'/') {
        let path = PathBuf::from(cmd);
        return is_valid_executable(fs::metadata(&path).ok()).then_some(path);
    }

    for path in paths {
        match fs::read_dir(path) {
            Ok(iter) => {
                for item in iter {
                    let Ok(item) = item else { continue };
                    if item.file_name() == OsStr::new(cmd)
                        && is_valid_executable(item.metadata().ok())
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
