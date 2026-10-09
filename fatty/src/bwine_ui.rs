use iced::{Background, Border, Padding, Length, alignment};
use iced::widget::{container, row, text, void, space, responsive, table::{self, Table}, grid, column, Widget};
use bwine::Value;

use crate::helpers::*;
use iced::widget::scrollable;
use crate::Elem;

pub fn to_iced<'a>(value: &'a Value, vsize: Option<iced::Size>) -> Elem<'a> {
    match value {
        Value::Path(p) => rtext(p.display().to_string()).boxed(),
        Value::Int(int) => rtext(int.to_string()).boxed(),
        Value::Bytes(bytes) => rtext(String::from_utf8_lossy(&bytes[..(50).min(bytes.len())])).boxed(),
        Value::Text(value) => rtext(value.to_string()).boxed(),
        Value::Array(values) => grid(values.iter().map(|v| to_iced(v, vsize))).boxed(),
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
            ).boxed()
        },
        Value::Tag(tag, value) => column![
            rtext(format!("Tag: {tag}")),
            to_iced(value, vsize),
        ].spacing(2).boxed(),
        Value::Float(float) => rtext(float.to_string()).boxed(),
        Value::Bool(value) => rtext(value.to_string()).boxed(),
        Value::Null => rtext("nil").boxed(),
        Value::Undefined => rtext("undefined").boxed(),
        Value::Simple(v) => rtext(format!("{v:0>2X}")).boxed(),
        Value::Table { header, rows } => {
            let columns = header.iter().enumerate().map(|(col_i, c)| {
                table::column(to_iced(c, vsize), move |e: &Vec<Value>| {
                    if let Some(e) = e.get(col_i) {
                        to_iced(e, vsize)
                    } else {
                        void().boxed()
                    }
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
                .boxed()
        },
        Value::Timestamp(t) => {
            use chrono::{Utc, DateTime};
            let d = DateTime::<Utc>::from_timestamp_millis(*t).unwrap();
            text(d.to_rfc3339()).boxed()
        }
    }
}
