use std::sync::Arc;

use element::{Page, heading, table, text};
use router::{Request, Response, Router};
use utils::{Value, json_obj, unix_now};

use crate::Node;

/// Dashboard routes: `GET /p2p` (page for any protocol), `GET /p2p/peers` and `GET /p2p/self` (json).
pub fn dashboard(node: Arc<Node>) -> Router {
    let n1 = node.clone();
    let n2 = node.clone();
    let n3 = node;
    Router::new()
        .get("/p2p", move |req: &Request| {
            let me = n1.self_info();
            let now = unix_now();
            let rows: Vec<Vec<String>> = n1
                .peers()
                .into_iter()
                .map(|p| {
                    vec![
                        p.name,
                        p.id.to_string(),
                        p.addr.unwrap_or_else(|| "(passive)".into()),
                        p.roles.join(", "),
                        format!("{}s ago", now.saturating_sub(p.last_seen)),
                    ]
                })
                .collect();
            let page = Page::new("P2P mesh")
                .add(heading(1, "P2P mesh"))
                .add(text(format!(
                    "This node: {} ({}) listening on {} with roles [{}]",
                    me.name,
                    me.id,
                    me.addr.unwrap_or_else(|| "nothing (passive)".into()),
                    me.roles.join(", ")
                )))
                .add(heading(2, "Peers"))
                .add(table(["Name", "Id", "Address", "Roles", "Last seen"], rows));
            Response::page(req, &page)
        })
        .get("/p2p/peers", move |_| {
            let peers: Vec<Value> = n2
                .peers()
                .into_iter()
                .map(|p| json_obj! {"id" => p.id, "name" => p.name, "addr" => p.addr, "roles" => p.roles, "last_seen" => p.last_seen})
                .collect();
            Response::json(&json_obj! {"peers" => peers})
        })
        .get("/p2p/self", move |_| {
            let me = n3.self_info();
            Response::json(&json_obj! {"id" => me.id, "name" => me.name, "addr" => me.addr, "roles" => me.roles})
        })
}
