//! OTA firmware offered to the device via `/api/display`.
//!
//! The Docker image bakes in a patched TRMNL firmware build (see
//! `firmware/build.sh`): `firmware.bin` plus `version.txt`. When a device of
//! the matching model reports a different `FW-Version`, the display response
//! sets `update_firmware` + `firmware_url` and the firmware flashes itself
//! (it rate-limits OTA attempts to one per 24h on its own).

use std::path::Path;

use tracing::{info, warn};

pub struct Firmware {
    pub version: String,
    pub bytes: Vec<u8>,
    /// `Model` header value the build is meant for (e.g. `reterminal_e1002`).
    pub model: String,
}

impl Firmware {
    /// Load from `FIRMWARE_DIR` (default `/app/firmware`). Returns `None` when
    /// OTA is disabled (`FIRMWARE_UPDATE=false`) or no build is present.
    pub fn from_env() -> Option<Self> {
        if std::env::var("FIRMWARE_UPDATE").is_ok_and(|v| v == "false" || v == "0") {
            info!("firmware OTA disabled by FIRMWARE_UPDATE");
            return None;
        }
        let dir = std::env::var("FIRMWARE_DIR").unwrap_or_else(|_| "/app/firmware".into());
        let model = std::env::var("FIRMWARE_MODEL").unwrap_or_else(|_| "reterminal_e1002".into());
        match Self::load(Path::new(&dir), model) {
            Ok(fw) => {
                info!(version = %fw.version, model = %fw.model, size = fw.bytes.len(), "firmware OTA available");
                Some(fw)
            }
            Err(e) => {
                warn!("no firmware OTA ({dir}): {e}");
                None
            }
        }
    }

    fn load(dir: &Path, model: String) -> anyhow::Result<Self> {
        let version = std::fs::read_to_string(dir.join("version.txt"))?.trim().to_string();
        anyhow::ensure!(!version.is_empty(), "empty version.txt");
        let bytes = std::fs::read(dir.join("firmware.bin"))?;
        Ok(Self { version, bytes, model })
    }

    /// Offer the update only to the model it was built for, and only when the
    /// device isn't already running it. Devices that don't report a version
    /// or model are left alone.
    pub fn should_offer(&self, device_model: Option<&str>, device_version: Option<&str>) -> bool {
        device_model == Some(self.model.as_str())
            && device_version.is_some_and(|v| v != self.version)
    }

    /// Versioned path so a new build never collides with a cached download.
    pub fn path(&self) -> String {
        format!("/firmware/trmnl-{}.bin", self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fw() -> Firmware {
        Firmware { version: "1.8.16-cp.abc1234".into(), bytes: vec![], model: "reterminal_e1002".into() }
    }

    #[test]
    fn offers_to_matching_model_on_other_version() {
        assert!(fw().should_offer(Some("reterminal_e1002"), Some("1.8.10")));
        assert!(fw().should_offer(Some("reterminal_e1002"), Some("1.8.16")));
    }

    #[test]
    fn skips_when_already_current() {
        assert!(!fw().should_offer(Some("reterminal_e1002"), Some("1.8.16-cp.abc1234")));
    }

    #[test]
    fn skips_other_models_and_unknowns() {
        assert!(!fw().should_offer(Some("og"), Some("1.8.10")));
        assert!(!fw().should_offer(None, Some("1.8.10")));
        assert!(!fw().should_offer(Some("reterminal_e1002"), None));
    }
}
