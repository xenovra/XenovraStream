use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use uuid::Uuid;

use crate::{
    common::{routing::app_state::AppState, telegram_api::bot_api::TelegramBotApi},
    errors::{XenovraStreamError, XenovraStreamResult},
    models::videos::Video,
    repositories::videos::VideosRepository,
    services::{
        playlist::Playlist, storage_workers_scheduler::StorageWorkersScheduler,
        videos::VideosService,
    },
};

/// How many segments ahead to warm the cache on each segment hit.
///
/// Players request segments one at a time, just in time. Without a read-ahead
/// the very first play of a video pays the full Telegram round-trip on every
/// segment boundary and stutters. Two is enough to stay ahead of a 6-second
/// segment while staying modest against the per-bot rate limit.
const PREFETCH_AHEAD: i32 = 2;

/// Public, unauthenticated playback. Mounted outside the JWT layer — the
/// unguessable `public_id` is the credential.
pub struct StreamRouter;

impl StreamRouter {
    pub fn get_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/:public_id/master.m3u8", get(Self::master))
            .route("/:public_id/:rendition/index.m3u8", get(Self::media))
            .route("/:public_id/:rendition/:segment", get(Self::segment))
            .with_state(state)
    }

    async fn master(
        State(state): State<Arc<AppState>>,
        Path(public_id): Path<String>,
    ) -> impl IntoResponse {
        let service = VideosService::new(&state.db);
        let video = service.get_public(&public_id).await?;
        let renditions = service.list_renditions(video.id).await?;

        if renditions.is_empty() {
            return Err(XenovraStreamError::VideoNotReady.into());
        }

        let body = Playlist::master(&public_id, &renditions);
        Ok::<_, (StatusCode, String)>(Self::playlist_response(body))
    }

    async fn media(
        State(state): State<Arc<AppState>>,
        Path((public_id, rendition_name)): Path<(String, String)>,
    ) -> impl IntoResponse {
        let video = VideosService::new(&state.db).get_public(&public_id).await?;
        let repo = VideosRepository::new(&state.db);

        let rendition = repo.get_rendition(video.id, &rendition_name).await?;
        let segments = repo.list_segments(rendition.id).await?;

        let body = Playlist::media(&public_id, &rendition, &segments);
        Ok::<_, (StatusCode, String)>(Self::playlist_response(body))
    }

    async fn segment(
        State(state): State<Arc<AppState>>,
        Path((public_id, rendition_name, segment)): Path<(String, String, String)>,
    ) -> impl IntoResponse {
        // `12.ts` -> 12
        let position: i32 = segment
            .strip_suffix(".ts")
            .and_then(|p| p.parse().ok())
            .ok_or(XenovraStreamError::InvalidPath)?;

        let video = VideosService::new(&state.db).get_public(&public_id).await?;
        let repo = VideosRepository::new(&state.db);
        let rendition = repo.get_rendition(video.id, &rendition_name).await?;
        let seg = repo.get_segment(rendition.id, position).await?;

        let bytes = Self::load_segment(&state, &video, seg.id, &seg.telegram_file_id).await?;

        Self::spawn_prefetch(&state, &video, rendition.id, position);

        Ok::<_, (StatusCode, String)>(
            (
                [
                    (header::CONTENT_TYPE, "video/mp2t".to_owned()),
                    // Segments are immutable once written — a segment id never
                    // points at different bytes, so let players and any CDN in
                    // front of us keep them forever.
                    (
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".to_owned(),
                    ),
                ],
                bytes,
            )
                .into_response(),
        )
    }

    /// Cache-first read of one segment, falling back to Telegram.
    async fn load_segment(
        state: &Arc<AppState>,
        video: &Video,
        segment_id: Uuid,
        telegram_file_id: &str,
    ) -> XenovraStreamResult<Vec<u8>> {
        let storage_id = video.storage_id;

        state
            .cache
            .get_or_fetch(segment_id, || async move {
                tracing::debug!("[STREAM] cache miss for segment {segment_id}");
                let scheduler =
                    StorageWorkersScheduler::new(&state.db, state.config.telegram_rate_limit);
                let api = TelegramBotApi::new(&state.config.telegram_api_base_url, scheduler);
                api.download(telegram_file_id, storage_id).await
            })
            .await
    }

    /// Warms the next few segments in the background. Failures are ignored on
    /// purpose — this is opportunistic, and the real request will fetch the
    /// segment properly if the guess was wrong or the prefetch lost a race.
    fn spawn_prefetch(state: &Arc<AppState>, video: &Video, rendition_id: Uuid, from: i32) {
        let state = state.clone();
        let video = video.clone();

        tokio::spawn(async move {
            let repo = VideosRepository::new(&state.db);

            for offset in 1..=PREFETCH_AHEAD {
                let next = from + offset;

                let seg = match repo.get_segment(rendition_id, next).await {
                    Ok(s) => s,
                    // Past the end of the video — nothing to warm.
                    Err(_) => return,
                };

                if state.cache.contains(seg.id).await {
                    continue;
                }

                let _ =
                    Self::load_segment(&state, &video, seg.id, &seg.telegram_file_id).await;
            }
        });
    }

    fn playlist_response(body: String) -> axum::response::Response {
        (
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.apple.mpegurl".to_owned(),
                ),
                // A VOD playlist is fixed once the video is ready, but keep this
                // short so a re-transcode or a delete is not masked by a cache.
                (header::CACHE_CONTROL, "public, max-age=60".to_owned()),
            ],
            body,
        )
            .into_response()
    }
}

/// `/s/<public_id>` — the shareable page. Serves the player shell; the video id
/// is read from the URL by the page itself.
pub async fn player_page() -> impl IntoResponse {
    match tokio::fs::read_to_string("ui/player.html").await {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(e) => {
            tracing::error!("cannot read player page: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "player unavailable").into_response()
        }
    }
}

/// Public metadata for the player: title and available renditions, without
/// exposing anything tied to the owning account.
pub async fn public_meta(
    State(state): State<Arc<AppState>>,
    Path(public_id): Path<String>,
) -> impl IntoResponse {
    let service = VideosService::new(&state.db);
    let video = service.get_public(&public_id).await?;

    Ok::<_, (StatusCode, String)>(axum::Json(serde_json::json!({
        "title": video.title,
        "duration_secs": video.duration_secs,
    })))
}
