//! Minimal JSON value, parser and serializer. Also the in-memory form of parsed TOML.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn object() -> Value {
        Value::Object(BTreeMap::new())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }

    /// Dotted path lookup, e.g. `router.protocols.http`.
    pub fn path(&self, path: &str) -> Option<&Value> {
        let mut cur = self;
        for seg in path.split('.') {
            cur = cur.get(seg)?;
        }
        Some(cur)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e18 => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64().and_then(|i| u64::try_from(i).ok())
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Insert into an object (no-op for non-objects).
    pub fn set(&mut self, key: impl Into<String>, v: impl Into<Value>) {
        if let Value::Object(m) = self {
            m.insert(key.into(), v.into());
        }
    }

    /// Set a nested value by path segments, creating objects as needed.
    pub fn set_path(&mut self, path: &[String], v: Value) {
        if path.is_empty() {
            *self = v;
            return;
        }
        if !matches!(self, Value::Object(_)) {
            *self = Value::object();
        }
        if let Value::Object(m) = self {
            let e = m.entry(path[0].clone()).or_insert(Value::Null);
            e.set_path(&path[1..], v);
        }
    }

    /// Deep-merge `other` over `self` (objects merge, everything else replaces).
    pub fn merge(&mut self, other: &Value) {
        match (self, other) {
            (Value::Object(a), Value::Object(b)) => {
                for (k, v) in b {
                    match a.get_mut(k) {
                        Some(existing) => existing.merge(v),
                        None => {
                            a.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
            (s, o) => *s = o.clone(),
        }
    }

    pub fn to_json(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Float(f) => {
                if f.is_finite() {
                    let s = f.to_string();
                    out.push_str(&s);
                    if !s.contains(['.', 'e', 'E']) {
                        out.push_str(".0");
                    }
                } else {
                    out.push_str("null");
                }
            }
            Value::Str(s) => write_str(s, out),
            Value::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Object(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json())
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Value {
            fn from(v: $t) -> Value {
                match i64::try_from(v) { Ok(i) => Value::Int(i), Err(_) => Value::Float(v as f64) }
            }
        }
    )*};
}
from_int!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl From<f64> for Value {
    fn from(v: f64) -> Value {
        Value::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Value {
        Value::Float(v as f64)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Value {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Value {
        Value::Str(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Value {
        Value::Str(v)
    }
}
impl From<&String> for Value {
    fn from(v: &String) -> Value {
        Value::Str(v.clone())
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Value {
        Value::Array(v.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Value {
        v.map(Into::into).unwrap_or(Value::Null)
    }
}
impl From<crate::Uuid> for Value {
    fn from(v: crate::Uuid) -> Value {
        Value::Str(v.to_string())
    }
}

/// Build an object: `json_obj! { "a" => 1, "b" => "x" }`.
#[macro_export]
macro_rules! json_obj {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut o = $crate::Value::object();
        $( o.set($k, $v); )*
        o
    }};
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub pos: usize,
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "json error at {}: {}", self.pos, self.msg)
    }
}
impl std::error::Error for ParseError {}

pub fn parse(input: &str) -> Result<Value, ParseError> {
    let mut p = Parser { s: input.as_bytes(), i: 0, depth: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn err(&self, m: &str) -> ParseError {
        ParseError { msg: m.to_string(), pos: self.i }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }
    fn value(&mut self) -> Result<Value, ParseError> {
        if self.depth > 128 {
            return Err(self.err("nesting too deep"));
        }
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'n') if self.eat("null") => Ok(Value::Null),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                self.depth += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        a.push(self.value()?);
                        self.ws();
                        match self.s.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                break;
                            }
                            _ => return Err(self.err("expected , or ]")),
                        }
                    }
                }
                self.depth -= 1;
                Ok(Value::Array(a))
            }
            Some(b'{') => {
                self.i += 1;
                self.depth += 1;
                let mut m = BTreeMap::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.s.get(self.i) != Some(&b'"') {
                            return Err(self.err("expected string key"));
                        }
                        let k = self.string()?;
                        self.ws();
                        if self.s.get(self.i) != Some(&b':') {
                            return Err(self.err("expected :"));
                        }
                        self.i += 1;
                        self.ws();
                        m.insert(k, self.value()?);
                        self.ws();
                        match self.s.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                break;
                            }
                            _ => return Err(self.err("expected , or }")),
                        }
                    }
                }
                self.depth -= 1;
                Ok(Value::Object(m))
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(self.err("unexpected character")),
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.s[self.i] == b'-' {
            self.i += 1;
        }
        let mut float = false;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'0'..=b'9' => {}
                b'.' | b'e' | b'E' | b'+' | b'-' => float = true,
                _ => break,
            }
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap();
        if !float {
            if let Ok(i) = t.parse::<i64>() {
                return Ok(Value::Int(i));
            }
        }
        t.parse::<f64>().map(Value::Float).map_err(|_| self.err("bad number"))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        if self.i + 4 > self.s.len() {
            return Err(self.err("bad \\u escape"));
        }
        let t = std::str::from_utf8(&self.s[self.i..self.i + 4]).map_err(|_| self.err("bad \\u escape"))?;
        let v = u32::from_str_radix(t, 16).map_err(|_| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else { return Err(self.err("unterminated string")) };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else { return Err(self.err("bad escape")) };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3ff);
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        _ => return Err(self.err("bad escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("invalid utf-8"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"{"a":[1,2.5,-3,true,null],"b":{"c":"x\ny\"z"},"u":"é😀"}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.path("b.c").unwrap().as_str(), Some("x\ny\"z"));
        assert_eq!(v.get("u").unwrap().as_str(), Some("é😀"));
        assert_eq!(parse(&v.to_json()).unwrap(), v);
    }

    #[test]
    fn errors() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("1 2").is_err());
        assert!(parse(&"[".repeat(500)).is_err());
    }

    #[test]
    fn helpers() {
        let mut v = json_obj! {"a" => 1, "b" => "x"};
        v.set_path(&["c".into(), "d".into()], Value::Bool(true));
        assert_eq!(v.path("c.d"), Some(&Value::Bool(true)));
        let mut base = json_obj! {"a" => json_obj!{"x" => 1, "y" => 2}};
        base.merge(&json_obj! {"a" => json_obj!{"y" => 3}});
        assert_eq!(base.path("a.x").unwrap().as_i64(), Some(1));
        assert_eq!(base.path("a.y").unwrap().as_i64(), Some(3));
        assert_eq!(Value::Float(2.0).to_json(), "2.0");
    }
}
