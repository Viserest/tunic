use tunic::{Serve, User, UserTrait};

#[tokio::main]
async fn main() {
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let state = tunic::State::default();
    let mut config = tunic::Config::default();
    config.http_add("pub", tunic::NetworkRule::new("127.0.0.1", 8000));
    let user = User::new("root", "password");
    state.users.insert(*user.get_id(), user).await;

    Serve::new(state, config).run_async().await;
}
