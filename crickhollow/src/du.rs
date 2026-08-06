use std::fmt;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use anyhow::{bail, Result};
use clap::Parser;
use dashmap::DashSet;
use rayon::prelude::*;

use bwine::{self, Value};

#[derive(Parser)]
#[command(version, about, long_about = None, disable_help_flag = true, allow_negative_numbers = true)]
struct Cli {
    #[arg(long, action = clap::ArgAction::Help, help = "Print help.")]
    help: Option<bool>,

    #[arg(value_name = "FILE")]
    paths: Vec<PathBuf>,

    #[arg(short = 'a', long = "all",            help = "Write counts for files, not just directories.")]
    all: bool,
    #[arg(short = 'c', long = "total",          help = "Produce a grand total.")]
    total: bool,
    #[arg(short = 'h', long = "human-readable", help = "Print sizes in human-readable format.")]
    human: bool,
    #[arg(             long = "inodes",         help = "List inode usage instead of block usage.")]
    inodes: bool,
    #[arg(short = 's', long = "summarize",      help = "Display only a total for each argument.")]
    summarize: bool,
    #[arg(short = 't', long = "threshold", value_name = "SIZE", help = "Exclude entries smaller than SIZE if positive, or larger than SIZE if negative.")]
    threshold: Option<String>,
    #[arg(            long = "block-size", value_name = "SIZE", help = "Scale sizes by SIZE before printing.")]
    block_size: Option<String>,
    #[arg(short = 'd', long = "max-depth", value_name = "N", help = "Print the total for a directory only if it is N or fewer levels below the argument.")]
    max_depth: Option<usize>,
}

#[derive(Copy, Clone)]
struct Opts {
    all: bool,
    human: bool,
    inodes: bool,
    summarize: bool,
    block_size: u64,
    max_depth: Option<usize>,
    threshold: Option<i64>,
}

pub fn main() {
    let args = Cli::parse();
    if let Err(e) = run(&args) {
        eprintln!("{e:?}");
    }
}

fn run(args: &Cli) -> Result<()> {
    let block_size = match &args.block_size {
        Some(s) => parse_size(s)?,
        None => 1024,
    };
    let threshold = match &args.threshold {
        Some(s) => Some(parse_threshold(s)?),
        None => None,
    };

    let opts = Opts {
        all: args.all,
        human: args.human,
        inodes: args.inodes,
        summarize: args.summarize,
        block_size,
        max_depth: args.max_depth,
        threshold,
    };

    let paths: &[PathBuf] =
        if args.paths.is_empty() {
            &[PathBuf::from(".")]
        } else {
            &args.paths
        };

    let (tx, rx) = mpsc::channel::<Dp>();
    let seen = DashSet::<(u64, u64)>::new();
    let mut gt = 0u64;

    if std::env::var_os("FATTY").is_some() {
        let mut bw = bwine::stdout_writer().unwrap();
        let mut stt = bwine::stream_table(&mut bw.0, ["size", "path"])?;

        for p in paths {
            gt += du(p, &opts, 0, &seen, tx.clone());
            while let Ok(Dp(value, path)) = rx.try_recv() {
                stt.row([Value::from(value), Value::Path(path.as_path().into())])?;
            }
        }

        if args.total {
            stt.row([Value::from(gt), Value::Null])?;
        }

        stt.end();
    } else {
        for p in paths {
            gt += du(p, &opts, 0, &seen, tx.clone());
            while let Ok(Dp(value, path)) = rx.try_recv() {
                println!("{}\t{}", Vs { value, opts: &opts }, path.display());
            }
        }

        if args.total {
            println!("{}\ttotal", Vs { value: gt, opts: &opts });
        }
    }

    Ok(())
}

struct Dp(u64, PathBuf);

struct Vs<'a> {
    value: u64,
    opts: &'a Opts,
}

impl fmt::Display for Vs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn human(f: &mut fmt::Formatter<'_>, n: u64) -> fmt::Result {
            let units = ["", "K", "M", "G", "T", "P", "E"];
            let mut v = n as f64;
            let mut i = 0;
            while v >= 1024.0 && i + 1 < units.len() {
                v /= 1024.0;
                i += 1;
            }
            if i == 0 { write!(f, "{n}") } else { write!(f, "{v:.1}{}", units[i]) }
        }

        let v = self.value;
        if self.opts.human {
            human(f, v)
        } else if self.opts.inodes {
            write!(f, "{v}")
        } else {
            write!(f, "{}", v.div_ceil(self.opts.block_size))
        }
    }
}


fn du(path: &Path, opts: &Opts, depth: usize, seen: &DashSet<(u64, u64)>, tx: Sender<Dp>) -> u64 {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("du: cannot stat '{}': {e}", path.display());
            return 0;
        }
    };

    let mut total = unit(&meta, opts);

    if (meta.is_dir() || meta.nlink() > 1) && !seen.insert((meta.ino(), meta.dev())) {
        return 0;
    }

    if meta.is_dir() {
        match fs::read_dir(path) {
            Ok(rd) => {
                let items: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                total += items
                    .par_iter()
                    .map(|e| du(e, opts, depth + 1, seen, tx.clone()))
                    .sum::<u64>();
            },
            Err(e) => eprintln!("du: cannot read '{}': {e}", path.display()),
        }
    }

    emit(total, path, &meta, opts, depth, tx);
    total
}

fn unit(meta: &Metadata, opts: &Opts) -> u64 {
    // Unit always 1 if counting inodes, else blocks*512
    if opts.inodes { 1 } else { meta.blocks() * 512 }
}

fn emit(total: u64, path: &Path, meta: &Metadata, opts: &Opts, depth: usize, tx: Sender<Dp>) {
    if !opts.all && !meta.is_dir() && depth != 0 {
        return;
    }

    // -s equiv to --max-depth=0
    let max_depth = if opts.summarize { Some(0) } else { opts.max_depth };
    if let Some(m) = max_depth && depth > m {
        return;
    }

    if let Some(t) = opts.threshold {
        let value = total as i64;
        if (t >= 0 && value < t) || (t < 0 && value > -t) {
            return;
        }
    }

    tx.send(Dp(total, path.to_path_buf())).unwrap();
}

// FIXME: KB -> x1 (because of ending in "B") rather than 1000.
fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => {
            let m = match c.to_ascii_uppercase() {
                'B' => 1,
                'K' => 1024,
                'M' => 1024u64.pow(2),
                'G' => 1024u64.pow(3),
                'T' => 1024u64.pow(4),
                'P' => 1024u64.pow(5),
                _ => bail!("invalid size suffix '{c}'"),
            };
            (&s[..s.len() - 1], m)
        }
        _ => (s, 1),
    };
    Ok(num.parse::<u64>()? * mult)
}

fn parse_threshold(s: &str) -> Result<i64> {
    match s.strip_prefix('-') {
        Some(rest) => Ok(-(parse_size(rest)? as i64)),
        None => Ok(parse_size(s)? as i64),
    }
}
