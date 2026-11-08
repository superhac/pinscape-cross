//! Plunger: VendorClient methods for CMD_PLUNGER (0x0F): plunger
//! configuration, live readings (incl. image-sensor pixels), calibration,
//! settings, commit/revert.
//!
//! All setters change the in-memory settings on the Pico only; they take
//! effect immediately but are lost on reset unless committed with
//! [`VendorClient::commit_plunger_settings`].

use pinscape_protocol::cmd;
use pinscape_protocol::plunger::{self as pl, sub, PlungerCal, PlungerConfig, PlungerReading};

use crate::{Result, VendorClient};

impl VendorClient {
    fn plunger_cmd(&mut self, args: &[u8]) -> Result<()> {
        self.transact(cmd::PLUNGER, args, &[]).map(|_| ())
    }

    /// Sensor type and native scale. `configured` is false when no plunger
    /// sensor is set up (the firmware then reports its null sensor).
    pub fn query_plunger_config(&mut self) -> Result<PlungerConfig> {
        let (_, data) = self.transact(cmd::PLUNGER, &[sub::QUERY_CONFIG], &[])?;
        Ok(PlungerConfig::decode(&data)?)
    }

    /// Live reading, including calibration data, settings, and any
    /// sensor-specific data (e.g. an image-sensor snapshot).
    pub fn query_plunger_reading(&mut self) -> Result<PlungerReading> {
        let (_, data) = self.transact(cmd::PLUNGER, &[sub::QUERY_READING], &[])?;
        Ok(PlungerReading::decode(&data)?)
    }

    /// Start the timed (about 15 s) calibration. With `auto_save`, the result
    /// is committed to flash when it ends; otherwise it's in memory only.
    pub fn start_plunger_calibration(&mut self, auto_save: bool) -> Result<()> {
        self.plunger_cmd(&pl::byte_args(sub::CALIBRATE, auto_save as u8))
    }

    /// Jitter filter window in native sensor units (0 disables the filter).
    pub fn set_plunger_jitter_filter(&mut self, window: u16) -> Result<()> {
        self.plunger_cmd(&pl::jitter_args(window))
    }

    /// Firing state time limit in microseconds (0 = firmware default).
    pub fn set_plunger_firing_time_limit(&mut self, us: u32) -> Result<()> {
        self.plunger_cmd(&pl::int_args(sub::SET_FIRING_TIME_LIMIT, us))
    }

    /// Imaging-sensor integration (exposure) time in microseconds (0 = sensor default).
    pub fn set_plunger_integration_time(&mut self, us: u32) -> Result<()> {
        self.plunger_cmd(&pl::int_args(sub::SET_INTEGRATION_TIME, us))
    }

    pub fn set_plunger_orientation(&mut self, reverse: bool) -> Result<()> {
        self.plunger_cmd(&pl::byte_args(sub::SET_ORIENTATION, reverse as u8))
    }

    /// Manual scaling factor, percent (100 = calibrated range unchanged).
    pub fn set_plunger_scaling_factor(&mut self, percent: u32) -> Result<()> {
        self.plunger_cmd(&pl::int_args(sub::SET_SCALING_FACTOR, percent))
    }

    /// Sensor-specific scan mode (edge detection algorithm; 0 = default).
    pub fn set_plunger_scan_mode(&mut self, mode: u8) -> Result<()> {
        self.plunger_cmd(&pl::byte_args(sub::SET_SCAN_MODE, mode))
    }

    /// Put previously saved calibration data into effect (in memory only).
    /// Only meaningful for the same sensor type it was taken from.
    pub fn set_plunger_calibration(&mut self, cal: &PlungerCal) -> Result<()> {
        self.transact(cmd::PLUNGER, &[sub::SET_CAL_DATA], &cal.encode()).map(|_| ())
    }

    /// Save the live settings and calibration to flash.
    pub fn commit_plunger_settings(&mut self) -> Result<()> {
        self.plunger_cmd(&[sub::COMMIT_SETTINGS])
    }

    /// Reload the settings and calibration last saved in flash (or the defaults).
    pub fn revert_plunger_settings(&mut self) -> Result<()> {
        self.plunger_cmd(&[sub::REVERT_SETTINGS])
    }
}
