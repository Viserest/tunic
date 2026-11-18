//! Parser for the practical subset of TOML used by `tunic.toml`.
//!
//! Supported: comments, `[table.paths]` (bare or quoted segments), `[[array.of.tables]]`,
//! `key = value` with basic/literal strings, integers, floats, booleans, arrays and inline tables,
//! dotted keys. Not supported: multi-line strings, datetimes.

use crate::json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct TomlError {
    pub line: usize,
    pub msg: String,
}
impl std::fmt::Display for TomlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "toml error on line {}: {}", self.line, self.msg)
    }
}
impl std::error::Error for TomlError {}

pub fn parse(input: &str) -> Result<Value, TomlError> {
    let mut root = Value::object();
    let mut current: Vec<String> = Vec::new();
    let mut array_table = false;
    for (n, raw) in input.lines().enumerate() {
        let line_no = n + 1;
        let err = |m: &str| TomlError { line: line_no, msg: m.to_string() };
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("[[") {
            let h = h.strip_suffix("]]").ok_or_else(|| err("unterminated [[header]]"))?;
            current = split_key(h).map_err(|m| err(&m))?;
            array_table = true;
            // append a fresh table to the array
            let mut parent = &mut root;
            for seg in &current[..current.len() - 1] {
                parent = descend(parent, seg);
            }
            let last = current.last().ok_or_else(|| err("empty header"))?;
            let m = parent.as_object_mut().unwrap();
            let arr = m.entry(last.clone()).or_insert_with(|| Value::Array(Vec::new()));
            match arr {
                Value::Array(a) => a.push(Value::object()),
                _ => return Err(err("key is not an array of tables")),
            }
            continue;
        }
        if let Some(h) = line.strip_prefix('[') {
            let h = h.strip_suffix(']').ok_or_else(|| err("unterminated [header]"))?;
            current = split_key(h).map_err(|m| err(&m))?;
            array_table = false;
            let mut t = &mut root;
            for seg in &current {
                t = descend(t, seg);
            }
            continue;
        }
        let (k, v) = split_assign(&line).ok_or_else(|| err("expected key = value"))?;
        let key = split_key(k).map_err(|m| err(&m))?;
        let mut p = ValParser { s: v.trim().as_bytes(), i: 0 };
        let val = p.value().map_err(|m| err(&m))?;
        p.ws();
        if p.i != p.s.len() {
            return Err(err("trailing characters after value"));
        }
        let mut t = &mut root;
        for (idx, seg) in current.iter().enumerate() {
            t = descend(t, seg);
            if array_table && idx == current.len() - 1 {
                if let Value::Array(a) = t {
                    t = a.last_mut().ok_or_else(|| err("empty array of tables"))?;
                }
            }
        }
        for seg in &key[..key.len() - 1] {
            t = descend(t, seg);
        }
        let m = t.as_object_mut().ok_or_else(|| err("cannot assign into non-table"))?;
        if m.contains_key(key.last().unwrap()) {
            return Err(err("duplicate key"));
        }
        m.insert(key.last().unwrap().clone(), val);
    }
    Ok(root)
}

fn descend<'a>(v: &'a mut Value, seg: &str) -> &'a mut Value {
    if !matches!(v, Value::Object(_)) {
        *v = Value::object();
    }
    let m = v.as_object_mut().unwrap();
    let e = m.entry(seg.to_string()).or_insert_with(Value::object);
    if matches!(e, Value::Array(a) if !a.is_empty()) {
        // `[a.b]` after `[[a]]` addresses the last element
        if let Value::Array(a) = e {
            return a.last_mut().unwrap();
        }
    }
    e
}

fn strip_comment(line: &str) -> &str {
    let mut in_str: Option<char> = None;
    let mut esc = false;
    for (i, c) in line.char_indices() {
        match in_str {
            Some(q) => {
                if esc {
                    esc = false;
                } else if c == '\\' && q == '"' {
                    esc = true;
                } else if c == q {
                    in_str = None;
                }
            }
            None => match c {
                '"' | '\'' => in_str = Some(c),
                '#' => return &line[..i],
                _ => {}
            },
        }
    }
    line
}

fn split_assign(line: &str) -> Option<(&str, &str)> {
    let mut in_str: Option<char> = None;
    for (i, c) in line.char_indices() {
        match in_str {
            Some(q) if c == q => in_str = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => in_str = Some(c),
                '=' => return Some((&line[..i], &line[i + 1..])),
                _ => {}
            },
        }
    }
    None
}

/// Split `a.b."c.d"` into segments.
pub fn split_key(k: &str) -> Result<Vec<String>, String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut was_quoted = false;
    for c in k.trim().chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    was_quoted = true;
                }
                '.' => {
                    if cur.trim().is_empty() && !was_quoted {
                        return Err("empty key segment".into());
                    }
                    segs.push(if was_quoted { cur.clone() } else { cur.trim().to_string() });
                    cur.clear();
                    was_quoted = false;
                }
                c if c.is_whitespace() && !was_quoted => {}
                c => cur.push(c),
            },
        }
    }
    if quote.is_some() {
        return Err("unterminated quoted key".into());
    }
    if cur.trim().is_empty() && !was_quoted {
        return Err("empty key".into());
    }
    segs.push(if was_quoted { cur } else { cur.trim().to_string() });
    Ok(segs)
}

struct ValParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl ValParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.s.get(self.i) {
            None => Err("missing value".into()),
            Some(b'"') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    let c = *self.s.get(self.i).ok_or("unterminated string")?;
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let e = *self.s.get(self.i).ok_or("bad escape")?;
                            self.i += 1;
                            out.push(match e {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                b'"' => b'"',
                                b'\\' => b'\\',
                                _ => return Err("unsupported escape".into()),
                            });
                        }
                        c => out.push(c),
                    }
                }
                String::from_utf8(out).map(Value::Str).map_err(|_| "invalid utf-8".into())
            }
            Some(b'\'') => {
                self.i += 1;
                let start = self.i;
                while self.s.get(self.i).ok_or("unterminated string")? != &b'\'' {
                    self.i += 1;
                }
                let t = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
                self.i += 1;
                Ok(Value::Str(t))
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b']') {
                        self.i += 1;
                        break;
                    }
                    a.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err("expected , or ]".into()),
                    }
                }
                Ok(Value::Array(a))
            }
            Some(b'{') => {
                self.i += 1;
                let mut o = Value::object();
                loop {
                    self.ws();
                    if self.s.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        break;
                    }
                    let start = self.i;
                    while self.i < self.s.len() && self.s[self.i] != b'=' {
                        self.i += 1;
                    }
                    let key = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "bad key")?;
                    let segs = split_key(key)?;
                    self.i += 1;
                    let v = self.value()?;
                    o.set_path(&segs, v);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            break;
                        }
                        _ => return Err("expected , or }".into()),
                    }
                }
                Ok(o)
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len() && !matches!(self.s[self.i], b',' | b']' | b'}') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap().trim().replace('_', "");
                match t.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => {
                        if let Ok(i) = t.parse::<i64>() {
                            Ok(Value::Int(i))
                        } else if let Ok(f) = t.parse::<f64>() {
                            Ok(Value::Float(f))
                        } else {
                            Err(format!("cannot parse value `{t}`"))
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_example() {
        let v = parse(
            r#"
# Web router is enabled
[router]
enabled = true

[router.protocols]
gopher = 70
http = 80
https = 443
gemini = 1965

[router.tags]
all = true

[router.access]
public = "0.0.0.0"
admin = "10.1.0.0/16"   # trailing comment

[router.reverse-proxy]
"example.com" = "localhost:2455"
"illustration.com" = "localhost:2456"

[database]
enabled = true
port = 54000
"#,
        )
        .unwrap();
        assert_eq!(v.path("router.enabled").unwrap().as_bool(), Some(true));
        assert_eq!(v.path("router.protocols.gemini").unwrap().as_i64(), Some(1965));
        assert_eq!(v.path("router.access.admin").unwrap().as_str(), Some("10.1.0.0/16"));
        assert_eq!(
            v.get("router").unwrap().get("reverse-proxy").unwrap().get("example.com").unwrap().as_str(),
            Some("localhost:2455")
        );
        assert_eq!(v.path("database.port").unwrap().as_i64(), Some(54000));
    }

    #[test]
    fn arrays_inline_and_dotted() {
        let v = parse("a = [1, 2, \"x#y\"]\nb = { c = 1, d.e = true }\nf.g = 2.5\n").unwrap();
        assert_eq!(v.get("a").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(v.path("b.d.e").unwrap().as_bool(), Some(true));
        assert_eq!(v.path("f.g").unwrap().as_f64(), Some(2.5));
    }

    #[test]
    fn array_of_tables() {
        let v = parse("[[job]]\nname = \"a\"\n[[job]]\nname = \"b\"\n").unwrap();
        assert_eq!(v.get("job").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn errors() {
        assert!(parse("a = ").is_err());
        assert!(parse("a = 1\na = 2").is_err());
        assert!(parse("[x").is_err());
    }
}
