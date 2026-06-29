use std::str::Chars;
use std::iter::Peekable;

#[cfg(not(target_os = "macos"))]
const IS_MACOS: bool = false;
#[cfg(target_os = "macos")]
const IS_MACOS: bool = true;

#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    String(String),
    Number(f64),
    Array(Vec<AttrValue>),
    Node(Node),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Text(String),
    Number(f64),
    Sexp {
        tag: String,
        attrs: Vec<(String, AttrValue)>,
        children: Vec<Node>,
    },
}

pub struct Parser<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            input: src.chars().peekable(),
        }
    }

    pub fn parse_document(&mut self) -> Result<Vec<Node>, String> {
        let mut nodes = Vec::new();
        while let Some(&c) = self.input.peek() {
            match c {
                '(' => nodes.push(self.parse_element()?),
                ')' => Err(format!("Unexpected ')'"))?,
                '"' => nodes.push(Node::Text(self.read_string()?)),

                '\n' | '\r' | '\t' | ' ' => _ = self.next_char(),

                c if c == '-' || c.is_ascii_digit() => {
                    let text = self.read_free_text();
                    if let Ok(f) = text.parse::<f64>() {
                        nodes.push(Node::Number(f));
                    } else {
                        Err(format!("Invalid number {text}"))?;
                    }
                },
                c => Err(format!("Unexpected character '{c}'"))?,
            }
        }
        Ok(nodes)
    }

    fn parse_element(&mut self) -> Result<Node, String> {
        self.expect('(')?;
        self.skip_ws();

        let tag = self.read_ident()?;

        let mut attrs = Vec::new();
        let mut children = Vec::new();

        loop {
            self.skip_ws();
            match self.peek_char() {
                Some(')') => {
                    self.next_char();
                    break;
                }
                Some(':') => {
                    self.next_char();
                    let key = self.read_ident()?;
                    self.skip_ws();
                    let value = self.parse_attr_value()?;
                    attrs.push((key, value));
                }
                Some('(') => children.push(self.parse_element()?),
                Some('"') => children.push(Node::Text(self.read_string()?)),
                Some(c) if c.is_ascii_digit() || c == '-' => {
                    children.push(Node::Text(self.read_number()?.to_string()));
                }
                Some(c) => Err(format!("Unexpected character inside element {c:?}"))?,
                None => return Err("Unexpected EOF inside element".into()),
            }
        }

        Ok(Node::Sexp {
            tag,
            attrs,
            children,
        })
    }

    fn parse_attr_value(&mut self) -> Result<AttrValue, String> {
        self.skip_ws();
        match self.peek_char() {
            Some('"') => Ok(AttrValue::String(self.read_string()?)),
            Some('[') => {
                self.next_char(); // '['
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    if self.peek_char() == Some(']') {
                        self.next_char();
                        break;
                    }
                    items.push(self.parse_attr_value()?);
                }
                Ok(AttrValue::Array(items))
            }
            Some('(') => Ok(AttrValue::Node(self.parse_element()?)),
            Some(c) if c.is_ascii_digit() || c == '-' => {
                Ok(AttrValue::Number(self.read_number()?))
            }
            c => Err(format!("Invalid attribute value {c:?}")),
        }
    }

    fn read_free_text(&mut self) -> String {
        let mut s = String::new();
        while let Some(&c) = self.input.peek() {
            if c == '(' || c == '"' || c == ')' {
                break;
            }
            s.push(c);
            self.next_char();
        }
        s
    }

    fn read_ident(&mut self) -> Result<String, String> {
        let mut s = String::new();
        while let Some(&c) = self.input.peek() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                s.push(c);
                self.next_char();
            } else {
                break;
            }
        }
        if s.is_empty() {
            Err("Expected identifier".into())
        } else {
            Ok(s)
        }
    }

    fn read_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut s = String::new();
        while let Some(c) = self.next_char() {
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let escaped = match self.next_char() {
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some(c) => c,
                        None => return Err("Unterminated escape".into()),
                    };
                    s.push(escaped);
                }

                // Skip irrelevant newline characters
                '\r' if !IS_MACOS => _ = self.next_char(),
                '\n' if IS_MACOS => _ = self.next_char(),

                _ => s.push(c),
            }
        }
        Err("Unterminated string".into())
    }

    fn read_number(&mut self) -> Result<f64, String> {
        let mut s = String::new();
        if self.peek_char() == Some('-') {
            s.push('-');
            self.next_char();
        }
        while let Some(&c) = self.input.peek() {
            if c.is_ascii_digit() || c == '.' {
                s.push(c);
                self.next_char();
            } else {
                break;
            }
        }
        s.parse().map_err(|_| "Invalid number".into())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek_char(), Some(c) if c.is_whitespace()) {
            self.next_char();
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        match self.next_char() {
            Some(c) if c == expected => Ok(()),
            _ => Err(format!("Expected '{}'", expected)),
        }
    }

    fn peek_char(&mut self) -> Option<char> {
        self.input.peek().copied()
    }

    fn next_char(&mut self) -> Option<char> {
        self.input.next()
    }
}

pub fn parse(src: &str) -> Result<Vec<Node>, String> {
    Parser::new(src).parse_document()
}

#[cfg(test)]
mod tests {
    use super::*;
 
    #[test]
    fn test_bold_italic() {
        let src = r#"Hello (b "world") and (i "stuff")"#;
        let nodes = parse(src).unwrap();
        assert!(nodes.len() >= 3);
        // "Hello " is free text at index 0, <b> is at index 1
        let b = nodes.iter().find(|n| matches!(n, Node::Sexp { tag, .. } if tag == "b"));
        assert!(matches!(b.unwrap(), Node::Sexp { inner: Some(_), .. }));
    }
 
    #[test]
    fn test_nested() {
        let src = r#"(b (i "amazing"))"#;
        let nodes = parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
        if let Node::Sexp { tag, inner: Some(inner), .. } = &nodes[0] {
            assert_eq!(tag, "b");
            if let Node::Sexp { tag: inner_tag, .. } = inner.as_ref() {
                assert_eq!(inner_tag, "i");
            } else {
                panic!("inner should be element");
            }
        } else {
            panic!("expected element with inner");
        }
    }
 
    #[test]
    fn test_void_element_no_inner() {
        let src = r#"(row :node "foo" :pred "bar" :dist "baz")"#;
        let nodes = parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
        if let Node::Sexp { tag, inner, attrs, .. } = &nodes[0] {
            assert_eq!(tag, "row");
            assert!(inner.is_none(), "row should have no inner text");
            assert_eq!(attrs.len(), 3);
        } else {
            panic!("expected element");
        }
    }
 
    #[test]
    fn test_table() {
        let src = r#"(table :columns ["node" "pred" "dist"] :id "mytable"
            (row :node "foo" :pred "bar" :dist "dist")
            (row :node "bar" :pred "baz" :dist 3))"#;
        let nodes = parse(src).unwrap();
        assert_eq!(nodes.len(), 1);
        if let Node::Sexp { tag, children, attrs, inner } = &nodes[0] {
            assert_eq!(tag, "table");
            assert!(inner.is_none());
            assert_eq!(children.len(), 2);
            // :columns attr is an array
            let col_attr = attrs.iter().find(|(k, _)| k == "columns").unwrap();
            assert!(matches!(&col_attr.1, AttrValue::Array(_)));
        } else {
            panic!("expected table element");
        }
    }
 
    #[test]
    fn test_number_dist_attr() {
        let src = r#"(row :dist 3)"#;
        let nodes = parse(src).unwrap();
        if let Node::Sexp { attrs, .. } = &nodes[0] {
            let dist = attrs.iter().find(|(k, _)| k == "dist").unwrap();
            assert_eq!(dist.1, AttrValue::Number(3.0));
        }
    }
 
    #[test]
    fn test_full_example() {
        let src = r#"Hey, it's me again, here's an (b "example") of bold text, and here's an (i "example") of italic text. (b (i "amazing")) huh? Here's a table, even:
 
(table :columns ["node" "pred" "dist"] :id "mytable"
    (row :node "foo" :pred "bar" :dist "dist")
    (row :node "bar" :pred "baz" :dist 3))"#;
        let nodes = parse(src).unwrap();
        // Should have: free text, b, free text, i, free text, b(i), free text, table
        // At minimum these element nodes:
        let elements: Vec<_> = nodes.iter().filter(|n| matches!(n, Node::Sexp { .. })).collect();
        assert!(elements.len() >= 4, "Expected at least 4 elements, got {}", elements.len());
    }
}
