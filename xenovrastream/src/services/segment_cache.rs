use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::errors::XenovraStreamResult;

/// On-disk cache of `.ts` segments fetched back from Telegram.
///
/// This is load-bearing, not an optimisation. Fetching one segment costs two
/// rate-limited Telegram calls (`getFile`, then the download), and a bot token
/// only allows `TELEGRAM_RATE_LIMIT` calls per minute. Uncached, a single
/// viewer watching a 6-second-segment stream would spend ~20 calls/min and
/// stall; a second viewer of the same video would double it. Cached, a segment
/// costs Telegram exactly once no matter how many people watch.
pub struct SegmentCache {
    dir: PathBuf,
    max_bytes: u64,
    /// Per-segment locks, so N simultaneous viewers hitting the same cold
    /// segment produce one Telegram fetch rather than N.
    inflight: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
}

impl SegmentCache {
    pub fn new(dir: PathBuf, max_mb: u64) -> Self {
        Self {
            dir,
            max_bytes: max_mb * 1024 * 1024,
            inflight: Mutex::new(HashMap::new()),
        }
    }

    pub async fn init(&self) -> XenovraStreamResult<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        Ok(())
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.ts"))
    }

    /// Returns the cached segment, fetching it via `fetch` on a miss.
    pub async fn get_or_fetch<F, Fut>(&self, id: Uuid, fetch: F) -> XenovraStreamResult<Vec<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = XenovraStreamResult<Vec<u8>>>,
    {
        if let Some(bytes) = self.read(id).await {
            return Ok(bytes);
        }

        // Take this segment's lock before fetching. Whoever loses the race waits
        // here and then finds the winner's bytes already on disk.
        let lock = {
            let mut inflight = self.inflight.lock().await;
            inflight.entry(id).or_default().clone()
        };
        let _guard = lock.lock().await;

        if let Some(bytes) = self.read(id).await {
            self.release(id).await;
            return Ok(bytes);
        }

        let result = fetch().await;

        if let Ok(bytes) = &result {
            self.write(id, bytes).await;
        }

        self.release(id).await;
        result
    }

    /// Whether a segment is already cached — lets prefetch skip work cheaply.
    pub async fn contains(&self, id: Uuid) -> bool {
        tokio::fs::metadata(self.path_for(id)).await.is_ok()
    }

    async fn read(&self, id: Uuid) -> Option<Vec<u8>> {
        tokio::fs::read(self.path_for(id)).await.ok()
    }

    async fn write(&self, id: Uuid, bytes: &[u8]) {
        // Write to a temp name and rename, so a crash mid-write can never leave
        // a truncated segment that would later be served as if it were whole.
        let final_path = self.path_for(id);
        let tmp_path = self.dir.join(format!("{id}.tmp"));

        if let Err(e) = tokio::fs::write(&tmp_path, bytes).await {
            tracing::warn!("[CACHE] cannot write segment {id}: {e}");
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &final_path).await {
            tracing::warn!("[CACHE] cannot commit segment {id}: {e}");
            let _ = tokio::fs::remove_file(&tmp_path).await;
        }
    }

    async fn release(&self, id: Uuid) {
        let mut inflight = self.inflight.lock().await;
        // Only drop the entry if we hold the last reference; otherwise a waiter
        // still needs it.
        if let Some(lock) = inflight.get(&id) {
            if Arc::strong_count(lock) == 1 {
                inflight.remove(&id);
            }
        }
    }

    /// Trims the cache back under its size cap, oldest file first.
    ///
    /// Eviction is by write time, not access time: for VOD the two orders
    /// mostly agree (people watch forwards), and relatime mounts make atime an
    /// unreliable signal anyway.
    pub async fn evict(&self) -> XenovraStreamResult<()> {
        let mut entries = Vec::new();
        let mut total: u64 = 0;

        let mut dir = match tokio::fs::read_dir(&self.dir).await {
            Ok(d) => d,
            Err(_) => return Ok(()),
        };
        while let Some(entry) = dir.next_entry().await? {
            let meta = match entry.metadata().await {
                Ok(m) if m.is_file() => m,
                _ => continue,
            };
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            total += meta.len();
            entries.push((entry.path(), meta.len(), modified));
        }

        if total <= self.max_bytes {
            return Ok(());
        }

        entries.sort_by_key(|(_, _, modified)| *modified);

        let mut freed = 0u64;
        for (path, len, _) in entries {
            if total - freed <= self.max_bytes {
                break;
            }
            if tokio::fs::remove_file(&path).await.is_ok() {
                freed += len;
            }
        }

        tracing::debug!(
            "[CACHE] evicted {} MB, now at {} MB",
            freed / 1024 / 1024,
            (total - freed) / 1024 / 1024
        );
        Ok(())
    }
}
