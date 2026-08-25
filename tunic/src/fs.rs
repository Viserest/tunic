mod mime {
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
}
pub use mime::{IMAGE_MIME_TYPES, MimeType};

mod permission {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Deserialize, Serialize)]
    pub enum Principal {
        Commenter,
        Viewer,
        Editor,
        Owner,
    }

    #[derive(Deserialize, Serialize)]
    pub struct Permission {
        id: Uuid,
        user_id: Uuid,
        level: Principal,
        created_at: u64,
        accessed_at: u64,
        deleted_at: u64,
    }
}
pub use permission::{Permission, Principal};

mod filemeta {
    use std::path::{self, PathBuf};

    use axum::{
        Extension, Json,
        body::Bytes,
        extract::Path,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
    };
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tokio::fs;
    use uuid::Uuid;

    use crate::{
        State,
        fs::{IMAGE_MIME_TYPES, MimeType, Permission},
    };

    #[derive(Deserialize, Serialize)]
    #[tunic_macro::make_crud(methods = [get, delete], state = State, table = state.files)]
    pub struct Filemeta {
        id: Uuid,
        og_name: String,
        content_type: MimeType,
        permissions: Vec<Permission>,
        size: usize,
        hash: [u8; 32],
        created_at: u64,
        deleted_at: u64,
    }

    impl Filemeta {
        /// Returns the absolute path of the disk stored file
        fn path(&self) -> PathBuf {
            let mut result = PathBuf::new();
            if let Some(a) = option_env!("STATE_DIR") {
                result.push(a);
            }
            result.push("files");
            result.push(self.id.to_string());
            result.add_extension(MimeType::to_ext(&self.content_type));
            path::absolute(result).unwrap()
        }
        pub async fn handle_post(
            Extension(state): Extension<State>,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            // Must have authorization, og filename, content type, and content length headers
            let (auth, og_filename, content_type, content_length) = match (
                headers.get("authorization").and_then(|f| f.to_str().ok()),
                headers.get("og-filename").and_then(|f| f.to_str().ok()),
                headers.get("content-type").and_then(|f| f.to_str().ok()),
                headers
                    .get("content-length")
                    .and_then(|f| f.to_str().ok())
                    .and_then(|f| f.parse::<usize>().ok()),
            ) {
                (Some(a), Some(og), Some(t), Some(l)) => (a, og, MimeType::from_mime_type(t), l),
                _ => {
                    tracing::warn!("handle_file_post missing required headers.");
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"msg": "Missing required headers"})),
                    )
                        .into_response();
                }
            };
            let (mime_types, max_size) = match auth {
                // Coops create message (admin)
                "Bearer 1f461ce5-6c03-428b-9cbf-85985cf94845" => (IMAGE_MIME_TYPES, 10_000_000),
                // Csigns contact page (public)
                "Bearer 317b0ad6-0b26-4165-8b57-824c2d5953bc" => (IMAGE_MIME_TYPES, 4_000_000),
                // Csigns create product (admin)
                "Bearer 00f215ce-29be-4a26-a4f7-3f309c227688" => (IMAGE_MIME_TYPES, 10_000_000),
                _ => {
                    tracing::warn!("handle_file_post invalid authorization header.");
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"msg": "Request authorization header is invalid"})),
                    )
                        .into_response();
                }
            };

            // Actual body length is different than content length header
            if body.len() != content_length {
                tracing::warn!(
                    "In file upload, body length is not equal to content length header. Continuing..."
                );
            }

            // Request is larger than max size
            if body.len() > max_size {
                tracing::warn!("handle_file_post request is larger than max size.");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"msg": "Request content size is too large"})),
                )
                    .into_response();
            }

            // Request has a different mime type than allowed
            if !mime_types.contains(&content_type) {
                tracing::warn!("handle_file_post request contains invalid content type.");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"msg": "Request content type is invalid"})),
                )
                    .into_response();
            }

            // Check if file was already provided
            let data = body.to_vec();
            let hash = Sha256::digest(&data).0;
            for (_, file) in state.files.iter().await {
                let file = file.lock().await;
                if file.hash == hash {
                    tracing::info!("File already created. Returning previous file id...");
                    return (
                        StatusCode::OK,
                        Json(json!({"msg": "Saved file", "file_id": file.id})),
                    )
                        .into_response();
                }
            }

            // Create file meta
            let file_id = Uuid::new_v4();
            let size = data.len();
            let meta = Filemeta {
                id: file_id,
                og_name: og_filename.into(),
                content_type: content_type,
                permissions: Vec::new(),
                size,
                hash,
                created_at: crate::unix_now(),
                deleted_at: 0,
            };

            // Create file
            let path = meta.path();
            tracing::info!("Saving file to {}.", path.display());
            fs::create_dir(&path.parent().unwrap()).await.unwrap();
            if let Err(e) = fs::write(path, data).await {
                tracing::warn!("handle_file_post {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"msg": "Internal server error"})),
                )
                    .into_response();
            }

            // Write filemeta to database
            {
                if state.files.insert(meta.id, meta).await.is_some() {
                    tracing::warn!("Overwrote key.");
                }
            }

            tracing::info!("Saved file.");
            (
                StatusCode::CREATED,
                Json(json!({"msg": "Saved file", "file_id": file_id})),
            )
                .into_response()
        }

        pub async fn handle_send(
            Extension(state): Extension<State>,
            Path(id): Path<Uuid>,
        ) -> Response {
            let file = match state.files.get(&id).await {
                Some(file) => file.clone(),
                None => {
                    tracing::warn!("File not found");
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"msg": "File not found"})),
                    )
                        .into_response();
                }
            };

            let path = { file.lock().await.path() };

            match fs::read(path).await {
                Ok(data) => {
                    tracing::info!("File({}) sent.", id);
                    (StatusCode::OK, data).into_response()
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"msg": "Error reading file"})),
                )
                    .into_response(),
            }
        }
    }
}
pub use filemeta::Filemeta;

pub mod crud {
    use axum::{Router, routing::post};

    use crate::fs::Filemeta;

    pub fn router() -> Router {
        Router::new().route(
            "/",
            post(Filemeta::handle_post)
                .get(Filemeta::handle_get)
                .delete(Filemeta::handle_delete),
        )
    }
}

#[cfg(feature = "media")]
mod playlist {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Deserialize, Serialize)]
    pub struct Playlist {
        id: Uuid,
        songs: Vec<Uuid>,
        created_at: u64,
        accessed_at: u64,
        updated_at: u64,
    }
}
#[cfg(feature = "media")]
pub use playlist::Playlist;

#[cfg(any(feature = "storage", feature = "media"))]
pub mod daemon {
    use axum::Router;
    #[cfg(feature = "storage")]
    use axum::routing::post;
    use tokio::sync::broadcast::Receiver;

    use crate::StateTrait;

    #[cfg(feature = "storage")]
    pub async fn run_storage(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running cloud storage daemon");
                }
            }
        }
    }

    #[cfg(feature = "media")]
    pub async fn run_media(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running media daemon");
                }
            }
        }
    }

    pub fn router() -> Router {
        let r = Router::new();

        #[cfg(feature = "storage")]
        let r = r.route(
            "/storage",
            post(async || "storage post").get(async || "storage get"),
        );

        #[cfg(feature = "media")]
        let r = r.route(
            "/media",
            post(async || "media post").get(async || "media get"),
        );

        r
    }
}
