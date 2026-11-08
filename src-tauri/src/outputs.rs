//! Outputs tester commands (CMD_OUTPUTS 0x11): logical ports, output
//! devices and device ports, levels, test mode, setting logical/physical
//! port levels, night mode.

use pinscape_protocol::outputs::{LogicalLevel, LogicalPort, OutputDevice};
use serde::Serialize;
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Longest test-mode timeout the UI may request; the UI renews it about once
/// a second, so the device reverts to normal soon after the app goes away.
const MAX_TEST_TIMEOUT_MS: u32 = 10_000;

/// Static output configuration (fetched once when the tab opens).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputsConfig {
    ports: Vec<LogicalPort>,
    devices: Vec<OutputDevice>,
}

/// Live state, polled while the tab is visible.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputsLevels {
    test_mode: bool,
    night_mode: bool,
    /// One per logical port (index 0 = port #1).
    logical: Vec<LogicalLevel>,
    /// One per device port, concatenated in device order (see `OutputDevice::first_port`).
    device: Vec<u16>,
}

#[tauri::command]
pub async fn outputs_config(mgr: State<'_, Manager>, key: String) -> Res<OutputsConfig> {
    mgr.run(&key, |c| Ok(OutputsConfig { ports: c.query_logical_ports()?, devices: c.query_output_devices_full()? }))
        .await
}

#[tauri::command]
pub async fn outputs_levels(mgr: State<'_, Manager>, key: String) -> Res<OutputsLevels> {
    mgr.run(&key, |c| {
        let l = c.query_logical_port_levels()?;
        Ok(OutputsLevels {
            test_mode: l.test_mode,
            logical: l.levels,
            device: c.query_device_port_levels()?,
            night_mode: c.query_night_mode()?,
        })
    })
    .await
}

/// Set a logical port (1-based) to 0..255, as DOF would.
#[tauri::command]
pub async fn outputs_set_port(mgr: State<'_, Manager>, key: String, port: u8, level: u8) -> Res<()> {
    mgr.run(&key, move |c| c.set_logical_port(port, level)).await
}

/// Enter (with a timeout, capped at 10 s and required to be non-zero) or leave test mode.
#[tauri::command]
pub async fn outputs_set_test_mode(mgr: State<'_, Manager>, key: String, enable: bool, timeout_ms: u32) -> Res<()> {
    let timeout = timeout_ms.clamp(500, MAX_TEST_TIMEOUT_MS);
    mgr.run(&key, move |c| c.set_output_test_mode(enable, if enable { timeout } else { 0 })).await
}

/// Set a physical device port on its native scale (test mode only).
#[tauri::command]
pub async fn outputs_set_device_port(
    mgr: State<'_, Manager>,
    key: String,
    dev_type: u8,
    config_index: u8,
    port: u8,
    level: u16,
) -> Res<()> {
    mgr.run(&key, move |c| c.set_device_port(dev_type, config_index, port, level)).await
}

#[tauri::command]
pub async fn outputs_set_night_mode(mgr: State<'_, Manager>, key: String, on: bool) -> Res<()> {
    mgr.run(&key, move |c| c.set_night_mode(on)).await
}

/// Turn everything off: all logical ports to 0 and, in test mode, every
/// physical output port to 0 as well.
#[tauri::command]
pub async fn outputs_all_off(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| {
        let levels = c.query_logical_port_levels()?;
        if levels.test_mode {
            // only ports actually configured as outputs (unused GPIOs may serve other functions)
            use pinscape_protocol::outputs::port_type;
            for d in c.query_output_devices_full()? {
                for (p, &t) in d.port_types.iter().enumerate().take(256) {
                    if t != port_type::UNUSED {
                        c.set_device_port(d.dev_type, d.config_index, p as u8, 0)?;
                    }
                }
            }
        }
        for p in 1..=levels.levels.len().min(255) {
            c.set_logical_port(p as u8, 0)?;
        }
        Ok(())
    })
    .await
}
