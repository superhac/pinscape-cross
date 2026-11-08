//! Auto-update support (Tauri updater plugin).
//!
//! The updater plugin is registered on desktop builds, but the UI only checks
//! for updates when [`updater_status`] says the build is set up for it:
//!
//! - `plugins.updater.pubkey` in `tauri.conf.json` is non-empty (until a real
//!   signing key exists it is `""`, so no check is ever made and nothing
//!   unsigned could be installed),
//! - at least one endpoint is configured,
//! - it's a release build (debug builds skip the check unless
//!   `PINSCAPE_UPDATER_DEV=1` is set), and
//! - `PINSCAPE_NO_UPDATE_CHECK` isn't set (opt-out for users and packagers).
//!
//! In-app installation is only offered for bundle types the updater can replace
//! in place (AppImage, MSI, NSIS, macOS .app). `.deb`/`.rpm` installs and
//! unbundled binaries get a link to the release page instead.

use serde::Serialize;

/// Where users download releases by hand.
pub const RELEASES_URL: &str = "https://github.com/superhac/pinscape-cross/releases/latest";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterStatus {
    /// Update checks may run.
    enabled: bool,
    /// Why checks are disabled (None when enabled).
    reason: Option<&'static str>,
    /// The updater can download and install a new version in place.
    can_install: bool,
    /// How this copy was installed: `appimage`, `deb`, `rpm`, `msi`, `nsis`,
    /// `app`, or None for an unbundled build.
    bundle_type: Option<&'static str>,
    /// Release page for manual downloads.
    releases_url: &'static str,
}

fn bundle_type() -> Option<&'static str> {
    use tauri::utils::{config::BundleType, platform};
    Some(match platform::bundle_type()? {
        BundleType::Deb => "deb",
        BundleType::Rpm => "rpm",
        BundleType::AppImage => "appimage",
        BundleType::Msi => "msi",
        BundleType::Nsis => "nsis",
        BundleType::App | BundleType::Dmg => "app",
    })
}

/// True if `plugins.updater` has a non-empty pubkey and at least one endpoint.
fn configured(config: &tauri::Config) -> bool {
    let Some(u) = config.plugins.0.get("updater") else {
        return false;
    };
    let pubkey = u.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
    let has_endpoint = u
        .get("endpoints")
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty());
    !pubkey.trim().is_empty() && has_endpoint
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

fn status(config: &tauri::Config, bundle: Option<&'static str>) -> UpdaterStatus {
    let reason = if cfg!(not(desktop)) {
        Some("not supported on this platform")
    } else if !configured(config) {
        Some("no update signing key is configured in this build")
    } else if env_set("PINSCAPE_NO_UPDATE_CHECK") {
        Some("disabled by PINSCAPE_NO_UPDATE_CHECK")
    } else if cfg!(debug_assertions) && !env_set("PINSCAPE_UPDATER_DEV") {
        Some("development build (set PINSCAPE_UPDATER_DEV=1 to test)")
    } else {
        None
    };
    UpdaterStatus {
        enabled: reason.is_none(),
        reason,
        can_install: matches!(bundle, Some("appimage" | "msi" | "nsis" | "app")),
        bundle_type: bundle,
        releases_url: RELEASES_URL,
    }
}

#[tauri::command]
pub fn updater_status(app: tauri::AppHandle) -> UpdaterStatus {
    status(app.config(), bundle_type())
}

/// Open the GitHub release page in the default browser (fixed URL).
#[tauri::command]
pub fn updater_open_releases() -> Result<(), String> {
    tauri_plugin_opener::open_url(RELEASES_URL, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(updater: serde_json::Value) -> tauri::Config {
        let mut c = tauri::Config::default();
        c.plugins.0.insert("updater".into(), updater);
        c
    }

    #[test]
    fn needs_pubkey_and_endpoint() {
        let ep = serde_json::json!(["https://example.com/latest.json"]);
        assert!(!configured(&tauri::Config::default()));
        assert!(!configured(&config(serde_json::json!({ "pubkey": "", "endpoints": ep }))));
        assert!(!configured(&config(serde_json::json!({ "pubkey": "  ", "endpoints": ep }))));
        assert!(!configured(&config(serde_json::json!({ "pubkey": "abc", "endpoints": [] }))));
        assert!(configured(&config(serde_json::json!({ "pubkey": "abc", "endpoints": ep }))));
    }

    #[test]
    fn shipped_config_is_valid() {
        // The real tauri.conf.json must keep a plugins.updater section (the
        // plugin fails to start without one) with a pubkey field and an https endpoint.
        let text = include_str!("../tauri.conf.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let u = &v["plugins"]["updater"];
        assert!(u["pubkey"].is_string(), "plugins.updater.pubkey must be a string");
        let eps = u["endpoints"].as_array().expect("endpoints");
        assert!(!eps.is_empty());
        assert!(eps.iter().all(|e| e.as_str().unwrap().starts_with("https://")));
        // createUpdaterArtifacts must stay off in the committed file: with it on,
        // `tauri build` fails without the private key (the release workflow
        // turns it on when the key is available).
        assert_ne!(v["bundle"]["createUpdaterArtifacts"], serde_json::json!(true));
    }

    #[test]
    fn disabled_without_key_and_install_by_bundle() {
        let s = status(&tauri::Config::default(), Some("deb"));
        assert!(!s.enabled);
        assert!(s.reason.is_some());
        assert!(!s.can_install);
        assert!(status(&tauri::Config::default(), Some("appimage")).can_install);
        assert!(!status(&tauri::Config::default(), None).can_install);
    }
}
