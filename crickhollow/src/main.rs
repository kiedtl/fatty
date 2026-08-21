use std::ffi::OsStr;
use std::path::Path;

mod bolger;
mod cli;
mod complete;
mod cp;
mod df;
mod du;
mod ls;
mod max;
mod ps;

fn main() {
    if let Some(argv0) = std::env::args_os().next() {
        let p = Path::new(&argv0);
        match p.file_name() {
            Some(f) if f == OsStr::new("cp") => cp::main(),
            Some(f) if f == OsStr::new("df") => df::main(),
            Some(f) if f == OsStr::new("du") => du::main(),
            Some(f) if f == OsStr::new("ls") => ls::main(),
            Some(f) if f == OsStr::new("ps") => ps::main(),
            Some(f) if f == OsStr::new("max") => max::main(),
            Some(f) if f == OsStr::new("crickhollow_complete") => complete::main(),
            Some(f) => eprintln!("Unknown file {f:?}."),
            None => eprintln!("Need argv[0]"),
        }
    } else {
        eprintln!("Need argv[0]");
    }
}
