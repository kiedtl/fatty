use std::fs;
use std::path::Path;

use clap::{Arg, Command};
use clap::CommandFactory as _;
use crate::cli;

fn find_arg<'a>(cmd: &'a Command, token: &str) -> Option<&'a Arg> {
    if let Some(long) = token.strip_prefix("--") {
        cmd.get_arguments().find(|a| a.get_long() == Some(long))
    } else if let Some(short) = token.strip_prefix('-') {
        if short.len() == 1 {
            let ch = short.chars().next()?;
            cmd.get_arguments().find(|a| a.get_short() == Some(ch))
        } else {
            None
        }
    } else {
        None
    }
}

fn arg_takes_value(arg: &Arg) -> bool {
    match arg.get_action() {
        clap::ArgAction::Set | clap::ArgAction::Append => true,
        _ => false,
    }
}

fn complete_paths(current_word: &str, completions: &mut Vec<String>) {
    let (dir_path, prefix) = if let Some(idx) = current_word.rfind('/') {
        let (d, p) = current_word.split_at(idx + 1);
        (d, p)
    } else {
        ("", current_word)
    };

    let scan_dir = if dir_path.is_empty() {
        Path::new(".")
    } else {
        Path::new(dir_path)
    };

    if let Ok(entries) = fs::read_dir(scan_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            // Ignore dotfiles unless prefix has a "."
            if name_str.starts_with('.') && !prefix.starts_with('.') {
                continue;
            }

            if name_str.starts_with(prefix) {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let completed = if is_dir {
                    format!("{dir_path}{name_str}/")
                } else {
                    format!("{dir_path}{name_str}")
                };
                completions.push(completed);
            }
        }
    }
}

// TODO
// - Don't suggest a one-time-use flag if it already appears in input
// - Flags can define their own completion mode for arguments (although files will remain
//   default...). Applies to positional arguments as well.
//   - Directory only
//   - File of a specific type, only
//     - Files tracked by git
//   - Array of strings
//   - Git branches

pub fn complete(command: &str, input: &str, cursor: usize) -> Vec<String> {
    let cursor = cursor.min(input.len());
    let mut safe_cursor = cursor;
    while safe_cursor > 0 && !input.is_char_boundary(safe_cursor) {
        safe_cursor -= 1;
    }
    let line = &input[..safe_cursor];

    let ends_with_ws = line.ends_with(|c: char| c.is_whitespace());
    let words: Vec<&str> = line.split_whitespace().collect();

    let mut completions = Vec::new();

    let mut cmd = match command {
        "cp" => cli::cp::Cli::command(),
        "df" => cli::df::Cli::command(),
        "du" => cli::du::Cli::command(),
        "ls" => cli::ls::Cli::command(),
        "max" => cli::max::Cli::command(),
        "ps" => cli::ps::Cli::command(),
        _ => return vec![],
    };
    cmd.build();

    let (preceding_args, current_word) = if ends_with_ws {
        (&words[..], "")
    } else {
        (&words[..words.len() - 1], *words.last().unwrap_or(&""))
    };

    // Suggest flags if current_word starts with '-'
    if current_word.starts_with('-') {
        for arg in cmd.get_arguments() {
            if let Some(long) = arg.get_long() {
                let flag = format!("--{long}");
                if flag.starts_with(current_word) {
                    completions.push(flag);
                }
            }
            if let Some(aliases) = arg.get_long_and_visible_aliases() {
                for alias in aliases {
                    let flag = format!("--{alias}");
                    if flag.starts_with(current_word) {
                        completions.push(flag);
                    }
                }
            }
            if let Some(short) = arg.get_short() {
                let flag = format!("-{short}");
                if flag.starts_with(current_word) {
                    completions.push(flag);
                }
            }
            if let Some(aliases) = arg.get_short_and_visible_aliases() {
                for alias in aliases {
                    let flag = format!("-{alias}");
                    if flag.starts_with(current_word) {
                        completions.push(flag);
                    }
                }
            }
        }
    } else {
        // Check if previous argument was a flag expecting a value
        if let Some(&prev_arg) = preceding_args.last() {
            if let Some(arg) = find_arg(&cmd, prev_arg) {
                if arg_takes_value(arg) {
                    for pv in arg.get_possible_values() {
                        if !pv.is_hide_set() && pv.get_name().starts_with(current_word) {
                            completions.push(pv.get_name().to_string());
                        }
                    }
                }
            }
        }

        // Subcommands
        for sub in cmd.get_subcommands() {
            if !sub.is_hide_set() {
                let name = sub.get_name();
                if name.starts_with(current_word) {
                    completions.push(name.to_string());
                }
            }
        }

        // Path / file completions
        complete_paths(current_word, &mut completions);
    }

    completions.sort();
    completions.dedup();
    completions
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();

    let input = &args[2];
    let completions = complete(
        &args[1], input,
        args.get(3).map(|s| s.parse::<usize>().unwrap()).unwrap_or(input.len()),
    );

    for item in completions {
        println!("{item}");
    }
}
