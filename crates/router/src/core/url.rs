//! Urls following RFC-3986
//!
//! Scheme is an enum of KnownScheme, or String
//! Authority is an enum of IpAddress, or String
//! Path is a Vec<String> for now.
//! Multimap is a HashMap<String, Vec<String>> for now.
//!
//! Fields:
//! - Scheme ("https", Scheme struct)
//! - Username ("username", String)
//! - Password ("password", String)
//! - Authority ("example.com", Authority struct)
//! - Path ("/many/multiple/much", Path struct)
//! - Query ("?query=hello&query=world", Multimap struct)
//! - Fragment ("#section-8", String)
//!

use std::{collections::HashMap, fmt::Display, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
    Gopher,
    Gemini,
    Unknown(String),
}

impl Display for Scheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "http"),
            Self::Https => write!(f, "https"),
            Self::Gopher => write!(f, "gopher"),
            Self::Gemini => write!(f, "gemini"),
            Self::Unknown(a) => a.fmt(f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authority {
    IpAddr(IpAddr),
    Unknown(String),
}

impl Display for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IpAddr(a) => a.fmt(f),
            Self::Unknown(a) => a.fmt(f),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Path {
    inner: Vec<String>,
}

impl Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.concat().fmt(f)
    }
}

#[derive(Debug, Eq)]
pub struct Url {
    scheme: Option<Scheme>,
    username: Option<String>,
    password: Option<String>,
    authority: Option<Authority>,
    path: Path,
    query: Option<Multimap>,
    fragment: Option<String>,
}

impl Url {
    /// Get Url Builder
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Get Url Scheme
    pub fn scheme(&self) -> Option<Scheme> {
        self.scheme.clone()
    }
    /// Get Url Scheme as a String
    pub fn scheme_str(&self) -> String {
        self.scheme
            .as_ref()
            .and_then(|f| Some(f.to_string()))
            .unwrap_or("".to_string())
    }

    /// Get Url Username
    pub fn username(&self) -> Option<String> {
        self.username.clone()
    }

    /// Get Url Password
    pub fn password(&self) -> Option<String> {
        self.password.clone()
    }

    /// Get Url Authority
    pub fn authority(&self) -> Option<Authority> {
        self.authority.clone()
    }
    /// Get Url Authority as a String
    pub fn authority_str(&self) -> Option<String> {
        self.authority.as_ref().and_then(|f| Some(f.to_string()))
    }

    /// Get Url Path
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    pub fn query(&self) -> Option<Multimap> {
        self.query.clone()
    }
}

impl PartialEq for Url {
    fn eq(&self, other: &Self) -> bool {
        self.path().eq(&other.path())
    }
}

#[derive(Debug)]
enum UrlError {
    InvalidScheme,
    InvalidUsername,
    InvalidPassword,
    InvalidAuthority,
    InvalidPath,
    InvalidQuery,
    InvalidFragment,
}

#[derive(Debug, Default)]
struct Parts {
    scheme: Option<Scheme>,
    username: Option<String>,
    password: Option<String>,
    authority: Option<Authority>,
    path: Option<Path>,
    query: Option<Multimap>,
    fragment: Option<String>,
}

#[derive(Debug)]
pub struct Builder {
    parts: Result<Parts, UrlError>,
}

impl Builder {
    pub fn build(self) -> Result<Url, UrlError> {
        todo!()
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            parts: Ok(Parts::default()),
        }
    }
}
