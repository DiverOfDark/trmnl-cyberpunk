mod dashboard;
mod data;
mod fetch;
mod render;
mod windows_tz;

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};

use data::DashData;
use fetch::Sources;

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Default, Clone, Serialize)]
struct DeviceState {
    battery_pct: u8,
    rssi: i32,
    firmware: String,
    last_seen: String,
}

#[derive(Clone)]
struct AppState {
    device_state: Arc<RwLock<DeviceState>>,
    data: Arc<RwLock<DashData>>,
    sources: Arc<Sources>,
    /// Serializes upstream-fetch runs. Every `/dashboard*` request triggers
    /// a fetch, so without this a burst of requests would fan out into N
    /// parallel upstream pulls.
    fetch_lock: Arc<tokio::sync::Mutex<()>>,
    /// Unix epoch (seconds) of the last successful render. Bumped on every
    /// PNG render; embedded into `/api/display` URLs as a cache-buster
    /// (`dashboard-{epoch}.png`) so the firmware skips its 24h dedupe and
    /// re-fetches when the image has changed.
    last_render_at: Arc<RwLock<i64>>,
    local_mode: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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

/// Filename the firmware uses as its 24h dedupe cache key. It expects a
/// 10-digit Unix epoch suffix (`dashboard-1778185416.png`) so the value
/// must change whenever the rendered bytes change.
fn dashboard_filename(epoch: i64) -> String {
    if epoch > 0 {
        format!("dashboard-{epoch}.png")
    } else {
        // Pre-first-render fallback.
        "dashboard.png".to_string()
    }
}

/// URL the firmware downloads. Goes through `/dashboard/{epoch}` rather than
/// `/dashboard-{epoch}.png` because Swagger UI is mounted at `/` and its
/// wildcard catch-all would otherwise swallow any single-segment request
/// (and reply 404 because Swagger has no such asset). Slash-separated
/// segments don't conflict with the UI's wildcard.
fn dashboard_url(epoch: i64) -> String {
    if epoch > 0 {
        format!("{}/dashboard/{epoch}", base_url())
    } else {
        format!("{}/dashboard.png", base_url())
    }
}

fn build_display_response(epoch: i64) -> DisplayResponse {
    DisplayResponse::new(dashboard_url(epoch), dashboard_filename(epoch))
        .with_refresh_rate(refresh_secs())
}

// ── Data refresh ──────────────────────────────────────────────────────────────

/// Pull from every configured upstream and stash the result in `state.data`.
/// Concurrent runs serialize via `fetch_lock` — callers always observe fresh
/// data after this returns. No rendering happens here; renders are per-
/// request in `serve_png` / `render_now`.
async fn refresh_data(state: &AppState) {
    let _guard = state.fetch_lock.lock().await;

    let mock = DashData::mock();
    let fresh = if state.local_mode {
        mock
    } else {
        state.sources.fetch(&mock).await
    };
    *state.data.write().await = fresh;
    info!("data refreshed");
}

/// Render the current `state.data` to a PNG. Called per-request from
/// `serve_png`, and once at the end of `RENDER_TO=...` mode. Bumps
/// `last_render_at` on success so the `/api/display` URL is cache-busted.
async fn render_now(state: &AppState) -> anyhow::Result<Vec<u8>> {
    let mut data = state.data.read().await.clone();
    data.refresh_clock();
    let device = state.device_state.read().await.clone();

    let bytes = tokio::task::spawn_blocking(move || {
        dashboard::render(&data, device.battery_pct, device.rssi)
    })
    .await??;

    *state.last_render_at.write().await = chrono::Utc::now().timestamp();
    Ok(bytes)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn api_setup(State(state): State<AppState>, device: DeviceInfo) -> impl IntoResponse {
    info!(mac = %device.mac_address, fw = ?device.firmware_version, "device setup");
    let api_key = std::env::var("TRMNL_API_KEY").unwrap_or_else(|_| "cyberpunk-byos".into());
    let epoch = *state.last_render_at.read().await;
    Json(json!({
        "api_key":     api_key,
        "friendly_id": mac_short_id(&device.mac_address),
        "image_url":   dashboard_url(epoch),
        "message":     "TRMNL//CYBERPUNK — BYOS",
    }))
}

async fn api_display(State(state): State<AppState>, device: DeviceInfo) -> Json<DisplayResponse> {
    info!(mac = %device.mac_address, bat = ?device.battery_percentage(), rssi = ?device.rssi, "device poll");

    {
        let mut ds = state.device_state.write().await;
        ds.battery_pct = device.battery_percentage().unwrap_or(0);
        ds.rssi = device.rssi.unwrap_or(0);
        ds.firmware = device.firmware_version.clone().unwrap_or_default();
        ds.last_seen = chrono::Local::now().format("%H:%M").to_string();
    }

    // No fetch here — the firmware will hit `/dashboard/{epoch}` next, and
    // `serve_png` refreshes upstream data before rendering.
    let epoch = *state.last_render_at.read().await;
    Json(build_display_response(epoch))
}

async fn api_log(State(_): State<AppState>, device: DeviceInfo, body: String) -> StatusCode {
    // Try to pretty-print as JSON (matches TRMNL spec body shape) and fall
    // back to the raw bytes if the device sent something else, so a malformed
    // log payload still surfaces in our output instead of getting dropped.
    let pretty = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| body.clone());
    info!(mac = %device.mac_address, "device log:\n{pretty}");
    StatusCode::NO_CONTENT
}

async fn serve_png(State(state): State<AppState>) -> Response {
    // Every request → fresh upstream pull → fresh render. Matches the user
    // expectation that pressing the device's refresh button gets new data.
    refresh_data(&state).await;
    match render_now(&state).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-cache")],
            bytes,
        )
            .into_response(),
        Err(e) => {
            warn!("render failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "render failed").into_response()
        }
    }
}

async fn force_refresh(State(state): State<AppState>) -> impl IntoResponse {
    refresh_data(&state).await;
    Json(json!({ "status": "ok" }))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "trmnl_cyberpunk=info,tower_http=warn".into()),
        )
        .init();

    // RENDER_TO=path.png → render a single PNG with mock data, write it, exit.
    // LOCAL_MODE=1       → serve normally but never fetch real data (mock only).
    let render_to = std::env::var("RENDER_TO").ok();
    let local_mode = render_to.is_some() || std::env::var("LOCAL_MODE").is_ok();

    // In RENDER_TO mode, bind to localhost on a random port — the rest of
    // the server is incidental; we just want a place to live until the
    // single render finishes, then exit.
    let addr = if render_to.is_some() {
        "127.0.0.1:0".to_string()
    } else {
        std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string())
    };
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    let bound = listener.local_addr().unwrap();

    // Initial DashData uses mock so the layout has content while
    // integrations boot up; the first background fetch overwrites it.
    let initial_data = DashData::mock();

    let state = AppState {
        device_state:   Arc::new(RwLock::new(DeviceState::default())),
        data:           Arc::new(RwLock::new(initial_data)),
        sources:        Arc::new(Sources::from_env()),
        fetch_lock:     Arc::new(tokio::sync::Mutex::new(())),
        last_render_at: Arc::new(RwLock::new(0)),
        local_mode,
    };

    let app = Router::new()
        .route("/api/setup",      get(api_setup))
        .route("/api/display",    get(api_display))
        .route("/api/log",        post(api_log))
        .route("/dashboard.png",  get(serve_png))
        // Cache-bustered URL for the firmware. `{epoch}` is just a marker
        // that changes whenever the rendered bytes do; the handler ignores
        // it and renders fresh either way. Slash-separated segments sidestep
        // axum-0.8's "no literals in a param segment" rule.
        .route("/dashboard/{epoch}", get(serve_png))
        .route("/refresh",        get(force_refresh))
        .route("/health",         get(health))
        .with_state(state.clone());

    if let Some(path) = render_to {
        // No server needed for one-shot render — fetch (or skip if local
        // mode) then render directly.
        info!("rendering one frame to {path} (mock data)");
        refresh_data(&state).await;
        let bytes = match render_now(&state).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("render failed: {e}");
                std::process::exit(1);
            }
        };
        std::fs::write(&path, &bytes).expect("write png");
        info!("wrote {} bytes to {path}", bytes.len());
        // Drop the bound listener — it was reserved early to claim the port
        // but `RENDER_TO` exits before serving any traffic.
        drop(listener);
        return;
    }

    info!("Listening on http://{bound}{}", if local_mode { " (LOCAL_MODE: mock data only)" } else { "" });
    info!("Image    →  http://{bound}/dashboard.png");
    axum::serve(listener, app).await.expect("server error");
}
