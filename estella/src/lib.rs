use std::io::{self, BufWriter};
use std::borrow::Cow;
use minicbor::data::{Int, Tag, Type};
use minicbor::{decode, Decoder};
use minicbor::{encode, Encoder};

pub const TABLE_TAG: u64 = 0x1FA772010;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i128),                // type 0/1: see range note below
    Bytes(Vec<u8>),           // type 2: opaque bytes
    Text(Cow<'static, str>),  // type 3: UTF-8
    Array(Vec<Value>),        // type 4
    Map(Vec<(Value, Value)>), // type 5: ordered pairs, not a HashMap
    Tag(u64, Box<Value>),     // type 6: semantic annotation on one item
    Float(f64),               // type 7: f16/f32/f64 all widened to f64
    Bool(bool),               // type 7
    Null,                     // type 7
    Undefined,                // type 7: distinct from Null
    Simple(u8),               // type 7: other simple values

    Table {
        header: Vec<Value>,
        rows: Vec<Vec<Value>>,
    },
}

impl From<usize>  for Value { fn from(s: usize)  -> Value { Value::Int(s as _) } }
impl From<u64>    for Value { fn from(s: u64)    -> Value { Value::Int(s as _) } }
impl From<u32>    for Value { fn from(s: u32)    -> Value { Value::Int(s as _) } }
impl From<u16>    for Value { fn from(s: u16)    -> Value { Value::Int(s as _) } }
impl From<u8>     for Value { fn from(s: u8)     -> Value { Value::Int(s as _) } }
impl From<isize>  for Value { fn from(s: isize)  -> Value { Value::Int(s as _) } }
impl From<i64>    for Value { fn from(s: i64)    -> Value { Value::Int(s as _) } }
impl From<i32>    for Value { fn from(s: i32)    -> Value { Value::Int(s as _) } }
impl From<i16>    for Value { fn from(s: i16)    -> Value { Value::Int(s as _) } }
impl From<i8>     for Value { fn from(s: i8)     -> Value { Value::Int(s as _) } }
impl From<String> for Value { fn from(s: String) -> Value { Value::text(s) } }
impl From<&'static str> for Value { fn from(s: &'static str) -> Value { Value::text(s) } }

// impl<T, I> From<I> for Value
// where
//     T: Into<Value> + 'static,
//     I: Iterator<Item = T>,
// {
//     fn from(s: I) -> Value {
//         Value::Array(s.map(|i| Value::from(i)).collect())
//     }
// }

impl Value {
    pub fn text(value: impl Into<Cow<'static, str>>) -> Self {
        Value::Text(value.into())
    }

    pub fn write<W: encode::Write>(&self, e: &mut Encoder<W>)
        -> Result<(), encode::Error<W::Error>>
    {
        match self {
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

    pub fn read(d: &mut Decoder) -> Result<Value, minicbor::decode::Error> {
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
                    TABLE_TAG => {
                        let Value::Array(header) = Value::read(d)? else { todo!() };
                        let mut rows = Vec::new();
                        let n = d.array()?;
                        read_seq(d, n, |d| {
                            let Value::Array(row) = Value::read(d)? else { todo!() };
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

pub struct StdoutEncoder(pub encode::Encoder<encode::write::Writer<BufWriter<std::io::StdoutLock<'static>>>>);

pub fn stdout_writer() -> StdoutEncoder {
    let writer = encode::write::Writer::new(BufWriter::new(io::stdout().lock()));
    StdoutEncoder(encode::Encoder::new(writer))
}

pub struct StreamingTable<'a, W: encode::Write> {
    e: &'a mut Encoder<W>,
    width: usize,
}

impl<W: encode::Write> StreamingTable<'_, W> {
    pub fn row<R>(&mut self, row: R) -> Result<(), encode::Error<W::Error>>
    where
        R: IntoIterator<Item = Value>,
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

pub fn stream_table<'a, T, I, W: encode::Write>(e: &'a mut Encoder<W>, headers: I)
    -> Result<StreamingTable<'a, W>, encode::Error<W::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<Value>,
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
