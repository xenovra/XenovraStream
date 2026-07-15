use std::path::Path;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use tokio::time::sleep;
use uuid::Uuid;

use crate::common::telegram_api::bot_api::TelegramBotApi;
use crate::config::Config;
use crate::errors::{XenovraStreamError, XenovraStreamResult};
use crate::models::jobs::TranscodeJob;
use crate::models::videos::{InRendition, InSegment, VideoStatus};
use crate::repositories::storages::StoragesRepository;
use crate::repositories::videos::VideosRepository;
use crate::services::ffmpeg::{Ffmpeg, Rung, SourceInfo};
use crate::services::storage_workers_scheduler::StorageWorkersScheduler;

/// Polling interval when the queue is empty.
const IDLE_POLL: Duration = Duration::from_secs(3);

/// Drains `transcode_jobs`, one job at a time.
///
/// Serial by design: ffmpeg already saturates every core, so running two jobs
/// concurrently would only make both slower and thrash the page cache. The
/// concurrency that matters — pushing segments to Telegram — happens inside a
/// job, where the work is network-bound.
pub struct Transcoder {
    db: PgPool,
    config: Config,
}

impl Transcoder {
    pub fn new(db: PgPool, config: Config) -> Self {
        Self { db, config }
    }

    pub async fn run(&self) {
        loop {
            let claimed = VideosRepository::new(&self.db).claim_job().await;

            match claimed {
                Ok(Some(job)) => {
                    let video_id = job.video_id;
                    let job_id = job.id;
                    let source_path = job.source_path.clone();

                    tracing::info!("[TRANSCODE] starting job {job_id} for video {video_id}");

                    match self.process(job).await {
                        Ok(()) => {
                            let _ = VideosRepository::new(&self.db).finish_job(job_id).await;
                            tracing::info!("[TRANSCODE] finished video {video_id}");
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            tracing::error!("[TRANSCODE] video {video_id} failed: {msg}");
                            let repo = VideosRepository::new(&self.db);
                            let _ = repo.fail_job(job_id, &msg).await;
                            let _ = repo.set_failed(video_id, &msg).await;
                        }
                    }

                    // The source is only useful to a retry, and we do not retry
                    // automatically, so drop it either way — a failed 4 GB
                    // upload should not sit on disk forever.
                    let _ = tokio::fs::remove_file(&source_path).await;
                    let _ = tokio::fs::remove_dir_all(self.job_dir(video_id)).await;
                }
                Ok(None) => sleep(IDLE_POLL).await,
                Err(e) => {
                    tracing::error!("[TRANSCODE] cannot claim a job: {e}");
                    sleep(IDLE_POLL).await;
                }
            }
        }
    }

    fn job_dir(&self, video_id: Uuid) -> std::path::PathBuf {
        self.config.transcode_dir().join(video_id.to_string())
    }

    async fn process(&self, job: TranscodeJob) -> XenovraStreamResult<()> {
        let repo = VideosRepository::new(&self.db);
        let video = repo.get(job.video_id).await?;
        let source = Path::new(&job.source_path);

        if tokio::fs::metadata(source).await.is_err() {
            return Err(XenovraStreamError::IoError(format!(
                "source file {} is gone",
                job.source_path
            )));
        }

        let info = Ffmpeg::probe(source).await?;
        repo.set_duration(video.id, info.duration_secs).await?;
        repo.set_status(video.id, VideoStatus::Transcoding).await?;

        let ladder = Ffmpeg::ladder_for(&info);
        tracing::info!(
            "[TRANSCODE] video {} is {}x{} {:.1}s -> {} rendition(s)",
            video.id,
            info.width,
            info.height,
            info.duration_secs,
            ladder.len()
        );

        let job_dir = self.job_dir(video.id);
        tokio::fs::create_dir_all(&job_dir).await?;

        // Transcode every rung first, then upload. Doing it this way means a
        // rung that fails to encode never leaves half its segments on Telegram.
        let mut encoded = Vec::new();
        for (index, rung) in ladder.iter().enumerate() {
            let out_dir = job_dir.join(rung.name);
            let mut last_reported = -1i16;

            let playlist = Ffmpeg::transcode_rung(
                source,
                &out_dir,
                rung,
                &info,
                self.config.segment_secs,
                &self.config.x264_preset,
                |done| {
                    // Transcoding is the first 80% of the bar; uploading is the
                    // rest. Each rung owns an equal slice of that 80%.
                    let span = 80.0 / ladder.len() as f64;
                    let overall = (index as f64 * span + done * span).round() as i16;
                    if overall != last_reported {
                        last_reported = overall;
                        let db = self.db.clone();
                        let video_id = video.id;
                        tokio::spawn(async move {
                            let _ = VideosRepository::new(&db).set_progress(video_id, overall).await;
                        });
                    }
                },
            )
            .await?;

            let segments = Ffmpeg::parse_playlist(&playlist).await?;
            encoded.push((*rung, segments));
        }

        repo.set_status(video.id, VideoStatus::Uploading).await?;

        let storage = StoragesRepository::new(&self.db)
            .get_by_id(video.storage_id)
            .await?;
        let total_segments: usize = encoded.iter().map(|(_, s)| s.len()).sum();
        let mut uploaded_so_far = 0usize;

        for (rung, segments) in encoded {
            let target_duration = segments
                .iter()
                .map(|(_, d)| *d)
                .fold(0.0f64, f64::max)
                .ceil() as i32;

            let rendition = repo
                .create_rendition(InRendition {
                    video_id: video.id,
                    name: rung.name.to_owned(),
                    // Width is what ffmpeg actually produced for this height,
                    // derived from the source aspect ratio and rounded even.
                    width: Self::scaled_width(&info, &rung),
                    height: rung.height.min(info.height),
                    bandwidth: (rung.video_kbps + rung.audio_kbps) * 1000,
                    codecs: rung.codecs.to_owned(),
                    target_duration: target_duration.max(1),
                })
                .await?;

            let rows = self
                .upload_segments(&segments, storage.chat_id, video.storage_id, rendition.id)
                .await?;

            repo.create_segments_batch(rows).await?;

            uploaded_so_far += segments.len();
            let progress = 80 + (uploaded_so_far as f64 / total_segments as f64 * 20.0) as i16;
            repo.set_progress(video.id, progress.min(99)).await?;
        }

        repo.set_progress(video.id, 100).await?;
        repo.set_status(video.id, VideoStatus::Ready).await?;
        Ok(())
    }

    /// Mirrors ffmpeg's `scale=-2:h` — keep the aspect ratio, round to even.
    fn scaled_width(info: &SourceInfo, rung: &Rung) -> i32 {
        let height = rung.height.min(info.height);
        let width = (info.width as f64 * height as f64 / info.height as f64).round() as i32;
        width + (width % 2)
    }

    async fn upload_segments(
        &self,
        segments: &[(std::path::PathBuf, f64)],
        chat_id: i64,
        storage_id: Uuid,
        rendition_id: Uuid,
    ) -> XenovraStreamResult<Vec<InSegment>> {
        let concurrency = self.config.upload_concurrency.max(1) as usize;

        // Collect owned items up front: mapping over borrowed tuples makes the
        // async blocks higher-ranked over the iterator's lifetime, which the
        // spawned worker future then cannot satisfy.
        let items: Vec<(i32, std::path::PathBuf, f64)> = segments
            .iter()
            .enumerate()
            .map(|(position, (path, duration))| (position as i32, path.clone(), *duration))
            .collect();

        // `buffered` keeps output in input order, so `position` stays aligned
        // with playback order even though uploads finish out of order.
        let results: Vec<XenovraStreamResult<InSegment>> =
            stream::iter(items.into_iter().map(|(position, path, duration)| async move {
                let bytes = tokio::fs::read(&path).await?;
                let size = bytes.len() as i32;

                let scheduler =
                    StorageWorkersScheduler::new(&self.db, self.config.telegram_rate_limit);
                let api = TelegramBotApi::new(&self.config.telegram_api_base_url, scheduler);
                let uploaded = api.upload(&bytes, chat_id, storage_id).await?;

                Ok(InSegment {
                    rendition_id,
                    position,
                    duration,
                    size,
                    telegram_file_id: uploaded.file_id,
                    telegram_message_id: uploaded.message_id,
                })
            }))
            .buffered(concurrency)
            .collect()
            .await;

        results.into_iter().collect()
    }
}
