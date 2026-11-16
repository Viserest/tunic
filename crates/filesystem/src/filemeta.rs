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
use tunic_macro::TableValueCrud;
use uuid::Uuid;

use super::{IMAGE_MIME_TYPES, MimeType, State};
use crate::db::table::{TableGetKey, TableValueDeleted, TableValueUpdate};

#[derive(Deserialize, Serialize, TableValueCrud)]
pub struct Filemeta {
    #[tunic(id)]
    id: Uuid,
    #[tunic(create)]
    og_name: String,
    #[tunic(create)]
    content_type: MimeType,
    #[tunic(update)]
    permission_index: Vec<Uuid>,
    size: usize,
    hash: [u8; 32],
    created_at: u64,
    #[tunic(delete)]
    deleted_at: u64,
}

impl Filemeta {
    pub fn new(og_name: String, content_type: MimeType) -> Self {
        Self {
            id: Uuid::new_v4(),
            og_name,
            content_type,
            permission_index: Vec::new(),
            size: 0,
            hash: [0u8; 32],
            created_at: crate::unix_now(),
            deleted_at: 0,
        }
    }
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
            permission_index: Vec::new(),
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

    /// Handle responding with a file
    pub async fn handle_send(Extension(state): Extension<State>, Path(id): Path<Uuid>) -> Response {
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
