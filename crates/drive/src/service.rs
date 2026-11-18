use std::fmt;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use database::{Database, DbError, Table, Uuid};
use utils::sha256::sha256;
use utils::unix_now;

use crate::{Directory, FileMeta, Level, MimeType, Permission, Playlist, sanitize_filename};

#[derive(Debug)]
pub enum DriveError {
    NotFound,
    Forbidden,
    TooLarge(u64),
    Invalid(String),
    Storage(String),
}

impl fmt::Display for DriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriveError::NotFound => write!(f, "Nothing found"),
            DriveError::Forbidden => write!(f, "Forbidden"),
            DriveError::TooLarge(max) => write!(f, "file is larger than the {max} byte limit"),
            DriveError::Invalid(s) => write!(f, "{s}"),
            DriveError::Storage(s) => write!(f, "storage error: {s}"),
        }
    }
}
impl std::error::Error for DriveError {}
impl From<DbError> for DriveError {
    fn from(e: DbError) -> Self {
        DriveError::Storage(e.to_string())
    }
}
impl From<std::io::Error> for DriveError {
    fn from(e: std::io::Error) -> Self {
        DriveError::Storage(e.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct DriveConfig {
    pub max_file_bytes: u64,
    /// Per-user storage quota in bytes (0 = unlimited).
    pub quota_bytes: u64,
}

impl Default for DriveConfig {
    fn default() -> Self {
        DriveConfig { max_file_bytes: 64 * 1024 * 1024, quota_bytes: 0 }
    }
}

#[derive(Clone)]
pub struct Drive {
    pub files: Table<FileMeta>,
    pub dirs: Table<Directory>,
    pub permissions: Table<Permission>,
    pub playlists: Table<Playlist>,
    dir: PathBuf,
    pub config: DriveConfig,
}

impl Drive {
    /// `db` must be a scoped handle; file contents are stored under `files_dir`.
    pub fn new(db: Database, files_dir: impl Into<PathBuf>, config: DriveConfig) -> Result<Drive, DriveError> {
        let dir = files_dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Drive {
            files: Table::new(db.clone())?,
            dirs: Table::new(db.clone())?,
            permissions: Table::new(db.clone())?,
            playlists: Table::new(db)?,
            dir,
            config,
        })
    }

    fn path_of(&self, id: &Uuid) -> PathBuf {
        // the id is a parsed Uuid, so the file name can only be hex and dashes
        self.dir.join(id.to_string())
    }

    /// Highest level `user` has on a file or directory: owner, a direct grant, or a grant on any
    /// folder above it.
    pub fn level(&self, user: &Uuid, resource: &Uuid) -> Option<Level> {
        let mut best: Option<Level> = None;
        let mut cur = Some(*resource);
        let mut hops = 0;
        let mut first = true;
        while let Some(id) = cur {
            hops += 1;
            if hops > 64 {
                break; // cycle or absurd nesting
            }
            let (owner, parent) = if let Some(f) = self.files.get(&id).ok().flatten() {
                (f.owner, f.directory_id)
            } else if let Some(d) = self.dirs.get(&id).ok().flatten() {
                (d.owner, d.parent_id)
            } else {
                if first {
                    return None;
                }
                break;
            };
            first = false;
            if owner == *user {
                return Some(Level::Owner);
            }
            if let Ok(perms) = self.permissions.find(|p| p.resource_id == id && p.user_id == *user) {
                for p in perms {
                    best = best.max(Some(p.level));
                }
            }
            cur = parent;
        }
        best
    }

    fn require(&self, user: &Uuid, resource: &Uuid, needed: Level) -> Result<(), DriveError> {
        match self.level(user, resource) {
            None => Err(DriveError::NotFound), // do not reveal existence
            Some(l) if l >= needed => Ok(()),
            Some(_) => Err(DriveError::Forbidden),
        }
    }

    fn used_bytes(&self, owner: &Uuid) -> u64 {
        self.files.find(|f| f.owner == *owner).unwrap_or_default().iter().map(|f| f.size).sum()
    }

    /// Store a new file.
    pub fn upload(&self, user: &Uuid, name: &str, content_type: Option<&str>, data: &[u8], dir: Option<Uuid>) -> Result<FileMeta, DriveError> {
        if data.len() as u64 > self.config.max_file_bytes {
            return Err(DriveError::TooLarge(self.config.max_file_bytes));
        }
        if self.config.quota_bytes > 0 && self.used_bytes(user) + data.len() as u64 > self.config.quota_bytes {
            return Err(DriveError::Invalid("storage quota exceeded".into()));
        }
        if let Some(d) = dir {
            if self.dirs.get(&d)?.is_none() {
                return Err(DriveError::NotFound);
            }
            self.require(user, &d, Level::Editor)?;
        }
        let name = sanitize_filename(name);
        // trust the extension over the client's claim when it names a type we know
        let by_ext = MimeType::from_ext(&name);
        let ct = if by_ext != MimeType::OCTET_STREAM {
            by_ext.0.to_string()
        } else {
            content_type.and_then(MimeType::from_mime).unwrap_or(MimeType::OCTET_STREAM).0.to_string()
        };
        let id = Uuid::new_v4();
        // write to a temp file, then rename: readers never see half a file
        let tmp = self.dir.join(format!("{id}.tmp"));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_data()?;
        }
        fs::rename(&tmp, self.path_of(&id))?;
        let now = unix_now();
        let meta = FileMeta { id, owner: *user, name, content_type: ct, size: data.len() as u64, hash: sha256(data), directory_id: dir, created_at: now, updated_at: now };
        if let Err(e) = self.files.insert(&meta) {
            let _ = fs::remove_file(self.path_of(&id));
            return Err(e.into());
        }
        Ok(meta)
    }

    /// Metadata and bytes of a file the user may view.
    pub fn read(&self, user: &Uuid, id: &Uuid) -> Result<(FileMeta, Vec<u8>), DriveError> {
        self.require(user, id, Level::Viewer)?;
        let meta = self.files.get(id)?.ok_or(DriveError::NotFound)?;
        let data = fs::read(self.path_of(id))?;
        if sha256(&data) != meta.hash {
            return Err(DriveError::Storage("file content does not match its stored hash".into()));
        }
        Ok((meta, data))
    }

    pub fn metadata(&self, user: &Uuid, id: &Uuid) -> Result<FileMeta, DriveError> {
        self.require(user, id, Level::Viewer)?;
        self.files.get(id)?.ok_or(DriveError::NotFound)
    }

    /// Replace the content of a file (editor or owner).
    pub fn overwrite(&self, user: &Uuid, id: &Uuid, data: &[u8]) -> Result<FileMeta, DriveError> {
        self.require(user, id, Level::Editor)?;
        if data.len() as u64 > self.config.max_file_bytes {
            return Err(DriveError::TooLarge(self.config.max_file_bytes));
        }
        let tmp = self.dir.join(format!("{id}.tmp"));
        fs::write(&tmp, data)?;
        fs::rename(&tmp, self.path_of(id))?;
        let (h, n, now) = (sha256(data), data.len() as u64, unix_now());
        self.files.update(id, |f| {
            f.hash = h;
            f.size = n;
            f.updated_at = now;
        })?.ok_or(DriveError::NotFound)
    }

    pub fn delete(&self, user: &Uuid, id: &Uuid) -> Result<(), DriveError> {
        self.require(user, id, Level::Owner)?;
        self.files.remove(id)?;
        let _ = fs::remove_file(self.path_of(id));
        for p in self.permissions.find(|p| p.resource_id == *id)? {
            self.permissions.remove(&p.id)?;
        }
        Ok(())
    }

    /// Give another user access. Only owners can share. Re-sharing updates the level.
    pub fn share(&self, owner: &Uuid, resource: &Uuid, with: &Uuid, level: Level) -> Result<Permission, DriveError> {
        self.require(owner, resource, Level::Owner)?;
        if level == Level::Owner {
            return Err(DriveError::Invalid("ownership cannot be shared".into()));
        }
        if let Some(existing) = self.permissions.first(|p| p.resource_id == *resource && p.user_id == *with)? {
            return self.permissions.update(&existing.id, |p| p.level = level)?.ok_or(DriveError::NotFound);
        }
        let p = Permission { id: Uuid::new_v4(), resource_id: *resource, user_id: *with, level, created_at: unix_now() };
        self.permissions.insert(&p)?;
        Ok(p)
    }

    pub fn unshare(&self, owner: &Uuid, resource: &Uuid, with: &Uuid) -> Result<bool, DriveError> {
        self.require(owner, resource, Level::Owner)?;
        let mut any = false;
        for p in self.permissions.find(|p| p.resource_id == *resource && p.user_id == *with)? {
            any |= self.permissions.remove(&p.id)?;
        }
        Ok(any)
    }

    pub fn mkdir(&self, user: &Uuid, name: &str, parent: Option<Uuid>) -> Result<Directory, DriveError> {
        let name = sanitize_filename(name);
        if let Some(p) = parent {
            if self.dirs.get(&p)?.is_none() {
                return Err(DriveError::NotFound);
            }
            self.require(user, &p, Level::Editor)?;
        }
        let d = Directory { id: Uuid::new_v4(), owner: *user, name, parent_id: parent, created_at: unix_now() };
        self.dirs.insert(&d)?;
        Ok(d)
    }

    /// Files and folders directly inside `dir` (`None` = the user's root: things they own with no
    /// parent plus things shared with them directly) that the user may see.
    pub fn list(&self, user: &Uuid, dir: Option<Uuid>) -> Result<(Vec<Directory>, Vec<FileMeta>), DriveError> {
        if let Some(d) = dir {
            self.require(user, &d, Level::Viewer)?;
        }
        let dirs = self.dirs.find(|d| d.parent_id == dir)?.into_iter().filter(|d| self.level(user, &d.id).is_some()).collect::<Vec<_>>();
        let files = self.files.find(|f| f.directory_id == dir)?.into_iter().filter(|f| self.level(user, &f.id).is_some()).collect::<Vec<_>>();
        let (mut dirs, mut files) = (dirs, files);
        if dir.is_none() {
            // things shared with the user that live inside someone else's folders
            for p in self.permissions.find(|p| p.user_id == *user)? {
                if let Some(f) = self.files.get(&p.resource_id)? {
                    if f.directory_id.is_some() && !files.iter().any(|x| x.id == f.id) {
                        files.push(f);
                    }
                } else if let Some(d) = self.dirs.get(&p.resource_id)? {
                    if d.parent_id.is_some() && !dirs.iter().any(|x| x.id == d.id) {
                        dirs.push(d);
                    }
                }
            }
        }
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok((dirs, files))
    }

    // ---- media playlists

    pub fn create_playlist(&self, user: &Uuid, name: &str) -> Result<Playlist, DriveError> {
        if name.trim().is_empty() {
            return Err(DriveError::Invalid("name is required".into()));
        }
        let p = Playlist { id: Uuid::new_v4(), owner: *user, name: name.trim().chars().take(200).collect(), file_ids: vec![], created_at: unix_now() };
        self.playlists.insert(&p)?;
        Ok(p)
    }

    pub fn playlist_add(&self, user: &Uuid, playlist: &Uuid, file: &Uuid) -> Result<Playlist, DriveError> {
        let pl = self.playlists.get(playlist)?.filter(|p| p.owner == *user).ok_or(DriveError::NotFound)?;
        self.require(user, file, Level::Viewer)?;
        let meta = self.files.get(file)?.ok_or(DriveError::NotFound)?;
        if !MimeType::from_mime(&meta.content_type).is_some_and(|m| m.is_media()) {
            return Err(DriveError::Invalid("only audio, video and images can be added to a playlist".into()));
        }
        if pl.file_ids.contains(file) {
            return Ok(pl);
        }
        self.playlists.update(playlist, |p| p.file_ids.push(*file))?.ok_or(DriveError::NotFound)
    }

    pub fn playlists_of(&self, user: &Uuid) -> Result<Vec<Playlist>, DriveError> {
        Ok(self.playlists.find(|p| p.owner == *user)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive() -> (Drive, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tunic-drive-{}", Uuid::new_v4()));
        (Drive::new(Database::memory().scope("drive").unwrap(), &dir, DriveConfig { max_file_bytes: 1000, quota_bytes: 1500 }).unwrap(), dir)
    }

    #[test]
    fn upload_read_hash_and_limits() {
        let (d, dir) = drive();
        let me = Uuid::new_v4();
        let f = d.upload(&me, "../notes.txt", Some("application/x-evil"), b"hello", None).unwrap();
        assert_eq!((f.name.as_str(), f.content_type.as_str(), f.size), ("notes.txt", "text/plain", 5));
        let (m, data) = d.read(&me, &f.id).unwrap();
        assert_eq!((data.as_slice(), m.hash), (&b"hello"[..], sha256(b"hello")));
        assert!(matches!(d.upload(&me, "big", None, &[0; 1001], None), Err(DriveError::TooLarge(_))));
        d.upload(&me, "a", None, &[0; 900], None).unwrap();
        assert!(d.upload(&me, "b", None, &[0; 900], None).is_err(), "quota");
        // tampering with the stored bytes is detected
        fs::write(d.path_of(&f.id), b"HELLO").unwrap();
        assert!(matches!(d.read(&me, &f.id), Err(DriveError::Storage(_))));
        // client-claimed type is only used when known
        let g = d.upload(&me, "blob", Some("image/png"), b"x", None).unwrap();
        assert_eq!(g.content_type, "image/png");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn permissions_and_inheritance() {
        let (d, dir) = drive();
        let (alice, bob, carol) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let folder = d.mkdir(&alice, "Projects", None).unwrap();
        let inner = d.mkdir(&alice, "2026", Some(folder.id)).unwrap();
        let f = d.upload(&alice, "plan.txt", None, b"plan", Some(inner.id)).unwrap();

        assert!(matches!(d.read(&bob, &f.id), Err(DriveError::NotFound)), "strangers cannot even tell it exists");
        d.share(&alice, &folder.id, &bob, Level::Viewer).unwrap();
        assert_eq!(d.level(&bob, &f.id), Some(Level::Viewer), "inherited from the folder two levels up");
        assert!(d.read(&bob, &f.id).is_ok());
        assert!(matches!(d.overwrite(&bob, &f.id, b"x"), Err(DriveError::Forbidden)));
        assert!(matches!(d.delete(&bob, &f.id), Err(DriveError::Forbidden)));
        assert!(matches!(d.share(&bob, &f.id, &carol, Level::Viewer), Err(DriveError::Forbidden)));
        // upgrade on the file itself
        d.share(&alice, &f.id, &bob, Level::Editor).unwrap();
        assert_eq!(d.level(&bob, &f.id), Some(Level::Editor));
        assert_eq!(d.overwrite(&bob, &f.id, b"edited").unwrap().size, 6);
        assert!(d.share(&alice, &f.id, &carol, Level::Owner).is_err());
        assert!(d.unshare(&alice, &folder.id, &bob).unwrap());
        assert_eq!(d.level(&bob, &f.id), Some(Level::Editor), "file-level grant remains");
        // owner deletes: bytes and permissions are gone
        d.delete(&alice, &f.id).unwrap();
        assert!(!d.path_of(&f.id).exists());
        assert!(d.permissions.find(|p| p.resource_id == f.id).unwrap().is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn listing_and_playlists() {
        let (d, dir) = drive();
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let folder = d.mkdir(&alice, "Music", None).unwrap();
        let song = d.upload(&alice, "song.mp3", None, b"mp3", Some(folder.id)).unwrap();
        let doc = d.upload(&alice, "doc.txt", None, b"t", None).unwrap();
        let (dirs, files) = d.list(&alice, None).unwrap();
        assert_eq!((dirs.len(), files.len()), (1, 1));
        assert_eq!(d.list(&alice, Some(folder.id)).unwrap().1[0].id, song.id);
        assert!(d.list(&bob, Some(folder.id)).is_err());
        assert_eq!(d.list(&bob, None).unwrap().0.len(), 0, "other users' root items are invisible");
        d.share(&alice, &song.id, &bob, Level::Viewer).unwrap();
        assert_eq!(d.list(&bob, None).unwrap().1.len(), 1, "shared items appear in the recipient's root");

        let pl = d.create_playlist(&alice, "Mix").unwrap();
        assert_eq!(d.playlist_add(&alice, &pl.id, &song.id).unwrap().file_ids, vec![song.id]);
        assert_eq!(d.playlist_add(&alice, &pl.id, &song.id).unwrap().file_ids.len(), 1, "no duplicates");
        assert!(d.playlist_add(&alice, &pl.id, &doc.id).is_err(), "text is not media");
        assert!(d.playlist_add(&bob, &pl.id, &song.id).is_err(), "not bob's playlist");
        assert!(d.create_playlist(&alice, "  ").is_err());
        fs::remove_dir_all(dir).ok();
    }
}
