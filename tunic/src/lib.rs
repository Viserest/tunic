use std::time::{Duration, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

mod db;
mod fs;
mod message;
mod p2p;
pub mod user;
#[macro_use]
pub mod macros;

pub use tunic_macro::make_crud;
pub use tunic_macro::make_user;

pub use db::{List, StateTrait, Table, TableValueTrait};
pub use user::{Session, User, UserTrait};

pub const IS_PROD: bool = match option_env!("ENV") {
    Some(val) => {
        let bytes = val.as_bytes();
        if bytes.len() == 4
            && bytes[0] == b'P'
            && bytes[1] == b'R'
            && bytes[2] == b'O'
            && bytes[3] == b'D'
        {
            true
        } else {
            false
        }
    }
    None => false,
};

/// ```rust
/// let result = tunic::to_hex(&[72, 69, 76, 76, 79]);
/// assert_eq!(result, "48454c4c4f".to_string())
/// ```
pub fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b).to_string())
        .collect::<String>()
}

/// ```rust
/// let result = tunic::is_email_address(&"john@example.com".into());
/// assert_eq!(result, Ok(true))
/// ```
pub fn is_email_address(maybe_email: &String) -> Result<bool, regex::Error> {
    // Verify email is valid using this regex
    // /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/
    let email_pattern = regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")?;
    if email_pattern.is_match(maybe_email) {
        return Ok(true);
    }
    Ok(false)
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

#[derive(Clone, Default)]
pub struct State {
    pub users: Table<Uuid, user::User>,
    pub user_sessions: Table<Uuid, user::Session>,
    pub files: Table<Uuid, fs::Filemeta>,
}
impl StateTrait for State {}

mod job {
    use std::sync::Arc;

    use serde::{Deserialize, Serialize};
    use tokio::{
        sync::Mutex,
        time::{Duration, sleep},
    };
    use uuid::Uuid;

    use crate::unix_now;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, PartialOrd)]
    pub enum JobResult {
        NotStarted,
        Running,
        Continue,
        Done,
        Error(String),
    }

    #[derive(Serialize, Deserialize)]
    pub struct Job {
        pub id: Uuid,
        pub delay: u64,
        pub created_at: u64,
        pub last_run_at: u64,
        pub state: JobResult,
    }

    impl Job {
        pub async fn run_spawn<F, Fut>(job: Arc<Mutex<Job>>, callback: F)
        where
            F: FnOnce() -> Fut + Send + Copy + 'static,
            Fut: Future<Output = JobResult> + Send,
        {
            tokio::spawn(async move {
                loop {
                    let mut guard = job.lock().await;
                    guard.state = JobResult::Running;
                    guard.last_run_at = unix_now();

                    // Execute job payload run function
                    guard.state = callback().await;

                    match guard.state {
                        // Schedule rerun if Continue
                        JobResult::Continue => {
                            sleep(Duration::from_secs(guard.delay)).await;
                        }
                        // Stop loop if Done or Error
                        _ => break,
                    }
                }
            });
        }
    }
}
pub use job::{Job, JobResult};

mod router {
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

    type BoxFuture =
        Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>>;

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
}
pub use router::HostRouter;

mod serve {
    use std::net::IpAddr;

    use axum::Router;
    use tokio::{net::TcpListener, runtime::Runtime, signal, sync::broadcast};

    use crate::{State, StateTrait};

    struct NetworkRule {
        address: IpAddr,
        prefix: u8,
        ports: Vec<u8>,
    }

    pub enum Features {
        Http {
            app: Router,
            host: NetworkRule,
        },
        Database {
            host: NetworkRule,
        },
        Storage,
        Media,
        Message {
            base: NetworkRule,
            xmpp: NetworkRule,
            signal: NetworkRule,
        },
        P2PTor {
            host: NetworkRule,
        },
        P2PI2p {
            host: NetworkRule,
        },
    }

    ///
    /// Feature config file
    ///
    /// This file should be present while compiling or running ANY compiled executables.
    /// Command line arguments specify which feature is compiled in but not how they are configured.
    ///
    /// Example:
    /// ```toml
    /// [http]
    /// pub = 0.0.0.0:80/443 # Public http pages (like media) will be accessible here
    /// sec = 10.1.0.0/16:8000 # Private http pages (like storage, message-base, and message-xmpp) will be accessible here
    ///
    /// [database]
    /// all = 10.2.0.0/16:8001 # database is only accessible from ip addresses: 10.2.0.0 to 10.2.255.255 at port 8001
    ///
    /// [storage]
    /// http     = sec/storage
    /// database = all
    ///
    /// [media]
    /// http     = pub/media
    /// database = all
    ///
    /// [message] # Http sub-feature creates the message page, all others are where feature specific clients connect to
    /// http    = pub/messages
    /// xmpp    = 0.0.0.0:32000 # XMPP clients send to this port
    /// signal  = 0.0.0.0:32001 # Signal clients send to this port
    /// simplex = 0.0.0.0:32002 # Simplex clients send to this port
    ///
    /// [p2p] # Http sub-feature creates the config page, all others are where feature specific nodes connect through
    /// http = sec/p2p-config
    ///
    /// [p2p.tor] # Relay should be global, Proxy should be localhost or secured. Ports dont need to be default.
    /// relay   = 0.0.0.0:9001   # Relay traffic
    /// socks1  = 127.0.0.1:9150 # Tor browser SOCKS proxy
    /// socks2  = 127.0.0.1:9050 # System SOCKS proxy
    /// control = 127.0.0.1:9051 # Optional local control
    ///
    /// [p2p.i2p] # Router should be global, Proxy should be localhost or secured.
    /// router        = 0.0.0.0:12345  # Router traffic. Port doesnt need to be default
    /// proxy_console = 127.0.0.1:7657 # Router console
    /// proxy_http    = 127.0.0.1:4444 # HTTP
    /// proxy_https   = 127.0.0.1:4445 # HTTPS
    /// proxy_socks   = 127.0.0.1:7656 # SOCKS
    /// proxy_i2cp    = 127.0.0.1:7654 # I2CP interface
    /// ```
    #[derive(Deserialize, Serialize)]
    pub struct Config {
        application: Option<Features::Application>,
        database: Option<Features::Database>,
        storage: Option<Features::Storage>,
        media: Option<Features::Media>,
        message_base: Option<Features::MessageBase>,
        message_xmpp: Option<Features::MessageXMPP>,
        message_signal: Option<Features::MessageSignal>,
        p2p_tor: Option<Features::P2PTor>,
        p2p_i2p: Option<Features::P2PI2p>,
    }

    pub struct Serve {
        app: Option<Router>,
    }

    impl Serve {
        pub fn new() -> Self {
            Self { app: None }
        }
        pub fn app(mut self, app: Router) -> Self {
            self.app = Some(app);
            self
        }
        pub async fn run_async(&self, state: impl StateTrait) {
            // One shutdown broadcaster
            let (shutdown_tx, _) = broadcast::channel::<()>(1);

            // On ctrlc it sends shutdown once to all tasks
            let ctrl_c_shutdown_tx = shutdown_tx.clone();
            let ctrl_c_shutdown = async move {
                let _ = signal::ctrl_c().await;
                let _ = ctrl_c_shutdown_tx.send(());
            };

            // Initialize ctrlc signal
            tokio::spawn(ctrl_c_shutdown);

            let mut handles = Vec::new();

            #[cfg(feature = "application")]
            {
                let mut shutdown_rx = shutdown_tx.subscribe();
                let app = self.app.clone().unwrap();

                handles.push(tokio::spawn(async {
                    let listener = TcpListener::bind("127.0.0.1:8000").await.unwrap();
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = shutdown_rx.recv().await;
                        })
                        .await
                        .unwrap();
                }));
            }

            #[cfg(feature = "database")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::db::daemon::run_database(shutdown_rx, state).await;
                }));
            }

            /*
             * Media, mostly filesystem handling
             */
            #[cfg(feature = "storage")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::fs::daemon::run_storage(shutdown_rx, state).await;
                }));
            }
            #[cfg(feature = "media")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::fs::daemon::run_media(shutdown_rx, state).await;
                }));
            }

            /*
             * Communication protocols
             */
            #[cfg(feature = "msg_xmpp")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::message::daemon::run_xmpp(shutdown_rx, state).await;
                }));
            }
            #[cfg(feature = "msg_signal")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::message::daemon::run_signal(shutdown_rx, state).await;
                }));
            }

            /*
             * Peer-to-peer protocols
             */
            #[cfg(feature = "p2p_tor")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::p2p::daemon::run_tor(shutdown_rx, state).await;
                }));
            }
            #[cfg(feature = "p2p_i2p")]
            {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = state.clone();
                handles.push(tokio::spawn(async {
                    crate::p2p::daemon::run_i2p(shutdown_rx, state).await;
                }));
            }

            // Wait for all tasks to shutdown
            for h in handles {
                let _ = h.await;
            }
        }
        pub fn run(&self, state: impl StateTrait) {
            let rt = Runtime::new().unwrap();

            rt.block_on(self.run_async(state));
        }
    }
}
pub use serve::{Scope, Serve};

pub mod files {
    pub const JS_GRECHA: &str = r###"
let AUTH_TOKEN = undefined;

function show_toast(message) {
  if (typeof toast !== "undefined") toast.textContent = message;
}

async function form_submittion_file(file_input) {
  // Type is files but none were provided
  if (file_input.files.length === 0) return [];
  // Must have AUTH_TOKEN variable
  if (typeof AUTH_TOKEN === "undefined") {
    show_toast("Missing AUTH_TOKEN.");
    console.log("Missing AUTH_TOKEN.");
    return [];
  }
  // Upload all files
  const file_entries = [];
  for (const file of [...file_input.files]) {
    // Upload file
    let res = await fetch("https://sethicus.com/fs", {
      method: "POST",
      body: await file.bytes(),
      headers: {
        Authorization: "Bearer " + AUTH_TOKEN,
        "Content-Type": file.type,
        "Content-Length": file.size,
        "Og-Filename": file.name,
      },
    });
    // Gather response
    if (!res.ok) {
      show_toast("Error while uploading file.");
      console.log(await res.text());
    }
    // Add file id
    let data = await res.json();
    file_entries.push(data["file_id"]);
  }
  // Return as either 1 file id or many
  if (file_entries.length === 1) return file_entries[0];
  else return file_entries;
}

// Special form submittion
// All forms are handled the same and sent as json
async function form_submittion(event) {
  // Stop default process
  event.preventDefault();
  show_toast("Sending form...");
  // Get all entries
  let form_entries = {};
  for (const t of [...event.target.children]) {
    // Skip inputs without name
    if (t.name === "" || t.name === null) break;
    // Upload file then set entry to file ids returned
    if (t.type === "file" && t.files !== null) {
      // Set form entry to file entries
      form_entries[t.name] = await form_submittion_file(t);
    } else {
      // Set form entry to input value
      form_entries[t.name] = t.value;
    }
  }
  // Send as json
  let res = await fetch(event.target.action, {
    method: event.target.method,
    body: JSON.stringify(form_entries),
    headers: {
      "Content-Type": "application/json",
    },
  });
  // Handle response
  if (!res.ok) {
    show_toast("Request failed.");
  }
  // Convert to json
  let data = await res.json();
  // If response is to redirect
  if (data["location"]) window.location.href = data["location"];
  // Show result
  show_toast(data["msg"]);
  console.log(data);
}

// Create html element
function tag(name, ...children) {
  // Handle element vs string
  const result = document.createElement(name);
  for (const child of children) {
    if (typeof child === "string") {
      result.appendChild(document.createTextNode(child));
    } else if (typeof child === "number") {
      result.appendChild(document.createTextNode(child.toString()));
    } else {
      result.appendChild(child);
    }
  }
  // Set attribute
  result.att$ = function (name, value) {
    this.setAttribute(name, value);
    return this;
  };
  // Set onclick function callback
  result.onclick$ = function (callback) {
    this.onclick = callback;
    return this;
  };
  // Set onsubmit function callback
  // This should only be called on forms
  result.onsubmit$ = function (callback) {
    this.onsubmit = callback;
    return this;
  };
  return result;
}

// Add elements as functions
const MUNDANE_TAGS = [
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "br",
  "hr",
  "p",
  "b",
  "i",
  "div",
  "span",
  "small",
  "select",
  "option",
  "button",
  "table",
  "tr",
  "th",
  "td",
  "pre",
];

for (let tagName of MUNDANE_TAGS) {
  window[tagName] = (...children) => tag(tagName, ...children);
}

// Layout div elements
function row(...children) {
  return tag("div", ...children).att$("class", "row");
}
function half_col(...children) {
  return tag("div", ...children).att$("class", "one-half column");
}

// Special elements
function img(src) {
  return tag("img").att$("src", src);
}
function input(type) {
  return tag("input").att$("type", type);
}
function cb(text) {
  return tag("label", tag("input").att$("type", "checkbox"), text);
}
function a(href, ...children) {
  return tag("a", ...children).att$("href", href);
}
function form(...children) {
  return tag("form", ...children).onsubmit$(form_submittion);
}

// Router
function router(routes) {
  let result = div();
  async function syncHash() {
    // Get hash location
    let hashLocation = document.location.hash.split("#")[1];
    if (!hashLocation) {
      hashLocation = "";
    }
    // Prepend slash
    hashLocation = "/" + hashLocation;
    // Handle 404 not found
    if (!(hashLocation in routes)) {
      const route404 = "/404";
      console.assert(route404 in routes);
      hashLocation = route404;
    }
    // Update children to route page
    result.replaceChildren(await routes[hashLocation]());
    return result;
  }
  // Handle hash change
  syncHash();
  window.addEventListener("hashchange", syncHash);
  result.refresh = syncHash;
  return result;
}
"###;
}
