use std::str::FromStr;

use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{Request, Response, Router, ServerError, Uri, Version};

const MAX_REQUEST_LINE: usize = 1024;

pub async fn handle(router: Router, stream: TcpStream) -> Result<(), ServerError> {
    // 1. decode stream into request
    let (read_half, writer) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let req = read_request(&mut reader).await?;

    // 2. call router.handle(request) -> response
    let res = router.handle(req).await;

    // 3. encode response into stream
    send_response(writer, res).await?;

    Ok(())
}

async fn read_request<R>(reader: &mut R) -> Result<Request, ServerError>
where
    R: AsyncBufRead + Unpin,
{
    // Get gemini url
    let url = read_line(reader).await?;

    Ok(Request {
        version: Version::Gemini,
        method: ::http::Method::GET,
        uri: Uri::Gemini(url),
        headers: ::http::HeaderMap::new(),
        extensions: ::http::Extensions::new(),
        body: Vec::new(),
    })
}

async fn read_line<R>(reader: &mut R) -> Result<::http::Uri, ServerError>
where
    R: AsyncBufRead + Unpin,
{
    // Get first line
    let mut first_line = String::new();
    reader.read_line(&mut first_line).await?;

    // Uri too long error
    if first_line.len() > MAX_REQUEST_LINE {
        return Err(ServerError::InvalidUri(
            "Request uri is too long".to_string(),
        ));
    }

    // Remove CRLF
    let first_line = first_line.trim_end_matches(['\r', '\n']);

    match ::http::Uri::from_str(first_line) {
        Ok(url) if url.scheme_str() == Some("gemini") => Ok(url),
        _ => {
            return Err(ServerError::InvalidUri("Invalid Gemini URL".to_string()));
        }
    }
}

async fn send_response<W>(mut writer: W, response: Response) -> Result<(), ServerError>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(format!("{} {}\r\n", response.status, response.meta).as_bytes())
        .await?;
    writer.write_all(&response.body).await?;

    // Gemini clients use EOF to determine the end of the response.
    writer.shutdown().await?;

    Ok(())
}
