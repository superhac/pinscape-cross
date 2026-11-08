//! Plunger commands (CMD_PLUNGER 0x0F): plunger configuration, live
//! readings (incl. image-sensor pixels), calibration, settings,
//! commit/revert.
//!
//! The setters change the Pico's in-memory settings only; `plunger_commit`
//! saves them (and the calibration) to flash, `plunger_revert` reloads the
//! saved ones.
//!
//! The app also keeps a host-side copy of the last calibration per device
//! (`<app data>/plunger-calibration/<hwid>.json`), so it can be put back
//! after the device was reset or its settings lost, as the Windows tool does.

use pinscape_protocol::plunger::{PlungerCal, PlungerConfig, PlungerReading, SensorData};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager as _;
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Largest manual scaling factor the UI may set, percent.
const MAX_SCALING_PCT: u32 = 1000;

#[tauri::command]
pub async fn plunger_config(mgr: State<'_, Manager>, key: String) -> Res<PlungerConfig> {
    mgr.run(&key, |c| c.query_plunger_config()).await
}

/// Live reading. Image-sensor snapshots are large (one byte per pixel), so
/// the pixel array is only included when `pixels` is true.
#[tauri::command]
pub async fn plunger_reading(mgr: State<'_, Manager>, key: String, pixels: bool) -> Res<PlungerReading> {
    mgr.run(&key, move |c| {
        let mut r = c.query_plunger_reading()?;
        if !pixels {
            if let Some(SensorData::ImageSensor { pixels, .. }) = r.sensor.as_mut() {
                pixels.clear();
            }
        }
        Ok(r)
    })
    .await
}

/// Start the timed calibration (about 15 s). With `auto_save`, the result is
/// saved to flash when it ends.
#[tauri::command]
pub async fn plunger_calibrate(mgr: State<'_, Manager>, key: String, auto_save: bool) -> Res<()> {
    mgr.run(&key, move |c| c.start_plunger_calibration(auto_save)).await
}

#[tauri::command]
pub async fn plunger_set_jitter_filter(mgr: State<'_, Manager>, key: String, window: u16) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_jitter_filter(window)).await
}

/// Firing time limit in microseconds (0 = firmware default).
#[tauri::command]
pub async fn plunger_set_firing_time_limit(mgr: State<'_, Manager>, key: String, us: u32) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_firing_time_limit(us)).await
}

/// Integration time in microseconds (0 = sensor default); imaging sensors only.
#[tauri::command]
pub async fn plunger_set_integration_time(mgr: State<'_, Manager>, key: String, us: u32) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_integration_time(us)).await
}

#[tauri::command]
pub async fn plunger_set_orientation(mgr: State<'_, Manager>, key: String, reverse: bool) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_orientation(reverse)).await
}

/// Manual scaling factor, percent (1..=1000; 100 = calibrated range unchanged).
#[tauri::command]
pub async fn plunger_set_scaling_factor(mgr: State<'_, Manager>, key: String, percent: u32) -> Res<()> {
    if percent == 0 || percent > MAX_SCALING_PCT {
        return Err(format!("scaling factor must be 1-{MAX_SCALING_PCT}%"));
    }
    mgr.run(&key, move |c| c.set_plunger_scaling_factor(percent)).await
}

#[tauri::command]
pub async fn plunger_set_scan_mode(mgr: State<'_, Manager>, key: String, mode: u8) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_scan_mode(mode)).await
}

/// Put calibration data (e.g. saved earlier from a reading) into effect, in memory only.
#[tauri::command]
pub async fn plunger_set_calibration(mgr: State<'_, Manager>, key: String, cal: PlungerCal) -> Res<()> {
    mgr.run(&key, move |c| c.set_plunger_calibration(&cal)).await
}

/// Save the live settings and calibration to flash.
#[tauri::command]
pub async fn plunger_commit(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| c.commit_plunger_settings()).await
}

/// Reload the settings and calibration saved in flash.
#[tauri::command]
pub async fn plunger_revert(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| c.revert_plunger_settings()).await
}

/// Put the firmware's defaults into effect (in memory only): uncalibrated
/// full-range calibration, no jitter filter, default firing time limit and
/// integration time, standard orientation, 100% scaling, default scan mode.
#[tauri::command]
pub async fn plunger_restore_defaults(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| {
        let cfg = c.query_plunger_config()?;
        c.set_plunger_calibration(&PlungerCal::defaults(cfg.native_scale))?;
        c.set_plunger_jitter_filter(0)?;
        c.set_plunger_firing_time_limit(50_000)?;
        c.set_plunger_orientation(false)?;
        c.set_plunger_scaling_factor(100)?;
        if cfg.has_integration_time {
            c.set_plunger_integration_time(0)?;
        }
        if !cfg.scan_modes.is_empty() {
            c.set_plunger_scan_mode(0)?;
        }
        Ok(())
    })
    .await
}

/// A calibration saved on this computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedCalibration {
    pub hwid: String,
    /// Sensor type it was made with (only restored onto the same type).
    pub sensor_type: u8,
    /// Local time, RFC 3339.
    pub saved_at: String,
    pub cal: PlungerCal,
}

fn cal_dir(app: &tauri::AppHandle) -> Res<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("plunger-calibration"))
}

fn cal_file(dir: &Path, hwid: &str) -> Res<PathBuf> {
    if hwid.is_empty() || hwid.len() > 32 || !hwid.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("invalid hardware ID '{hwid}'"));
    }
    Ok(dir.join(format!("{}.json", hwid.to_ascii_uppercase())))
}

fn load_saved_in(dir: &Path, hwid: &str) -> Res<Option<SavedCalibration>> {
    let f = cal_file(dir, hwid)?;
    match std::fs::read_to_string(&f) {
        Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| format!("{}: {e}", f.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", f.display())),
    }
}

fn save_in(dir: &Path, hwid: &str, sensor_type: u8, cal: PlungerCal) -> Res<SavedCalibration> {
    let f = cal_file(dir, hwid)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let saved = SavedCalibration {
        hwid: hwid.to_ascii_uppercase(),
        sensor_type,
        saved_at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        cal,
    };
    let text = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?;
    // write-then-rename so a crash can't leave a truncated file
    let tmp = f.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &f).map_err(|e| format!("{}: {e}", f.display()))?;
    Ok(saved)
}

/// The calibration last saved on this computer for a device, if any.
#[tauri::command]
pub fn plunger_saved_calibration(app: tauri::AppHandle, hwid: String) -> Res<Option<SavedCalibration>> {
    load_saved_in(&cal_dir(&app)?, &hwid)
}

/// Keep a copy of a device's calibration on this computer.
#[tauri::command]
pub fn plunger_save_calibration(app: tauri::AppHandle, hwid: String, sensor_type: u8, cal: PlungerCal) -> Res<SavedCalibration> {
    save_in(&cal_dir(&app)?, &hwid, sensor_type, cal)
}

#[cfg(test)]
mod tests {
    #[test]
    fn saved_calibration_roundtrip() {
        use super::*;
        let dir = std::env::temp_dir().join(format!("pinscape-plcal-{}", std::process::id()));
        assert_eq!(load_saved_in(&dir, "E66038B7133B6B2E").unwrap(), None);
        let cal = PlungerCal { calibrated: true, cal_min: 10, cal_zero: 300, cal_max: 4000, firing_time_measured: 21_000, ..Default::default() };
        let s = save_in(&dir, "e66038b7133b6b2e", 1, cal).unwrap();
        assert_eq!(s.hwid, "E66038B7133B6B2E");
        let back = load_saved_in(&dir, "E66038B7133B6B2E").unwrap().unwrap();
        assert_eq!((back.cal, back.sensor_type), (cal, 1));
        assert!(dir.join("E66038B7133B6B2E.json").exists());
        // the UI relies on these names
        let v = serde_json::to_value(&back).unwrap();
        assert!(v["savedAt"].is_string() && v["sensorType"] == 1 && v["cal"]["calZero"] == 300);
        assert!(save_in(&dir, "../etc/passwd", 1, cal).is_err());
        assert!(load_saved_in(&dir, "").is_err());
        std::fs::write(dir.join("BAD.json"), "{").unwrap();
        assert!(load_saved_in(&dir, "BAD").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use pinscape_protocol::plunger::{PlungerConfig, PlungerReading, SensorData};

    /// The UI (ui/src/lib/api-plunger.ts) relies on these JSON field names.
    #[test]
    fn json_shape() {
        let r = PlungerReading {
            sensor: Some(SensorData::ImageSensor { sensor_type: 5, timestamp: 1, n_pix: 2, pixels: vec![3, 4] }),
            ..Default::default()
        }
        .with_derived();
        let v = serde_json::to_value(&r).unwrap();
        for k in ["rawPos", "z0", "firingStateName", "calZero", "jfLastPre", "manualScalingFactor", "zbLaunch"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert_eq!(v["sensor"]["kind"], "imageSensor");
        assert_eq!(v["sensor"]["nPix"], 2);
        assert_eq!(v["sensor"]["sensorType"], 5);
        let q = serde_json::to_value(SensorData::Vcnl4010 { sensor_type: 3, prox_count: 9 }).unwrap();
        assert_eq!(q["kind"], "vcnl4010");
        assert_eq!(q["proxCount"], 9);
        let c = serde_json::to_value(PlungerConfig::new(6, 1280, 0)).unwrap();
        assert_eq!(c["scanModes"][0], "Steady Slope");
        assert_eq!(c["hasIntegrationTime"], true);
        let cal: pinscape_protocol::plunger::PlungerCal = serde_json::from_value(serde_json::json!({
            "calibrated": true, "calMin": 1, "calZero": 2, "calMax": 3, "firingTimeMeasured": 4, "sensorData": [0,0,0,0,0,0,0,0]
        }))
        .unwrap();
        assert_eq!(cal.cal_max, 3);
    }
}
