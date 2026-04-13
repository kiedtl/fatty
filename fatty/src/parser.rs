use itertools::Itertools;

use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "src/grammar.pest"]
struct CommandParser;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    String(String),
    Word(String),
}

impl Token {
    pub fn to_string(&self) -> String {
        match self {
            Token::String(s) => s.clone(),
            Token::Word(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub argv: Vec<Token>,
}

impl Command {
    pub fn to_string(&self) -> String {
        self.argv
            .iter()
            .map(|t| t.to_string())
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    pub items: Vec<SubOrCommand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub first: Box<Stmt>,
    pub tokens: Vec<(Connector, Stmt)>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Connector {
    And,
    Or
}

#[derive(Debug, Clone, PartialEq)]
pub enum SubOrCommand {
    Command(Command),
    Sub(Box<Ast>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Chain(Chain),
    Pipeline(Pipeline),
    Sub(Box<Ast>),
    Background(Box<Ast>),
    Command(Command),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Stmt(Stmt),
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

fn parse_command<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Command, String> {
    let mut argv = Vec::new();
    for pair in pairs {
        match pair.as_rule() {
            Rule::single_quoted => argv.push(Token::String(pair.as_str().to_owned())),
            Rule::double_quoted => argv.push(Token::String(unescape_dq(pair.as_str()))),
            Rule::unquoted => argv.push(Token::Word(unescape_unquoted(pair.as_str()))),
            _ => unreachable!(),
        }
    }
    Ok(Command { argv })
}

fn parse_ast<'a>(pair: Pair<'a, Rule>) -> Result<Ast, String> {
    Ok(match pair.as_rule() {
        Rule::program => unreachable!(),

        Rule::stmt => parse_ast(pair.into_inner().next().unwrap())?,
        Rule::background => Ast::Stmt(Stmt::Background(Box::new(parse_ast(pair.into_inner().next().unwrap())?))),
        Rule::chain => todo!(),
        Rule::pipeline => {
            let mut items = Vec::new();

            for pair in pair.into_inner() {
                match parse_ast(pair)? {
                    Ast::Stmt(Stmt::Command(c)) => items.push(SubOrCommand::Command(c)),
                    Ast::Stmt(Stmt::Sub(s)) => items.push(SubOrCommand::Sub(s)),
                    _ => unreachable!(),
                }
            }

            Ast::Stmt(Stmt::Pipeline(Pipeline { items }))
        },
        Rule::sub => Ast::Stmt(Stmt::Sub(Box::new(parse_ast(pair.into_inner().next().unwrap())?))),
        Rule::command => Ast::Stmt(Stmt::Command(parse_command(pair.into_inner())?)),

        Rule::token => unreachable!(),
        Rule::single_quoted => unreachable!(),
        Rule::double_quoted => unreachable!(),
        Rule::unquoted => unreachable!(),
        Rule::connector => unreachable!(),

        Rule::WHITESPACE => unreachable!(),
        Rule::EOI => unreachable!(),
    })
}

fn parse_list<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Vec<Ast>, String> {
    let mut ast = Vec::new();

    for pair in pairs {
        match pair.as_rule() {
            Rule::EOI => (),
            _ => ast.push(parse_ast(pair)?),
        }
    }

    Ok(ast)
}

pub fn parse_str(input: &str) -> Result<Vec<Ast>, String> {
    let mut pairs = CommandParser::parse(Rule::program, input)
        .map_err(|e| e.to_string())?;

    let pair = pairs.next().unwrap();
    let ast = match pair.as_rule() {
        Rule::program => parse_list(pair.into_inner()),
        _ => unreachable!(),
    };

    assert_eq!(None, pairs.next());
    ast
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
