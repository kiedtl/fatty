use std::collections::HashMap;

use bitflags::bitflags;
use iced::widget::text::{Span, Rich};
use iced::widget::span;

use crate::bolger::parser::*;
use crate::styles::Theme;

pub type Id = String;

bitflags! {
    #[derive(PartialEq, Copy, Clone)]
    pub struct TextFlags: u8 {
        const NONE   = 0b00000000;
        const BOLD   = 0b00000001;
        const ITALIC = 0b00000010;
        const MONO   = 0b00000100;
        const SERIF  = 0b00001000;
    }
}

#[derive(Default, PartialEq, Copy, Clone)]
pub enum TextSize {
    #[default]
    Normal,
    H(u8),
}

#[derive(PartialEq, Copy, Clone)]
pub struct Style {
    flags: TextFlags,
    size: TextSize,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            flags: TextFlags::NONE,
            size: Default::default()
        }
    }
}

impl Style {
    fn font(&self) -> iced::Font {
        let mut font = iced::Font::default();

        if self.flags & TextFlags::BOLD == TextFlags::BOLD {
            font.weight = iced::font::Weight::Bold;
        }

        if self.flags & TextFlags::ITALIC == TextFlags::ITALIC {
            font.style = iced::font::Style::Italic;
        }

        let is_mono = self.flags & TextFlags::ITALIC == TextFlags::MONO;
        let is_serif = self.flags & TextFlags::ITALIC == TextFlags::SERIF;
        font.family = match (is_mono, is_serif) {
            (true, true)   => iced::font::Family::name("Drafting* Mono"),
            (true, false)  => iced::font::Family::name("Fira Code"),
            (false, true)  => iced::font::Family::name("Nimbus Serif"),
            (false, false) => iced::font::Family::name("Nimbus Sans"),
        };

        font.weight = match self.size {
            TextSize::Normal => font.weight,
            TextSize::H(_) => iced::font::Weight::Semibold,
        };

        font
    }

    fn size(&self) -> f32 {
        match self.size {
            TextSize::Normal => 16.,
            TextSize::H(1) => 24.,
            TextSize::H(2) => 18.,
            TextSize::H(3) => 16.,
            //TextSize::H(4) => 24.,
            //TextSize::H(5) => 18.,
            //TextSize::H(6) => 16.,
            TextSize::H(_) => 2., // not reachable
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    name: String,
}

#[derive(Debug, Clone)]
pub enum Element {
    Number(f64),
    Text(String),
    Bold(Box<Element>),
    Italic(Box<Element>),
    Header(u8, Box<Element>),
    Table {
        columns: Vec<TableColumn>,
        rows: Vec<Vec<Option<Element>>>,
    },
    Column(TableColumn),
    Id(Id),
}

impl Element {
    pub fn is_block(&self) -> bool {
        match self {
            Element::Number(_) | Element::Bold(_) | Element::Text(_) | Element::Italic(_) => false,
            _ => true,
        }
    }

    pub fn to_iced_span<'a>(&'a self, style: Style, ids: &'a HashMap<Id, Element>) -> Span<'a, iced::Never> {
        assert!(!self.is_block());

        let mut v = match self {
            Element::Text(s) => span(s.clone()).size(style.size()).font(style.font()),
            Element::Number(s) => span(s.to_string()).size(style.size()).font(style.font()),
            Element::Bold(e) => e.to_iced_span(Style { flags: style.flags | TextFlags::BOLD, ..style }, ids),
            Element::Italic(e) => e.to_iced_span(Style { flags: style.flags | TextFlags::ITALIC, ..style }, ids),
            Element::Id(id) => ids.get(id).unwrap().to_iced_span(style, ids),
            _ => unreachable!(),
        };
        v = v.line_height(1.15);
        v
    }

    pub fn to_iced<'a>(&'a self, style: Style, ids: &'a HashMap<Id, Element>) -> crate::Elem<'a> {
        use iced::widget::{text, space, table::{self, Table}};
        use crate::Elem;

        match self {
            Element::Header(l, e) => e.to_iced(Style { size: TextSize::H(*l), ..style }, ids),
            Element::Table { columns, rows } => {
                let columns = columns.iter().enumerate().map(|(col_i, c)| {
                    table::column(text(c.name.clone()), move |e: &Vec<Option<Element>>| {
                        let e: Elem<'_> =
                            if let Some(Some(e)) = e.get(col_i) {
                                e.to_iced(style, ids)
                            } else {
                                space().into()
                            };
                        e
                    })
                }).collect::<Vec<_>>();

                Table::new(columns, rows)
                    .padding_y(1)
                    .into()
            },
            Element::Column(c) => text(format!("<column {c:?}>")).into(),
            Element::Id(id) => ids.get(id).unwrap().to_iced(style, ids),
            c => Rich::with_spans(vec![c.to_iced_span(style, ids)]).into(),
        }
    }
}

pub struct Document {
    pub elements: Vec<Element>,
    pub ids: HashMap<Id, Element>,
}

pub fn consume_ast(ast: &[Node]) -> Result<Document, String> {
    let mut ids = HashMap::new();
    let mut elements = Vec::new();
    for node in ast {
        elements.push(consume(node, &mut ids)?);
    }
    Ok(Document { elements, ids })
}

pub fn consume(node: &Node, ids: &mut HashMap<Id, Element>) -> Result<Element, String> {
    let process_style_elem = |
        attrs: &[(String, AttrValue)],
        children: &[Node],
        ids: &mut HashMap<Id, Element>,
    | -> Result<Box<Element>, String> {
        if !attrs.is_empty() {
            Err(format!("`b` and `i` should not have attributes"))?;
        }

        if children.len() != 1 {
            Err(format!("`b` and `i` should only have one non-block child"))?;
        }

        let inner = consume(&children[0], ids)?;
        if inner.is_block() {
            Err(format!("`b` and `i` should only have one non-block child"))?;
        }

        Ok(Box::new(inner))
    };

    let mut id = None;
    match node {
        Node::Sexp { attrs, .. } => {
            for (attr, value) in attrs {
                match attr.as_str() {
                    "id" => match value {
                        AttrValue::String(s) => {
                            id = Some(s.clone());
                            break;
                        }
                        _ => Err(format!("Id must be a string"))?,
                    },
                    _ => (),
                }
            }
        },
        _ => (),
    }

    let elem = match node {
        Node::Text(s) => Element::Text(s.clone()),
        Node::Number(f) => Element::Number(*f),

        Node::Sexp { tag, attrs, children } if tag == "b" => Element::Bold(process_style_elem(attrs, children, ids)?),
        Node::Sexp { tag, attrs, children } if tag == "i" => Element::Italic(process_style_elem(attrs, children, ids)?),
        Node::Sexp { tag, attrs, children } if tag == "h1" => Element::Header(1, process_style_elem(attrs, children, ids)?),
        Node::Sexp { tag, attrs, children } if tag == "h2" => Element::Header(2, process_style_elem(attrs, children, ids)?),
        Node::Sexp { tag, attrs, children } if tag == "h3" => Element::Header(3, process_style_elem(attrs, children, ids)?),

        Node::Sexp { tag, attrs, children } if tag == "table" => {
            let mut columns: Vec<TableColumn> = Vec::new();
            let mut rows: Vec<Vec<Option<Element>>> = Vec::new();

            for (attr, value) in attrs {
                match attr.as_str() {
                    "id" => (),
                    "columns" => {
                        match value {
                            AttrValue::Array(a) => for value in a {
                                match value {
                                    AttrValue::Node(n) => {
                                        match consume(n, ids)? {
                                            Element::Column(c) => columns.push(c),
                                            _ => Err(format!("Expected list of columns"))?,
                                        }
                                    },
                                    _ => Err(format!("Expected list of columns"))?,
                                }
                            },
                            _ => Err(format!("Expected list of columns"))?,
                        }
                    },
                    s => Err(format!("Unknown table attribute {s}"))?,
                }
            }

            for child in children {
                let mut values = Vec::new();
                match child {
                    Node::Sexp { tag, attrs, children } if tag == "row"=> {
                        for (attr, _value) in attrs {
                            match attr.as_str() {
                                s => Err(format!("Unknown row attribute {s}"))?,
                            }
                        }

                        for child in children {
                            values.push(Some(consume(child, ids)?));
                        }
                    },
                    _ => Err(format!("Expected row"))?,
                }
                rows.push(values);
            }

            Element::Table { columns, rows }
        },
        Node::Sexp { tag, attrs, children } if tag == "column" => {
            for (attr, _value) in attrs {
                match attr {
                    s => Err(format!("Unknown column attribute {s}"))?,
                }
            }

            if children.len() != 1 {
                Err(format!("Expected one inner value for column."))?;
            }

            let name = match &children[0] {
                Node::Number(f) => f.to_string(),
                Node::Text(s) => s.clone(),
                _ => Err(format!("Column inner value must not be a block."))?,
            };

            Element::Column(TableColumn { name })
        }
        Node::Sexp { tag, .. } if tag == "row" => Err(format!("Row element can only appear within a table."))?,
        Node::Sexp { tag, .. } => Err(format!("Unknown element {tag}."))?,
    };

    if let Some(id) = id {
        ids.insert(id.clone(), elem);
        Ok(Element::Id(id))
    } else {
        Ok(elem)
    }
}
