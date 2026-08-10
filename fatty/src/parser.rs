use itertools::Itertools;

use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "src/grammar.pest"]
struct CommandParser;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LineCol(pub usize, pub usize, pub usize);

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

    pub fn as_str(&self) -> &str {
        match self {
            Token::String(s) => s,
            Token::Word(s) => s,
        }
    }
}

// #[derive(Debug, Clone, PartialEq)]
// pub struct Query {
//     pub lc: LineCol,
//     pub items: Vec<Token>,
// }

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub lc: LineCol,
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
    pub items: Vec<PipelineItem>,
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
pub enum PipelineItem {
    Command(Command),
    Sub(Box<Ast>),
    Where(Option<Box<Ast>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Chain(Chain),
    Pipeline(Pipeline),
    Sub(Box<Ast>),
    Background(Box<Ast>),
    Command(Command),
    Where(Option<Box<Ast>>),
    // Query(Query),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Stmt(LineCol, Stmt),
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

fn parse_tokens<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<(LineCol, Vec<Token>), String> {
    let mut argv = Vec::new();
    let mut lc = None;
    for pair in pairs {
        if lc.is_none() {
            let l = pair.line_col().0;
            let s = pair.as_span();
            lc = Some(LineCol(l, s.start(), s.end()));
        }

        match pair.as_rule() {
            Rule::single_quoted => argv.push(Token::String(pair.as_str().to_owned())),
            Rule::double_quoted => argv.push(Token::String(unescape_dq(pair.as_str()))),
            Rule::unquoted => argv.push(Token::Word(unescape_unquoted(pair.as_str()))),
            _ => unreachable!(),
        }
    }

    let lc = lc.unwrap();
    Ok((lc, argv))
}

fn parse_command<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Command, String> {
    let (lc, argv) = parse_tokens(pairs)?;
    Ok(Command { lc, argv })
}

// fn parse_query<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Query, String> {
//     let (lc, items) = parse_tokens(pairs)?;
//     Ok(Query { lc, items })
// }

fn parse_ast<'a>(pair: Pair<'a, Rule>) -> Result<Ast, String> {
    let l = pair.line_col().0;
    let s = pair.as_span();
    let lc = LineCol(l, s.start(), s.end());

    Ok(match pair.as_rule() {
        Rule::program => unreachable!(),

        Rule::stmt => parse_ast(pair.into_inner().next().unwrap())?,
        Rule::background => Ast::Stmt(lc, Stmt::Background(Box::new(parse_ast(pair.into_inner().next().unwrap())?))),
        Rule::chain => todo!(),
        Rule::pipeline => {
            let mut items = Vec::new();

            for pair in pair.into_inner() {
                match parse_ast(pair)? {
                    Ast::Stmt(_, Stmt::Command(c)) => items.push(PipelineItem::Command(c)),
                    Ast::Stmt(_, Stmt::Sub(s)) => items.push(PipelineItem::Sub(s)),
                    Ast::Stmt(_, Stmt::Where(s)) => items.push(PipelineItem::Where(s)),
                    // Ast::Stmt(_, Stmt::Query(q)) => items.push(PipelineItem::Query(q)),
                    _ => unreachable!(),
                }
            }

            Ast::Stmt(lc, Stmt::Pipeline(Pipeline { items }))
        },
        // Rule::query => Ast::Stmt(lc, Stmt::Query(parse_query(pair.into_inner())?)),
        Rule::sub => Ast::Stmt(lc, Stmt::Sub(Box::new(parse_ast(pair.into_inner().next().unwrap())?))),
        Rule::s_where => {
            let s = if let Some(sub) = pair.into_inner().next() {
                Some(Box::new(parse_ast(sub)?))
            } else {
                None
            };
            Ast::Stmt(lc, Stmt::Where(s))
        }
        Rule::command => Ast::Stmt(lc, Stmt::Command(parse_command(pair.into_inner())?)),

        Rule::special => unreachable!(),
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
