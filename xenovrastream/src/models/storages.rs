use serde::Serialize;

use crate::common::types::ChatId;

pub struct InStorage {
    pub name: String,
    pub chat_id: ChatId,
}

impl InStorage {
    pub fn new(name: String, chat_id: ChatId) -> Self {
        Self { name, chat_id }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Storage {
    pub id: uuid::Uuid,
    pub name: String,
    pub chat_id: ChatId,
}

impl Storage {
    pub fn new(id: uuid::Uuid, name: String, chat_id: ChatId) -> Self {
        Self { id, name, chat_id }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StorageWithInfo {
    pub id: uuid::Uuid,
    pub name: String,
    pub chat_id: ChatId,
    pub videos_amount: i64,
    /// Total source bytes ingested — not what ended up on Telegram, since the
    /// ABR ladder stores several renditions per source.
    pub size: i64,
}
