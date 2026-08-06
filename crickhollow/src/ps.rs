use std::fs;
use std::fmt;

use anyhow::{bail, Result, Context};
use bwine::{self, Value};
use clap::Parser;
use libc::pid_t;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
}

pub fn main() {
    let args = Cli::parse();
    _ = args;

    let procs = match readprocs() {
        Ok(procs) => procs,
        Err(e) => {
            eprintln!("ps: {e:?}");
            std::process::exit(1);
        }
    };

    if std::env::var_os("FATTY").is_some() {
        let mut writer = bwine::stdout_writer().unwrap();
        let mut stt = bwine::stream_table(
            &mut writer.0,
            ["pid", "state", "ppid", "utime", "stime", "maj", "min", "tcomm"],
        ).unwrap();
        for proc in procs {
            let (maj, min) = proc.majmin();
            stt.row([
                Value::from(proc.pid as usize),
                Value::from(proc.state.to_str()),
                Value::from(proc.ppid as usize),
                Value::from(proc.utime),
                Value::from(proc.stime),
                Value::from(maj),
                Value::from(min),
                Value::from(proc.tcomm),
            ]).unwrap();
        }
        stt.end();
    } else {
        for proc in procs {
            let (maj, min) = proc.majmin();
            println!("{}\t{}\t{}\t{}\t{}\t{}\t{}:{}", proc.pid, proc.state, proc.ppid, proc.utime, proc.stime, maj, min, proc.tcomm);
        }
    }
}

enum State {
    Running,
    Sleeping,
    SleepingD, // Sleeping with uninterruptible wait
    Zombie,
    Stopped,
    TracingStopped,
    IdleKernel,
}

impl State {
    fn to_str(&self) -> &'static str {
        match self {
            State::Running => "Running",
            State::Sleeping => "Sleeping",
            State::SleepingD => "SleepingD",
            State::Zombie => "Zombie",
            State::Stopped => "Stopped",
            State::TracingStopped => "TracingStopped",
            State::IdleKernel => "IdleKernel",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            State::Running => "R",
            State::Sleeping => "S",
            State::SleepingD => "D",
            State::Zombie => "Z",
            State::Stopped => "T",
            State::TracingStopped => "t",
            State::IdleKernel => "I",
        })
    }
}

struct Process {
    pid: pid_t,
    tcomm: String,
    state: State,
    ppid: pid_t,
    utime: usize,
    stime: usize,
    tty_nr: usize,
}

impl Process {
    fn majmin(&self) -> (usize, usize) {
        (
            (self.tty_nr >> 8) & 0xff,
            (self.tty_nr & 0xff) | ((self.tty_nr >> 12) & !0xff)
        )
    }
}

// /proc/PID/stat fields:
//     0: pid
//     1: tcomm
//     2: state
//     3: ppid
//     4: pgrp: process group
//     5: sid: session id
//     6: tty_nr: processes' tty
//     7: tty_pgrp: pgrp of the tty
//     8: flags: task flags
//     9: min_flt: minor faults
//     10: cmin_flt: minor faults with child's
//     11: maj_flt: major faults
//     12: cmaj_flt: major faults with child's
// https://github.com/torvalds/linux/blob/master/Documentation/filesystems/proc.rst
fn readprocs() -> Result<Vec<Process>> {
    let mut b = Vec::new();
    for item in fs::read_dir("/proc/")? {
        let Ok(item) = item else { continue; };
        if item.file_type()?.is_dir() && let Ok(pid) = item.file_name().to_string_lossy().parse::<pid_t>() {
            let statsp = item.path().join("stat");
            if statsp.exists() {
                let mut contents = fs::read_to_string(&statsp)?;

                let after_tcomm_ind = contents.find(")")
                    .with_context(|| format!("{}: Couldn't parse tcomm/pid", statsp.display()))?;
                let tcomm_end_paren = contents.split_off(after_tcomm_ind);

                let tcomm_beginning = contents.find("(")
                    .with_context(|| format!("{}: Couldn't parse tcomm/pid", statsp.display()))?;
                let tcomm = contents[tcomm_beginning + 1..].to_owned();

                let after_tcomm = &tcomm_end_paren[2..];
                let stats = after_tcomm.split(" ").collect::<Vec<_>>();
                let state = match stats[0] {
                    "R" => State::Running,
                    "S" => State::Sleeping,
                    "D" => State::SleepingD,
                    "Z" => State::Zombie,
                    "T" => State::Stopped,
                    "t" => State::TracingStopped,
                    "I" => State::IdleKernel,
                    c => bail!("{}: Unknown process state {}", statsp.display(), c),
                };
                let ppid = stats[2].parse()
                    .with_context(|| format!("{}: Couldn't parse ppid", statsp.display()))?;
                let tty_nr = stats[2].parse()
                    .with_context(|| format!("{}: Couldn't parse ppid", statsp.display()))?;
                let utime = stats[11].parse()
                    .with_context(|| format!("{}: Couldn't parse ppid", statsp.display()))?;
                let stime = stats[12].parse()
                    .with_context(|| format!("{}: Couldn't parse ppid", statsp.display()))?;

                b.push(Process { pid, tcomm, state, ppid, utime, stime, tty_nr });
            }
        }
    }
    Ok(b)
}
