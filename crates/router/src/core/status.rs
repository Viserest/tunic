//! [`Status`], [`StatusCode`], and [`StatusKind`]

mod status_code {
    use std::fmt::Display;

    /// Wrapper around [`crate::http::StatusCode`](Http StatusCode) and [`crate::gemini::StatusCode`](Gemini StatusCode)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StatusCode {
        Http(crate::http::StatusCode),
        Gopher,
        Gemini(crate::gemini::StatusCode),
    }

    // Convert between crate::http::StatusCode and crate::core::StatusCode
    impl From<crate::http::StatusCode> for StatusCode {
        fn from(value: crate::http::StatusCode) -> Self {
            Self::Http(value)
        }
    }

    impl TryInto<crate::http::StatusCode> for StatusCode {
        type Error = ();
        fn try_into(self) -> Result<crate::http::StatusCode, Self::Error> {
            match self {
                Self::Http(a) => Ok(a),
                _ => Err(()),
            }
        }
    }

    // Convert between crate::core::StatusCode and u16
    impl Into<u16> for StatusCode {
        fn into(self) -> u16 {
            match self {
                StatusCode::Http(a) => Into::into(a),
                StatusCode::Gopher => 0,
                StatusCode::Gemini(a) => <_ as Into<u8>>::into(a) as u16,
            }
        }
    }

    impl Display for StatusCode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                StatusCode::Http(a) => a.fmt(f),
                StatusCode::Gopher => write!(f, ""),
                StatusCode::Gemini(a) => a.fmt(f),
            }
        }
    }
}

mod status_kind {
    macro_rules! helper {
        ($($kind:ident, $http_code:path, $gemini_code:path;)*) => {
            /// [`StatusKind`] can be mapped into a [`crate::http::StatusCode`](Http StatusCode) or [`crate::gemini::StatusCode`](Gemini StatusCode)
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub enum StatusKind {
                $($kind,)*
            }

            // Convert between crate::http::StatusCode and crate::core::StatusKind
            impl TryFrom<crate::http::StatusCode> for StatusKind {
                type Error = ();
                fn try_from(value: crate::http::StatusCode) -> Result<Self, Self::Error> {
                    match value {
                        $($http_code => Ok(StatusKind::$kind),)*
                        _ => Err(()),
                    }
                }
            }

            impl Into<crate::http::StatusCode> for StatusKind {
                fn into(self) -> crate::http::StatusCode {
                    match self {
                        $(StatusKind::$kind => $http_code,)*
                    }
                }
            }

            // Convert between crate::gemini::StatusCode and crate::core::StatusKind
            impl TryFrom<crate::gemini::StatusCode> for StatusKind {
                type Error = ();
                fn try_from(value: crate::gemini::StatusCode) -> Result<Self, Self::Error> {
                    match value {
                        $($gemini_code => Ok(StatusKind::$kind),)*
                        _ => Err(()),
                    }
                }
            }

            impl Into<crate::gemini::StatusCode> for StatusKind {
                fn into(self) -> crate::gemini::StatusCode {
                    match self {
                        $(StatusKind::$kind => $gemini_code),*
                    }
                }
            }
        };
    }

    helper! {
        Success,           crate::http::StatusCode::Ok,                  crate::gemini::StatusCode::Success;
        RedirectTemporary, crate::http::StatusCode::RedirectTemporary,   crate::gemini::StatusCode::RedirectTemporary;
        RedirectPermanent, crate::http::StatusCode::RedirectPermanent,   crate::gemini::StatusCode::RedirectPermanent;
        BadRequest,        crate::http::StatusCode::BadRequest,          crate::gemini::StatusCode::BadRequest;
        NotFound,          crate::http::StatusCode::NotFound,            crate::gemini::StatusCode::NotFound;
        Gone,              crate::http::StatusCode::Gone,                crate::gemini::StatusCode::Gone;
        Failure,           crate::http::StatusCode::InternalServerError, crate::gemini::StatusCode::FailurePermanent;
    }
}

pub use status_code::StatusCode;
pub use status_kind::StatusKind;

/// [`Status`] wraps [`StatusCode`] and [`StatusKind`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    StatusCode(StatusCode),
    StatusKind(StatusKind),
}
