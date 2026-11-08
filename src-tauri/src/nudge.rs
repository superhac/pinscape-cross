//! Nudge device commands (CMD_NUDGE 0x07): accelerometer status,
//! parameters, noise calibration, centering, commit/revert.

use pinscape_protocol::nudge::{NudgeParams, NudgeStatus};
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Live readings, polled at ~30-60 Hz while the tab is visible.
#[tauri::command]
pub async fn nudge_status(mgr: State<'_, Manager>, key: String) -> Res<NudgeStatus> {
    mgr.run(&key, |c| c.query_nudge_status()).await
}

/// Current in-memory parameters.
#[tauri::command]
pub async fn nudge_params(mgr: State<'_, Manager>, key: String) -> Res<NudgeParams> {
    mgr.run(&key, |c| c.query_nudge_params()).await
}

/// Apply parameters (RAM only); returns the parameters as the device now reports them.
#[tauri::command]
pub async fn nudge_put_params(mgr: State<'_, Manager>, key: String, params: NudgeParams) -> Res<NudgeParams> {
    mgr.run(&key, move |c| {
        c.put_nudge_params(&params)?;
        c.query_nudge_params()
    })
    .await
}

/// Save the in-memory calibration and parameters to flash.
#[tauri::command]
pub async fn nudge_commit(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| c.commit_nudge_settings()).await
}

/// Reload the saved settings from flash; returns the reloaded parameters.
#[tauri::command]
pub async fn nudge_revert(mgr: State<'_, Manager>, key: String) -> Res<NudgeParams> {
    mgr.run(&key, |c| {
        c.revert_nudge_settings()?;
        c.query_nudge_params()
    })
    .await
}

/// Set the center point now from the recent average.
#[tauri::command]
pub async fn nudge_center(mgr: State<'_, Manager>, key: String) -> Res<()> {
    mgr.run(&key, |c| c.set_nudge_center_point()).await
}

/// Start the ~15 s noise calibration; `auto_save` commits the result when done.
#[tauri::command]
pub async fn nudge_calibrate(mgr: State<'_, Manager>, key: String, auto_save: bool) -> Res<()> {
    mgr.run(&key, move |c| c.start_nudge_calibration(auto_save)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_json_shape() {
        let p = NudgeParams { lp_freq_hz: 30, ..Default::default() };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["autoCenterIntervalMs"], 4000);
        assert_eq!(j["lpFreqHz"], 30);
        assert_eq!(j["velocityDecayMs"], 2000);
        let back: NudgeParams = serde_json::from_value(j).unwrap();
        assert_eq!(back, p);
        // fields the UI may omit
        let mut j = serde_json::to_value(&p).unwrap();
        j.as_object_mut().unwrap().remove("otherFlags");
        j.as_object_mut().unwrap().remove("hasLpFreq");
        let back: NudgeParams = serde_json::from_value(j).unwrap();
        assert!(back.has_lp_freq);

        let s = serde_json::to_value(NudgeStatus::default()).unwrap();
        assert!(s.get("xFiltered").is_some() && s.get("lastCenteringAgeUs").is_some());
        assert_eq!(s["hasAccelerometer"], false);
    }
}
