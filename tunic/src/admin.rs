use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    response::{Html, Response},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    State,
    user::{Session, SessionLoginPayload, UserResetPasswordPayload},
};

#[derive(Serialize, Deserialize)]
pub struct Permission {
    id: Uuid,
    created_at: u64,
    accessed_at: u64,
    deleted_at: u64,
}

#[derive(Serialize, Deserialize)]
#[crate::make_user(namespace = "65dfa7c0-66cd-49ab-b62e-96f409efbdfb")]
#[crate::make_crud(state = State, table = state.admins)]
pub struct Admin {
    #[skip(post)]
    #[id]
    id: Uuid,
    #[username]
    username: String,
    #[password]
    password: String,
    #[skip(post)]
    permissions: Vec<Permission>,
    #[skip(post, put)]
    created_at: u64,
    #[skip(post, put)]
    deleted_at: u64,
}

pub fn router() -> Router {
    Router::new().route("/login", post(login_handle_post).get(login_handle_get))
}

pub fn admin_router() -> Router {
    Router::new()
        .route("/dashboard", get(handle_dashboard_get))
        .route(
            "/session",
            get(Session::handle_get).delete(Session::handle_delete),
        )
        .route(
            "/admin",
            post(Admin::handle_post)
                .get(Admin::handle_get)
                .put(Admin::handle_put)
                .delete(Admin::handle_delete),
        )
        .route("/admin/reset-password", post(admin_handle_reset_password))
}

async fn login_handle_post(
    Extension(state): Extension<State>,
    jar: CookieJar,
    payload: Json<SessionLoginPayload>,
) -> Response {
    Session::handle_login(
        &Uuid::parse_str("65dfa7c0-66cd-49ab-b62e-96f409efbdfb").unwrap(),
        state.admins,
        state.admin_sessions,
        jar,
        payload,
    )
    .await
}

async fn login_handle_get() -> Html<String> {
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
}); entry.appendChild(r);</script>
</body>
</html>"#
            .to_string(),
    )
}

async fn handle_dashboard_get() -> Html<String> {
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
}

async fn admin_handle_reset_password(
    Extension(state): Extension<State>,
    session: Extension<Arc<Mutex<Session>>>,
    payload: Json<UserResetPasswordPayload>,
) -> Response {
    Session::handle_reset_password(state.admins, session, payload).await
}
