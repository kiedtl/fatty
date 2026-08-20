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

#[derive(Copy, Clone)]
struct Ctx {
    muffle: bool,
}

impl Ctx {
    fn muffle(mut self) -> Self {
        self.muffle = true;
        self
    }
}

/// Compiles AST into assembly blocks. First block is the entry one; last block is always an
/// artificial block that call all the test blocks
pub fn compile(path: &[PathBuf], ast: &[Ast]) -> Result<Vec<Block>, CompileError> {
    let ctx = Ctx {
        muffle: false,
    };
    let mut blocks = vec![Block { id: BlockId::Entry, contents: Vec::new() }];

    let mut base_block = Vec::new();
    for ast in ast {
        compile_ast(ctx, path, ast, &mut base_block, &mut blocks)?;
    }

    base_block.push(Instr::Return);
    blocks[0].contents = base_block;

    // Create test block
    let mut test_block = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if matches!(b.id, BlockId::Test { .. }) {
            test_block.push(Instr::Call { block: i });
        }
    }
    test_block.push(Instr::Return);
    blocks.push(Block { id: BlockId::TestsEntry, contents: test_block });

    Ok(blocks)
}

fn compile_token(
    ctx: Ctx,
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
                        compile_single(ctx, path, operand, out, blocks)?;
                        out.push(Instr::GetColumn);
                    }
                    FieldExpr::Index(operand) => {
                        compile_single(ctx, path, operand, out, blocks)?;
                        out.push(Instr::GetIndex);
                    }
                }
            }
            out.push(Instr::Deref);
        }
        Token::Array(Array { items }) => {
            for item in items.iter().rev() {
                compile_single(ctx, path, item, out, blocks)?;
            }
            out.push(Instr::CollectArray(items.len()));
        }
        Token::Table(Table { header, rows }) => {
            for item in header.iter().rev() {
                compile_single(ctx, path, item, out, blocks)?;
            }
            out.push(Instr::CollectArray(header.len()));
            for row in rows.iter().rev() {
                for item in row.iter().rev() {
                    compile_single(ctx, path, item, out, blocks)?;
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
    ctx: Ctx,
    path: &[PathBuf],
    operand: &Single,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>
) -> Result<(), CompileError> {
    match operand {
        Single::Token(tok) => compile_token(ctx, path, &tok, out, blocks)?,
        Single::Sub(body) => {
            let mut b = Block::new_internal();
            compile_ast(ctx, path, body, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
            out.push(Instr::Call { block: blocks.len() - 1 });
        },
    }
    Ok(())
}

fn compile_ast(
    ctx: Ctx,
    path: &[PathBuf],
    ast: &Ast,
    out: &mut Vec<Instr>,
    blocks: &mut Vec<Block>
) -> Result<(), CompileError> {
    match ast {
        Ast::Stmt(_lc, Stmt::Token(token)) => {
            compile_token(ctx, path, &token, out, blocks)?;
        },
        Ast::Stmt(_lc, Stmt::Assert(Assert { lc, body })) => {
            let mut b = Block::new_internal();
            compile_ast(ctx, path, body, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
            out.push(Instr::Call { block: blocks.len() - 1 });
            out.push(Instr::Assert(*lc));
        }
        Ast::Stmt(_lc, Stmt::Assignment(Assignment { lhs, rhs })) => {
            compile_single(ctx.muffle(), path, rhs, out, blocks)?;
            out.push(Instr::VarRef(lhs.name.clone()));
            for field in &lhs.fields {
                match field {
                    FieldExpr::Column(operand) => {
                        compile_single(ctx, path, &operand, out, blocks)?;
                        out.push(Instr::GetColumn);
                    }
                    FieldExpr::Index(operand) => {
                        compile_single(ctx, path, &operand, out, blocks)?;
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
                compile_ast(ctx, path, item, &mut b.contents, blocks)?;
            }
            b.contents.push(Instr::RecordTestSuccess);
            b.contents.push(Instr::Return);
            blocks.push(b);
        }
        Ast::Stmt(_lc, Stmt::Sub(body)) => {
            let mut b = Block::new_internal();
            compile_ast(ctx, path, body, &mut b.contents, blocks)?;
            b.contents.push(Instr::Return);
            blocks.push(b);
            out.push(Instr::Call { block: blocks.len() - 1 });
        }
        Ast::Stmt(_lc, Stmt::BoolExpr(BoolExpr { lhs, rhs, op })) => {
            compile_single(ctx, path, lhs, out, blocks)?;
            compile_single(ctx, path, rhs, out, blocks)?;
            out.push(Instr::Op(*op));
        }
        Ast::Stmt(_lc, Stmt::BoolNegate(inner)) => {
            compile_single(ctx, path, inner, out, blocks)?;
            out.push(Instr::Negate);
        },
        Ast::Stmt(_lc, Stmt::Where(_)) => todo!(),
        Ast::Stmt(_lc, Stmt::Command(command)) => {
            match command.cmd.as_str() {
                "cd" => {
                    if command.argv.len() != 1 {
                        panic!("TODO: handle cd getting wrong number of args");
                    }
                    compile_single(ctx, path, &command.argv[0], out, blocks)?;
                    out.push(Instr::ChangeDir);
                }
                command_str => {
                    let command_path = resolve(path, &command_str)
                        .ok_or_else(|| CompileError::CommandNotFound(command.lc, command_str.to_owned()))?;
                    for item in command.argv.iter().rev() {
                        compile_single(ctx.muffle(), path, item, out, blocks)?;
                    }
                    let argc = command.argv.len();
                    let orig = command_str.to_string();
                    out.push(Instr::Run {
                        muffle: ctx.muffle,
                        command: Command2 { orig, path: command_path, argc }
                    });
                },
            }
        },
        Ast::Stmt(_lc, Stmt::Pipeline(Pipeline { initial_value, items })) => {
            let is_simple = !items.iter().any(|c| matches!(c, PipelineItem::Sub(_)));

            let is_there_initial_value = initial_value.is_some();
            if let Some(token) = initial_value.clone() {
                compile_token(ctx, path, &token, out, blocks)?;
            }

            if is_simple {
                let instr = Instr::RunPipeline {
                    muffle: ctx.muffle,
                    is_there_initial_value,
                    items: items.into_iter()
                        .map(|c| match c {
                            PipelineItem::Command(c) => {
                                // FIXME: handle "cd" here
                                let orig = c.cmd.clone();
                                let command_path = resolve(path, &orig)
                                    .ok_or_else(|| CompileError::CommandNotFound(c.lc, orig.clone()))?;
                                for item in c.argv.iter().rev() {
                                    compile_single(ctx.muffle(), path, item, out, blocks)?;
                                }
                                let argc = c.argv.len();
                                Ok(RunPipelineItem::Command(Command2 { orig, argc, path: command_path }))
                            },
                            PipelineItem::Where(func) => {
                                if let Some(func) = func {
                                    let mut b = Block::new_internal();
                                    compile_ast(ctx, path, func, &mut b.contents, blocks)?;
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

            let mut b = Block::new_internal();
            compile_ast(ctx, path, ast, &mut b.contents, blocks)?;
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
