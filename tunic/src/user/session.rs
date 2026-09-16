use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{CookieJar, cookie::Cookie};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tunic_macro::TableValueCrud;
use uuid::Uuid;

use crate::db::table::{TableGetKey, TableValueDeleted, TableValueUpdate};

/// Sessions for userspaces
#[derive(Serialize, Deserialize, TableValueCrud)]
pub struct Session {
    #[tunic(id)]
    id: Uuid,
    #[tunic(create)]
    user_id: Uuid,
    created_at: u64,
    accessd_at: u64,
    #[tunic(create)]
    expires_at: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SessionLoginPayload {
    username: String,
    password: String,
}

#[derive(Serialize, Deserialize)]
pub struct SessionPasswordResetPayload {
    old_password: String,
    new_password: String,
}

impl Session {
    pub fn new(user_id: Uuid, expires_at: u64) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            created_at: crate::unix_now(),
            accessd_at: 0,
            expires_at: crate::unix_now() + expires_at,
        }
    }

    /// Handle login for all sessions
    /// All userspaces will use this middleware
    pub async fn handle_login<T: super::UserTrait>(
        users_namespace: &Uuid,
        users: crate::db::Table<Uuid, T>,
        sessions: crate::db::Table<Uuid, Session>,
        jar: CookieJar,
        Json(payload): Json<SessionLoginPayload>,
    ) -> Response {
        let id = Uuid::new_v5(users_namespace, payload.username.as_bytes());

        // Verify username and password match
        let user = match users.get(&id).await {
            Some(a) => a,
            None => {
                tracing::warn!("handle_login_post couldn't find user.");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"msg": "User not found"})),
                )
                    .into_response();
            }
        };
        let user = user.lock().await;

        // Check password for admin
        if !user.verify(payload.password) {
            tracing::warn!("handle_login_post user found but invalid password.");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"msg": "Invalid password"})),
            )
                .into_response();
        }

        // Create session
        let session = Session::new(*user.get_id(), 6 * 60 * 60);
        let session_id = session.id;
        let cookie = Cookie::new("session_id", session.id.to_string());

        if sessions.insert(session.id, session).await.is_some() {
            tracing::warn!("Overwrote id.");
        }

        tracing::info!(
            "handle_login_post Successfully logged in: {} with session: {}.",
            user.get_username(),
            session_id,
        );
        (
            StatusCode::OK,
            jar.add(cookie),
            Json(json!({"msg": "Successfully logged in", "location": "/admin/dashboard"})),
        )
            .into_response()
    }

    /// Middleware for sessions
    /// All userspaces will use this middleware and the user_id will work for a specific userspace
    pub async fn handle_middleware(
        sessions: crate::db::Table<Uuid, Session>,
        redirect: &'static str,
        mut req: Request,
        next: Next,
    ) -> Result<Response, Redirect> {
        // Add session to extensions if cookie is found and valid
        let jar = CookieJar::from_headers(req.headers());
        if let Some(parsed) = jar
            .get("session_id")
            .and_then(|f| Uuid::parse_str(f.value()).ok())
        {
            if let Some(session) = sessions.get(&parsed).await {
                let guard = session.lock().await;
                if guard.created_at <= crate::unix_now() && guard.expires_at >= crate::unix_now() {
                    tracing::info!("session_middleware login with session: {}.", parsed);

                    drop(guard);
                    req.extensions_mut().insert(session);
                    return Ok(next.run(req).await);
                }
            }
        }

        tracing::warn!("session_middleware login with invalid credentials.");
        Err(Redirect::to(redirect))
    }

    pub async fn handle_reset_password<T: super::UserTrait>(
        users: crate::db::Table<Uuid, T>,
        Extension(session): Extension<Arc<Mutex<Session>>>,
        Json(payload): Json<SessionPasswordResetPayload>,
    ) -> Response {
        let session = session.lock().await;
        let user = match users.get_mut(&session.user_id).await {
            Some(a) => a,
            None => {
                tracing::warn!("handle_user_reset_password couldn't find user.");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"msg": "Internal server error"})),
                )
                    .into_response();
            }
        };
        let mut user = user.lock().await;

        if !user.reset_password(payload.old_password, payload.new_password) {
            tracing::warn!("handle_admin_reset_password admin found but invalid password.");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"msg": "Couldn't change password. Try again later."})),
            )
                .into_response();
        }

        tracing::info!(
            "handle_admin_reset_password successfully changed password for admin: {}.",
            user.get_username()
        );
        (
            StatusCode::OK,
            Json(json!({"msg": "Successfully changed your password."})),
        )
            .into_response()
    }
}
