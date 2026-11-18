use database::{Record, Uuid, field_enum};

/// What a user may do with a file or folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Commenter,
    Viewer,
    Editor,
    Owner,
}
field_enum!(Level { Commenter => "commenter", Viewer => "viewer", Editor => "editor", Owner => "owner" });

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s {
            "commenter" => Level::Commenter,
            "viewer" => Level::Viewer,
            "editor" => Level::Editor,
            "owner" => Level::Owner,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct FileMeta {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    /// SHA-256 of the content.
    pub hash: [u8; 32],
    pub directory_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Directory {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub created_at: u64,
}

/// `user_id` may access `resource_id` (a file or directory) at `level`.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Permission {
    pub id: Uuid,
    pub resource_id: Uuid,
    pub user_id: Uuid,
    pub level: Level,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Playlist {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    pub file_ids: Vec<Uuid>,
    pub created_at: u64,
}
