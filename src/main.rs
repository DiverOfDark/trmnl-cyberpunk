mod data;
mod render;

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};

use data::DashData;
use render::Fonts;

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    fonts: Arc<Fonts>,
    bmp_cache: Arc<RwLock<Vec<u8>>>,
    png_cache: Arc<RwLock<Vec<u8>>>,
    data: Arc<RwLock<DashData>>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Last 6 hex digits of a MAC address, no colons — e.g. "AABBCC".
fn mac_short_id(mac: &str) -> String {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    hex[hex.len().saturating_sub(6)..].to_uppercase()
}

fn base_url() -> String {
    std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

fn refresh_secs() -> u32 {
    std::env::var("REFRESH_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600)
}

fn build_display_response() -> DisplayResponse {
    let url = format!("{}/dashboard.png", base_url());
    DisplayResponse::new(&url, "dashboard.png").with_refresh_rate(refresh_secs())
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Called once on first boot — device exchanges its MAC for an api_key.
async fn api_setup(State(_): State<AppState>, device: DeviceInfo) -> impl IntoResponse {
    info!(
        mac  = %device.mac_address,
        fw   = ?device.firmware_version,
        bat  = ?device.battery_voltage,
        "device setup"
    );

    let api_key = std::env::var("TRMNL_API_KEY").unwrap_or_else(|_| "seedbox-byos".into());

    Json(json!({
        "api_key":     api_key,
        "friendly_id": mac_short_id(&device.mac_address),
        "image_url":   format!("{}/dashboard.png", base_url()),
        "message":     "TRMNL-SEEDBOX // BYOS // NEO-TOKYO",
    }))
}

/// Polled every `refresh_rate` seconds — returns the current image URL.
async fn api_display(State(_): State<AppState>, device: DeviceInfo) -> Json<DisplayResponse> {
    info!(
        mac  = %device.mac_address,
        bat  = ?device.battery_percentage(),
        rssi = ?device.rssi,
        "device poll"
    );
    Json(build_display_response())
}

/// Device POSTs diagnostic logs here; we acknowledge with 204.
async fn api_log(State(_): State<AppState>, device: DeviceInfo) -> StatusCode {
    warn!(mac = %device.mac_address, "device log received");
    StatusCode::NO_CONTENT
}

/// Serve the rendered BMP — what the device actually downloads and displays.
async fn serve_bmp(State(state): State<AppState>) -> Response {
    let bytes = state.bmp_cache.read().await.clone();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/bmp"), (header::CACHE_CONTROL, "no-cache")],
        bytes,
    )
        .into_response()
}

/// PNG variant — for browser preview and debugging.
async fn serve_png(State(state): State<AppState>) -> Response {
    let bytes = state.png_cache.read().await.clone();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-cache")],
        bytes,
    )
        .into_response()
}

/// Force an immediate re-render (e.g. from a home-automation webhook).
async fn refresh(State(state): State<AppState>) -> impl IntoResponse {
    regenerate(&state).await;
    Json(json!({ "status": "ok", "message": "dashboard regenerated" }))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ── Render ────────────────────────────────────────────────────────────────────

async fn regenerate(state: &AppState) {
    let data  = state.data.read().await.clone();
    let fonts = state.fonts.clone();

    let (bmp, png) = tokio::task::spawn_blocking(move || {
        (render::render_bmp(&data, &fonts), render::render_png(&data, &fonts))
    })
    .await
    .expect("render task panicked");

    *state.bmp_cache.write().await = bmp;
    *state.png_cache.write().await = png;
    info!("dashboard regenerated");
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "trmnl_seedbox=info,tower_http=warn".into()),
        )
        .init();

    let font_dir = std::env::var("FONT_DIR").unwrap_or_else(|_| "fonts".to_string());
    info!("Loading fonts from {font_dir}");
    let fonts = Arc::new(
        Fonts::load(&font_dir).expect("Failed to load fonts — run ./setup-fonts.sh first"),
    );

    let data  = Arc::new(RwLock::new(DashData::mock()));
    let state = AppState {
        fonts,
        bmp_cache: Arc::new(RwLock::new(Vec::new())),
        png_cache: Arc::new(RwLock::new(Vec::new())),
        data,
    };

    regenerate(&state).await;

    // Re-render on the same cadence as the device refresh so data stays fresh.
    let bg = state.clone();
    tokio::spawn(async move {
        let secs = refresh_secs() as u64;
        let mut ticker = tokio::time::interval(Duration::from_secs(secs));
        ticker.tick().await; // first tick fires immediately — skip it
        loop {
            ticker.tick().await;
            bg.data.write().await.clone_from(&DashData::mock());
            regenerate(&bg).await;
        }
    });

    let app = Router::new()
        .route("/api/setup",     get(api_setup))
        .route("/api/display",   get(api_display))
        .route("/api/log",       post(api_log))
        .route("/dashboard.bmp", get(serve_bmp))
        .route("/dashboard.png", get(serve_png))
        .route("/refresh",       get(refresh))
        .route("/health",        get(health))
        .with_state(state);

    let addr = std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    info!("Listening on http://{addr}");
    info!("TRMNL  →  setup:   http://{addr}/api/setup");
    info!("TRMNL  →  display: http://{addr}/api/display");
    info!("Preview  →         http://{addr}/dashboard.png");

    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}
