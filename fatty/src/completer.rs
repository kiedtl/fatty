use std::fs;
use std::path::Path;

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

pub fn dumb(_command: &str, input: &str, cursor: usize) -> Vec<String> {
    let cursor = cursor.min(input.len());
    let mut safe_cursor = cursor;
    while safe_cursor > 0 && !input.is_char_boundary(safe_cursor) {
        safe_cursor -= 1;
    }
    let line = &input[..safe_cursor];

    let ends_with_ws = line.ends_with(|c: char| c.is_whitespace());
    let words: Vec<&str> = line.split_whitespace().collect();

    let (_preceding_args, current_word) = if ends_with_ws {
        (&words[..], "")
    } else {
        (&words[..words.len() - 1], *words.last().unwrap_or(&""))
    };

    if !current_word.starts_with('-') {
        let mut completions = Vec::new();
        complete_paths(current_word, &mut completions);
        completions.sort();
        completions.dedup();
        completions
    } else {
        vec![]
    }
}
