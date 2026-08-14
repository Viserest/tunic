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

#[macro_export]
macro_rules! make_list {
    ($struct:tt, $state:tt, $list:tt) => {
        struct $struct {}
        impl $struct {
            async fn handle_post(
                ::axum::Extension(state): ::axum::Extension<$state>,
                ::axum::Json(payload): ::axum::Json<::serde_json::Value>,
            ) -> impl ::axum::response::IntoResponse {
                let new = match payload
                    .as_object()
                    .and_then(|f| f.get("new"))
                    .and_then(|f| f.as_str())
                    .map(|f| f.to_string())
                {
                    Some(a) => a,
                    None => {
                        println!("[WARN] Could not find field 'new' in payload");
                        return (
                            ::axum::http::StatusCode::BAD_REQUEST,
                            ::axum::Json(::serde_json::json!({"msg": "Missing field 'new'"})),
                        )
                        .into_response();
                    }
                };
                if state.$list.insert(new).await {
                    println!("[INFO] Item added to list");
                    (::axum::http::StatusCode::OK, ::axum::Json(::serde_json::json!({"msg": "Added item"}))).into_response()
                } else {
                    println!("[WARN] Item not added to list");
                    (
                        ::axum::http::StatusCode::BAD_REQUEST,
                        ::axum::Json(::serde_json::json!({"msg": "Error adding item"})),
                    )
                    .into_response()
                }
            }

            async fn handle_get(
                ::axum::Extension(state): ::axum::Extension<$state>,
                ::axum::extract::Query(params): ::axum::extract::Query<::std::collections::HashMap<String, String>>,
            ) -> impl IntoResponse {
                let limit = params
                    .get("limit")
                    .and_then(|f| f.parse::<usize>().ok())
                    .unwrap_or(20)
                    .clamp(1, 100);
                let offset = params
                    .get("offset")
                    .and_then(|f| f.parse::<usize>().ok())
                    .unwrap_or(0);

                let result: Vec<String> = state.$list.iter().await.skip(offset).take(limit).collect();
                (
                    ::axum::http::StatusCode::OK,
                    ::axum::Json(::serde_json::json!({
                        "msg": "Found items",
                        "length": result.len(),
                        "limit": limit,
                        "offset": offset,
                        "list": result,
                    })),
                )
                .into_response()
            }

            async fn handle_delete(
                ::axum::Extension(state): ::axum::Extension<$state>,
                ::axum::Json(payload): ::axum::Json<::serde_json::Value>,
            ) -> impl IntoResponse {
                let old = match payload
                    .as_object()
                    .and_then(|f| f.get("old"))
                    .and_then(|f| f.as_str())
                    .map(|f| f.to_string())
                {
                    Some(a) => a,
                    None => {
                        println!("[WARN] Could not find field 'old' in payload");
                        return (
                            ::axum::http::StatusCode::BAD_REQUEST,
                            ::axum::Json(::serde_json::json!({"msg": "Missing field 'old'"})),
                        )
                        .into_response();
                    }
                };
                if state.$list.remove(&old).await {
                    println!("[INFO] Item removed from list");
                    (::axum::http::StatusCode::OK, ::axum::Json(::serde_json::json!({"msg": "Removed item"}))).into_response()
                } else {
                    println!("[WARN] Item not removed from list");
                    (
                        ::axum::http::StatusCode::BAD_REQUEST,
                        ::axum::Json(::serde_json::json!({"msg": "Error removing item"})),
                    )
                    .into_response()
                }
            }
        }
    };
}
