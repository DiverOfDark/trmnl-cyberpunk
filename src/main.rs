mod data;
mod fetch;

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
    port: u16,
    render_lock: Arc<tokio::sync::Mutex<()>>,
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

fn build_display_response() -> DisplayResponse {
    let url = format!("{}/dashboard.png", base_url());
    DisplayResponse::new(&url, "dashboard.png").with_refresh_rate(refresh_secs())
}

// ── Rendering ─────────────────────────────────────────────────────────────────

async fn screenshot_dashboard(port: u16) -> anyhow::Result<Vec<u8>> {
    use chromiumoxide::{Browser, BrowserConfig};
    use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
    use chromiumoxide::handler::viewport::Viewport;

    let chrome = std::env::var("CHROME_PATH").unwrap_or_else(|_| {
        for p in ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome-stable"] {
            if std::path::Path::new(p).exists() {
                return p.to_string();
            }
        }
        "chromium".to_string()
    });

    // Render at 2× DPR (1600×960 device pixels) then downscale to the device's
    // native 800×480 — supersampling AA gives crisper text than rendering 1:1.
    let config = BrowserConfig::builder()
        .chrome_executable(chrome)
        .window_size(1600, 960)
        .viewport(Viewport { width: 800, height: 480, device_scale_factor: Some(2.0), emulating_mobile: false, is_landscape: true, has_touch: false })
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

    downscale_supersampled(&png)
}

/// Downscale the 2× supersampled screenshot to the panel's native 800×480
/// and quantize to the 6-color e-ink palette with Floyd–Steinberg dithering,
/// then encode as an indexed PNG with max zlib compression.
///
/// The panel can only render the colors in `PALETTE`, so any other pixel from
/// the browser must be approximated. Doing the quantization here (instead of
/// letting the device firmware do it) gives:
///   - predictable on-panel output (we control the dither pattern)
///   - tiny PNGs (1 byte per pixel + a 6-entry palette compresses very well)
fn downscale_supersampled(input: &[u8]) -> anyhow::Result<Vec<u8>> {
    use image::{imageops::FilterType, ImageReader};
    use std::io::Cursor;

    let img = ImageReader::with_format(Cursor::new(input), image::ImageFormat::Png)
        .decode()?;
    let (w, h) = (img.width(), img.height());
    let small = img.resize_exact(w / 2, h / 2, FilterType::Lanczos3).to_rgb8();

    let (indices, palette) = quantize_floyd_steinberg(&small);

    // 6 colors fit in 4 bits — pack two indices per byte to halve raw size
    // before zlib runs. PNG only supports power-of-two bit depths, so 4 is
    // the smallest legal one for 6 entries.
    let packed = pack_nibbles(&indices, small.width() as usize);

    let mut out = Vec::with_capacity(48 * 1024);
    {
        let mut encoder = png::Encoder::new(&mut out, small.width(), small.height());
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Four);
        encoder.set_palette(palette);
        encoder.set_compression(png::Compression::Best);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&packed)?;
    }
    Ok(out)
}

/// Pack one nibble per pixel, two pixels per byte, with the high nibble first.
/// Rows are padded to a whole byte (PNG requirement for sub-byte bit depths).
fn pack_nibbles(indices: &[u8], width: usize) -> Vec<u8> {
    let row_bytes = (width + 1) / 2;
    let height = indices.len() / width;
    let mut out = vec![0u8; row_bytes * height];
    for y in 0..height {
        for x in 0..width {
            let v = indices[y * width + x] & 0x0f;
            let dst = y * row_bytes + x / 2;
            if x & 1 == 0 {
                out[dst] |= v << 4;
            } else {
                out[dst] |= v;
            }
        }
    }
    out
}

/// 6-color palette matching the panel + the dashboard's accent colors.
/// Order is fixed so palette indices stay stable across renders.
const PALETTE: [[u8; 3]; 6] = [
    [0x00, 0x00, 0x00], // BLACK
    [0xff, 0xff, 0xff], // WHITE
    [0xc1, 0x12, 0x1f], // RED   (matches dashboard accent)
    [0xe5, 0xb8, 0x00], // YELLOW (matches dashboard warning)
    [0x00, 0x80, 0x40], // GREEN
    [0x10, 0x40, 0xa0], // BLUE
];

/// Quantize an RGB image to `PALETTE` with Floyd–Steinberg error diffusion.
/// Returns (per-pixel palette indices, flattened RGB palette bytes).
fn quantize_floyd_steinberg(img: &image::RgbImage) -> (Vec<u8>, Vec<u8>) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    // Working buffer in i16 so we can carry signed quantization error.
    let mut buf: Vec<[i16; 3]> = img
        .pixels()
        .map(|p| [p[0] as i16, p[1] as i16, p[2] as i16])
        .collect();

    let mut indices = vec![0u8; w * h];

    for y in 0..h {
        // Serpentine scan: alternate row direction so the dither texture
        // doesn't drift across the image.
        let ltr = y & 1 == 0;
        let xs: Box<dyn Iterator<Item = usize>> = if ltr {
            Box::new(0..w)
        } else {
            Box::new((0..w).rev())
        };
        for x in xs {
            let i = y * w + x;
            let old = buf[i];
            let (idx, chosen) = nearest_palette(old);
            indices[i] = idx as u8;
            let err = [old[0] - chosen[0] as i16, old[1] - chosen[1] as i16, old[2] - chosen[2] as i16];

            // Floyd–Steinberg distribution (mirrored on right-to-left rows):
            //         X    7
            //    3    5    1     (/16)
            let next_x: i32 = if ltr { 1 } else { -1 };
            let push = |buf: &mut [[i16; 3]], xx: i32, yy: usize, num: i16| {
                if xx < 0 || xx as usize >= w || yy >= h {
                    return;
                }
                let j = yy * w + xx as usize;
                for c in 0..3 {
                    buf[j][c] = (buf[j][c] + err[c] * num / 16).clamp(0, 255);
                }
            };
            push(&mut buf, x as i32 + next_x,     y,     7);
            push(&mut buf, x as i32 - next_x, y + 1,     3);
            push(&mut buf, x as i32,          y + 1,     5);
            push(&mut buf, x as i32 + next_x, y + 1,     1);
        }
    }

    let palette_bytes = PALETTE.iter().flatten().copied().collect();
    (indices, palette_bytes)
}

fn nearest_palette(px: [i16; 3]) -> (usize, [u8; 3]) {
    let mut best = 0usize;
    let mut best_d = i32::MAX;
    for (i, c) in PALETTE.iter().enumerate() {
        let dr = px[0] as i32 - c[0] as i32;
        let dg = px[1] as i32 - c[1] as i32;
        let db = px[2] as i32 - c[2] as i32;
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    (best, PALETTE[best])
}

async fn regenerate(state: &AppState) {
    let Ok(_guard) = state.render_lock.try_lock() else {
        info!("render already in progress, skipping");
        return;
    };

    let mock = DashData::mock();
    *state.data.write().await = if state.local_mode {
        mock
    } else {
        state.sources.fetch(&mock).await
    };

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

    // RENDER_TO=path.png → render a single PNG with mock data, write it, exit.
    // LOCAL_MODE=1       → serve normally but never fetch real data (mock only).
    let render_to = std::env::var("RENDER_TO").ok();
    let local_mode = render_to.is_some() || std::env::var("LOCAL_MODE").is_ok();

    // In RENDER_TO mode, bind to localhost on a random port — we just need
    // Chrome to be able to reach the page; nothing external should hit it.
    let addr = if render_to.is_some() {
        "127.0.0.1:0".to_string()
    } else {
        std::env::var("LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".to_string())
    };
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind failed");
    let bound = listener.local_addr().unwrap();
    let port = bound.port();

    let state = AppState {
        png_cache:    Arc::new(RwLock::new(Vec::new())),
        device_state: Arc::new(RwLock::new(DeviceState::default())),
        data:         Arc::new(RwLock::new(DashData::mock())),
        sources:      Arc::new(Sources::from_env()),
        port,
        render_lock:  Arc::new(tokio::sync::Mutex::new(())),
        local_mode,
    };

    let app = Router::new()
        .route("/api/setup",      get(api_setup))
        .route("/api/display",    get(api_display))
        .route("/api/log",        post(api_log))
        .route("/api/data",       get(api_data))
        .route("/dashboard.html", get(serve_dashboard_html))
        .route("/dashboard.png",  get(serve_png))
        .route("/refresh",        get(force_refresh))
        .route("/health",         get(health))
        .with_state(state.clone());

    if let Some(path) = render_to {
        // Run the server just long enough for a single in-process screenshot.
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        // Yield so the listener is actively accepting before Chrome connects.
        tokio::time::sleep(Duration::from_millis(100)).await;
        info!("rendering one frame to {path} (mock data, port {port})");
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
    info!("Preview  →  http://{bound}/dashboard.html");
    info!("Image    →  http://{bound}/dashboard.png");
    axum::serve(listener, app).await.expect("server error");
}
