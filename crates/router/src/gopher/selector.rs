use url::{ParseError, Url};

use super::GopherType;

/// Gopher selectors from [`RFC 4266`](https://www.rfc-editor.org/info/rfc4266/)
///
/// Format:\
/// ```[gopher://<host>[:<port>]][GopherPath]```
///
/// Gopher Path:\
/// ```/<gophertype><selector>[%09<search>[%09<gopher+ options]]]```
///
/// Examples:
/// - ```Empty string```: Root menu
/// - ```/1```: Root menu
/// - ```gopher://example.com```: Root menu
/// - ```gopher://example.com/1```: Root menu
/// - ```gopher://example.com/0docs/readme.txt```: Text file at "docs/readme.txt"
/// - ```gopher://example.com/7search%09cats```: Full-text search for "cats"
/// - ```gopher://example.com/0file%09%09+```: Text file at "file" with "+" (default representation, gopher+ option)
#[derive(Debug, PartialEq, Eq)]
pub struct Selector {
    scheme: Option<String>,
    host: Option<String>,
    // Either provided or is 70
    port: u16,
    // Either provided or is GopherType::Submenu
    gophertype: GopherType,
    // Either provided or is "/"
    selector: String,
    search: Option<String>,
    data: Option<String>,
}

impl TryFrom<&str> for Selector {
    type Error = ParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let url = Url::parse(value)?;

        let mut path = url.path();

        let gophertype = path
            .strip_prefix('/')
            .and_then(|path| path.as_bytes().first().copied())
            .map(GopherType::from)
            .unwrap_or(GopherType::Submenu);

        // Remove the leading `/` and the one-byte Gopher type.
        let selector = if path.starts_with('/') {
            path = &path[1..];

            if path.is_empty() {
                "/".to_string()
            } else {
                path[1..].to_string()
            }
        } else {
            "/".to_string()
        };

        Ok(Self {
            scheme: Some(url.scheme().to_string()),
            host: url.host_str().map(str::to_owned),
            port: url.port().unwrap_or(70),
            gophertype,
            selector: percent_encoding::percent_decode_str(&selector)
                .decode_utf8_lossy()
                .into_owned(),
            search: url.query().map(str::to_owned),
            data: url.fragment().map(str::to_owned),
        })
    }
}

#[derive(Debug, Default)]
pub struct SelectorBuilder {
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    gophertype: Option<GopherType>,
    selector: Option<String>,
    search: Option<String>,
    data: Option<String>,
}
impl SelectorBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn scheme<S>(&mut self, scheme: impl Into<String>) -> &mut Self {
        self.scheme = Some(scheme.into());
        self
    }
    pub fn host<S>(&mut self, host: impl Into<String>) -> &mut Self {
        self.host = Some(host.into());
        self
    }
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = Some(port);
        self
    }
    pub fn gophertype(&mut self, gophertype: GopherType) -> &mut Self {
        self.gophertype = Some(gophertype);
        self
    }
    pub fn selector(&mut self, selector: impl Into<String>) -> &mut Self {
        self.selector = Some(selector.into());
        self
    }
    pub fn search<S>(&mut self, search: impl Into<String>) -> &mut Self {
        self.search = Some(search.into());
        self
    }
    pub fn data(&mut self, data: impl Into<String>) -> &mut Self {
        self.data = Some(data.into());
        self
    }

    pub fn build(self) -> Selector {
        Selector {
            scheme: self.scheme,
            host: self.host,
            port: self.port.unwrap_or(70),
            gophertype: self.gophertype.unwrap_or(GopherType::Submenu),
            selector: self.selector.unwrap_or("/".to_string()),
            search: self.search,
            data: self.data,
        }
    }
}
