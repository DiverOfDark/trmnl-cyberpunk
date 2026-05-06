use chrono::{Datelike, Local, Timelike};

#[derive(Clone)]
pub struct HostData {
    pub name: String,
    pub cpu: u8,
    pub cpu_temp: u8,
    pub ram_pct: u8,
    #[allow(dead_code)] // available for future widgets
    pub ram_used_gb: f32,
    #[allow(dead_code)]
    pub ram_total_gb: u8,
    pub disk_pct: u8,
    pub uptime_days: u32,
    pub load: [f32; 3],
}

#[derive(Clone)]
pub struct WeatherDay {
    pub day: String,
    pub hi: i8,
    pub lo: i8,
    pub cond: String,
}

#[derive(Clone)]
pub struct WeatherData {
    pub temp_c: i8,
    pub condition: String,
    pub hi: i8,
    pub lo: i8,
    pub forecast: Vec<WeatherDay>,
}

#[derive(Clone)]
pub struct AgendaItem {
    pub time: String,
    pub title: String,
    pub tag: String,
}

#[derive(Clone)]
pub struct Task {
    pub text: String,
    pub done: bool,
    pub priority: String, // HI / MED / LOW
}

#[derive(Clone)]
pub struct BudgetCat {
    pub label: String,
    pub spent: u32,
    pub cap: u32,
}

#[derive(Clone)]
pub struct BudgetData {
    pub month_label: String,
    pub spent: u32,
    pub cap: u32,
    #[allow(dead_code)]
    pub runway_days: u8,
    pub cats: Vec<BudgetCat>,
}

#[derive(Clone)]
pub struct Alert {
    pub level: String,
    #[allow(dead_code)]
    pub time: String,
    pub message: String,
}

#[derive(Clone)]
pub struct DashData {
    pub device: String,
    pub date_dow: String,
    pub date_display: String,
    #[allow(dead_code)]
    pub date_iso: String,
    pub battery_pct: u8,
    pub signal_bars: u8, // 0-4
    pub last_sync: String,
    pub next_sync: String,
    pub motto: String,
    pub host: HostData,
    pub weather: WeatherData,
    pub agenda: Vec<AgendaItem>,
    pub tasks: Vec<Task>,
    pub budget: BudgetData,
    pub alerts: Vec<Alert>,
}

impl DashData {
    /// Mock data sourced from github.com/DiverOfDark/homelab —
    /// Norse-mythology hostnames, Talos/k3s cluster, BudgetTracker app.
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

        Self {
            device: "TRMNL-01".into(),
            date_dow: dow.into(),
            date_display: format!("{:02} {} {}", now.day(), month, now.year()),
            date_iso: now.format("%Y-%m-%d").to_string(),
            battery_pct: 78,
            signal_bars: 3,
            last_sync: now.format("%H:%M").to_string(),
            next_sync: format!("{:02}:{:02}", (now.hour() + 1) % 24, now.minute()),
            motto: "STAY PARANOID. STAY ONLINE.".into(),
            host: HostData {
                name: "asgard".into(),
                cpu: 34,
                cpu_temp: 58,
                ram_pct: 62,
                ram_used_gb: 39.8,
                ram_total_gb: 64,
                disk_pct: 47,
                uptime_days: 142,
                load: [0.82, 0.74, 0.69],
            },
            weather: WeatherData {
                temp_c: 14,
                condition: "RAIN".into(),
                hi: 17,
                lo: 9,
                forecast: vec![
                    WeatherDay { day: "THU".into(), hi: 18, lo: 10, cond: "CLOUD".into() },
                    WeatherDay { day: "FRI".into(), hi: 21, lo: 11, cond: "SUN".into() },
                    WeatherDay { day: "SAT".into(), hi: 19, lo: 12, cond: "RAIN".into() },
                    WeatherDay { day: "SUN".into(), hi: 16, lo:  9, cond: "STORM".into() },
                ],
            },
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
            budget: BudgetData {
                month_label: format!("{} '{}", month, &now.format("%Y").to_string()[2..]),
                spent: 1842,
                cap: 2600,
                runway_days: 18,
                cats: vec![
                    BudgetCat { label: "RENT".into(),  spent: 980, cap: 980  },
                    BudgetCat { label: "FOOD".into(),  spent: 312, cap: 500  },
                    BudgetCat { label: "INFRA".into(), spent: 187, cap: 250  },
                    BudgetCat { label: "TRANS".into(), spent:  94, cap: 200  },
                    BudgetCat { label: "MISC".into(),  spent: 269, cap: 670  },
                ],
            },
            alerts: vec![
                Alert { level: "WRN".into(), time: "14:02".into(), message: "muspelheimr cpu_temp 71C > 65".into() },
                Alert { level: "ERR".into(), time: "13:48".into(), message: "velero backup.daily failed rc=2".into() },
                Alert { level: "INF".into(), time: "12:00".into(), message: "cert-manager renewed *.lab".into()     },
                Alert { level: "WRN".into(), time: "09:14".into(), message: "fritzbox wan p95 142ms".into()         },
            ],
        }
    }
}
