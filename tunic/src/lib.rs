use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use tokio::runtime::Runtime;
use tokio::signal;
use tokio::sync::broadcast;

mod feature;
mod fs;
mod http;
mod message;
mod p2p;

mod db;
pub use db::{List, Table};
mod job;
pub use job::{Job, JobResult};
mod user;
pub use user::{Session, User, UserTrait};
mod hostrouter;
pub use hostrouter::HostRouter;

#[macro_use]
pub mod macros;

pub use tunic_macro::TableValueCrud;
pub use tunic_macro::make_user;

use crate::feature::FeatureTrait;

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
struct State {
    user: user::Feature,
    fs: fs::Feature,
    storage: fs::storage::Feature,
    media: fs::media::Feature,
    message: message::Feature,
    p2p: p2p::Feature,
}
#[derive(Default)]
struct Config {}
enum Scope {}

#[derive(Clone, Default)]
struct Feature {
    state: State,
    config: Arc<Config>,
}
impl FeatureTrait for Feature {
    type State = State;
    type Config = Config;
    type Scope = Scope;
    fn router_all(self) -> Router {
        Router::new()
            .nest("/user", self.state.user.router_all())
            .nest("/storage", self.state.storage.router_all())
            .nest("/media", self.state.media.router_all())
            .nest("/message", self.state.message.router_all())
            .nest("/p2p", self.state.p2p.router_all())
    }
    fn router_crud(self) -> Router {
        Router::new()
            .nest("/user", self.state.user.router_crud())
            .nest("/fs", self.state.fs.router_crud())
    }
    fn run(self, shutdown_tx: broadcast::Sender<()>) -> Vec<tokio::task::JoinHandle<()>> {
        let handles = Vec::new();
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
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
            for feature in inventory::iter::<Feature> {
                handles.extend(feature.run());
            }

            // If no features are enabled
            if handles.len() == 0 {
                println!("Nothing to run!");
            }

            // Wait for all tasks to shutdown
            for h in handles {
                let _ = h.await;
            }
        });
        handles
    }
}

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
