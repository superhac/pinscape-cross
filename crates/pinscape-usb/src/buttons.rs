//! Buttons: VendorClient methods for CMD_BUTTONS (0x10): button descriptors,
//! logical button states, GPIO / PCA9555 / 74HC165 input states, shift state.

use pinscape_protocol::buttons::{self as b, subcmd, ButtonConfig, ButtonEvent, ButtonStates};
use pinscape_protocol::{cmd, Status};

use crate::{Result, VendorClient};

impl VendorClient {
    /// Logical button descriptors plus the PCA9555 / 74HC165 input devices.
    pub fn query_button_config(&mut self) -> Result<ButtonConfig> {
        let (_, data) = self.transact(cmd::BUTTONS, &[subcmd::QUERY_DESCS], &[])?;
        Ok(ButtonConfig::decode(&data)?)
    }

    /// Logical button states (after shift, pulse, toggle processing) and the global shift state.
    pub fn query_button_states(&mut self) -> Result<ButtonStates> {
        let (r, data) = self.transact(cmd::BUTTONS, &[subcmd::QUERY_STATES], &[])?;
        Ok(ButtonStates::decode(&r, &data))
    }

    /// Raw Pico GPIO levels, starting at GP0 (true = HIGH).
    pub fn query_button_gpio_states(&mut self) -> Result<Vec<bool>> {
        self.button_levels(subcmd::QUERY_GPIO_STATES)
    }

    /// Raw PCA9555 port levels, 16 per chip in config order.
    pub fn query_button_pca9555_states(&mut self) -> Result<Vec<bool>> {
        self.button_levels(subcmd::QUERY_PCA9555_STATES)
    }

    /// Raw 74HC165 port levels, 8 per chip, numbered across each chain, chains in config order.
    pub fn query_button_74hc165_states(&mut self) -> Result<Vec<bool>> {
        self.button_levels(subcmd::QUERY_74HC165_STATES)
    }

    /// New GPIO edge events for an event-logged GPIO button (oldest first);
    /// reading removes them from the Pico's buffer (64 entries, oldest
    /// dropped when full). `None` if no button on that GPIO has event
    /// logging enabled (`enableLogging: true` in its source config).
    pub fn query_button_event_log(&mut self, gpio: u8) -> Result<Option<Vec<ButtonEvent>>> {
        match self.transact(cmd::BUTTONS, &[subcmd::QUERY_EVENT_LOG, gpio], &[]) {
            Ok((_, data)) => Ok(Some(b::decode_event_log(&data)?)),
            Err(e) if e.status() == Some(Status::Failed) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Discard buffered events for one GPIO, or for all (`None`).
    pub fn clear_button_event_log(&mut self, gpio: Option<u8>) -> Result<()> {
        let g = gpio.unwrap_or(b::ALL_GPIOS);
        match self.transact(cmd::BUTTONS, &[subcmd::CLEAR_EVENT_LOG, g], &[]) {
            Ok(_) => Ok(()),
            // Firmware quirk (Buttons.cpp ClearEventLog): it clears the log but only
            // reports success for the wildcard GPIO 255.
            Err(e) if gpio.is_some() && e.status() == Some(Status::Failed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn button_levels(&mut self, sub: u8) -> Result<Vec<bool>> {
        let (_, data) = self.transact(cmd::BUTTONS, &[sub], &[])?;
        Ok(b::decode_levels(&data))
    }
}
