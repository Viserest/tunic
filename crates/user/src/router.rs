use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    response::{Html, Redirect},
    routing::{get, post},
};
use tokio::sync::Mutex;

use super::{Session, SessionPasswordResetPayload, State};

crate::router! {
    // Login pages (can be logged-out)
    Login {
        sec (style: [{width: "min", origin: "center", pos: "center", padding: 5}]) {
            header (style: [{origin: "center", pos: "center"}]) {"Login"},
            input (style: [input], name: "username") {"Username"},
            input (style: [input protected], name: "password") {"Password"},
            button (onclick: login_post()) {"Login"},
        }
    },
    // User pages (must be logged-in)
    Home {
        sec {
            link {to: Self::Home, text: "Home"},
            link {to: Self::Profile, text: "Profile"},
            link {to: Self::Settings, text: "Settings"},
        },
        sec {
            p {"GIT COMMIT HASH"},
            p {"CARGO PACKAGE VERSION"},
        },
    },
    Profile {},
    Settings {},
    // Managing page
    Console {},
}

async fn login_get() -> Html<String> {
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
}

async fn dashboard() -> Html<String> {
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

pub fn router_crud(state: State) -> Router {
    Router::new()
        .nest(
            "/user",
            state
                .users
                .router::<super::CreatePayload, super::UpdatePayload>(),
        )
        .nest(
            "/session",
            state
                .sessions
                .router::<super::session::CreatePayload, super::session::UpdatePayload>(),
        )
}

pub fn router_all(state: State) -> Router {
    Router::new()
        // Webpages
        .route("/", get(async || Redirect::to("/login")))
        .route("/login", post(login_post).get(login_get))
        .route("/dashboard", get(dashboard))
        .route(
            "/reset-password",
            post(
                async |Extension(state): Extension<State>,
                       session: Extension<Arc<Mutex<Session>>>,
                       payload: Json<SessionPasswordResetPayload>| {
                    Session::handle_reset_password(state.users, session, payload).await
                },
            ),
        )
}
