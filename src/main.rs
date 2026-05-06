mod data;

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use futures::StreamExt;
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};
use trmnl::{DeviceInfo, DisplayResponse};

use data::DashData;

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
    port: u16,
    render_lock: Arc<tokio::sync::Mutex<()>>,
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

fn build_display_response() -> DisplayResponse {
    let url = format!("{}/dashboard.png", base_url());
    DisplayResponse::new(&url, "dashboard.png").with_refresh_rate(refresh_secs())
}

// ── Rendering ─────────────────────────────────────────────────────────────────

async fn screenshot_dashboard(port: u16) -> anyhow::Result<Vec<u8>> {
    use chromiumoxide::{Browser, BrowserConfig};
    use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;

    let chrome = std::env::var("CHROME_PATH").unwrap_or_else(|_| {
        for p in ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome-stable"] {
            if std::path::Path::new(p).exists() {
                return p.to_string();
            }
        }
        "chromium".to_string()
    });

    let config = BrowserConfig::builder()
        .chrome_executable(chrome)
        .window_size(800, 480)
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-software-rasterizer")
        .arg("--hide-scrollbars")
        .build()
        .map_err(|e| anyhow::anyhow!("browser config: {e}"))?;

    let (mut browser, mut handler) = Browser::launch(config).await?;

    let handler_task = tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let page = browser
        .new_page(format!("http://127.0.0.1:{port}/dashboard.html"))
        .await?;

    // Wait up to 15 s for the JS to fetch /api/data and set data-ready
    tokio::time::timeout(
        Duration::from_secs(15),
        page.find_element("[data-ready=true]"),
    )
    .await
    .map_err(|_| anyhow::anyhow!("page render timed out"))??;

    let png = page
        .screenshot(
            chromiumoxide::page::ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .full_page(false)
                .build(),
        )
        .await?;

    browser.close().await?;
    let _ = handler_task.await;

    Ok(png)
}

async fn regenerate(state: &AppState) {
    let Ok(_guard) = state.render_lock.try_lock() else {
        info!("render already in progress, skipping");
        return;
    };

    *state.data.write().await = DashData::mock();

    match screenshot_dashboard(state.port).await {
        Ok(png) => {
            *state.png_cache.write().await = png;
            info!("dashboard regenerated");
        }
        Err(e) => {
            warn!("render failed: {e}");
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn api_setup(State(_): State<AppState>, device: DeviceInfo) -> impl IntoResponse {
    info!(mac = %device.mac_address, fw = ?device.firmware_version, "device setup");
    let api_key = std::env::var("TRMNL_API_KEY").unwrap_or_else(|_| "cyberpunk-byos".into());
    Json(json!({
        "api_key":     api_key,
        "friendly_id": mac_short_id(&device.mac_address),
        "image_url":   format!("{}/dashboard.png", base_url()),
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

    Json(build_display_response())
}

async fn api_log(State(_): State<AppState>, device: DeviceInfo) -> StatusCode {
    warn!(mac = %device.mac_address, "device log received");
    StatusCode::NO_CONTENT
}

async fn api_data(State(state): State<AppState>) -> impl IntoResponse {
    let data   = state.data.read().await.clone();
    let device = state.device_state.read().await.clone();
    Json(json!({
        "device": device,
        "time":     data.time,
        "date":     data.date,
        "date_dow": data.date_dow,
        "motto":    data.motto,
        "hosts":    data.hosts,
        "weather":  data.weather,
        "agenda":   data.agenda,
        "tasks":    data.tasks,
        "budget":   data.budget,
        "alerts":   data.alerts,
    }))
}

async fn serve_dashboard_html() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8"),
         (header::CACHE_CONTROL, "no-cache")],
        include_str!("../templates/dashboard.html"),
    )
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

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "trmnl_cyberpunk=info,tower_http=warn".into()),
        )
        .init();

    let addr = std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    let port = listener.local_addr().unwrap().port();

    let state = AppState {
        png_cache:   Arc::new(RwLock::new(Vec::new())),
        device_state: Arc::new(RwLock::new(DeviceState::default())),
        data:        Arc::new(RwLock::new(DashData::mock())),
        port,
        render_lock: Arc::new(tokio::sync::Mutex::new(())),
    };

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

    let app = Router::new()
        .route("/api/setup",      get(api_setup))
        .route("/api/display",    get(api_display))
        .route("/api/log",        post(api_log))
        .route("/api/data",       get(api_data))
        .route("/dashboard.html", get(serve_dashboard_html))
        .route("/dashboard.png",  get(serve_png))
        .route("/refresh",        get(force_refresh))
        .route("/health",         get(health))
        .with_state(state);

    info!("Listening on http://{addr}");
    info!("Preview  →  http://{addr}/dashboard.html");
    info!("Image    →  http://{addr}/dashboard.png");
    axum::serve(listener, app).await.expect("server error");
}
