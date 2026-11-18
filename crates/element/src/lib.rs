//! Building blocks of a page for any device (phone, desktop, web, gemini, gopher).
//!
//! Elements: [`section`] (div), [`table`], [`code`], [`text`] (paragraph), [`link`] (anchor),
//! plus headings, lists, and a 12-column [`columns`] grid.
//!
//! A tree of [`Element`]s renders to HTML ([`Element::to_html`]), Gemini gemtext
//! ([`Element::to_gemtext`]) or plain text ([`Element::to_text`]), so a router can answer the same
//! page over every protocol.
//!
//! ```
//! use element::*;
//! let page = section([heading(1, "Hello"), text("world"), link("/x", "go")]);
//! assert_eq!(page.to_html(), "<div><h1>Hello</h1><p>world</p><a href=\"/x\">go</a></div>");
//! assert!(page.to_gemtext().contains("=> /x go"));
//! ```

use std::fmt::Write;

/// Tags rendered without a closing tag.
const VOID: &[&str] = &["br", "hr", "img", "input", "meta", "link"];

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// Text, escaped on output.
    Text(String),
    /// Trusted markup, emitted as-is in html (and ignored by gemtext/text).
    Raw(String),
    Element(Element),
}

impl From<Element> for Node {
    fn from(e: Element) -> Node {
        Node::Element(e)
    }
}
impl From<&str> for Node {
    fn from(s: &str) -> Node {
        Node::Text(s.to_string())
    }
}
impl From<String> for Node {
    fn from(s: String) -> Node {
        Node::Text(s)
    }
}
macro_rules! node_from_display {
    ($($t:ty),*) => {$(
        impl From<$t> for Node { fn from(v: $t) -> Node { Node::Text(v.to_string()) } }
    )*};
}
node_from_display!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64, bool, char);

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

impl Element {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into(), attrs: Vec::new(), children: Vec::new() }
    }

    /// Set an attribute. Repeating a key appends the value separated by a space (`class`).
    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let (key, value) = (key.into(), value.into());
        if let Some((_, v)) = self.attrs.iter_mut().find(|(k, _)| *k == key) {
            v.push(' ');
            v.push_str(&value);
        } else {
            self.attrs.push((key, value));
        }
        self
    }

    pub fn append_attribute(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attr(key, value)
    }

    pub fn append_attributes(mut self, attrs: Vec<(impl Into<String>, impl Into<String>)>) -> Self {
        for (k, v) in attrs {
            self = self.attr(k, v);
        }
        self
    }

    pub fn class(self, c: impl Into<String>) -> Self {
        self.attr("class", c)
    }

    pub fn child(mut self, c: impl Into<Node>) -> Self {
        self.children.push(c.into());
        self
    }

    pub fn append_child(self, c: impl Into<Node>) -> Self {
        self.child(c)
    }

    pub fn children<N: Into<Node>>(mut self, cs: impl IntoIterator<Item = N>) -> Self {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }

    pub fn append_children<N: Into<Node>>(self, cs: Vec<N>) -> Self {
        self.children(cs)
    }

    fn is_void(&self) -> bool {
        VOID.contains(&self.tag.as_str())
    }

    pub fn to_html(&self) -> String {
        let mut s = String::new();
        self.write_html(&mut s);
        s
    }

    fn write_html(&self, out: &mut String) {
        let _ = write!(out, "<{}", self.tag);
        for (k, v) in &self.attrs {
            let _ = write!(out, " {}=\"{}\"", k, escape(v));
        }
        out.push('>');
        if self.is_void() {
            return;
        }
        for c in &self.children {
            match c {
                Node::Text(t) => out.push_str(&escape(t)),
                Node::Raw(r) => out.push_str(r),
                Node::Element(e) => e.write_html(out),
            }
        }
        let _ = write!(out, "</{}>", self.tag);
    }

    /// Concatenated text of this subtree (no markup).
    pub fn inner_text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => s.push_str(t),
                Node::Raw(_) => {}
                Node::Element(e) => s.push_str(&e.inner_text()),
            }
        }
        s
    }

    fn attr_value(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    /// Render as Gemini gemtext.
    ///
    /// Line types: text, `=> url name` links, `#`/`##`/`###` headings, `* ` list items,
    /// `> ` quotes and ```` ``` ```` preformatted toggles.
    pub fn to_gemtext(&self) -> String {
        let mut out = String::new();
        self.write_gem(&mut out);
        // collapse 3+ newlines
        while out.contains("\n\n\n") {
            out = out.replace("\n\n\n", "\n\n");
        }
        out.trim_start_matches('\n').to_string()
    }

    fn write_gem(&self, out: &mut String) {
        let block_end = |out: &mut String| {
            if !out.ends_with('\n') {
                out.push('\n');
            }
        };
        match self.tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let n = (self.tag.as_bytes()[1] - b'0').min(3) as usize;
                block_end(out);
                let _ = writeln!(out, "\n{} {}", "#".repeat(n), self.inner_text());
            }
            "a" => {
                // inline links become link lines
                block_end(out);
                let href = self.attr_value("href").unwrap_or("");
                let t = self.inner_text();
                let _ = writeln!(out, "=> {}{}{}", href, if t.is_empty() { "" } else { " " }, t);
            }
            "ul" | "ol" => {
                block_end(out);
                for c in &self.children {
                    if let Node::Element(li) = c {
                        let _ = writeln!(out, "* {}", li.inner_text());
                    }
                }
                out.push('\n');
            }
            "pre" | "code" => {
                block_end(out);
                let _ = writeln!(out, "```\n{}\n```", self.inner_text().trim_end());
            }
            "table" => {
                block_end(out);
                out.push_str("```\n");
                out.push_str(&self.table_text());
                out.push_str("```\n");
            }
            "blockquote" => {
                block_end(out);
                for l in self.inner_text().lines() {
                    let _ = writeln!(out, "> {l}");
                }
            }
            "br" => out.push('\n'),
            "hr" => {
                block_end(out);
                out.push_str("----\n");
            }
            "script" | "style" | "head" | "input" | "button" | "form" => {}
            "p" => {
                block_end(out);
                let _ = writeln!(out, "\n{}", self.inner_text());
            }
            _ => {
                // container: recurse, text nodes become lines
                for c in &self.children {
                    match c {
                        Node::Text(t) => {
                            block_end(out);
                            let _ = writeln!(out, "{t}");
                        }
                        Node::Raw(_) => {}
                        Node::Element(e) => e.write_gem(out),
                    }
                }
            }
        }
    }

    fn table_rows(&self) -> Vec<Vec<String>> {
        let mut rows = Vec::new();
        fn collect(e: &Element, rows: &mut Vec<Vec<String>>) {
            for c in &e.children {
                if let Node::Element(ch) = c {
                    if ch.tag == "tr" {
                        rows.push(
                            ch.children
                                .iter()
                                .filter_map(|c| if let Node::Element(cell) = c { Some(cell.inner_text()) } else { None })
                                .collect(),
                        );
                    } else {
                        collect(ch, rows);
                    }
                }
            }
        }
        collect(self, &mut rows);
        rows
    }

    fn table_text(&self) -> String {
        let rows = self.table_rows();
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; cols];
        for r in &rows {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(c.chars().count());
            }
        }
        let mut s = String::new();
        for r in &rows {
            let line: Vec<String> =
                r.iter().enumerate().map(|(i, c)| format!("{:<w$}", c, w = widths[i])).collect();
            let _ = writeln!(s, "{}", line.join("  ").trim_end());
        }
        s
    }

    /// Render as plain text (used for Gopher).
    pub fn to_text(&self) -> String {
        match self.tag.as_str() {
            "table" => self.table_text(),
            "script" | "style" => String::new(),
            "a" => {
                let href = self.attr_value("href").unwrap_or("");
                format!("{} <{}>\n", self.inner_text(), href)
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => format!("\n{}\n", self.inner_text().to_uppercase()),
            "li" => format!("* {}\n", self.inner_text()),
            "p" => format!("{}\n", self.inner_text()),
            "br" => "\n".into(),
            _ => {
                let mut s = String::new();
                for c in &self.children {
                    match c {
                        Node::Text(t) => {
                            s.push_str(t);
                            s.push('\n');
                        }
                        Node::Raw(_) => {}
                        Node::Element(e) => s.push_str(&e.to_text()),
                    }
                }
                s
            }
        }
    }
}

/// Short-hand element creation:
///
/// ```
/// use element::tag;
/// assert_eq!(tag!("div").to_html(), "<div></div>");
/// assert_eq!(tag!("div", ["class" => "row"]).to_html(), "<div class=\"row\"></div>");
/// assert_eq!(tag!("div", {"hi"}).to_html(), "<div>hi</div>");
/// assert_eq!(tag!("p", ["id" => "a"], {"x", 1}).to_html(), "<p id=\"a\">x1</p>");
/// ```
#[macro_export]
macro_rules! tag {
    ($name:expr) => { $crate::Element::new($name) };
    ($name:expr, [$($k:expr => $v:expr),+ $(,)?]) => {
        $crate::Element::new($name)$(.attr($k, $v))+
    };
    ($name:expr, {$($c:expr),+ $(,)?}) => {
        $crate::Element::new($name)$(.child($c))+
    };
    ($name:expr, [$($k:expr => $v:expr),+ $(,)?], {$($c:expr),+ $(,)?}) => {
        $crate::Element::new($name)$(.attr($k, $v))+$(.child($c))+
    };
}

// ---------------------------------------------------------------- building blocks

/// Section (html `div`).
pub fn section<N: Into<Node>>(children: impl IntoIterator<Item = N>) -> Element {
    Element::new("div").children(children)
}

/// Text (html paragraph).
pub fn text(t: impl Into<String>) -> Element {
    Element::new("p").child(Node::Text(t.into()))
}

pub fn heading(level: u8, t: impl Into<String>) -> Element {
    Element::new(format!("h{}", level.clamp(1, 6))).child(Node::Text(t.into()))
}

/// Link (html anchor).
pub fn link(href: impl Into<String>, t: impl Into<String>) -> Element {
    Element::new("a").attr("href", href).child(Node::Text(t.into()))
}

/// Code (html `pre` > `code`).
pub fn code(t: impl Into<String>) -> Element {
    Element::new("pre").child(Element::new("code").child(Node::Text(t.into())))
}

pub fn list<N: Into<Node>>(items: impl IntoIterator<Item = N>) -> Element {
    Element::new("ul").children(items.into_iter().map(|i| Element::new("li").child(i)))
}

/// Table with a header row and body rows.
pub fn table<H: Into<String>, C: Into<Node>>(
    headers: impl IntoIterator<Item = H>,
    rows: impl IntoIterator<Item = Vec<C>>,
) -> Element {
    let head = Element::new("tr").children(headers.into_iter().map(|h| Element::new("th").child(Node::Text(h.into()))));
    let mut t = Element::new("table").child(head);
    for r in rows {
        t = t.child(Element::new("tr").children(r.into_iter().map(|c| Element::new("td").child(c))));
    }
    t
}

/// 12-column grid row. Each entry is `(span, content)`; spans are clamped to 1..=12.
pub fn columns(cols: impl IntoIterator<Item = (u8, Element)>) -> Element {
    Element::new("div")
        .class("row")
        .children(cols.into_iter().map(|(span, e)| {
            Element::new("div").class(format!("col-{}", span.clamp(1, 12))).child(e)
        }))
}

/// Base stylesheet implementing the 12-column grid with margins between columns.
pub const BASE_CSS: &str = "\
*{box-sizing:border-box}body{font-family:system-ui,sans-serif;margin:0 auto;max-width:72rem;padding:1rem;line-height:1.5}\
.row{display:flex;flex-wrap:wrap;gap:1rem;margin-bottom:1rem}\
.col-1{flex:0 0 calc(8.33% - 1rem)}.col-2{flex:0 0 calc(16.66% - 1rem)}.col-3{flex:0 0 calc(25% - 1rem)}\
.col-4{flex:0 0 calc(33.33% - 1rem)}.col-5{flex:0 0 calc(41.66% - 1rem)}.col-6{flex:0 0 calc(50% - 1rem)}\
.col-7{flex:0 0 calc(58.33% - 1rem)}.col-8{flex:0 0 calc(66.66% - 1rem)}.col-9{flex:0 0 calc(75% - 1rem)}\
.col-10{flex:0 0 calc(83.33% - 1rem)}.col-11{flex:0 0 calc(91.66% - 1rem)}.col-12{flex:0 0 100%}\
@media(max-width:640px){[class^=col-]{flex:0 0 100%}}\
table{border-collapse:collapse;width:100%}th,td{border:1px solid #8884;padding:.3rem .5rem;text-align:left}\
pre{overflow:auto;background:#8881;padding:.5rem}nav a{margin-right:1rem}input,button,select,textarea{font:inherit;padding:.3rem;margin:.2rem 0}";

/// A full html page.
pub struct Page {
    pub title: String,
    pub body: Vec<Node>,
    pub head_extra: String,
}

impl Page {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), body: Vec::new(), head_extra: String::new() }
    }
    pub fn add(mut self, n: impl Into<Node>) -> Self {
        self.body.push(n.into());
        self
    }
    pub fn raw(mut self, html: impl Into<String>) -> Self {
        self.body.push(Node::Raw(html.into()));
        self
    }
    pub fn head(mut self, html: impl Into<String>) -> Self {
        self.head_extra.push_str(&html.into());
        self
    }
    pub fn to_html(&self) -> String {
        let body = Element::new("body").children(self.body.clone()).to_html();
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{}</style>{}</head>{}</html>",
            escape(&self.title),
            BASE_CSS,
            self.head_extra,
            body
        )
    }
    pub fn to_gemtext(&self) -> String {
        let mut s = format!("# {}\n", self.title);
        s.push_str(&Element::new("div").children(self.body.clone()).to_gemtext());
        s
    }
    pub fn to_text(&self) -> String {
        format!("{}\n{}\n{}", self.title, "=".repeat(self.title.chars().count()), Element::new("div").children(self.body.clone()).to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div_simple() {
        assert_eq!(tag!("div").to_html(), "<div></div>");
        assert_eq!(Element::new("div").child(Element::new("p")).to_html(), "<div><p></p></div>");
    }

    #[test]
    fn attrs_merge_and_order() {
        let a = tag!("div", ["class" => "row", "class" => "one-half columns", "id" => "x"]).to_html();
        assert_eq!(a, "<div class=\"row one-half columns\" id=\"x\"></div>");
    }

    #[test]
    fn nested_macro() {
        let a = tag!("div", ["class" => "row"], {
            tag!("p", {"testing", "what happened"}),
            tag!("p"),
        })
        .to_html();
        assert_eq!(a, "<div class=\"row\"><p>testingwhat happened</p><p></p></div>");
    }

    #[test]
    fn escapes_text_and_attrs() {
        let e = tag!("a", ["href" => "x\"y"], {"<b>&"});
        assert_eq!(e.to_html(), "<a href=\"x&quot;y\">&lt;b&gt;&amp;</a>");
    }

    #[test]
    fn void_tags() {
        assert_eq!(tag!("br").to_html(), "<br>");
        assert_eq!(tag!("input", ["type" => "text"]).to_html(), "<input type=\"text\">");
    }

    #[test]
    fn gemtext() {
        let page = section([
            heading(1, "Title"),
            text("Hello"),
            link("/a", "Alpha"),
            list(["one", "two"]),
            code("let x = 1;"),
            table(["a", "bb"], vec![vec!["1", "2"], vec!["333", "4"]]),
        ]);
        let g = page.to_gemtext();
        assert!(g.contains("# Title"));
        assert!(g.contains("=> /a Alpha"));
        assert!(g.contains("* one\n* two"));
        assert!(g.contains("```\nlet x = 1;\n```"));
        assert!(g.contains("a    bb"), "{g}");
        assert!(g.contains("333  4"), "{g}");
    }

    #[test]
    fn grid_and_page() {
        let g = columns([(4, text("a")), (8, text("b"))]).to_html();
        assert!(g.contains("col-4") && g.contains("col-8"));
        let p = Page::new("T <x>").add(text("hi")).to_html();
        assert!(p.contains("<title>T &lt;x&gt;</title>") && p.contains("<body><p>hi</p></body>"));
    }

    #[test]
    fn plain_text() {
        let t = section([heading(2, "Hi"), table(["k"], vec![vec!["v"]])]).to_text();
        assert!(t.contains("HI") && t.contains("k") && t.contains("v"));
    }
}
