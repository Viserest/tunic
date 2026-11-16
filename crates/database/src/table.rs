use axum::{Extension, Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, hash::Hash, sync::Arc, vec::IntoIter};
use tokio::sync::{Mutex, MutexGuard};

use super::ListPayload;

/// Async database tables using [`std::collections::HashMap`]
///
/// Uses [`std::sync::Arc`] for cheap cloning
pub struct Table<K, V>(Arc<Mutex<HashMap<K, Arc<Mutex<V>>>>>);

// K and V have no requirements
impl<K, V> Table<K, V> {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
    /// Safely lock the table for reading and writing
    pub async fn lock(&self) -> MutexGuard<'_, HashMap<K, Arc<Mutex<V>>>> {
        self.0.lock().await
    }
    /// Wrapped len function
    pub async fn len(&self) -> usize {
        self.lock().await.len()
    }
    /// Wrapped values function
    pub async fn values(&self) -> IntoIter<Arc<Mutex<V>>> {
        let guard = self.lock().await;
        let items: Vec<_> = guard.iter().map(|(_, v)| v.clone()).collect();
        items.into_iter()
    }
}

// K must be Equatable and Hashable
impl<K, V> Table<K, V>
where
    K: Eq + Hash,
{
    /// Wrapped get function
    pub async fn get(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        let inner_guard = self.lock().await;
        Some(inner_guard.get(key)?.clone())
    }

    /// Wrapped get_mut function
    pub async fn get_mut(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        let mut inner_guard = self.lock().await;
        Some(inner_guard.get_mut(key)?.clone())
    }
    /// Wrapped insert function
    pub async fn insert(&self, key: K, value: V) -> Option<Arc<Mutex<V>>> {
        self.lock().await.insert(key, Arc::new(Mutex::new(value)))
    }
    /// Wrapped remove function
    pub async fn remove(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        self.lock().await.remove(key)
    }
}

// K must be Clonable
impl<K, V> Table<K, V>
where
    K: Clone,
{
    /// Wrapped iter function
    pub async fn iter(&self) -> IntoIter<(K, Arc<Mutex<V>>)> {
        let guard = self.lock().await;
        let items: Vec<_> = guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        items.into_iter()
    }

    /// Wrapped keys function
    pub async fn keys(&self) -> IntoIter<K> {
        let guard = self.lock().await;
        let items: Vec<_> = guard.iter().map(|(k, _)| k.clone()).collect();
        items.into_iter()
    }
}

#[derive(Deserialize)]
pub struct ReadPayload<K> {
    id: K,
}
#[derive(Deserialize)]
pub struct DeletePayload<K> {
    id: K,
    real: bool,
}

pub trait TableGetKey<K> {
    fn get_key(&self) -> K;
}
pub trait TableValueUpdate<T> {
    fn update(&self, target: &mut T);
}
pub trait TableValueDeleted {
    fn set_deleted(&mut self) -> bool;
}

impl<'de, K, V> Table<K, V>
where
    K: Eq + Hash + Clone + Deserialize<'de> + Serialize + Send + Sync + 'static,
    V: TableGetKey<K> + TableValueDeleted + Serialize + Send + Sync + 'static,
{
    pub fn router<C, U>(self) -> Router
    where
        C: Into<V> + Deserialize<'de>,
        U: TableValueUpdate<V> + TableGetKey<K> + Deserialize<'de>,
    {
        Router::new()
            .route("/list", post(Self::handle_list))
            .route("/create", post(Self::handle_create))
            .route("/read", post(Self::handle_read))
            .route("/update", post(Self::handle_update))
            .route("/delete", post(Self::handle_delete))
            .layer(Extension(self.clone()))
    }
    async fn handle_list(
        Extension(table): Extension<Self>,
        Json(payload): Json<ListPayload>,
    ) -> impl IntoResponse {
        let limit = payload.limit.unwrap_or(20).clamp(1, 100);
        let offset = payload.offset.unwrap_or(0);
        // Gather list
        let mut data = HashMap::with_capacity(limit);
        for (key, value) in table.iter().await.skip(offset).take(limit) {
            data.insert(key, serde_json::to_value(&*value.lock().await).unwrap());
        }
        // Send list
        if data.is_empty() {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "msg": "Error finding list",
                })),
            )
                .into_response()
        } else {
            (
                StatusCode::OK,
                Json(json!({
                    "msg": "Successfully found list",
                    "content": {
                        "length": data.len(),
                        "limit": limit,
                        "offset": offset,
                        "data": data,
                    },
                })),
            )
                .into_response()
        }
    }
    async fn handle_create<C>(
        Extension(table): Extension<Self>,
        Json(payload): Json<C>,
    ) -> impl IntoResponse
    where
        C: Into<V> + Deserialize<'de>,
    {
        // Convert generic C (or CreatePayload) into generic V
        let value: V = payload.into();
        let key = value.get_key();
        if table.insert(value.get_key(), value).await.is_some() {
            tracing::warn!("Entry overwrote a key. Continuing...");
        }
        (
            StatusCode::CREATED,
            Json(json!({
                "msg": "Successfully created",
                "content": key,
            })),
        )
            .into_response()
    }
    async fn handle_read(
        Extension(table): Extension<Self>,
        Json(payload): Json<ReadPayload<K>>,
    ) -> impl IntoResponse {
        let data = table.get(&payload.id).await;
        if let Some(data) = data {
            (
                StatusCode::OK,
                Json(json!({
                    "msg": "Successfully found",
                    "content": *data.lock().await,
                })),
            )
                .into_response()
        } else {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "msg": "Nothing found",
                })),
            )
                .into_response()
        }
    }
    async fn handle_update<U>(
        Extension(table): Extension<Self>,
        Json(payload): Json<U>,
    ) -> impl IntoResponse
    where
        U: TableValueUpdate<V> + TableGetKey<K> + Deserialize<'de>,
    {
        let data = table.get_mut(&payload.get_key()).await;
        if let Some(data) = data {
            payload.update(&mut *data.lock().await);
            (
                StatusCode::OK,
                Json(json!({
                    "msg": "Successfully updated",
                    "content": &payload.get_key(),
                })),
            )
                .into_response()
        } else {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "msg": "Nothing found"
                })),
            )
                .into_response()
        }
    }
    async fn handle_delete(
        Extension(table): Extension<Self>,
        Json(payload): Json<DeletePayload<K>>,
    ) -> impl IntoResponse {
        // Successful delete response
        let r = (
            StatusCode::OK,
            Json(json!({
                "msg": "Successfully deleted",
            })),
        )
            .into_response();
        if payload.real {
            if table.remove(&payload.id).await.is_some() {
                return r;
            }
        } else {
            let data = table.get(&payload.id).await;
            if let Some(data) = data {
                if (*data.lock().await).set_deleted() {
                    return r;
                } else {
                    if table.remove(&payload.id).await.is_some() {
                        return r;
                    }
                }
            }
        }
        // Default delete response
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "msg": "Nothing found",
            })),
        )
            .into_response()
    }
}

impl<K, V> Clone for Table<K, V> {
    /// Clone the inner arc
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<K, V> Default for Table<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
