mod dashboard;
mod data;
mod fetch;
mod render;
mod windows_tz;

use std::sync::Arc;
use std::time::Duration;

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
    /// Serializes upstream-fetch runs so a manual `/refresh` landing mid-cycle
    /// can't fan out into a second parallel pull of every upstream.
    fetch_lock: Arc<tokio::sync::Mutex<()>>,
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

/// How often the background refresher pulls upstream. Independent of
/// `REFRESH_SECS` (how often the *device* wakes up): the device should always
/// find a rendered-in-milliseconds image waiting, which means the data behind
/// it has to be refreshed on our own clock, not the device's.
fn fetch_interval_secs() -> u64 {
    std::env::var("FETCH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v| v > 0)
        .unwrap_or(300)
}

/// Filename the firmware uses as its 24h dedupe cache key. It expects a
/// 10-digit Unix epoch suffix (`dashboard-1778185416.png`); we stamp it
/// with the current time on every poll so the firmware always sees a
/// fresh URL and re-downloads.
fn dashboard_filename(epoch: i64) -> String {
    format!("dashboard-{epoch}.png")
}

/// URL the firmware downloads. Goes through `/dashboard/{epoch}` rather than
/// `/dashboard-{epoch}.png` because Swagger UI is mounted at `/` and its
/// wildcard catch-all would otherwise swallow any single-segment request
/// (and reply 404 because Swagger has no such asset). Slash-separated
/// segments don't conflict with the UI's wildcard.
fn dashboard_url(epoch: i64) -> String {
    format!("{}/dashboard/{epoch}", base_url())
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

    let prev = state.data.read().await.clone();
    let fresh = if state.local_mode {
        DashData::mock()
    } else {
        // Hand the previous snapshot down so sections whose upstream is
        // failing keep their last good values (flagged stale) rather than
        // blanking their panel.
        state.sources.fetch(&prev).await
    };
    let degraded = fresh.status.degraded(chrono::Utc::now());
    *state.data.write().await = fresh;
    if degraded.is_empty() {
        info!("data refreshed");
    } else {
        let summary = degraded
            .iter()
            .map(|(tag, marker)| format!("{tag} {marker}"))
            .collect::<Vec<_>>()
            .join(", ");
        info!("data refreshed; degraded: {summary}");
    }
}

/// Refresh upstream data on a fixed cadence, decoupled from HTTP traffic.
/// `/dashboard*` used to fetch inline, which made every device poll wait out
/// the slowest upstream (retries and all) before a single pixel was drawn.
async fn refresh_loop(state: AppState) {
    let interval = Duration::from_secs(fetch_interval_secs());
    loop {
        tokio::time::sleep(interval).await;
        refresh_data(&state).await;
    }
}

/// Render the current `state.data` to a PNG. Called per-request from
/// `serve_png`, and once at the end of `RENDER_TO=...` mode.
async fn render_now(state: &AppState) -> anyhow::Result<Vec<u8>> {
    let mut data = state.data.read().await.clone();
    data.refresh_clock();
    let device = state.device_state.read().await.clone();

    let bytes = tokio::task::spawn_blocking(move || {
        dashboard::render(&data, device.battery_pct, device.rssi)
    })
    .await??;

    Ok(bytes)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn api_setup(State(_): State<AppState>, device: DeviceInfo) -> impl IntoResponse {
    info!(mac = %device.mac_address, fw = ?device.firmware_version, "device setup");
    let api_key = std::env::var("TRMNL_API_KEY").unwrap_or_else(|_| "cyberpunk-byos".into());
    let epoch = chrono::Utc::now().timestamp();
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
    // `serve_png` refreshes upstream data before rendering. Stamp the URL
    // with the current timestamp so the firmware's 24h filename-dedupe sees
    // a new key on every poll and re-downloads.
    let epoch = chrono::Utc::now().timestamp();
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
    // Render straight from the cache the background refresher maintains — the
    // device gets its PNG in milliseconds instead of waiting on upstreams.
    // Anything the refresher couldn't reach is drawn with a STALE marker, so
    // serving cached data never passes as current.
    match render_now(&state).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
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
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("bind failed");
    let bound = listener.local_addr().unwrap();

    // Serving real data means starting empty: a request arriving in the second
    // or two before the priming fetch lands gets empty panels rather than mock
    // numbers a viewer would read as real. LOCAL_MODE is the one place mock
    // data belongs.
    let initial_data = if local_mode {
        DashData::mock()
    } else {
        DashData::empty()
    };

    let state = AppState {
        device_state: Arc::new(RwLock::new(DeviceState::default())),
        data: Arc::new(RwLock::new(initial_data)),
        sources: Arc::new(Sources::from_env()),
        fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
        local_mode,
    };

    let app = Router::new()
        .route("/api/setup", get(api_setup))
        .route("/api/display", get(api_display))
        .route("/api/log", post(api_log))
        .route("/dashboard.png", get(serve_png))
        // Cache-bustered URL for the firmware. `{epoch}` is just a marker
        // that changes whenever the rendered bytes do; the handler ignores
        // it and renders fresh either way. Slash-separated segments sidestep
        // axum-0.8's "no literals in a param segment" rule.
        .route("/dashboard/{epoch}", get(serve_png))
        .route("/refresh", get(force_refresh))
        .route("/health", get(health))
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

    // Prime the cache before serving, then keep it warm on a timer. Both the
    // priming pull and the loop are detached so a slow upstream delays only
    // the data, not the listener.
    tokio::spawn({
        let state = state.clone();
        async move {
            refresh_data(&state).await;
            refresh_loop(state).await;
        }
    });

    info!(
        "Listening on http://{bound}{}",
        if local_mode {
            " (LOCAL_MODE: mock data only)"
        } else {
            ""
        }
    );
    info!("Refreshing upstream data every {}s", fetch_interval_secs());
    info!("Image    →  http://{bound}/dashboard.png");
    axum::serve(listener, app).await.expect("server error");
}
