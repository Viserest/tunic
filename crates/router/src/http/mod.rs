pub mod serve {
    use std::{net::SocketAddr, str::FromStr};

    use quinn::{Endpoint, ServerConfig};
    use tokio::{
        net::TcpListener,
        select, spawn,
        sync::broadcast::{Receiver, error::RecvError},
        task::{JoinHandle, JoinSet},
    };

    use crate::{Router, ServerError};

    /// Handle http version 0.9/1.0/1.1/2 protocols
    pub async fn v1_v2(mut shutdown_rx: Receiver<()>, router: Router) -> JoinHandle<()> {
        let listener = TcpListener::bind("127.0.0.1:8010")
            .await
            .expect("failed to bind HTTP listener");

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
                        // Get connection
                        let (stream, _addr) = match result {
                            Ok(connection) => connection,
                            Err(error) => {
                                tracing::error!(%error, "HTTP accept failed");
                                continue;
                            }
                        };
                        let router = router.clone();

                        // Spawn task for connection
                        connections.spawn(async move {
                            if let Err(error) =
                                super::handle::v1_v2(router, stream).await
                            {
                                tracing::error!(%error, "HTTP error");
                            }
                        });
                    }
                }
            }

            tracing::info!("HTTP listener stopped accepting connections");

            // Stop accepting new connections, then wait for existing ones.
            while let Some(result) = connections.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "connection task failed");
                }
            }

            tracing::info!("All HTTP connections finished");
        })
    }

    /// Handle http version 3 protocol
    pub async fn v3(
        mut shutdown_rx: Receiver<()>,
        router: Router,
        server_config: ServerConfig,
    ) -> JoinHandle<()> {
        // Create quinn endpoint
        let addr = SocketAddr::from_str("127.0.0.1:8011").unwrap();
        let endpoint = Endpoint::server(server_config, addr)
            .map_err(|error| ServerError::Http3(error.to_string()))
            .unwrap();

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
                    result = endpoint.accept() => {
                        let incoming = result.unwrap();
                        let connection = incoming
                            .await
                            .map_err(|error| ServerError::Http3(error.to_string()))
                            .unwrap();
                        let router = router.clone();
                        connections.spawn(async move {
                            if let Err(error) = super::handle::v3(router, connection).await {
                                println!("Http error: {}", error);
                            }
                        });
                    }
                }
            }

            tracing::info!("HTTP listener stopped accepting connections");

            // Stop accepting new connections, then wait for existing ones.
            while let Some(result) = connections.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "connection task failed");
                }
            }

            tracing::info!("All HTTP connections finished");
        })
    }
}

pub mod handle {
    use tokio::net::TcpStream;

    use crate::{Router, ServerError};

    const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

    pub async fn v1_v2(router: Router, stream: TcpStream) -> Result<(), ServerError> {
        // Read the preface size
        let mut initial = vec![0u8; HTTP2_PREFACE.len()];
        let bytes_read = stream.peek(&mut initial).await?;
        initial.truncate(bytes_read);

        // If what was read is the http2 preface
        if initial == HTTP2_PREFACE {
            super::http2::handle(router, stream).await
        }
        // Otherwise try http 0.9/1
        else {
            super::http1::handle(router, stream).await
        }
    }
    pub async fn v3(router: Router, connection: quinn::Connection) -> Result<(), ServerError> {
        super::http3::handle(router, connection).await
    }
}

// Protocol versions
mod http1;
mod http2;
mod http3;

// Protocol impl
mod extensions;
mod headers;
mod status;
mod version;

pub use extensions::*;
pub use headers::*;
pub use status::*;
pub use version::*;
