use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Cluster-wide rollups that aren't a simple arithmetic mean of per-host
/// values. CPU and RAM use `sum(consumed) / sum(total)` so a small idle
/// node doesn't pull the cluster number down. Disk reads from Ceph rather
/// than node-exporter's `/` filesystem (which is often the container's
/// rootfs and unhelpful in k8s).
#[derive(Clone, Default, Serialize)]
pub struct ClusterMetrics {
    /// Cluster CPU utilization (0..100).
    pub cpu_pct: u8,
    /// Cluster RAM utilization (0..100).
    pub ram_pct: u8,
    /// Cluster disk utilization from `ceph_cluster_total_used_bytes /
    /// ceph_cluster_total_bytes` (0..100).
    pub disk_pct: u8,
}

#[derive(Clone, Serialize)]
pub struct HostData {
    pub name: String,
    pub cpu: u8,
    pub cpu_temp: u8,
    pub ram_pct: u8,
    pub disk_pct: u8,
    pub uptime_days: u32,
    pub load: [f32; 3],
}

#[derive(Clone, Serialize)]
pub struct WeatherDay {
    pub day: String,
    pub hi: i8,
    pub lo: i8,
    pub cond: String,
}

#[derive(Clone, Default, Serialize)]
pub struct WeatherData {
    pub temp_c: i8,
    pub condition: String,
    pub hi: i8,
    pub lo: i8,
    pub forecast: Vec<WeatherDay>,
}

#[derive(Clone, Serialize)]
pub struct AgendaItem {
    pub time: String,
    pub title: String,
    pub tag: String,
}

/// One row in the OPS panel's task list. Pushed via `PUT /api/tasks` from an
/// external service (e.g. an AI agent). `priority == "HI"` is rendered in
/// red; any other value (typically "MED" / "LOW") is rendered in black.
/// `done` strikes the row through.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct Task {
    /// Task description. Strings longer than the panel's column width are
    /// ellipsized at render time.
    pub text: String,
    /// `true` to render the row with a strikethrough.
    pub done: bool,
    /// Priority label: "HI" / "MED" / "LOW" (case-sensitive). Unknown
    /// values render in black like "MED".
    pub priority: String,
}

#[derive(Clone, Serialize)]
pub struct BudgetCat {
    /// Full category name (no truncation). The renderer truncates for
    /// display; keeping the full name here lets clients (and future logic
    /// like fixed-category detection) match without ambiguity.
    pub label: String,
    pub spent: u32,
    pub cap: u32,
    /// `true` if this envelope behaves like a fixed cost (rent, subscription) —
    /// detected as ≤2 transactions in the current month. Fixed envelopes
    /// don't trigger overpace alerts when they hit 100% early in the month.
    #[serde(default)]
    pub is_fixed: bool,
}

#[derive(Clone, Serialize)]
pub struct BudgetData {
    pub month_label: String,
    pub spent: u32,
    pub cap: u32,
    pub cats: Vec<BudgetCat>,
}

#[derive(Clone, Serialize)]
pub struct Alert {
    pub level: String,
    pub time: String,
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct DashData {
    pub time: String,
    pub date: String,
    pub date_dow: String,
    pub motto: String,
    pub last_sync: String,
    pub next_sync: String,
    pub hosts: Vec<HostData>,
    /// Cluster rollups (CPU/RAM/DSK). `None` falls back to per-host
    /// arithmetic mean in the renderer so mock data and partial outages
    /// still produce a reasonable summary.
    pub cluster: Option<ClusterMetrics>,
    /// `None` when the integration isn't configured (env vars empty);
    /// renderer skips the WX panel body in that case. `Some` when fetched
    /// successfully OR when configured-but-failed (mock fallback).
    pub weather: Option<WeatherData>,
    pub agenda: Vec<AgendaItem>,
    pub tasks: Vec<Task>,
    /// Same not-configured-vs-mocked-fallback semantics as `weather`.
    pub budget: Option<BudgetData>,
    pub alerts: Vec<Alert>,
}

impl DashData {
    pub fn mock() -> Self {
        let now = Local::now();
        let months = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN",
            "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        use chrono::Weekday;
        let dow = match now.weekday() {
            Weekday::Sun => "SUN", Weekday::Mon => "MON", Weekday::Tue => "TUE",
            Weekday::Wed => "WED", Weekday::Thu => "THU", Weekday::Fri => "FRI",
            Weekday::Sat => "SAT",
        };
        let month = months[(now.month() - 1) as usize];

        let refresh_secs: i64 = std::env::var("REFRESH_SECS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(3600);
        let next = now + chrono::Duration::seconds(refresh_secs);

        Self {
            time: now.format("%H:%M").to_string(),
            date: format!("{:02} {} {}", now.day(), month, now.year()),
            date_dow: dow.into(),
            motto: "STAY PARANOID. STAY ONLINE.".into(),
            last_sync: now.format("%H:%M").to_string(),
            next_sync: next.format("%H:%M").to_string(),
            // Mock cluster left as None so the renderer falls back to the
            // per-host arithmetic mean — keeps mock previews unchanged.
            cluster: None,
            hosts: vec![
                HostData {
                    name: "asgard".into(),
                    cpu: 34, cpu_temp: 58, ram_pct: 62,
                    disk_pct: 47, uptime_days: 142,
                    load: [0.82, 0.74, 0.69],
                },
                HostData {
                    name: "muspelheimr".into(),
                    cpu: 71, cpu_temp: 71, ram_pct: 81,
                    disk_pct: 23, uptime_days: 7,
                    load: [1.92, 1.74, 1.55],
                },
                HostData {
                    name: "niflheimr".into(),
                    cpu: 12, cpu_temp: 44, ram_pct: 38,
                    disk_pct: 88, uptime_days: 203,
                    load: [0.24, 0.18, 0.21],
                },
            ],
            weather: Some(WeatherData {
                temp_c: 14,
                condition: "RAIN".into(),
                hi: 17, lo: 9,
                forecast: vec![
                    WeatherDay { day: "THU".into(), hi: 18, lo: 10, cond: "CLOUD".into() },
                    WeatherDay { day: "FRI".into(), hi: 21, lo: 11, cond: "SUN".into()   },
                    WeatherDay { day: "SAT".into(), hi: 19, lo: 12, cond: "RAIN".into()  },
                    WeatherDay { day: "SUN".into(), hi: 16, lo:  9, cond: "STORM".into() },
                ],
            }),
            agenda: vec![
                AgendaItem { time: "15:30".into(), title: "Standup // k3s infra".into(), tag: "WORK".into() },
                AgendaItem { time: "17:00".into(), title: "Replace UPS battery".into(),  tag: "LAB".into()  },
                AgendaItem { time: "19:00".into(), title: "Dinner w/ A.".into(),          tag: "PERS".into() },
                AgendaItem { time: "21:30".into(), title: "Rotate cert-mgr certs".into(), tag: "OPS".into()  },
            ],
            tasks: vec![
                Task { text: "Patch muspelheimr · Talos 1.8".into(),  done: false, priority: "HI".into()  },
                Task { text: "Renew *.lab via cert-manager".into(),    done: true,  priority: "MED".into() },
                Task { text: "Backup nextcloud → rclone/B2".into(),    done: false, priority: "HI".into()  },
                Task { text: "Rotate vaultwarden secrets".into(),       done: false, priority: "LOW".into() },
            ],
            budget: Some(BudgetData {
                month_label: format!("{} '{}", month, &now.format("%Y").to_string()[2..]),
                spent: 1842,
                cap: 2600,
                cats: vec![
                    // Mock mix: a fixed envelope (rent paid in full),
                    // an overpace one (food at 1.5× pace assuming mid-month),
                    // and some on-track ones — exercises every branch.
                    BudgetCat { label: "Rent".into(),       spent: 980, cap: 980, is_fixed: true  },
                    BudgetCat { label: "Internet".into(),    spent: 50,  cap: 50,  is_fixed: true  },
                    BudgetCat { label: "Food".into(),        spent: 412, cap: 500, is_fixed: false },
                    BudgetCat { label: "Shisha".into(),      spent: 180, cap: 200, is_fixed: false },
                    BudgetCat { label: "Infrastructure".into(), spent: 80, cap: 250, is_fixed: false },
                    BudgetCat { label: "Transit".into(),     spent:  94, cap: 200, is_fixed: false },
                    BudgetCat { label: "Misc".into(),        spent:  46, cap: 420, is_fixed: false },
                ],
            }),
            alerts: vec![
                Alert { level: "WRN".into(), time: "14:02".into(), message: "muspelheimr cpu_temp 71C > 65".into() },
                Alert { level: "ERR".into(), time: "13:48".into(), message: "velero backup.daily failed rc=2".into() },
                Alert { level: "INF".into(), time: "12:00".into(), message: "cert-manager renewed *.lab".into()     },
                Alert { level: "WRN".into(), time: "09:14".into(), message: "fritzbox wan p95 142ms".into()         },
            ],
        }
    }
}
