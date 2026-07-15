use std::sync::Arc;

use sqlx::{Pool, Postgres};

use crate::{config::Config, services::segment_cache::SegmentCache};

#[derive(Clone)]
pub struct AppState {
    pub db: Pool<Postgres>,
    pub config: Config,
    pub cache: Arc<SegmentCache>,
}

impl AppState {
    pub fn new(db: Pool<Postgres>, config: Config, cache: Arc<SegmentCache>) -> Self {
        Self { db, config, cache }
    }
}
