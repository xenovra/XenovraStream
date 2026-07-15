use std::{env, str::FromStr};

use super::errors::{XenovraStreamError, XenovraStreamResult};

#[derive(Debug, Clone)]
pub struct Config {
    pub db_uri: String,
    pub db_uri_without_dbname: String,
    pub db_name: String,
    pub port: u16,
    pub workers: u16,
    pub superuser_email: String,
    pub superuser_pass: String,

    pub access_token_expire_in_secs: u32,
    pub refresh_token_expire_in_days: u16,
    pub secret_key: String,

    pub telegram_api_base_url: String,
    pub telegram_rate_limit: u8,

    /// Root for uploads, ffmpeg scratch space, and the segment cache.
    pub work_dir: String,
    /// Upper bound on the on-disk segment cache, in megabytes. The cache is what
    /// keeps playback viable: without it every seek re-fetches from Telegram and
    /// burns two rate-limited API calls per segment.
    pub cache_max_mb: u64,
    /// HLS segment length in seconds. Also the forced keyframe interval, so that
    /// every segment starts on an IDR frame and renditions stay switchable.
    pub segment_secs: u8,
    /// How many segments are pushed to Telegram concurrently. The per-bot rate
    /// limiter still gates the actual call rate; this just keeps it fed.
    pub upload_concurrency: u8,
    /// x264 preset. `veryfast` is the sane default on a CPU-only box; `medium`
    /// buys ~15% bitrate at roughly 3x the transcode time.
    pub x264_preset: String,
}

impl Config {
    pub fn new() -> XenovraStreamResult<Self> {
        let db_user: String = Self::get_env_var("DATABASE_USER")?;
        let db_password: String = Self::get_env_var("DATABASE_PASSWORD")?;
        let db_name: String = Self::get_env_var("DATABASE_NAME")?;
        let db_host: String = Self::get_env_var("DATABASE_HOST")?;
        let db_port: String = Self::get_env_var("DATABASE_PORT")?;
        let db_uri =
            { format!("postgres://{db_user}:{db_password}@{db_host}:{db_port}/{db_name}") };
        let db_uri_without_dbname =
            { format!("postgres://{db_user}:{db_password}@{db_host}:{db_port}") };
        let port = Self::get_env_var("PORT")?;
        let workers = Self::get_env_var("WORKERS")?;
        let superuser_email = Self::get_env_var("SUPERUSER_EMAIL")?;
        let superuser_pass = Self::get_env_var("SUPERUSER_PASS")?;
        let access_token_expire_in_secs = Self::get_env_var("ACCESS_TOKEN_EXPIRE_IN_SECS")?;
        let refresh_token_expire_in_days = Self::get_env_var("REFRESH_TOKEN_EXPIRE_IN_DAYS")?;
        let secret_key = Self::get_env_var("SECRET_KEY")?;
        let telegram_api_base_url = Self::get_env_var("TELEGRAM_API_BASE_URL")?;
        let telegram_rate_limit = Self::get_env_var_with_default("TELEGRAM_RATE_LIMIT", 18)?;
        let work_dir =
            Self::get_env_var_with_default("WORK_DIR", "/var/lib/xenovrastream".to_owned())?;
        let cache_max_mb = Self::get_env_var_with_default("CACHE_MAX_MB", 4096)?;
        let segment_secs = Self::get_env_var_with_default("SEGMENT_SECS", 6)?;
        let upload_concurrency = Self::get_env_var_with_default("UPLOAD_CONCURRENCY", 4)?;
        let x264_preset = Self::get_env_var_with_default("X264_PRESET", "veryfast".to_owned())?;

        Ok(Self {
            db_uri,
            db_uri_without_dbname,
            db_name,
            port,
            workers,
            superuser_email,
            superuser_pass,
            access_token_expire_in_secs,
            refresh_token_expire_in_days,
            secret_key,
            telegram_api_base_url,
            telegram_rate_limit,
            work_dir,
            cache_max_mb,
            segment_secs,
            upload_concurrency,
            x264_preset,
        })
    }

    /// Where uploaded source files land while they wait for a transcode slot.
    pub fn uploads_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.work_dir).join("uploads")
    }

    /// ffmpeg scratch space; wiped per job once its segments are on Telegram.
    pub fn transcode_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.work_dir).join("transcode")
    }

    /// Segment cache. Safe to delete at any time — it refills from Telegram.
    pub fn cache_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.work_dir).join("cache")
    }

    #[inline]
    fn get_env_var<T: FromStr>(env_var: &str) -> XenovraStreamResult<T> {
        env::var(env_var)
            .map_err(|_| XenovraStreamError::EnvConfigLoadingError(env_var.to_owned()))?
            .parse::<T>()
            .map_err(|_| XenovraStreamError::EnvVarParsingError(env_var.to_owned()))
    }

    #[inline]
    fn get_env_var_with_default<T: FromStr>(env_var: &str, default: T) -> XenovraStreamResult<T> {
        let result = Self::get_env_var(env_var);

        if matches!(result, Err(XenovraStreamError::EnvConfigLoadingError(_))) {
            return Ok(default);
        }

        result
    }
}
