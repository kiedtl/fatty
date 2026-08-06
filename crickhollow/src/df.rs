use std::fs;

use anyhow::Result;
use clap::Parser;
use nix::sys::statvfs::statvfs;

use bwine::{self, Value};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short = 'H', help = "Human-readable sizes.")]
    human: bool,
    #[arg(short = 'i', help = "Show inode information.")]
    inodes: bool,
    #[arg(short = 'T', help = "Show filesystem type.")]
    fstype: bool,
}

struct Mount {
    device: String,
    mountpoint: String,
    fstype: String,
}

pub fn main() {
    let args = Cli::parse();
    if let Err(e) = run(&args) {
        eprintln!("{e:?}");
    }
}

fn run(args: &Cli) -> Result<()> {
    let base = if args.human { Some(1024) } else { None };

    if std::env::var_os("FATTY").is_some() {
        let (h3, h4, h5, h6) = if args.inodes {
            ("inodes", "iused", "ifree", "iuse%")
        } else {
            (
                if base.is_some() { "hsize" } else { "size" },
                "used", "avail", "use%"
            )
        };

        let mut writer = bwine::stdout_writer().unwrap();
        let mut stt = bwine::stream_table(&mut writer.0, ["filesystem", "type", h3, h4, h5, h6, "mountpoint"]).unwrap();

        for m in readmounts()? {
            let Ok(st) = statvfs(m.mountpoint.as_str())
                else { continue };

            let fs = Value::text(m.device);
            let ty = if args.fstype { Value::text(m.fstype) } else { Value::Null };

            // third column is IFree (free) for inodes, but Avail (avail) for blocks
            let (total, used, third, avail) = if args.inodes {
                let total = st.files() as u64;
                let free = st.files_free() as u64;
                (total, total - free, free, st.files_available() as u64)
            } else {
                let bs = st.fragment_size() as u64;
                let total = st.blocks() as u64 * bs;
                let free = st.blocks_free() as u64 * bs;
                let avail = st.blocks_available() as u64 * bs;
                (total, total - free, avail, avail)
            };

            stt.row([
                fs, ty,
                Value::text(fmt(total, base, args.inodes)),
                Value::text(fmt(used, base, args.inodes)),
                Value::text(fmt(third, base, args.inodes)),
                Value::text(pct(used, used + avail)),
                Value::text(m.mountpoint),
            ])?;
        }

        stt.end();
    } else {
        let mut header = vec!["Filesystem".to_string()];
        if args.fstype { header.push("Type".into()); }
        if args.inodes {
            header.extend(["Inodes", "IUsed", "IFree", "IUse%"].map(String::from));
        } else {
            header.push(if base.is_some() { "Size".into() } else { "1K-blocks".into() });
            header.extend(["Used", "Avail", "Use%"].map(String::from));
        }
        header.push("Mounted on".into());

        let mut rows = vec![header];
        for m in readmounts()? {
            let Ok(st) = statvfs(m.mountpoint.as_str())
                else { continue; };

            let mut row = vec![m.device];
            if args.fstype { row.push(m.fstype); }

            // third column is IFree (free) for inodes, but Avail (avail) for blocks
            let (total, used, third, avail) = if args.inodes {
                let total = st.files() as u64;
                let free = st.files_free() as u64;
                (total, total - free, free, st.files_available() as u64)
            } else {
                let bs = st.fragment_size() as u64;
                let total = st.blocks() as u64 * bs;
                let free = st.blocks_free() as u64 * bs;
                let avail = st.blocks_available() as u64 * bs;
                (total, total - free, avail, avail)
            };

            row.push(fmt(total, base, args.inodes));
            row.push(fmt(used, base, args.inodes));
            row.push(fmt(third, base, args.inodes));
            row.push(pct(used, used + avail));
            row.push(m.mountpoint);
            rows.push(row);
        }

        for r in &rows {
            println!("{}", r.join("\t"));
        }
    }

    Ok(())
}

fn fmt(n: u64, base: Option<u64>, inodes: bool) -> String {
    match base {
        Some(b) => human(n, b),
        None if inodes => n.to_string(),
        None => (n / 1024).to_string(),
    }
}

fn human(n: u64, base: u64) -> String {
    let units = ["", "K", "M", "G", "T", "P", "E"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= base as f64 && i + 1 < units.len() {
        v /= base as f64;
        i += 1;
    }
    if i == 0 { n.to_string() } else { format!("{v:.1}{}", units[i]) }
}

fn pct(used: u64, total: u64) -> String {
    if total == 0 {
        "-".into()
    } else {
        format!("{}%", (used as u128 * 100).div_ceil(total as u128))
    }
}

fn readmounts() -> Result<Vec<Mount>> {
    let mut v = Vec::new();
    for line in fs::read_to_string("/proc/mounts")?.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 { continue; }
        v.push(Mount { device: f[0].into(), mountpoint: f[1].into(), fstype: f[2].into() });
    }
    Ok(v)
}
