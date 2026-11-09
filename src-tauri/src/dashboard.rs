//! Multi-device dashboard: a cheap status poll of every attached device, plus
//! detection of problems that only show up across devices (duplicate unit
//! numbers, overlapping LedWiz units, mixed firmware versions, ...).
//!
//! The UI calls `dashboard_poll` every few seconds while the dashboard is
//! visible. Each device's queries run as one job on that device's own worker
//! thread (docs/PLAN.md §4.3), and the devices are queried concurrently, so a
//! slow device doesn't hold up the others' workers.

use std::collections::{BTreeMap, HashMap};

use pinscape_protocol::{config, DeviceIds, Statistics, UsbInterfaces, Version};
use serde::Serialize;
use tauri::State;

use crate::session::{DeviceSnapshot, Manager};

/// The firmware's identity when the config has no `id` section, which is also
/// what a device in Safe Mode or Factory Mode usually reports (Main.cpp).
const DEFAULT_UNIT_NUM: u8 = 1;
const DEFAULT_UNIT_NAME: &str = "Pinscape";

/// Live status of one device (what the Overview tab shows, minus GPIO).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashStatus {
    /// Fresh IDs (the XInput player index can change while connected).
    pub ids: Option<DeviceIds>,
    pub stats: Statistics,
    pub usb: UsbInterfaces,
    pub has_main_config: bool,
    pub has_safe_mode_config: bool,
    /// `None` when the outputs query isn't available.
    pub test_mode: Option<bool>,
    pub night_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashDevice {
    pub key: String,
    pub status: Option<DashStatus>,
    /// Why the status query failed (device busy rebooting, timed out, ...).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub severity: Severity,
    /// Stable identifier: `duplicate-unit`, `duplicate-xinput`, `ledwiz-overlap`,
    /// `firmware-mismatch`, `test-mode`, `maybe-safe-mode`, `no-config`, `unavailable`.
    pub kind: &'static str,
    pub message: String,
    /// Device keys involved (so the UI can mark their cards).
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashReport {
    pub devices: Vec<DashDevice>,
    pub problems: Vec<Problem>,
}

/// One status round trip per connected device (run concurrently, each on its
/// own worker), plus the cross-device problem list.
#[tauri::command]
pub async fn dashboard_poll(mgr: State<'_, Manager>) -> Result<DashReport, String> {
    let snaps = mgr.snapshot();
    let mut jobs = Vec::new();
    for d in snaps.iter().filter(|d| d.ids.is_some()) {
        let (m, key) = (mgr.inner().clone(), d.key.clone());
        jobs.push((d.key.clone(), tauri::async_runtime::spawn(async move { m.run(&key, query_status).await })));
    }
    let mut devices = Vec::new();
    for (key, job) in jobs {
        let r = job.await.map_err(|e| e.to_string()).and_then(|r| r);
        let (status, error) = match r {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        devices.push(DashDevice { key, status, error });
    }
    let status: HashMap<String, DashStatus> =
        devices.iter().filter_map(|d| Some((d.key.clone(), d.status.clone()?))).collect();
    Ok(DashReport { problems: detect_problems(&snaps, &status), devices })
}

fn query_status(c: &mut pinscape_usb::VendorClient) -> pinscape_usb::Result<DashStatus> {
    Ok(DashStatus {
        ids: c.query_ids().ok(),
        stats: c.query_stats(false)?,
        usb: c.query_usb_interfaces()?,
        has_main_config: c.config_exists(config::FILE_MAIN)?,
        has_safe_mode_config: c.config_exists(config::FILE_SAFE_MODE)?,
        test_mode: c.query_logical_port_levels().ok().map(|l| l.test_mode),
        night_mode: c.query_night_mode().ok(),
    })
}

fn label(ids: &DeviceIds) -> String {
    format!("#{} {}", ids.unit_num, ids.unit_name)
}

fn fmt_version(v: &Version) -> String {
    format!("{}.{}.{}", v.major, v.minor, v.patch)
}

fn join(labels: &[String]) -> String {
    match labels {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {}", init.join(", "), last),
    }
}

/// Problems across the attached devices. `status` holds the latest live status
/// by device key; checks that need it are skipped for devices without one.
pub fn detect_problems(devices: &[DeviceSnapshot], status: &HashMap<String, DashStatus>) -> Vec<Problem> {
    let mut out = Vec::new();
    // Connected Pinscape devices with their (freshest) IDs.
    let live: Vec<(&DeviceSnapshot, &DeviceIds, Option<&DashStatus>)> = devices
        .iter()
        .filter_map(|d| {
            let st = status.get(&d.key);
            let ids = st.and_then(|s| s.ids.as_ref()).or(d.ids.as_ref())?;
            Some((d, ids, st))
        })
        .collect();

    // Unit numbers identify the devices to DOF and the config tools.
    let mut by_unit: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
    for (i, (_, ids, _)) in live.iter().enumerate() {
        by_unit.entry(ids.unit_num).or_default().push(i);
    }
    for (unit, idx) in by_unit.iter().filter(|(_, v)| v.len() > 1) {
        let labels: Vec<_> = idx.iter().map(|&i| format!("{} ({})", label(live[i].1), live[i].1.hwid)).collect();
        out.push(Problem {
            severity: Severity::Error,
            kind: "duplicate-unit",
            message: format!(
                "{} devices use unit number {unit}: {}. DOF and other programs tell Pinscape units apart by this \
                 number; give each device its own id.unitNum.",
                idx.len(),
                join(&labels)
            ),
            keys: idx.iter().map(|&i| live[i].0.key.clone()).collect(),
        });
    }

    // XInput player numbers are assigned by Windows; two devices showing the same one is odd.
    // Only devices with XInput enabled count: firmware reports a player index
    // even when the interface isn't configured (seen on hardware).
    let mut by_player: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
    for (i, (_, ids, st)) in live.iter().enumerate() {
        if !st.is_some_and(|s| s.usb.xinput_configured) {
            continue;
        }
        if let Some(p) = ids.xinput_player_index {
            by_player.entry(p).or_default().push(i);
        }
    }
    for (p, idx) in by_player.iter().filter(|(_, v)| v.len() > 1) {
        let labels: Vec<_> = idx.iter().map(|&i| label(live[i].1)).collect();
        out.push(Problem {
            severity: Severity::Warning,
            kind: "duplicate-xinput",
            message: format!("{} report the same XInput player number ({}).", join(&labels), p + 1),
            keys: idx.iter().map(|&i| live[i].0.key.clone()).collect(),
        });
    }

    // LedWiz emulation: each LedWiz unit number may be claimed by one device only.
    let mut by_lw: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
    for (i, (_, ids, st)) in live.iter().enumerate() {
        if st.is_some_and(|s| s.usb.ledwiz_configured) {
            for bit in 0..16u8 {
                if ids.led_wiz_unit_mask & (1 << bit) != 0 {
                    by_lw.entry(bit + 1).or_default().push(i);
                }
            }
        }
    }
    for (unit, idx) in by_lw.iter().filter(|(_, v)| v.len() > 1) {
        let labels: Vec<_> = idx.iter().map(|&i| label(live[i].1)).collect();
        out.push(Problem {
            severity: Severity::Warning,
            kind: "ledwiz-overlap",
            message: format!(
                "{} all answer as LedWiz unit {unit}. LedWiz software would drive them as one device; give each a \
                 different id.ledWizUnitNum.",
                join(&labels)
            ),
            keys: idx.iter().map(|&i| live[i].0.key.clone()).collect(),
        });
    }

    // Mixed firmware versions.
    let mut by_version: BTreeMap<(u8, u8, u8), Vec<usize>> = BTreeMap::new();
    for (i, (d, _, _)) in live.iter().enumerate() {
        if let Some(v) = &d.version {
            by_version.entry((v.major, v.minor, v.patch)).or_default().push(i);
        }
    }
    if by_version.len() > 1 {
        let parts: Vec<_> = by_version
            .values()
            .rev()
            .map(|idx| {
                let labels: Vec<_> = idx.iter().map(|&i| label(live[i].1)).collect();
                format!("v{} on {}", fmt_version(live[idx[0]].0.version.as_ref().unwrap()), join(&labels))
            })
            .collect();
        out.push(Problem {
            severity: Severity::Warning,
            kind: "firmware-mismatch",
            message: format!("The devices run different firmware versions: {}.", parts.join("; ")),
            keys: by_version.values().flatten().map(|&i| live[i].0.key.clone()).collect(),
        });
    }

    // Per-device conditions worth seeing at a glance.
    for (d, ids, st) in &live {
        let Some(st) = st else { continue };
        if st.test_mode == Some(true) {
            out.push(Problem {
                severity: Severity::Warning,
                kind: "test-mode",
                message: format!(
                    "Output test mode is active on {}: its outputs ignore DOF and games until test mode ends.",
                    label(ids)
                ),
                keys: vec![d.key.clone()],
            });
        }
        if !st.has_main_config {
            out.push(Problem {
                severity: Severity::Info,
                kind: "no-config",
                message: format!("{} has no configuration stored and runs on factory defaults.", label(ids)),
                keys: vec![d.key.clone()],
            });
        } else if ids.unit_num == DEFAULT_UNIT_NUM && ids.unit_name == DEFAULT_UNIT_NAME {
            // The firmware doesn't report its boot mode over USB, so this is a hint only.
            out.push(Problem {
                severity: Severity::Info,
                kind: "maybe-safe-mode",
                message: format!(
                    "{} reports the default identity although a configuration is stored. It may be running in Safe \
                     Mode or Factory Mode (check its Log tab), or the configuration has no id section.",
                    label(ids)
                ),
                keys: vec![d.key.clone()],
            });
        }
    }

    // Devices found but not usable.
    for d in devices.iter().filter(|d| d.ids.is_none()) {
        let why = match d.error_kind {
            Some("permission") => "the app has no permission to open it",
            Some("busy") => "it's in use by another program",
            _ => "it can't be opened",
        };
        out.push(Problem {
            severity: Severity::Warning,
            kind: "unavailable",
            message: format!("A Pinscape Pico was found but {why}, so it can't be checked."),
            keys: vec![d.key.clone()],
        });
    }

    out.sort_by_key(|p| p.severity);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(hwid: &str, unit: u8, name: &str, ver: (u8, u8, u8)) -> DeviceSnapshot {
        DeviceSnapshot {
            key: hwid.into(),
            vid: 0x1209,
            pid: 0xEAEB,
            ids: Some(DeviceIds {
                hwid: hwid.into(),
                cpu_type: 2040,
                cpu_version: 2,
                rom_version: 3,
                unit_num: unit,
                xinput_player_index: None,
                led_wiz_unit_mask: 1,
                unit_name: name.into(),
                target_board: "pico".into(),
                pico_sdk_version: String::new(),
                tinyusb_version: String::new(),
                compiler_version: String::new(),
            }),
            version: Some(Version { major: ver.0, minor: ver.1, patch: ver.2, build_date: String::new() }),
            error: None,
            error_kind: None,
        }
    }

    fn configured() -> DashStatus {
        DashStatus { has_main_config: true, test_mode: Some(false), ..Default::default() }
    }

    fn statuses(devs: &[DeviceSnapshot], f: impl Fn(&DeviceSnapshot) -> DashStatus) -> HashMap<String, DashStatus> {
        devs.iter().map(|d| (d.key.clone(), f(d))).collect()
    }

    fn kinds(p: &[Problem]) -> Vec<&'static str> {
        p.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn single_or_distinct_devices_have_no_problems() {
        let one = vec![dev("A", 1, "Cab", (1, 0, 2))];
        assert!(detect_problems(&one, &statuses(&one, |_| configured())).is_empty());
        let two = vec![dev("A", 1, "Buttons", (1, 0, 2)), dev("B", 2, "Outputs", (1, 0, 2))];
        assert!(detect_problems(&two, &statuses(&two, |_| configured())).is_empty());
        assert!(detect_problems(&[], &HashMap::new()).is_empty());
    }

    #[test]
    fn duplicate_unit_numbers() {
        let devs = vec![dev("A", 2, "One", (1, 0, 2)), dev("B", 3, "Two", (1, 0, 2)), dev("C", 2, "Three", (1, 0, 2))];
        let p = detect_problems(&devs, &statuses(&devs, |_| configured()));
        assert_eq!(kinds(&p), ["duplicate-unit"]);
        assert_eq!(p[0].severity, Severity::Error);
        assert_eq!(p[0].keys, ["A", "C"]);
        assert!(p[0].message.contains("#2 One (A) and #2 Three (C)"), "{}", p[0].message);
    }

    #[test]
    fn duplicate_unit_detected_without_live_status() {
        // Checks based on IDs work before the first status poll arrives.
        let devs = vec![dev("A", 5, "One", (1, 0, 2)), dev("B", 5, "Two", (1, 0, 2))];
        assert_eq!(kinds(&detect_problems(&devs, &HashMap::new())), ["duplicate-unit"]);
    }

    #[test]
    fn fresh_ids_from_status_win_over_snapshot() {
        let devs = vec![dev("A", 1, "One", (1, 0, 2)), dev("B", 2, "Two", (1, 0, 2))];
        let st = statuses(&devs, |d| {
            let mut ids = d.ids.clone().unwrap();
            ids.xinput_player_index = Some(0);
            let mut s = DashStatus { ids: Some(ids), ..configured() };
            s.usb.xinput_configured = true;
            s
        });
        let p = detect_problems(&devs, &st);
        assert_eq!(kinds(&p), ["duplicate-xinput"]);
        assert!(p[0].message.contains("player number (1)"));
    }

    #[test]
    fn xinput_player_ignored_when_xinput_is_off() {
        // Real firmware reports a player index with XInput not configured.
        let devs = vec![dev("A", 1, "One", (1, 0, 2)), dev("B", 2, "Two", (1, 0, 2))];
        let st = statuses(&devs, |d| {
            let mut ids = d.ids.clone().unwrap();
            ids.xinput_player_index = Some(0);
            let mut s = DashStatus { ids: Some(ids), ..configured() };
            s.usb.xinput_configured = d.key == "A";
            s
        });
        assert!(kinds(&detect_problems(&devs, &st)).is_empty());
    }

    #[test]
    fn ledwiz_overlap_only_when_emulation_configured() {
        let mut devs = vec![dev("A", 1, "One", (1, 0, 2)), dev("B", 2, "Two", (1, 0, 2))];
        devs[0].ids.as_mut().unwrap().led_wiz_unit_mask = 0b0110;
        devs[1].ids.as_mut().unwrap().led_wiz_unit_mask = 0b0100;
        // Both default to mask 1 / overlapping masks, but LedWiz isn't configured: fine.
        assert!(detect_problems(&devs, &statuses(&devs, |_| configured())).is_empty());
        let lw = |_: &DeviceSnapshot| {
            let mut s = configured();
            s.usb.ledwiz_configured = true;
            s
        };
        let p = detect_problems(&devs, &statuses(&devs, lw));
        assert_eq!(kinds(&p), ["ledwiz-overlap"]);
        assert!(p[0].message.contains("LedWiz unit 3"), "{}", p[0].message);
        assert_eq!(p[0].keys, ["A", "B"]);
    }

    #[test]
    fn firmware_mismatch_lists_versions_newest_first() {
        let devs = vec![dev("A", 1, "One", (1, 0, 2)), dev("B", 2, "Two", (1, 0, 1)), dev("C", 3, "Three", (1, 0, 2))];
        let p = detect_problems(&devs, &statuses(&devs, |_| configured()));
        assert_eq!(kinds(&p), ["firmware-mismatch"]);
        assert_eq!(p[0].message, "The devices run different firmware versions: v1.0.2 on #1 One and #3 Three; v1.0.1 on #2 Two.");
        assert_eq!(p[0].keys.len(), 3);
    }

    #[test]
    fn per_device_conditions() {
        let devs = vec![dev("A", 1, "Pinscape", (1, 0, 2)), dev("B", 2, "Two", (1, 0, 2)), dev("C", 3, "Three", (1, 0, 2))];
        let st = statuses(&devs, |d| match d.key.as_str() {
            "A" => configured(),
            "B" => DashStatus { test_mode: Some(true), ..configured() },
            _ => DashStatus { has_main_config: false, ..configured() },
        });
        let p = detect_problems(&devs, &st);
        assert_eq!(kinds(&p), ["test-mode", "maybe-safe-mode", "no-config"]);
        assert_eq!(p[0].keys, ["B"]);
        assert_eq!(p[1].keys, ["A"]);
        assert_eq!(p[2].keys, ["C"]);
        // Default identity without a stored config is just "no config", not a Safe Mode hint.
        let st = statuses(&devs[..1], |_| DashStatus { has_main_config: false, ..configured() });
        assert_eq!(kinds(&detect_problems(&devs[..1], &st)), ["no-config"]);
    }

    #[test]
    fn unavailable_devices_and_sorting() {
        let mut busy = dev("usb:1", 0, "", (0, 0, 0));
        busy.ids = None;
        busy.version = None;
        busy.error_kind = Some("busy");
        let devs = vec![busy, dev("A", 4, "One", (1, 0, 2)), dev("B", 4, "Two", (1, 0, 2))];
        let p = detect_problems(&devs, &statuses(&devs[1..], |_| DashStatus { test_mode: Some(true), ..configured() }));
        assert_eq!(kinds(&p), ["duplicate-unit", "test-mode", "test-mode", "unavailable"]);
        assert!(p[3].message.contains("in use by another program"));
        assert_eq!(p[3].keys, ["usb:1"]);
    }

    #[test]
    fn mock_fleet_shows_every_problem_kind() {
        use pinscape_usb::{mock, VendorClient};
        let mut devs = Vec::new();
        let mut st = HashMap::new();
        for s in mock::fleet(4) {
            let mut c = VendorClient::with_transport(Box::new(mock::MockDevice::with_state(s)));
            let ids = c.query_ids().unwrap();
            let mut d = dev(&ids.hwid, 0, "", (0, 0, 0));
            d.version = Some(c.query_version().unwrap());
            d.ids = Some(ids);
            st.insert(d.key.clone(), query_status(&mut c).unwrap());
            devs.push(d);
        }
        let mut k = kinds(&detect_problems(&devs, &st));
        k.dedup();
        assert_eq!(k, ["duplicate-unit", "ledwiz-overlap", "firmware-mismatch", "test-mode", "no-config", "maybe-safe-mode"]);
    }
}
