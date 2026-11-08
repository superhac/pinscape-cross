//! Buttons: VendorClient methods for CMD_BUTTONS (0x10): button descriptors,
//! logical button states, GPIO / PCA9555 / 74HC165 input states, shift state.

use pinscape_protocol::buttons::{self as b, subcmd, ButtonConfig, ButtonStates};
use pinscape_protocol::cmd;

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

    fn button_levels(&mut self, sub: u8) -> Result<Vec<bool>> {
        let (_, data) = self.transact(cmd::BUTTONS, &[sub], &[])?;
        Ok(b::decode_levels(&data))
    }
}
