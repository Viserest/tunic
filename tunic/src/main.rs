use std::{collections::HashMap, env};

use axum::{Router, routing::get};
use tunic::{Serve, User, UserTrait};

#[tokio::main]
async fn main() {
    let mut args = HashMap::new();
    for arg in env::args() {
        args.insert(arg)
    }

    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let state = tunic::State::default();
    let user = User::new("root", "password");
    state.users.insert(*user.get_id(), user).await;

    let app = Router::new()
        .route("/static/grecha.js", get(async || tunic::files::JS_GRECHA))
        .route("/", get(async || "Hello World"))
        .nest("/admin", tunic::user::admin_router(state.clone()));
    Serve::new().app(app).run_async(state).await;
}
