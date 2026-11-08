//! Config templates and file import/export.

use serde::Serialize;

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Intended for the Safe Mode config slot rather than the main config.
    pub safe_mode: bool,
    #[serde(skip)]
    text: &'static str,
}

/// Upstream templates (assets/templates, synced by scripts/sync-upstream-assets.sh).
const TEMPLATES: &[Template] = &[
    Template {
        id: "starter",
        name: "Starter",
        description: "General starting point: identification, USB interfaces and logging enabled, with commented-out examples of most settings.",
        safe_mode: false,
        text: include_str!("../../assets/templates/Starter.json"),
    },
    Template {
        id: "diy",
        name: "DIY-Friendly Expansion Board",
        description: "Hardware setup for the Pinscape Pico DIY-Friendly two-board expansion set.",
        safe_mode: false,
        text: include_str!("../../assets/templates/DIYExpansionBoard.json"),
    },
    Template {
        id: "pro",
        name: "Pro Expansion Board",
        description: "Hardware setup for the all-in-one Pinscape Pico \"Pro\" expansion board.",
        safe_mode: false,
        text: include_str!("../../assets/templates/ProExpansionBoard.json"),
    },
    Template {
        id: "opd-tester",
        name: "Open Pinball Device Tester",
        description: "Turns the Pico into an automated input source for testing Open Pinball Device clients.",
        safe_mode: false,
        text: include_str!("../../assets/templates/OpenPinDevTester.json"),
    },
    Template {
        id: "safe-mode",
        name: "Safe Mode Starter",
        description: "Minimal configuration for the Safe Mode slot, used after a crash at startup.",
        safe_mode: true,
        text: include_str!("../../assets/templates/SafeModeStarter.json"),
    },
];

#[tauri::command]
pub fn list_templates() -> Vec<Template> {
    TEMPLATES.to_vec()
}

#[tauri::command]
pub fn read_template(id: String) -> Result<String, String> {
    TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .map(|t| t.text.to_string())
        .ok_or_else(|| format!("unknown template '{id}'"))
}

/// Largest config the firmware accepts (128 KB); refuse anything wildly bigger.
const MAX_IMPORT: u64 = 1024 * 1024;

/// Read a text file chosen by the user in an Open dialog.
#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("{path}: {e}"))?;
    if meta.len() > MAX_IMPORT {
        return Err(format!("{path} is too large to be a config file ({} bytes)", meta.len()));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not a UTF-8 text file"))
}

/// Write a text file chosen by the user in a Save dialog.
#[tauri::command]
pub fn write_text_file(path: String, text: String) -> Result<(), String> {
    std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))
}

// ---- Automatic backups of every config saved to a device ----
// Like the Windows tool: one file per save, named
// <HWID>_<main|safemode>_<YYYYMMDD>_<HHMMSS>.json, pruned after the number of
// days set in Preferences (default 30; 0 keeps them forever). Backups can be
// turned off in Preferences.

use std::path::PathBuf;
use tauri::Manager;

pub fn backup_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?.join("backups");
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

fn slot(safe_mode: bool) -> &'static str {
    if safe_mode {
        "safemode"
    } else {
        "main"
    }
}

fn sanitize(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Message returned by [`write_backup`] when backups are turned off.
pub const BACKUPS_DISABLED: &str = "automatic backups are turned off in Preferences";

/// Save a backup copy of `text` (unless turned off in Preferences); returns its path.
pub fn write_backup(app: &tauri::AppHandle, hwid: &str, safe_mode: bool, text: &str) -> Result<PathBuf, String> {
    let prefs = crate::prefs::load(app).backups;
    if !prefs.enabled {
        return Err(BACKUPS_DISABLED.into());
    }
    write_backup_in(&backup_dir(app)?, hwid, safe_mode, text, prefs.retention_days)
}

fn write_backup_in(
    dir: &std::path::Path,
    hwid: &str,
    safe_mode: bool,
    text: &str,
    keep_days: u32,
) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let path = dir.join(format!("{}_{}_{stamp}.json", sanitize(hwid), slot(safe_mode)));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    prune_backups(dir, keep_days);
    Ok(path)
}

/// Delete backups older than `keep_days` (0 = keep everything).
fn prune_backups(dir: &std::path::Path, keep_days: u32) {
    if keep_days == 0 {
        return;
    }
    let cutoff = std::time::Duration::from_secs(u64::from(keep_days) * 86_400);
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|age| age > cutoff);
        if p.extension().is_some_and(|x| x == "json") && old == Some(true) {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file: String,
    /// `YYYY-MM-DD HH:MM:SS`, from the file name.
    pub saved_at: String,
    pub size: u64,
}

/// Backups for a device and config slot, newest first.
#[tauri::command]
pub fn list_backups(app: tauri::AppHandle, hwid: String, safe_mode: bool) -> Result<Vec<BackupInfo>, String> {
    list_backups_in(&backup_dir(&app)?, &hwid, safe_mode)
}

fn list_backups_in(dir: &std::path::Path, hwid: &str, safe_mode: bool) -> Result<Vec<BackupInfo>, String> {
    let prefix = format!("{}_{}_", sanitize(hwid), slot(safe_mode));
    let mut out: Vec<BackupInfo> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stamp = name.strip_prefix(&prefix)?.strip_suffix(".json")?;
            let (d, t) = stamp.split_once('_')?;
            if d.len() != 8 || t.len() != 6 {
                return None;
            }
            let saved_at = format!("{}-{}-{} {}:{}:{}", &d[0..4], &d[4..6], &d[6..8], &t[0..2], &t[2..4], &t[4..6]);
            Some(BackupInfo { file: name, saved_at, size: e.metadata().map(|m| m.len()).unwrap_or(0) })
        })
        .collect();
    out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Ok(out)
}

#[tauri::command]
pub fn read_backup(app: tauri::AppHandle, file: String) -> Result<String, String> {
    // only plain file names inside the backup folder
    if file.contains(['/', '\\']) || file.contains("..") {
        return Err("invalid backup name".into());
    }
    let path = backup_dir(&app)?.join(&file);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[tauri::command]
pub fn backup_folder(app: tauri::AppHandle) -> Result<String, String> {
    Ok(backup_dir(&app)?.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_roundtrip_and_filtering() {
        let dir = std::env::temp_dir().join(format!("pinscape-backup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = write_backup_in(&dir, "E664-8045", false, "{ id: {} }", 30).unwrap();
        assert!(p.file_name().unwrap().to_str().unwrap().starts_with("E6648045_main_"));
        write_backup_in(&dir, "E6648045", true, "{}", 30).unwrap();
        write_backup_in(&dir, "OTHERDEVICE", false, "{}", 30).unwrap();
        std::fs::write(dir.join("E6648045_main_garbage.json"), "x").unwrap();

        let main = list_backups_in(&dir, "E6648045", false).unwrap();
        assert_eq!(main.len(), 1, "only this device's main-slot backups, well-formed names only");
        assert_eq!(main[0].size, 10);
        assert_eq!(main[0].saved_at.len(), 19);
        assert_eq!(list_backups_in(&dir, "E6648045", true).unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prune_respects_retention() {
        let dir = std::env::temp_dir().join(format!("pinscape-prune-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("X_main_20000101_000000.json");
        std::fs::write(&old, "{}").unwrap();
        let ten_days = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400);
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(ten_days).unwrap();

        prune_backups(&dir, 0); // keep forever
        assert!(old.exists());
        prune_backups(&dir, 30);
        assert!(old.exists(), "younger than the retention period");
        prune_backups(&dir, 7);
        assert!(!old.exists(), "older than the retention period");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn templates_present() {
        assert_eq!(list_templates().len(), 5);
        assert!(read_template("starter".into()).unwrap().contains("Starter Template"));
        assert!(read_template("nope".into()).is_err());
    }
}
