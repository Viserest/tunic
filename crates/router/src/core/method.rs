use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Head,
    Options,
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Head => "HEAD",
                Self::Options => "OPTIONS",
                Self::Get => "GET",
                Self::Post => "POST",
                Self::Put => "PUT",
                Self::Patch => "PATCH",
                Self::Delete => "DELETE",
            }
        )
    }
}
