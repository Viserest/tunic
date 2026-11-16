use bytes::{Buf, Bytes};

use crate::{Request, Response, Router, ServerError, Uri, Version};

pub async fn handle(router: Router, connection: quinn::Connection) -> Result<(), ServerError> {
    let quic_connection = h3_quinn::Connection::new(connection);

    let mut http3_connection = h3::server::Connection::new(quic_connection)
        .await
        .map_err(|error| ServerError::Http3(error.to_string()))?;

    // Keep-alive connection
    while let Ok(resolver) = http3_connection.accept().await {
        // Accept connection
        let request = match resolver {
            Some(resolver) => resolver
                .resolve_request()
                .await
                .map_err(|error| ServerError::Http3(error.to_string()))?,
            None => return Err(ServerError::Http3("Couldn't accept connection".to_string())),
        };

        // 1. decode stream into request
        let (request, mut stream) = request;
        let req = read_request(&mut stream, request).await?;

        // 2. call router.handle(request) -> response
        let res = router.handle(req).await;

        // 3. encode response into stream
        send_response(&mut stream, res).await?;
    }

    Ok(())
}

async fn read_request(
    stream: &mut h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    request: ::http::Request<()>,
) -> Result<Request, ServerError> {
    let (parts, _) = request.into_parts();
    let mut body = Vec::new();

    loop {
        match stream.recv_data().await {
            Ok(Some(data)) => body.extend_from_slice(data.chunk()),
            Ok(None) => break,
            Err(error) => return Err(ServerError::Http3(error.to_string())),
        }
    }

    Ok(Request {
        version: Version::Http(::http::Version::HTTP_3),
        method: parts.method,
        uri: Uri::Http(parts.uri),
        headers: parts.headers,
        extensions: ::http::Extensions::new(),
        body,
    })
}

async fn send_response(
    stream: &mut h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    response: Response,
) -> Result<(), ServerError> {
    let mut response_builder = ::http::Response::builder().status(response.status.to_u16());
    for (name, value) in &response.headers {
        response_builder = response_builder.header(name, value);
    }
    let response_head = response_builder
        .body(())
        .map_err(|error| ServerError::Http3(error.to_string()))?;

    stream
        .send_response(response_head)
        .await
        .map_err(|error| ServerError::Http3(error.to_string()))?;

    stream
        .send_data(Bytes::from(response.body))
        .await
        .map_err(|error| ServerError::Http3(error.to_string()))?;

    stream
        .finish()
        .await
        .map_err(|error| ServerError::Http3(error.to_string()))?;

    Ok(())
}
