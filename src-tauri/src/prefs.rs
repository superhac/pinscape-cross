//! User preferences, stored as JSON in the app config folder
//! (`<app_config_dir>/preferences.json`).
//!
//! Forward/backward compatible: missing fields take their defaults, fields this
//! version doesn't know are kept (and written back) untouched, and a field with
//! an invalid value falls back to its default instead of discarding the file.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    /// Follow the OS (also used for unknown values).
    #[default]
    #[serde(other)]
    System,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiFontSize {
    Small,
    Large,
    #[default]
    #[serde(other)]
    Normal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorPrefs {
    pub font_size: u32,
    pub tab_size: u32,
    pub insert_spaces: bool,
    pub minimap: bool,
    pub word_wrap: bool,
    pub emacs_keys: bool,
    /// Ask for confirmation before writing a config to the device (warnings
    /// from the rules check are always confirmed).
    pub confirm_save: bool,
    /// Initial state of the "reboot after saving" checkbox.
    pub reboot_after_save: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        Self {
            font_size: 13,
            tab_size: 4,
            insert_spaces: true,
            minimap: true,
            word_wrap: false,
            emacs_keys: false,
            confirm_save: true,
            reboot_after_save: true,
            extra: Map::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupPrefs {
    /// Keep a copy of every config saved to a device.
    pub enabled: bool,
    /// Delete backups older than this many days (0 = keep forever).
    pub retention_days: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for BackupPrefs {
    fn default() -> Self {
        Self { enabled: true, retention_days: 30, extra: Map::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StartupPrefs {
    pub remember_tab: bool,
    pub remember_device: bool,
    pub last_tab: Option<String>,
    pub last_device: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for StartupPrefs {
    fn default() -> Self {
        Self { remember_tab: true, remember_device: true, last_tab: None, last_device: None, extra: Map::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub theme: Theme,
    pub ui_font_size: UiFontSize,
    pub editor: EditorPrefs,
    pub backups: BackupPrefs,
    pub startup: StartupPrefs,
    /// Settings written by other (newer) versions; preserved as-is.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Preferences {
    /// Clamp numbers into sensible ranges.
    pub fn sanitized(mut self) -> Self {
        let e = &mut self.editor;
        e.font_size = e.font_size.clamp(8, 32);
        e.tab_size = e.tab_size.clamp(1, 8);
        self.backups.retention_days = self.backups.retention_days.min(3650);
        self
    }

    /// Parse leniently: a bad value for one field resets only that field.
    pub fn from_json(text: &str) -> Self {
        if let Ok(p) = serde_json::from_str::<Preferences>(text) {
            return p.sanitized();
        }
        let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let mut acc = match serde_json::to_value(Self::default()) {
            Ok(Value::Object(m)) => m,
            _ => return Self::default(),
        };
        for (k, v) in obj {
            let merged = merge_field(&acc, &k, v);
            if serde_json::from_value::<Preferences>(Value::Object(merged.clone())).is_ok() {
                acc = merged;
            }
        }
        serde_json::from_value::<Preferences>(Value::Object(acc)).unwrap_or_default().sanitized()
    }
}

/// `acc` with field `k` set to `v`; nested objects are merged one level deep
/// field by field so one bad nested value doesn't drop its siblings.
fn merge_field(acc: &Map<String, Value>, k: &str, v: Value) -> Map<String, Value> {
    let mut out = acc.clone();
    match (out.get(k).cloned(), v) {
        (Some(Value::Object(base)), Value::Object(new)) => {
            let mut inner = base;
            for (nk, nv) in new {
                let mut trial = inner.clone();
                trial.insert(nk.clone(), nv.clone());
                let mut probe = out.clone();
                probe.insert(k.to_string(), Value::Object(trial));
                if serde_json::from_value::<Preferences>(Value::Object(probe)).is_ok() {
                    inner.insert(nk, nv);
                }
            }
            out.insert(k.to_string(), Value::Object(inner));
        }
        (_, v) => {
            out.insert(k.to_string(), v);
        }
    }
    out
}

pub fn prefs_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("preferences.json"))
}

pub fn load_from(path: &Path) -> Preferences {
    match std::fs::read_to_string(path) {
        Ok(t) => Preferences::from_json(&t),
        Err(_) => Preferences::default(),
    }
}

pub fn save_to(path: &Path, p: &Preferences) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    // write-then-rename so a crash never leaves a truncated file
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Current preferences (defaults if there's no file yet).
pub fn load(app: &tauri::AppHandle) -> Preferences {
    prefs_path(app).map(|p| load_from(&p)).unwrap_or_default()
}

#[tauri::command]
pub fn prefs_get(app: tauri::AppHandle) -> Preferences {
    load(&app)
}

/// Store the preferences; returns them as saved (numbers clamped).
#[tauri::command]
pub fn prefs_set(app: tauri::AppHandle, prefs: Preferences) -> Result<Preferences, String> {
    let p = prefs.sanitized();
    save_to(&prefs_path(&app)?, &p)?;
    Ok(p)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    /// Contents of assets/UPSTREAM_REVISION (upstream commit and date the
    /// bundled help, templates and schema were synced from).
    pub upstream_revision: &'static str,
}

#[tauri::command]
pub fn about_info() -> AboutInfo {
    AboutInfo { upstream_revision: include_str!("../../assets/UPSTREAM_REVISION").trim() }
}

/// Print the current page with the platform's print dialog (window.print()
/// isn't implemented by every webview, e.g. WKWebView on macOS).
#[tauri::command]
pub fn print_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty_or_garbage() {
        assert_eq!(Preferences::from_json(""), Preferences::default());
        assert_eq!(Preferences::from_json("not json"), Preferences::default());
        assert_eq!(Preferences::from_json("[1,2]"), Preferences::default());
        let d = Preferences::default();
        assert_eq!(d.theme, Theme::System);
        assert_eq!(d.editor.font_size, 13);
        assert!(d.editor.confirm_save);
        assert!(d.backups.enabled);
        assert_eq!(d.backups.retention_days, 30);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let p = Preferences::from_json(r#"{ "theme": "dark", "editor": { "fontSize": 16 } }"#);
        assert_eq!(p.theme, Theme::Dark);
        assert_eq!(p.editor.font_size, 16);
        assert_eq!(p.editor.tab_size, 4);
        assert_eq!(p.backups, BackupPrefs::default());
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let src = r#"{ "theme": "light", "futureThing": { "a": 1 }, "editor": { "futureKey": true } }"#;
        let p = Preferences::from_json(src);
        assert_eq!(p.extra.get("futureThing"), Some(&serde_json::json!({ "a": 1 })));
        let out = serde_json::to_value(&p).unwrap();
        assert_eq!(out["futureThing"]["a"], 1);
        assert_eq!(out["editor"]["futureKey"], true);
        assert_eq!(out["theme"], "light");
    }

    #[test]
    fn bad_values_reset_only_that_field() {
        // unknown enum value → default variant
        assert_eq!(Preferences::from_json(r#"{ "theme": "purple" }"#).theme, Theme::System);
        // wrong type for one nested field keeps its siblings
        let p = Preferences::from_json(r#"{ "theme": "dark", "editor": { "fontSize": "big", "tabSize": 2 }, "backups": 5 }"#);
        assert_eq!(p.theme, Theme::Dark);
        assert_eq!(p.editor.font_size, 13);
        assert_eq!(p.editor.tab_size, 2);
        assert_eq!(p.backups, BackupPrefs::default());
    }

    #[test]
    fn numbers_are_clamped() {
        let p = Preferences::from_json(r#"{ "editor": { "fontSize": 200, "tabSize": 0 }, "backups": { "retentionDays": 99999 } }"#);
        assert_eq!(p.editor.font_size, 32);
        assert_eq!(p.editor.tab_size, 1);
        assert_eq!(p.backups.retention_days, 3650);
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("pinscape-prefs-test-{}", std::process::id()));
        let path = dir.join("sub").join("preferences.json");
        assert_eq!(load_from(&path), Preferences::default());
        let mut p = Preferences { theme: Theme::Dark, ..Default::default() };
        p.startup.last_tab = Some("Config".into());
        p.backups.retention_days = 7;
        save_to(&path, &p).unwrap();
        assert_eq!(load_from(&path), p);
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn about_has_revision() {
        assert!(!about_info().upstream_revision.is_empty());
    }
}
