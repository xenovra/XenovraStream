use sqlx::PgPool;
use uuid::Uuid;

use crate::common::db::errors::map_not_found;
use crate::errors::{XenovraStreamError, XenovraStreamResult};
use crate::models::jobs::TranscodeJob;
use crate::models::videos::{InRendition, InSegment, InVideo, Rendition, Segment, Video, VideoStatus};

const VIDEOS_TABLE: &str = "videos";
const RENDITIONS_TABLE: &str = "renditions";
const SEGMENTS_TABLE: &str = "segments";
const JOBS_TABLE: &str = "transcode_jobs";

pub struct VideosRepository<'d> {
    db: &'d PgPool,
}

impl<'d> VideosRepository<'d> {
    pub fn new(db: &'d PgPool) -> Self {
        Self { db }
    }

    pub async fn create(&self, in_obj: InVideo) -> XenovraStreamResult<Video> {
        sqlx::query_as(&format!(
            "
            INSERT INTO {VIDEOS_TABLE}
                (id, public_id, title, original_filename, size, storage_id, user_id, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued')
            RETURNING *;
        "
        ))
        .bind(Uuid::new_v4())
        .bind(in_obj.public_id)
        .bind(in_obj.title)
        .bind(in_obj.original_filename)
        .bind(in_obj.size)
        .bind(in_obj.storage_id)
        .bind(in_obj.user_id)
        .fetch_one(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub async fn list_by_user_id(&self, user_id: Uuid) -> XenovraStreamResult<Vec<Video>> {
        sqlx::query_as(&format!(
            "SELECT * FROM {VIDEOS_TABLE} WHERE user_id = $1 ORDER BY created_at DESC"
        ))
        .bind(user_id)
        .fetch_all(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub async fn get(&self, id: Uuid) -> XenovraStreamResult<Video> {
        sqlx::query_as(&format!("SELECT * FROM {VIDEOS_TABLE} WHERE id = $1"))
            .bind(id)
            .fetch_one(self.db)
            .await
            .map_err(|e| map_not_found(e, "video"))
    }

    pub async fn get_by_public_id(&self, public_id: &str) -> XenovraStreamResult<Video> {
        sqlx::query_as(&format!("SELECT * FROM {VIDEOS_TABLE} WHERE public_id = $1"))
            .bind(public_id)
            .fetch_one(self.db)
            .await
            .map_err(|e| map_not_found(e, "video"))
    }

    pub async fn delete(&self, id: Uuid) -> XenovraStreamResult<()> {
        sqlx::query(&format!("DELETE FROM {VIDEOS_TABLE} WHERE id = $1"))
            .bind(id)
            .execute(self.db)
            .await
            .map_err(|e| map_not_found(e, "video"))?;
        Ok(())
    }

    pub async fn set_status(&self, id: Uuid, status: VideoStatus) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "UPDATE {VIDEOS_TABLE} SET status = $2, updated_at = NOW() WHERE id = $1"
        ))
        .bind(id)
        .bind(status)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "video"))?;
        Ok(())
    }

    pub async fn set_progress(&self, id: Uuid, progress: i16) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "UPDATE {VIDEOS_TABLE} SET progress = $2, updated_at = NOW() WHERE id = $1"
        ))
        .bind(id)
        .bind(progress)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "video"))?;
        Ok(())
    }

    pub async fn set_duration(&self, id: Uuid, duration: f64) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "UPDATE {VIDEOS_TABLE} SET duration_secs = $2, updated_at = NOW() WHERE id = $1"
        ))
        .bind(id)
        .bind(duration)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "video"))?;
        Ok(())
    }

    pub async fn set_failed(&self, id: Uuid, error: &str) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "
            UPDATE {VIDEOS_TABLE}
            SET status = 'failed', error = $2, updated_at = NOW()
            WHERE id = $1
        "
        ))
        .bind(id)
        .bind(error)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "video"))?;
        Ok(())
    }

    // -- renditions ---------------------------------------------------------

    pub async fn create_rendition(&self, in_obj: InRendition) -> XenovraStreamResult<Rendition> {
        sqlx::query_as(&format!(
            "
            INSERT INTO {RENDITIONS_TABLE}
                (id, video_id, name, width, height, bandwidth, codecs, target_duration)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *;
        "
        ))
        .bind(Uuid::new_v4())
        .bind(in_obj.video_id)
        .bind(in_obj.name)
        .bind(in_obj.width)
        .bind(in_obj.height)
        .bind(in_obj.bandwidth)
        .bind(in_obj.codecs)
        .bind(in_obj.target_duration)
        .fetch_one(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    /// Ordered smallest-first so the master playlist lists the cheapest variant
    /// first — players without bandwidth history start there and step up.
    pub async fn list_renditions(&self, video_id: Uuid) -> XenovraStreamResult<Vec<Rendition>> {
        sqlx::query_as(&format!(
            "SELECT * FROM {RENDITIONS_TABLE} WHERE video_id = $1 ORDER BY bandwidth ASC"
        ))
        .bind(video_id)
        .fetch_all(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub async fn get_rendition(
        &self,
        video_id: Uuid,
        name: &str,
    ) -> XenovraStreamResult<Rendition> {
        sqlx::query_as(&format!(
            "SELECT * FROM {RENDITIONS_TABLE} WHERE video_id = $1 AND name = $2"
        ))
        .bind(video_id)
        .bind(name)
        .fetch_one(self.db)
        .await
        .map_err(|e| map_not_found(e, "rendition"))
    }

    // -- segments -----------------------------------------------------------

    pub async fn create_segments_batch(&self, segments: Vec<InSegment>) -> XenovraStreamResult<()> {
        if segments.is_empty() {
            return Ok(());
        }

        let mut query = format!(
            "INSERT INTO {SEGMENTS_TABLE}
                (id, rendition_id, position, duration, size, telegram_file_id, telegram_message_id)
             VALUES "
        );
        let values = (0..segments.len())
            .map(|i| {
                let o = i * 7;
                format!(
                    "(${}, ${}, ${}, ${}, ${}, ${}, ${})",
                    o + 1,
                    o + 2,
                    o + 3,
                    o + 4,
                    o + 5,
                    o + 6,
                    o + 7
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        query.push_str(&values);

        let mut q = sqlx::query(&query);
        for s in segments {
            q = q
                .bind(Uuid::new_v4())
                .bind(s.rendition_id)
                .bind(s.position)
                .bind(s.duration)
                .bind(s.size)
                .bind(s.telegram_file_id)
                .bind(s.telegram_message_id);
        }

        q.execute(self.db).await.map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })?;
        Ok(())
    }

    pub async fn list_segments(&self, rendition_id: Uuid) -> XenovraStreamResult<Vec<Segment>> {
        sqlx::query_as(&format!(
            "SELECT * FROM {SEGMENTS_TABLE} WHERE rendition_id = $1 ORDER BY position ASC"
        ))
        .bind(rendition_id)
        .fetch_all(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub async fn get_segment(
        &self,
        rendition_id: Uuid,
        position: i32,
    ) -> XenovraStreamResult<Segment> {
        sqlx::query_as(&format!(
            "SELECT * FROM {SEGMENTS_TABLE} WHERE rendition_id = $1 AND position = $2"
        ))
        .bind(rendition_id)
        .bind(position)
        .fetch_one(self.db)
        .await
        .map_err(|e| map_not_found(e, "segment"))
    }

    /// Every Telegram message backing a video, for cleanup on delete.
    pub async fn list_video_telegram_messages(
        &self,
        video_id: Uuid,
    ) -> XenovraStreamResult<Vec<(Uuid, i64)>> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(&format!(
            "
            SELECT s.id, s.telegram_message_id
            FROM {SEGMENTS_TABLE} s
            JOIN {RENDITIONS_TABLE} r ON s.rendition_id = r.id
            WHERE r.video_id = $1 AND s.telegram_message_id <> 0
        "
        ))
        .bind(video_id)
        .fetch_all(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })?;
        Ok(rows)
    }

    // -- jobs ---------------------------------------------------------------

    pub async fn create_job(&self, video_id: Uuid, source_path: &str) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "
            INSERT INTO {JOBS_TABLE} (id, video_id, source_path, state)
            VALUES ($1, $2, $3, 'pending');
        "
        ))
        .bind(Uuid::new_v4())
        .bind(video_id)
        .bind(source_path)
        .execute(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })?;
        Ok(())
    }

    /// Atomically takes the oldest pending job. `FOR UPDATE SKIP LOCKED` means
    /// two workers racing here can never claim the same row, so the worker count
    /// can be raised later without reworking this.
    pub async fn claim_job(&self) -> XenovraStreamResult<Option<TranscodeJob>> {
        sqlx::query_as(&format!(
            "
            UPDATE {JOBS_TABLE}
            SET state = 'running', attempts = attempts + 1, started_at = NOW()
            WHERE id = (
                SELECT id FROM {JOBS_TABLE}
                WHERE state = 'pending'
                ORDER BY created_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, video_id, source_path, state, attempts, last_error;
        "
        ))
        .fetch_optional(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub async fn finish_job(&self, id: Uuid) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "UPDATE {JOBS_TABLE} SET state = 'done', finished_at = NOW() WHERE id = $1"
        ))
        .bind(id)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "job"))?;
        Ok(())
    }

    pub async fn fail_job(&self, id: Uuid, error: &str) -> XenovraStreamResult<()> {
        sqlx::query(&format!(
            "
            UPDATE {JOBS_TABLE}
            SET state = 'failed', last_error = $2, finished_at = NOW()
            WHERE id = $1
        "
        ))
        .bind(id)
        .bind(error)
        .execute(self.db)
        .await
        .map_err(|e| map_not_found(e, "job"))?;
        Ok(())
    }

    /// Re-queues jobs that were mid-flight when the process died. Called once at
    /// boot: a `running` row with nobody running it is by definition orphaned,
    /// since a claim only survives in memory.
    pub async fn requeue_orphaned_jobs(&self) -> XenovraStreamResult<u64> {
        let result = sqlx::query(&format!(
            "
            UPDATE {JOBS_TABLE}
            SET state = 'pending', last_error = 'requeued after restart'
            WHERE state = 'running'
        "
        ))
        .execute(self.db)
        .await
        .map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })?;
        Ok(result.rows_affected())
    }
}
