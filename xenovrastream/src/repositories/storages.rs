use sqlx::PgPool;
use uuid::Uuid;

use crate::common::db::errors::map_not_found;
use crate::errors::{XenovraStreamError, XenovraStreamResult};
use crate::models::storages::{InStorage, Storage, StorageWithInfo};
use crate::repositories::access::TABLE as ACCESS_TABLE;

const VIDEOS_TABLE: &str = "videos";

pub const TABLE: &str = "storages";

pub struct StoragesRepository<'d> {
    db: &'d PgPool,
}

impl<'d> StoragesRepository<'d> {
    pub fn new(db: &'d PgPool) -> Self {
        Self { db }
    }

    pub async fn create(&self, in_obj: InStorage) -> XenovraStreamResult<Storage> {
        let id = Uuid::new_v4();

        sqlx::query(
            format!("INSERT INTO {TABLE} (id, name, chat_id) VALUES ($1, $2, $3)").as_str(),
        )
        .bind(id)
        .bind(in_obj.name.clone())
        .bind(in_obj.chat_id)
        .execute(self.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(dbe) if dbe.is_foreign_key_violation() => {
                XenovraStreamError::UserWasRemoved
            }
            sqlx::Error::Database(dbe) if dbe.is_unique_violation() => {
                XenovraStreamError::StorageChatIdConflict
            }
            _ => {
                tracing::error!("{e}");
                XenovraStreamError::Unknown
            }
        })?;

        let storage = Storage::new(id, in_obj.name, in_obj.chat_id);
        Ok(storage)
    }

    pub async fn list_by_user_id(&self, user_id: Uuid) -> XenovraStreamResult<Vec<StorageWithInfo>> {
        sqlx::query_as(
            format!(
                "
                SELECT s.*,
                       COUNT(v.id) AS videos_amount,
                       COALESCE(SUM(v.size), 0)::BigInt as size
                FROM {TABLE} s
                JOIN {ACCESS_TABLE} a ON s.id = a.storage_id
                LEFT JOIN {VIDEOS_TABLE} v ON s.id = v.storage_id
                WHERE a.user_id = $1
                GROUP by s.id
            "
            )
            .as_str(),
        )
        .bind(user_id)
        .fetch_all(self.db)
        .await
        .map_err(|e| map_not_found(e, "storages"))
    }

    pub async fn get_by_id(&self, id: Uuid) -> XenovraStreamResult<Storage> {
        sqlx::query_as(format!("SELECT * FROM {TABLE} WHERE id = $1").as_str())
            .bind(id)
            .fetch_one(self.db)
            .await
            .map_err(|e| map_not_found(e, "storage"))
    }

    pub async fn get_by_name_and_user_id(
        &self,
        name: &str,
        user_id: Uuid,
    ) -> XenovraStreamResult<Storage> {
        sqlx::query_as(
            format!(
                "
                SELECT s.* 
                FROM {TABLE} s
                JOIN {ACCESS_TABLE} a ON s.id = a.storage_id
                WHERE a.user_id = $1 AND s.name = $2
            "
            )
            .as_str(),
        )
        .bind(user_id)
        .bind(name)
        .fetch_one(self.db)
        .await
        .map_err(|e| map_not_found(e, "storage"))
    }

    pub async fn delete_storage(&self, storage_id: Uuid) -> XenovraStreamResult<()> {
        sqlx::query(format!("DELETE FROM {TABLE} WHERE id = $1").as_str())
            .bind(storage_id)
            .execute(self.db)
            .await
            .map_err(|e| map_not_found(e, "storage"))?;
        Ok(())
    }
}
