use std::path::{Path, PathBuf};
use std::os::unix::ffi::OsStrExt;
use std::io::{self, BufWriter};
use std::ffi::{OsStr, OsString};
use std::borrow::Cow;
use std::sync::LazyLock;

use minicbor::data::{Int, Tag, Type};
use minicbor::{decode, Decoder};
use minicbor::{encode, Encoder};
use rustix::fd::{OwnedFd, BorrowedFd, FromRawFd, AsFd};

pub const TABLE_TAG: u64 = 0x1FA772010;
pub const PATH_TAG: u64 = 0x1FA772011;

#[derive(Debug, Clone)]
pub enum Value<'a> {
    Path(Cow<'a, Path>),
    // type 0/1
    Int(i128),
    // type 2: opaque bytes
    Bytes(Vec<u8>),
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

// impl<T, I> From<I> for Value
// where
//     T: Into<Value> + 'static,
//     I: Iterator<Item = T>,
// {
//     fn from(s: I) -> Value {
//         Value::Array(s.map(|i| Value::from(i)).collect())
//     }
// }

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
            Value::Bytes(b) => _ = e.bytes(b)?,
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
        }
        Ok(())
    }

    pub fn read(d: &mut Decoder) -> Result<Value<'static>, minicbor::decode::Error> {
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

            Type::Bytes | Type::BytesIndef => Ok(Value::Bytes(read_chunks(d)?)),
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

fn fd3_fd() -> Option<BorrowedFd<'static>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static IS_TAKEN: AtomicBool = AtomicBool::new(false);
    if IS_TAKEN.load(Ordering::SeqCst) {
        return None;
    }
    IS_TAKEN.store(true, Ordering::SeqCst);

    static STDBINOUT: LazyLock<OwnedFd> = LazyLock::new(|| unsafe { OwnedFd::from_raw_fd(3) });

    // SAFETY: we assert that the FD is open right afterwards.
    if rustix::fs::fcntl_getfl(unsafe { BorrowedFd::borrow_raw(3) }).is_ok() {
        Some((&*STDBINOUT).as_fd())
    } else {
        None
    }
}

pub struct Fd3(BorrowedFd<'static>);

impl io::Read for Fd3 {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(rustix::io::read(self.0, buf)?)
    }
}

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

pub fn stdout_writer() -> StdoutEncoder {
    let f = Fd3(fd3_fd().unwrap()); // TODO: fallback to stdout
    let writer = encode::write::Writer::new(BufWriter::new(f));
    StdoutEncoder(encode::Encoder::new(writer))
}

pub struct StreamingTable<'a, W: encode::Write> {
    e: &'a mut Encoder<W>,
    width: usize,
}

impl<'a, W: encode::Write> StreamingTable<'a, W> {
    pub fn row<'v, R>(&mut self, row: R) -> Result<(), encode::Error<W::Error>>
    where
        R: IntoIterator<Item = Value<'v>>,
        R::IntoIter: std::iter::ExactSizeIterator
    {
        let row = row.into_iter();
        if row.len() != self.width {
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

pub fn stream_table<'a, 'v, T, I, W: encode::Write>(e: &'a mut Encoder<W>, headers: I)
    -> Result<StreamingTable<'a, W>, encode::Error<W::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<Value<'v>>,
{
    e.tag(Tag::new(TABLE_TAG))?;

    let header: Vec<_> = headers.into_iter().map(|i| i.into()).collect();
    let width = header.len();
    e.array(width as _)?;
    for item in header {
        item.write(e)?;
    }

    e.begin_array()?;
    Ok(StreamingTable { e, width })
}

pub type BufferDecoder<'a> = decode::Decoder<'a>;

pub fn buffer_decoder<'a>(bytes: &'a [u8]) -> BufferDecoder<'a> {
    decode::Decoder::new(bytes)
}
