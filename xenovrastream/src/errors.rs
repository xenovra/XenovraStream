use axum::http::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XenovraStreamError {
    #[error("environment variable `{0}` is not set")]
    EnvConfigLoadingError(String),
    #[error("environment variable `{0}` cannot be parsed")]
    EnvVarParsingError(String),

    #[error("user was removed")]
    UserWasRemoved,

    #[error("{0} already exists")]
    AlreadyExists(String),
    #[error("{0} does not exist")]
    DoesNotExist(String),
    #[error("User already has a storage with such name")]
    StorageNameConflict,
    #[error("User already has a storage with such chat id")]
    StorageChatIdConflict,
    #[error("User already has a storage worker with such name")]
    StorageWorkerNameConflict,
    #[error("Token must be unique")]
    StorageWorkerTokenConflict,
    #[error("not authenticated")]
    NotAuthenticated,
    #[error("[Telegram API] {0}")]
    TelegramAPIError(String),
    #[error("You need to add at least 1 storage worker")]
    NoStorageWorkers,
    #[error("Invalid path")]
    InvalidPath,
    #[error("You cannot manage access of yourself")]
    CannotManageAccessOfYourself,
    #[error("Storage does not have workers")]
    StorageDoesNotHaveWorkers,

    #[error("the uploaded file has no playable video track")]
    NotAVideo,
    #[error("no file was uploaded")]
    EmptyUpload,
    #[error("this video is not ready for playback yet")]
    VideoNotReady,
    #[error("[ffmpeg] {0}")]
    FfmpegError(String),
    #[error("io error: {0}")]
    IoError(String),

    #[error("unknown error")]
    Unknown,
    #[error("{0} header is required")]
    HeaderMissed(String),
    #[error("{0} header should be a valid {1}")]
    HeaderIsInvalid(String, String),
}

impl From<XenovraStreamError> for (StatusCode, String) {
    fn from(e: XenovraStreamError) -> Self {
        match &e {
            XenovraStreamError::AlreadyExists(_)
            | XenovraStreamError::StorageNameConflict
            | XenovraStreamError::StorageChatIdConflict
            | XenovraStreamError::StorageWorkerNameConflict
            | XenovraStreamError::StorageWorkerTokenConflict
            | XenovraStreamError::StorageDoesNotHaveWorkers
            // Uploading before adding a bot is the most likely first-run
            // mistake; it fell through to a 500 "Something went wrong", which
            // told the user nothing about the one thing they had to do.
            | XenovraStreamError::NoStorageWorkers
            | XenovraStreamError::CannotManageAccessOfYourself => (StatusCode::CONFLICT, e.to_string()),
            XenovraStreamError::NotAuthenticated => (StatusCode::UNAUTHORIZED, e.to_string()),
            XenovraStreamError::DoesNotExist(_) => (StatusCode::NOT_FOUND, e.to_string()),
            XenovraStreamError::HeaderMissed(_)
            | XenovraStreamError::HeaderIsInvalid(..)
            | XenovraStreamError::NotAVideo
            | XenovraStreamError::EmptyUpload
            | XenovraStreamError::InvalidPath => (StatusCode::BAD_REQUEST, e.to_string()),
            // Not an error the client can fix by retrying differently — the
            // transcode simply hasn't finished.
            XenovraStreamError::VideoNotReady => (StatusCode::CONFLICT, e.to_string()),
            _ => {
                tracing::error!("{e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong".to_owned(),
                )
            }
        }
    }
}

impl From<reqwest::Error> for XenovraStreamError {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(e) if e.is_client_error() => XenovraStreamError::TelegramAPIError(e.to_string()),
            Some(_) | None => {
                tracing::error!("{e}");
                XenovraStreamError::Unknown
            }
        }
    }
}

impl From<std::io::Error> for XenovraStreamError {
    fn from(e: std::io::Error) -> Self {
        tracing::error!("{e}");
        XenovraStreamError::IoError(e.to_string())
    }
}

pub type XenovraStreamResult<T> = Result<T, XenovraStreamError>;
