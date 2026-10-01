//! The device registry: every panel that has polled this server, what it
//! reported last time, and which screen it's assigned.
//!
//! Persisted as one JSON file (`$DATA_DIR/devices.json`) so assignments and
//! names survive restarts and stay hand-editable. The last-seen status lives
//! in the same file — a poll arrives at most every few minutes per device, so
//! rewriting it on each one is cheap, and the device list in the web UI isn't
//! blank after a restart.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};
use trmnl::DeviceInfo;
use utoipa::ToSchema;

use crate::note::Screen;

/// Which screen a device is assigned.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Always the dashboard. Where a new device starts.
    #[default]
    Dashboard,
    /// Always the memo (its empty-state screen when there is none).
    Note,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Device {
    /// MAC address from the firmware's `ID` header; the registry key.
    pub mac: String,
    /// Display name set in the web UI. Empty until someone names it.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mode: Mode,
    pub battery_pct: Option<u8>,
    pub battery_voltage: Option<f32>,
    /// WiFi signal strength, dBm.
    pub rssi: Option<i32>,
    pub firmware: Option<String>,
    /// Hardware model from the `Model` header (e.g. `reterminal_e1002`).
    #[serde(default)]
    pub model: Option<String>,
    /// The refresh interval the device reported it's running on, seconds.
    pub refresh_rate: Option<u32>,
    pub last_seen: Option<DateTime<Utc>>,
    /// The screen handed out on the last poll.
    pub last_screen: Option<Screen>,
}

impl Device {
    fn new(mac: String) -> Self {
        Self {
            mac,
            name: String::new(),
            mode: Mode::default(),
            battery_pct: None,
            battery_voltage: None,
            rssi: None,
            firmware: None,
            model: None,
            refresh_rate: None,
            last_seen: None,
            last_screen: None,
        }
    }
}

/// Fields the web UI may change. Absent fields are left alone.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct DevicePatch {
    pub name: Option<String>,
    pub mode: Option<Mode>,
}

pub struct DeviceStore {
    path: PathBuf,
    devices: Mutex<BTreeMap<String, Device>>,
}

/// One spelling per device, whatever case the firmware sends.
fn normalize(mac: &str) -> String {
    mac.trim().to_uppercase()
}

/// Lowercase MAC hex (`ac276ea69d18`): stable per device and URL-safe, so it's
/// what the per-device screen URLs carry.
pub fn device_key(mac: &str) -> String {
    mac.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase()
}

impl DeviceStore {
    /// Load `$DATA_DIR/devices.json`. A missing file is a fresh install; an
    /// unreadable one is logged and starts empty rather than refusing to boot.
    pub async fn open(data_dir: impl Into<PathBuf>) -> Self {
        let path = data_dir.into().join("devices.json");
        let devices = match tokio::fs::read_to_string(&path).await {
            Ok(text) => match serde_json::from_str::<Vec<Device>>(&text) {
                Ok(list) => {
                    info!("loaded {} device(s) from {}", list.len(), path.display());
                    list.into_iter().map(|d| (normalize(&d.mac), d)).collect()
                }
                Err(e) => {
                    warn!("ignoring malformed {}: {e}", path.display());
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                warn!("failed to read {}: {e}", path.display());
                BTreeMap::new()
            }
        };
        Self {
            path,
            devices: Mutex::new(devices),
        }
    }

    /// Record a device's status. Called from `/api/setup`, which hands out
    /// a dashboard URL whatever the device is assigned.
    pub async fn register(&self, info: &DeviceInfo, model: Option<&str>) {
        let mut devices = self.devices.lock().await;
        record(&mut devices, info, model);
        self.save(&devices).await;
    }

    /// Record a poll and return the screen this device is assigned.
    pub async fn checkin(&self, info: &DeviceInfo, model: Option<&str>) -> Screen {
        let mut devices = self.devices.lock().await;
        let device = record(&mut devices, info, model);
        let screen = match device.mode {
            Mode::Dashboard => Screen::Dashboard,
            Mode::Note => Screen::Note,
        };
        device.last_screen = Some(screen);
        self.save(&devices).await;
        screen
    }

    pub async fn list(&self) -> Vec<Device> {
        self.devices.lock().await.values().cloned().collect()
    }

    /// The device for `key` (MAC in any spelling, including `device_key`
    /// hex) if known. With no key, whichever device polled last — the browser
    /// previews have no device of their own, so they borrow the most recent
    /// reading.
    pub async fn get_or_latest(&self, key: Option<&str>) -> Option<Device> {
        let devices = self.devices.lock().await;
        match key {
            Some(k) => {
                let k = device_key(k);
                devices.values().find(|d| device_key(&d.mac) == k).cloned()
            }
            None => devices.values().max_by_key(|d| d.last_seen).cloned(),
        }
    }

    /// Apply a patch; `Ok(None)` if no such device.
    pub async fn update(&self, mac: &str, patch: DevicePatch) -> anyhow::Result<Option<Device>> {
        let mut devices = self.devices.lock().await;
        let Some(device) = devices.get_mut(&normalize(mac)) else {
            return Ok(None);
        };
        if let Some(name) = patch.name {
            device.name = name.trim().to_string();
        }
        if let Some(mode) = patch.mode {
            device.mode = mode;
        }
        let device = device.clone();
        self.try_save(&devices).await?;
        Ok(Some(device))
    }

    /// Forget a device; it re-registers with default settings if it polls
    /// again. Returns whether it existed.
    pub async fn remove(&self, mac: &str) -> anyhow::Result<bool> {
        let mut devices = self.devices.lock().await;
        if devices.remove(&normalize(mac)).is_none() {
            return Ok(false);
        }
        self.try_save(&devices).await?;
        Ok(true)
    }

    /// Status writes from polls are best-effort: a read-only or missing data
    /// dir must not stop the device from getting its screen.
    async fn save(&self, devices: &BTreeMap<String, Device>) {
        if let Err(e) = self.try_save(devices).await {
            warn!("saving {} failed: {e}", self.path.display());
        }
    }

    /// Temp file + rename, so a crash mid-write never leaves a torn file.
    /// Callers hold the lock, which keeps concurrent writes from racing on
    /// the temp file.
    async fn try_save(&self, devices: &BTreeMap<String, Device>) -> anyhow::Result<()> {
        if let Some(dir) = self.path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let list: Vec<&Device> = devices.values().collect();
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(&list)?).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }
}

/// Upsert the status fields from a request's headers.
fn record<'a>(
    devices: &'a mut BTreeMap<String, Device>,
    info: &DeviceInfo,
    model: Option<&str>,
) -> &'a mut Device {
    let mac = normalize(&info.mac_address);
    let device = devices
        .entry(mac.clone())
        .or_insert_with(|| Device::new(mac));
    device.battery_pct = info.battery_percentage();
    device.battery_voltage = info.battery_voltage;
    device.rssi = info.rssi;
    device.firmware = info.firmware_version.clone();
    device.model = model.map(str::to_string);
    device.refresh_rate = info.refresh_rate;
    device.last_seen = Some(Utc::now());
    device
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("trmnl-devices-{tag}-{}", std::process::id()))
    }

    #[test]
    fn device_key_normalizes_mac() {
        assert_eq!(device_key("AC:27:6E:A6:9D:18"), "ac276ea69d18");
    }

    #[tokio::test]
    async fn modes_pick_the_screen() {
        let dir = temp_dir("modes");
        let store = DeviceStore::open(&dir).await;
        let a = DeviceInfo::new("aa:aa:aa:aa:aa:01");
        let b = DeviceInfo::new("AA:AA:AA:AA:AA:02");
        // New devices start on the dashboard.
        assert_eq!(store.checkin(&a, None).await, Screen::Dashboard);
        assert_eq!(store.checkin(&b, None).await, Screen::Dashboard);
        store.update("AA:AA:AA:AA:AA:01", DevicePatch { mode: Some(Mode::Note), ..Default::default() })
            .await
            .unwrap();
        for _ in 0..3 {
            assert_eq!(store.checkin(&a, None).await, Screen::Note);
            assert_eq!(store.checkin(&b, None).await, Screen::Dashboard);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn settings_and_status_survive_reopen() {
        let dir = temp_dir("persist");
        let store = DeviceStore::open(&dir).await;
        let info = DeviceInfo::new("AA:BB").with_battery_voltage(4.2).with_rssi(-60);
        store.checkin(&info, Some("reterminal_e1002")).await;
        store
            .update("AA:BB", DevicePatch { name: Some(" Kitchen ".into()), mode: Some(Mode::Note) })
            .await
            .unwrap();
        let reopened = DeviceStore::open(&dir).await;
        let d = reopened.get_or_latest(Some("aabb")).await.unwrap();
        assert_eq!(d.model.as_deref(), Some("reterminal_e1002"));
        assert_eq!(d.name, "Kitchen");
        assert_eq!(d.mode, Mode::Note);
        assert_eq!(d.battery_pct, Some(100));
        assert_eq!(d.rssi, Some(-60));
        assert_eq!(d.last_screen, Some(Screen::Dashboard));
        assert!(reopened.remove("AA:BB").await.unwrap());
        assert!(reopened.list().await.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
