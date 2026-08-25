use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};

pub fn password_hash(password: impl Into<String>) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.into().as_bytes(), &salt)
        .expect("[ERROR] Hashing password")
        .to_string()
}

mod user {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };
    use axum::{
        Extension, Json, Router,
        response::{Html, Redirect},
        routing::{get, post},
    };
    use axum_extra::extract::CookieJar;
    use serde::{Deserialize, Serialize};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[cfg(feature = "media")]
    use crate::fs::Playlist;
    use crate::message::Message;
    use crate::{
        Session, State,
        user::{SessionLoginPayload, SessionPasswordResetPayload},
    };

    pub trait UserTrait {
        // These must be set here because id, username, and password can have any field name
        fn get_id(&self) -> &Uuid;
        fn get_username(&self) -> &String;
        fn set_username(&mut self, username: impl Into<String>);
        fn get_password(&self) -> &String;
        fn set_password(&mut self, password: impl Into<String>);
        // Normal user functions
        fn verify(&self, password: impl Into<String>) -> bool {
            let hash = PasswordHash::new(self.get_password()).unwrap();
            Argon2::default()
                .verify_password(password.into().as_bytes(), &hash)
                .is_ok()
        }
        fn reset_password(
            &mut self,
            old_password: impl Into<String>,
            new_password: impl Into<String>,
        ) -> bool {
            // Check if old password is valid
            if self.verify(old_password) {
                self.set_password(new_password);
                return true;
            }
            false
        }
    }

    #[derive(Serialize, Deserialize)]
    #[crate::make_user(namespace = "65dfa7c0-66cd-49ab-b62e-96f409efbdfb")]
    #[crate::make_crud(state = State, table = state.users)]
    pub struct User {
        #[skip(post)]
        #[id]
        id: Uuid,
        #[username]
        username: String,
        #[password]
        password: String,

        #[skip(post)]
        scope: Vec<Scope>,
        #[cfg(feature = "media")]
        playlist: Vec<Playlist>,
        #[cfg(any(
            feature = "msg",
            feature = "msg_base",
            feature = "msg_xmpp",
            feature = "msg_signal"
        ))]
        messages: BTreeMap<Uuid, Message>,

        #[skip(post, put)]
        created_at: u64,
        #[skip(post, put)]
        deleted_at: u64,
    }

    /// This router is nested or merged to the location used for an admin dashboard
    pub fn admin_router(state: State) -> Router {
        let public = Router::new()
            .route("/", get(async || Redirect::to("/admin/login")))
            .route(
                "/login",
                post(
                    async |Extension(state): Extension<State>,
                           jar: CookieJar,
                           payload: Json<SessionLoginPayload>| {
                        Session::handle_login(
                            &Uuid::parse_str("65dfa7c0-66cd-49ab-b62e-96f409efbdfb").unwrap(),
                            state.users,
                            state.user_sessions,
                            jar,
                            payload,
                        )
                        .await
                    },
                )
                .get(async || {
                    Html(
                        r#"<!doctype html>
<html>
    <head>
        <meta charset="utf-8" />
        <title>Admin Login</title>
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
        <style>
            form {min-width: 400px; width: 30%; margin: 0 auto;}
            form * {width: 100%; box-sizing:border-box;}
            form h1 {text-align: center;}
        </style>
    </head>
    <body>
        <div id="entry"></div>
        <script src="/static/grecha.js"></script>
        <script>const r = router({
            "/404": () => form(
            h2("Login"),
            input("text").att$("name", "username").att$("placeholder", "Username"),
            input("password").att$("name", "password").att$("placeholder", "Password"),
            input("submit").att$("value", "Login"),
            ).att$("action", "/admin/login").att$("method", "POST"),
            }); entry.appendChild(r);
        </script>
    </body>
</html>"#
                            .to_string(),
                    )
                }),
            );

        let private = Router::new()
            .route(
                "/dashboard",
                get(async || {
                    Html(format!(
                        r##"<!doctype html>
<html>
    <head>
        <meta charset="utf-8" />
        <title>Admin Dashboard</title>
        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    </head>
    <body>
        <div>
            <a href="">Home</a>
            <div>
                <a href="#admin_admin">Admin</a>
                <a href="#admin_session">Session</a>
                <a href="#admin_file">File</a>
                <a href="#admin_event">Event</a>
            </div>
            <div>
                <a href="#coops_subscriber">Subscriber</a>
                <a href="#coops_link">Link</a>
                <a href="#coops_message">Message</a>
            </div>
            <div>
                <a href="#csigns_cart">Cart</a>
                <a href="#csigns_contact">Contact</a>
                <a href="#csigns_invoice">Invoice</a>
                <a href="#csigns_product">Product</a>
                <a href="#csigns_store">Store</a>
                <a href="#csigns_user">User</a>
            </div>
        </div>
        <div>
            <div>Version: {}</div>
            <div>Git Hash: {}</div>
        </div>
        <div id="entry"></div>
        <script src="/static/grecha.js"></script>
        <script src="/static/router.js"></script>
    </body>
</html>"##,
                        option_env!("CARGO_PKG_VERSION").unwrap_or("Development"),
                        option_env!("GIT_COMMIT_HASH").unwrap_or("Development")
                    ))
                }),
            )
            .route(
                "/session",
                get(Session::handle_get).delete(Session::handle_delete),
            )
            .route(
                "/admin",
                post(User::handle_post)
                    .get(User::handle_get)
                    .put(User::handle_put)
                    .delete(User::handle_delete),
            )
            .route(
                "/admin/reset-password",
                post(
                    async |Extension(state): Extension<State>,
                           session: Extension<Arc<Mutex<Session>>>,
                           payload: Json<SessionPasswordResetPayload>| {
                        Session::handle_reset_password(state.users, session, payload).await
                    },
                ),
            )
            .layer(Extension(state));
        #[cfg(feature = "storage")]
        let private = private.route("/storage", get(async || "Storage client"));

        #[cfg(feature = "media")]
        let private = private.route("/media", get(async || "Media client"));

        #[cfg(any(
            feature = "msg",
            feature = "msg_base",
            feature = "msg_xmpp",
            feature = "msg_signal"
        ))]
        let private = private.route("/messages", get(async || "Messaging client"));

        #[cfg(any(feature = "p2p", feature = "p2p_tor", feature = "p2p_i2p"))]
        let private = private.route("/p2p", get(async || "P2P client"));

        Router::new().merge(public).merge(private)
    }
}
pub use user::{User, UserTrait, admin_router};

mod session {
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
    use uuid::Uuid;

    use crate::{State, Table, UserTrait, unix_now};

    /// Sessions for userspaces
    #[derive(Serialize, Deserialize)]
    #[crate::make_crud(methods = [get, delete], delete_type = real, state = State, table = state.user_sessions)]
    pub struct Session {
        id: Uuid,
        user_id: Uuid,
        created_at: u64,
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
        pub fn new(user_id: &Uuid, expires_at: u64) -> Self {
            Self {
                id: Uuid::new_v4(),
                user_id: *user_id,
                created_at: unix_now(),
                expires_at: unix_now() + expires_at,
            }
        }
        /// Handle login for all sessions
        /// All userspaces will use this middleware
        pub async fn handle_login<T: UserTrait>(
            users_namespace: &Uuid,
            users: Table<Uuid, T>,
            sessions: Table<Uuid, Session>,
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
            let session = Session::new(user.get_id(), 6 * 60 * 60);
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
            sessions: Table<Uuid, Session>,
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
                    if guard.created_at <= unix_now() && guard.expires_at >= unix_now() {
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

        pub async fn handle_reset_password<T: UserTrait>(
            users: Table<Uuid, T>,
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
}
pub use session::{Session, SessionLoginPayload, SessionPasswordResetPayload};
