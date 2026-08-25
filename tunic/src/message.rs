mod message {
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize)]
    pub enum Status {
        Sent(u32),
        Delivered(u32),
        Read(u32),
    }

    #[derive(Deserialize, Serialize)]
    pub struct Message {
        text: String,
        status: Status,
    }
}
pub use message::Message;

#[cfg(any(feature = "msg_base", feature = "msg_xmpp", feature = "msg_signal"))]
pub mod daemon {
    use axum::{Router, routing::post};
    use tokio::sync::broadcast::Receiver;

    use crate::StateTrait;

    #[cfg(feature = "msg_base")]
    pub async fn run_base(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        loop {}
    }

    #[cfg(feature = "msg_xmpp")]
    pub async fn run_xmpp(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running xmpp daemon");
                }
            }
        }
    }

    #[cfg(feature = "msg_signal")]
    pub async fn run_signal(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running signal daemon");
                }
            }
        }
    }

    pub fn router() -> Router {
        Router::new().route(
            "/message",
            post(async || "message post").get(async || "message get"),
        )
    }
}
