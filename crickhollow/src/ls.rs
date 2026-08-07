// TODO: brandywine enum definitions

use std::fmt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::ffi::OsString;

use anyhow::Result;
use bwine::{self, Value};
use chrono::{DateTime, Local};
use clap::Parser;
use rustix::fs::FileType;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
}

pub fn main() {
    let args = Cli::parse();
    _ = args;

    let files = match readdir() {
        Ok(files) => files,
        Err(e) => {
            eprintln!("ls: {e:?}");
            std::process::exit(1);
        }
    };

    if std::env::var_os("FATTY").is_some() {
        let mut writer = bwine::stdout_writer().unwrap();
        let mut stt = bwine::stream_table(
            &mut writer.0,
            [
                "name", "type", "mode", "size", "links", "owner", "group",
                "modified", "accessed", "created",
            ],
        ).unwrap();
        for f in files {
            stt.row([
                Value::from(f.name.as_os_str()),
                Value::from(filetype_to_str(&f.kind)),
                Value::from(f.mode),
                Value::from(f.size),
                Value::from(f.links),
                Value::from(f.owner),
                Value::from(f.group),
                Value::from(f.modified),
                Value::from(f.accessed),
                Value::from(f.created),
            ]).unwrap();
        }
        stt.end();
    } else {
        use tabled::{builder::Builder, settings::{object::Rows, Color, Style}};
        let mut builder = Builder::from_iter(
            files.into_iter().map(
                |Entry {
                    mode, name, links,
                    owner, group,
                    size, kind,
                    accessed, modified, created
                }| [
                    Mode(mode).to_string(),
                    links.to_string(),
                    owner,
                    group,
                    size.to_string(),
                    accessed.to_rfc3339(),
                    modified.to_rfc3339(),
                    created.to_rfc3339(),
                    filetype_to_str(&kind).to_string(),
                    name.to_string_lossy().to_string(),
                ]
            )
        );
        builder.insert_record(0, [
            "mode", "links", "owner", "group", "size",
            "accessed", "modified", "created", "kind", "name"
        ]);
        let mut table = builder.build();
        table.modify(Rows::first(), Color::BOLD);
        table.with(Style::empty());
        println!("{table}");
    }
}

struct Entry {
    mode: u32,
    name: OsString,
    links: u64,
    owner: String,
    group: String,
    size: u64,
    kind: FileType,
    accessed: DateTime<Local>,
    modified: DateTime<Local>,
    created: DateTime<Local>,
}

fn readdir() -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for d in std::fs::read_dir(".")? {
        let d = d?;
        let met = d.metadata().unwrap();

        let mode = met.permissions().mode();
        let kind = FileType::from_raw_mode(mode);
        let owner = unsafe {
            let r = libc::getpwuid(met.uid());
            assert!(!r.is_null());
            std::ffi::CStr::from_ptr((*r).pw_name)
                .to_string_lossy()
                .to_string()
        };
        let group = unsafe {
            let r = libc::getgrgid(met.gid());
            assert!(!r.is_null());
            std::ffi::CStr::from_ptr((*r).gr_name)
                .to_string_lossy()
                .to_string()
        };
        let size = met.len();
        let name = d.file_name();

        let created: DateTime<Local> = met.created()?.into();
        let accessed: DateTime<Local> = met.accessed()?.into();
        let modified: DateTime<Local> = met.modified()?.into();

        let links = met.nlink();

        entries.push(Entry {
            mode, kind, size, links,
            owner, group, name,
            created, accessed, modified,
        });
    }
    entries.sort_by_key(|i| i.name.clone());
    Ok(entries)
}

struct Mode(pub u32);

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.0;

        let file_type = match m & 0o170000 {
            0o100000 => '-', // file
            0o040000 => 'd', // dir
            0o120000 => 'l', // symlink
            0o020000 => 'c', // char device (??)
            0o060000 => 'b', // block device
            0o010000 => 'p', // fifo/pipe
            0o140000 => 's', // socket
            _        => '?',
        };

        let bit = |mask: u32, ch: char| if m & mask != 0 { ch } else { '-' };

        let owner_x = match (m & 0o4000 != 0, m & 0o0100 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        let group_x = match (m & 0o2000 != 0, m & 0o0010 != 0) {
            (true,  true)  => 's',
            (true,  false) => 'S',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        let other_x = match (m & 0o1000 != 0, m & 0o0001 != 0) {
            (true,  true)  => 't',
            (true,  false) => 'T',
            (false, true)  => 'x',
            (false, false) => '-',
        };

        write!(f, "{}{}{}{}{}{}{}{}{}{}",
            file_type,
            bit(0o0400, 'r'), bit(0o0200, 'w'), owner_x,
            bit(0o0040, 'r'), bit(0o0020, 'w'), group_x,
            bit(0o0004, 'r'), bit(0o0002, 'w'), other_x,
        )
    }
}

fn filetype_to_str(f: &FileType) -> &'static str {
    match f {
        FileType::RegularFile => "file",
        FileType::Directory => "dir",
        FileType::Symlink => "sym",
        FileType::Fifo => "fifo",
        FileType::Socket => "sock",
        FileType::CharacterDevice => "char",
        FileType::BlockDevice => "blck",
        FileType::Unknown => "?",
    }
}
