use chrono::{Datelike, Local};
use serde::Serialize;

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
    /// Used RAM in GiB. Rendered alongside total in the per-host detail
    /// rows ("1.6G/16G") because absolute usage is more actionable than
    /// the percentage when the cluster mixes hosts of different sizes.
    pub ram_used_gib: f32,
    /// Total RAM in GiB.
    pub ram_total_gib: f32,
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

#[derive(Clone, Serialize)]
pub struct WeatherHour {
    pub time: String,
    pub temp_c: i8,
    pub cond: String,
}

#[derive(Clone, Default, Serialize)]
pub struct WeatherData {
    pub temp_c: i8,
    pub condition: String,
    pub hi: i8,
    pub lo: i8,
    pub hourly: Vec<WeatherHour>,
    pub forecast: Vec<WeatherDay>,
}

#[derive(Clone, Serialize)]
pub struct AgendaItem {
    pub time: String,
    pub title: String,
    pub tag: String,
    /// Pre-formatted meeting duration ("30m", "1h", "1h30m"). Empty for
    /// all-day events or when the source ICS didn't supply DTEND/DURATION.
    #[serde(default)]
    pub duration: String,
}

/// How an envelope behaves, derived from the Actual category *group* the user
/// already curated (with a transaction-count fallback when the group name is
/// unrecognized). Drives the budget panel's triage:
///
/// - `Fixed` — rent, insurances, subscriptions. Paid in lumps; reported as upcoming debits rather than flagged for overspend.
/// - `Variable` — day-to-day discretionary spend. Drives the hero "discretionary left" figure and is flagged when its `balance` goes negative (overspent, carryover included).
/// - `Savings` — goals / sinking funds. Excluded from "spendable" money and reported separately.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum BudgetClass {
    Fixed,
    Variable,
    Savings,
}

#[derive(Clone, Serialize)]
pub struct BudgetCat {
    /// Full category name (no truncation). The renderer truncates for
    /// display; keeping the full name here lets clients match without
    /// ambiguity.
    pub label: String,
    pub spent: u32,
    pub cap: u32,
    /// Envelope balance straight from Actual (carryover + budgeted − spent).
    /// Negative means the envelope is overspent / in the red — the truest
    /// signal of trouble in a YNAB-style budget, since it includes money
    /// carried over from prior months that month-only `cap − spent` misses.
    #[serde(default)]
    pub balance: i32,
    /// Envelope behavior class, derived from the Actual category group.
    pub class: BudgetClass,
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
pub struct ShipmentHighlight {
    pub number: String,
    pub remark: String,
    pub status: String,
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
    /// Same not-configured-vs-mocked-fallback semantics as `weather`.
    pub budget: Option<BudgetData>,
    pub alerts: Vec<Alert>,
    pub shipments_due_today: Vec<ShipmentHighlight>,
}

/// Cyberpunk-corpo-PR mottos for the header banner. Cycled per render
/// using a clock-based index — no `rand` dependency required, and a
/// once-per-second-changing pick is plenty for a panel that refreshes at
/// most once per minute.
pub const MOTTOS: &[&str] = &[
    "STAY PARANOID. STAY ONLINE.",
    "WORK. SLEEP. DEPLOY.",
    "THE NETWORK REMEMBERS.",
    "NEVER TRUST. ALWAYS VERIFY.",
    "UPDATES ARE MANDATORY.",
    "UPTIME IS LIFE.",
    "THE BACKUP IS YOU.",
    "PRODUCTIVITY IS PATRIOTISM.",
    "SLEEP IS A LATENCY ISSUE.",
    "SILENCE IS COMPLIANCE.",
    "DATA IS DESTINY.",
];

pub fn pick_motto() -> &'static str {
    let idx = chrono::Utc::now().timestamp().unsigned_abs() as usize % MOTTOS.len();
    MOTTOS[idx]
}

impl DashData {
    /// Re-stamp the clock-derived fields (time, date, motto, sync markers)
    /// to wall-clock now. Called on every PNG render so the panel shows the
    /// current time without re-fetching upstream data.
    pub fn refresh_clock(&mut self) {
        use chrono::{Datelike, Local, Weekday};
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

        self.time = now.format("%H:%M").to_string();
        self.date = format!("{:02} {} {}", now.day(), month, now.year());
        self.date_dow = dow.into();
        self.last_sync = now.format("%H:%M").to_string();
        self.next_sync = next.format("%H:%M").to_string();
        self.motto = pick_motto().into();
    }

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
            motto: pick_motto().into(),
            last_sync: now.format("%H:%M").to_string(),
            next_sync: next.format("%H:%M").to_string(),
            // Mock cluster left as None so the renderer falls back to the
            // per-host arithmetic mean — keeps mock previews unchanged.
            cluster: None,
            hosts: vec![
                HostData {
                    name: "asgard".into(),
                    cpu: 34, cpu_temp: 58, ram_pct: 62,
                    ram_used_gib: 9.9, ram_total_gib: 16.0,
                    disk_pct: 47, uptime_days: 142,
                    load: [0.82, 0.74, 0.69],
                },
                HostData {
                    name: "muspelheimr".into(),
                    cpu: 71, cpu_temp: 71, ram_pct: 81,
                    ram_used_gib: 25.9, ram_total_gib: 32.0,
                    disk_pct: 23, uptime_days: 7,
                    load: [1.92, 1.74, 1.55],
                },
                HostData {
                    name: "niflheimr".into(),
                    cpu: 12, cpu_temp: 44, ram_pct: 38,
                    ram_used_gib: 3.0, ram_total_gib: 8.0,
                    disk_pct: 88, uptime_days: 203,
                    load: [0.24, 0.18, 0.21],
                },
            ],
            weather: Some(WeatherData {
                temp_c: 14,
                condition: "RAIN".into(),
                hi: 17, lo: 9,
                hourly: vec![
                    WeatherHour { time: "12:00".into(), temp_c: 14, cond: "RAIN".into() },
                    WeatherHour { time: "15:00".into(), temp_c: 15, cond: "CLOUD".into() },
                    WeatherHour { time: "18:00".into(), temp_c: 13, cond: "RAIN".into() },
                ],
                forecast: vec![
                    WeatherDay { day: "THU".into(), hi: 18, lo: 10, cond: "CLOUD".into() },
                    WeatherDay { day: "FRI".into(), hi: 21, lo: 11, cond: "SUN".into()   },
                    WeatherDay { day: "SAT".into(), hi: 19, lo: 12, cond: "RAIN".into()  },
                    WeatherDay { day: "SUN".into(), hi: 16, lo:  9, cond: "STORM".into() },
                ],
            }),
            agenda: vec![
                AgendaItem { time: "15:30".into(), title: "Standup // k3s infra".into(), tag: "WORK".into(), duration: "30m".into() },
                AgendaItem { time: "17:00".into(), title: "Replace UPS battery".into(),  tag: "LAB".into(),  duration: "1h".into()  },
                AgendaItem { time: "19:00".into(), title: "Dinner w/ A.".into(),          tag: "PERS".into(), duration: "2h".into()  },
                AgendaItem { time: "21:30".into(), title: "Rotate cert-mgr certs".into(), tag: "OPS".into(),  duration: "45m".into() },
            ],
             budget: Some(BudgetData {
                month_label: format!("{} '{}", month, &now.format("%Y").to_string()[2..]),
                spent: 1842,
                cap: 2600,
                cats: vec![
                    // Mock mix exercises every branch: fixed bills (some still
                    // pending), variable envelopes both over- and under-pace
                    // (one with a negative balance), and a savings goal.
                    BudgetCat { label: "Rent".into(),        spent: 980, cap: 980, balance:   0, class: BudgetClass::Fixed    },
                    BudgetCat { label: "Internet".into(),    spent:   0, cap:  50, balance:  50, class: BudgetClass::Fixed    },
                    BudgetCat { label: "Insurance".into(),   spent:  60, cap:  60, balance:  40, class: BudgetClass::Fixed    },
                    BudgetCat { label: "Food".into(),        spent: 512, cap: 500, balance: -12, class: BudgetClass::Variable },
                    BudgetCat { label: "Shisha".into(),      spent: 180, cap: 200, balance:  60, class: BudgetClass::Variable },
                    BudgetCat { label: "Transit".into(),     spent:  94, cap: 200, balance: 106, class: BudgetClass::Variable },
                    BudgetCat { label: "Misc".into(),        spent:  46, cap: 420, balance: 374, class: BudgetClass::Variable },
                    BudgetCat { label: "Vacation".into(),    spent:   0, cap: 300, balance: 1800, class: BudgetClass::Savings },
                ],
            }),
            alerts: vec![
                Alert { level: "WRN".into(), time: "14:02".into(), message: "muspelheimr cpu_temp 71C > 65".into() },
                Alert { level: "ERR".into(), time: "13:48".into(), message: "velero backup.daily failed rc=2".into() },
                Alert { level: "WRN".into(), time: "09:14".into(), message: "fritzbox wan p95 142ms".into()         },
            ],
            shipments_due_today: vec![
                ShipmentHighlight { number: "00340435063414124778".into(), remark: "SeeedStudio - reTerminal".into(), status: "Delivered today".into() },
            ],
        }
    }
}
