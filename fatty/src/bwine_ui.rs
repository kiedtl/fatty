use iced::{Background, Border, Padding, Length, alignment};
use iced::widget::{container, row, text, space, responsive, table::{self, Table}, grid, column};
use bwine::Value;

use crate::helpers::*;
use crate::widgets::scrollable;
use crate::Elem;

pub fn to_iced<'a>(value: &'a Value, vsize: Option<iced::Size>) -> Elem<'a> {
    match value {
        Value::Path(p) => text(p.display().to_string()).into(),
        Value::Int(int) => text(int.to_string()).into(),
        Value::Bytes(bytes) => text(String::from_utf8_lossy(&bytes[..(50).min(bytes.len())])).into(),
        Value::Text(value) => text(value.to_string()).into(),
        Value::Array(values) => grid(values.iter().map(|v| to_iced(v, vsize))).into(),
        Value::Map(rows) => {
            let columns = [
                table::column(thead("field"), move |(field, _): &(Value, Value)| to_iced(field, vsize)),
                table::column(thead("value"), move |(_, value): &(Value, Value)| to_iced(value, vsize)),
            ];

            let dir =
                if rows.len() > 30 {
                    scrollable::Direction::Both {
                        vertical: scrollable::Scrollbar::default(),
                        horizontal: scrollable::Scrollbar::default(),
                    }
                } else {
                    scrollable::Direction::Horizontal(scrollable::Scrollbar::default())
                };
            scrollable::Scrollable::with_direction(
                Table::new(columns, rows).padding_y(1),
                dir
            ).into()
        },
        Value::Tag(tag, value) => column![
            text(format!("Tag: {tag}")),
            to_iced(value, vsize),
        ].spacing(2).into(),
        Value::Float(float) => text(float.to_string()).into(),
        Value::Bool(value) => text(value.to_string()).into(),
        Value::Null => text("nil").into(),
        Value::Undefined => text("undefined").into(),
        Value::Simple(v) => text(format!("{v:0>2X}")).into(),
        Value::Table { header, rows } => {
            let columns = header.iter().enumerate().map(|(col_i, c)| {
                table::column(to_iced(c, vsize), move |e: &Vec<Value>| {
                    let e: Elem<'_> =
                        if let Some(e) = e.get(col_i) {
                            to_iced(e, vsize)
                        } else {
                            space().into()
                        };
                    e
                })
            }).collect::<Vec<_>>();

            let dir = scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            };

            let height = if let Some(sz) = vsize {
                Length::Bounded {
                    bounds: iced_core::length::Bounds::Max(sz.height * 0.8),
                    sizing: iced_core::length::Sizing::Fit,
                }
            } else {
                Length::Shrink
            };

            scrollable::Scrollable::with_direction(
                container(
                    Table::new(columns, rows).padding_y(1)
                )
                    .padding(iced::Padding {
                        right: 15.,
                        bottom: 15.,
                        left: 0.,
                        top: 0.,
                    })
                    .clip(true),
                dir
            )
                .height(height)
                .into()
        },
        Value::Timestamp(t) => {
            use chrono::{Utc, DateTime};
            let d = DateTime::<Utc>::from_timestamp_millis(*t).unwrap();
            text(d.to_rfc3339()).into()
        }
    }
}
