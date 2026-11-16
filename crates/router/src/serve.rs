use std::sync::Arc;

use quinn::{ServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{runtime::Runtime, signal::ctrl_c, sync::broadcast::channel};

use crate::Router;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Invalid HTTP request: {0}")]
    InvalidRequest(String),

    #[error("Invalid HTTP method: {0}")]
    InvalidMethod(String),

    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("Invalid HTTP response: {0}")]
    InvalidResponse(String),

    #[error("HTTP/1 error: {0}")]
    Http1(String),

    #[error("HTTP/2 error: {0}")]
    Http2(#[from] h2::Error),

    #[error("HTTP/3 error: {0}")]
    Http3(String),

    #[error("Gopher error: {0}")]
    Gopher(String),

    #[error("Gemini error: {0}")]
    Gemini(String),
}

/// Serve the router without handling a tokio runtime.
///
/// If you're already using a runtime then call serve_async instead.
pub fn serve(router: Router) {
    let rt = Runtime::new().unwrap();
    rt.block_on(serve_async(router));
}

/// Serve the router asynchronisly
pub async fn serve_async(router: Router) {
    // Generate a certificate for local development.
    //
    // The certificate must contain the hostname used by the client,
    // such as "localhost".
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let certificate_der = CertificateDer::from(certificate.cert.der().to_vec());
    let private_key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        certificate.signing_key.serialize_der(),
    ));

    let mut server_config =
        ServerConfig::with_single_cert(vec![certificate_der.clone()], private_key).unwrap();

    // Optional QUIC transport configuration
    let mut transport = TransportConfig::default();
    transport.max_concurrent_bidi_streams(100u32.into());
    transport.max_concurrent_uni_streams(100u32.into());
    server_config.transport = Arc::new(transport);

    // One shutdown broadcaster
    let (shutdown_tx, _) = channel::<()>(1);

    // On ctrlc it sends shutdown once to all tasks
    let ctrl_c_shutdown_tx = shutdown_tx.clone();
    let ctrl_c_shutdown = async move {
        let _ = ctrl_c().await;
        let _ = ctrl_c_shutdown_tx.send(());
    };

    // Initialize ctrlc signal
    tokio::spawn(ctrl_c_shutdown);

    // Collect all spawned tasks for checking if all get shutdown
    let listeners = vec![
        crate::http::serve::v1_v2(shutdown_tx.subscribe(), router.clone()).await,
        crate::http::serve::v3(shutdown_tx.subscribe(), router.clone(), server_config).await,
        crate::gopher::serve(shutdown_tx.subscribe(), router.clone()).await,
        crate::gemini::serve(shutdown_tx.subscribe(), router.clone()).await,
    ];

    // Handle shutting down all tasks
    for h in listeners {
        let _ = h.await;
    }
}
