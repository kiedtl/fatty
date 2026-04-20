use std::fs;
use std::ffi::OsStr;
use std::io::{self, Read, BufReader, BufWriter};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt, FileTypeExt};
use std::os::linux::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result, Context};
use clap::Parser;
use libc::{ ENOENT, S_ISUID, S_ISGID };
use nix::{
    fcntl::{AT_FDCWD, AtFlags},
    sys::stat::{mknod, SFlag, Mode, utimensat, UtimensatFlags},
    sys::time::TimeSpec,
    unistd::{chown, fchownat, Uid, Gid},
};

use crate::bolger::Bolger;

macro_rules! confirm {
    ($fmt:literal $(, $arg:expr)*) => {{
        eprintln!($fmt, $($arg,)*);
        read_confirm()
    }}
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(required = true, value_name = "SOURCE")]
    source: Vec<PathBuf>,
    #[arg(required = true, value_name = "DEST")]
    dest: PathBuf,

    #[arg(short = 'a', long = "archive",     help = "Preserve special files. Implies -rp.")]
    archive: bool,
    #[arg(short = 'f', long = "force",       help = "Delete unopenable destination files if needed.")]
    force: bool,
    #[arg(short = 'i', long = "interactive", help = "Confirm before overwriting.")]
    interactive: bool,
    #[arg(short = 'p', long = "preserve",    help = "Preserve file attributes.")]
    preserve: bool,
    #[arg(short = 'r', long = "recursive",   help = "Copy recursively.")]
    recursive: bool,
    // #[arg(short = 'v', long = "verbose",     help = "Print verbose logs.")]
    // verbose: bool,
    #[arg(short = 'H',                       help = "Follow SOURCE if it's a symbolic link.")]
    follow_h: bool,
    #[arg(short = 'L',                       help = "Always follow symbolic links in SOURCE.")]
    follow_l: bool,
    #[arg(short = 'P',                       help = "Never follow symbolic links in SOURCE.")]
    follow_p: bool,
}

#[derive(Copy, Clone)]
struct CopyOpts {
    archive: bool,
    force: bool,
    interactive: bool,
    preserve: bool,
    recursive: bool,
    //verbose: bool,
    follow: Follow,
}

#[derive(Copy, Clone)]
enum Follow {
    P, // Preserve symbolic links
    H, // Dereference source if it's a symbolic link.
    L, // Dereference all symbolic links
}

pub fn main() {
    let args = Cli::parse();
    let mut b = Bolger::new();

    let recursive = args.recursive || args.archive;
    let follow =
        if args.follow_l {
            Follow::L
        } else if args.follow_h {
            Follow::H
        } else if args.follow_p || recursive {
            Follow::P
        } else {
            Follow::L
        };

    let opts = CopyOpts {
        archive: args.archive,
        force: args.force,
        interactive: args.interactive,
        preserve: args.preserve || args.archive,
        recursive,
        // verbose: args.verbose,
        follow,
    };

    b.begin("table");
    b.attr_str("id", "t");
    print!(" :columns [ (column \"src\") (column \"dest\") (column \"status\") ]");
    b.end("table");

    let mut is_success = true;
    if args.source.len() == 1 && !args.dest.is_dir() {
        is_success = apply(&mut b, &args.source[0], &args.dest, cp, 0, opts) && is_success;
    } else {
        for source in &args.source {
            let dest = args.dest.join(source.file_name().unwrap_or(OsStr::new("")));
            is_success = apply(&mut b, source, &dest, cp, 0, opts) && is_success;
        }
    }

    if !is_success {
        std::process::exit(1);
    }
}

// Returns true if successful, false otherwise.
fn apply<O>(bolger: &mut Bolger, a: &Path, b: &Path, func: fn(&mut Bolger, &Path, &Path, usize, O) -> Result<()>, depth: usize, opts: O) -> bool {
    if let Ok(a_met) = fs::metadata(a) && let Ok(b_met) = fs::metadata(b)
        && a_met.dev() == b_met.dev()
        && a_met.ino() == b_met.ino()
    {
        //eprintln!("{} -> {}: same file", a.display(), b.display());
        bolger.str("same file");
        return true;
    }

    let result = (func)(bolger, a, b, depth, opts);

    bolger.begin("row");
    bolger.attr_str("for", "t");
    bolger.str(a.display());
    bolger.str(b.display());

    if let Err(e) = result {
        //eprintln!("{} -> {}: {e}", a.display(), b.display());
        bolger.str(e);
        bolger.end("row");
        false
    } else {
        bolger.str("ok");
        bolger.end("row");
        true
    }
}

fn cp(b: &mut Bolger, s1: &Path, s2: &Path, depth: usize, opts: CopyOpts) -> Result<()> {
    let mut cperr = None;

    let met = match opts.follow {
        Follow::P => fs::symlink_metadata(s1)?,
        Follow::H if depth > 0 => fs::symlink_metadata(s1)?,
        _ => fs::metadata(s1)?,
    };
    let perms = met.permissions();
    let mode = perms.mode();
    let ft = met.file_type();

    if opts.interactive && s2.exists() && !confirm!("overwrite '{}'?", s2.display()) {
        return Ok(());
    }

    // if opts.verbose {
    //     println!("{} -> {}", s1.display(), s2.display());
    // }

    if ft.is_symlink() {
        let target = fs::read_link(s1)
            .with_context(|| format!("readlink {}", s1.display()))?;

        if opts.force {
            if let Err(e) = fs::remove_file(s2) && e.raw_os_error() != Some(ENOENT) {
                bail!("unlink {}: {}", s2.display(), e);
            }
        }

        if let Err(e) = std::os::unix::fs::symlink(&target, s2) {
            eprintln!("symlink {} -> {}: {}", s2.display(), target.display(), e);
            cperr = Some(Err(e.into()));
        }
    } else if met.is_dir() {
        if !opts.recursive {
            bail!("{} is a directory", s1.display());
        }

        let entries = fs::read_dir(s1)
            .with_context(|| format!("couldn't open '{}'", s1.display()))?;

        if let Err(e) = fs::create_dir(s2) && e.raw_os_error() != Some(libc::EEXIST) {
            bail!("mkdir {}: {}", s2.display(), e);
        } else {
            let _ = fs::set_permissions(s2, perms);
        }

        for entry in entries {
            let name = entry?.file_name();
            let ns1 = s1.join(&name);
            let ns2 = s2.join(&name);
            let _ = apply(b, &ns1, &ns2, cp, depth + 1, opts);
        }
    } else if opts.archive && (ft.is_block_device() || ft.is_char_device() || ft.is_socket() || ft.is_fifo()) {
        if opts.force {
            if let Err(e) = fs::remove_file(s2) && e.raw_os_error() != Some(ENOENT) {
                bail!("unlink {}: {}", s2.display(), e);
            }
        }

        mknod(s2, SFlag::from_bits(mode).unwrap(), Mode::from_bits(mode).unwrap(), met.dev())?;
    } else {
        let f1 = fs::File::open(s1)
            .with_context(|| format!("open {}", s1.display()))?;

        let mut f2_opt = fs::OpenOptions::new().write(true).create(true).truncate(true).open(s2);

        if f2_opt.is_err() && opts.force {
            if let Err(e) = fs::remove_file(s2) && e.raw_os_error() != Some(ENOENT) {
                bail!("unlink {}: {}", s2.display(), e);
            }
            f2_opt = fs::OpenOptions::new().write(true).create(true).truncate(true).open(s2);
        }

        let f2 = f2_opt
            .with_context(|| format!("open {}", s2.display()))?;
        let _ = f2.set_permissions(perms);

        let mut reader = ProgressReader::new(BufReader::new(f1), met.size(), b);
        let mut writer = BufWriter::new(f2);
        io::copy(&mut reader, &mut writer)?;
    }

    // Preserve timestamps and ownership (-a or -p)
    if opts.archive || opts.preserve {
        let atime = TimeSpec::new(met.st_atime(), met.st_atime_nsec());
        let mtime = TimeSpec::new(met.st_mtime(), met.st_mtime_nsec());
        if let Err(e) = utimensat(AT_FDCWD, s2, &atime, &mtime, UtimensatFlags::NoFollowSymlink) {
            eprintln!("couldn't change a/mtime on {}: {e:#}", s2.display());
            cperr = Some(Err(e.into()));
        }

        let uid = Uid::from_raw(met.st_uid());
        let gid = Gid::from_raw(met.st_gid());

        if !ft.is_symlink() {
            if let Err(e) = chown(s2, Some(uid), Some(gid)) {
                // clear setuid/setgid bits on chown failure
                let stripped = mode & !(S_ISUID | S_ISGID);
                let _ = fs::set_permissions(s2, fs::Permissions::from_mode(stripped));

                eprintln!("chown {}: {e:#}", s2.display());
                cperr = Some(Err(e.into()));
            } else if let Err(e) = fs::set_permissions(s2, fs::Permissions::from_mode(mode)) {
                eprintln!("chmod {}: {e:#}", s2.display());
                cperr = Some(Err(e.into()));
            }
        } else {
            if let Err(e) = fchownat(AT_FDCWD, s2, Some(uid), Some(gid), AtFlags::AT_SYMLINK_NOFOLLOW) {
                eprintln!("lchown {}: {e}", s2.display());
                cperr = Some(Err(e.into()));
            }
        }
    }

    cperr.unwrap_or(Ok(()))
}

fn read_confirm() -> bool {
    let mut line = String::new();
    io::stdin().read_line(&mut line).unwrap_or(0);
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

struct ProgressReader<'a, R> {
    inner: R,
    cur: usize,
    total: u64,
    b: &'a mut Bolger,
}

impl<'a, R: Read> ProgressReader<'a, R> {
    fn new(inner: R, total: u64, b: &'a mut Bolger) -> Self {
        Self { inner, cur: 0, total, b }
    }
}

impl<'a, R: Read> Read for ProgressReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.cur += n;

        self.b.begin("progress");
        self.b.attr_str("id", "p");
        self.b.attr_num("done", self.cur as f64);
        self.b.attr_num("max", self.total as f64);
        self.b.end("progress");

        Ok(n)
    }
}
