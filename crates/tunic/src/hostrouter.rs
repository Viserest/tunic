use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header::HOST},
};
use tower::{Service, ServiceExt};

#[derive(Clone, Default)]
pub struct HostRouter {
    routes: HashMap<String, Router>,
}

impl HostRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route(mut self, host: impl Into<String>, router: Router) -> Self {
        self.routes.insert(normalize_host(&host.into()), router);
        self
    }

    fn lookup(&self, host: &str) -> Option<Router> {
        self.routes.get(host).cloned()
    }
}

type BoxFuture = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>>;

impl Service<Request<Body>> for HostRouter {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = BoxFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let host = req
            .headers()
            .get(HOST)
            .and_then(|h| h.to_str().ok())
            .map(normalize_host);

        let Some(host) = host else {
            return Box::pin(async { Ok(not_found()) });
        };

        let Some(router) = self.routes.get(&host).cloned() else {
            return Box::pin(async { Ok(not_found()) });
        };

        Box::pin(async move { Ok(router.oneshot(req).await.unwrap()) })
    }
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();

    // example.com:8080
    if let Some((hostname, _port)) = host.rsplit_once(':') {
        if !hostname.contains(':') {
            return hostname.to_owned();
        }
    }

    host
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("Unknown host"))
        .unwrap()
}
