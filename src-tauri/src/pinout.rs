//! Pinout: Tauri command for the visual Pico pinout / GPIO-assignment view.
//! The board model and usage categories are in `pinscape_protocol::pinout`.

use pinscape_protocol::pinout::Pinout;
use pinscape_usb::{self as usb, VendorClient};
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Device IDs (CPU, build target board) plus QUERY_GPIO_CONFIG, as one model.
fn pinout(c: &mut VendorClient) -> usb::Result<Pinout> {
    let ids = c.query_ids()?;
    let ports = c.query_gpio_config()?;
    Ok(Pinout::build(&ids, &ports))
}

#[tauri::command]
pub async fn gpio_pinout(mgr: State<'_, Manager>, key: String) -> Res<Pinout> {
    mgr.run(&key, pinout).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinscape_usb::mock::MockDevice;

    #[test]
    fn pinout_against_mock_serializes_for_the_ui() {
        let mut c = VendorClient::with_transport(Box::new(MockDevice::new()));
        let p = pinout(&mut c).unwrap();
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["board"], "Raspberry Pi Pico");
        assert_eq!(j["chip"], "RP2040");
        assert_eq!(j["targetBoard"], "pico");
        assert!(j["fallbackReason"].is_null());
        assert_eq!(j["header"].as_array().unwrap().len(), 40);
        assert_eq!(j["header"][2]["kind"], "ground");
        assert_eq!(j["header"][30]["adcChannel"], 0);
        let gp4 = &j["pins"][4];
        assert_eq!(gp4["category"], "button");
        assert_eq!(gp4["headerPin"], 6);
        assert_eq!(gp4["funcName"], "SIO");
        assert_eq!(gp4["summary"], "GP4 (pin 6): Button #0 · GPIO input");
        assert_eq!(j["pins"][2]["category"], "i2c");
        assert_eq!(j["pins"][26]["category"], "plunger");
        assert_eq!(j["pins"][25]["boardUse"], "on-board LED");
        assert_eq!(j["free"], serde_json::json!([14, 27, 28]));
    }
}
