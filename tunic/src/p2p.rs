#[cfg(any(feature = "p2p_tor", feature = "p2p_i2p"))]
pub mod daemon {
    use axum::{Router, routing::get};
    use tokio::sync::broadcast::Receiver;

    use crate::StateTrait;

    #[cfg(feature = "p2p_tor")]
    pub async fn run_tor(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running tor daemon");
                }
            }
        }
    }

    #[cfg(feature = "p2p_i2p")]
    pub async fn run_i2p(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        use std::time::Duration;

        use tokio::time::sleep;

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = sleep(Duration::from_millis(200)) => {
                    tracing::trace!("running i2p daemon");
                }
            }
        }
    }

    pub fn router() -> Router {
        Router::new().route("/p2p_config", get(async || "p2p config"))
    }
}
