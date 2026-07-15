use sqlx::PgPool;
use uuid::Uuid;

use crate::common::access::check_access;
use crate::common::jwt_manager::AuthUser;
use crate::common::telegram_api::bot_api::TelegramBotApi;
use crate::config::Config;
use crate::errors::{XenovraStreamError, XenovraStreamResult};
use crate::models::access::AccessType;
use crate::models::videos::{InVideo, Rendition, Video, VideoStatus};
use crate::repositories::access::AccessRepository;
use crate::repositories::storage_workers::StorageWorkersRepository;
use crate::repositories::storages::StoragesRepository;
use crate::repositories::videos::VideosRepository;
use crate::services::storage_workers_scheduler::StorageWorkersScheduler;

pub struct VideosService<'d> {
    db: &'d PgPool,
    repo: VideosRepository<'d>,
    access_repo: AccessRepository<'d>,
}

impl<'d> VideosService<'d> {
    pub fn new(db: &'d PgPool) -> Self {
        Self {
            db,
            repo: VideosRepository::new(db),
            access_repo: AccessRepository::new(db),
        }
    }

    /// A link slug that cannot be enumerated. 20 hex chars is 80 bits of
    /// randomness — brute-forcing it is not a threat model we need to worry
    /// about, which is the whole premise of an unlisted link.
    fn generate_public_id() -> String {
        Uuid::new_v4().simple().to_string()[..20].to_owned()
    }

    /// Registers an already-on-disk upload and queues it for transcoding.
    ///
    /// The bytes are written by the router straight to `source_path` — they
    /// never pass through here, so a 10 GB upload costs 10 GB of disk and
    /// almost no RAM.
    pub async fn register_upload(
        &self,
        title: String,
        original_filename: String,
        size: i64,
        source_path: &str,
        storage_id: Uuid,
        user: &AuthUser,
    ) -> XenovraStreamResult<Video> {
        check_access(&self.access_repo, user.id, storage_id, &AccessType::W).await?;

        if !StorageWorkersRepository::new(self.db)
            .storage_has_any(storage_id)
            .await?
        {
            return Err(XenovraStreamError::NoStorageWorkers);
        }

        let video = self
            .repo
            .create(InVideo {
                public_id: Self::generate_public_id(),
                title,
                original_filename,
                size,
                storage_id,
                user_id: user.id,
            })
            .await?;

        self.repo.create_job(video.id, source_path).await?;
        Ok(video)
    }

    pub async fn list(&self, user: &AuthUser) -> XenovraStreamResult<Vec<Video>> {
        self.repo.list_by_user_id(user.id).await
    }

    pub async fn get(&self, id: Uuid, user: &AuthUser) -> XenovraStreamResult<Video> {
        let video = self.repo.get(id).await?;
        check_access(&self.access_repo, user.id, video.storage_id, &AccessType::R).await?;
        Ok(video)
    }

    /// Public lookup for playback — deliberately skips the access check, since
    /// holding the unguessable `public_id` *is* the authorisation.
    pub async fn get_public(&self, public_id: &str) -> XenovraStreamResult<Video> {
        let video = self.repo.get_by_public_id(public_id).await?;

        if video.status != VideoStatus::Ready {
            return Err(XenovraStreamError::VideoNotReady);
        }

        Ok(video)
    }

    pub async fn list_renditions(&self, video_id: Uuid) -> XenovraStreamResult<Vec<Rendition>> {
        self.repo.list_renditions(video_id).await
    }

    /// Deletes the video, its Telegram messages, and its rows.
    ///
    /// Telegram cleanup is best-effort and happens first: if we dropped the
    /// rows first and then failed, the messages would be orphaned in the chat
    /// with nothing left pointing at them.
    pub async fn delete(&self, id: Uuid, config: &Config, user: &AuthUser) -> XenovraStreamResult<()> {
        let video = self.repo.get(id).await?;
        check_access(&self.access_repo, user.id, video.storage_id, &AccessType::W).await?;

        let storage = StoragesRepository::new(self.db)
            .get_by_id(video.storage_id)
            .await?;
        let messages = self.repo.list_video_telegram_messages(id).await?;

        for (segment_id, message_id) in &messages {
            let scheduler = StorageWorkersScheduler::new(self.db, config.telegram_rate_limit);
            let api = TelegramBotApi::new(&config.telegram_api_base_url, scheduler);

            if let Err(e) = api
                .delete_message(*message_id, storage.chat_id, video.storage_id)
                .await
            {
                tracing::warn!("[DELETE] cannot remove telegram message {message_id}: {e}");
            }

            let cached = config.cache_dir().join(format!("{segment_id}.ts"));
            let _ = tokio::fs::remove_file(cached).await;
        }

        // renditions/segments/jobs cascade from here.
        self.repo.delete(id).await
    }
}
