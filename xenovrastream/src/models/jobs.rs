use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_state", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Done,
    Failed,
}

/// A unit of transcode work. Unlike the drive's in-memory channel, this is a
/// real row: a crash mid-transcode leaves the job claimable again rather than
/// silently losing the upload.
#[derive(Debug, Clone, FromRow)]
pub struct TranscodeJob {
    pub id: Uuid,
    pub video_id: Uuid,
    /// Absolute path of the uploaded source file on local disk.
    pub source_path: String,
    pub state: JobState,
    pub attempts: i16,
    pub last_error: Option<String>,
}
