use std::{net::SocketAddr, sync::Arc};

use axum::{routing::get, Router};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{
    cors,
    services::{ServeDir, ServeFile},
};

use crate::{
    common::routing::app_state::AppState,
    routers::{
        auth::AuthRouter,
        storage_workers::StorageWorkersRouter,
        storages::StoragesRouter,
        stream::{self, StreamRouter},
        users::UsersRouter,
        videos::VideosRouter,
    },
};

pub struct Server {
    router: Router,
}

impl Server {
    pub fn build_server(workers: usize, app_state: Arc<AppState>) -> Self {
        let serve_ui = ServeFile::new("ui/index.html");
        let serve_assets = ServeDir::new("ui/assets");

        let router = Router::new()
            .nest("/api", Self::build_api_router(workers, app_state.clone()))
            // The shareable link. Kept short and outside /api because it is the
            // thing people actually paste around.
            .route("/s/:public_id", get(stream::player_page))
            .nest_service("/assets", serve_assets)
            .fallback_service(serve_ui);

        Self { router }
    }

    #[inline]
    fn build_api_router(workers: usize, app_state: Arc<AppState>) -> Router {
        let app_cors = cors::CorsLayer::new()
            .allow_methods(cors::Any)
            .allow_headers(cors::Any)
            .allow_origin(cors::Any);

        // Management endpoints: short request/response calls, safe to cap.
        let managed = Router::new()
            .nest("/users", UsersRouter::get_router(app_state.clone()))
            .nest("/auth", AuthRouter::get_router(app_state.clone()))
            .nest("/storages", StoragesRouter::get_router(app_state.clone()))
            .nest(
                "/storage_workers",
                StorageWorkersRouter::get_router(app_state.clone()),
            )
            .nest("/videos", VideosRouter::get_router(app_state.clone()))
            .layer(ConcurrencyLimitLayer::new(workers));

        // Playback is deliberately outside that cap. One viewer holds several
        // in-flight segment requests, and a segment can block for seconds
        // waiting on a Telegram rate-limit slot — a `workers`-sized cap would
        // let a handful of viewers stall every other request in the app.
        //
        // It is also outside the auth layer: anyone holding the unguessable
        // link must be able to play without an account.
        let public = Router::new()
            .nest("/stream", StreamRouter::get_router(app_state.clone()))
            .route(
                "/public/:public_id",
                get(stream::public_meta).with_state(app_state),
            );

        managed.merge(public).layer(app_cors)
    }

    pub async fn run(self, addr: &SocketAddr) {
        tracing::info!("listening on http://{addr}");
        axum::Server::bind(addr)
            .serve(self.router.into_make_service())
            .await
            .unwrap();
    }
}
