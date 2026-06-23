use iced::{Background, Border, Padding, Length, alignment};
use iced::widget::{container, row, text, space, responsive, table::{self, Table}, grid, column};
use estella::Value;

use crate::helpers::*;
use crate::Elem;

pub fn to_iced<'a>(value: &'a Value) -> Elem<'a> {
    match value {
        Value::Int(int) => text(int.to_string()).into(),
        Value::Bytes(bytes) => grid(bytes.iter().map(|v| text(format!("{v:0>2X}")).into())).into(),
        Value::Text(value) => text(value.to_string()).into(),
        Value::Array(values) => grid(values.iter().map(|v| to_iced(v))).into(),
        Value::Map(rows) => {
            let columns = [
                table::column(thead("field"), move |(field, _): &(Value, Value)| to_iced(field)),
                table::column(thead("value"), move |(_, value): &(Value, Value)| to_iced(value)),
            ];

            container(
                Table::new(columns, rows)
                    .padding_y(1)
            )
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center)
                .into()
        },
        Value::Tag(tag, value) => column![
            text(format!("Tag: {tag}")),
            to_iced(value),
        ].spacing(2).into(),
        Value::Float(float) => text(float.to_string()).into(),
        Value::Bool(value) => text(value.to_string()).into(),
        Value::Null => text("nil").into(),
        Value::Undefined => text("undefined").into(),
        Value::Simple(v) => text(format!("{v:0>2X}")).into(),
        Value::Table { header, rows } => {
            let columns = header.iter().enumerate().map(|(col_i, c)| {
                table::column(to_iced(c), move |e: &Vec<Value>| {
                    let e: Elem<'_> =
                        if let Some(e) = e.get(col_i) {
                            to_iced(e)
                        } else {
                            space().into()
                        };
                    e
                })
            }).collect::<Vec<_>>();

            container(
                Table::new(columns, rows)
                    .padding_y(1)
            )
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center)
                .into()
        },
    }
}
