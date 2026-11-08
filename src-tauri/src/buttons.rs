//! Buttons: Tauri commands for CMD_BUTTONS (0x10): button descriptors, logical
//! button states, GPIO / PCA9555 / 74HC165 input states, shift state.

use pinscape_protocol::buttons::ButtonConfig;
use serde::Serialize;
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Logical button descriptors and physical input devices (fetched once per visit).
#[tauri::command]
pub async fn buttons_config(mgr: State<'_, Manager>, key: String) -> Res<ButtonConfig> {
    mgr.run(&key, |c| c.query_button_config()).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonsLive {
    /// Logical button states, in descriptor order.
    states: Vec<bool>,
    shift_state: u32,
    /// Raw GPIO levels from GP0 (true = HIGH).
    gpio: Vec<bool>,
    /// Raw PCA9555 levels, 16 per chip.
    pca9555: Vec<bool>,
    /// Raw 74HC165 levels, 8 per chip, chains in config order.
    hc165: Vec<bool>,
}

/// Everything the Buttons tab polls, in one round trip to the worker.
/// The expander queries are skipped when the config has none of that device.
#[tauri::command]
pub async fn buttons_live(mgr: State<'_, Manager>, key: String, pca9555: bool, hc165: bool) -> Res<ButtonsLive> {
    mgr.run(&key, move |c| {
        let s = c.query_button_states()?;
        Ok(ButtonsLive {
            states: s.states,
            shift_state: s.shift_state,
            gpio: c.query_button_gpio_states()?,
            pca9555: if pca9555 { c.query_button_pca9555_states()? } else { Vec::new() },
            hc165: if hc165 { c.query_button_74hc165_states()? } else { Vec::new() },
        })
    })
    .await
}
