use serde::{Deserialize, Serialize};
use tunic_macro::TableValueCrud;
use uuid::Uuid;

use crate::db::table::{TableGetKey, TableValueDeleted, TableValueUpdate};

#[derive(Deserialize, Serialize, TableValueCrud)]
pub struct Directory {
    #[tunic(id)]
    id: Uuid,
    #[tunic(create, update)]
    parent_id: Uuid,
    #[tunic(update)]
    permission_index: Vec<Uuid>,
    #[tunic(update)]
    files: Vec<Uuid>,
    created_at: u64,
    accessd_at: u64,
    updated_at: u64,
    #[tunic(delete)]
    deleted_at: u64,
}
impl Directory {
    pub fn new(parent_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_id,
            permission_index: Vec::new(),
            files: Vec::new(),
            created_at: crate::unix_now(),
            accessd_at: 0,
            updated_at: 0,
            deleted_at: 0,
        }
    }
}
