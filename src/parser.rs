use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "src/grammar.pest"]
struct CommandParser;

pub fn parse_command(input: &str) -> Result<(String, Vec<String>), String> {
    let pairs = CommandParser::parse(Rule::command, input)
        .map_err(|e| e.to_string())?;

    let tokens: Vec<String> = pairs
        .into_iter()
        .next()
        .unwrap()
        .into_inner()
        .filter(|p| p.as_rule() != Rule::EOI)
        .map(|pair| match pair.as_rule() {
            Rule::single_quoted => {
                let s = pair.as_str();
                s[1..s.len() - 1].to_string()
            }
            Rule::double_quoted => {
                let s = pair.as_str();
                unescape_dq(&s[1..s.len() - 1])
            }
            Rule::unquoted => unescape_unquoted(pair.as_str()),
            _ => unreachable!(),
        })
        .collect();

    let mut iter = tokens.into_iter();
    let cmd = iter.next().unwrap_or_default();
    Ok((cmd, iter.collect()))
}

fn unescape_dq(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn unescape_unquoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() { out.push(next); }
        } else {
            out.push(ch);
        }
    }
    out
}

mod tests {
    use super::*;

    #[test]
    pub fn basic() {
        macro_rules! c {
            ($cmd:literal $(, $arg:literal)*) => {
                Ok(($cmd.to_string(), vec![$($arg.to_string(),)*]))
            }
        }

        assert_eq!(c!("ls"),                parse_command("ls"));
        assert_eq!(c!("ls", "test"),        parse_command("ls test"));
        assert_eq!(c!("ls", "test"),        parse_command("ls 'test'"));
        assert_eq!(c!("ls", "test"),        parse_command("ls \"test\""));
        assert_eq!(c!("ls", "test"),        parse_command("ls \"test\""));
        assert_eq!(c!("ls", "a", "b", "c"), parse_command("ls a \"b\" 'c'"));
    }
}
