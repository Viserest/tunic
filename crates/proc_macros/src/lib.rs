//! `#[derive(Record)]`: turn a plain Rust struct into a database record.
//!
//! ```ignore
//! #[derive(Clone, Record)]
//! #[record(collection = "songs")]          // optional, defaults to the snake_case struct name
//! struct Song {
//!     id: Uuid,                            // required: the primary key
//!     title: String,
//!     #[record(default)]                   // missing in stored/incoming json => Default::default()
//!     plays: u64,
//!     #[record(hidden)]                    // never shown by the generic http crud routes
//!     secret: String,
//! }
//! ```
//!
//! The derive is written against `proc_macro` only (no `syn`/`quote`), so it adds no dependencies.
//! Use `#[record(crate = "tunic::db")]` when the `database` crate is reached through a re-export.

use proc_macro::{Delimiter, TokenStream, TokenTree};

struct Field {
    name: String,
    default: bool,
    hidden: bool,
}

fn compile_error(msg: &str) -> TokenStream {
    format!("compile_error!({:?});", msg).parse().unwrap()
}

/// Parse `key = "value"`, `flag` items out of an attribute's parenthesised content.
fn attr_items(stream: TokenStream) -> Vec<(String, Option<String>)> {
    let toks: Vec<TokenTree> = stream.into_iter().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let TokenTree::Ident(id) = &toks[i] {
            let key = id.to_string();
            if matches!(toks.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '=') {
                if let Some(TokenTree::Literal(l)) = toks.get(i + 2) {
                    out.push((key, Some(l.to_string().trim_matches('"').to_string())));
                    i += 3;
                    continue;
                }
            }
            out.push((key, None));
        } else if let TokenTree::Ident(id) = &toks[i] {
            out.push((id.to_string(), None));
        }
        i += 1;
    }
    out
}

/// If `tok`/`next` is `#[record(...)]`, return its items.
fn record_attr(group: &proc_macro::Group) -> Option<Vec<(String, Option<String>)>> {
    if group.delimiter() != Delimiter::Bracket {
        return None;
    }
    let inner: Vec<TokenTree> = group.stream().into_iter().collect();
    match (inner.first(), inner.get(1)) {
        (Some(TokenTree::Ident(id)), Some(TokenTree::Group(g))) if id.to_string() == "record" => Some(attr_items(g.stream())),
        _ => None,
    }
}

fn snake(name: &str) -> String {
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.extend(c.to_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

#[proc_macro_derive(Record, attributes(record))]
pub fn derive_record(input: TokenStream) -> TokenStream {
    let toks: Vec<TokenTree> = input.into_iter().collect();
    let mut collection: Option<String> = None;
    let mut krate = "::database".to_string();
    let mut i = 0;
    let mut name = String::new();
    let mut body = None;

    while i < toks.len() {
        match &toks[i] {
            TokenTree::Punct(p) if p.as_char() == '#' => {
                if let Some(TokenTree::Group(g)) = toks.get(i + 1) {
                    if let Some(items) = record_attr(g) {
                        for (k, v) in items {
                            match (k.as_str(), v) {
                                ("collection", Some(v)) => collection = Some(v),
                                ("crate", Some(v)) => krate = v,
                                _ => return compile_error("unknown #[record(..)] option on struct"),
                            }
                        }
                    }
                }
                i += 2;
                continue;
            }
            TokenTree::Ident(id) if id.to_string() == "struct" => {
                match toks.get(i + 1) {
                    Some(TokenTree::Ident(n)) => name = n.to_string(),
                    _ => return compile_error("expected struct name"),
                }
                match toks.get(i + 2) {
                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => body = Some(g.stream()),
                    Some(TokenTree::Punct(p)) if p.as_char() == '<' => return compile_error("derive(Record) does not support generic structs"),
                    _ => return compile_error("derive(Record) only supports structs with named fields"),
                }
                break;
            }
            TokenTree::Ident(id) if id.to_string() == "enum" || id.to_string() == "union" => {
                return compile_error("derive(Record) only supports structs");
            }
            _ => {}
        }
        i += 1;
    }
    let Some(body) = body else { return compile_error("derive(Record) only supports structs with named fields") };

    // fields
    let ft: Vec<TokenTree> = body.into_iter().collect();
    let mut fields: Vec<Field> = Vec::new();
    let mut j = 0;
    while j < ft.len() {
        let (mut default, mut hidden) = (false, false);
        // attributes
        while matches!(&ft.get(j), Some(TokenTree::Punct(p)) if p.as_char() == '#') {
            if let Some(TokenTree::Group(g)) = ft.get(j + 1) {
                if let Some(items) = record_attr(g) {
                    for (k, _) in items {
                        match k.as_str() {
                            "default" => default = true,
                            "hidden" => hidden = true,
                            _ => return compile_error("unknown #[record(..)] option on field"),
                        }
                    }
                }
            }
            j += 2;
        }
        // visibility
        if matches!(&ft.get(j), Some(TokenTree::Ident(id)) if id.to_string() == "pub") {
            j += 1;
            if matches!(&ft.get(j), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis) {
                j += 1;
            }
        }
        let fname = match ft.get(j) {
            Some(TokenTree::Ident(id)) => id.to_string(),
            None => break,
            _ => return compile_error("expected field name"),
        };
        j += 1;
        if !matches!(&ft.get(j), Some(TokenTree::Punct(p)) if p.as_char() == ':') {
            return compile_error("expected `:` after field name");
        }
        j += 1;
        // skip the type up to the next top-level comma
        let mut depth = 0i32;
        while j < ft.len() {
            match &ft[j] {
                TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
                TokenTree::Punct(p) if p.as_char() == '>' => depth -= 1,
                TokenTree::Punct(p) if p.as_char() == ',' && depth <= 0 => break,
                _ => {}
            }
            j += 1;
        }
        j += 1; // the comma
        fields.push(Field { name: fname.trim_start_matches("r#").to_string(), default, hidden });
    }

    if !fields.iter().any(|f| f.name == "id") {
        return compile_error("derive(Record) requires a field `id: Uuid`");
    }

    let collection = collection.unwrap_or_else(|| snake(&name));
    let mut to_v = String::new();
    let mut from_v = String::new();
    for f in &fields {
        to_v.push_str(&format!("o.set({n:?}, {k}::FieldValue::to_value(&self.{n}));\n", n = f.name, k = krate));
        let missing = if f.default {
            "::core::default::Default::default()".to_string()
        } else {
            format!("{k}::FieldValue::from_missing().map_err(|e| format!(\"field `{n}`: {{e}}\"))?", k = krate, n = f.name)
        };
        from_v.push_str(&format!(
            "{n}: match v.get({n:?}) {{ Some(x) if !x.is_null() || {opt} => {k}::FieldValue::from_value(x).map_err(|e| format!(\"field `{n}`: {{e}}\"))?, _ => {missing} }},\n",
            n = f.name,
            k = krate,
            missing = missing,
            opt = "false",
        ));
    }
    let hidden: Vec<String> = fields.iter().filter(|f| f.hidden).map(|f| format!("{:?}", f.name)).collect();
    let code = format!(
        "impl {k}::Record for {name} {{
            const COLLECTION: &'static str = {coll:?};
            fn id(&self) -> {k}::Uuid {{ self.id }}
            fn to_value(&self) -> {k}::Value {{
                let mut o = {k}::Value::object();
                {to_v}
                o
            }}
            fn from_value(v: &{k}::Value) -> ::core::result::Result<Self, ::std::string::String> {{
                if v.as_object().is_none() {{ return Err(\"expected a json object\".to_string()); }}
                Ok(Self {{ {from_v} }})
            }}
            fn hidden_fields() -> &'static [&'static str] {{ &[{hidden}] }}
        }}",
        k = krate,
        name = name,
        coll = collection,
        to_v = to_v,
        from_v = from_v,
        hidden = hidden.join(", "),
    );
    code.parse().unwrap_or_else(|_| compile_error("derive(Record) generated invalid code"))
}
