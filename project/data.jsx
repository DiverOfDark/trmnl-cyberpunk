// Real homelab data — sourced from github.com/DiverOfDark/homelab
// Norse-mythology hostnames, multi-master k3s cluster on Talos.

const DASH_DATA = {
  device: "TRMNL-01",
  hostname: "asgard.lab",
  date: { dow: "WED", day: "06", month: "MAY", year: "2026", iso: "2026-05-06" },
  battery: 78,
  signal: 3,
  lastSync: "14:00",
  nextSync: "15:00",
  motto: "STAY PARANOID. STAY ONLINE.",
  mottos: [
    "STAY PARANOID. STAY ONLINE.",
    "THE NET IS VAST AND INFINITE.",
    "TRUST THE PROCESS. VERIFY THE OUTPUT.",
    "ALL SYSTEMS NOMINAL // ALL HUMANS OPTIONAL",
  ],

  // Pulled from README hardware table — Asgard is the worker beast.
  host: {
    name: "asgard",
    cpu: 34,
    cpuTemp: 58,
    ram: 62,
    ramUsed: "39.8",
    ramTotal: "64",
    disk: 47,
    uptimeDays: 142,
    load: [0.82, 0.74, 0.69],
  },

  // Cluster nodes — used by HUD variation if it wants to show topology.
  cluster: [
    { n: "asgard",      ip: ".11", role: "WRK", state: "UP" },
    { n: "niflheimr",   ip: ".12", role: "MST", state: "UP" },
    { n: "jotunheimr",  ip: ".13", role: "MST", state: "UP" },
    { n: "muspelheimr", ip: ".14", role: "MST", state: "WRN" },
    { n: "yggdrasil",   ip: "off", role: "BAK", state: "UP" },
  ],

  weather: {
    nowTempC: 14,
    nowCond: "RAIN",
    hi: 17,
    lo: 9,
    forecast: [
      { d: "THU", hi: 18, lo: 10, cond: "CLOUD" },
      { d: "FRI", hi: 21, lo: 11, cond: "SUN" },
      { d: "SAT", hi: 19, lo: 12, cond: "RAIN" },
      { d: "SUN", hi: 16, lo: 9,  cond: "STORM" },
    ],
  },

  agenda: [
    { time: "15:30", title: "Standup // k3s infra",     tag: "WORK" },
    { time: "17:00", title: "Replace UPS battery",      tag: "LAB"  },
    { time: "19:00", title: "Dinner w/ A.",             tag: "PERS" },
    { time: "21:30", title: "Rotate cert-mgr certs",    tag: "OPS"  },
  ],

  // Tasks reflect real homelab repo apps (paperless, vaultwarden, rook, etc.)
  tasks: [
    { t: "Patch muspelheimr · Talos 1.8",    done: false, prio: "HI"  },
    { t: "Renew *.lab via cert-manager",     done: true,  prio: "MED" },
    { t: "Backup nextcloud → rclone/B2",     done: false, prio: "HI"  },
    { t: "Rotate vaultwarden secrets (eso)", done: false, prio: "LOW" },
    { t: "Tune rook-ceph osd weights",       done: false, prio: "MED" },
  ],

  // Budget feeds from his own BudgetTracker app.
  budget: {
    monthLabel: "MAY '26",
    spent: 1842,
    cap:   2600,
    runway: 18,
    cats: [
      { k: "RENT",   v: 980, cap: 980  },
      { k: "FOOD",   v: 312, cap: 500  },
      { k: "INFRA",  v: 187, cap: 250  },
      { k: "TRANS",  v: 94,  cap: 200  },
      { k: "MISC",   v: 269, cap: 670  },
    ],
  },

  alerts: [
    { lvl: "WRN", t: "14:02", msg: "muspelheimr cpu_temp 71°C > 65" },
    { lvl: "ERR", t: "13:48", msg: "velero backup.daily failed rc=2" },
    { lvl: "INF", t: "12:00", msg: "cert-manager renewed *.lab"      },
    { lvl: "WRN", t: "09:14", msg: "fritzbox wan p95 142ms"          },
  ],
};

window.DASH_DATA = DASH_DATA;
