use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use utils::{Uuid, lock};

use crate::{Database, DbError, Record};

/// Typed collection of [`Record`]s inside a scope.
///
/// Cheap to clone. [`Table::update`] is an atomic read-modify-write among all tables of the same
/// database handle in this process (remote databases are not locked across processes).
pub struct Table<V: Record> {
    db: Database,
    lock: Arc<Mutex<()>>,
    _p: PhantomData<fn() -> V>,
}

impl<V: Record> Clone for Table<V> {
    fn clone(&self) -> Self {
        Table { db: self.db.clone(), lock: self.lock.clone(), _p: PhantomData }
    }
}

impl<V: Record> Table<V> {
    /// `db` must be a scoped handle (see [`Database::scope`]).
    pub fn new(db: Database) -> Result<Self, DbError> {
        if db.scope_name().is_none() {
            return Err(DbError::ScopeRequired);
        }
        let lock = db.collection_lock(V::COLLECTION)?;
        Ok(Table { db, lock, _p: PhantomData })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    fn decode(id: &str, v: &utils::Value) -> Result<V, DbError> {
        V::from_value(v).map_err(|e| DbError::Invalid(format!("{}/{id}: {e}", V::COLLECTION)))
    }

    /// Insert or overwrite.
    pub fn insert(&self, v: &V) -> Result<(), DbError> {
        let _g = lock(&self.lock);
        self.db.put(V::COLLECTION, &v.id().to_string(), &v.to_value())
    }

    /// Insert only if the id is free. Returns `false` if it already exists.
    pub fn create(&self, v: &V) -> Result<bool, DbError> {
        let _g = lock(&self.lock);
        let id = v.id().to_string();
        if self.db.get(V::COLLECTION, &id)?.is_some() {
            return Ok(false);
        }
        self.db.put(V::COLLECTION, &id, &v.to_value())?;
        Ok(true)
    }

    pub fn get(&self, id: &Uuid) -> Result<Option<V>, DbError> {
        let id = id.to_string();
        self.db.get(V::COLLECTION, &id)?.map(|v| Self::decode(&id, &v)).transpose()
    }

    /// Atomically modify a record. Returns the new value, or `None` if it does not exist.
    pub fn update(&self, id: &Uuid, f: impl FnOnce(&mut V)) -> Result<Option<V>, DbError> {
        let _g = lock(&self.lock);
        let sid = id.to_string();
        let Some(raw) = self.db.get(V::COLLECTION, &sid)? else { return Ok(None) };
        let mut v = Self::decode(&sid, &raw)?;
        f(&mut v);
        if v.id() != *id {
            return Err(DbError::Invalid("a record's id cannot change".into()));
        }
        self.db.put(V::COLLECTION, &sid, &v.to_value())?;
        Ok(Some(v))
    }

    pub fn remove(&self, id: &Uuid) -> Result<bool, DbError> {
        let _g = lock(&self.lock);
        self.db.delete(V::COLLECTION, &id.to_string())
    }

    pub fn all(&self) -> Result<Vec<V>, DbError> {
        self.db.list(V::COLLECTION)?.iter().map(|(id, v)| Self::decode(id, v)).collect()
    }

    pub fn count(&self) -> Result<usize, DbError> {
        Ok(self.db.list(V::COLLECTION)?.len())
    }

    /// A page of records ordered by id.
    pub fn list(&self, offset: usize, limit: usize) -> Result<Vec<V>, DbError> {
        self.db.list(V::COLLECTION)?.iter().skip(offset).take(limit).map(|(id, v)| Self::decode(id, v)).collect()
    }

    pub fn find(&self, pred: impl Fn(&V) -> bool) -> Result<Vec<V>, DbError> {
        Ok(self.all()?.into_iter().filter(|v| pred(v)).collect())
    }

    pub fn first(&self, pred: impl Fn(&V) -> bool) -> Result<Option<V>, DbError> {
        Ok(self.all()?.into_iter().find(|v| pred(v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Record;

    #[derive(Clone, Debug, PartialEq, Record)]
    #[record(collection = "widgets")]
    struct Widget {
        id: Uuid,
        name: String,
        #[record(default)]
        count: u32,
        tags: Vec<String>,
        owner: Option<Uuid>,
        #[record(hidden)]
        secret: String,
    }

    fn w(name: &str) -> Widget {
        Widget { id: Uuid::new_v4(), name: name.into(), count: 0, tags: vec!["a".into()], owner: None, secret: "s3cret".into() }
    }

    fn table() -> Table<Widget> {
        Table::new(Database::memory().scope("test").unwrap()).unwrap()
    }

    #[test]
    fn crud() {
        let t = table();
        let a = w("a");
        assert!(t.create(&a).unwrap());
        assert!(!t.create(&a).unwrap());
        assert_eq!(t.get(&a.id).unwrap(), Some(a.clone()));
        let up = t.update(&a.id, |x| x.count += 5).unwrap().unwrap();
        assert_eq!(up.count, 5);
        assert!(t.update(&Uuid::new_v4(), |_| {}).unwrap().is_none());
        assert!(t.update(&a.id, |x| x.id = Uuid::new_v4()).is_err());
        assert_eq!(t.count().unwrap(), 1);
        assert!(t.remove(&a.id).unwrap());
        assert_eq!(t.get(&a.id).unwrap(), None);
    }

    #[test]
    fn queries_and_pages() {
        let t = table();
        for i in 0..10 {
            t.insert(&w(&format!("w{i}"))).unwrap();
        }
        assert_eq!(t.all().unwrap().len(), 10);
        assert_eq!(t.list(3, 4).unwrap().len(), 4);
        assert_eq!(t.list(8, 4).unwrap().len(), 2);
        assert_eq!(t.find(|w| w.name == "w3").unwrap().len(), 1);
        assert!(t.first(|w| w.name == "zzz").unwrap().is_none());
    }

    #[test]
    fn root_handle_is_rejected_and_scopes_isolate() {
        assert!(Table::<Widget>::new(Database::memory()).is_err());
        let root = Database::memory();
        let a: Table<Widget> = Table::new(root.scope("a").unwrap()).unwrap();
        let b: Table<Widget> = Table::new(root.scope("b").unwrap()).unwrap();
        a.insert(&w("x")).unwrap();
        assert_eq!(a.count().unwrap(), 1);
        assert_eq!(b.count().unwrap(), 0);
    }

    #[test]
    fn derive_semantics() {
        let x = w("n");
        let v = x.to_value();
        assert_eq!(v.get("name").unwrap().as_str(), Some("n"));
        assert!(v.get("owner").unwrap().is_null());
        // default + optional fields may be missing
        let mut raw = v.clone();
        raw.as_object_mut().unwrap().remove("count");
        raw.as_object_mut().unwrap().remove("owner");
        let back = Widget::from_value(&raw).unwrap();
        assert_eq!(back.count, 0);
        assert_eq!(back.owner, None);
        // required fields are errors, with the field named
        raw.as_object_mut().unwrap().remove("name");
        assert!(Widget::from_value(&raw).unwrap_err().contains("name"));
        assert!(Widget::from_value(&utils::Value::Int(1)).is_err());
        // hidden fields
        assert_eq!(Widget::hidden_fields(), &["secret"]);
        assert!(x.public_value().get("secret").is_none());
        assert!(x.to_value().get("secret").is_some());
    }

    #[test]
    fn concurrent_updates_do_not_lose_writes() {
        let t = table();
        let a = w("c");
        t.insert(&a).unwrap();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let t = t.clone();
                let id = a.id;
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        t.update(&id, |w| w.count += 1).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(t.get(&a.id).unwrap().unwrap().count, 400);
    }
}
