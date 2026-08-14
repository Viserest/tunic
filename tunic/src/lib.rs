use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Router, body::Body, extract::Request, http::StatusCode, response::IntoResponse,
    serve::Serve,
};
use tokio::net::TcpListener;
use tower::util::ServiceExt;
use tower_http::compression::CompressionLayer;
use uuid::Uuid;

use crate::{db::Table, user::UserTrait};

pub use tunic_macro::make_crud;
pub use tunic_macro::make_user;

pub mod admin;
pub mod db;
pub mod fs;
pub mod job;
pub mod log;
pub mod user;
#[macro_use]
pub mod macros;

pub const IS_PROD: bool = match option_env!("ENV") {
    Some(val) => {
        let bytes = val.as_bytes();
        if bytes.len() == 4
            && bytes[0] == b'P'
            && bytes[1] == b'R'
            && bytes[2] == b'O'
            && bytes[3] == b'D'
        {
            true
        } else {
            false
        }
    }
    None => false,
};

pub fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b).to_string())
        .collect::<String>()
}

pub fn is_email_address(maybe_email: &String) -> Result<bool, regex::Error> {
    // Verify email is valid using this regex
    // /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
    let email_pattern = match regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
    {
        Ok(r) => r,
        Err(e) => return Err(e),
    };
    if email_pattern.is_match(maybe_email) {
        return Ok(true);
    }
    Ok(false)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[derive(Clone)]
pub struct State {
    pub admins: Table<Uuid, admin::Admin>,
    pub admin_sessions: Table<Uuid, user::Session>,
    pub files: Table<Uuid, fs::Filemeta>,
    pub jobs: Table<Uuid, job::Job>,
}

pub async fn axum_serve(
    host: &str,
    port: u16,
    admin_user: impl Into<String> + Copy,
    admin_pass: impl Into<String>,
    domains: Arc<HashMap<String, Router>>,
) -> Serve<TcpListener, Router, Router> {
    let state = State {
        admins: Table::new(),
        admin_sessions: Table::new(),
        files: Table::new(),
        jobs: Table::new(),
    };

    println!("[INFO] Initializing root user...");
    let admin = admin::Admin::new(admin_user, admin_pass);
    match state.admins.insert(*admin.get_id(), admin).await {
        Some(_) => {
            println!("[INFO] Root user already created as {}", admin_user.into());
        }
        None => {
            println!("[INFO] Root user created as {}", admin_user.into());
        }
    }

    println!("[INFO] Initializing axum...");
    let app = Router::new()
        .fallback(
            async |Extension(domains): Extension<Arc<HashMap<String, Router>>>,
                   req: Request<Body>| {
                // Get uri
                let host = req.headers().get("host").unwrap().to_str().unwrap();
                let hostname = host.split(':').next().unwrap_or(&host).to_string();

                // Get router
                let router = match domains.get(&hostname) {
                    Some(r) => r.clone(),
                    None => {
                        println!("[WARN] Unknown host: {}", hostname);
                        return (StatusCode::NOT_FOUND, format!("Unknown host: {}", hostname))
                            .into_response();
                    }
                };

                // Use router for request
                router
                    .layer(Extension(state))
                    .layer(CompressionLayer::new())
                    .as_service()
                    .oneshot(req)
                    .await
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            },
        )
        .layer(Extension(domains));

    let socket = format!("{}:{}", host, port);
    let listener = TcpListener::bind(socket.clone()).await.unwrap();
    println!("[INFO] Listening at {}", socket);
    axum::serve(listener, app)
}
