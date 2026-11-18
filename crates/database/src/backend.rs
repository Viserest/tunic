use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use utils::{Value, json_obj, lock};

use crate::DbError;
use crate::scope::valid_segment;

/// One stored document.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub ns: String,
    pub col: String,
    pub id: String,
    pub value: Value,
}

/// Document storage addressed by `namespace / collection / id`.
pub trait Backend: Send + Sync {
    fn get(&self, ns: &str, col: &str, id: &str) -> Result<Option<Value>, DbError>;
    fn put(&self, ns: &str, col: &str, id: &str, value: &Value) -> Result<(), DbError>;
    fn delete(&self, ns: &str, col: &str, id: &str) -> Result<bool, DbError>;
    /// All documents of a collection, ordered by id.
    fn list(&self, ns: &str, col: &str) -> Result<Vec<(String, Value)>, DbError>;
    fn collections(&self, ns: &str) -> Result<Vec<String>, DbError>;
    /// Every document in `prefix` and its child scopes (used to replicate/sync).
    fn dump(&self, prefix: &str) -> Result<Vec<Entry>, DbError>;
}

/// Copy everything under `prefix` from one backend to another. Returns the number of documents.
pub fn replicate(src: &dyn Backend, dst: &dyn Backend, prefix: &str) -> Result<usize, DbError> {
    let entries = src.dump(prefix)?;
    for e in &entries {
        dst.put(&e.ns, &e.col, &e.id, &e.value)?;
    }
    Ok(entries.len())
}

type Key = (String, String);

#[derive(Default)]
struct State {
    data: HashMap<Key, BTreeMap<String, Value>>,
    loaded: HashSet<Key>,
    files: HashMap<Key, File>,
}

/// In-memory storage, optionally persisted to a directory.
///
/// Layout on disk: `<root>/<scope segments>/<collection>.jsonl`, an append-only log with one
/// `{"id":..,"value":..}` (put) or `{"id":..,"deleted":true}` line per change. Logs are replayed
/// when a collection is first used and rewritten (compacted) if they hold mostly dead lines.
/// A torn last line (crash mid-write) is ignored.
pub struct LocalBackend {
    root: Option<PathBuf>,
    state: Mutex<State>,
    sync_writes: bool,
    _lock: Option<DirLock>,
}

struct DirLock(PathBuf);

impl DirLock {
    fn acquire(root: &Path) -> Result<DirLock, DbError> {
        let path = root.join(".lock");
        let me = std::process::id();
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{me}");
                    return Ok(DirLock(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let pid: Option<u32> = fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok());
                    let alive = pid.is_some_and(|p| p != me && Path::new(&format!("/proc/{p}")).exists());
                    if alive {
                        return Err(DbError::Io(format!("data directory {} is in use by process {}", root.display(), pid.unwrap())));
                    }
                    let _ = fs::remove_file(&path); // stale lock from a dead process
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(DbError::Io("could not lock data directory".into()))
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl LocalBackend {
    pub fn memory() -> Self {
        LocalBackend { root: None, state: Mutex::new(State::default()), sync_writes: false, _lock: None }
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, DbError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let lock = DirLock::acquire(&root)?;
        Ok(LocalBackend { root: Some(root), state: Mutex::new(State::default()), sync_writes: false, _lock: Some(lock) })
    }

    /// `fsync` after every write (slower, survives power loss).
    pub fn sync_writes(mut self, yes: bool) -> Self {
        self.sync_writes = yes;
        self
    }

    fn check(ns: &str, col: &str) -> Result<(), DbError> {
        if ns.split('/').all(valid_segment) && valid_segment(col) {
            Ok(())
        } else {
            Err(DbError::InvalidScope(format!("{ns}/{col}")))
        }
    }

    fn path(&self, ns: &str, col: &str) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let mut p = root.clone();
        for seg in ns.split('/') {
            p.push(seg);
        }
        p.push(format!("{col}.jsonl"));
        Some(p)
    }

    fn load(&self, st: &mut State, ns: &str, col: &str) -> Result<(), DbError> {
        let key = (ns.to_string(), col.to_string());
        if st.loaded.contains(&key) {
            return Ok(());
        }
        let mut map = BTreeMap::new();
        if let Some(path) = self.path(ns, col) {
            if let Ok(text) = fs::read_to_string(&path) {
                let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                let mut dead = 0usize;
                for (n, line) in lines.iter().enumerate() {
                    match utils::json::parse(line) {
                        Ok(v) => {
                            let Some(id) = v.get("id").and_then(|i| i.as_str()) else { continue };
                            if v.get("deleted").and_then(|d| d.as_bool()) == Some(true) {
                                dead += 1 + map.remove(id).is_some() as usize;
                            } else if let Some(val) = v.get("value") {
                                dead += map.insert(id.to_string(), val.clone()).is_some() as usize;
                            }
                        }
                        // a torn final line is normal after a crash; earlier corruption is not
                        Err(e) if n + 1 == lines.len() => {
                            let _ = e;
                        }
                        Err(e) => return Err(DbError::Io(format!("{}: line {}: {e}", path.display(), n + 1))),
                    }
                }
                if dead > 64 && dead > map.len() {
                    Self::compact(&path, &map)?;
                }
            }
        }
        st.data.insert(key.clone(), map);
        st.loaded.insert(key);
        Ok(())
    }

    fn compact(path: &Path, map: &BTreeMap<String, Value>) -> Result<(), DbError> {
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut f = File::create(&tmp)?;
            for (id, v) in map {
                writeln!(f, "{}", json_obj! {"id" => id.as_str(), "value" => v.clone()})?;
            }
            f.sync_data()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    fn append(&self, st: &mut State, ns: &str, col: &str, line: &Value) -> Result<(), DbError> {
        let Some(path) = self.path(ns, col) else { return Ok(()) };
        let key = (ns.to_string(), col.to_string());
        if !st.files.contains_key(&key) {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            st.files.insert(key.clone(), OpenOptions::new().create(true).append(true).open(&path)?);
        }
        let f = st.files.get_mut(&key).unwrap();
        // one write call per line keeps lines intact
        f.write_all(format!("{line}\n").as_bytes())?;
        if self.sync_writes {
            f.sync_data()?;
        }
        Ok(())
    }
}

impl Backend for LocalBackend {
    fn get(&self, ns: &str, col: &str, id: &str) -> Result<Option<Value>, DbError> {
        Self::check(ns, col)?;
        let mut st = lock(&self.state);
        self.load(&mut st, ns, col)?;
        Ok(st.data.get(&(ns.to_string(), col.to_string())).and_then(|m| m.get(id)).cloned())
    }

    fn put(&self, ns: &str, col: &str, id: &str, value: &Value) -> Result<(), DbError> {
        Self::check(ns, col)?;
        let mut st = lock(&self.state);
        self.load(&mut st, ns, col)?;
        self.append(&mut st, ns, col, &json_obj! {"id" => id, "value" => value.clone()})?;
        st.data.get_mut(&(ns.to_string(), col.to_string())).unwrap().insert(id.to_string(), value.clone());
        Ok(())
    }

    fn delete(&self, ns: &str, col: &str, id: &str) -> Result<bool, DbError> {
        Self::check(ns, col)?;
        let mut st = lock(&self.state);
        self.load(&mut st, ns, col)?;
        let key = (ns.to_string(), col.to_string());
        if !st.data.get(&key).is_some_and(|m| m.contains_key(id)) {
            return Ok(false);
        }
        self.append(&mut st, ns, col, &json_obj! {"id" => id, "deleted" => true})?;
        st.data.get_mut(&key).unwrap().remove(id);
        Ok(true)
    }

    fn list(&self, ns: &str, col: &str) -> Result<Vec<(String, Value)>, DbError> {
        Self::check(ns, col)?;
        let mut st = lock(&self.state);
        self.load(&mut st, ns, col)?;
        Ok(st.data.get(&(ns.to_string(), col.to_string())).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
    }

    fn collections(&self, ns: &str) -> Result<Vec<String>, DbError> {
        if !ns.split('/').all(valid_segment) {
            return Err(DbError::InvalidScope(ns.into()));
        }
        let mut set: std::collections::BTreeSet<String> = Default::default();
        {
            let st = lock(&self.state);
            for ((n, c), m) in &st.data {
                if n == ns && !m.is_empty() {
                    set.insert(c.clone());
                }
            }
        }
        if let Some(root) = &self.root {
            let mut dir = root.clone();
            ns.split('/').for_each(|s| dir.push(s));
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    if let Some(name) = e.file_name().to_str().and_then(|n| n.strip_suffix(".jsonl")) {
                        set.insert(name.to_string());
                    }
                }
            }
        }
        Ok(set.into_iter().collect())
    }

    fn dump(&self, prefix: &str) -> Result<Vec<Entry>, DbError> {
        // find every (ns, col) on disk and in memory below `prefix`
        let mut keys: std::collections::BTreeSet<Key> = Default::default();
        {
            let st = lock(&self.state);
            for k in st.data.keys() {
                keys.insert(k.clone());
            }
        }
        if let Some(root) = &self.root {
            fn walk(dir: &Path, ns: &str, out: &mut Vec<Key>) {
                let Ok(rd) = fs::read_dir(dir) else { return };
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let p = e.path();
                    if p.is_dir() {
                        let child = if ns.is_empty() { name.clone() } else { format!("{ns}/{name}") };
                        walk(&p, &child, out);
                    } else if let Some(col) = name.strip_suffix(".jsonl") {
                        if !ns.is_empty() {
                            out.push((ns.to_string(), col.to_string()));
                        }
                    }
                }
            }
            let mut found = Vec::new();
            walk(root, "", &mut found);
            keys.extend(found);
        }
        let mut out = Vec::new();
        for (ns, col) in keys {
            let inside = prefix.is_empty() || ns == prefix || ns.starts_with(&format!("{prefix}/"));
            if !inside {
                continue;
            }
            for (id, value) in self.list(&ns, &col)? {
                out.push(Entry { ns: ns.clone(), col: col.clone(), id, value });
            }
        }
        Ok(out)
    }
}

/// Sends each scope to the backend registered for its longest matching prefix.
///
/// This is how one process talks to several databases: e.g. `hpc` on database 2 and everything
/// else on database 1.
pub struct RoutedBackend {
    default: Arc<dyn Backend>,
    routes: Vec<(String, Arc<dyn Backend>)>,
}

impl RoutedBackend {
    pub fn new(default: Arc<dyn Backend>) -> Self {
        RoutedBackend { default, routes: Vec::new() }
    }

    pub fn route(mut self, scope_prefix: &str, backend: Arc<dyn Backend>) -> Self {
        self.routes.push((scope_prefix.trim_matches('/').to_string(), backend));
        self.routes.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        self
    }

    fn pick(&self, ns: &str) -> &Arc<dyn Backend> {
        self.routes
            .iter()
            .find(|(p, _)| ns == p || ns.starts_with(&format!("{p}/")))
            .map(|(_, b)| b)
            .unwrap_or(&self.default)
    }
}

impl Backend for RoutedBackend {
    fn get(&self, ns: &str, col: &str, id: &str) -> Result<Option<Value>, DbError> {
        self.pick(ns).get(ns, col, id)
    }
    fn put(&self, ns: &str, col: &str, id: &str, v: &Value) -> Result<(), DbError> {
        self.pick(ns).put(ns, col, id, v)
    }
    fn delete(&self, ns: &str, col: &str, id: &str) -> Result<bool, DbError> {
        self.pick(ns).delete(ns, col, id)
    }
    fn list(&self, ns: &str, col: &str) -> Result<Vec<(String, Value)>, DbError> {
        self.pick(ns).list(ns, col)
    }
    fn collections(&self, ns: &str) -> Result<Vec<String>, DbError> {
        self.pick(ns).collections(ns)
    }
    fn dump(&self, prefix: &str) -> Result<Vec<Entry>, DbError> {
        let mut out = Vec::new();
        let mut seen: Vec<*const ()> = Vec::new();
        for b in std::iter::once(&self.default).chain(self.routes.iter().map(|(_, b)| b)) {
            let ptr = Arc::as_ptr(b) as *const ();
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            for e in b.dump(prefix)? {
                if std::ptr::eq(Arc::as_ptr(self.pick(&e.ns)) as *const (), ptr) {
                    out.push(e);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use utils::Uuid;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tunic-db-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn memory_crud() {
        let b = LocalBackend::memory();
        assert_eq!(b.get("a", "c", "1").unwrap(), None);
        b.put("a", "c", "2", &Value::Int(2)).unwrap();
        b.put("a", "c", "1", &Value::Int(1)).unwrap();
        assert_eq!(b.list("a", "c").unwrap(), vec![("1".into(), Value::Int(1)), ("2".into(), Value::Int(2))]);
        assert!(b.delete("a", "c", "1").unwrap());
        assert!(!b.delete("a", "c", "1").unwrap());
        assert_eq!(b.collections("a").unwrap(), vec!["c".to_string()]);
        assert!(b.get("../etc", "passwd", "x").is_err());
    }

    #[test]
    fn persists_and_replays() {
        let dir = tmp();
        {
            let b = LocalBackend::open(&dir).unwrap();
            b.put("app/x", "things", "1", &json_obj! {"n" => 1}).unwrap();
            b.put("app/x", "things", "2", &json_obj! {"n" => 2}).unwrap();
            b.put("app/x", "things", "1", &json_obj! {"n" => 10}).unwrap();
            b.delete("app/x", "things", "2").unwrap();
        }
        let b = LocalBackend::open(&dir).unwrap();
        assert_eq!(b.get("app/x", "things", "1").unwrap().unwrap().path("n").unwrap().as_i64(), Some(10));
        assert_eq!(b.get("app/x", "things", "2").unwrap(), None);
        assert_eq!(b.dump("app").unwrap().len(), 1);
        assert_eq!(b.dump("other").unwrap().len(), 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn torn_last_line_is_ignored_and_compaction_works() {
        let dir = tmp();
        {
            let b = LocalBackend::open(&dir).unwrap();
            for i in 0..300 {
                b.put("s", "c", "k", &Value::Int(i)).unwrap();
            }
        }
        let path = dir.join("s").join("c.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"id\":\"k\",\"val").unwrap(); // torn write
        drop(f);
        let b = LocalBackend::open(&dir).unwrap();
        assert_eq!(b.get("s", "c", "k").unwrap(), Some(Value::Int(299)));
        // compaction rewrote the log down to one line
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corruption_in_the_middle_is_an_error() {
        let dir = tmp();
        fs::create_dir_all(dir.join("s")).unwrap();
        fs::write(dir.join("s").join("c.jsonl"), "not json\n{\"id\":\"a\",\"value\":1}\n").unwrap();
        let b = LocalBackend::open(&dir).unwrap();
        assert!(b.get("s", "c", "a").is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn data_dir_is_locked() {
        let dir = tmp();
        let _b = LocalBackend::open(&dir).unwrap();
        // our own pid does not count as "another process": a stale-looking lock is taken over
        assert!(LocalBackend::open(&dir).is_ok());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn routing_and_replication() {
        let a: Arc<dyn Backend> = Arc::new(LocalBackend::memory());
        let b: Arc<dyn Backend> = Arc::new(LocalBackend::memory());
        let r = RoutedBackend::new(a.clone()).route("hpc", b.clone());
        r.put("user", "u", "1", &Value::Int(1)).unwrap();
        r.put("hpc", "jobs", "1", &Value::Int(2)).unwrap();
        r.put("hpc/deep", "jobs", "1", &Value::Int(3)).unwrap();
        r.put("hpcx", "jobs", "1", &Value::Int(4)).unwrap();
        assert_eq!(a.get("user", "u", "1").unwrap(), Some(Value::Int(1)));
        assert_eq!(b.get("hpc", "jobs", "1").unwrap(), Some(Value::Int(2)));
        assert_eq!(b.get("hpc/deep", "jobs", "1").unwrap(), Some(Value::Int(3)));
        assert_eq!(a.get("hpcx", "jobs", "1").unwrap(), Some(Value::Int(4)));
        assert_eq!(r.dump("").unwrap().len(), 4);
        let c = LocalBackend::memory();
        assert_eq!(replicate(&r, &c, "hpc").unwrap(), 2);
        assert_eq!(c.get("hpc", "jobs", "1").unwrap(), Some(Value::Int(2)));
    }
}
