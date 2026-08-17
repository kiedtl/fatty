use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::os::unix::fs::PermissionsExt;

use crate::parser::*;
use crate::vm::*;

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

    base_block.push(Instr::Return);
    blocks[0].contents = base_block;

    Ok(blocks)
}

fn compile_token(
    path: &[PathBuf],
    tok: &Token,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>,
) -> Result<(), CompileError> {
    match tok {
        Token::Var(v) => {
            out.push(Instr::VarRef(v.name.clone()));
            for field in &v.fields {
                match field {
                    FieldExpr::Column(operand) => {
                        compile_single(path, operand, out, blocks)?;
                        out.push(Instr::GetColumn);
                    }
                    FieldExpr::Index(operand) => {
                        compile_single(path, operand, out, blocks)?;
                        out.push(Instr::GetIndex);
                    }
                }
            }
            out.push(Instr::Deref);
        }
        Token::Array(Array { items }) => {
            for item in items.iter().rev() {
                compile_single(path, item, out, blocks)?;
            }
            out.push(Instr::CollectArray(items.len()));
        }
        Token::Table(Table { header, rows }) => {
            for item in header.iter().rev() {
                compile_single(path, item, out, blocks)?;
            }
            out.push(Instr::CollectArray(header.len()));
            for row in rows.iter().rev() {
                for item in row.iter().rev() {
                    compile_single(path, item, out, blocks)?;
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
    path: &[PathBuf],
    operand: &Single,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>
) -> Result<(), CompileError> {
    match operand {
        Single::Token(tok) => compile_token(path, &tok, out, blocks)?,
        Single::Sub(body) => {
            let mut b = Block { contents: Vec::new() };
            compile_ast(path, body, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
            out.push(Instr::Call { block: blocks.len() - 1 });
        },
    }
    Ok(())
}

fn compile_ast(
    path: &[PathBuf],
    ast: &Ast,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>
) -> Result<(), CompileError> {
    match ast {
        Ast::Stmt(_lc, Stmt::Assignment(Assignment { lhs, rhs })) => {
            compile_single(path, rhs, out, blocks)?;
            out.push(Instr::VarRef(lhs.name.clone()));
            for field in &lhs.fields {
                match field {
                    FieldExpr::Column(operand) => {
                        compile_single(path, &operand, out, blocks)?;
                        out.push(Instr::GetColumn);
                    }
                    FieldExpr::Index(operand) => {
                        compile_single(path, &operand, out, blocks)?;
                        out.push(Instr::GetIndex);
                    }
                }
            }
            out.push(Instr::SetVar);
        }
        Ast::Stmt(_lc, Stmt::Sub(body)) => {
            let mut b = Block { contents: Vec::new() };
            compile_ast(path, body, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
            out.push(Instr::Call { block: blocks.len() - 1 });
        }
        Ast::Stmt(_lc, Stmt::BoolExpr(BoolExpr { lhs, rhs, op })) => {
            compile_single(path, lhs, out, blocks)?;
            compile_single(path, rhs, out, blocks)?;
            out.push(Instr::Op(*op));
        }
        Ast::Stmt(_lc, Stmt::BoolNegate(inner)) => {
            compile_single(path, inner, out, blocks)?;
            out.push(Instr::Negate);
        },
        Ast::Stmt(_lc, Stmt::Where(_)) => todo!(),
        Ast::Stmt(_lc, Stmt::Command(command)) => {
            match command.cmd.as_str() {
                "cd" => {
                    if command.argv.len() != 1 {
                        panic!("TODO: handle cd getting wrong number of args");
                    }
                    compile_single(path, &command.argv[0], out, blocks)?;
                    out.push(Instr::ChangeDir);
                }
                command_str => {
                    let command_path = resolve(path, &command_str)
                        .ok_or_else(|| CompileError::CommandNotFound(command.lc, command_str.to_owned()))?;
                    for item in command.argv.iter().rev() {
                        compile_single(path, item, out, blocks)?;
                    }
                    let argc = command.argv.len();
                    let orig = command_str.to_string();
                    out.push(Instr::Run { command: Command2 { orig, path: command_path, argc } });
                },
            }
        },
        Ast::Stmt(_lc, Stmt::Pipeline(Pipeline { items })) => {
            let is_simple = !items.iter().any(|c| matches!(c, PipelineItem::Sub(_)));

            if is_simple {
                let instr = Instr::RunPipeline {
                    items: items.into_iter()
                        .map(|c| match c {
                            PipelineItem::Command(c) => {
                                // FIXME: handle "cd" here
                                let orig = c.cmd.clone();
                                let command_path = resolve(path, &orig)
                                    .ok_or_else(|| CompileError::CommandNotFound(c.lc, orig.clone()))?;
                                for item in c.argv.iter().rev() {
                                    compile_single(path, item, out, blocks)?;
                                }
                                let argc = c.argv.len();
                                Ok(RunPipelineItem::Command(Command2 { orig, argc, path: command_path }))
                            },
                            PipelineItem::Where(func) => {
                                if let Some(func) = func {
                                    let mut b = Block { contents: Vec::new() };
                                    compile_ast(path, func, &mut b.contents, blocks)?;
                                    b.contents.push(Instr::Return);
                                    blocks.push(b);
                                    Ok(RunPipelineItem::Where { block: blocks.len() - 1 })
                                } else {
                                    Ok(RunPipelineItem::Where { block: 0 })
                                }
                            },
                            // PipelineItem::Query(q) => {
                            //     Ok(RunPipelineItem::Query(q.clone()))
                            // },
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
            out.push(Instr::CallAsync { block: blocks.len() });

            let mut b = Block { contents: Vec::new() };
            compile_ast(path, ast, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
        },
        _ => todo!(),
    }

    Ok(())
}

fn resolve(paths: &[PathBuf], cmd: &str) -> Option<PathBuf> {
    fn is_valid(met: Option<fs::Metadata>) -> bool {
        met
            .map(|m| !m.file_type().is_dir() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    if cmd.as_bytes().contains(&b'/') {
        let path = PathBuf::from(cmd);
        return is_valid(fs::metadata(&path).ok()).then_some(path);
    }

    for path in paths {
        match fs::read_dir(path) {
            Ok(iter) => {
                for item in iter {
                    let Ok(item) = item else { continue };
                    if item.file_name() == OsStr::new(cmd) && is_valid(item.metadata().ok()) {
                        return Some(item.path().to_owned());
                    }
                }
            },
            Err(_) => continue,
        }
    }
    None
}
