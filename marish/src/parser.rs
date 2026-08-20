use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;
use pest::pratt_parser::{PrattParser, Op as PrattOp, Assoc};

use std::sync::LazyLock;

#[derive(Parser)]
#[grammar = "src/grammar.pest"]
struct CommandParser;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Operator {
    And, Xor, Or,
    Eq, Ne, Lt, Gt, Le, Ge,
    Like, NotLike,
    Add, Sub, Mul, Div,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Single {
    Token(Token),
    Sub(Box<Ast>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoolExpr {
    pub lhs: Single,
    pub rhs: Single,
    pub op: Operator,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LineCol(pub usize, pub usize, pub usize);

#[derive(Debug, Clone, PartialEq)]
pub struct Var {
    pub name: String,
    pub fields: Vec<FieldExpr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldExpr {
    Column(Single),
    Index(Single),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Array {
    pub items: Vec<Single>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub header: Vec<Single>,
    pub rows: Vec<Vec<Single>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Int(i128),
    Float(f64),
    String(String),
    Word(String),
    Var(Var),
    Array(Array),
    Table(Table),
}

impl Token {
    pub fn to_string(&self) -> String {
        match self {
            Token::Array(_) => todo!(),
            Token::Table(_) => todo!(),
            Token::Int(i) => i.to_string(),
            Token::Float(i) => i.to_string(),
            Token::String(s) => s.clone(),
            Token::Word(s) => s.clone(),
            Token::Var(s) => format!("{:?}", s),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub lc: LineCol,
    pub cmd: String,
    pub argv: Vec<Single>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    pub initial_value: Option<Token>,
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
pub struct Assignment {
    pub lhs: Var,
    pub rhs: Single,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestDecl {
    pub name: String,
    pub body: Vec<Ast>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assert {
    pub lc: LineCol,
    pub body: Box<Ast>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Token(Token),
    Assert(Assert),
    TestDecl(TestDecl),
    Assignment(Assignment),
    Chain(Chain),
    Pipeline(Pipeline),
    Sub(Box<Ast>),
    Background(Box<Ast>),
    Command(Command),
    Where(Option<Box<Ast>>),
    BoolExpr(BoolExpr),
    BoolNegate(Single),
    // Query(Query),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Stmt(LineCol, Stmt),
}

static PRATT: LazyLock<PrattParser<Rule>> = LazyLock::new(|| {
    // Lowest precedence first
    PrattParser::new()
        .op(PrattOp::infix(Rule::b_or_op, Assoc::Left))
        .op(PrattOp::infix(Rule::b_xor_op, Assoc::Left))
        .op(PrattOp::infix(Rule::b_and_op, Assoc::Left))
        .op(PrattOp::prefix(Rule::b_not_op)) // -not is tightest binding
});

fn escape(chars: &mut impl Iterator<Item = char>, out: &mut String) -> Option<()> {
    let c = chars.next()?;

    match c {
        'r' => out.push('\r'),
        'n' => out.push('\n'),
        't' => out.push('\t'),
        'a' => out.push('\x07'),
        'b' => out.push('\x08'),
        '0' => out.push('\0'),

        'x' => {
            let hi = chars.next()?.to_digit(16)?;
            let lo = chars.next()?.to_digit(16)?;

            let value = (hi << 4) | lo;
            out.push(char::from_u32(value)?);
        }

        'o' => {
            let mut value = 0u32;

            for _ in 0..3 {
                let c = chars.next()?;
                let digit = c.to_digit(8)?;

                value = (value << 3) | digit;
            }

            out.push(char::from_u32(value)?);
        }

        _ => return None,
    }

    Some(())
}

fn unescape_dq(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            escape(&mut chars, &mut out)
                .ok_or(format!("Escape sequence in string {s} not supported"))?;
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn unescape_unquoted(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            escape(&mut chars, &mut out)
                .ok_or(format!("Escape sequence in string {s} not supported"))?;
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn parse_token<'a>(pair: Pair<'a, Rule>) -> Result<Token, String> {
    Ok(match pair.as_rule() {
        Rule::var => {
            let mut name = None;
            let mut fields = Vec::new();
            for item in pair.into_inner() {
                match item.as_rule() {
                    Rule::var_name => {
                        assert!(name.is_none());
                        name = Some(item.as_str().to_owned());
                    },
                    Rule::field_col => {
                        let value = item.as_str()[1..].to_owned();
                        fields.push(FieldExpr::Column(Single::Token(Token::String(value))));
                    },
                    Rule::field_col_br => {
                        let sub = Box::new(parse_ast(item.into_inner().next().unwrap())?);
                        fields.push(FieldExpr::Column(Single::Sub(sub)));
                    },
                    Rule::field_index_l => {
                        let v = parse_token(item.into_inner().next().unwrap())?;
                        fields.push(FieldExpr::Index(Single::Token(v)));
                    },
                    Rule::field_index => {
                        let sub = Box::new(parse_ast(item.into_inner().next().unwrap())?);
                        fields.push(FieldExpr::Index(Single::Sub(sub)));
                    },
                    _ => unreachable!(),
                }
            }
            let name = name.unwrap();
            Token::Var(Var { name, fields })
        },
        Rule::array => {
            Token::Array(Array {
                items: pair.into_inner()
                    .map(|p| parse_single(p))
                    .collect::<Result<Vec<_>, _>>()?,
            })
        },
        Rule::table => {
            let mut inner = pair.into_inner();
            let header_pair = inner.next().unwrap();
            let header = match header_pair.as_rule() {
                Rule::header_row => {
                    header_pair.into_inner()
                        .map(|p| parse_single(p))
                        .collect::<Result<Vec<_>, _>>()?
                }
                _ => unreachable!(),
            };
            let rows = inner
                .map(|row_pair| {
                    row_pair.into_inner()
                        .map(|p| parse_single(p))
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?;
            Token::Table(Table { header, rows })
        },
        Rule::single_quoted => Token::String(pair.as_str()[1..].strip_suffix('\'').unwrap().to_owned()),
        Rule::double_quoted => Token::String(unescape_dq(pair.as_str()[1..].strip_suffix('"').unwrap())?),
        Rule::integer => Token::Int(pair.as_str().parse::<i128>().unwrap()),
        Rule::float_lit => Token::Float(pair.as_str().parse::<f64>().unwrap()),
        Rule::unquoted => Token::Word(unescape_unquoted(pair.as_str())?),
        s => panic!("todo: {:?}", s),
    })
}

fn parse_command<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Command, String> {
    let mut cmd = None;
    let mut argv = Vec::new();
    let mut lc = None;
    for pair in pairs {
        if lc.is_none() {
            lc = Some(span_lc(&pair));
            cmd = Some(pair.as_str().to_owned());
        } else {
            argv.push(parse_single(pair)?);
        }
    }

    let lc = lc.unwrap();
    let cmd = cmd.unwrap();
    Ok(Command { lc, cmd, argv })
}

fn span_lc(pair: &Pair<Rule>) -> LineCol {
    let l = pair.line_col().0;
    let s = pair.as_span();
    LineCol(l, s.start(), s.end())
}

fn str_to_op(s: &str) -> Operator {
    match s {
        "+" => Operator::Add,
        "-" => Operator::Sub,
        "/" => Operator::Div,
        "*" => Operator::Mul,
        "-eq" => Operator::Eq,
        "-ne" => Operator::Ne,
        "-ge" => Operator::Ge,
        "-gt" => Operator::Gt,
        "-le" => Operator::Le,
        "-lt" => Operator::Lt,
        "-lk" => Operator::Like,
        "-nk" => Operator::NotLike,
        _ => unreachable!(),
    }
}

fn parse_single<'a>(pair: Pair<'a, Rule>) -> Result<Single, String> {
    Ok(match pair.as_rule() {
        Rule::sub => Single::Sub(Box::new(parse_ast(pair.into_inner().next().unwrap())?)),
        Rule::stmt => Single::Sub(Box::new(parse_ast(pair)?)),
        _ => Single::Token(parse_token(pair)?),
    })
}

fn parse_bool_expr<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Result<Ast, String> {
    PRATT.map_primary(|primary| -> Result<Single, String> {
            match primary.as_rule() {
                Rule::small_op => {
                    let lc = span_lc(&primary);
                    let mut inner = primary.into_inner();
                    let lhs = parse_single(inner.next().unwrap())?;
                    let op = str_to_op(inner.next().unwrap().as_str());
                    let rhs = parse_single(inner.next().unwrap())?;
                    Ok(Single::Sub(Box::new(Ast::Stmt(lc, Stmt::BoolExpr(BoolExpr { op, lhs, rhs })))))
                }
                Rule::double_quoted | Rule::single_quoted | Rule::unquoted
                    => Ok(Single::Token(parse_token(primary)?)),
                _ => Ok(Single::Sub(Box::new(parse_ast(primary)?))),
            }
        })
        .map_prefix(|op, rhs| {
            Ok(Single::Sub(Box::new(Ast::Stmt(span_lc(&op), Stmt::BoolNegate(rhs?)))))
        })
        .map_infix(|lhs, rule, rhs| {
            let (lhs, rhs) = (lhs?, rhs?);
            let op = match rule.as_rule() {
                Rule::b_and_op => Operator::And,
                Rule::b_xor_op => Operator::Xor,
                Rule::b_or_op  => Operator::Or,
                r => unreachable!("unexpected infix {r:?}"),
            };
            Ok(Single::Sub(Box::new(Ast::Stmt(span_lc(&rule), Stmt::BoolExpr(BoolExpr { op, lhs, rhs })))))
        })
        .parse(pairs)
        .map(|ok| match ok {
            Single::Sub(s) => *s,
            _ => unreachable!()
        })
}

fn parse_ast<'a>(pair: Pair<'a, Rule>) -> Result<Ast, String> {
    let lc = span_lc(&pair);

    Ok(match pair.as_rule() {
        Rule::program => unreachable!(),

        Rule::stmt => parse_ast(pair.into_inner().next().unwrap())?,
        Rule::background => Ast::Stmt(lc, Stmt::Background(Box::new(parse_ast(pair.into_inner().next().unwrap())?))),
        Rule::chain => todo!(),
        Rule::pipeline => {
            let mut items = Vec::new();
            let mut initial_value = None;

            for pair in pair.into_inner() {
                match parse_ast(pair)? {
                    Ast::Stmt(_, Stmt::Token(c)) => {
                        if items.len() != 0 {
                            return Err(format!("Value in pipeline must be first item"));
                        }
                        initial_value = Some(c);
                    },
                    Ast::Stmt(_, Stmt::Command(c)) => items.push(PipelineItem::Command(c)),
                    Ast::Stmt(_, Stmt::Sub(s)) => items.push(PipelineItem::Sub(s)),
                    Ast::Stmt(_, Stmt::Where(s)) => items.push(PipelineItem::Where(s)),
                    // Ast::Stmt(_, Stmt::Query(q)) => items.push(PipelineItem::Query(q)),
                    _ => unreachable!(),
                }
            }

            Ast::Stmt(lc, Stmt::Pipeline(Pipeline { items, initial_value }))
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
        Rule::assignment => {
            let mut inner = pair.into_inner();
            let Token::Var(lhs) = parse_token(inner.next().unwrap())? else { unreachable!() };
            match lhs.name.as_str() {
                "t" | "f" | "nil" | "undef" => return Err(format!("Reserved variable {} cannot be assigned to.", lhs.name)),
                _ => (),
            }
            let rhs = parse_single(inner.next().unwrap())?;
            Ast::Stmt(lc, Stmt::Assignment(Assignment { lhs, rhs }))
        },
        Rule::b_expr => parse_bool_expr(pair.into_inner())?,

        Rule::test_decl => {
            let mut inner = pair.into_inner();
            let name = match parse_token(inner.next().unwrap())? {
                Token::Word(w) => w,
                Token::String(s) => s,
                _ => unreachable!(), // Only un/single/double-quoted strings allowed by grammer
            };
            let body_pair = inner.next().unwrap().into_inner();
            let body = parse_list(body_pair)?;
            Ast::Stmt(lc, Stmt::TestDecl(TestDecl { name, body }))
        },
        Rule::s_assert => {
            let body = Box::new(parse_ast(pair.into_inner().next().unwrap())?);
            Ast::Stmt(lc, Stmt::Assert(Assert { body, lc }))
        },

        Rule::var
        | Rule::array
        | Rule::table
        | Rule::single_quoted
        | Rule::double_quoted
        | Rule::integer
        | Rule::float_lit
        | Rule::unquoted => Ast::Stmt(lc, Stmt::Token(parse_token(pair)?)),

        Rule::decl => unreachable!(),
        Rule::special => unreachable!(),
        Rule::token => unreachable!(),
        Rule::connector => unreachable!(),

        Rule::WHITESPACE => unreachable!(),
        Rule::EOI => unreachable!(),

        _ => unreachable!(),
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
