/// Version wrapper for the different protocols.
///
/// This is also used as a protocol type enum. Each enum and nested enum has
/// a different request and response. So this is a perfect memory location for
/// how to parse the request and respond.
///
/// [`Version`] wrapper around [`::http::Version`](Http Version)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VersionWrapper {
    Http(crate::http::Version),
    Gopher,
    Gemini,
}

/// Combined Uri format
///
/// Http and gemini use the same RFC-3986 uris
/// Gopher uses Gopher Selectors
#[derive(Debug, PartialEq, Eq)]
pub enum UrlWrapper {
    // RFC-3986 uri
    Http(crate::core::Url),
    // Gopher selectors
    Gopher(crate::gopher::Selector),
    // RFC-3986 uri
    Gemini(crate::core::Url),
}

#[derive(Debug)]
pub struct Request {
    // http, gopher, gemini
    version: crate::core::VersionWrapper,
    // http, gopher and gemini are always GET
    method: crate::core::Method,
    // uri in http, selector in gopher, url in gemini
    url: crate::core::UrlWrapper,
    // http
    headers: crate::http::HeaderMap,
    // http
    extensions: crate::http::Extensions,
    // http
    body: Vec<u8>,
}

impl Request {
    pub fn builder() -> Builder {
        Builder::new()
    }
}

enum Error {
    InvalidVersion,
    InvalidMethod,
    InvalidUrl,
    InvalidHeader,
    InvalidExtension,
}

#[derive(Default)]
struct Parts {
    version: Option<crate::core::Version>,
    method: Option<crate::core::Method>,
    url: Option<crate::core::Url>,
    headers: Option<crate::http::HeaderMap>,
    extensions: Option<crate::http::Extensions>,
}

pub struct Builder {
    parts: Result<Parts, Error>,
}

impl Builder {
    /// Get new empty Request Builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the Version for this Request Builder
    pub fn version<T>(self, version: T) -> Self
    where
        T: TryInto<crate::Version>,
        <T as TryInto<crate::Version>>::Error: Into<Error>,
    {
        self.map(move |mut parts| {
            let version = version.try_into().map_err(Into::into)?;
            parts.version = Some(version);
            Ok(parts)
        })
    }

    /// Set the Method for this Request Builder
    pub fn method<T>(self, method: T) -> Self
    where
        T: TryInto<::http::Method>,
        <T as TryInto<::http::Method>>::Error: Into<Error>,
    {
        self.map(move |mut parts| {
            let method = method.try_into().map_err(Into::into)?;
            parts.method = Some(method);
            Ok(parts)
        })
    }

    /// Set the Url for this Request Builder
    pub fn url<T>(self, url: T) -> Self
    where
        T: TryInto<crate::Url>,
        <T as TryInto<crate::Url>>::Error: Into<Error>,
    {
        self.map(move |mut parts| {
            let url = url.try_into().map_err(Into::into)?;
            parts.url = Some(url);
            Ok(parts)
        })
    }

    /// Set the Headers for this Request Builder
    pub fn headers<T>(self, headers: T) -> Self
    where
        T: TryInto<::http::HeaderMap>,
        <T as TryInto<::http::HeaderMap>>::Error: Into<Error>,
    {
        self.map(move |mut parts| {
            let headers = headers.try_into().map_err(Into::into)?;
            parts.headers = Some(headers);
            Ok(parts)
        })
    }

    /// Set the Extensions for this Request Builder
    pub fn extensions<T>(self, extensions: T) -> Self
    where
        T: TryInto<::http::Extensions>,
        <T as TryInto<::http::Extensions>>::Error: Into<Error>,
    {
        self.map(move |mut parts| {
            let extensions = extensions.try_into().map_err(Into::into)?;
            parts.extensions = Some(extensions);
            Ok(parts)
        })
    }

    /// Build this Request Builder into Request
    pub fn build(self) -> Result<Request, Error> {
        let parts = self.parts?;
        Request::from_parts(parts).map_err(Into::into)
    }

    fn map<F>(self, func: F) -> Self
    where
        F: FnOnce(Parts) -> Result<Parts, Error>,
    {
        Builder {
            parts: self.parts.and_then(func),
        }
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            parts: Ok(Parts::default()),
        }
    }
}

impl From<Request> for Builder {
    fn from(request: Request) -> Self {
        Self {
            parts: Ok(request.into_parts()),
        }
    }
}
