use std::path::PathBuf;
use clap::Parser;

pub mod cp {
    use super::*;

    #[derive(Parser)]
    #[command(name = "cp", version, about, long_about = None)]
    pub struct Cli {
        #[arg(required = true, value_name = "SOURCE")]
        pub source: Vec<PathBuf>,
        #[arg(required = true, value_name = "DEST")]
        pub dest: PathBuf,

        #[arg(short = 'a', long = "archive", help = "Preserve special files. Implies -rp.")]
        pub archive: bool,
        #[arg(short = 'f', long = "force", help = "Delete unopenable destination files if needed.")]
        pub force: bool,
        #[arg(short = 'i', long = "interactive", help = "Confirm before overwriting.")]
        pub interactive: bool,
        #[arg(short = 'p', long = "preserve", help = "Preserve file attributes.")]
        pub preserve: bool,
        #[arg(short = 'r', long = "recursive", help = "Copy recursively.")]
        pub recursive: bool,
        // #[arg(short = 'v', long = "verbose", help = "Print verbose logs.")]
        // pub verbose: bool,
        #[arg(short = 'H', help = "Follow SOURCE if it's a symbolic link.")]
        pub follow_h: bool,
        #[arg(short = 'L', help = "Always follow symbolic links in SOURCE.")]
        pub follow_l: bool,
        #[arg(short = 'P', help = "Never follow symbolic links in SOURCE.")]
        pub follow_p: bool,
    }
}

pub mod df {
    use super::*;

    #[derive(Parser)]
    #[command(name = "df", version, about, long_about = None)]
    pub struct Cli {
        #[arg(short = 'H', help = "Human-readable sizes.")]
        pub human: bool,
        #[arg(short = 'i', help = "Show inode information.")]
        pub inodes: bool,
        #[arg(short = 'T', help = "Show filesystem type.")]
        pub fstype: bool,
    }
}

pub mod du {
    use super::*;

    #[derive(Parser)]
    #[command(
        name = "du",
        version,
        about,
        long_about = None,
        disable_help_flag = true,
        allow_negative_numbers = true
    )]
    pub struct Cli {
        #[arg(long, action = clap::ArgAction::Help, help = "Print help.")]
        pub help: Option<bool>,

        #[arg(value_name = "FILE")]
        pub paths: Vec<PathBuf>,

        #[arg(short = 'a', long = "all", help = "Write counts for files, not just directories.")]
        pub all: bool,
        #[arg(short = 'c', long = "total", help = "Produce a grand total.")]
        pub total: bool,
        #[arg(short = 'h', long = "human-readable", help = "Print sizes in human-readable format.")]
        pub human: bool,
        #[arg(long = "inodes", help = "List inode usage instead of block usage.")]
        pub inodes: bool,
        #[arg(short = 's', long = "summarize", help = "Display only a total for each argument.")]
        pub summarize: bool,
        #[arg(
            short = 't',
            long = "threshold",
            value_name = "SIZE",
            help = "Exclude entries smaller than SIZE if positive, or larger than SIZE if negative."
        )]
        pub threshold: Option<String>,
        #[arg(long = "block-size", value_name = "SIZE", help = "Scale sizes by SIZE before printing.")]
        pub block_size: Option<String>,
        #[arg(
            short = 'd',
            long = "max-depth",
            value_name = "N",
            help = "Print the total for a directory only if it is N or fewer levels below the argument."
        )]
        pub max_depth: Option<usize>,
    }
}

pub mod ls {
    use super::*;

    #[derive(Parser)]
    #[command(name = "ls", version, about, long_about = None)]
    pub struct Cli {}
}

pub mod max {
    use super::*;

    #[derive(Parser)]
    #[command(name = "max", version, about, long_about = None)]
    pub struct Cli {
        #[arg(value_name = "NUMBER")]
        pub num: usize,
        #[arg(
            short = 'f',
            long = "field",
            value_name = "FIELD",
            help = "Which field (if a table) to rank by."
        )]
        pub field: Option<String>,
    }
}

pub mod ps {
    use super::*;

    #[derive(Parser)]
    #[command(name = "ps", version, about, long_about = None)]
    pub struct Cli {}
}
