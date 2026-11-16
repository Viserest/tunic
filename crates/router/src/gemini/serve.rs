use tokio::{
    net::TcpListener,
    select, spawn,
    sync::broadcast::{Receiver, error::RecvError},
    task::{JoinHandle, JoinSet},
};

use crate::Router;

/// Handle gemini protocol
pub async fn serve(mut shutdown_rx: Receiver<()>, router: Router) -> JoinHandle<()> {
    let listener = TcpListener::bind("127.0.0.1:8030").await.unwrap();
    spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            select! {
                // Await shutdown signal
                result = shutdown_rx.recv() => {
                    // Close no matter the result
                    match result {
                        Ok(()) | Err(RecvError::Closed) => break,
                        Err(RecvError::Lagged(count)) => {
                            // Print error if lagged
                            tracing::warn!(
                                count,
                                "shutdown receiver lagged"
                            );
                            break;
                        }
                    }
                }
                // Await accepting client
                result = listener.accept() => {
                    let (stream, _addr) = result.unwrap();
                    let router = router.clone();
                    connections.spawn(async move {
                        if let Err(error) = super::handle(router, stream).await {
                            println!("Gemini protocol error: {}", error);
                        }
                    });
                }
            }
        }

        // Stop accepting new connections, then wait for existing ones.
        while let Some(result) = connections.join_next().await {
            if let Err(error) = result {
                tracing::error!(%error, "connection task failed");
            }
        }
    })
}
