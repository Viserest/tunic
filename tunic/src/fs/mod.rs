use std::{
    path::{self, PathBuf},
    sync::Arc,
};

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::fs;
use uuid::Uuid;

use crate::State;

pub mod mime;

#[derive(Serialize, Deserialize)]
#[tunic_macro::make_crud(methods = [get, delete], state = State, table = state.files)]
pub struct Filemeta {
    id: Uuid,
    og_name: String,
    content_type: mime::MimeType,
    size: usize,
    hash: [u8; 32],
    created_at: u64,
    deleted_at: u64,
}

impl Filemeta {
    /// Returns the absolute path of the disk stored file
    pub fn path(&self) -> PathBuf {
        let mut result = PathBuf::new();
        if let Some(a) = option_env!("STATE_DIR") {
            result.push(a);
        }
        result.push("files");
        result.push(self.id.to_string());
        result.add_extension(mime::MimeType::to_ext(&self.content_type));
        path::absolute(result).unwrap()
    }
    async fn handle_post(
        Extension(state): Extension<Arc<State>>,
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
            (Some(a), Some(og), Some(t), Some(l)) => (a, og, mime::MimeType::from_mime_type(t), l),
            _ => {
                println!("[WARN] handle_file_post missing required headers.");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"msg": "Missing required headers"})),
                )
                    .into_response();
            }
        };
        let (mime_types, max_size) = match auth {
            // Coops create message (admin)
            "Bearer 1f461ce5-6c03-428b-9cbf-85985cf94845" => (mime::IMAGE_MIME_TYPES, 10_000_000),
            // Csigns contact page (public)
            "Bearer 317b0ad6-0b26-4165-8b57-824c2d5953bc" => (mime::IMAGE_MIME_TYPES, 4_000_000),
            // Csigns create product (admin)
            "Bearer 00f215ce-29be-4a26-a4f7-3f309c227688" => (mime::IMAGE_MIME_TYPES, 10_000_000),
            _ => {
                println!("[WARN] handle_file_post invalid authorization header.");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"msg": "Request authorization header is invalid"})),
                )
                    .into_response();
            }
        };

        // Actual body length is different than content length header
        if body.len() != content_length {
            println!(
                "[WARN] In file upload, body length is not equal to content length header. Continuing..."
            );
        }

        // Request is larger than max size
        if body.len() > max_size {
            println!("[WARN] handle_file_post request is larger than max size.");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"msg": "Request content size is too large"})),
            )
                .into_response();
        }

        // Request has a different mime type than allowed
        if !mime_types.contains(&content_type) {
            println!("[WARN] handle_file_post request contains invalid content type.");
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
                println!("[INFO] File already created. Returning previous file id...");
                return (
                    StatusCode::OK,
                    Json(json!({"msg": "Saved file", "file_id": file.id})),
                )
                    .into_response();
            }
        }

        // Create file meta
        let file_id = Uuid::now_v7();
        let size = data.len();
        let meta = Filemeta {
            id: file_id,
            og_name: og_filename.into(),
            content_type: content_type,
            size,
            hash,
            created_at: crate::unix_now(),
            deleted_at: 0,
        };

        // Create file
        let path = meta.path();
        println!("[INFO] Saving file to {}.", path.display());
        fs::create_dir(&path.parent().unwrap()).await.unwrap();
        if let Err(e) = fs::write(path, data).await {
            println!("[WARN] handle_file_post {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"msg": "Internal server error"})),
            )
                .into_response();
        }

        // Write filemeta to database
        {
            if state.files.insert(meta.id, meta).await.is_some() {
                println!("[WARN] Overwrote key.");
            }
        }

        println!("[INFO] Saved file.");
        (
            StatusCode::CREATED,
            Json(json!({"msg": "Saved file", "file_id": file_id})),
        )
            .into_response()
    }
}

pub async fn handle_file_send(
    Extension(state): Extension<Arc<State>>,
    Path(id): Path<Uuid>,
) -> Response {
    let file = match state.files.get(&id).await {
        Some(file) => file.clone(),
        None => {
            println!("[WARN] File not found");
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
            println!("[INFO] File({}) sent.", id);
            (StatusCode::OK, data).into_response()
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"msg": "Error reading file"})),
        )
            .into_response(),
    }
}

pub fn router() -> Router {
    Router::new().route(
        "/",
        post(Filemeta::handle_post)
            .get(Filemeta::handle_get)
            .delete(Filemeta::handle_delete),
    )
}
