use std::fmt::Display;

pub struct Bolger {
    stack: Vec<&'static str>,
}

impl Bolger {
    pub fn new() -> Bolger {
        Bolger {
            stack: Vec::new(),
        }
    }

    pub fn begin(&mut self, s: &'static str) {
        self.stack.push(s);
        print!("({s} ");
    }

    pub fn end(&mut self, s: &str) {
        let f = self.stack.pop();
        if f != Some(s) {
            panic!("Tried to end {s} section, but was in {f:?} section instead.");
        }
        print!(")");
    }

    pub fn attr_str<A: Display, V: Display>(&mut self, a: A, v: V) {
        assert!(!self.stack.is_empty());
        print!(" :{a} \"{v}\"");
    }

    pub fn attr_num<A: Display, V: Into<f64>>(&mut self, a: A, v: V) {
        assert!(!self.stack.is_empty());
        print!(" :{a} {}", v.into());
    }

    pub fn str<S: Display>(&mut self, s: S) {
        print!(" \"{s}\" ");
    }

    pub fn num<V: Into<f64>>(&mut self, v: V) {
        print!(" {}", v.into());
    }
}
