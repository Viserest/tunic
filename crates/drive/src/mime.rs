/// Content types the drive knows by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MimeType(pub &'static str);

const TABLE: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("csv", "text/csv"),
    ("json", "application/json"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("xml", "application/xml"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("tar", "application/x-tar"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("svg", "image/svg+xml"),
    ("mp3", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("flac", "audio/flac"),
    ("wav", "audio/wav"),
    ("opus", "audio/opus"),
    ("m4a", "audio/mp4"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
];

impl MimeType {
    pub const OCTET_STREAM: MimeType = MimeType("application/octet-stream");

    pub fn from_ext(name: &str) -> MimeType {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        TABLE.iter().find(|(e, _)| *e == ext).map(|(_, m)| MimeType(m)).unwrap_or(MimeType::OCTET_STREAM)
    }

    pub fn from_mime(m: &str) -> Option<MimeType> {
        let base = m.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        TABLE.iter().find(|(_, t)| *t == base).map(|(_, t)| MimeType(t))
    }

    pub fn is_media(&self) -> bool {
        self.0.starts_with("audio/") || self.0.starts_with("video/") || self.0.starts_with("image/")
    }
}

/// Types the browser may render inline from our origin. Everything else (html, svg, xml,
/// javascript, ...) is served as a download so an uploaded file can never run as a page.
pub fn safe_to_inline(mime: &str) -> bool {
    matches!(mime, "text/plain" | "application/pdf" | "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif" | "image/bmp")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
}

/// Strip any path, control characters and quotes; limit the length. Never returns an empty name.
pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| !c.is_control() && !matches!(c, '"' | '<' | '>' | ':' | '|' | '?' | '*')).take(200).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "file".into() } else { cleaned }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types() {
        assert_eq!(MimeType::from_ext("a.PNG").0, "image/png");
        assert_eq!(MimeType::from_ext("noext"), MimeType::OCTET_STREAM);
        assert_eq!(MimeType::from_mime("text/html; charset=utf-8").unwrap().0, "text/html");
        assert!(MimeType::from_ext("song.flac").is_media());
        assert!(safe_to_inline("image/png") && !safe_to_inline("text/html") && !safe_to_inline("image/svg+xml"));
    }

    #[test]
    fn filenames() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Users\\a\\b.txt"), "b.txt");
        assert_eq!(sanitize_filename("we\"ird\r\nname.txt"), "weirdname.txt");
        assert_eq!(sanitize_filename(""), "file");
        assert_eq!(sanitize_filename("..."), "file");
        assert_eq!(sanitize_filename(&"a".repeat(500)).len(), 200);
    }
}
