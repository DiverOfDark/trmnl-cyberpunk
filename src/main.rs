mod dashboard;
mod data;
mod fetch;
mod firmware;
mod note;
mod note_screen;
mod render;
mod windows_tz;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use data::DashData;
use fetch::Sources;
use firmware::Firmware;
use note::{NoteStore, Playlist, Screen};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Default, Clone, Serialize)]
struct DeviceState {
    battery_pct: u8,
    rssi: i32,
    firmware: String,
    last_seen: String,
    /// Unix time of the last poll; picks the device device-less URLs preview.
    #[serde(skip)]
    seen_at: i64,
}

#[derive(Clone)]
struct AppState {
    /// Per-device telemetry, keyed by `device_key` (MAC hex), so each device
    /// renders its own battery/RSSI in the header.
    devices: Arc<RwLock<HashMap<String, DeviceState>>>,
    data: Arc<RwLock<DashData>>,
    sources: Arc<Sources>,
    /// Serializes upstream-fetch runs so a manual `/refresh` landing mid-cycle
    /// can't fan out into a second parallel pull of every upstream.
    fetch_lock: Arc<tokio::sync::Mutex<()>>,
    local_mode: bool,
    note: Arc<NoteStore>,
    playlist: Arc<std::sync::Mutex<Playlist>>,
    /// Patched firmware build offered to the device as an OTA update.
    firmware: Option<Arc<Firmware>>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mac_short_id(mac: &str) -> String {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    hex[hex.len().saturating_sub(6)..].to_uppercase()
}

/// Lowercase MAC hex (`ac276ea69d18`): stable per device and URL-safe.
fn device_key(mac: &str) -> String {
    mac.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase()
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
/// `/dashboard-{epoch}.png` because axum 0.8 doesn't allow a literal and a
/// param in the same path segment.
/// `{device}` picks whose battery/RSSI the header shows.
fn dashboard_url(device: &str, epoch: i64) -> String {
    format!("{}/dashboard/{device}/{epoch}", base_url())
}

/// Same cache-busting scheme as `dashboard_url`, for the memo screen.
fn note_url(device: &str, epoch: i64) -> String {
    format!("{}/note/{device}/{epoch}", base_url())
}

fn build_display_response(device: &str, epoch: i64, screen: Screen) -> DisplayResponse {
    let url = match screen {
        Screen::Dashboard => dashboard_url(device, epoch),
        Screen::Note => note_url(device, epoch),
    };
    DisplayResponse::new(url, dashboard_filename(epoch)).with_refresh_rate(refresh_secs())
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

/// Telemetry for the header: the named device, or — for device-less URLs like
/// `/dashboard.png` — whichever device polled most recently.
fn pick_device(devices: &HashMap<String, DeviceState>, key: Option<&str>) -> DeviceState {
    match key {
        Some(k) => devices.get(k).cloned().unwrap_or_default(),
        None => devices.values().max_by_key(|d| d.seen_at).cloned().unwrap_or_default(),
    }
}

/// Render one screen from the current `state.data` (and memo) to a PNG for
/// `device` (see `pick_device`). Called per-request from `serve_screen`, and
/// once at the end of `RENDER_TO=...` mode.
async fn render_now(state: &AppState, screen: Screen, device: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let mut data = state.data.read().await.clone();
    data.refresh_clock();
    let device = pick_device(&*state.devices.read().await, device);
    let note = state.note.get().await;

    let bytes = tokio::task::spawn_blocking(move || match screen {
        Screen::Dashboard => dashboard::render(&data, device.battery_pct, device.rssi),
        Screen::Note => note_screen::render(&data, &note, device.battery_pct, device.rssi),
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
        "image_url":   dashboard_url(&device_key(&device.mac_address), epoch),
        "message":     "TRMNL//CYBERPUNK — BYOS",
    }))
}

async fn api_display(
    State(state): State<AppState>,
    headers: HeaderMap,
    device: DeviceInfo,
) -> Json<DisplayResponse> {
    let model = headers.get("Model").and_then(|v| v.to_str().ok());
    info!(mac = %device.mac_address, model = ?model, fw = ?device.firmware_version, bat = ?device.battery_percentage(), rssi = ?device.rssi, "device poll");

    let key = device_key(&device.mac_address);
    {
        let mut devices = state.devices.write().await;
        let ds = devices.entry(key.clone()).or_default();
        ds.seen_at = chrono::Utc::now().timestamp();
        ds.battery_pct = device.battery_percentage().unwrap_or(0);
        ds.rssi = device.rssi.unwrap_or(0);
        ds.firmware = device.firmware_version.clone().unwrap_or_default();
        ds.last_seen = chrono::Local::now().format("%H:%M").to_string();
    }

    // No fetch here — the firmware downloads the screen's URL next, and that
    // handler renders from the cache. Stamp the URL with the current
    // timestamp so the firmware's 24h filename-dedupe sees a new key on
    // every poll and re-downloads.
    let has_note = !state.note.get().await.is_empty();
    let screen = state.playlist.lock().unwrap().next(has_note);
    let epoch = chrono::Utc::now().timestamp();
    let mut resp = build_display_response(&key, epoch, screen);
    if let Some(fw) = &state.firmware {
        if fw.should_offer(model, device.firmware_version.as_deref()) {
            info!(from = ?device.firmware_version, to = %fw.version, "offering firmware update");
            resp.update_firmware = true;
            resp.firmware_url = Some(format!("{}{}", base_url(), fw.path()));
        }
    }
    Json(resp)
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

#[utoipa::path(
    get,
    path = "/dashboard.png",
    responses((status = 200, description = "Dashboard screen, rendered from the cache", content_type = "image/png")),
    tag = "screens",
)]
async fn serve_png(State(state): State<AppState>) -> Response {
    // Render straight from the cache the background refresher maintains — the
    // device gets its PNG in milliseconds instead of waiting on upstreams.
    // Anything the refresher couldn't reach is drawn with a STALE marker, so
    // serving cached data never passes as current.
    serve_screen(&state, Screen::Dashboard, None).await
}

#[utoipa::path(
    get,
    path = "/note.png",
    responses((status = 200, description = "Memo screen, as the device would show it", content_type = "image/png")),
    tag = "screens",
)]
async fn serve_note(State(state): State<AppState>) -> Response {
    serve_screen(&state, Screen::Note, None).await
}

/// Per-device screen URLs handed out by `/api/display`; `_epoch` is only the
/// cache-buster.
async fn serve_device_png(
    State(state): State<AppState>,
    Path((device, _epoch)): Path<(String, String)>,
) -> Response {
    serve_screen(&state, Screen::Dashboard, Some(&device)).await
}

async fn serve_device_note(
    State(state): State<AppState>,
    Path((device, _epoch)): Path<(String, String)>,
) -> Response {
    serve_screen(&state, Screen::Note, Some(&device)).await
}

async fn serve_screen(state: &AppState, screen: Screen, device: Option<&str>) -> Response {
    match render_now(state, screen, device).await {
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

/// OTA download. The version in the path is informational; whatever build is
/// baked into the image gets served.
async fn serve_firmware(State(state): State<AppState>) -> Response {
    match &state.firmware {
        Some(fw) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            fw.bytes.clone(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/refresh",
    responses((status = 200, description = "Upstreams re-fetched; returns once the pull finishes")),
    tag = "ops",
)]
async fn force_refresh(State(state): State<AppState>) -> impl IntoResponse {
    refresh_data(&state).await;
    Json(json!({ "status": "ok" }))
}

#[utoipa::path(get, path = "/health", responses((status = 200, description = "Alive")), tag = "ops")]
async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ── Memo editor ───────────────────────────────────────────────────────────────

async fn editor() -> Html<&'static str> {
    Html(include_str!("editor.html"))
}

#[derive(Serialize, ToSchema)]
struct NoteDto {
    markdown: String,
    /// `null` until the memo has been written once.
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
struct NoteSaved {
    updated_at: Option<DateTime<Utc>>,
}

#[utoipa::path(
    get,
    path = "/api/note",
    responses((status = 200, description = "The current memo", body = NoteDto)),
    tag = "memo",
)]
async fn get_note(State(state): State<AppState>) -> Json<NoteDto> {
    let note = state.note.get().await;
    Json(NoteDto { markdown: note.markdown, updated_at: note.updated_at })
}

/// Replace the memo. The body is the raw markdown, not JSON, so it's
/// curl-friendly; the editor PUTs the full text on every pause in typing.
/// An empty body clears the memo and drops its screen from the rotation.
#[utoipa::path(
    put,
    path = "/api/note",
    request_body(content = String, content_type = "text/markdown", description = "Full memo markdown"),
    responses(
        (status = 200, description = "Saved; the device shows it on its next wake-up", body = NoteSaved),
        (status = 500, description = "Writing to DATA_DIR failed"),
    ),
    tag = "memo",
)]
async fn put_note(State(state): State<AppState>, body: String) -> Response {
    match state.note.set(body).await {
        Ok(note) => {
            state.playlist.lock().unwrap().note_edited();
            Json(NoteSaved { updated_at: note.updated_at }).into_response()
        }
        Err(e) => {
            warn!("saving memo failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "save failed").into_response()
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "trmnl-cyberpunk",
        description = "BYOS server for TRMNL e-ink panels. The device-protocol endpoints (`/api/setup`, `/api/display`, `/api/log`) and the cache-busted screen URLs (`/dashboard/{epoch}`, `/note/{epoch}`) are reachable too but are driven by the firmware, so they aren't part of this schema.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(get_note, put_note, serve_png, serve_note, force_refresh, health),
    components(schemas(NoteDto, NoteSaved)),
    tags(
        (name = "memo", description = "Read and write the memo screen's markdown"),
        (name = "screens", description = "Rendered 800x480 panel images"),
        (name = "ops", description = "Refresh and health"),
    ),
)]
struct ApiDoc;

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

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into());

    let state = AppState {
        devices: Arc::new(RwLock::new(HashMap::new())),
        data: Arc::new(RwLock::new(initial_data)),
        sources: Arc::new(Sources::from_env()),
        fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
        local_mode,
        note: Arc::new(NoteStore::open(&data_dir).await),
        playlist: Arc::new(std::sync::Mutex::new(Playlist::default())),
        firmware: if render_to.is_some() { None } else { Firmware::from_env().map(Arc::new) },
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
        // Per-device URLs handed out by `/api/display` and `/api/setup`.
        .route("/dashboard/{device}/{epoch}", get(serve_device_png))
        .route("/note.png", get(serve_note))
        .route("/note/{epoch}", get(serve_note))
        .route("/note/{device}/{epoch}", get(serve_device_note))
        .route("/firmware/{file}", get(serve_firmware))
        .route("/refresh", get(force_refresh))
        .route("/health", get(health))
        .route("/", get(editor))
        .route("/api/note", get(get_note).put(put_note))
        .merge(SwaggerUi::new("/swagger").url("/openapi.json", ApiDoc::openapi()))
        .with_state(state.clone());

    if let Some(path) = render_to {
        // No server needed for one-shot render — fetch (or skip if local
        // mode) then render directly.
        info!("rendering one frame to {path} (mock data)");
        refresh_data(&state).await;
        let bytes = match render_now(&state, Screen::Dashboard, None).await {
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
    info!("Memo     →  http://{bound}/  (stored in {data_dir}/note.md)");
    info!("API docs →  http://{bound}/swagger");
    axum::serve(listener, app).await.expect("server error");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(bat: u8, at: i64) -> DeviceState {
        DeviceState { battery_pct: bat, seen_at: at, ..Default::default() }
    }

    #[test]
    fn device_key_normalizes_mac() {
        assert_eq!(device_key("AC:27:6E:A6:9D:18"), "ac276ea69d18");
    }

    #[test]
    fn picks_named_device_or_most_recent() {
        let devices = HashMap::from([("a".to_string(), seen(10, 100)), ("b".to_string(), seen(90, 200))]);
        assert_eq!(pick_device(&devices, Some("a")).battery_pct, 10);
        assert_eq!(pick_device(&devices, None).battery_pct, 90);
        assert_eq!(pick_device(&devices, Some("unknown")).battery_pct, 0);
    }
}
