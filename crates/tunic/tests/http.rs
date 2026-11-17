use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode, header::HOST},
    routing::{get, post},
};
use tower::ServiceExt;
use tunic::HostRouter;

fn make_app() -> HostRouter {
    let app_router = Router::new()
        .route("/", get(async || "Home page"))
        .route("/about", get(async || "About page"));
    let api_router = Router::new().route(
        "/",
        post(async || {
            (
                StatusCode::CREATED,
                Json("{\"message\":\"Created successfully\"}"),
            )
        }),
    );

    HostRouter::new()
        .route("www.example.com", app_router)
        .route("api.example.com", api_router)
}

#[tokio::test]
async fn check_home_page() {
    let response = make_app()
        .oneshot(
            Request::builder()
                .header(HOST, "www.example.com")
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn check_about_page() {
    let response = make_app()
        .oneshot(
            Request::builder()
                .header(HOST, "www.example.com")
                .uri("/about")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn check_api_handler() {
    let response = make_app()
        .oneshot(
            Request::builder()
                .header(HOST, "api.example.com")
                .uri("/")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}
