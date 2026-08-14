use std::{cell::RefCell, default::Default, slice};

use axum::{extract::Request, middleware::Next, response::Response};

use crate::unix_now;

#[macro_use]
mod macros;

#[derive(Debug, Clone)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

// Nested node structure
#[derive(Debug, Clone)]
pub struct Node {
    id: usize,
    level: Level,
    message: String,
    children: Vec<Node>,
}

impl Node {
    fn print(&self, n: usize) {
        println!(
            "{}{} {:?}: {}",
            " ".repeat(n),
            unix_now(),
            self.level,
            self.message
        );
        for node in self.children.iter() {
            node.print(n + 4);
        }
    }
}

// Registry to manage the flat list of root trees
// and track the index/path to the newest node if needed.
pub struct LogTree {
    root: Node,
    // We track the ID of the newest node rather than a raw pointer
    newest_node_id: Option<usize>,
    next_id: usize,
}

impl LogTree {
    fn find_prev_node_mut<'a>(nodes: &'a mut [Node], id: usize) -> Option<&'a mut Node> {
        for node in nodes.iter_mut() {
            // 1. Check if any of THIS node's immediate children is the target.
            if node.children.iter().any(|child| child.id == id) {
                return Some(node);
            }

            // 2. If it's not an immediate child, recurse into the children.
            if let Some(parent) = Self::find_prev_node_mut(&mut node.children, id) {
                return Some(parent);
            }
        }
        None
    }

    /// Helper to find a mutable reference to a node by its ID using DFS
    fn find_node_mut<'a>(nodes: &'a mut [Node], id: usize) -> Option<&'a mut Node> {
        for node in nodes.iter_mut() {
            if node.id == id {
                return Some(node);
            }
            if let Some(found) = Self::find_node_mut(&mut node.children, id) {
                return Some(found);
            }
        }
        None
    }

    /// Adds a child to the node matching `parent_id`
    fn add_child(
        &mut self,
        parent_id: usize,
        level: Level,
        message: impl Into<String>,
    ) -> Option<usize> {
        let child_id = self.next_id;

        // Find the parent safely
        if let Some(parent) = Self::find_node_mut(slice::from_mut(&mut self.root), parent_id) {
            self.next_id += 1;
            parent.children.push(Node {
                id: child_id,
                level,
                message: message.into(),
                children: Vec::new(),
            });

            // Update the tracker to point to this new child
            self.newest_node_id = Some(child_id);
            Some(child_id)
        } else {
            None
        }
    }

    fn add_sibling(
        &mut self,
        parent_id: usize,
        level: Level,
        message: impl Into<String>,
    ) -> Option<usize> {
        let child_id = self.next_id;

        // If root node then just add as child
        if self.root.id == parent_id {
            self.next_id += 1;
            self.root.children.push(Node {
                id: child_id,
                level,
                message: message.into(),
                children: Vec::new(),
            });
            self.newest_node_id = Some(child_id);
            return Some(child_id);
        }

        // Find the parent safely
        if let Some(parent) = Self::find_prev_node_mut(slice::from_mut(&mut self.root), parent_id) {
            self.next_id += 1;
            parent.children.push(Node {
                id: child_id,
                level,
                message: message.into(),
                children: Vec::new(),
            });

            // Update the tracker to point to this new child
            self.newest_node_id = Some(child_id);
            Some(child_id)
        } else {
            None
        }
    }

    fn clear(&mut self) {
        self.root.children.clear();
        self.next_id = 1;
        self.newest_node_id = Some(0); // Reset back to root ID instead of None
    }
}

impl Default for LogTree {
    fn default() -> Self {
        Self {
            root: Node {
                id: 0,
                level: Level::Trace,
                message: "Root".into(),
                children: Vec::new(),
            },
            newest_node_id: Option::Some(0),
            next_id: 1,
        }
    }
}

tokio::task_local! {
    pub static TASK_STORAGE: RefCell<LogTree>;
}

/// Initialize log tree
/// For the logs to print call log::exit()
pub async fn init<F>(future: F) -> F::Output
where
    F: Future,
{
    let initial_registry = RefCell::new(LogTree::default());
    // Scope the registry to the task and await the execution
    TASK_STORAGE.scope(initial_registry, future).await
}

/// Initialize log tree for an axum middleware
/// For the logs to print call log::exit()
pub async fn layer(req: Request, next: Next) -> Response {
    init(async move {
        let res = next.run(req).await;
        exit();
        res
    })
    .await
}

/// Add node to current point
pub fn event(level: Level, message: impl Into<String>) {
    TASK_STORAGE.with(|storage| {
        let mut registry = storage.borrow_mut();
        if let Some(latest_id) = registry.newest_node_id {
            registry.add_sibling(latest_id, level, message);
        }
    });
}

/// Add node to parent point
pub fn span(level: Level, message: impl Into<String>) {
    TASK_STORAGE.with(|storage| {
        let mut registry = storage.borrow_mut();
        if let Some(latest_id) = registry.newest_node_id {
            registry.add_child(latest_id, level, message);
        }
    });
}

/// Print and clear tree
pub fn exit() {
    TASK_STORAGE.with(|storage| {
        storage.borrow().root.print(0);
        storage.borrow_mut().clear();
    });
}
