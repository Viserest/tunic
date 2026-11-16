use std::collections::HashMap;

mod node;
pub use node::*;

/// HTML Element Structures
///
/// Can be built into HTML tag structures or Gemini Gemtext
pub struct Element {
    tag_name: String,
    attributes: HashMap<String, String>,
    children: Vec<Node>,
}

impl Element {
    /// Create new element
    pub fn new(tag_name: impl Into<String>) -> Element {
        Element {
            tag_name: tag_name.into(),
            attributes: HashMap::new(),
            children: Vec::new(),
        }
    }

    /// Append a single attribute
    pub fn append_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Element {
        let key = key.into();
        let value = value.into();
        if self.attributes.contains_key(&key) {
            self.attributes.get_mut(&key).and_then(|f| {
                f.push(' ');
                f.push_str(&value);
                Some(f)
            });
        } else {
            self.attributes.insert(key, value);
        }
        self
    }

    /// Append many attributes using a [`Vec`]
    pub fn append_attributes(
        mut self,
        attributes: Vec<(impl Into<String>, impl Into<String>)>,
    ) -> Element {
        for (key, value) in attributes {
            let key = key.into();
            let value = value.into();
            if self.attributes.contains_key(&key) {
                self.attributes.get_mut(&key).and_then(|f| {
                    f.push(' ');
                    f.push_str(&value);
                    Some(f)
                });
            } else {
                self.attributes.insert(key, value);
            }
        }
        self
    }

    /// Append a single child
    pub fn append_child(mut self, child: impl Into<Node>) -> Element {
        self.children.push(child.into());
        self
    }

    /// Append many children using a [`Vec`]
    pub fn append_children(mut self, children: Vec<impl Into<Node>>) -> Element {
        for child in children {
            self.children.push(child.into());
        }
        self
    }

    /// Builds all child elements into HTML tags
    ///
    /// ```pseudo
    /// <TAG ATTR_NAME="ATTR_VALUE"><CHILDREN[]></TAG>
    /// ```
    pub fn to_html(&self) -> String {
        format!(
            "<{0}{1}>{2}</{0}>",
            self.tag_name,
            self.attributes
                .iter()
                .map(|(key, value)| format!(" {}=\"{}\"", key, value))
                .collect::<String>(),
            self.children
                .iter()
                .map(|f| f.to_html())
                .collect::<String>(),
        )
    }

    /// Builds all child elements into Gemtext
    ///
    /// Specifications: [`gemini://geminiprotocol.net/docs/gemtext-specification.gmi`]
    ///
    /// The six line types are:
    /// 1. Text lines (default)
    /// 2. Link lines ("=> <URL> <LINK NAME>")
    /// 3. Heading lines ("#", largest; "##", middle; "###", smallest)
    /// 4. List items ("* ")
    /// 5. Quote lines ("> ")
    /// 6. Preformat toggle lines ("```")
    fn to_gemtext(&self) -> String {
        match self.tag_name.as_str() {
            // anchor
            "a" => format!(
                "=> {}",
                self.children
                    .iter()
                    .map(|f| f.to_gemtext())
                    .collect::<String>()
            ),
            // lists
            "ol" => self
                .children
                .iter()
                .map(|f| f.to_gemtext())
                .collect::<String>(),
            "ul" => self
                .children
                .iter()
                .map(|f| f.to_gemtext())
                .collect::<String>(),
            _ => "".to_string(),
        }
    }
}

/// Short-hand element creation
///
/// Create an empty div
/// ```rust
/// tag!("div") -> "Div {attributes: none, children: none}"
/// ```
/// ```html
/// <div></div>
/// ```
///
/// Create a div with attributes
/// ```rust
/// tag!("div", ["class" => "row"]);
/// ```
/// ```html
/// <div class="row"></div>
/// ```
///
/// Create a div with children elements
/// ```rust
/// tag!("div", {tag!("p")});
/// ```
/// ```html
/// <div><p></p></div>
/// ```
///
/// Create a div with attributes and children elements
/// ```rust
/// tag!("div", ["class" => "row"], {tag!("p")});
/// ```
/// ```html
/// <div class="row"><p></p></div>
/// ```
#[allow(unused_macros)]
macro_rules! tag {
    // just tag name
    ($name:literal) => {
        Element::new($name)
    };

    // tagname and attributes
    ($name:literal,
        [$( $key:expr => $value:expr ),+ $(,)?]
    ) => {
        Element::new($name)
            .append_attributes(vec![
                $(($key, $value)),+
            ])
    };

    // tagname and children
    ($name:literal,
        {$( $children:expr ),+ $(,)?}
    ) => {
        Element::new($name)
            .append_children(vec![
                $($children),+
            ])
    };

    // tagname, attributes and children
    ($name:literal,
        [$( $key:expr => $value:expr ),+ $(,)?],
        {$( $children:expr ),+ $(,)?}
    ) => {
        Element::new($name)
            .append_attributes(vec![
                $(($key, $value)),+
            ])
            .append_children(vec![
                $($children),+
            ])
    };
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use crate::Element;

    #[test]
    fn div_non_macro() {
        let a = Element::new("div").to_html();
        let b = "<div></div>";
        assert_eq!(a, b);
    }

    #[test]
    fn div_children_non_macro() {
        let a = Element::new("div")
            .append_child(Element::new("p"))
            .to_html();
        let b = "<div><p></p></div>";
        assert_eq!(a, b);
    }

    #[test]
    fn div_attrs_children_non_macro() {
        let a = Element::new("div")
            .append_attributes(vec![("id", "entry"), ("class", "row")])
            .append_child(Element::new("p"))
            .to_html();
        let b = "<div id=\"entry\" class=\"row\"><p></p></div>";
        let c = "<div class=\"row\" id=\"entry\"><p></p></div>";
        assert!(a == b || a == c);
    }

    #[test]
    fn div_simple() {
        let a = tag!("div").to_html();
        let b = "<div></div>";
        assert_eq!(a, b);
    }

    #[test]
    fn div_attrs() {
        let a = tag!("div", ["class" => "row", "class" => "one-half columns"]).to_html();
        let b = "<div class=\"row one-half columns\"></div>";
        assert_eq!(a, b);
    }

    #[test]
    fn div_attrs_children() {
        let a = tag!("div", ["class" => "row", "class" => "one-half columns"], {
            tag!("p", {
                "testing",
                "what happened"
            }),
            tag!("p"),
        })
        .to_html();
        let b = "<div class=\"row one-half columns\"><p>testingwhat happened</p><p></p></div>";
        assert_eq!(a, b);
    }

    #[test]
    fn div_attrs_many() {
        // Paragraph element
        let a = tag!("p", [
            "id" => "message",
            "class" => "doctor name",
            "href" => "https://example.com",
            "data-target" => "324789234892",
        ]);
        // Attributes
        let mut b: HashMap<String, String> = HashMap::new();
        let _ = b.insert("id".into(), "message".into());
        let _ = b.insert("class".into(), "doctor name".into());
        let _ = b.insert("href".into(), "https://example.com".into());
        let _ = b.insert("data-target".into(), "324789234892".into());
        assert!(a.attributes == b);
    }
}
