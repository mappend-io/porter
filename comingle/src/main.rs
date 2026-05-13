use crate::app_state::AppState;
use crate::config::Config;
use crate::handlers::*;
use crate::middleware::*;
use anyhow::{Context, Result};
use axum::middleware::from_fn;
use axum::{
    Router,
    http::HeaderValue,
    routing::{get, post},
};
use moka::future::Cache;
use resource_io::{ResourceLoader, ResourceLoaderConfig};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

pub mod app_state;
pub mod config;
pub mod emulation;
pub mod handlers;
pub mod layer_definition;
pub mod logging;
pub mod middleware;
pub mod s2_utils;
pub mod tiles3d;
pub mod utils;

#[cfg(feature = "embedded-viewer")]
pub mod viewer;

#[cfg(feature = "embedded-viewer")]
pub mod terrarium_viewer;

#[tokio::main]
async fn main() -> Result<()> {
    // Slurp from .env
    dotenvy::dotenv().ok();

    // Load config
    let config = Config::load();

    // Setup logging
    logging::setup_logging(&config.log_level, config.pretty_log);

    // Using moka here to get ttl
    let layer_definition_cache = Cache::builder()
        .time_to_live(config.layer_definition_ttl)
        .max_capacity(1_024) // TODO: from config
        .build();

    let resource_loader_config = ResourceLoaderConfig {
        block_cache_bytes: config.block_cache_size.as_u64(),
        ..Default::default()
    };

    let app_state = AppState {
        config: config.clone(),
        resource_loader: ResourceLoader::new(resource_loader_config).await,
        layer_definition_cache: Arc::new(layer_definition_cache),
    };

    let compat_routes = Router::new()
        .route("/appData", get(emulation::app_data))
        .route("/oauth", get(emulation::oauth))
        .route("/oauth/token", post(emulation::oauth_token))
        .route("/v2/tokens", get(emulation::list_tokens))
        .route("/v1/defaults", get(emulation::get_defaults))
        .route("/v1/me", get(emulation::me))
        .route("/v1/assets", get(emulation::list_assets))
        .route("/v1/assets/{id}", get(emulation::get_asset))
        .route(
            "/v1/assets/{id}/endpoint",
            get(emulation::get_asset_endpoint),
        );

    let short_cache_routes = Router::new()
        .route("/{id}", get(get_root_tileset))
        .route("/layers", get(get_layers))
        .route_layer(from_fn(cache_short));

    let long_cache_routes = Router::new()
        .route("/{id}/{hash}/tileset.json", get(get_root_tileset_top_node))
        .route(
            "/{id}/{hash}/t/{face}/{level}/{col}/{row}",
            get(get_child_tileset),
        )
        .route(
            "/{id}/{hash}/c/{token}/tileset.json",
            get(get_content_toplevel),
        )
        .route("/{id}/{hash}/c/{token}/{*rest}", get(get_content_payload))
        .route(
            "/{id}/{hash}/bgc/{*rest}",
            get(get_base_globe_terrain_payload),
        )
        .route("/terrarium/{id}/{zoom}/{x}/{y}", get(get_terrarium_tile))
        .route_layer(from_fn(cache_forever));

    let app = Router::new()
        .merge(compat_routes)
        .merge(short_cache_routes)
        .merge(long_cache_routes);

    #[cfg(feature = "embedded-viewer")]
    let app = app.fallback(viewer::static_handler);

    #[cfg(feature = "embedded-viewer")]
    let app = app
        .route(
            "/terrarium_viewer/{id}",
            get(terrarium_viewer::index_handler),
        )
        .route(
            "/terrarium_viewer/assets/{*path}",
            get(terrarium_viewer::asset_handler),
        );

    let app = app.with_state(app_state.clone());

    let app = match config.cors_origin.as_deref() {
        None => app,
        Some("*") => app.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        ),
        Some(origin) => app.layer(
            CorsLayer::new()
                .allow_origin(origin.parse::<HeaderValue>().context("Bad CORS origin")?)
                .allow_methods(Any)
                .allow_headers(Any),
        ),
    };

    let app = app
        .layer(from_fn(security_headers))
        .layer(TraceLayer::new_for_http());

    info!("🚀 Listening on {}", app_state.config.listen_addr);

    let listener = TcpListener::bind(config.listen_addr).await?;
    let _ = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    tracing::info!("Shutting down...");
}
