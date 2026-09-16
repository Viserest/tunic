#[macro_export]
macro_rules! static_files {
    ($name:ident, { $($url:expr => $file_path:expr),+ $(,)? }) => {
        async fn $name(
            ::axum::extract::Path(file): ::axum::extract::Path<String>
        ) -> ::axum::response::Response {
            $(
                if file == $url {
                    // From mimetype from file extension then convert to content type string
                    let mime = ::tunic::fs::mime::MimeType::from_ext($file_path).to_mime_type();

                    if ::tunic::IS_PROD {
                        // PROD MODE
                        // We use the helper macro here so include_bytes! sees a literal expression
                        let contents = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $file_path));
                        return (
                            ::axum::http::StatusCode::OK,
                            [("content-type", mime)],
                            contents.to_vec()
                        ).into_response();
                    } else {
                        // DEV MODE
                        // tokio can safely use the runtime path resolution
                        if let Ok(contents) = ::tokio::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/", $file_path)).await {
                            return (
                                ::axum::http::StatusCode::OK,
                                [("content-type", mime)],
                                contents
                            ).into_response();
                        }
                    }
                }
            )+

            return (::axum::http::StatusCode::NOT_FOUND, "Not Found").into_response();
        }
    };
}
