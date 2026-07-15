use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    common::{
        jwt_manager::AuthUser,
        routing::{app_state::AppState, middlewares::auth::logged_in_required},
    },
    errors::XenovraStreamError,
    models::videos::Video,
    services::videos::VideosService,
};

pub struct VideosRouter;

impl VideosRouter {
    pub fn get_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/", get(Self::list))
            .route("/:video_id", get(Self::get).delete(Self::delete))
            .route("/upload/:storage_id", post(Self::upload))
            // Videos are large by nature; the guard against a runaway upload is
            // disk space, not a body limit, since we never buffer in memory.
            .layer(DefaultBodyLimit::disable())
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                logged_in_required,
            ))
            .with_state(state)
    }

    /// Streams the multipart body straight to disk, then queues a transcode.
    ///
    /// Returns as soon as the bytes are safe on disk — the response carries the
    /// video id and public link, and the client polls status from there.
    /// Transcoding a feature-length file takes tens of minutes; holding the
    /// connection open for it was never an option.
    async fn upload(
        State(state): State<Arc<AppState>>,
        Extension(user): Extension<AuthUser>,
        Path(storage_id): Path<Uuid>,
        mut multipart: Multipart,
    ) -> Result<(StatusCode, Json<Video>), (StatusCode, String)> {
        let upload_id = Uuid::new_v4();
        let uploads_dir = state.config.uploads_dir();
        tokio::fs::create_dir_all(&uploads_dir)
            .await
            .map_err(XenovraStreamError::from)?;

        let source_path = uploads_dir.join(upload_id.to_string());
        let mut original_filename = String::new();
        let mut title = String::new();
        let mut size: i64 = 0;
        let mut got_file = false;

        while let Some(mut field) = multipart.next_field().await.map_err(|e| {
            tracing::error!("[UPLOAD] malformed multipart: {e}");
            XenovraStreamError::EmptyUpload
        })? {
            match field.name() {
                Some("title") => {
                    title = field.text().await.unwrap_or_default();
                }
                Some("file") => {
                    original_filename =
                        field.file_name().unwrap_or("video.mp4").to_owned();

                    let mut file = tokio::fs::File::create(&source_path)
                        .await
                        .map_err(XenovraStreamError::from)?;

                    // The whole point: chunk-by-chunk, so peak RAM is one chunk
                    // rather than the entire file.
                    while let Some(chunk) = field.chunk().await.map_err(|e| {
                        tracing::error!("[UPLOAD] stream broke: {e}");
                        XenovraStreamError::EmptyUpload
                    })? {
                        size += chunk.len() as i64;
                        file.write_all(&chunk)
                            .await
                            .map_err(XenovraStreamError::from)?;
                    }
                    file.flush().await.map_err(XenovraStreamError::from)?;
                    got_file = true;
                }
                _ => continue,
            }
        }

        if !got_file || size == 0 {
            let _ = tokio::fs::remove_file(&source_path).await;
            return Err(XenovraStreamError::EmptyUpload.into());
        }

        if title.trim().is_empty() {
            title = original_filename.clone();
        }
        title.truncate(255);
        original_filename.truncate(255);

        let result = VideosService::new(&state.db)
            .register_upload(
                title,
                original_filename,
                size,
                &source_path.to_string_lossy(),
                storage_id,
                &user,
            )
            .await;

        match result {
            Ok(video) => Ok((StatusCode::ACCEPTED, Json(video))),
            Err(e) => {
                // Nothing will ever pick this file up, so do not leave it behind.
                let _ = tokio::fs::remove_file(&source_path).await;
                Err(e.into())
            }
        }
    }

    async fn list(
        State(state): State<Arc<AppState>>,
        Extension(user): Extension<AuthUser>,
    ) -> impl IntoResponse {
        let videos = VideosService::new(&state.db).list(&user).await?;
        Ok::<_, (StatusCode, String)>(Json(videos))
    }

    async fn get(
        State(state): State<Arc<AppState>>,
        Extension(user): Extension<AuthUser>,
        Path(video_id): Path<Uuid>,
    ) -> impl IntoResponse {
        let video = VideosService::new(&state.db).get(video_id, &user).await?;
        Ok::<_, (StatusCode, String)>(Json(video))
    }

    async fn delete(
        State(state): State<Arc<AppState>>,
        Extension(user): Extension<AuthUser>,
        Path(video_id): Path<Uuid>,
    ) -> Result<StatusCode, (StatusCode, String)> {
        VideosService::new(&state.db)
            .delete(video_id, &state.config, &user)
            .await?;
        Ok(StatusCode::NO_CONTENT)
    }
}
