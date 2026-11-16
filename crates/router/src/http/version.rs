use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    Http09,
    Http10,
    Http11,
    Http2,
    Http3,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Http09 => "HTTP/0.9",
                Self::Http10 => "HTTP/1.0",
                Self::Http11 => "HTTP/1.1",
                Self::Http2 => "HTTP/2",
                Self::Http3 => "HTTP/3",
            }
        )
    }
}
