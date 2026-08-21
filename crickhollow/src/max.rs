// TODO: move this to fatty/bin, along with select

use std::time::{Instant, Duration};
use std::io::Read;

use anyhow::Result;
use clap::Parser;

use crate::bolger::Bolger;
use crate::cli::max::Cli;
use bwine::{self, Value, Token};

#[derive(Clone)]
struct Opts {
    num: usize,
    field: Option<String>,
}

pub fn main() {
    let args = Cli::parse();
    if let Err(e) = run(args) {
        eprintln!("{e:?}");
    }
}

fn run(Cli { num, field }: Cli) -> Result<()> {
    let opts = Opts { num, field };

    let mut b = Bolger::new();
    let mut r = 0;
    let mut t = Instant::now();
    let bolger_print = |b: &mut Bolger, h: &[Value], rows: &[Vec<Value>]| {
        b.begin("table");
        b.attr_str("id", "t");
        print!(" :columns [ ");
        for c in h {
            print!(" (column \"{c}\") ");
        }
        print!("]");
        for r in rows {
            b.begin("tr");
            for c in r {
                b.str(c.to_string());
            }
            b.end("tr");
        }
        b.end("table");
    };

    let bolger_print_sp = |b: &mut Bolger, r| {
        b.begin("row");
        b.attr_str("id", "sp");

        b.begin("spin");
        b.attr_num("tick", r);
        b.end("spin");

        b.str(" ");
        b.num(r);
        b.str(" rows");

        b.end("row");
    };

    let mut fd4 = bwine::Fd4::acquire().unwrap();
    let mut sr = bwine::StreamingReader::new();
    let mut buf = Vec::<u8>::new();
    let mut ast = Vec::new();
    let mut consumed = 0;

    #[derive(Debug)]
    enum S { V, H, PT, PA }
    let mut s = S::V;
    let mut h = vec![];
    let mut hp = usize::MAX;
    let mut ai = 0;

    let mut rows = Vec::new();

    while !sr.is_done() {
        let mut tmp = [0u8; 1024];
        let n = fd4.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[0..n]);

        while !sr.is_done() {
            let mut changed_something = false;

            match sr.read_once(&buf[consumed..], &mut ast) {
                Ok(nn) => consumed += nn,
                Err(e) if e.is_end_of_input() => break,
                Err(e) => {
                    eprintln!("Error: {e:?}");
                    return Ok(());
                },
            }

            if ai < ast.len() {
                match s {
                    S::V => {
                        match &ast[ai] {
                            Token::Table => {
                                s = S::H;
                                if opts.field.is_none() {
                                    eprintln!("Found a table without any field specified.");
                                    return Ok(());
                                }
                            }
                            Token::Array(_) => {
                                s = S::PA;
                                if opts.field.is_some() {
                                    eprintln!("Found an array, but a field was specified.");
                                    return Ok(());
                                }
                            }
                            _ => {
                                eprintln!("Expected table or array.");
                                return Ok(());
                            }
                        }
                        ai += 1;
                    }
                    S::H => {
                        if let Token::Array(_) = &ast[ai]
                            && let Some((ns, Value::Array(harr))) = Token::collect(&ast[ai..])
                        {
                            let Some(p) = harr.iter().position(|t| match t {
                                Value::Text(s) if s == opts.field.as_ref().unwrap() => true,
                                _ => false,
                            }) else {
                                eprintln!("Table doesn't have that field.");
                                return Ok(());
                            };
                            hp = p;
                            h = harr.into_owned();
                            ai += ns + 1; // Skip next Token::Array that begins rows.
                            s = S::PT;
                        }
                    }
                    S::PT => {
                        if let Token::Array(_) = &ast[ai]
                            && let Some((ns, Value::Array(row))) = Token::collect(&ast[ai..])
                        {
                            r += 1;
                            if rows.len() < opts.num {
                                rows.push(row.into_owned());
                                changed_something = true;
                            } else {
                                for p in 0..rows.len() {
                                    let gt = match (&row[hp], &rows[p][hp]) {
                                        (Value::Int(a), Value::Int(b)) => *a > *b,
                                        (Value::Float(a), Value::Float(b)) => *a > *b,
                                        _ => {
                                            eprintln!("Expected homogenous column of either Int or Float.");
                                            return Ok(());
                                        },
                                    };
                                    if gt {
                                        rows[p] = row.into_owned();
                                        changed_something = true;
                                        break;
                                    }
                                }
                            }
                            ai += ns;
                        }
                    },
                    S::PA => todo!(),
                }
            }

            if changed_something {
                bolger_print(&mut b, &h, &rows);
            }

            if t.elapsed() > Duration::from_millis(300) {
                bolger_print_sp(&mut b, r);
                t = Instant::now();
            }
        }

        buf.drain(..consumed);
        consumed = 0;
    }

    if false && std::env::var_os("FATTY").is_some() {
        let mut bw = bwine::stdout_writer().unwrap();
        Value::Table { header: h, rows, }.write(&mut bw.0).unwrap();
    } else if true {
        bolger_print(&mut b, &h, &rows);

        b.begin("row");
        b.attr_str("id", "sp");
        b.end("row");
    } else {
        use tabled::{builder::Builder, settings::{object::Rows, Color, Style}};
        let mut builder = Builder::from_iter(
            rows.into_iter().map(|r| r.into_iter().map(|v| v.to_string()))
        );
        builder.insert_record(0, h.into_iter().map(|v| v.to_string()));
        let mut table = builder.build();
        table.modify(Rows::first(), Color::BOLD);
        table.with(Style::empty());
        println!("{table}");
    }

    Ok(())
}
