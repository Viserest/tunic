use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{Request, Response, Router, ServerError, Uri, Version};

const MAX_REQUEST_LINE: usize = 1024;

// 1. Calls handle(router, stream, initial)
//    2. Calls read_request(stream)   -> Request
//       3. Calls read_line(stream)   -> Method, Uri, Version
//       4. Calls read_header(stream) -> HeaderMap
//       5. Calls read_body(stream)   -> Vec<u8>
//    6. Calls router.handle(request)
//    7. Calls send_response(stream, response)
//

pub async fn handle(router: Router, stream: TcpStream) -> Result<(), ServerError> {
    // 1. decode stream into request
    let (read_half, writer) = stream.into_split();
    let reader = BufReader::new(read_half);

    // Keep-alive connection
    loop {
        let req = read_request(reader).await?;

        // 2. call router.handle(request) -> response
        let res = router.handle(req).await;

        // 3. encode response into stream
        send_response(writer, res).await?;

        // HTTP/0.9 and HTTP/1.0 are normally one-request connections.
        // HTTP/1.1 can persist, but this simple implementation closes after
        // one response unless the client explicitly requests keep-alive.
        break;
    }

    Ok(())
}

async fn read_request<R>(mut reader: R) -> Result<Request, ServerError>
where
    R: AsyncBufRead + Unpin,
{
    // Read request line
    let (method, uri, version) = read_line(&mut reader).await?;

    // Read headers
    let (headers, content_length) = {
        if version == ::http::Version::HTTP_09 {
            (::http::HeaderMap::new(), 0)
        } else {
            let headers = read_headers(&mut reader).await?;
            let content_length = headers
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            (headers, content_length)
        }
    };

    // Read body using content-length header
    let body = {
        if content_length == 0 {
            Vec::new()
        } else {
            read_body(&mut reader, content_length).await?
        }
    };

    Ok(Request {
        version: Version::Http(version),
        method,
        uri: Uri::Http(uri),
        headers,
        extensions: ::http::Extensions::new(),
        body,
    })
}

async fn read_line<R>(
    reader: &mut R,
) -> Result<(::http::Method, ::http::Uri, ::http::Version), ServerError>
where
    R: AsyncBufRead + Unpin,
{
    // Get request line as vec<u8>
    let mut request_line = Vec::new();
    let bytes_read = reader.read_until(b'\n', &mut request_line).await?;
    if bytes_read == 0 {
        return Err(ServerError::InvalidRequest("Empty request".to_string()));
    }

    // Max request line
    if request_line.len() > MAX_REQUEST_LINE {
        return Err(ServerError::InvalidUri(
            "Request line is too long".to_string(),
        ));
    }

    if !request_line.ends_with(b"\r\n") {
        return Err(ServerError::InvalidRequest("Malformed request".to_string()));
    }

    request_line.truncate(request_line.len() - 2);

    // Convert vec<u8> to str
    let first_line = std::str::from_utf8(&request_line)
        .map_err(|error| ServerError::InvalidRequest(error.to_string()))?;
    let request_parts: Vec<&str> = first_line.split_whitespace().collect();
    if request_parts.len() < 2 {
        return Err(ServerError::InvalidRequest(
            "request line must contain method and URI".to_string(),
        ));
    }

    // Read method
    let method = ::http::Method::try_from(request_parts[0])
        .map_err(|_| ServerError::InvalidMethod(request_parts[0].to_string()))?;

    // Read uri
    let uri = ::http::Uri::try_from(request_parts[1])
        .map_err(|_| ServerError::InvalidUri(request_parts[1].to_string()))?;

    // Read version
    let version = if request_parts.len() >= 3 {
        match request_parts[2] {
            "HTTP/1.0" => ::http::Version::HTTP_10,
            "HTTP/1.1" => ::http::Version::HTTP_11,
            other => {
                return Err(ServerError::InvalidRequest(format!(
                    "unsupported HTTP version: {other}"
                )));
            }
        }
    } else {
        ::http::Version::HTTP_09
    };

    Ok((method, uri, version))
}

async fn read_headers<R>(reader: &mut R) -> Result<::http::HeaderMap, ServerError>
where
    R: AsyncBufRead + Unpin,
{
    let mut headers = ::http::HeaderMap::new();
    let mut line = String::new();

    // Allow maximum of 64 headers
    for _ in 0..64 {
        // Read line
        reader.read_line(&mut line).await?;

        // If empty line then headers is finished
        if line == "\r\n" {
            break;
        }

        match line.split_once(": ") {
            Some((name, value)) => {
                // Parse header name
                let name = name.to_ascii_lowercase();
                let name = ::http::HeaderName::from_str(&name)
                    .map_err(|error| ServerError::InvalidRequest(error.to_string()))?;

                // Parse header value
                let value = value.to_ascii_lowercase();
                let value = ::http::HeaderValue::from_str(&value)
                    .map_err(|error| ServerError::InvalidRequest(error.to_string()))?;

                // Insert header
                headers.insert(name, value);
            }
            None => {}
        }

        // Clear line
        line.clear();
    }
    Ok(headers)
}

async fn read_body<R>(reader: &mut R, size: usize) -> Result<Vec<u8>, ServerError>
where
    R: AsyncBufRead + Unpin,
{
    let mut body = Vec::new();
    while body.len() < size {
        let mut temporary = [0u8; 8192];
        let read = reader.read(&mut temporary).await?;

        if read == 0 {
            return Err(ServerError::InvalidRequest(
                "Unexpected end of request body".to_string(),
            ));
        }

        body.extend_from_slice(&temporary[..read]);
    }
    Ok(body)
}

async fn send_response<W>(mut writer: W, response: Response) -> Result<(), ServerError>
where
    W: AsyncWrite + Unpin,
{
    let mut output = Vec::new();

    // Write response line
    output.extend_from_slice(format!("HTTP/1.1 {}\r\n", response.status).as_bytes());

    // Write headers
    for (name, value) in response.headers {
        let name = name.ok_or(ServerError::InvalidResponse("header name".to_string()))?;
        let value = value
            .to_str()
            .map_err(|error| ServerError::InvalidResponse(error.to_string()))?;
        output.extend_from_slice(format!("{}: {}\r\n", name, value).as_bytes());
    }

    // Write ending headers
    output.extend_from_slice(
        format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            response.body.len()
        )
        .as_bytes(),
    );

    // Write body
    output.extend_from_slice(&response.body);

    writer.write_all(&output).await?;
    writer.shutdown().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tests::router;

    #[tokio::test]
    async fn simple() {
        let app = router();
    }
}
