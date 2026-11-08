//! Firmware update / boot loader commands (docs/PLAN.md §4.4).
//!
//! * A watcher thread lists Picos in boot loader mode once a second and
//!   pushes `bootloaders-changed` events (a separate list from the Pinscape
//!   devices in `session.rs`; boot ROM devices are never opened by it).
//! * `firmware_install` runs the whole update: reboot a Pinscape device into
//!   its boot loader (optional), wait for the boot ROM, flash over PICOBOOT
//!   (or copy onto the boot drive as a fallback), and wait for the device to
//!   come back. Progress goes out as `firmware-progress` events.
//!
//! `PINSCAPE_MOCK_BOOTLOADER=1` adds a simulated RP2040 boot device (flash
//! in memory, slowed down) for UI work without hardware.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pinscape_protocol::reset;
use pinscape_protocol::uf2::{Chip, Uf2Image, Uf2Info};
use pinscape_protocol::Version;
use pinscape_usb::bootloader::{
    self as boot, sim::SimPicoboot, BootDevice, BootDeviceSummary, BootError, BootVolume, FlashOptions, Phase, Picoboot, Progress,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::session::Manager;

type Res<T> = Result<T, String>;

const SIM_KEY: &str = "boot:sim";
/// Largest file we'll treat as a UF2 image (the XIP window is 16 MB; UF2 doubles it).
const MAX_UF2: u64 = 40 * 1024 * 1024;

#[derive(Default)]
struct Inner {
    devices: Vec<BootDevice>,
    summaries: Vec<BootDeviceSummary>,
}

/// Managed state: the boot-device list and the install lock.
#[derive(Clone)]
pub struct Firmware {
    inner: Arc<Mutex<Inner>>,
    busy: Arc<AtomicBool>,
    sim: Option<SimPicoboot>,
}

impl Firmware {
    pub fn start(app: AppHandle) -> Firmware {
        let sim = std::env::var_os("PINSCAPE_MOCK_BOOTLOADER").map(|_| {
            let s = SimPicoboot::new(2 << 20, 0xFF);
            s.state().delay = Duration::from_millis(15);
            s
        });
        let fw = Firmware { inner: Arc::default(), busy: Arc::default(), sim };
        let w = fw.clone();
        thread::Builder::new()
            .name("pinscape-bootloader-watcher".into())
            .spawn(move || loop {
                if w.scan() {
                    let _ = app.emit("bootloaders-changed", w.list());
                }
                thread::sleep(Duration::from_secs(1));
            })
            .expect("spawn boot loader watcher");
        fw
    }

    /// Rescan; true if the list changed.
    fn scan(&self) -> bool {
        let Ok(devices) = boot::find_boot_devices() else { return false };
        let summaries: Vec<_> = devices.iter().map(|d| d.summary()).collect();
        let mut st = self.inner.lock().unwrap();
        let changed = st.summaries != summaries;
        st.devices = devices;
        st.summaries = summaries;
        changed
    }

    fn list(&self) -> Vec<BootDeviceSummary> {
        let mut v = self.inner.lock().unwrap().summaries.clone();
        if self.sim.is_some() {
            v.push(BootDeviceSummary {
                key: SIM_KEY.into(),
                chip: Chip::Rp2040,
                vid: 0x2E8A,
                pid: boot::RP2040_BOOT_PID,
                serial: Some("SIMULATED".into()),
                picoboot: true,
            });
        }
        v
    }

    fn device(&self, key: &str) -> Option<BootDevice> {
        self.inner.lock().unwrap().devices.iter().find(|d| d.key() == key).cloned()
    }

    fn open(&self, key: &str) -> Result<Picoboot, BootError> {
        if key == SIM_KEY {
            if let Some(s) = &self.sim {
                return Picoboot::connect(Box::new(s.clone()), Chip::Rp2040);
            }
        }
        self.device(key).ok_or(BootError::NotFound)?.open()
    }

    fn chip_of(&self, key: &str) -> Option<Chip> {
        if key == SIM_KEY && self.sim.is_some() {
            return Some(Chip::Rp2040);
        }
        self.device(key).map(|d| d.chip)
    }
}

/// Clears the install lock when dropped.
struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Uf2Summary {
    path: String,
    file_name: String,
    #[serde(flatten)]
    info: Uf2Info,
    /// Lowest flash address written and one past the highest (for the first compatible chip).
    flash_start: Option<u32>,
    flash_end: Option<u32>,
    /// Number of 4 KB sectors that would be erased.
    sectors: Option<u32>,
    /// Why the image can't be installed, if it can't (e.g. it reaches the config area).
    plan_error: Option<String>,
}

fn load_image(path: &str) -> Res<(Vec<u8>, Uf2Image)> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if meta.len() > MAX_UF2 {
        return Err(format!("{path} is too large to be a Pico firmware image ({} bytes)", meta.len()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let img = Uf2Image::parse(&bytes).map_err(|e| format!("{}: {e}", file_name(path)))?;
    Ok((bytes, img))
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn summarize(path: &str, img: &Uf2Image) -> Uf2Summary {
    let info = img.info();
    let (mut flash_start, mut flash_end, mut sectors, mut plan_error) = (None, None, None, None);
    match info.chips.first().map(|&c| img.plan(c)) {
        Some(Ok(p)) => {
            let (lo, hi) = p.range();
            (flash_start, flash_end, sectors) = (Some(lo), Some(hi), Some(p.sectors.len() as u32));
        }
        Some(Err(e)) => plan_error = Some(e.to_string()),
        None => plan_error = Some("no RP2040/RP2350 firmware in this file".into()),
    }
    Uf2Summary { path: path.to_string(), file_name: file_name(path), info, flash_start, flash_end, sectors, plan_error }
}

/// Read and validate a .uf2 file.
#[tauri::command]
pub fn firmware_inspect(path: String) -> Res<Uf2Summary> {
    let (_, img) = load_image(&path)?;
    Ok(summarize(&path, &img))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootList {
    devices: Vec<BootDeviceSummary>,
    volumes: Vec<BootVolume>,
}

/// Picos in boot loader mode, plus mounted boot drives (RPI-RP2 / RP2350).
#[tauri::command]
pub async fn bootloader_list(fw: State<'_, Firmware>) -> Res<BootList> {
    let fw = fw.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        fw.scan();
        BootList { devices: fw.list(), volumes: boot::find_boot_volumes() }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Leave the boot loader without installing anything (reboots into whatever is in flash).
#[tauri::command]
pub async fn bootloader_reboot(fw: State<'_, Firmware>, key: String) -> Res<()> {
    let fw = fw.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut pb = fw.open(&key).map_err(|e| open_hint(&e, fw.chip_of(&key)))?;
        pb.reboot(100).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Auto,
    Picoboot,
    Volume,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    path: String,
    /// A running Pinscape device to reboot into its boot loader first.
    device_key: Option<String>,
    /// A Pico already in boot loader mode (from `bootloader_list`).
    boot_key: Option<String>,
    /// A boot drive path (mass-storage method).
    volume: Option<String>,
    method: Method,
    verify: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    /// `picoboot` or `volume`.
    method: &'static str,
    /// The Pinscape device that came back (hardware ID), if seen.
    device_key: Option<String>,
    version: Option<Version>,
    notes: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    #[serde(flatten)]
    p: Progress,
    message: Option<String>,
}

fn emit(app: &AppHandle, phase: Phase, done: u64, total: u64, message: Option<String>) {
    let _ = app.emit("firmware-progress", ProgressEvent { p: Progress { phase, done, total }, message });
}

/// User-facing explanation for a failure to open the PICOBOOT interface.
fn open_hint(e: &BootError, chip: Option<Chip>) -> String {
    if e.is_permission() {
        if cfg!(target_os = "linux") {
            return "No permission to open the Pico boot loader (USB 2e8a). Install the udev rule \
                    packaging/udev/99-pinscape.rules (it covers the boot loader IDs), then unplug and replug the Pico."
                .into();
        }
        return format!("No permission to open the Pico boot loader: {e}");
    }
    if cfg!(windows) && chip == Some(Chip::Rp2040) {
        return format!(
            "Windows can't open the RP2040 boot loader's PICOBOOT interface without a driver ({e}). \
             Use the RPI-RP2 drive instead (the copy method)."
        );
    }
    format!("Can't open the Pico boot loader: {e}")
}

fn boot_keys(fw: &Firmware) -> HashSet<String> {
    fw.scan();
    fw.list().into_iter().map(|d| d.key).collect()
}

fn volume_paths() -> HashSet<PathBuf> {
    boot::find_boot_volumes().into_iter().map(|v| v.path).collect()
}

/// Poll `f` every 250 ms until it returns Some or `timeout` passes.
fn wait_for<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() >= end {
            return None;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn pick_volume(chip: Chip, explicit: Option<&str>, exclude: &HashSet<PathBuf>) -> Option<BootVolume> {
    if let Some(p) = explicit {
        return boot::probe_volume(Path::new(p)).filter(|v| v.chip == chip);
    }
    let vols: Vec<_> = boot::find_boot_volumes().into_iter().filter(|v| v.chip == chip).collect();
    // prefer a drive that just appeared; else the only one
    let fresh: Vec<_> = vols.iter().filter(|v| !exclude.contains(&v.path)).cloned().collect();
    match (fresh.len(), vols.len()) {
        (1, _) => fresh.into_iter().next(),
        (_, 1) => vols.into_iter().next(),
        _ => None,
    }
}

/// Install a UF2 image. Emits `firmware-progress` events throughout.
#[tauri::command]
pub async fn firmware_install(
    app: AppHandle,
    mgr: State<'_, Manager>,
    fw: State<'_, Firmware>,
    req: InstallRequest,
) -> Res<InstallResult> {
    let fw = fw.inner().clone();
    let mgr = mgr.inner().clone();
    if fw.busy.swap(true, Ordering::SeqCst) {
        return Err("a firmware installation is already running".into());
    }
    let guard = BusyGuard(fw.busy.clone());

    let (bytes, img) = load_image(&req.path)?;

    // Which chip are we installing onto?
    let snapshot = mgr.snapshot();
    let target_dev = match &req.device_key {
        Some(k) => Some(
            snapshot
                .iter()
                .find(|d| &d.key == k)
                .and_then(|d| d.ids.clone())
                .ok_or_else(|| format!("device {k} is not connected"))?,
        ),
        None => None,
    };
    let chip = if let Some(ids) = &target_dev {
        Chip::from_cpu_type(ids.cpu_type).ok_or_else(|| format!("unknown CPU type RP{}", ids.cpu_type))?
    } else if let Some(k) = &req.boot_key {
        fw.chip_of(k).ok_or("the boot loader device is no longer connected")?
    } else if let Some(v) = &req.volume {
        boot::probe_volume(Path::new(v)).ok_or_else(|| format!("{v} is not a Pico boot drive"))?.chip
    } else {
        return Err("no target device given".into());
    };

    // Validate before touching anything: family vs chip, flash ranges, config area.
    let plan = img
        .plan(chip)
        .map_err(|e| format!("This file can't be installed on this Pico ({}): {e}", chip.name()))?;

    let pinscape_before: HashSet<String> = snapshot.iter().map(|d| d.key.clone()).collect();
    let boot_before = boot_keys(&fw);
    let vols_before = volume_paths();

    if let Some(k) = &req.device_key {
        emit(&app, Phase::EnterBootLoader, 0, 0, None);
        mgr.run(k, |c| c.reset(reset::BOOTLOADER)).await?;
    }

    let fw2 = fw.clone();
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let fw = fw2;
        let app = app2;
        let mut notes = Vec::new();

        // Find the boot device (or drive) to use.
        let mut boot_key = req.boot_key.clone();
        if req.device_key.is_some() {
            emit(&app, Phase::WaitBootLoader, 0, 0, None);
            let found = wait_for(Duration::from_secs(20), || {
                fw.scan();
                if req.method != Method::Volume {
                    if let Some(d) = fw.list().into_iter().find(|d| !boot_before.contains(&d.key) && d.chip == chip) {
                        return Some(Some(d.key));
                    }
                }
                pick_volume(chip, None, &vols_before).filter(|v| !vols_before.contains(&v.path)).map(|_| None)
            });
            match found {
                Some(k) => boot_key = k,
                None => return Err("The Pico didn't appear in boot loader mode within 20 seconds. \
                    Hold BOOTSEL while plugging it in, then install from the boot loader entry in the sidebar."
                    .to_string()),
            }
        }

        // PICOBOOT first (unless the copy method was requested).
        let mut method = "volume";
        let mut picoboot_err = None;
        if req.method != Method::Volume {
            if let Some(k) = &boot_key {
                emit(&app, Phase::Connecting, 0, 0, None);
                // the boot ROM may need a moment after enumerating
                let pb = wait_for(Duration::from_secs(3), || match fw.open(k) {
                    Ok(pb) => Some(Ok(pb)),
                    Err(e) if e.is_permission() || matches!(e, BootError::NoPicobootInterface) => Some(Err(e)),
                    Err(_) => None,
                })
                .unwrap_or_else(|| fw.open(k));
                match pb {
                    Ok(mut pb) => {
                        let opts = FlashOptions { verify: req.verify, reboot: true };
                        boot::flash(&mut pb, &plan, opts, &mut |p| emit(&app, p.phase, p.done, p.total, None))
                            .map_err(|e| format!("Flashing failed: {e}. The Pico stays in boot loader mode; try again."))?;
                        method = "picoboot";
                    }
                    Err(e) => {
                        let hint = open_hint(&e, Some(chip));
                        if req.method == Method::Picoboot {
                            return Err(hint);
                        }
                        notes.push(format!("{hint} Falling back to copying the file onto the boot drive."));
                        picoboot_err = Some(hint);
                    }
                }
            } else if req.method == Method::Picoboot {
                return Err("No Pico in boot loader mode with a PICOBOOT interface was found.".into());
            }
        }

        if method == "volume" {
            emit(&app, Phase::WaitBootLoader, 0, 0, Some("Looking for the Pico's boot drive…".into()));
            let vol = wait_for(Duration::from_secs(10), || pick_volume(chip, req.volume.as_deref(), &vols_before));
            let Some(vol) = vol else {
                let base = "No mounted Pico boot drive (RPI-RP2 / RP2350) was found. If your system doesn't \
                            mount it automatically, mount it and try again.";
                return Err(match picoboot_err {
                    Some(h) => format!("{h} {base}"),
                    None => base.to_string(),
                });
            };
            boot::copy_to_volume(&vol.path, &bytes, &mut |p| emit(&app, p.phase, p.done, p.total, None))
                .map_err(|e| e.to_string())?;
            notes.push(format!("Copied to {}; the Pico restarts by itself.", vol.path.display()));
        }

        // Wait for Pinscape to come back.
        if boot_key.as_deref() == Some(SIM_KEY) {
            emit(&app, Phase::Done, 1, 1, None);
            notes.push("Simulated boot loader: nothing was installed on real hardware.".into());
            return Ok(InstallResult { method, device_key: None, version: None, notes });
        }
        emit(&app, Phase::WaitDevice, 0, 0, None);
        let back = wait_for(Duration::from_secs(30), || {
            mgr.snapshot().into_iter().find(|d| {
                d.version.is_some()
                    && match &req.device_key {
                        Some(k) => &d.key == k,
                        None => !pinscape_before.contains(&d.key),
                    }
            })
        });
        emit(&app, Phase::Done, 1, 1, None);
        match back {
            Some(d) => Ok(InstallResult { method, device_key: Some(d.key), version: d.version, notes }),
            None => {
                notes.push(
                    "The firmware was installed, but no Pinscape device has reported back yet. \
                     If it doesn't appear, check the USB connection or the device's config."
                        .into(),
                );
                Ok(InstallResult { method, device_key: None, version: None, notes })
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinscape_protocol::uf2::{build_image, family, FLASH_START};

    #[test]
    fn inspect_reports_range_and_plan_errors() {
        let dir = std::env::temp_dir().join(format!("pinscape-fw-inspect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.uf2");
        std::fs::write(&ok, build_image(family::RP2040, &[(FLASH_START, &[0u8; 5000])])).unwrap();
        let s = firmware_inspect(ok.display().to_string()).unwrap();
        assert_eq!(s.file_name, "ok.uf2");
        assert_eq!((s.flash_start, s.flash_end, s.sectors), (Some(FLASH_START), Some(FLASH_START + 5120), Some(2)));
        assert!(s.plan_error.is_none());

        let bad = dir.join("config-area.uf2");
        std::fs::write(&bad, build_image(family::RP2040, &[(FLASH_START + (2 << 20) - 256, &[0u8; 256])])).unwrap();
        let s = firmware_inspect(bad.display().to_string()).unwrap();
        assert!(s.plan_error.unwrap().contains("config"));

        let junk = dir.join("junk.uf2");
        std::fs::write(&junk, [0u8; 700]).unwrap();
        assert!(firmware_inspect(junk.display().to_string()).unwrap_err().contains("not a multiple of 512"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
