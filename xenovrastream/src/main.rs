use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tokio::time;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    common::{db::pool::get_pool, routing::app_state::AppState},
    config::Config,
    repositories::videos::VideosRepository,
    server::Server,
    services::{segment_cache::SegmentCache, transcoder::Transcoder},
    startup::{create_db, create_superuser, init_db},
};

mod common;
mod config;
mod errors;
mod models;
mod repositories;
mod routers;
mod schemas;
mod server;
mod services;
mod startup;

/// How often the segment cache is trimmed back under its cap.
const CACHE_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

#[tokio::main]
async fn main() {
    let config = Config::new().unwrap();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "xenovrastream=debug,tower_http=debug,axum::rejection=trace".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    create_db(
        &config.db_uri_without_dbname,
        &config.db_name,
        config.workers.into(),
        time::Duration::from_secs(30),
    )
    .await;

    let db = get_pool(
        &config.db_uri,
        config.workers.into(),
        time::Duration::from_secs(30),
    )
    .await;

    init_db(&db).await;
    create_superuser(&db, &config).await;

    for dir in [
        config.uploads_dir(),
        config.transcode_dir(),
        config.cache_dir(),
    ] {
        tokio::fs::create_dir_all(&dir)
            .await
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
    }

    // A job left `running` means the process died mid-transcode: a claim only
    // ever lives in memory, so nothing can still be working on it.
    match VideosRepository::new(&db).requeue_orphaned_jobs().await {
        Ok(0) => (),
        Ok(n) => tracing::warn!("requeued {n} job(s) orphaned by a restart"),
        Err(e) => tracing::error!("cannot requeue orphaned jobs: {e}"),
    }

    let cache = Arc::new(SegmentCache::new(config.cache_dir(), config.cache_max_mb));
    cache.init().await.unwrap();

    // Transcode worker.
    {
        let config = config.clone();
        let db = db.clone();
        tokio::spawn(async move {
            tracing::debug!("running transcoder");
            Transcoder::new(db, config).run().await;
        });
    }

    // Cache sweeper.
    {
        let cache = cache.clone();
        tokio::spawn(async move {
            loop {
                time::sleep(CACHE_SWEEP_INTERVAL).await;
                if let Err(e) = cache.evict().await {
                    tracing::error!("cache eviction failed: {e}");
                }
            }
        });
    }

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), config.port);

    let server = {
        let workers = config.workers;
        let app_state = AppState::new(db, config, cache);
        Server::build_server(workers.into(), Arc::new(app_state))
    };

    server.run(&addr).await
}
