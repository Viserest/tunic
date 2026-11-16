use super::{Status, StatusKind};

pub struct Response {
    // http, gemini
    pub status: Status,
    // http
    pub headers: ::http::HeaderMap,
    // content_type in gopher, meta in gemini
    pub meta: String,
    // http, gopher, gemini
    pub body: Vec<u8>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            status: Status::StatusKind(StatusKind::NotFound),
            headers: ::http::HeaderMap::new(),
            meta: String::new(),
            body: b"Not Found".into(),
        }
    }
}
