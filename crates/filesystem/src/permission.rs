use serde::{Deserialize, Serialize};
use tunic_macro::TableValueCrud;
use uuid::Uuid;

use crate::db::table::{TableGetKey, TableValueDeleted, TableValueUpdate};

#[derive(Deserialize, Serialize, Clone, Copy)]
pub enum Principal {
    Commenter,
    Viewer,
    Editor,
    Owner,
}

#[derive(Deserialize, Serialize, TableValueCrud)]
pub struct Permission {
    #[tunic(id)]
    id: Uuid,
    #[tunic(create)]
    user_id: Uuid,
    #[tunic(create)]
    resource_id: Uuid,
    #[tunic(create, update)]
    level: Principal,
    created_at: u64,
    accessed_at: u64,
    #[tunic(delete)]
    deleted_at: u64,
}
impl Permission {
    pub fn new(user_id: Uuid, resource_id: Uuid, level: Principal) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            resource_id,
            level,
            created_at: crate::unix_now(),
            accessed_at: 0,
            deleted_at: 0,
        }
    }
}
