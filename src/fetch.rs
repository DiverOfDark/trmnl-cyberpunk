use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};
use reqwest::Client;
use serde_json::Value;
use tracing::warn;

use crate::data::*;

// ── Sources config ─────────────────────────────────────────────────────────

/// Sentinel error returned when an integration's required env var is empty
/// or unset. The parent `fetch()` distinguishes this from a real fetch
/// failure: unconfigured → empty data (don't fake it), failed → mock
/// fallback (so a transient outage doesn't blank the dashboard).
#[derive(Debug, Clone, Copy)]
pub struct NotConfigured(pub &'static str);

impl std::fmt::Display for NotConfigured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} not configured", self.0)
    }
}

impl std::error::Error for NotConfigured {}

fn is_not_configured(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NotConfigured>().is_some()
}

pub struct Sources {
    client: Client,
    pub prometheus: String,
    pub alertmanager: String,
    pub actualbudget_url: String,
    pub actualbudget_key: String,
    pub actualbudget_sync_id: String,
    pub actualbudget_password: String,
    pub ics_urls: Vec<String>,
    pub weather_lat: f64,
    pub weather_lon: f64,
    pub weather_tz: String,
}

impl Sources {
    pub fn from_env() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                // Outlook published-calendar endpoints (and a few other ICS
                // hosts) refuse the default reqwest user-agent and respond
                // with TLS-level resets that surface here as a bare
                // "error sending request". A browser-shaped UA gets through.
                .user_agent("Mozilla/5.0 (compatible; trmnl-cyberpunk/0.1; +https://github.com/DiverOfDark/trmnl-cyberpunk)")
                .build()
                .unwrap(),
            // All URL fields default to empty; the helm chart and
            // docker-compose pass "" for unset values. An empty URL means
            // the integration is opted-out and the fetch fn returns
            // `NotConfigured` instead of attempting a request.
            prometheus:       std::env::var("PROMETHEUS_URL").unwrap_or_default(),
            alertmanager:     std::env::var("ALERTMANAGER_URL").unwrap_or_default(),
            actualbudget_url: std::env::var("ACTUALBUDGET_URL").unwrap_or_default(),
            actualbudget_key: std::env::var("ACTUALBUDGET_KEY").unwrap_or_default(),
            actualbudget_sync_id: std::env::var("ACTUALBUDGET_SYNC_ID").unwrap_or_default(),
            actualbudget_password: std::env::var("ACTUALBUDGET_PASSWORD").unwrap_or_default(),
            ics_urls: std::env::var("ICS_URLS")
                .unwrap_or_default()
                .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            weather_lat: std::env::var("WEATHER_LAT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(52.50),
            weather_lon: std::env::var("WEATHER_LON")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(13.33),
            weather_tz: std::env::var("WEATHER_TZ")
                .unwrap_or_else(|_| "Europe/Berlin".into()),
        }
    }

    // Fetch all data concurrently; fall back to mock values per section on error.
    pub async fn fetch(&self, mock: &DashData) -> DashData {
        let (hosts_r, cluster_r, weather_r, alerts_r, budget_r, agenda_r) = tokio::join!(
            self.hosts(),
            self.cluster(),
            self.weather(),
            self.alerts(),
            self.budget(),
            self.agenda(),
        );

        // Per-section dispatch:
        //   Ok(data)              → use real data
        //   Err(NotConfigured)    → integration opted out; show empty/None
        //                            instead of mock so the dashboard stays
        //                            honest about what isn't wired up
        //   Err(other)            → integration is configured but the
        //                            request failed; mock fallback so a
        //                            transient outage doesn't blank the
        //                            panel (and we log the actual error)
        let hosts = match hosts_r {
            Ok(h) => h,
            Err(e) if is_not_configured(&e) => Vec::new(),
            Err(e) => { warn!("hosts fetch failed: {e:#}"); mock.hosts.clone() }
        };
        let cluster = match cluster_r {
            Ok(c) => Some(c),
            Err(e) if is_not_configured(&e) => None,
            Err(e) => { warn!("cluster fetch failed: {e:#}"); mock.cluster.clone() }
        };
        let weather = match weather_r {
            Ok(w) => Some(w),
            Err(e) if is_not_configured(&e) => None,
            Err(e) => { warn!("weather fetch failed: {e:#}"); Some(mock.weather.clone().unwrap_or_default()) }
        };
        let alerts = match alerts_r {
            Ok(a) => a,
            Err(e) if is_not_configured(&e) => Vec::new(),
            Err(e) => { warn!("alerts fetch failed: {e:#}"); mock.alerts.clone() }
        };
        let budget = match budget_r {
            Ok(b) => Some(b),
            Err(e) if is_not_configured(&e) => None,
            Err(e) => { warn!("budget fetch failed: {e:#}"); mock.budget.clone() }
        };
        let agenda = match agenda_r {
            Ok(a) => a,
            Err(e) if is_not_configured(&e) => Vec::new(),
            Err(e) => { warn!("agenda fetch failed: {e:#}"); mock.agenda.clone() }
        };

        let now = Local::now();
        let months = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN",
            "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        let dow = match now.weekday() {
            Weekday::Sun => "SUN", Weekday::Mon => "MON", Weekday::Tue => "TUE",
            Weekday::Wed => "WED", Weekday::Thu => "THU", Weekday::Fri => "FRI",
            Weekday::Sat => "SAT",
        };
        let month = months[(now.month() - 1) as usize];
        let refresh_secs: i64 = std::env::var("REFRESH_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(3600);
        let next = now + chrono::Duration::seconds(refresh_secs);

        DashData {
            time: now.format("%H:%M").to_string(),
            date: format!("{:02} {} {}", now.day(), month, now.year()),
            date_dow: dow.into(),
            motto: mock.motto.clone(),
            last_sync: now.format("%H:%M").to_string(),
            next_sync: next.format("%H:%M").to_string(),
            hosts,
            cluster,
            weather,
            agenda,
            tasks: mock.tasks.clone(),
            budget,
            alerts,
        }
    }
}

// ── Prometheus helpers ─────────────────────────────────────────────────────

async fn prom_query(client: &Client, base: &str, query: &str) -> Result<Vec<(String, f64)>> {
    let url = format!("{}/api/v1/query", base);
    let resp: Value = client
        .get(&url)
        .query(&[("query", query)])
        .send()
        .await?
        .json()
        .await?;

    let results = resp["data"]["result"]
        .as_array()
        .ok_or_else(|| anyhow!("no result array in prometheus response"))?;

    let mut out = Vec::new();
    for r in results {
        let instance = r["metric"]["instance"].as_str()
            .or_else(|| r["metric"]["node"].as_str())
            .unwrap_or("unknown");
        let host = instance.split(':').next().unwrap_or(instance).to_string();
        let val: f64 = r["value"][1]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        out.push((host, val));
    }
    Ok(out)
}

fn to_map(pairs: Vec<(String, f64)>) -> HashMap<String, f64> {
    pairs.into_iter().collect()
}

/// Run a Prometheus instant query and pull two labels off each sample:
/// the `instance` label (host key, normalized to drop the `:port` suffix)
/// and `label_name`. Used to map IP-shaped `instance` values to readable
/// `nodename` from `node_uname_info`.
async fn prom_label_pairs(
    client: &Client,
    base: &str,
    query: &str,
    label_name: &str,
) -> Result<Vec<(String, String)>> {
    let url = format!("{}/api/v1/query", base);
    let resp: Value = client
        .get(&url)
        .query(&[("query", query)])
        .send()
        .await?
        .json()
        .await?;
    let mut out = Vec::new();
    if let Some(arr) = resp["data"]["result"].as_array() {
        for r in arr {
            let inst = r["metric"]["instance"].as_str().unwrap_or("");
            let key = inst.split(':').next().unwrap_or(inst).to_string();
            let val = r["metric"][label_name].as_str().unwrap_or("").to_string();
            if !key.is_empty() && !val.is_empty() {
                out.push((key, val));
            }
        }
    }
    Ok(out)
}

// ── Host metrics ──────────────────────────────────────────────────────────

impl Sources {
    async fn hosts(&self) -> Result<Vec<HostData>> {
        if self.prometheus.is_empty() {
            return Err(NotConfigured("PROMETHEUS_URL").into());
        }
        let (cpu_r, temp_r, ram_r, disk_r, uptime_r, l1_r, l5_r, l15_r, uname_r) = tokio::join!(
            prom_query(&self.client, &self.prometheus,
                r#"100 - (avg by(instance) (rate(node_cpu_seconds_total{mode="idle"}[5m])) * 100)"#),
            prom_query(&self.client, &self.prometheus,
                r#"avg by(instance) (node_hwmon_temp_celsius{chip=~".*coretemp.*|.*k10temp.*|.*zenpower.*"})"#),
            prom_query(&self.client, &self.prometheus,
                r#"100 * (1 - node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)"#),
            prom_query(&self.client, &self.prometheus,
                r#"100 * (1 - node_filesystem_avail_bytes{mountpoint="/",fstype!~"rootfs|tmpfs"} / node_filesystem_size_bytes{mountpoint="/",fstype!~"rootfs|tmpfs"})"#),
            prom_query(&self.client, &self.prometheus,
                r#"(time() - node_boot_time_seconds) / 86400"#),
            prom_query(&self.client, &self.prometheus, "node_load1"),
            prom_query(&self.client, &self.prometheus, "node_load5"),
            prom_query(&self.client, &self.prometheus, "node_load15"),
            // Map IP-shaped `instance` labels (`192.168.1.10:9100`) to real
            // hostnames via node_uname_info's `nodename` label so the panel
            // shows "asgard" instead of "192.168.1.10".
            prom_label_pairs(
                &self.client, &self.prometheus,
                "node_uname_info", "nodename",
            ),
        );

        let cpu_map    = to_map(cpu_r.unwrap_or_default());
        let temp_map   = to_map(temp_r.unwrap_or_default());
        let ram_map    = to_map(ram_r.unwrap_or_default());
        let disk_map   = to_map(disk_r.unwrap_or_default());
        let uptime_map = to_map(uptime_r.unwrap_or_default());
        let l1_map     = to_map(l1_r.unwrap_or_default());
        let l5_map     = to_map(l5_r.unwrap_or_default());
        let l15_map    = to_map(l15_r.unwrap_or_default());
        let uname_map: HashMap<String, String> = uname_r.unwrap_or_default().into_iter().collect();

        // Union of all known instances (prefer uptime as the canonical set)
        let mut hosts_set: std::collections::BTreeSet<String> =
            uptime_map.keys().cloned().collect();
        for map in [&cpu_map, &ram_map, &disk_map] {
            hosts_set.extend(map.keys().cloned());
        }

        if hosts_set.is_empty() {
            return Err(anyhow!("no hosts found in prometheus"));
        }

        let hosts = hosts_set
            .into_iter()
            .map(|key| {
                let display_name = uname_map.get(&key).cloned().unwrap_or_else(|| key.clone());
                HostData {
                    cpu:         cpu_map.get(&key).copied().unwrap_or(0.0).round() as u8,
                    cpu_temp:    temp_map.get(&key).copied().unwrap_or(0.0).round() as u8,
                    ram_pct:     ram_map.get(&key).copied().unwrap_or(0.0).round() as u8,
                    disk_pct:    disk_map.get(&key).copied().unwrap_or(0.0).round() as u8,
                    uptime_days: uptime_map.get(&key).copied().unwrap_or(0.0) as u32,
                    load: [
                        l1_map.get(&key).copied().unwrap_or(0.0) as f32,
                        l5_map.get(&key).copied().unwrap_or(0.0) as f32,
                        l15_map.get(&key).copied().unwrap_or(0.0) as f32,
                    ],
                    name: display_name,
                }
            })
            .collect();

        Ok(hosts)
    }

    /// Cluster-wide rollups for the SYS panel summary cells. Computed via
    /// dedicated PromQL queries because:
    ///   - CPU/RAM use `sum(consumed) / sum(total)`, not the arithmetic
    ///     mean of per-host percentages (a small idle node would otherwise
    ///     pull the cluster figure down).
    ///   - Disk reads from Ceph (`ceph_cluster_total_used_bytes /
    ///     ceph_cluster_total_bytes`) because in k8s the per-node `/`
    ///     filesystem is the container rootfs and reports near-zero.
    async fn cluster(&self) -> Result<ClusterMetrics> {
        if self.prometheus.is_empty() {
            return Err(NotConfigured("PROMETHEUS_URL").into());
        }
        let (cpu_r, ram_r, disk_r) = tokio::join!(
            prom_query(
                &self.client,
                &self.prometheus,
                r#"100 * (1 - sum(rate(node_cpu_seconds_total{mode="idle"}[5m])) / sum(rate(node_cpu_seconds_total[5m])))"#,
            ),
            prom_query(
                &self.client,
                &self.prometheus,
                "100 * (1 - sum(node_memory_MemAvailable_bytes) / sum(node_memory_MemTotal_bytes))",
            ),
            prom_query(
                &self.client,
                &self.prometheus,
                "100 * sum(ceph_cluster_total_used_bytes) / sum(ceph_cluster_total_bytes)",
            ),
        );

        // Each query returns a single scalar wrapped in a Vec; pick the
        // first sample if present, default to 0 if nothing came back.
        let pick = |r: Result<Vec<(String, f64)>>| -> f64 {
            r.ok()
                .and_then(|v| v.into_iter().next())
                .map(|(_, x)| x)
                .unwrap_or(0.0)
        };
        let to_pct = |v: f64| -> u8 {
            if v.is_nan() || v.is_infinite() { 0 } else { v.clamp(0.0, 100.0).round() as u8 }
        };

        Ok(ClusterMetrics {
            cpu_pct:  to_pct(pick(cpu_r)),
            ram_pct:  to_pct(pick(ram_r)),
            disk_pct: to_pct(pick(disk_r)),
        })
    }
}

// ── Weather (Open-Meteo) ───────────────────────────────────────────────────

fn wmo_to_cond(code: u64) -> &'static str {
    match code {
        0..=1           => "SUN",
        2..=3           => "CLOUD",
        45..=48         => "FOG",
        51..=67         => "RAIN",
        71..=77         => "SNOW",
        80..=82         => "RAIN",
        85..=86         => "SNOW",
        95..=99         => "STORM",
        _               => "CLOUD",
    }
}

fn dow_from_date(date_str: &str) -> &'static str {
    use chrono::NaiveDate;
    let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else { return "---" };
    match d.weekday() {
        Weekday::Sun => "SUN", Weekday::Mon => "MON", Weekday::Tue => "TUE",
        Weekday::Wed => "WED", Weekday::Thu => "THU", Weekday::Fri => "FRI",
        Weekday::Sat => "SAT",
    }
}

impl Sources {
    async fn weather(&self) -> Result<WeatherData> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast\
             ?latitude={}&longitude={}&timezone={}\
             &current=temperature_2m,weather_code\
             &daily=weather_code,temperature_2m_max,temperature_2m_min\
             &forecast_days=5",
            self.weather_lat, self.weather_lon,
            urlencoding::encode(&self.weather_tz),
        );

        let resp: Value = self.client.get(&url).send().await?.json().await?;

        let cur  = &resp["current"];
        let daily = &resp["daily"];

        let temp_c    = cur["temperature_2m"].as_f64().unwrap_or(0.0).round() as i8;
        let cur_code  = cur["weather_code"].as_u64().unwrap_or(0);
        let condition = wmo_to_cond(cur_code).to_string();

        let times: Vec<&str> = daily["time"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let codes:  Vec<u64> = daily["weather_code"].as_array()
            .map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0)).collect())
            .unwrap_or_default();
        let maxs: Vec<f64> = daily["temperature_2m_max"].as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_default();
        let mins: Vec<f64> = daily["temperature_2m_min"].as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
            .unwrap_or_default();

        let today_hi = maxs.first().copied().unwrap_or(0.0).round() as i8;
        let today_lo = mins.first().copied().unwrap_or(0.0).round() as i8;

        // Skip today (index 0) for the forecast strip
        let forecast = times.iter().enumerate().skip(1).take(4).map(|(i, date)| {
            WeatherDay {
                day:  dow_from_date(date).to_string(),
                hi:   maxs.get(i).copied().unwrap_or(0.0).round() as i8,
                lo:   mins.get(i).copied().unwrap_or(0.0).round() as i8,
                cond: wmo_to_cond(*codes.get(i).unwrap_or(&0)).to_string(),
            }
        }).collect();

        Ok(WeatherData {
            temp_c,
            condition,
            hi: today_hi,
            lo: today_lo,
            forecast,
        })
    }
}

// ── Alertmanager ──────────────────────────────────────────────────────────

impl Sources {
    async fn alerts(&self) -> Result<Vec<Alert>> {
        if self.alertmanager.is_empty() {
            return Err(NotConfigured("ALERTMANAGER_URL").into());
        }
        let url = format!("{}/api/v2/alerts?silenced=false&inhibited=false", self.alertmanager);
        let resp: Value = self.client.get(&url).send().await?.json().await?;

        let arr = resp.as_array()
            .ok_or_else(|| anyhow!("alertmanager response not array"))?;

        let mut alerts: Vec<Alert> = arr
            .iter()
            // Drop the Alertmanager Watchdog heartbeat — it always fires
            // by design, so it's pure noise on the panel.
            .filter(|a| a["labels"]["alertname"].as_str() != Some("Watchdog"))
            .map(|a| {
                let severity = a["labels"]["severity"].as_str().unwrap_or("info");
                let level = match severity {
                    "critical" | "error" => "ERR",
                    "warning"            => "WRN",
                    _                    => "INF",
                }.to_string();

                let starts_at = a["startsAt"].as_str().unwrap_or("");
                let time = chrono::DateTime::parse_from_rfc3339(starts_at)
                    .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
                    .unwrap_or_else(|_| "--:--".to_string());

                // Prefer alertname (short, like "TargetDown") over the
                // verbose summary, prefixed with the offending instance/pod
                // when available so duplicate alertname rows still convey
                // *which* target is down.
                let alertname = a["labels"]["alertname"].as_str().unwrap_or("");
                let target = a["labels"]["instance"].as_str()
                    .or_else(|| a["labels"]["pod"].as_str())
                    .or_else(|| a["labels"]["namespace"].as_str())
                    .unwrap_or("");
                let message = if !alertname.is_empty() && !target.is_empty() {
                    format!("{alertname} · {target}").chars().take(60).collect()
                } else if !alertname.is_empty() {
                    alertname.chars().take(60).collect()
                } else {
                    a["annotations"]["summary"]
                        .as_str()
                        .or_else(|| a["annotations"]["description"].as_str())
                        .unwrap_or("unknown alert")
                        .chars().take(60).collect()
                };

                Alert { level, time, message }
            })
            // Render-side caps at panel-fit; this is just a sanity bound.
            .take(20)
            .collect();

        // Sort: ERR first, then WRN, then INF
        alerts.sort_by_key(|a| match a.level.as_str() { "ERR" => 0, "WRN" => 1, _ => 2 });
        Ok(alerts)
    }
}

/// Render an error and its `source()` chain on a single line so we can see
/// the *actual* cause (TLS handshake failure, connect refused, etc.) instead
/// of just reqwest's outermost "error sending request" wrapper.
fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut current = e.source();
    while let Some(cause) = current {
        out.push_str(" → ");
        out.push_str(&cause.to_string());
        current = cause.source();
    }
    out
}

// ── ActualBudget HTTP API ─────────────────────────────────────────────────

/// GET an x-api-key-authed endpoint and parse JSON, with detailed error
/// context on non-2xx or non-JSON bodies — much easier to debug than
/// reqwest's bare "error decoding response body".
///
/// `encryption_password` is sent as `budget-encryption-password` when non-empty;
/// it's required on the first request against an end-to-end encrypted budget
/// and harmless on unencrypted ones.
async fn get_json(
    client: &Client,
    url: &str,
    api_key: &str,
    encryption_password: &str,
) -> Result<Value> {
    let mut req = client.get(url).header("x-api-key", api_key);
    if !encryption_password.is_empty() {
        req = req.header("budget-encryption-password", encryption_password);
    }
    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        let snippet: String = body.chars().take(200).collect();
        return Err(anyhow!("{url} → {status}: {snippet}"));
    }
    serde_json::from_str(&body).map_err(|e| {
        let snippet: String = body.chars().take(200).collect();
        anyhow!("{url} → JSON decode failed ({e}); body starts: {snippet}")
    })
}

impl Sources {
    async fn budget(&self) -> Result<BudgetData> {
        if self.actualbudget_url.is_empty() {
            return Err(NotConfigured("ACTUALBUDGET_URL").into());
        }
        if self.actualbudget_key.is_empty() {
            return Err(NotConfigured("ACTUALBUDGET_KEY").into());
        }
        if self.actualbudget_sync_id.is_empty() {
            return Err(NotConfigured("ACTUALBUDGET_SYNC_ID").into());
        }

        let now   = Local::now();
        let month = format!("{}-{:02}", now.year(), now.month());

        let month_data: Value = get_json(
            &self.client,
            &format!(
                "{}/budgets/{}/months/{}",
                self.actualbudget_url, self.actualbudget_sync_id, month,
            ),
            &self.actualbudget_key,
            &self.actualbudget_password,
        )
        .await?;

        let cats_raw = month_data["data"]["categoryGroups"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let mut cats: Vec<BudgetCat> = Vec::new();
        let mut total_spent: u32 = 0;
        let mut total_cap: u32 = 0;

        // Flatten all category groups → categories
        for group in &cats_raw {
            if let Some(cat_list) = group["categories"].as_array() {
                for cat in cat_list {
                    let name  = cat["name"].as_str().unwrap_or("?");
                    let spent = (cat["spent"].as_f64().unwrap_or(0.0).abs() / 100.0).round() as u32;
                    let cap   = (cat["budgeted"].as_f64().unwrap_or(0.0) / 100.0).round() as u32;
                    if cap > 0 || spent > 0 {
                        total_spent += spent;
                        total_cap   += cap;
                        cats.push(BudgetCat {
                            label: name.chars().take(5).collect::<String>().to_uppercase(),
                            spent,
                            cap,
                        });
                    }
                }
            }
        }

        // Keep top 5 by cap
        cats.sort_by_key(|c| std::cmp::Reverse(c.cap));
        cats.truncate(5);

        let months = ["JAN","FEB","MAR","APR","MAY","JUN","JUL","AUG","SEP","OCT","NOV","DEC"];
        let month_label = format!("{} '{}", months[(now.month()-1) as usize], &now.format("%Y").to_string()[2..]);

        Ok(BudgetData { month_label, spent: total_spent, cap: total_cap, cats })
    }
}

// ── ICS calendar feeds ────────────────────────────────────────────────────

impl Sources {
    async fn agenda(&self) -> Result<Vec<AgendaItem>> {
        if self.ics_urls.is_empty() {
            return Err(NotConfigured("ICS_URLS").into());
        }

        let today = Local::now().date_naive();
        let fetches = self.ics_urls.iter().map(|url| {
            let client = self.client.clone();
            let url = url.clone();
            async move {
                let body = client.get(&url).send().await?.text().await?;
                Ok::<_, reqwest::Error>(body)
            }
        });
        let results = futures::future::join_all(fetches).await;

        let mut items = Vec::new();
        for (url, r) in self.ics_urls.iter().zip(results) {
            match r {
                Ok(body) => items.extend(parse_ical_events(&body, today)),
                Err(e) => warn!("ics fetch failed for {url}: {}", error_chain(&e)),
            }
        }

        // De-dup by (time, title) — a calendar invited to multiple feeds duplicates.
        items.sort_by(|a, b| a.time.cmp(&b.time).then(a.title.cmp(&b.title)));
        items.dedup_by(|a, b| a.time == b.time && a.title == b.title);
        // Generous fetcher cap; renderer caps to whatever fits the panel.
        items.truncate(20);

        // Empty is a valid result (no meetings today) — return Ok so the
        // parent renders an empty agenda instead of the mock fallback.
        Ok(items)
    }
}

fn parse_ical_events(content: &str, today: NaiveDate) -> Vec<AgendaItem> {
    let unfolded = unfold(content);
    let mut items = Vec::new();

    for vevent_block in unfolded.split("BEGIN:VEVENT") {
        if !vevent_block.contains("END:VEVENT") {
            continue;
        }
        let block = &vevent_block[..vevent_block.find("END:VEVENT").unwrap_or(vevent_block.len())];

        let summary    = ical_field(block, "SUMMARY").unwrap_or_default();
        let categories = ical_field_value(block, "CATEGORIES").unwrap_or_default();
        let Some((tzid, dtstart)) = ical_dtstart(block) else { continue };

        if summary.is_empty() || dtstart.is_empty() {
            continue;
        }

        let Some((start_local, all_day)) = parse_ical_dtstart(&dtstart, tzid.as_deref()) else { continue };
        if start_local.date_naive() != today {
            continue;
        }

        let time = if all_day {
            "ALL".to_string()
        } else {
            start_local.format("%H:%M").to_string()
        };

        let tag = if !categories.is_empty() {
            categories.split(',').next().unwrap_or("").trim()
                .chars().take(4).collect::<String>().to_uppercase()
        } else {
            "PERS".to_string()
        };

        items.push(AgendaItem { time, title: summary, tag });
    }

    items
}

fn ical_field(block: &str, key: &str) -> Option<String> {
    // Exact key match: "KEY:value"
    let prefix = format!("{}:", key);
    for line in block.lines() {
        if line.starts_with(&prefix) {
            return Some(line[prefix.len()..].to_string());
        }
    }
    None
}

fn ical_field_value(block: &str, key: &str) -> Option<String> {
    // Matches "KEY:value" OR "KEY;param=...:value", returns the value only.
    let exact    = format!("{}:", key);
    let prefixed = format!("{};", key);
    for line in block.lines() {
        if line.starts_with(&exact) {
            return Some(line[exact.len()..].to_string());
        }
        if line.starts_with(&prefixed) {
            if let Some(pos) = line.find(':') {
                return Some(line[pos + 1..].to_string());
            }
        }
    }
    None
}

fn unfold(s: &str) -> String {
    // RFC 5545: a CRLF (or LF) followed by a single SPACE or TAB continues the
    // previous line — the join character is removed, *not* preserved.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            if matches!(chars.peek(), Some(' ') | Some('\t')) {
                chars.next();
                continue;
            }
            out.push('\n');
        } else if c == '\r' {
            // Skip lone CR; the LF will trigger the fold check above.
            continue;
        } else {
            out.push(c);
        }
    }
    out
}

/// Locate the DTSTART line in a VEVENT block and return (TZID, value).
fn ical_dtstart(block: &str) -> Option<(Option<String>, String)> {
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("DTSTART:") {
            return Some((None, rest.to_string()));
        }
        if let Some(rest) = line.strip_prefix("DTSTART;") {
            let colon = rest.find(':')?;
            let params = &rest[..colon];
            let value  = &rest[colon + 1..];
            let tzid = params.split(';').find_map(|p| {
                p.strip_prefix("TZID=").map(|v| v.trim_matches('"').to_string())
            });
            return Some((tzid, value.to_string()));
        }
    }
    None
}

fn parse_ical_dtstart(value: &str, tzid: Option<&str>) -> Option<(chrono::DateTime<Local>, bool)> {
    // Accepts:
    //   20260507T153000Z   → UTC datetime
    //   20260507T153000    → floating / TZID-relative datetime
    //   20260507           → all-day (VALUE=DATE)
    let v = value.trim();
    if v.contains('T') {
        let is_utc = v.ends_with('Z');
        let core = v.trim_end_matches('Z');
        let naive = NaiveDateTime::parse_from_str(core, "%Y%m%dT%H%M%S").ok()?;
        let local = if is_utc {
            Utc.from_utc_datetime(&naive).with_timezone(&Local)
        } else if let Some(tz_str) = tzid {
            // Try IANA first; if that fails, try the CLDR Windows-name
            // mapping (Outlook / Exchange feeds use names like
            // "W. Europe Standard Time"). Only warn if both lookups fail.
            let resolved: Option<chrono_tz::Tz> = tz_str.parse().ok().or_else(|| {
                crate::windows_tz::windows_to_iana(tz_str).and_then(|iana| iana.parse().ok())
            });
            match resolved {
                Some(tz) => tz.from_local_datetime(&naive).single()?.with_timezone(&Local),
                None => {
                    warn!("unknown TZID '{tz_str}', treating as local");
                    Local.from_local_datetime(&naive).single()?
                }
            }
        } else {
            Local.from_local_datetime(&naive).single()?
        };
        Some((local, false))
    } else {
        let date = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        let dt = date.and_hms_opt(0, 0, 0)?;
        Some((Local.from_local_datetime(&dt).single()?, true))
    }
}

// ── urlencoding helper ────────────────────────────────────────────────────

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'~' {
                out.push(b as char);
            } else {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
        out
    }
}
