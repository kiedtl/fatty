// The long-term plan is to have functionality like this built into the fatty shell.

use std::io::Read;

use anyhow::{anyhow, bail, Result};
use clap::Parser;

use bwine::{self, Value, Token};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "EXPRESSIONS", value_parser = parse_expr)]
    exprs: Vec<Expr>,
}

#[derive(Debug, Clone)]
enum Seg {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone)]
struct Expr {
    raw: String, // Orig expr string, used for output column header
    segments: Vec<Seg>,
}

pub fn main() {
    let args = Cli::parse();
    if let Err(e) = run(args) {
        eprintln!("{e:?}");
    }
}

fn parse_expr(input: &str) -> Result<Expr, String> {
    let rest = input
        .strip_prefix('.')
        .ok_or_else(|| format!("expr `{input}` must be prefixed with `.`"))?;

    if rest.is_empty() {
        return Err(format!("expr is empty"));
    }

    let mut segments = Vec::new();
    for part in rest.split('.') {
        if part.is_empty() {
            return Err(format!("expr `{input}` has empty path segment"));
        }

        segments.push(match part.parse::<usize>() {
            Ok(i) => Seg::Index(i),
            Err(_) => Seg::Field(part.to_string()),
        });
    }

    // Unreachable due to previous checks?
    if segments.is_empty() {
        return Err(format!("empty exprs are not supported."));
    }

    Ok(Expr { raw: input.to_string(), segments })
}

enum S {
    Init, // Expected initial token
    TableHeader, // Expecting full table header, or waiting for it to arrive completely
    TableRowsOpen, // Expected rows array to open
    TableRow, // Expected next row, or end of table
    ArrayItem, // Expecting next array element
    Fin,
}

fn run(Cli { exprs }: Cli) -> Result<()> {
    let mut fd4 = bwine::Fd4::acquire().unwrap();
    let mut sr = bwine::StreamingReader::new();
    let mut buf = Vec::<u8>::new();
    let mut ast = Vec::new();
    let mut consumed = 0;
    let mut ai = 0;
    let mut h = vec![];
    let mut bw = bwine::stdout_writer().unwrap();
    let mut s = S::Init;

    let header_labels: Vec<String> = exprs.iter().map(|e| e.raw.clone()).collect();
    let mut out = bwine::stream_table(&mut bw.0, header_labels)
        .map_err(|e| anyhow!("select: couldn't write the output header: {e:?}"))?;

    while !sr.is_done() {
        let mut tmp = [0u8; 1024];
        let n = fd4.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[0..n]);

        while !sr.is_done() {
            match sr.read_once(&buf[consumed..], &mut ast) {
                Ok(nn) => consumed += nn,
                Err(e) if e.is_end_of_input() => break,
                Err(e) => {
                    eprintln!("Error: {e:?}");
                    return Ok(());
                },
            }
        }

        'l: loop {
            match s {
                S::Init => match ast.get(ai) {
                    None => break 'l,
                    Some(Token::Table) => { s = S::TableHeader; ai += 1; }
                    Some(Token::Array(_)) => { s = S::ArrayItem; ai += 1; }
                    Some(other) => bail!(
                        "select needs a table, array, or map at the top level, found {other:?}"
                    ),
                },

                S::TableHeader => match ast.get(ai) {
                    None => break 'l,
                    Some(Token::Array(_)) => match Token::collect(&ast[ai..]) {
                        Some((ns, Value::Array(items))) => {
                            h = items;
                            ai += ns;
                            s = S::TableRowsOpen;
                        }
                        Some(_) => bail!("select: malformed table header"),
                        None => break 'l,
                    },
                    Some(other) => bail!("select: expected a table header, found {other:?}"),
                },

                S::TableRowsOpen => match ast.get(ai) {
                    None => break 'l,
                    Some(Token::Array(_)) => { ai += 1; s = S::TableRow; }
                    Some(other) => bail!("select: expected the table's rows, found {other:?}"),
                },

                S::TableRow => match ast.get(ai) {
                    None => break 'l,
                    Some(Token::End) => { ai += 1; s = S::Fin; }
                    Some(Token::Array(_)) => match Token::collect(&ast[ai..]) {
                        Some((ns, Value::Array(row))) => {
                            ai += ns;
                            let values: Vec<Value<'static>> = exprs
                                .iter()
                                .map(|e| select_from_row(&h, &row, e))
                                .collect();
                            out.row(values)
                                .map_err(|e| anyhow::anyhow!("select: couldn't write a row: {e:?}"))?;
                            ast.drain(0..ai);
                            ai = 0;
                        }
                        Some(_) => bail!("select: malformed table row"),
                        None => break 'l,
                    },
                    Some(other) => bail!("select: expected a table row, found {other:?}"),
                },

                S::ArrayItem => match ast.get(ai) {
                    None => break 'l,
                    Some(Token::End) => { ai += 1; s = S::Fin; }
                    Some(_) => match Token::collect(&ast[ai..]) {
                        Some((ns, item)) => {
                            ai += ns;
                            let values: Vec<Value<'static>> = exprs
                                .iter()
                                .map(|e| nav(&item, &e.segments).unwrap_or(Value::Null))
                                .collect();
                            out.row(values)
                                .map_err(|e| anyhow::anyhow!("select: couldn't write a row: {e:?}"))?;
                            ast.drain(0..ai);
                            ai = 0;
                        }
                        None => break 'l,
                    },
                },
                S::Fin => break 'l,
            }
        }

        buf.drain(..consumed);
        consumed = 0;
    }

    if !matches!(s, S::Fin) {
        eprintln!("select: input ended prematurely");
    }

    out.end();
    Ok(())
}

fn nav(value: &Value<'static>, segments: &[Seg]) -> Option<Value<'static>> {
    let Some((seg, rest)) = segments.split_first() else {
        return Some(value.clone());
    };

    match value {
        Value::Tag(_, inner) => nav(inner, segments),
        Value::Array(items) => match seg {
            Seg::Index(i) => items.get(*i).and_then(|v| nav(v, rest)),
            Seg::Field(_) => None,
        },
        Value::Table { rows, .. } => match seg {
            Seg::Index(i) => rows
                .get(*i)
                .map(|row| Value::Array(row.clone()))
                .and_then(|row| nav(&row, rest)),
            Seg::Field(_) => None,
        },
        _ => None,
    }
}

/// Selects `expr` out of one table row. The first segment picks a column — by
/// name against `header`, or by position if it's numeric — then any remaining
/// segments navigate into that cell as usual.
fn select_from_row(header: &[Value<'static>], row: &[Value<'static>], expr: &Expr) -> Value<'static> {
    let Some((first, rest)) = expr.segments.split_first() else {
        return Value::Array(row.to_vec());
    };

    let idx = match first {
        Seg::Index(i) => Some(*i),
        Seg::Field(name) => header
            .iter()
            .position(|h| matches!(h, Value::Text(s) if s.as_ref() == name.as_str())),
    };

    idx.and_then(|i| row.get(i))
        .and_then(|cell| nav(cell, rest))
        .unwrap_or(Value::Null)
}
