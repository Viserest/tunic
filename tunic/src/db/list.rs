use axum::Json;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, hash::Hash, sync::Arc, vec::IntoIter};
use tokio::sync::{Mutex, MutexGuard};

use super::ListPayload;

pub struct AddPayload<T> {
    value: T,
}
pub struct DeletePayload<T> {
    value: T,
}

pub trait ListCrud<T> {
    async fn handle_list(self, payload: Json<ListPayload>) -> Value;
    async fn handle_add(self, payload: Json<AddPayload<T>>) -> Value;
    async fn handle_delete(self, payload: Json<DeletePayload<T>>) -> Value;
}

/// Async database lists using [`std::collections::HashSet`]
///
/// Uses [`std::sync::Arc`] for cheap cloning
pub struct List<T>(Arc<Mutex<HashSet<T>>>);

impl<T> List<T> {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashSet::new())))
    }
    /// Safely lock the table for reading and writing
    pub async fn lock(&self) -> MutexGuard<'_, HashSet<T>> {
        self.0.lock().await
    }
}

impl<T> List<T>
where
    T: Hash + Eq + Clone,
{
    /// Wrapped insert function
    pub async fn insert(&self, value: T) -> bool {
        self.lock().await.insert(value)
    }
    /// Wrapped remove function
    pub async fn remove(&self, value: &T) -> bool {
        self.lock().await.remove(value)
    }
    /// Wrapped iter function
    pub async fn iter(&self) -> IntoIter<T> {
        let guard = self.lock().await;
        let values: Vec<_> = guard.iter().map(|v| v.clone()).collect();
        values.into_iter()
    }
}

impl<T> ListCrud<T> for List<T>
where
    T: Hash + Eq + Copy + Serialize,
{
    async fn handle_list(self, payload: Json<ListPayload>) -> Value {
        let limit = payload.limit.unwrap_or(20).clamp(1, 100);
        let offset = payload.offset.unwrap_or(0);
        let data = self
            .iter()
            .await
            .skip(offset)
            .take(limit)
            .collect::<Vec<T>>();
        if data.len() == 0 {
            json!({
                "msg": "Error finding list",
            })
        } else {
            json!({
                "msg": "Successfully found list",
                "content": {
                    "length": data.len(),
                    "limit": limit,
                    "offset": offset,
                    "data": data,
                },
            })
        }
    }
    async fn handle_add(self, payload: Json<AddPayload<T>>) -> Value {
        if self.insert(payload.value).await {
            json!({
                "msg": "Successfully added",
            })
        } else {
            json!({
                "msg": "Could not add"
            })
        }
    }
    async fn handle_delete(self, payload: Json<DeletePayload<T>>) -> Value {
        if self.remove(&payload.value).await {
            json!({
                "msg": "Successfully removed",
            })
        } else {
            json!({
                "msg": "Could not remove"
            })
        }
    }
}

impl<T> Clone for List<T> {
    /// Clone the inner arc
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self::new()
    }
}
