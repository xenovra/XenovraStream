use serde::Deserialize;

#[derive(Deserialize)]
pub struct UploadBodySchema {
    pub result: UploadResultSchema,
}

#[derive(Deserialize)]
pub struct UploadResultSchema {
    pub message_id: i64,
    pub document: UploadSchema,
}

#[derive(Deserialize)]
pub struct UploadSchema {
    pub file_id: String,
}

/// Result of uploading a single chunk to Telegram: the document `file_id`
/// (used to download it later) and the `message_id` (used to delete it).
pub struct UploadedChunk {
    pub file_id: String,
    pub message_id: i64,
}

#[derive(Deserialize)]
pub struct DownloadBodySchema {
    pub result: DownloadSchema,
}

#[derive(Deserialize)]
pub struct DownloadSchema {
    pub file_path: String,
}
