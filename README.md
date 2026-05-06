# TRMNL//CYBERPUNK

A self-hosted [TRMNL](https://usetrmnl.com/) BYOS (Bring Your Own Server) dashboard for the Seeed e1002 800×480 Spectra 6-color e-ink display.

Instead of drawing pixels in Rust, the server renders a full HTML/CSS/JS dashboard page via headless Chromium and serves the resulting PNG to the device.

![CI](https://github.com/DiverOfDark/trmnl-cyberpunk/actions/workflows/ci.yml/badge.svg)

---

## Features

- Full TRMNL BYOS protocol — `/api/setup`, `/api/display`, `/api/log`
- Battery % and RSSI from the device are stored and shown on the dashboard
- Dashboard page (`/dashboard.html`) is a plain HTML file — tweak the design without touching Rust
- Live data endpoint at `/api/data` (JSON) — easy to wire up real homelab data sources
- Mock data: Norse-mythology hostnames, multi-day weather, calendar agenda, task list, budget categories, alert feed

### Dashboard panels

| Column | Panels |
|---|---|
| Left (240 px) | Hosts (CPU / RAM / disk bars) · Budget (per-category progress) |
| Middle (268 px) | Weather (current + 4-day forecast) · Tasks |
| Right (292 px) | Agenda · Alerts |

### Color palette — Spectra 6-color e-ink

| Role | Color |
|---|---|
| Background | `#000000` black |
| Text | `#ffffff` white |
| Headers / accent | `#0055cc` blue |
| Warnings / temperature | `#ccaa00` yellow |
| Errors / critical bars | `#cc0000` red |
| OK / good bars | `#007700` green |

---

## Quick start (Docker Compose)

```bash
git clone https://github.com/DiverOfDark/trmnl-cyberpunk
cd trmnl-cyberpunk

# Edit BASE_URL to your machine's LAN address so the device can reach it
docker compose up -d

# Preview in browser
open http://localhost:8080/dashboard.html

# Force an immediate re-render
curl http://localhost:8080/refresh
```

### Environment variables

| Variable | Default | Description |
|---|---|---|
| `BASE_URL` | `http://localhost:8080` | Public URL the TRMNL device uses to fetch `/dashboard.png` |
| `REFRESH_SECS` | `3600` | How often the dashboard re-renders (seconds) |
| `TRMNL_API_KEY` | `cyberpunk-byos` | API key returned to the device on `/api/setup` |
| `LISTEN` | `0.0.0.0:8080` | Bind address |
| `CHROME_PATH` | `/usr/bin/chromium` | Path to the Chromium binary |
| `RUST_LOG` | `trmnl_cyberpunk=info` | Log level |

---

## Kubernetes (Helm)

```bash
helm install trmnl-cyberpunk \
  oci://ghcr.io/diverofdark/charts/trmnl-cyberpunk \
  --set baseUrl=http://192.168.1.x:8080 \
  --set trmnlApiKey=your-secret-key
```

Key values:

```yaml
baseUrl: "http://192.168.1.x:8080"   # reachable from the TRMNL device
refreshSecs: "3600"
trmnlApiKey: "your-secret-key"
image:
  tag: "main"                          # or a semver tag like "0.1.0"
```

---

## Render pipeline

```
Device  →  GET /api/display  →  stores battery + rssi
                                spawns async re-render
                                returns /dashboard.png URL

Re-render:
  1. chromiumoxide launches chromium --headless --no-sandbox
  2. browser opens http://127.0.0.1:{port}/dashboard.html
  3. JS fetches /api/data  →  populates all panels
  4. JS sets <html data-ready=true>
  5. server takes 800×480 PNG screenshot
  6. PNG cached → served on next GET /dashboard.png
```

The first render fires 500 ms after startup. Subsequent renders happen every `REFRESH_SECS` and also on every device poll (rate-limited: skipped if a render is already in progress).

---

## Customising the dashboard

The entire visual design lives in one file:

```
templates/dashboard.html
```

It is embedded into the binary at compile time (`include_str!`), so no runtime file path is needed. Edit it and push — the CI pipeline rebuilds and pushes a new image automatically.

To add a real data source, replace `DashData::mock()` in `src/data.rs` with a real HTTP fetch (e.g. from Prometheus, Nextcloud, a weather API, etc.).

---

## API endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/setup` | TRMNL device provisioning |
| `GET` | `/api/display` | TRMNL device poll — returns image URL |
| `POST` | `/api/log` | TRMNL device diagnostic logs |
| `GET` | `/api/data` | Dashboard data as JSON |
| `GET` | `/dashboard.html` | The rendered dashboard page |
| `GET` | `/dashboard.png` | Cached 800×480 PNG for the device |
| `GET` | `/refresh` | Force an immediate re-render |
| `GET` | `/health` | Health check |

---

## Development

```bash
# Install Chromium (needed at runtime)
sudo apt install chromium        # Debian/Ubuntu
brew install --cask chromium     # macOS

# Run locally
CHROME_PATH=/usr/bin/chromium cargo run

# Open dashboard preview
open http://localhost:8080/dashboard.html
```

The `templates/` directory is watched by the binary via `include_str!` — restart the server after editing the HTML to pick up changes.

---

## CI/CD

GitHub Actions (`.github/workflows/ci.yml`):

1. **test** — `cargo check`, `clippy -D warnings`, `cargo test`
2. **docker** — builds and pushes to `ghcr.io/diverofdark/trmnl-cyberpunk` with tags `main`, `sha-<hash>`, and semver on `v*` tags
3. **helm** — packages and pushes chart to `oci://ghcr.io/diverofdark/charts/trmnl-cyberpunk`

Tag a release to get pinned versioned artifacts:

```bash
git tag v0.2.0 && git push origin v0.2.0
```
