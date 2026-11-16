use crate::Element;

pub enum Node {
    String(String),
    Element(Element),
}

impl Node {
    pub fn to_html(&self) -> String {
        match self {
            Self::String(a) => a.clone(),
            Self::Element(a) => a.to_html(),
        }
    }
    pub fn to_gemtext(&self) -> String {
        match self {
            Self::String(a) => a.clone(),
            Self::Element(a) => a.to_gemtext(),
        }
    }
}

impl Into<Node> for Element {
    fn into(self) -> Node {
        Node::Element(self)
    }
}

impl Into<Node> for String {
    fn into(self) -> Node {
        Node::String(self)
    }
}

impl Into<Node> for &str {
    fn into(self) -> Node {
        Node::String(self.to_string())
    }
}

macro_rules! impl_to_node {
    ($s:ident) => {
        impl Into<Node> for $s {
            fn into(self) -> Node {
                Node::String(self.to_string())
            }
        }
    };
}

// Implement Into<Node> for primitive types
impl_to_node!(u8);
impl_to_node!(u16);
impl_to_node!(u32);
impl_to_node!(u64);
impl_to_node!(u128);
impl_to_node!(usize);
impl_to_node!(i8);
impl_to_node!(i16);
impl_to_node!(i32);
impl_to_node!(i64);
impl_to_node!(i128);
impl_to_node!(isize);
impl_to_node!(f32);
impl_to_node!(f64);
impl_to_node!(bool);
impl_to_node!(char);
