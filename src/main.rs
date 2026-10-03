mod agents;
mod dashboard;
mod data;
mod desk;
mod devices;
mod fetch;
mod firmware;
mod github;
mod note;
mod note_screen;
mod render;
mod stackchan;
mod windows_tz;

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
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use agents::{Agents, LoginStatus, Pending, Provider};
use data::DashData;
use devices::{device_key, Device, DevicePatch, DeviceStore, Mode};
use fetch::Sources;
use firmware::Firmware;
use note::{NoteStore, Screen};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct AppState {
    data: Arc<RwLock<DashData>>,
    sources: Arc<Sources>,
    /// Serializes upstream-fetch runs so a manual `/refresh` landing mid-cycle
    /// can't fan out into a second parallel pull of every upstream.
    fetch_lock: Arc<tokio::sync::Mutex<()>>,
    local_mode: bool,
    note: Arc<NoteStore>,
    /// Registered panels: settings, screen assignment and last status.
    devices: Arc<DeviceStore>,
    /// Claude / Codex sign-ins (the same logins the fetcher reads usage with).
    agents: Arc<Agents>,
    /// Patched firmware build offered to the device as an OTA update.
    firmware: Option<Arc<Firmware>>,
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

fn desk_url(device: &str, epoch: i64) -> String {
    format!("{}/desk/{device}/{epoch}", base_url())
}

fn build_display_response(device: &str, epoch: i64, screen: Screen) -> DisplayResponse {
    let url = match screen {
        Screen::Dashboard => dashboard_url(device, epoch),
        Screen::Note => note_url(device, epoch),
        Screen::Desk => desk_url(device, epoch),
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

/// Render one screen from the current `state.data` (and memo) to a PNG for
/// `device` — or, for device-less URLs like `/dashboard.png`, whichever
/// device polled most recently. Called per-request from `serve_screen`, and
/// once at the end of `RENDER_TO=...` mode.
async fn render_now(state: &AppState, screen: Screen, device: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let mut data = state.data.read().await.clone();
    data.refresh_clock();
    let device = state.devices.get_or_latest(device).await;
    let battery = device.as_ref().and_then(|d| d.battery_pct).unwrap_or(0);
    let rssi = device.as_ref().and_then(|d| d.rssi).unwrap_or(0);
    let unit = device.map(|d| d.name).unwrap_or_default();
    let note = state.note.get().await;

    let bytes = tokio::task::spawn_blocking(move || match screen {
        Screen::Dashboard => dashboard::render(&data, &unit, battery, rssi),
        Screen::Note => note_screen::render(&data, &note, &unit, battery, rssi),
        Screen::Desk => desk::render(&data, &note, &unit, battery, rssi),
    })
    .await??;

    Ok(bytes)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn api_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    device: DeviceInfo,
) -> impl IntoResponse {
    info!(mac = %device.mac_address, fw = ?device.firmware_version, "device setup");
    let model = headers.get("Model").and_then(|v| v.to_str().ok());
    state.devices.register(&device, model).await;
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

    // No fetch here — the firmware downloads the screen's URL next, and that
    // handler renders from the cache. Stamp the URL with the current
    // timestamp so the firmware's 24h filename-dedupe sees a new key on
    // every poll and re-downloads.
    let screen = state.devices.checkin(&device, model).await;
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

#[utoipa::path(
    get,
    path = "/desk.png",
    responses((status = 200, description = "Desk screen: agent limits, today's events, alerts and the memo", content_type = "image/png")),
    tag = "screens",
)]
async fn serve_desk(State(state): State<AppState>) -> Response {
    serve_screen(&state, Screen::Desk, None).await
}

async fn serve_device_desk(
    State(state): State<AppState>,
    Path((device, _epoch)): Path<(String, String)>,
) -> Response {
    serve_screen(&state, Screen::Desk, Some(&device)).await
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
/// An empty body clears the memo; devices assigned to it show its empty state.
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
        Ok(note) => Json(NoteSaved { updated_at: note.updated_at }).into_response(),
        Err(e) => {
            warn!("saving memo failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "save failed").into_response()
        }
    }
}

// ── Agent sign-in ─────────────────────────────────────────────────────────────

async fn agents_page() -> Html<&'static str> {
    Html(include_str!("agents.html"))
}

#[derive(Serialize, ToSchema)]
struct AgentDto {
    #[serde(flatten)]
    login: LoginStatus,
    /// Last pulled rate limits, percent used. `null` until a pull succeeds.
    session_pct: Option<u8>,
    week_pct: Option<u8>,
}

#[utoipa::path(
    get,
    path = "/api/agents",
    responses((status = 200, description = "Sign-in state and last limits of each agent", body = [AgentDto])),
    tag = "agents",
)]
async fn list_agents(State(state): State<AppState>) -> Json<Vec<AgentDto>> {
    let data = state.data.read().await;
    let agents = state
        .agents
        .status()
        .await
        .into_iter()
        .map(|login| {
            let usage = match login.provider.as_str() {
                "claude" => data.claude.as_ref(),
                _ => data.codex.as_ref(),
            }
            .filter(|_| login.signed_in);
            AgentDto {
                session_pct: usage.map(|u| u.session_pct),
                week_pct: usage.map(|u| u.week_pct),
                login,
            }
        })
        .collect();
    Json(agents)
}

fn bad_provider() -> Response {
    (StatusCode::BAD_REQUEST, "provider must be claude or codex").into_response()
}

/// Start signing in. The response's `pending` says what to do next: open a
/// link and paste back a code (Claude), or enter a code on a page (Codex).
#[utoipa::path(
    post,
    path = "/api/agents/{provider}/login",
    params(("provider" = String, Path, description = "`claude` or `codex`")),
    responses(
        (status = 200, description = "Sign-in started", body = Pending),
        (status = 502, description = "The provider refused to start one"),
    ),
    tag = "agents",
)]
async fn start_login(State(state): State<AppState>, Path(provider): Path<String>) -> Response {
    let Some(provider) = Provider::parse(&provider) else {
        return bad_provider();
    };
    if let Err(e) = state.agents.start_login(provider).await {
        warn!("starting sign-in failed: {e:#}");
        return (StatusCode::BAD_GATEWAY, format!("{e:#}")).into_response();
    }
    let status = state.agents.status().await;
    let pending = status.into_iter().find_map(|s| (Provider::parse(&s.provider) == Some(provider)).then_some(s.pending)).flatten();
    Json(pending).into_response()
}

#[derive(Deserialize, ToSchema)]
struct PastedCode {
    /// What the page showed after approving (`code#state`), or the callback URL.
    code: String,
}

/// Finish a Claude sign-in with the code its callback page showed.
#[utoipa::path(
    post,
    path = "/api/agents/claude/code",
    request_body = PastedCode,
    responses(
        (status = 204, description = "Signed in"),
        (status = 400, description = "Wrong or expired code; the sign-in stays open for another try"),
    ),
    tag = "agents",
)]
async fn finish_claude(State(state): State<AppState>, Json(body): Json<PastedCode>) -> Response {
    match state.agents.finish_claude(&body.code).await {
        Ok(()) => {
            spawn_refresh(&state);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e:#}")).into_response(),
    }
}

/// Sign out (and drop any sign-in in progress). The stored tokens are deleted.
#[utoipa::path(
    delete,
    path = "/api/agents/{provider}/login",
    params(("provider" = String, Path, description = "`claude` or `codex`")),
    responses((status = 204, description = "Signed out")),
    tag = "agents",
)]
async fn sign_out(State(state): State<AppState>, Path(provider): Path<String>) -> Response {
    let Some(provider) = Provider::parse(&provider) else {
        return bad_provider();
    };
    match state.agents.sign_out(provider).await {
        Ok(()) => {
            spawn_refresh(&state);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            warn!("signing out failed: {e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "sign-out failed").into_response()
        }
    }
}

/// Re-pull upstreams in the background so a sign-in shows up on the panel
/// without waiting out the fetch interval.
fn spawn_refresh(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move { refresh_data(&state).await });
}

// ── Devices ───────────────────────────────────────────────────────────────────

async fn devices_page() -> Html<&'static str> {
    Html(include_str!("devices.html"))
}

#[derive(Serialize, ToSchema)]
struct DevicesDto {
    /// The refresh interval handed to devices, seconds — a device not seen
    /// for well over this is probably asleep for good or offline.
    refresh_secs: u32,
    devices: Vec<Device>,
}

#[utoipa::path(
    get,
    path = "/api/devices",
    responses((status = 200, description = "Every device that has polled, with its last status", body = DevicesDto)),
    tag = "devices",
)]
async fn list_devices(State(state): State<AppState>) -> Json<DevicesDto> {
    Json(DevicesDto { refresh_secs: refresh_secs(), devices: state.devices.list().await })
}

/// Rename a device or change which screen it shows. Takes effect on the
/// device's next wake-up.
#[utoipa::path(
    patch,
    path = "/api/devices/{mac}",
    params(("mac" = String, Path, description = "Device MAC address")),
    request_body = DevicePatch,
    responses(
        (status = 200, description = "Updated", body = Device),
        (status = 404, description = "No device with that MAC has polled"),
        (status = 500, description = "Writing to DATA_DIR failed"),
    ),
    tag = "devices",
)]
async fn patch_device(
    State(state): State<AppState>,
    Path(mac): Path<String>,
    Json(patch): Json<DevicePatch>,
) -> Response {
    match state.devices.update(&mac, patch).await {
        Ok(Some(device)) => Json(device).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "unknown device").into_response(),
        Err(e) => {
            warn!("saving devices failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "save failed").into_response()
        }
    }
}

/// Forget a device. If it polls again it comes back with default settings.
#[utoipa::path(
    delete,
    path = "/api/devices/{mac}",
    params(("mac" = String, Path, description = "Device MAC address")),
    responses(
        (status = 204, description = "Removed"),
        (status = 404, description = "No device with that MAC"),
        (status = 500, description = "Writing to DATA_DIR failed"),
    ),
    tag = "devices",
)]
async fn delete_device(State(state): State<AppState>, Path(mac): Path<String>) -> Response {
    match state.devices.remove(&mac).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "unknown device").into_response(),
        Err(e) => {
            warn!("saving devices failed: {e}");
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
    paths(get_note, put_note, list_devices, patch_device, delete_device, list_agents, start_login, finish_claude, sign_out, serve_png, serve_note, serve_desk, force_refresh, health, stackchan::get_usage),
    components(schemas(NoteDto, NoteSaved, DevicesDto, Device, DevicePatch, Mode, Screen, AgentDto, LoginStatus, Pending, PastedCode, stackchan::StackchanUsage, stackchan::Window)),
    tags(
        (name = "memo", description = "Read and write the memo screen's markdown"),
        (name = "devices", description = "Registered panels, their last status, and which screen each shows"),
        (name = "agents", description = "Claude / Codex sign-in for the desk screen"),
        (name = "stackchan", description = "Claude usage for the Femto StackChan desk robot"),
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
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into());

    let initial_data = if local_mode {
        DashData::mock()
    } else {
        DashData::empty()
    };

    let sources = Arc::new(Sources::from_env(std::path::Path::new(&data_dir)));
    let state = AppState {
        data: Arc::new(RwLock::new(initial_data)),
        agents: sources.agents.clone(),
        sources,
        fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
        local_mode,
        note: Arc::new(NoteStore::open(&data_dir).await),
        devices: Arc::new(DeviceStore::open(&data_dir).await),
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
        .route("/desk.png", get(serve_desk))
        .route("/desk/{epoch}", get(serve_desk))
        .route("/desk/{device}/{epoch}", get(serve_device_desk))
        .route("/agents", get(agents_page))
        .route("/api/agents", get(list_agents))
        .route("/api/stackchan/usage", get(stackchan::get_usage))
        .route("/api/agents/claude/code", post(finish_claude))
        .route("/api/agents/{provider}/login", post(start_login).delete(sign_out))
        .route("/firmware/{file}", get(serve_firmware))
        .route("/refresh", get(force_refresh))
        .route("/health", get(health))
        .route("/", get(editor))
        .route("/api/note", get(get_note).put(put_note))
        .route("/devices", get(devices_page))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/{mac}", axum::routing::patch(patch_device).delete(delete_device))
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
    info!("Image    →  http://{bound}/dashboard.png  ·  /desk.png");
    info!("Memo     →  http://{bound}/  (stored in {data_dir}/note.md)");
    info!("Devices  →  http://{bound}/devices  (stored in {data_dir}/devices.json)");
    info!("Agents   →  http://{bound}/agents  (Claude / Codex sign-in for the desk screen)");
    info!("API docs →  http://{bound}/swagger");
    axum::serve(listener, app).await.expect("server error");
}
