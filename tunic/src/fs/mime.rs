use std::path::Path;

use serde::{Deserialize, Serialize};

/// Mime types have 3 identifiers to its type:
/// 1. By extension: ".html",
/// 2. By mime type: "text/html",
/// 3. By this enum: TextHtml
#[derive(PartialEq, Clone, Copy, Serialize, Deserialize, Debug)]
pub enum MimeType {
    // Text types
    TextPlain,
    TextCSV,
    TextJson,
    TextHtml,
    TextCss,
    TextJavascript,
    // Image types
    ImageJpeg,
    ImagePNG,
    ImageSVGXML,
    ImageGIF,
    ImageAVIF,
    ImageBMP,
    // Application types
    ApplicationOctetStream,
}

impl MimeType {
    /// Get [`MimeType`] from a String path.
    ///
    /// Defaults to MimeType::ApplicationOctetStream
    pub fn from_ext<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        match path.as_ref().extension().and_then(|f| f.to_str()) {
            Some("txt") => Self::TextPlain,
            Some("csv") => Self::TextCSV,
            Some("json") => Self::TextJson,
            Some("html") => Self::TextHtml,
            Some("css") => Self::TextCss,
            Some("js") => Self::TextJavascript,
            Some("jpeg") | Some("jpg") => Self::ImageJpeg,
            Some("png") => Self::ImagePNG,
            Some("svg") => Self::ImageSVGXML,
            Some("gif") => Self::ImageGIF,
            Some("avif") => Self::ImageAVIF,
            Some("bmp") => Self::ImageBMP,
            _ => Self::ApplicationOctetStream,
        }
    }

    /// Get [`MimeType`] from a String path.
    ///
    /// Defaults to MimeType::ApplicationOctetStream
    pub fn from_mime_type<P>(mime_type: P) -> Self
    where
        P: Into<String>,
    {
        match mime_type.into().as_str() {
            "text/plain" => Self::TextPlain,
            "text/csv" => Self::TextCSV,
            "text/json" => Self::TextJson,
            "text/html" => Self::TextHtml,
            "text/css" => Self::TextCss,
            "text/javascript" => Self::TextJavascript,
            "image/jpeg" => Self::ImageJpeg,
            "image/png" => Self::ImagePNG,
            "image/svg" => Self::ImageSVGXML,
            "image/gif" => Self::ImageGIF,
            "image/avif" => Self::ImageAVIF,
            "image/bmp" => Self::ImageBMP,
            _ => Self::ApplicationOctetStream,
        }
    }

    /// Get file extension from [`MimeType`].
    ///
    /// Defaults to "bin"
    pub fn to_ext(&self) -> &'static str {
        match self {
            Self::TextPlain => "txt",
            Self::TextCSV => "csv",
            Self::TextJson => "json",
            Self::TextHtml => "html",
            Self::TextCss => "css",
            Self::TextJavascript => "js",
            // Image types
            Self::ImageJpeg => "jpeg",
            Self::ImagePNG => "png",
            Self::ImageSVGXML => "svg",
            Self::ImageGIF => "gif",
            Self::ImageAVIF => "avif",
            Self::ImageBMP => "bmp",
            // Application types
            _ => "bin",
        }
    }

    /// Get file extension from [`MimeType`].
    ///
    /// Defaults to "application/octet-stream"
    pub fn to_mime_type(&self) -> &'static str {
        match self {
            Self::TextPlain => "text/plain",
            Self::TextCSV => "text/csv",
            Self::TextJson => "text/json",
            Self::TextHtml => "text/html",
            Self::TextCss => "text/css",
            Self::TextJavascript => "text/javascript",
            // Image types
            Self::ImageJpeg => "image/jpeg",
            Self::ImagePNG => "image/png",
            Self::ImageSVGXML => "image/svg+xml",
            Self::ImageGIF => "image/gif",
            Self::ImageAVIF => "image/avif",
            Self::ImageBMP => "image/bmp",
            // Application types
            _ => "application/octet-stream",
        }
    }
}

pub const IMAGE_MIME_TYPES: [MimeType; 6] = [
    MimeType::ImageJpeg,
    MimeType::ImagePNG,
    MimeType::ImageSVGXML,
    MimeType::ImageGIF,
    MimeType::ImageAVIF,
    MimeType::ImageBMP,
];
