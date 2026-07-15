use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Lifecycle of a video, from the moment its bytes land on disk to the moment
/// it is playable. Stored as the `video_status` postgres enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "video_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum VideoStatus {
    /// Source file is on disk, waiting for a transcode slot.
    Queued,
    /// ffmpeg is producing HLS renditions.
    Transcoding,
    /// Segments are being pushed to Telegram.
    Uploading,
    /// Playable.
    Ready,
    /// Gave up; `error` holds the reason.
    Failed,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Video {
    pub id: Uuid,
    /// The unguessable slug used in the public `/s/<public_id>` link.
    pub public_id: String,
    pub title: String,
    pub original_filename: String,
    pub size: i64,
    pub duration_secs: Option<f64>,
    pub storage_id: Uuid,
    pub user_id: Uuid,
    pub status: VideoStatus,
    /// 0-100, meaningful while `Transcoding`/`Uploading`.
    pub progress: i16,
    pub error: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

pub struct InVideo {
    pub public_id: String,
    pub title: String,
    pub original_filename: String,
    pub size: i64,
    pub storage_id: Uuid,
    pub user_id: Uuid,
}

/// One quality level of a video. Rows only exist for renditions we actually
/// produced — we never upscale, so a 480p source yields only `360p`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Rendition {
    pub id: Uuid,
    pub video_id: Uuid,
    /// `360p` / `720p` / `1080p` — also the path segment in the playlist URL.
    pub name: String,
    pub width: i32,
    pub height: i32,
    /// Advertised bits/sec in the master playlist (video + audio).
    pub bandwidth: i32,
    /// RFC 6381 codec string, e.g. `avc1.4d401e,mp4a.40.2`.
    pub codecs: String,
    /// Longest segment, rounded up — becomes `#EXT-X-TARGETDURATION`.
    pub target_duration: i32,
}

pub struct InRendition {
    pub video_id: Uuid,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub bandwidth: i32,
    pub codecs: String,
    pub target_duration: i32,
}

/// A single `.ts` segment, living as one Telegram message.
#[derive(Debug, Clone, FromRow)]
pub struct Segment {
    pub id: Uuid,
    pub rendition_id: Uuid,
    pub position: i32,
    pub duration: f64,
    pub size: i32,
    pub telegram_file_id: String,
    pub telegram_message_id: i64,
}

pub struct InSegment {
    pub rendition_id: Uuid,
    pub position: i32,
    pub duration: f64,
    pub size: i32,
    pub telegram_file_id: String,
    pub telegram_message_id: i64,
}
