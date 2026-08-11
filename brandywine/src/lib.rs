// TODO: do we really need a separate map type? Seems redundant when a single-row table could be
// used.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, BufWriter};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub use minicbor;
use minicbor::data::{Int, Tag, Type};
use minicbor::{decode, Decoder};
use minicbor::{encode, Encoder};
use rustix::fd::{OwnedFd, BorrowedFd, FromRawFd, AsFd};

pub const TABLE_TAG: u64 = 0x1FA772010;
pub const PATH_TAG: u64 = 0x1FA772011;
pub const TIMESTAMP_TAG: u64 = 0x01;

#[derive(Debug, Clone)]
pub enum Token<'a> {
    Int(i128),
    Path,
    Bytes,
    BytesPart(Cow<'a, [u8]>),
    Text,
    TextPart(Cow<'a, str>),
    Array(Option<u64>),
    Map(Option<u64>),
    // MapFirst,
    // MapSecond,
    Tag(u64),

    Float(f64),
    Bool(bool),
    Null,
    Undefined,
    Simple(u8),

    Table,
    Timestamp,

    End,
}

impl<'a> Token<'a> {
    pub fn collect(ast: &[Token<'a>]) -> Option<(usize, Value<'static>)> {
        let Some(head) = ast.first() else { return None };

        Some(match head {
            Token::Int(i) => (1, Value::Int(*i)),
            Token::Float(i) => (1, Value::Float(*i)),
            Token::Bool(i) => (1, Value::Bool(*i)),
            Token::Simple(i) => (1, Value::Simple(*i)),
            Token::Null => (1, Value::Null),
            Token::Undefined => (1, Value::Undefined),

            Token::End | Token::TextPart(_) | Token::BytesPart(_) => return None,

            Token::Text => {
                let mut s = String::new();
                let mut i = 1;
                loop {
                    match ast.get(i) {
                        None => return None,
                        Some(Token::End) => break,
                        Some(Token::TextPart(p)) => {
                            i += 1;
                            s.push_str(&*p);
                        }
                        Some(_) => return None,
                    }
                }
                i += 1; // Move past end
                (i, Value::Text(s.into()))
            }
            Token::Path => {
                let mut s = OsString::new();
                let mut i = 1;
                loop {
                    match ast.get(i) {
                        None => return None,
                        Some(Token::End) => break,
                        Some(Token::BytesPart(p)) => {
                            i += 1;
                            s.push(OsStr::from_bytes(&p));
                        }
                        Some(_) => return None,
                    }
                }
                i += 1; // Move past end
                (i, Value::Path(PathBuf::from(s).into()))
            }
            Token::Bytes => {
                let mut s = Vec::new();
                let mut i = 1;
                loop {
                    match ast.get(i) {
                        None => return None,
                        Some(Token::End) => break,
                        Some(Token::BytesPart(p)) => {
                            i += 1;
                            s.extend_from_slice(&p);
                        }
                        Some(_) => return None,
                    }
                }
                i += 1; // Move past end
                (i, Value::Bytes(s.into()))
            }

            Token::Array(_) => {
                let mut i = 1;
                let mut v = Vec::new();
                loop {
                    match ast.get(i) {
                        None => return None,
                        Some(Token::End) => break,
                        Some(_) => {
                            let (ns, it) = Self::collect(&ast[i..])?;
                            i += ns;
                            v.push(it);
                        },
                    }
                }
                i += 1; // Move past end
                (i, Value::Array(v))
            }
            Token::Map(_) => todo!(),

            Token::Tag(t) => {
                let (ns, it) = Self::collect(&ast[1..])?;
                (1 + ns, Value::Tag(*t, Box::new(it)))
            }

            Token::Table => {
                let mut i = 1;
                let (ns, Value::Array(he)) = Self::collect(&ast[i..])?
                    else { return None }; // TODO: error
                i += ns;
                let (ns, Value::Array(rw)) = Self::collect(&ast[i..])?
                    else { return None }; // TODO: error
                i += ns;
                let rows = rw.into_iter().map(|v| match v {
                    Value::Array(row) => row,
                    _ => todo!(), // TODO: error
                }).collect();
                (i, Value::Table { header: he, rows })
            },

            Token::Timestamp => {
                let (ns, Value::Int(ts)) = Self::collect(&ast[1..])?
                    else { return None }; // TODO: error
                (1 + ns, Value::Timestamp(ts as i64))
            },
        })
    }

    // pub fn span(ast: &[Token<'_>]) -> Option<usize> {
    //     let Some(head) = ast.first() else { return None };

    //     let n = match head {
    //         // Atomics: a single self-contained token.
    //         Token::Int(_) | Token::Float(_) | Token::Bool(_)
    //         | Token::Null | Token::Undefined | Token::Simple(_) => 1,

    //         // Parts only occur inside a value, and `End` only closes one; as a head
    //         // they're malformed, so consume just the one token to keep callers moving.
    //         Token::End | Token::TextPart(_) | Token::BytesPart(_) => 1,

    //         // String-likes: opener, parts, through the closing `End`.
    //         Token::Text | Token::Bytes | Token::Path => {
    //             let mut i = 1;
    //             loop {
    //                 match ast.get(i) {
    //                     None => return None,
    //                     Some(Token::End) => break i + 1,
    //                     Some(_) => i += 1,
    //                 }
    //             }
    //         }

    //         // Composites: opener, values (recursively), through the closing `End`.
    //         Token::Array(_) | Token::Map(_) => {
    //             let mut i = 1;
    //             loop {
    //                 match ast.get(i) {
    //                     None => return None,
    //                     Some(Token::End) => break i + 1,
    //                     Some(_) => i += token_span(&ast[i..]).len(),
    //                 }
    //             }
    //         }

    //         // A tag wraps exactly one following value.
    //         Token::Tag(_) => 1 + token_span(&ast[1..]).len(),

    //         // A table is itself plus two values: the header array and the rows array.
    //         Token::Table => {
    //             let header = token_span(&ast[1..]).len();
    //             let rows = token_span(&ast[1 + header..]).len();
    //             1 + header + rows
    //         }
    //     };

    //     Some(n)
    // }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Expecting {
    Something,
    Map(usize, Option<u64>),
    Array(usize, Option<u64>),
    TimestampValue,
    TableHeader,
    TableRows,
    TableRow(usize),
    String(usize, Option<u64>),
    Bytes(usize, Option<u64>),
}

pub struct StreamingReader {
    pub stack: Vec<Expecting>,
}

impl StreamingReader {
    pub fn new() -> Self {
        Self {
            stack: vec![Expecting::Something],
        }
    }

    pub fn is_done(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn read_once<'a>(&mut self, buf: &'a [u8], ast: &'_ mut Vec<Token<'static>>) -> Result<usize, decode::Error> {
        let mut d = Decoder::new(buf);

        let Some(&expecting) = self.stack.last()
            else { return Ok(0) /* done */ };

        fn decode_atomic<'a>(d: &mut Decoder<'a>) -> Result<Option<Token<'static>>, decode::Error> {
            Ok(match d.datatype()? {
                Type::U8 | Type::U16 | Type::U32 | Type::U64
                | Type::I8 | Type::I16 | Type::I32 | Type::I64
                | Type::Int => Some(Token::Int(d.int()?.into())),

                Type::F16 | Type::F32 | Type::F64 => Some(Token::Float(d.f64()?)),
                Type::Bool => Some(Token::Bool(d.bool()?)),
                Type::Null => {
                    d.null()?;
                    Some(Token::Null)
                }
                Type::Undefined => {
                    d.undefined()?;
                    Some(Token::Undefined)
                }
                Type::Simple => Some(Token::Simple(d.simple()?)),
                _ => None,
            })
        }

        fn decode_beginner<'a>(d: &mut Decoder<'a>, astlen: usize) -> Result<Option<(Token<'static>, Expecting)>, decode::Error> {
            Ok(match d.datatype()? {
                Type::Bytes | Type::BytesIndef => {
                    Some((Token::Bytes, Expecting::Bytes(astlen, read_len(d)?)))
                },
                Type::String | Type::StringIndef => {
                    Some((Token::Text, Expecting::String(astlen, read_len(d)?)))
                },
                Type::Array | Type::ArrayIndef => {
                    let n = d.array()?;
                    Some((Token::Array(n), Expecting::Array(astlen, n)))
                },
                Type::Map | Type::MapIndef => {
                    let n = d.map()?.map(|n| n * 2);
                    Some((Token::Map(n), Expecting::Map(astlen, n)))
                }
                Type::Tag => {
                    match d.tag()?.into() {
                        PATH_TAG => Some((Token::Path, Expecting::Bytes(astlen, read_len(d)?))),
                        TABLE_TAG => Some((Token::Table, Expecting::TableHeader)),
                        TIMESTAMP_TAG => Some((Token::Timestamp, Expecting::TimestampValue)),
                        tag => Some((Token::Tag(tag), Expecting::Something)),
                    }
                }
                _ => None,
            })
        }

        fn read_len(d: &mut Decoder<'_>) -> Result<Option<u64>, decode::Error> {
            let p = d.position();
            //d.set_position(d.position() + 1);
            Ok(match read(d)? & 0x1F {
                31 => None,
                n => Some(unsigned(d, n, p)?),
            })
        }

        fn read(d: &mut Decoder<'_>) -> Result<u8, decode::Error> {
            if let Some(b) = d.input().get(d.position()) {
                d.set_position(d.position() + 1);
                return Ok(*b)
            }
            Err(decode::Error::end_of_input())
        }

        fn read_slice<'b>(d: &mut Decoder<'b>, n: usize) -> Result<&'b [u8], decode::Error> {
            let p = d.position();
            if let Some(b) = p.checked_add(n).and_then(|end| d.input().get(p..end)) {
                d.set_position(p + n);
                return Ok(b)
            }
            Err(decode::Error::end_of_input())
        }

        fn read_array<'b, const N: usize>(d: &mut Decoder<'b>) -> Result<[u8; N], decode::Error> {
            read_slice(d, N).map(|s| {
                let mut a = [0; N];
                a.copy_from_slice(s);
                a
            })
        }

        fn unsigned(d: &mut Decoder<'_>, b: u8, p: usize) -> Result<u64, decode::Error> {
            match b {
                n @ 0 ..= 0x17 => Ok(u64::from(n)),
                0x18 => read(d).map(u64::from),
                0x19 => read_array(d).map(u16::from_be_bytes).map(u64::from),
                0x1a => read_array(d).map(u32::from_be_bytes).map(u64::from),
                0x1b => read_array(d).map(u64::from_be_bytes),
                _    => Err(decode::Error::type_mismatch(Type::Break).with_message("expected u64").at(p)),
            }
        }

        fn current(d: &Decoder<'_>) -> Result<u8, decode::Error> {
            if let Some(b) = d.input().get(d.position()) {
                return Ok(*b)
            }
            Err(decode::Error::end_of_input())
        }

        match expecting {
            Expecting::TimestampValue => {
                match decode_atomic(&mut d)? {
                    None => return Err(decode::Error::end_of_input()),
                    Some(bt @ Token::Int(_)) => {
                        ast.push(bt);
                        self.stack.pop();
                    },
                    v => panic!("expected scalar timestamp value (i64), found {v:?}"),
                }
            }
            Expecting::Array(_, Some(0)) => {
                ast.push(Token::End);
                self.stack.pop();
            }
            Expecting::Array(i, Some(num)) => {
                let aexp = Expecting::Array(i, Some(num - 1));
                if let Some(token) = decode_atomic(&mut d)? {
                    ast.push(token);
                    self.stack.pop();
                    self.stack.push(aexp);
                } else if let Some((btok, exp)) = decode_beginner(&mut d, ast.len())? {
                    ast.push(btok);
                    self.stack.pop();
                    self.stack.push(aexp);
                    self.stack.push(exp);
                } else {
                    unreachable!();
                }
            }
            Expecting::Array(_, None) => {
                if let Type::Break = d.datatype()? {
                    d.skip()?;
                    ast.push(Token::End);
                    self.stack.pop();
                } else {
                    if let Some(token) = decode_atomic(&mut d)? {
                        ast.push(token);
                    } else if let Some((btok, exp)) = decode_beginner(&mut d, ast.len())? {
                        ast.push(btok);
                        self.stack.push(exp);
                    } else {
                        unreachable!();
                    }
                }
            },
            Expecting::TableHeader => {
                match decode_beginner(&mut d, ast.len())? {
                    None => return Err(decode::Error::end_of_input()),
                    Some((bt @ Token::Array(_), exp)) => {
                        ast.push(bt);
                        self.stack.pop();
                        self.stack.push(Expecting::TableRows);
                        self.stack.push(exp);
                    },
                    v => panic!("expected table header (an array), found {v:?}"),
                }
            },
            Expecting::TableRows => {
                match decode_beginner(&mut d, ast.len())? {
                    None => return Err(decode::Error::end_of_input()),
                    Some((bt @ Token::Array(_), exp)) => {
                        ast.push(bt);
                        self.stack.pop();
                        self.stack.push(exp);
                    },
                    _ => panic!("expected table rows (an array)"),
                }
            },
            Expecting::TableRow(_) => {
                match decode_beginner(&mut d, ast.len())? {
                    None => return Err(decode::Error::end_of_input()),
                    Some((bt @ Token::Array(_), exp)) => {
                        ast.push(bt);
                        self.stack.pop();
                        self.stack.push(exp);
                    }
                    _ => panic!("expected single table row (an array)"),
                }
            },
            Expecting::String(_, num) => {
                match num {
                    None => match current(&mut d)? {
                        0xFF => {
                            _ = read(&mut d)?;
                            ast.push(Token::End);
                            self.stack.pop();
                        },
                        _ => ast.push(Token::TextPart(d.str()?.to_string().into())),
                    },
                    Some(n) => {
                        ast.push(Token::TextPart(
                                str::from_utf8(read_slice(&mut d, n as _)?)
                                    .map_err(|_| todo!())? // Error::utf8 is private; TODO: add our own error
                                    .to_string()
                                    .into()
                        ));
                        ast.push(Token::End);
                        self.stack.pop();
                    }
                }
            },
            Expecting::Bytes(_, num) => {
                match num {
                    None => match current(&mut d)? {
                        0xFF => {
                            _ = read(&mut d)?;
                            ast.push(Token::End);
                            self.stack.pop();
                        },
                        _ => ast.push(Token::BytesPart(d.bytes()?.to_vec().into())),
                    },
                    Some(n) => {
                        ast.push(Token::BytesPart(read_slice(&mut d, n as _)?.to_vec().into()));
                        ast.push(Token::End);
                        self.stack.pop();
                    }
                }
            },
            Expecting::Map(_, _) => todo!(),
            Expecting::Something => {
                if let Some(token) = decode_atomic(&mut d)? {
                    ast.push(token);
                    self.stack.pop();
                } else if let Some((btok, exp)) = decode_beginner(&mut d, ast.len())? {
                    ast.push(btok);
                    self.stack.pop();
                    self.stack.push(exp);
                } else {
                    unreachable!();
                }
            },
        }

        Ok(d.position())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Path(Cow<'a, Path>),
    // type 0/1
    Int(i128),
    // type 2: opaque bytes
    Bytes(Cow<'a, [u8]>),
    // type 3: UTF-8
    Text(Cow<'static, str>),
    // type 4
    Array(Vec<Value<'a>>),
    // type 5: ordered pairs
    Map(Vec<(Value<'a>, Value<'a>)>),
    // type 6: semantic annotation on one item
    Tag(u64, Box<Value<'a>>),

    // Type 7 ----
    Float(f64),
    Bool(bool),
    Null,
    Undefined,
    Simple(u8),

    // Tagged value
    Table {
        header: Vec<Value<'a>>,
        rows: Vec<Vec<Value<'a>>>,
    },
    // Must be i64 when decoding as well
    // TODO: add a Timestampf64 type as well maybe? or a timestampf128?
    Timestamp(i64),
}

impl From<usize>  for Value<'static> { fn from(s: usize)  -> Value<'static> { Value::Int(s as _) } }
impl From<u64>    for Value<'static> { fn from(s: u64)    -> Value<'static> { Value::Int(s as _) } }
impl From<u32>    for Value<'static> { fn from(s: u32)    -> Value<'static> { Value::Int(s as _) } }
impl From<u16>    for Value<'static> { fn from(s: u16)    -> Value<'static> { Value::Int(s as _) } }
impl From<u8>     for Value<'static> { fn from(s: u8)     -> Value<'static> { Value::Int(s as _) } }
impl From<isize>  for Value<'static> { fn from(s: isize)  -> Value<'static> { Value::Int(s as _) } }
impl From<i64>    for Value<'static> { fn from(s: i64)    -> Value<'static> { Value::Int(s as _) } }
impl From<i32>    for Value<'static> { fn from(s: i32)    -> Value<'static> { Value::Int(s as _) } }
impl From<i16>    for Value<'static> { fn from(s: i16)    -> Value<'static> { Value::Int(s as _) } }
impl From<i8>     for Value<'static> { fn from(s: i8)     -> Value<'static> { Value::Int(s as _) } }
impl From<String> for Value<'static> { fn from(s: String) -> Value<'static> { Value::text(s) } }
impl From<&'static str> for Value<'static> { fn from(s: &'static str) -> Value<'static> { Value::text(s) } }

impl<'a> From<&'a std::ffi::OsStr> for Value<'a> {
    fn from(s: &'a std::ffi::OsStr) -> Value<'a> {
        Value::Bytes(s.as_bytes().into())
    }
}

impl<T: chrono::TimeZone> From<chrono::DateTime<T>> for Value<'static> {
    fn from(s: chrono::DateTime<T>) -> Value<'static> {
        Value::Timestamp(s.timestamp_millis())
    }
}

// impl<T, I> From<I> for Value
// where
//     T: Into<Value> + 'static,
//     I: Iterator<Item = T>,
// {
//     fn from(s: I) -> Value {
//         Value::Array(s.map(|i| Value::from(i)).collect())
//     }
// }


impl<'b> decode::Decode<'b, ()> for Value<'static> {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut ()) -> Result<Value<'static>, decode::Error> {
        fn read_chunks_osstring(d: &mut Decoder) -> Result<OsString, minicbor::decode::Error> {
            let mut buf = OsString::new();
            for chunk in d.bytes_iter()? {
                buf.push(OsStr::from_bytes(chunk?));
            }
            Ok(buf)
        }

        fn read_chunks(d: &mut Decoder) -> Result<Vec<u8>, minicbor::decode::Error> {
            let mut buf = Vec::new();
            for chunk in d.bytes_iter()? {
                buf.extend_from_slice(chunk?);
            }
            Ok(buf)
        }

        fn read_seq(
            d: &mut Decoder,
            count: Option<u64>,
            mut on_item: impl FnMut(&mut Decoder) -> Result<(), minicbor::decode::Error>,
        ) -> Result<(), minicbor::decode::Error> {
            if let Some(n) = count { // Known length
                for _ in 0..n {
                    on_item(d)?;
                }
            } else {
                while d.datatype()? != Type::Break {
                    on_item(d)?;
                }
                d.skip()?; // Consume the break (0xff)
            }
            Ok(())
        }

        match d.datatype()? {
            Type::U8 | Type::U16 | Type::U32 | Type::U64
            | Type::I8 | Type::I16 | Type::I32 | Type::I64
            | Type::Int => Ok(Value::Int(d.int()?.into())),

            Type::Bytes | Type::BytesIndef => Ok(Value::Bytes(read_chunks(d)?.into())),
            Type::String | Type::StringIndef => Ok(Value::Text(d.str_iter()?.collect::<Result<String, _>>()?.into())),

            Type::Array | Type::ArrayIndef => {
                let n = d.array()?;
                let mut items = Vec::new();
                read_seq(d, n, |d| {
                    items.push(Self::read(d)?);
                    Ok(())
                })?;
                Ok(Value::Array(items))
            }
            Type::Map | Type::MapIndef => {
                let n = d.map()?;
                let mut pairs = Vec::new();
                read_seq(d, n, |d| {
                    let k = Self::read(d)?;
                    let v = Self::read(d)?;
                    pairs.push((k, v));
                    Ok(())
                })?;
                Ok(Value::Map(pairs))
            }
            Type::Tag => {
                match d.tag()?.into() {
                    PATH_TAG => {
                        let path = PathBuf::from(read_chunks_osstring(d)?);
                        Ok(Value::Path(path.into()))
                    },
                    TABLE_TAG => {
                        let Value::Array(header) = Value::read(d)? else { todo!() };
                        let mut rows = Vec::new();
                        let n = d.array()?;
                        read_seq(d, n, |d| {
                            let row = match Value::read(d)? {
                                Value::Array(row) => row,
                                c => panic!("Expected array, got {c:?}"),
                            };
                            rows.push(row);
                            Ok(())
                        })?;
                        Ok(Value::Table { header, rows })
                    },
                    tag => {
                        let inner = Value::read(d)?;
                        Ok(Value::Tag(tag, Box::new(inner)))
                    }
                }
            }

            Type::F16 | Type::F32 | Type::F64 => Ok(Value::Float(d.f64()?)),
            Type::Bool => Ok(Value::Bool(d.bool()?)),
            Type::Null => {
                d.null()?;
                Ok(Value::Null)
            }
            Type::Undefined => {
                d.undefined()?;
                Ok(Value::Undefined)
            }
            Type::Simple => Ok(Value::Simple(d.simple()?)),

            Type::Break => todo!(), // Malformed input
            _ => todo!(), // Unknown initial byte
        }
    }
}

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Path(p) => write!(f, "{}", p.display()),
            Value::Text(s)  => write!(f, "{}", &*s),
            Value::Int(x) => write!(f, "{x}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Bool(x) => write!(f, "{x}"),
            Value::Simple(x) => write!(f, "{x}"),
            Value::Bytes(_) => write!(f, "<bytes>"),
            Value::Table { .. } => write!(f, "<table>"),
            Value::Null => write!(f, "<nil>"),
            Value::Undefined => write!(f, "<undefined>"),
            Value::Array(_) => write!(f, "<array>"),
            Value::Map(_) => write!(f, "<map>"),
            Value::Tag(_, _) => write!(f, "<tagged>"),
            Value::Timestamp(t) => write!(f, "{}", {
                use chrono::{Utc, DateTime};
                DateTime::<Utc>::from_timestamp_millis(*t).unwrap().to_rfc3339()
            }),
        }
    }
}

impl<'a> Value<'a> {
    pub fn text(value: impl Into<Cow<'static, str>>) -> Self {
        Value::Text(value.into())
    }

    pub fn write<W: encode::Write>(&self, e: &mut Encoder<W>)
        -> Result<(), encode::Error<W::Error>>
    {
        match self {
            Value::Path(p) => {
                e.tag(Tag::new(PATH_TAG))?;
                e.bytes(p.as_os_str().as_bytes())?;
            }
            Value::Int(x) => {
                let n = Int::try_from(*x)
                    // TODO: Outside CBOR range, need bignum tag
                    .unwrap();
                e.int(n)?;
            }
            Value::Bytes(b) => _ = e.bytes(&b)?,
            Value::Text(s)  => _ = e.str(&*s)?,
            Value::Array(items) => {
                e.array(items.len() as u64)?;
                for it in items {
                    it.write(e)?;
                }
            }
            Value::Map(pairs) => {
                e.map(pairs.len() as u64)?;
                for (k, val) in pairs {
                    k.write(e)?;
                    val.write(e)?;
                }
            }
            Value::Tag(tag, inner) => {
                e.tag(Tag::new(*tag))?;
                inner.write(e)?;
            }
            Value::Float(f) => _ = e.f64(*f)?,
            Value::Bool(b) => _ = e.bool(*b)?,
            Value::Null => _ = e.null()?,
            Value::Undefined => _ = e.undefined()?,
            Value::Simple(n) => _ = e.simple(*n)?,

            Value::Table { header, rows } => {
                e.tag(Tag::new(TABLE_TAG))?;

                e.array(header.len() as _)?;
                for item in header {
                    item.write(e)?;
                }

                e.array(rows.len() as _)?;
                for row in rows {
                    e.array(row.len() as _)?;
                    for item in row {
                        item.write(e)?;
                    }
                }
            },
            Value::Timestamp(t) => {
                e.tag(Tag::new(TIMESTAMP_TAG))?;
                e.i64(*t)?;
            }
        }
        Ok(())
    }

    pub fn read(d: &mut Decoder) -> Result<Value<'static>, minicbor::decode::Error> {
        d.decode::<Value>()
    }

    pub fn try_read(d: &mut Decoder) -> Result<Value<'static>, minicbor::decode::Error> {
        let p = d.position();
        match d.decode::<Value>() {
            Ok(v) => Ok(v),
            Err(e) => {
                d.set_position(p);
                Err(e)
            }
        }
    }
}

fn fd4_fd() -> Option<BorrowedFd<'static>> {

    // SAFETY: we assert that the FD is open right afterwards.
    if rustix::fs::fcntl_getfl(unsafe { BorrowedFd::borrow_raw(4) }).is_ok() {
        static STDBININ: LazyLock<OwnedFd> = LazyLock::new(|| unsafe { OwnedFd::from_raw_fd(4) });
        Some((&*STDBININ).as_fd())
    } else if rustix::fs::fcntl_getfl(unsafe { BorrowedFd::borrow_raw(0) }).is_ok() {
        // silly
        static STDIN: LazyLock<OwnedFd> = LazyLock::new(|| unsafe { OwnedFd::from_raw_fd(0) });
        Some((&*STDIN).as_fd())
    } else {
        None
    }
}

pub struct Fd4(BorrowedFd<'static>);

impl Fd4 {
    pub fn acquire() -> Option<Self> {
        Some(Fd4(fd4_fd()?))
    }
}

impl io::Read for Fd4 {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(rustix::io::read(self.0, buf)?)
    }
}

fn fd3_fd() -> Option<BorrowedFd<'static>> {
    // use std::sync::atomic::{AtomicBool, Ordering};
    // static IS_TAKEN: AtomicBool = AtomicBool::new(false);
    // if IS_TAKEN.load(Ordering::SeqCst) {
    //     return None;
    // }
    // IS_TAKEN.store(true, Ordering::SeqCst);

    static STDBINOUT: LazyLock<OwnedFd> = LazyLock::new(|| unsafe { OwnedFd::from_raw_fd(3) });

    // SAFETY: we assert that the FD is open right afterwards.
    if rustix::fs::fcntl_getfl(unsafe { BorrowedFd::borrow_raw(3) }).is_ok() {
        Some((&*STDBINOUT).as_fd())
    } else {
        None
    }
}

pub struct Fd3(BorrowedFd<'static>);

impl io::Write for Fd3 {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            return match rustix::io::write(self.0, buf) {
                Err(rustix::io::Errno::AGAIN) => continue,
                Ok(c) => Ok(c),
                Err(e) => Err(io::Error::from_raw_os_error(e.raw_os_error())),
            };
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        _ = self;
        Ok(())
    }
}

pub struct StdoutEncoder(pub encode::Encoder<encode::write::Writer<BufWriter<Fd3>>>);

pub fn stdout_writer() -> Option<StdoutEncoder> {
    let f = Fd3(fd3_fd()?); // TODO: fallback to stdout
    let writer = encode::write::Writer::new(BufWriter::new(f));
    Some(StdoutEncoder(encode::Encoder::new(writer)))
}

pub struct StreamingTable<'a, W: encode::Write> {
    e: &'a mut Encoder<W>,
    width: Option<usize>, // None if not known yet (i.e. headers not yet provided)
}

impl<'a, W: encode::Write> StreamingTable<'a, W> {
    pub fn headers<'v, T, I>(&mut self, headers: I) -> Result<(), encode::Error<W::Error>>
    where
        I: IntoIterator<Item = T>,
        T: Into<Value<'v>>,
    {
        assert!(self.width.is_none());

        let header: Vec<_> = headers.into_iter().map(|i| i.into()).collect();
        let width = header.len();
        self.e.array(width as _)?;
        for item in header {
            item.write(self.e)?;
        }

        self.width = Some(width);
        self.e.begin_array()?;
        Ok(())
    }

    pub fn row<'v, R>(&mut self, row: R) -> Result<(), encode::Error<W::Error>>
    where
        R: IntoIterator<Item = Value<'v>>,
        R::IntoIter: std::iter::ExactSizeIterator
    {
        let row = row.into_iter();
        if row.len() != self.width.unwrap() {
            todo!();
        }

        self.e.array(row.len() as _)?;
        for item in row {
            item.write(self.e)?;
        }
        Ok(())
    }

    pub fn end(self) {
        _ = self;
    }
}

impl<W: encode::Write> Drop for StreamingTable<'_, W> {
    fn drop(&mut self) {
        _ = self.e.end();
    }
}

pub fn stream_table_no_headers<'a, 'v, W: encode::Write>(e: &'a mut Encoder<W>)
    -> Result<StreamingTable<'a, W>, encode::Error<W::Error>>
{
    e.tag(Tag::new(TABLE_TAG))?;
    Ok(StreamingTable { e, width: None })
}

pub fn stream_table<'a, 'v, T, I, W: encode::Write>(e: &'a mut Encoder<W>, headers: I)
    -> Result<StreamingTable<'a, W>, encode::Error<W::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<Value<'v>>,
{
    e.tag(Tag::new(TABLE_TAG))?;
    let mut stt = StreamingTable { e, width: None };
    stt.headers(headers)?;
    Ok(stt)
}

pub type BufferDecoder<'a> = decode::Decoder<'a>;

pub fn buffer_decoder<'a>(bytes: &'a [u8]) -> BufferDecoder<'a> {
    decode::Decoder::new(bytes)
}
