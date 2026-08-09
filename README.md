# TRMNL//CYBERPUNK

A self-hosted [TRMNL](https://usetrmnl.com/) BYOS (Bring Your Own Server) dashboard for the Seeed e1002 800×480 Spectra 6-color e-ink display.

The server renders the dashboard pixel-by-pixel in Rust (`embedded-graphics` + u8g2 bitmap fonts) into a 4-bit indexed PNG using the panel's measured 6-color palette, then serves it to the device. No browser, no fonts on disk, no template files.

![Dashboard preview rendered with mock data](dashboard.png)

*Mock-data render — produced by `RENDER_TO=dashboard.png cargo run`. Colors look muted on a normal monitor because the PNG palette uses the panel's actual measured ink RGB values, not vivid sRGB equivalents.*

![CI](https://github.com/DiverOfDark/trmnl-cyberpunk/actions/workflows/ci.yml/badge.svg)

---

## Features

- Full TRMNL BYOS protocol — `/api/setup`, `/api/display`, `/api/log`
- Battery % and RSSI from the device are shown in the dashboard header
- 4-bit indexed PNG output: every pixel is exactly one of the six panel inks (no dithering, no antialiasing) so the panel renders what we drew
- Pluggable upstreams: Prometheus + Alertmanager, Nextcloud CalDAV, ActualBudget, Open-Meteo, [trackhound](https://github.com/DiverOfDark/trackhound). Mock fallbacks for everything when env vars are blank
- Norse-mythology mock hostnames, multi-day weather, calendar agenda, budget categories, alert feed

### Dashboard panels

| Column | Width | Panels |
|---|---|---|
| Left | 260 px | WX (current temp + 4-day forecast, full height) |
| Middle | 320 px | AGENDA (top) · BUDGET (bottom) |
| Right | 220 px | SYS — hosts (top) · OPS — alerts (bottom) |

### Color palette — Spectra 6-color e-ink

The panel uses six fixed inks. The values below are the *measured* on-panel RGB values (from the firmware's calibration table), which is what we encode into the PNG palette. They look muted/olive on a normal monitor but accurately preview what the panel actually shows.

| Role | Color |
|---|---|
| Background | `#020202` black |
| Body / fills | `#b3b6ab` "white" |
| Headers / accent | `#002f6b` blue |
| Warnings / temperature | `#cdca00` yellow |
| Errors / critical bars | `#750a00` red |
| OK / good bars | `#214528` green |

---

## Quick start (Docker Compose)

```bash
git clone https://github.com/DiverOfDark/trmnl-cyberpunk
cd trmnl-cyberpunk

# Edit BASE_URL to your machine's LAN address so the device can reach it
docker compose up -d

# Preview in browser
open http://localhost:8080/dashboard.png

# Force an immediate upstream re-fetch
curl http://localhost:8080/refresh
```

### Environment variables

| Variable | Default | Description |
|---|---|---|
| `BASE_URL` | `http://localhost:8080` | Public URL the TRMNL device uses to fetch the PNG |
| `REFRESH_SECS` | `3600` | Poll cadence reported to the firmware (seconds) |
| `FETCH_INTERVAL_SECS` | `300` | How often the background task refetches upstreams (seconds). Independent of the device's poll cadence |
| `FETCH_TIMEOUT_SECS` | `45` | Per-source ceiling on one upstream pull; a source that overruns keeps its last values and is marked stale |
| `TRMNL_API_KEY` | `cyberpunk-byos` | API key returned to the device on `/api/setup` |
| `LISTEN` | `0.0.0.0:8080` | Bind address |
| `RUST_LOG` | `trmnl_cyberpunk=info` | Log level |
| `LOCAL_MODE` | _(unset)_ | If set, never fetch upstreams — serve mock data only |
| `RENDER_TO` | _(unset)_ | If set to a path, render one PNG with mock data, write it, and exit |

Upstream-specific env vars (leave blank to use the matching mock data) are documented in `docker-compose.yml`.

### The budget panel

The hero is **safe-to-spend per day**: what's left across the day-to-day envelopes divided by the days until payday. Payday is derived from when large inflows have actually landed over the last three months — a salary paid on the last working day is recognised as month-end rather than pinned to a date that drifts.

Below it, **this month against the last three**, measured at the same day of the month so it's like for like, and stated in euros: `€531 MORE THAN USUAL`, with the two figures behind it on the line beneath. A bare percentage asks the reader to reconstruct a baseline that isn't on screen; euros are the unit every other figure on the panel already uses. A delta inside ±€25 reads as `ON USUAL PACE`, since naming rounding noise invites false precision. Savings goals are excluded from both sides — money moved into a goal is allocation, not consumption, and one lumpy investment transfer would otherwise swamp the comparison. Nothing is shown before the 3rd of the month, when the baseline is still one or two bills.

This replaced a spent-vs-budget pace bar. Budgets tend to be set below what a category actually costs, so measuring against them mostly reported that the budget was wrong; measuring against recent behavior reports whether *this* month is different, which is the question worth a wall panel.

Then **total capital** — on-budget cash plus off-budget holdings — over roughly twelve month-ends, read from Actual's `balancehistory` endpoint (one small request per account, no transaction arithmetic). Transfers between your own accounts cancel out by construction, so this line is immune to the accounting artifacts that distort income-vs-spend charts.

It is a line, not bars, and deliberately so. The series sits in a narrow band far from zero. A bar encodes quantity as *length*, so it carries an implicit zero and truncating its axis lies — drawn honestly from zero, a 30% move compresses into the top quarter and every bar looks identical. A line encodes quantity as *position*, where a truncated axis is conventional and readable; the absolute value and the year delta are printed alongside so the missing baseline can't mislead. The final segment is dashed because the month in progress is pre-payday for most of its length, and that predictable dip shouldn't read as a real decline.

At most two envelopes are flagged, and only for two reasons:

- **OVER** — Actual's `balance` (carryover + budgeted − spent) is already negative, i.e. genuinely out of money. Using `balance` rather than `cap − spent` means an envelope that overshot this month but is still covered by accumulated funds isn't flagged.
- **HOT** — spending is ≥40% above *this envelope's own* median spend by this same day-of-month over the prior three months, by at least €25, from the 4th of the month onward. Comparing an envelope to its own history catches a change in behavior; comparing it to a budget only catches an envelope that was set too low.

Classification matches the Actual category *group* name against `ACTUALBUDGET_FIXED_GROUPS` / `ACTUALBUDGET_VARIABLE_GROUPS` / `ACTUALBUDGET_SAVINGS_GROUPS` (comma-separated, case-insensitive substrings; sensible English defaults built in), falling back to a per-category transaction-count heuristic when a group name is unrecognized. Income groups are skipped so inflows don't pollute the spend rollups.

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
                                returns dashboard PNG URL
                                  (BASE_URL/dashboard/{epoch})

Per-request render (every device fetch):
  1. clone the latest fetched data
  2. re-stamp clock-derived fields (time, date, motto, sync markers)
  3. draw straight into an 800×480 indexed framebuffer
  4. encode as 4-bit indexed PNG with the panel palette
  5. bump last_render_at → /api/display URL gets a new {epoch} suffix,
     defeating the firmware's 24h dedupe cache
```

A background task refetches upstreams every `FETCH_INTERVAL_SECS`, with the first run fired at startup. Requests never fetch: `/dashboard.png` renders from the cached snapshot and returns in tens of milliseconds regardless of how slow (or unreachable) the upstreams are. `/refresh` forces a synchronous pull; concurrent pulls are deduped via a mutex.

### Staleness

Each source's last successful pull is timestamped. When a source fails, its panel keeps the last good values rather than blanking — but the panel header swaps its `// NN` sequence label for a red **`STALE 12m`** tag (or **`NO DATA`** if it has never succeeded since boot), and the footer switches from `ONLINE` to `DEGRADED:` plus the affected panels. Sources whose env vars are unset are opted out, not stale, and are never marked. Mock data appears only under `LOCAL_MODE` / `RENDER_TO` — never as an outage fallback, so nothing on the panel is invented.

---

## Customising the dashboard

The entire visual design lives in two files:

- `src/render.rs` — the indexed-color canvas, palette, and pixel primitives
- `src/dashboard.rs` — panel layout, fonts, and per-section rendering

There is no HTML template — every shape on the panel is drawn by Rust code. To preview a change locally without a device:

```bash
RENDER_TO=/tmp/dash.png cargo run
open /tmp/dash.png
```

To add a real data source, extend `Sources::fetch` in `src/fetch.rs`. The mock data in `DashData::mock()` (in `src/data.rs`) is the fallback whenever an upstream's env vars are unset or the fetch fails.

---

## Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/setup` | TRMNL device provisioning |
| `GET` | `/api/display` | TRMNL device poll — returns image URL |
| `POST` | `/api/log` | TRMNL device diagnostic logs |
| `GET` | `/dashboard.png` | Rendered fresh on every request (no cache) |
| `GET` | `/dashboard/{epoch}` | Same handler; `{epoch}` is the cache-buster the firmware sees |
| `GET` | `/refresh` | Force an immediate upstream re-fetch |
| `GET` | `/health` | Health check |

---

## Development

```bash
# Run locally
cargo run

# Open dashboard preview
open http://localhost:8080/dashboard.png

# One-shot render (no server)
RENDER_TO=/tmp/dash.png cargo run
```

No system dependencies beyond a Rust toolchain — fonts are bundled into the `u8g2-fonts` crate, and the renderer writes PNGs directly via the `png` crate.

---

## CI/CD

GitHub Actions (`.github/workflows/ci.yml`):

1. **test** — `cargo check`, `clippy -D warnings`, `cargo test`
2. **docker** — builds and pushes to `ghcr.io/diverofdark/trmnl-cyberpunk` with tags `main`, `sha-<hash>`, and semver on `v*` tags
3. **helm** — packages and pushes the chart to `oci://ghcr.io/diverofdark/charts/trmnl-cyberpunk`

Tag a release to get pinned versioned artifacts:

```bash
git tag v0.2.0 && git push origin v0.2.0
```
