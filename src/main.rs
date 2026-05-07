mod dashboard;
mod data;
mod fetch;
mod render;
mod windows_tz;

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use data::{DashData, Task};
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
    png_cache: Arc<RwLock<Vec<u8>>>,
    device_state: Arc<RwLock<DeviceState>>,
    data: Arc<RwLock<DashData>>,
    sources: Arc<Sources>,
    render_lock: Arc<tokio::sync::Mutex<()>>,
    /// Unix epoch (seconds) of the last successful render. Bumped on every
    /// regenerate; embedded into `/api/display` URLs as a cache-buster
    /// (`dashboard-{epoch}.png`) so the firmware skips its 24h dedupe and
    /// fetches the new bytes whenever the image content has actually changed.
    last_render_at: Arc<RwLock<i64>>,
    /// Externally-pushed task list (via `PUT /api/tasks`). Lives outside
    /// `data` so a background fetch doesn't overwrite it. `regenerate`
    /// overlays this onto `DashData.tasks` before rendering.
    tasks: Arc<RwLock<Vec<Task>>>,
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

// ── Rendering ─────────────────────────────────────────────────────────────────

async fn regenerate(state: &AppState) {
    let Ok(_guard) = state.render_lock.try_lock() else {
        info!("render already in progress, skipping");
        return;
    };

    let mock = DashData::mock();
    let mut fresh = if state.local_mode {
        mock
    } else {
        state.sources.fetch(&mock).await
    };
    // Overlay externally-pushed tasks onto the freshly-fetched data so the
    // render reflects the latest `PUT /api/tasks` push.
    fresh.tasks = state.tasks.read().await.clone();
    *state.data.write().await = fresh.clone();

    let device = state.device_state.read().await.clone();
    let png = tokio::task::spawn_blocking(move || {
        dashboard::render(&fresh, device.battery_pct, device.rssi)
    })
    .await;

    match png {
        Ok(Ok(bytes)) => {
            *state.png_cache.write().await = bytes;
            *state.last_render_at.write().await = chrono::Utc::now().timestamp();
            info!("dashboard regenerated");
        }
        Ok(Err(e)) => warn!("render failed: {e}"),
        Err(e)     => warn!("render task panicked: {e}"),
    }
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

    // Trigger async re-render so the new battery/signal shows on next refresh
    let s = state.clone();
    tokio::spawn(async move { regenerate(&s).await });

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

/// Get the current OPS task list as it will appear on the panel.
#[utoipa::path(
    get,
    path = "/api/tasks",
    responses(
        (status = 200, description = "Current task list", body = Vec<Task>),
    ),
    tag = "tasks",
)]
async fn get_tasks(State(state): State<AppState>) -> Json<Vec<Task>> {
    Json(state.tasks.read().await.clone())
}

/// Replace the OPS task list. Accepts a JSON array of tasks; the panel
/// re-renders with the new list within a few seconds. If the `API_TOKEN`
/// env var is set on the server, requests must include
/// `Authorization: Bearer <token>`.
#[utoipa::path(
    put,
    path = "/api/tasks",
    request_body = Vec<Task>,
    responses(
        (status = 204, description = "Task list replaced; re-render scheduled"),
        (status = 401, description = "API_TOKEN is configured and the bearer token is missing or wrong"),
    ),
    security(("bearer" = [])),
    tag = "tasks",
)]
async fn put_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(tasks): Json<Vec<Task>>,
) -> StatusCode {
    if !check_token(&headers) {
        return StatusCode::UNAUTHORIZED;
    }
    *state.tasks.write().await = tasks;
    // Re-render in the background so the new list reaches the panel on the
    // device's next poll without forcing the caller to wait for it.
    let s = state.clone();
    tokio::spawn(async move { regenerate(&s).await });
    StatusCode::NO_CONTENT
}

/// If `API_TOKEN` is set and non-empty, require an `Authorization: Bearer
/// <token>` header that matches. Empty / unset env var = open access.
fn check_token(headers: &HeaderMap) -> bool {
    let Ok(token) = std::env::var("API_TOKEN") else { return true };
    if token.is_empty() { return true; }
    let expected = format!("Bearer {token}");
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == expected)
}

/// Snapshot of the current dashboard data (everything `/dashboard.png` is
/// rendered from). Useful for debugging and for clients that want to render
/// their own preview.
#[utoipa::path(
    get,
    path = "/api/data",
    responses(
        (status = 200, description = "Current dashboard data + device state",
         content_type = "application/json"),
    ),
    tag = "data",
)]
async fn api_data(State(state): State<AppState>) -> impl IntoResponse {
    let data   = state.data.read().await.clone();
    let device = state.device_state.read().await.clone();
    Json(json!({
        "device": device,
        "time":     data.time,
        "date":     data.date,
        "date_dow": data.date_dow,
        "motto":     data.motto,
        "last_sync": data.last_sync,
        "next_sync": data.next_sync,
        "hosts":     data.hosts,
        "weather":  data.weather,
        "agenda":   data.agenda,
        "tasks":    data.tasks,
        "budget":   data.budget,
        "alerts":   data.alerts,
    }))
}

async fn serve_png(State(state): State<AppState>) -> Response {
    let bytes = state.png_cache.read().await.clone();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-cache")],
        bytes,
    )
        .into_response()
}

async fn force_refresh(State(state): State<AppState>) -> impl IntoResponse {
    regenerate(&state).await;
    Json(json!({ "status": "ok" }))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// ── OpenAPI ───────────────────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    info(
        title = "trmnl-cyberpunk",
        description = "BYOS server for TRMNL e-ink panels with the cyberpunk dashboard. Public endpoints: tasks (push from external services) + data (snapshot). Device-protocol endpoints (`/api/setup`, `/api/display`, `/api/log`) and the rendered PNG (`/dashboard.png`, `/dashboard/{epoch}`) are also reachable but not part of this schema.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(get_tasks, put_tasks, api_data),
    components(schemas(Task)),
    tags(
        (name = "tasks", description = "Push/read the OPS panel task list"),
        (name = "data",  description = "Read the current dashboard snapshot"),
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
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    let bound = listener.local_addr().unwrap();

    // Seed the externally-pushed task list with the mock tasks so the panel
    // has something to show before any client has called `PUT /api/tasks`.
    let initial_mock = DashData::mock();
    let initial_tasks = initial_mock.tasks.clone();

    let state = AppState {
        png_cache:      Arc::new(RwLock::new(Vec::new())),
        device_state:   Arc::new(RwLock::new(DeviceState::default())),
        data:           Arc::new(RwLock::new(initial_mock)),
        sources:        Arc::new(Sources::from_env()),
        render_lock:    Arc::new(tokio::sync::Mutex::new(())),
        last_render_at: Arc::new(RwLock::new(0)),
        tasks:          Arc::new(RwLock::new(initial_tasks)),
        local_mode,
    };

    let app = Router::new()
        .route("/api/setup",      get(api_setup))
        .route("/api/display",    get(api_display))
        .route("/api/log",        post(api_log))
        .route("/api/data",       get(api_data))
        .route("/api/tasks",      get(get_tasks).put(put_tasks))
        .route("/dashboard.png",  get(serve_png))
        // Cache-bustered URL for the firmware. `{epoch}` is just a marker
        // that changes whenever the rendered bytes do; the handler ignores
        // it and always serves the current cache. Slash-separated segments
        // sidestep both axum-0.8's "no literals in a param segment" rule
        // *and* Swagger UI's wildcard catch-all at `/`.
        .route("/dashboard/{epoch}", get(serve_png))
        .route("/refresh",        get(force_refresh))
        .route("/health",         get(health))
        .merge(SwaggerUi::new("/").url("/openapi.json", ApiDoc::openapi()))
        .with_state(state.clone());

    if let Some(path) = render_to {
        // Run the server just long enough for a single in-process screenshot.
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        // Yield so the listener is actively accepting before Chrome connects.
        tokio::time::sleep(Duration::from_millis(100)).await;
        info!("rendering one frame to {path} (mock data)");
        regenerate(&state).await;
        let bytes = state.png_cache.read().await.clone();
        if bytes.is_empty() {
            eprintln!("render produced no bytes — see warnings above");
            std::process::exit(1);
        }
        std::fs::write(&path, &bytes).expect("write png");
        info!("wrote {} bytes to {path}", bytes.len());
        server.abort();
        return;
    }

    // Background render loop — first render fires after server is ready.
    let bg = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let secs = refresh_secs() as u64;
        loop {
            regenerate(&bg).await;
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    });

    info!("Listening on http://{bound}{}", if local_mode { " (LOCAL_MODE: mock data only)" } else { "" });
    info!("Image    →  http://{bound}/dashboard.png");
    axum::serve(listener, app).await.expect("server error");
}
