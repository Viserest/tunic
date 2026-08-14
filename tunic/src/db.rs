use std::{
    collections::{BTreeMap, HashSet},
    hash::Hash,
    sync::Arc,
    vec::IntoIter,
};

use serde::{Deserialize, Serialize, Serializer};
use tokio::{
    runtime::Handle,
    sync::{Mutex, MutexGuard},
};

#[derive(Default)]
pub struct Table<K, V> {
    inner: Arc<Mutex<BTreeMap<K, Arc<Mutex<V>>>>>,
}

impl<K: Ord + Clone, V> Table<K, V> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Wrapped len function
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Wrapped insert function
    pub async fn insert(&self, key: K, value: V) -> Option<Arc<Mutex<V>>> {
        self.inner
            .lock()
            .await
            .insert(key, Arc::new(Mutex::new(value)))
    }

    /// Wrapped remove function
    pub async fn remove(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        self.inner.lock().await.remove(key)
    }

    /// Wrapped get function
    pub async fn get(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        let inner_guard = self.inner.lock().await;
        Some(inner_guard.get(key)?.clone())
    }

    /// Wrapped get_mut function
    pub async fn get_mut(&self, key: &K) -> Option<Arc<Mutex<V>>> {
        let mut inner_guard = self.inner.lock().await;
        Some(inner_guard.get_mut(key)?.clone())
    }

    /// Wrapped iter function
    pub async fn iter(&self) -> IntoIter<(K, Arc<Mutex<V>>)> {
        let guard = self.inner.lock().await;
        let items: Vec<_> = guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        items.into_iter()
    }

    /// Wrapped keys function
    pub async fn keys(&self) -> IntoIter<K> {
        let guard = self.inner.lock().await;
        let items: Vec<_> = guard.iter().map(|(k, _)| k.clone()).collect();
        items.into_iter()
    }

    /// Wrapped values function
    pub async fn values(&self) -> IntoIter<Arc<Mutex<V>>> {
        let guard = self.inner.lock().await;
        let items: Vec<_> = guard.iter().map(|(_, v)| v.clone()).collect();
        items.into_iter()
    }

    /// Safely lock the table for reading and writing
    pub async fn lock(&self) -> MutexGuard<'_, BTreeMap<K, Arc<Mutex<V>>>> {
        self.inner.lock().await
    }
}

impl<K, V> Clone for Table<K, V> {
    /// Clone the inner arc
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V> Serialize for Table<K, V>
where
    K: Serialize + Ord,
    V: Serialize + Clone,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        tokio::task::block_in_place(|| {
            Handle::current().block_on(async {
                let outer_guard = self.inner.lock().await;
                let mut flattened: BTreeMap<&K, V> = BTreeMap::new();
                for (k, v) in outer_guard.iter() {
                    flattened.insert(k, v.lock().await.clone());
                }
                flattened.serialize(serializer)
            })
        })
    }
}

impl<'de, K, V> Deserialize<'de> for Table<K, V>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let standard_map = BTreeMap::<K, V>::deserialize(deserializer)?;
        let wrapped_map: BTreeMap<K, Arc<Mutex<V>>> = standard_map
            .into_iter()
            .map(|(k, v)| (k, Arc::new(Mutex::new(v))))
            .collect();
        Ok(Table {
            inner: Arc::new(Mutex::new(wrapped_map)),
        })
    }
}

pub struct List<V> {
    inner: Arc<Mutex<HashSet<V>>>,
}

impl<V: Hash + Eq + Clone> List<V> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Wrapped insert function
    pub async fn insert(&self, key: V) -> bool {
        self.inner.lock().await.insert(key)
    }

    /// Wrapped remove function
    pub async fn remove(&self, key: &V) -> bool {
        self.inner.lock().await.remove(key)
    }

    /// Wrapped iter function
    pub async fn iter(&self) -> IntoIter<V> {
        let guard = self.inner.lock().await;
        let items: Vec<_> = guard.iter().map(|v| v.clone()).collect();
        items.into_iter()
    }

    /// Safely lock the table for reading and writing
    pub async fn lock(&self) -> MutexGuard<'_, HashSet<V>> {
        self.inner.lock().await
    }
}

impl<V> Clone for List<V> {
    /// Clone the inner arc
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<V> Serialize for List<V>
where
    V: Serialize + Hash + Eq + Clone,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        tokio::task::block_in_place(|| {
            Handle::current().block_on(async {
                let outer_guard = self.inner.lock().await;
                let mut flattened: HashSet<V> = HashSet::new();
                for k in outer_guard.iter() {
                    flattened.insert(k.clone());
                }
                flattened.serialize(serializer)
            })
        })
    }
}

impl<'de, V> Deserialize<'de> for List<V>
where
    V: Deserialize<'de> + Ord + Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let standard_map = HashSet::<V>::deserialize(deserializer)?;
        Ok(List {
            inner: Arc::new(Mutex::new(standard_map)),
        })
    }
}
