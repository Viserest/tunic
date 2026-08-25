pub trait TableValueTrait {}
pub trait StateTrait: Default + Clone + Send + 'static {}

///
/// Async database tables using [`std::collections::HashMap`]
///
/// Uses [`std::sync::Arc`] for cheap cloning
///
mod table {
    use std::{collections::HashMap, hash::Hash, sync::Arc, vec::IntoIter};
    use tokio::sync::{Mutex, MutexGuard};

    pub struct Table<K, V> {
        inner: Arc<Mutex<HashMap<K, Arc<Mutex<V>>>>>,
        cache: Arc<Mutex<HashMap<K, u32>>>,
    }

    impl<K, V> Table<K, V>
    where
        K: Hash + Ord + Clone,
    {
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(HashMap::new())),
                cache: Arc::new(Mutex::new(HashMap::new())),
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
        pub async fn lock(&self) -> MutexGuard<'_, HashMap<K, Arc<Mutex<V>>>> {
            self.inner.lock().await
        }
    }

    impl<K, V> Clone for Table<K, V> {
        /// Clone the inner arc
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
                cache: self.cache.clone(),
            }
        }
    }

    impl<K, V> Default for Table<K, V>
    where
        K: Hash + Ord + Clone,
    {
        fn default() -> Self {
            Self::new()
        }
    }
}
pub use table::Table;

///
/// Async database lists using [`std::collections::HashSet`]
///
/// Uses [`std::sync::Arc`] for cheap cloning
///
mod list {
    use std::{collections::HashSet, hash::Hash, sync::Arc, vec::IntoIter};
    use tokio::sync::{Mutex, MutexGuard};

    pub struct List<V> {
        inner: Arc<Mutex<HashSet<V>>>,
    }

    impl<V> List<V>
    where
        V: Hash + Eq + Clone,
    {
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

    impl<V> Default for List<V>
    where
        V: Hash + Eq + Clone,
    {
        fn default() -> Self {
            Self::new()
        }
    }
}
pub use list::List;

#[cfg(feature = "database")]
pub mod daemon {
    use serde::{Deserialize, Serialize};
    use tokio::{
        io::AsyncReadExt,
        net::{TcpListener, TcpStream},
        sync::broadcast::Receiver,
    };

    #[derive(Deserialize, Serialize)]
    #[repr(u8)]
    enum PacketType {
        Request = 0,
        Response = 1,
        Sync = 2,
    }

    impl TryFrom<u8> for PacketType {
        type Error = u8;

        fn try_from(v: u8) -> Result<Self, Self::Error> {
            match v {
                0 => Ok(PacketType::Request),
                1 => Ok(PacketType::Response),
                2 => Ok(PacketType::Sync),
                other => Err(other),
            }
        }
    }

    async fn handle_request(client: &mut TcpStream) {}

    pub async fn run_database(mut shutdown_rx: Receiver<()>, state: impl StateTrait) {
        tracing::info!("Database server started.");

        let listener = TcpListener::bind("127.0.0.1:34423").await.unwrap();
        loop {
            tokio::select! {
                res = listener.accept() => {
                    let (mut client, _) = res.unwrap();
                    let packet_type_byte = client.read_u8().await.unwrap();
                    let packet_type = PacketType::try_from(packet_type_byte).unwrap();
                    match packet_type {
                        PacketType::Request => {}
                        PacketType::Response => {}
                        PacketType::Sync => {}
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }
    }
}
