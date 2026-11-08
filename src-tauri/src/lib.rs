//! Tauri application: exposes device operations to the web UI.
//!
//! Device access goes through [`session::Manager`], which keeps one worker
//! thread per attached device and pushes `devices-changed` events on hot-plug.
//! Commands are addressed by device key (the hardware ID).

mod buttons;
mod files;
mod ir;
mod outputs;
mod session;

use pinscape_protocol::{config, reset, GpioPort, Statistics, UsbInterfaces};
use serde::Serialize;
use session::{DeviceSnapshot, Manager};
use tauri::{Manager as _, State};

type Res<T> = Result<T, String>;

#[tauri::command]
fn list_devices(mgr: State<'_, Manager>) -> Vec<DeviceSnapshot> {
    mgr.snapshot()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Overview {
    stats: Statistics,
    usb: UsbInterfaces,
    gpio: Vec<GpioPort>,
    has_main_config: bool,
    has_safe_mode_config: bool,
}

/// Everything the Overview tab shows, in one round trip to the worker.
#[tauri::command]
async fn device_overview(mgr: State<'_, Manager>, key: String) -> Res<Overview> {
    mgr.run(&key, |c| {
        Ok(Overview {
            stats: c.query_stats(false)?,
            usb: c.query_usb_interfaces()?,
            gpio: c.query_gpio_config()?,
            has_main_config: c.config_exists(config::FILE_MAIN)?,
            has_safe_mode_config: c.config_exists(config::FILE_SAFE_MODE)?,
        })
    })
    .await
}

#[tauri::command]
async fn query_stats(mgr: State<'_, Manager>, key: String) -> Res<Statistics> {
    mgr.run(&key, |c| c.query_stats(false)).await
}

fn file_id(safe_mode: bool) -> u8 {
    if safe_mode {
        config::FILE_SAFE_MODE
    } else {
        config::FILE_MAIN
    }
}

/// Download the main or Safe Mode config text; `None` if none is stored.
#[tauri::command]
async fn get_config(mgr: State<'_, Manager>, key: String, safe_mode: bool) -> Res<Option<String>> {
    mgr.run(&key, move |c| {
        Ok(c.get_config(file_id(safe_mode))?.map(|d| String::from_utf8_lossy(&d).into_owned()))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PutResult {
    /// Backup copy written before the upload (None if it couldn't be written).
    backup: Option<String>,
    backup_error: Option<String>,
}

/// Upload config text, optionally resetting so it takes effect. A backup copy
/// is saved first (see files::write_backup).
#[tauri::command]
async fn put_config(
    app: tauri::AppHandle,
    mgr: State<'_, Manager>,
    key: String,
    safe_mode: bool,
    text: String,
    reset_after: bool,
) -> Res<PutResult> {
    let (backup, backup_error) = match files::write_backup(&app, &key, safe_mode, &text) {
        Ok(p) => (Some(p.display().to_string()), None),
        Err(e) => (None, Some(e)),
    };
    mgr.run(&key, move |c| {
        c.put_config(file_id(safe_mode), text.as_bytes())?;
        if reset_after {
            c.reset(reset::NORMAL)?;
        }
        Ok(())
    })
    .await?;
    Ok(PutResult { backup, backup_error })
}

/// Reboot: `normal`, `safe-mode`, `factory` or `bootloader`.
#[tauri::command]
async fn reset_device(mgr: State<'_, Manager>, key: String, mode: String) -> Res<()> {
    let m = match mode.as_str() {
        "normal" => reset::NORMAL,
        "safe-mode" => reset::SAFE_MODE,
        "factory" => reset::FACTORY,
        "bootloader" => reset::BOOTLOADER,
        other => return Err(format!("unknown reset mode '{other}'")),
    };
    mgr.run(&key, move |c| c.reset(m)).await
}

/// Erase `main`, `safe-mode` or `all` config files.
#[tauri::command]
async fn erase_config(mgr: State<'_, Manager>, key: String, which: String) -> Res<()> {
    let fid = match which.as_str() {
        "main" => config::FILE_MAIN,
        "safe-mode" => config::FILE_SAFE_MODE,
        "all" => config::FILE_ALL,
        other => return Err(format!("unknown config '{other}'")),
    };
    mgr.run(&key, move |c| c.erase_config(fid)).await
}

/// Drain whatever log text is waiting on the device (up to ~64 KB per call).
#[tauri::command]
async fn query_log(mgr: State<'_, Manager>, key: String) -> Res<String> {
    mgr.run(&key, |c| {
        let mut out = Vec::new();
        while out.len() < 65536 {
            match c.query_log()? {
                Some((text, avail)) => {
                    out.extend_from_slice(&text);
                    if avail == 0 {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    })
    .await
}

#[tauri::command]
async fn set_clock(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| c.set_clock(session::local_clock())).await
}

/// Development/automation options from the environment.
/// `PINSCAPE_START_TAB=Config` opens that tab at startup, and
/// `PINSCAPE_START_DEVICE=<hwid>` selects that device (handy for screenshots).
#[tauri::command]
fn dev_options() -> serde_json::Value {
    serde_json::json!({
        "startTab": std::env::var("PINSCAPE_START_TAB").ok(),
        "startDevice": std::env::var("PINSCAPE_START_DEVICE").ok(),
    })
}

/// Host OS: `linux`, `windows`, `macos`.
#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

// ---- Config editor services (pinscape-config) ----
// The text is sent with each call; parsing a 65 KB config takes well under a millisecond.

/// Syntax errors (firmware parser) and rules-check warnings (schema).
#[tauri::command]
fn config_validate(text: String) -> Vec<pinscape_config::Diagnostic> {
    pinscape_config::validate(&text)
}

#[tauri::command]
fn config_completions(text: String, line: u32, col: u32) -> Vec<pinscape_config::Completion> {
    pinscape_config::completions(&text, line, col)
}

#[tauri::command]
fn config_hover(text: String, line: u32, col: u32) -> Option<pinscape_config::Hover> {
    pinscape_config::hover(&text, line, col)
}

#[tauri::command]
fn config_breadcrumb(text: String, line: u32, col: u32) -> pinscape_config::Breadcrumb {
    pinscape_config::breadcrumb(&text, line, col)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let mgr = Manager::start(app.handle().clone());
            app.manage(mgr);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_devices,
            device_overview,
            query_stats,
            get_config,
            put_config,
            reset_device,
            erase_config,
            query_log,
            set_clock,
            config_validate,
            config_completions,
            config_hover,
            config_breadcrumb,
            platform,
            dev_options,
            files::list_templates,
            files::read_template,
            files::read_text_file,
            files::write_text_file,
            files::list_backups,
            files::read_backup,
            files::backup_folder,
            buttons::buttons_config,
            buttons::buttons_live,
            outputs::outputs_config,
            outputs::outputs_levels,
            outputs::outputs_set_port,
            outputs::outputs_set_test_mode,
            outputs::outputs_set_device_port,
            outputs::outputs_set_night_mode,
            outputs::outputs_all_off,
            ir::ir_poll,
            ir::ir_send,
            ir::ir_set_tv_relay,
            ir::ir_config_status,
            ir::ir_parse_code,
            ir::ir_protocols,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
