use chrono::{DateTime, Datelike, Local, Utc};
use serde::Serialize;

// ── Freshness tracking ─────────────────────────────────────────────────────

/// Outcome of the most recent upstream pull for one data source.
///
/// The dashboard serves whatever is in the cache, so a panel can be showing
/// values from an hour ago without anything on screen saying so. This carries
/// the "when was this actually true?" signal from `Sources::fetch` through to
/// the renderer, which stamps a marker on any panel whose source is failing.
#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq, Debug)]
pub struct SectionStatus {
    /// False when the integration's env vars are unset. An opted-out source
    /// renders an empty panel and never gets a stale marker — there's nothing
    /// to be stale about. Also false before the first fetch completes.
    pub configured: bool,
    /// Whether the most recent pull attempt succeeded.
    pub ok: bool,
    /// When this section last returned data. `None` → never succeeded since
    /// boot, so the panel is empty rather than stale.
    pub last_ok: Option<DateTime<Utc>>,
}

impl SectionStatus {
    pub fn unconfigured() -> Self {
        Self::default()
    }

    pub fn fresh(now: DateTime<Utc>) -> Self {
        Self { configured: true, ok: true, last_ok: Some(now) }
    }

    /// Failed pull: keep the previous success timestamp so the marker can say
    /// how old the values on screen are.
    pub fn failed(prev: Self) -> Self {
        Self { configured: true, ok: false, last_ok: prev.last_ok }
    }

    /// Combine two sources feeding the same panel — the panel is only as
    /// trustworthy as its worst-off source. An unconfigured source is ignored
    /// (it neither invents staleness nor hides a sibling's).
    pub fn worse_of(a: Self, b: Self) -> Self {
        match (a.configured, b.configured) {
            (false, _) => b,
            (_, false) => a,
            _ if a.ok && b.ok => a,
            _ if a.ok => b,
            _ if b.ok => a,
            // Both failing: report the one that's been dark the longest
            // (`None` = never succeeded, which is the worst case of all).
            _ => match (a.last_ok, b.last_ok) {
                (Some(x), Some(y)) if y < x => b,
                (Some(_), None) => b,
                _ => a,
            },
        }
    }

    /// Short marker for the panel header, e.g. `STALE 12m`. `None` when the
    /// section is healthy or opted out — nothing worth the ink.
    pub fn marker(&self, now: DateTime<Utc>) -> Option<String> {
        if !self.configured || self.ok {
            return None;
        }
        Some(match self.last_ok {
            Some(t) => format!("STALE {}", age_label((now - t).num_seconds())),
            None => "NO DATA".to_string(),
        })
    }
}

/// Compact age for the panel markers: `2m`, `3h`, `4d`. Sub-minute ages read
/// as `<1m` rather than `0m` so a marker never claims to be current.
fn age_label(secs: i64) -> String {
    match secs {
        s if s < 60 => "<1m".to_string(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Per-source freshness, rolled up per panel by the accessors below.
#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq, Debug)]
pub struct Status {
    pub hosts: SectionStatus,
    pub cluster: SectionStatus,
    pub weather: SectionStatus,
    pub alerts: SectionStatus,
    pub budget: SectionStatus,
    pub agenda: SectionStatus,
    pub shipments: SectionStatus,
}

impl Status {
    /// Every source healthy as of `now` — used by mock data, which is never
    /// stale by construction.
    pub fn all_fresh(now: DateTime<Utc>) -> Self {
        let f = SectionStatus::fresh(now);
        Self {
            hosts: f, cluster: f, weather: f, alerts: f,
            budget: f, agenda: f, shipments: f,
        }
    }

    pub fn wx(&self) -> SectionStatus {
        self.weather
    }
    pub fn agenda_panel(&self) -> SectionStatus {
        SectionStatus::worse_of(self.agenda, self.shipments)
    }
    pub fn sys(&self) -> SectionStatus {
        SectionStatus::worse_of(self.hosts, self.cluster)
    }
    pub fn budget_panel(&self) -> SectionStatus {
        self.budget
    }
    pub fn ops(&self) -> SectionStatus {
        self.alerts
    }

    /// Most recent successful pull across all sources — i.e. the freshest
    /// thing on the screen. `None` before the first successful fetch.
    pub fn newest_ok(&self) -> Option<DateTime<Utc>> {
        [
            self.hosts, self.cluster, self.weather, self.alerts,
            self.budget, self.agenda, self.shipments,
        ]
        .into_iter()
        .filter_map(|s| s.last_ok)
        .max()
    }

    /// `(panel tag, marker)` for every panel currently degraded — drives the
    /// footer summary so a glance at the bottom line says whether anything on
    /// screen is out of date.
    pub fn degraded(&self, now: DateTime<Utc>) -> Vec<(&'static str, String)> {
        [
            ("WX", self.wx()),
            ("AGENDA", self.agenda_panel()),
            ("SYS", self.sys()),
            ("€", self.budget_panel()),
            ("OPS", self.ops()),
        ]
        .into_iter()
        .filter_map(|(tag, s)| s.marker(now).map(|m| (tag, m)))
        .collect()
    }
}

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
/// - `Variable` — day-to-day discretionary spend. Drives the hero "discretionary left" figure; flagged OVER when its `balance` is negative, or AT RISK when a frequent envelope's burn rate projects the balance negative before month-end (carryover included, so a category cushioned by accumulated funds isn't falsely flagged).
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
    /// Number of transactions in the category this month. Distinguishes a
    /// frequent day-to-day envelope from a lumpy once-a-month one; also gates
    /// the classification fallback when a category group is unrecognized.
    #[serde(default)]
    pub txns: u32,
    /// Median spend in this envelope by this same day-of-month over the prior
    /// three months — the baseline the panel calls "typical". `None` when
    /// fewer than two prior months have data: without a baseline there is
    /// nothing to call abnormal, and guessing one produces false alarms.
    #[serde(default)]
    pub typical_to_date: Option<u32>,
    /// Envelope behavior class, derived from the Actual category group.
    pub class: BudgetClass,
}

/// Length of the given month, defaulting to 30 for dates chrono rejects.
/// Shared by the fetcher (payday countdown) and the renderer (pace bar).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1);
    let next = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    match (first, next) {
        (Some(f), Some(n)) => (n - f).num_days() as u32,
        _ => 30,
    }
}

impl BudgetData {
    /// Euros more (positive) or less (negative) than this month usually costs
    /// by this same day. `None` until there's a usable baseline.
    ///
    /// Deliberately euros rather than a percentage. "+20%" requires knowing
    /// the baseline to mean anything, and the baseline isn't on screen — so
    /// it reads as a number without a unit. "€531 more than usual" is the
    /// same fact in the currency every other figure on the panel uses, and
    /// sits naturally against the daily allowance above it.
    ///
    /// This replaced a spent-vs-budget pace bar. Budgets here are routinely
    /// set below what a category actually costs, so measuring against them
    /// mostly reported that the budget was wrong; measuring against recent
    /// behavior reports whether *this month* is different, which is the
    /// question worth a wall panel.
    pub fn vs_usual(&self) -> Option<i64> {
        let typical = self.typical_mtd?;
        (typical > 0).then(|| self.mtd_spend as i64 - typical as i64)
    }

    /// Cash plus holdings.
    pub fn capital_total(&self) -> i64 {
        self.cash + self.invested
    }
}

impl BudgetCat {
    /// How far above this envelope's own normal it is running, as a percent,
    /// when that gap is worth flagging. `None` means "nothing to say" — which
    /// is most envelopes, most days.
    ///
    /// Three gates, each earning its place against the old burn-rate
    /// projection that fired on 3 of 8 envelopes:
    /// - **≥ +40%** — below that is ordinary month-to-month variation.
    /// - **≥ €25 absolute** — keeps a €4-over coffee budget from shouting
    ///   "+80%" and crowding out something that matters.
    /// - **day ≥ 4** — the first days of a month have a baseline of one or two
    ///   transactions, where a single early shop looks like a spending spree.
    pub fn hot_pct(&self, day: u32) -> Option<i32> {
        let typical = self.typical_to_date?;
        let over = self.spent as i64 - typical as i64;
        (typical > 0 && day >= 4 && self.spent as i64 * 5 >= typical as i64 * 7 && over >= 25)
            .then(|| (over * 100 / typical as i64) as i32)
    }
}

/// One month-end sample of total capital (on-budget cash + off-budget
/// holdings) for the trend line.
#[derive(Clone, Serialize)]
pub struct CapitalPoint {
    /// Three-letter month name, e.g. `JUL`.
    pub label: String,
    /// Cash plus holdings at that month end, in euros.
    pub total: i64,
    /// True for the month still in progress. Its balance is pre-payday for
    /// most of the month, so the last point always dips and recovers — drawn
    /// dashed so that dip doesn't read as a real decline.
    pub provisional: bool,
}

#[derive(Clone, Serialize)]
pub struct BudgetData {
    pub month_label: String,
    pub spent: u32,
    pub cap: u32,
    pub cats: Vec<BudgetCat>,
    /// Month-end total capital for the last ~12 months, oldest first. Empty
    /// when the history fetch failed — the panel drops the chart rather than
    /// the whole section.
    #[serde(default)]
    pub capital: Vec<CapitalPoint>,
    /// Spend so far this month across everything except savings goals, in
    /// euros. Money moved into a goal is allocation, not consumption, and
    /// counting it makes an investment month look like a spending disaster.
    #[serde(default)]
    pub mtd_spend: u32,
    /// Median spend by this same day-of-month over the prior three months,
    /// on the same basis as `mtd_spend`. `None` in the first days of a month,
    /// when the baseline is one or two bills and the ratio is meaningless.
    #[serde(default)]
    pub typical_mtd: Option<u32>,
    /// Off-budget holdings (investment accounts), in euros.
    #[serde(default)]
    pub invested: i64,
    /// Days until the next expected payday, derived from when large inflows
    /// have historically landed. Falls back to days remaining in the month.
    #[serde(default)]
    pub days_to_payday: u32,
    /// On-budget cash: Actual's `totalBalance`, i.e. every envelope balance
    /// summed. Comes free in the month payload we already fetch.
    #[serde(default)]
    pub cash: i64,
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
    /// Per-source freshness of everything above. The dashboard is rendered
    /// from cache, so this is how the panel admits when what it's showing is
    /// older than it looks.
    pub status: Status,
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
        // SYNC reports when the *data* last came in, not when this frame was
        // drawn — renders now happen on every request while fetches run on
        // their own timer, so stamping this with the render clock would claim
        // a freshness the panel doesn't have.
        self.last_sync = match self.status.newest_ok() {
            Some(t) => t.with_timezone(&Local).format("%H:%M").to_string(),
            None => "--:--".into(),
        };
        self.next_sync = next.format("%H:%M").to_string();
        self.motto = pick_motto().into();
    }

    /// Content-free dashboard used as the pre-first-fetch placeholder in
    /// server mode. Every panel renders its own empty state and no source is
    /// marked stale yet — "nothing pulled so far" isn't a failure.
    pub fn empty() -> Self {
        let mut d = Self {
            time: String::new(),
            date: String::new(),
            date_dow: String::new(),
            motto: String::new(),
            last_sync: String::new(),
            next_sync: String::new(),
            hosts: Vec::new(),
            cluster: None,
            weather: None,
            agenda: Vec::new(),
            budget: None,
            alerts: Vec::new(),
            shipments_due_today: Vec::new(),
            status: Status::default(),
        };
        d.refresh_clock();
        d
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
                    // Mock mix exercises every branch: fixed bills (rent still
                    // pending, so it wins the DUE line), a variable envelope
                    // already in the red (OVER), one running well above its own
                    // typical pace (HOT), one above its pace but under the
                    // absolute floor (gated out), one with no baseline at all
                    // (gated out), healthy ones, and a savings goal.
                    BudgetCat { label: "Rent".into(),      spent: 980, cap: 980, balance:    0, txns:  1, typical_to_date: Some(980), class: BudgetClass::Fixed    },
                    BudgetCat { label: "Internet".into(),  spent:   0, cap:  50, balance:   50, txns:  0, typical_to_date: Some( 50), class: BudgetClass::Fixed    },
                    BudgetCat { label: "Insurance".into(), spent:  60, cap:  60, balance:   40, txns:  1, typical_to_date: Some( 60), class: BudgetClass::Fixed    },
                    BudgetCat { label: "Food".into(),      spent: 512, cap: 500, balance:  -12, txns: 40, typical_to_date: Some(300), class: BudgetClass::Variable },
                    BudgetCat { label: "Eating Out".into(),spent: 494, cap: 900, balance:  406, txns: 22, typical_to_date: Some(316), class: BudgetClass::Variable },
                    BudgetCat { label: "Hookah".into(),    spent: 301, cap: 800, balance:  499, txns: 14, typical_to_date: Some(298), class: BudgetClass::Variable },
                    BudgetCat { label: "Entertainment".into(), spent: 41, cap: 50, balance: 9, txns:  4, typical_to_date: Some( 25), class: BudgetClass::Variable },
                    BudgetCat { label: "Cleaning".into(),  spent: 130, cap: 130, balance:    5, txns:  1, typical_to_date: None,      class: BudgetClass::Variable },
                    BudgetCat { label: "Transit".into(),   spent:  94, cap: 200, balance:  106, txns:  8, typical_to_date: Some(110), class: BudgetClass::Variable },
                    BudgetCat { label: "Misc".into(),      spent:  46, cap: 420, balance:  374, txns:  3, typical_to_date: Some( 60), class: BudgetClass::Variable },
                    BudgetCat { label: "Vacation".into(),  spent:   0, cap: 300, balance: 1800, txns:  0, typical_to_date: None,      class: BudgetClass::Savings  },
                ],
                // A year of capital with a step up, a plateau, and a
                // provisional final point — enough shape to prove the line
                // chart's truncated axis and its dashed tail.
                capital: [
                    ("SEP", 39171), ("OCT", 39660), ("NOV", 37919), ("DEC", 38454),
                    ("JAN", 41056), ("FEB", 46678), ("MAR", 47642), ("APR", 47869),
                    ("MAY", 49189), ("JUN", 49335), ("JUL", 48281), ("AUG", 47295),
                ]
                .iter()
                .enumerate()
                .map(|(i, (label, total))| CapitalPoint {
                    label: (*label).into(),
                    total: *total,
                    provisional: i == 11,
                })
                .collect(),
                mtd_spend: 3186,
                typical_mtd: Some(2655),
                days_to_payday: 22,
                cash: 18581,
                invested: 28714,
            }),
            alerts: vec![
                Alert { level: "WRN".into(), time: "14:02".into(), message: "muspelheimr cpu_temp 71C > 65".into() },
                Alert { level: "ERR".into(), time: "13:48".into(), message: "velero backup.daily failed rc=2".into() },
                Alert { level: "WRN".into(), time: "09:14".into(), message: "fritzbox wan p95 142ms".into()         },
            ],
            shipments_due_today: vec![
                ShipmentHighlight { number: "00340435063414124778".into(), remark: "SeeedStudio - reTerminal".into(), status: "Delivered today".into() },
            ],
            // Mock data is fabricated on the spot, so nothing is ever stale.
            status: Status::all_fresh(Utc::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(spent: u32, typical: Option<u32>) -> BudgetCat {
        BudgetCat {
            label: "Eating Out".into(),
            spent,
            cap: 900,
            balance: 406,
            txns: 22,
            typical_to_date: typical,
            class: BudgetClass::Variable,
        }
    }

    /// The case the redesign exists for: real data on 2026-08-09 had Eating
    /// Out at €494 against a €316 norm, while Hookah (€301 vs €298) and
    /// Groceries (€158 vs €190) were normal — yet the old burn-rate
    /// projection flagged all three.
    #[test]
    fn only_genuinely_abnormal_envelopes_are_hot() {
        assert_eq!(cat(494, Some(316)).hot_pct(9), Some(56));
        assert_eq!(cat(301, Some(298)).hot_pct(9), None);
        assert_eq!(cat(158, Some(190)).hot_pct(9), None);
    }

    /// A tiny envelope can double and still not be worth anyone's attention.
    #[test]
    fn small_absolute_overspend_stays_quiet() {
        assert_eq!(cat(9, Some(4)).hot_pct(9), None, "+125% but only €5");
        assert_eq!(cat(80, Some(40)).hot_pct(9), Some(100), "+100% and €40");
    }

    /// Early in the month the baseline is one or two transactions, so a single
    /// early shop must not read as a spree.
    #[test]
    fn no_pace_flags_in_the_first_days() {
        assert_eq!(cat(494, Some(316)).hot_pct(3), None);
        assert_eq!(cat(494, Some(316)).hot_pct(4), Some(56));
    }

    fn budget(mtd: u32, typical: Option<u32>) -> BudgetData {
        BudgetData {
            month_label: "AUG '26".into(),
            spent: 0,
            cap: 0,
            cats: Vec::new(),
            capital: Vec::new(),
            mtd_spend: mtd,
            typical_mtd: typical,
            days_to_payday: 22,
            cash: 18581,
            invested: 28714,
        }
    }

    /// The whole-budget comparison the panel leads with, in euros rather than
    /// percent. Real figures from 2026-08-09: €3,186 spent by day 9 against a
    /// €2,655 three-month norm.
    #[test]
    fn month_compares_against_its_own_norm() {
        assert_eq!(budget(3186, Some(2655)).vs_usual(), Some(531));
        assert_eq!(budget(2655, Some(2655)).vs_usual(), Some(0));
        assert_eq!(budget(1900, Some(2655)).vs_usual(), Some(-755));
    }

    /// Without a baseline the panel says nothing rather than guessing.
    #[test]
    fn no_baseline_means_no_comparison() {
        assert_eq!(budget(3186, None).vs_usual(), None);
        assert_eq!(budget(3186, Some(0)).vs_usual(), None);
    }

    /// Capital is cash plus holdings, which is what the trend line plots.
    #[test]
    fn capital_is_cash_plus_holdings() {
        assert_eq!(budget(0, None).capital_total(), 47295);
    }

    /// No baseline → no claim. An envelope we've never seen before is not
    /// evidence of overspending.
    #[test]
    fn missing_baseline_never_flags() {
        assert_eq!(cat(9999, None).hot_pct(20), None);
        assert_eq!(cat(9999, Some(0)).hot_pct(20), None);
    }
}
