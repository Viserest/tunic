use tokio::net::TcpStream;

use crate::{Request, Response, Router, ServerError, Uri, Version};

pub async fn handle(router: Router, stream: TcpStream) -> Result<(), ServerError> {
    // Start connection
    let mut connection = h2::server::handshake(stream).await?;

    // Keep-alive connection
    while let Some(resolver) = connection.accept().await {
        // 1. decode stream into request
        let (request, respond) = resolver?;
        let req = read_request(request).await?;

        // 2. call router.handle(request) -> response
        let res = router.handle(req).await;

        // 3. encode response into stream
        send_response(respond, res)?;
    }

    Ok(())
}

async fn read_request(request: ::http::Request<h2::RecvStream>) -> Result<Request, ServerError> {
    let (parts, mut body) = request.into_parts();

    // Get request body
    let mut request_body = Vec::new();
    while let Some(data) = body.data().await {
        let data = data?;

        body.flow_control().release_capacity(data.len())?;

        request_body.extend_from_slice(&data);
    }

    Ok(Request {
        version: Version::Http(::http::Version::HTTP_2),
        method: parts.method,
        uri: Uri::Http(parts.uri),
        headers: parts.headers,
        extensions: ::http::Extensions::new(),
        body: request_body,
    })
}

fn send_response(
    mut respond: h2::server::SendResponse<bytes::Bytes>,
    response: Response,
) -> Result<(), ServerError> {
    // Convert crate response into http response
    let mut res_builder = ::http::Response::builder().status(response.status.to_u16());
    for (name, value) in &response.headers {
        res_builder = res_builder.header(name, value);
    }
    let res_head = res_builder
        .body(())
        .map_err(|error| ServerError::InvalidRequest(error.to_string()))?;

    let mut send_stream = respond.send_response(res_head, false)?;
    send_stream.send_data(bytes::Bytes::from(response.body), true)?;

    Ok(())
}
