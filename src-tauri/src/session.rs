//! Device session manager (docs/PLAN.md §4.3).
//!
//! * A watcher thread rescans the bus once a second. New Pinscape devices are
//!   opened and handed to their own **worker thread**, which owns the USB
//!   connection and runs jobs one at a time (the vendor protocol is strictly
//!   request/response, and access is exclusive).
//! * When a device disappears (unplug, reset, firmware update), its worker
//!   exits and the device is dropped from the list; when it comes back it's
//!   re-attached under the same hardware ID, so the UI keeps its selection.
//! * Every change to the device list is pushed to the UI as a
//!   `devices-changed` event carrying the full list.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{Datelike, Timelike};
use pinscape_protocol::{ClockArgs, DeviceIds, Version};
use pinscape_usb::{self as usb, VendorClient};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut VendorClient) -> bool + Send>;

/// What the UI knows about one attached device.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    /// Hardware ID when known; otherwise a bus-location key (for devices we couldn't open).
    pub key: String,
    pub vid: u16,
    pub pid: u16,
    pub ids: Option<DeviceIds>,
    pub version: Option<Version>,
    /// Set when the device was found but couldn't be opened or queried.
    pub error: Option<String>,
    /// `permission`, `busy` or `other`, so the UI can show targeted help.
    pub error_kind: Option<&'static str>,
}

struct Worker {
    tx: mpsc::Sender<Job>,
    snapshot: DeviceSnapshot,
}

#[derive(Default)]
struct State {
    /// Keyed by `DeviceSnapshot::key`.
    devices: HashMap<String, Worker>,
    /// Devices that failed to open, keyed by nusb device id, so we don't retry every second.
    failed: HashMap<usb::DeviceId, DeviceSnapshot>,
    /// nusb device ids currently owned by a worker.
    owned: HashMap<usb::DeviceId, String>,
}

#[derive(Clone)]
pub struct Manager {
    state: Arc<Mutex<State>>,
    app: AppHandle,
}

impl Manager {
    pub fn start(app: AppHandle) -> Manager {
        let m = Manager { state: Arc::default(), app };
        if std::env::var_os("PINSCAPE_MOCK").is_some() {
            m.add_mock();
        }
        let watcher = m.clone();
        thread::Builder::new()
            .name("pinscape-watcher".into())
            .spawn(move || loop {
                watcher.scan();
                thread::sleep(Duration::from_secs(1));
            })
            .expect("spawn watcher");
        m
    }

    /// Current device list, sorted by unit number then hardware ID.
    pub fn snapshot(&self) -> Vec<DeviceSnapshot> {
        let st = self.state.lock().unwrap();
        let mut v: Vec<_> = st.devices.values().map(|w| w.snapshot.clone()).collect();
        v.extend(st.failed.values().cloned());
        v.sort_by_key(|d| (d.ids.as_ref().map(|i| i.unit_num).unwrap_or(u8::MAX), d.key.clone()));
        v
    }

    fn emit(&self) {
        let _ = self.app.emit("devices-changed", self.snapshot());
    }

    /// One pass of device discovery.
    fn scan(&self) {
        let found = match usb::find_devices() {
            Ok(f) => f,
            Err(_) => return,
        };
        let present: HashSet<usb::DeviceId> = found.iter().map(|c| c.info.id()).collect();
        let mut changed = false;

        // Forget failed devices that went away, so a replug retries them.
        {
            let mut st = self.state.lock().unwrap();
            let before = st.failed.len();
            st.failed.retain(|id, _| present.contains(id));
            changed |= st.failed.len() != before;
        }

        for c in found {
            let id = c.info.id();
            {
                let st = self.state.lock().unwrap();
                if st.owned.contains_key(&id) || st.failed.contains_key(&id) {
                    continue;
                }
            }
            changed = true;
            let s = c.summary();
            match open_and_identify(&c) {
                Ok((client, ids, version)) => {
                    let key = ids.hwid.clone();
                    let snapshot = DeviceSnapshot {
                        key: key.clone(),
                        vid: s.vid,
                        pid: s.pid,
                        ids: Some(ids),
                        version: Some(version),
                        error: None,
                        error_kind: None,
                    };
                    let tx = self.spawn_worker(key.clone(), Some(id), client);
                    let mut st = self.state.lock().unwrap();
                    st.owned.insert(id, key.clone());
                    st.devices.insert(key, Worker { tx, snapshot });
                }
                Err(e) => {
                    let snapshot = DeviceSnapshot {
                        key: format!("usb:{id:?}"),
                        vid: s.vid,
                        pid: s.pid,
                        ids: None,
                        version: None,
                        error: Some(e.to_string()),
                        error_kind: Some(match e {
                            usb::Error::PermissionDenied => "permission",
                            usb::Error::Busy => "busy",
                            _ => "other",
                        }),
                    };
                    self.state.lock().unwrap().failed.insert(id, snapshot);
                }
            }
        }

        if changed {
            self.emit();
        }
    }

    /// Attach a simulated device (PINSCAPE_MOCK=1), for UI work without hardware.
    fn add_mock(&self) {
        let mut client = VendorClient::with_transport(Box::new(usb::mock::MockDevice::new()));
        let (Ok(ids), Ok(version)) = (client.query_ids(), client.query_version()) else { return };
        let key = ids.hwid.clone();
        let snapshot = DeviceSnapshot {
            key: key.clone(),
            vid: pinscape_protocol::DEFAULT_VID,
            pid: pinscape_protocol::DEFAULT_PIDS[1],
            ids: Some(ids),
            version: Some(version),
            error: None,
            error_kind: None,
        };
        let tx = self.spawn_worker(key.clone(), None, client);
        self.state.lock().unwrap().devices.insert(key, Worker { tx, snapshot });
    }

    fn spawn_worker(&self, key: String, id: Option<usb::DeviceId>, mut client: VendorClient) -> mpsc::Sender<Job> {
        let (tx, rx) = mpsc::channel::<Job>();
        let mgr = self.clone();
        thread::Builder::new()
            .name(format!("pinscape-{key}"))
            .spawn(move || {
                let mut idle_pings = 0u32;
                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        // A job returns false when the device is gone.
                        Ok(job) => {
                            if !job(&mut client) {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Detect silent disconnects while idle.
                            idle_pings += 1;
                            if idle_pings.is_multiple_of(2) && is_disconnect_result(client.ping()) {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                drop(client);
                let mut st = mgr.state.lock().unwrap();
                if let Some(id) = id {
                    st.owned.remove(&id);
                }
                st.devices.remove(&key);
                drop(st);
                mgr.emit();
            })
            .expect("spawn worker");
        tx
    }

    /// Run `f` on the device's worker thread and await its result.
    pub async fn run<T, F>(&self, key: &str, f: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&mut VendorClient) -> usb::Result<T> + Send + 'static,
    {
        let tx = {
            let st = self.state.lock().unwrap();
            st.devices
                .get(key)
                .map(|w| w.tx.clone())
                .ok_or_else(|| format!("device {key} is not connected"))?
        };
        let (otx, orx) = oneshot::channel();
        let job: Job = Box::new(move |client| {
            let r = f(client);
            let alive = !matches!(&r, Err(e) if is_disconnect(e));
            let _ = otx.send(r.map_err(|e| e.to_string()));
            alive
        });
        tx.send(job).map_err(|_| format!("device {key} disconnected"))?;
        orx.await.map_err(|_| format!("device {key} disconnected"))?
    }
}

fn open_and_identify(c: &usb::Candidate) -> usb::Result<(VendorClient, DeviceIds, Version)> {
    let mut client = c.open()?;
    let ids = client.query_ids()?;
    let version = client.query_version()?;
    // Give the device the wall-clock time for its log timestamps (the
    // Windows tool does the same on connect). Not fatal if unsupported.
    let _ = client.set_clock(local_clock());
    Ok((client, ids, version))
}

pub fn local_clock() -> ClockArgs {
    let t = chrono::Local::now();
    ClockArgs {
        year: t.year() as i16,
        month: t.month() as u8,
        day: t.day() as u8,
        hour: t.hour() as u8,
        minute: t.minute() as u8,
        second: t.second() as u8,
    }
}

fn is_disconnect(e: &usb::Error) -> bool {
    e.is_disconnect()
}

fn is_disconnect_result(r: usb::Result<()>) -> bool {
    matches!(&r, Err(e) if e.is_disconnect() || matches!(e, usb::Error::Timeout))
}
