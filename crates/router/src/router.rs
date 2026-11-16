use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

use crate::{Request, Response};

type BoxRouteFuture = Pin<Box<dyn Future<Output = Response> + Send + 'static>>;
type BoxRouteMatcher = Box<dyn Fn(&Request) -> bool + Send + Sync + 'static>;
type BoxRouteHandler = Box<dyn Fn(Request) -> BoxRouteFuture + Send + Sync + 'static>;

pub struct Route {
    matcher: BoxRouteMatcher,
    handler: BoxRouteHandler,
}

struct RouterInner {
    routes: Vec<Route>,
    fallback: Option<BoxRouteHandler>,
}

impl RouterInner {
    fn new() -> Self {
        Self {
            routes: Vec::new(),
            fallback: None,
        }
    }
}

#[derive(Clone)]
pub struct Router {
    inner: Arc<Mutex<RouterInner>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RouterInner::new())),
        }
    }

    /// Add a route to the router.
    ///
    /// The returned router is another handle to the same router state.
    pub fn route<M, H, F>(&self, matcher: M, handler: H) -> Self
    where
        M: Fn(&Request) -> bool + Send + Sync + 'static,
        H: Fn(Request) -> F + Send + Sync + 'static,
        F: Future<Output = Response> + Send + 'static,
    {
        let boxed_handler: BoxRouteHandler = Box::new(move |request| Box::pin(handler(request)));

        let mut inner = self.inner.lock().unwrap();

        inner.routes.push(Route {
            matcher: Box::new(matcher),
            handler: boxed_handler,
        });

        self.clone()
    }

    /// Set the fallback handler. This is only for requests which are not found.
    pub fn fallback<H, F>(&self, handler: H) -> Self
    where
        H: Fn(Request) -> F + Send + Sync + 'static,
        F: Future<Output = Response> + Send + 'static,
    {
        let boxed_handler: BoxRouteHandler = Box::new(move |request| Box::pin(handler(request)));

        let mut inner = self.inner.lock().unwrap();
        inner.fallback = Some(boxed_handler);

        self.clone()
    }

    /// Handle a request
    pub async fn handle(&self, request: Request) -> Response {
        let future: Option<BoxRouteFuture> = {
            let inner = self.inner.lock().unwrap();

            let matched_route = inner.routes.iter().find(|route| (route.matcher)(&request));

            match matched_route {
                Some(route) => Some((route.handler)(request)),
                None => inner.fallback.as_ref().map(|fallback| fallback(request)),
            }
        };

        match future {
            Some(future) => future.await,
            None => Response::default(),
        }
    }
}
