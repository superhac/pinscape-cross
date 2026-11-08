//! Nudge: VendorClient methods for CMD_NUDGE (0x07): accelerometer nudge
//! status, parameters, noise calibration, centering, commit/revert.

use pinscape_protocol::cmd;
use pinscape_protocol::nudge::{calibrate_args, sub, NudgeParams, NudgeStatus};

use crate::{Result, VendorClient};

impl VendorClient {
    fn nudge_simple(&mut self, subcmd: u8) -> Result<Vec<u8>> {
        Ok(self.transact(cmd::NUDGE, &[subcmd], &[])?.1)
    }

    /// Live readings, centering/calibration state and velocity.
    pub fn query_nudge_status(&mut self) -> Result<NudgeStatus> {
        let data = self.nudge_simple(sub::QUERY_STATUS)?;
        Ok(NudgeStatus::decode(&data)?)
    }

    /// The in-memory (live) nudge parameters.
    pub fn query_nudge_params(&mut self) -> Result<NudgeParams> {
        let data = self.nudge_simple(sub::QUERY_PARAMS)?;
        Ok(NudgeParams::decode(&data)?)
    }

    /// Apply new parameters immediately, in RAM only (see [`Self::commit_nudge_settings`]).
    ///
    /// The current parameters are read first so the request matches the
    /// device's struct size and carries any fields newer firmware added
    /// unchanged (the firmware rejects a struct shorter than its own).
    pub fn put_nudge_params(&mut self, params: &NudgeParams) -> Result<()> {
        let base = self.nudge_simple(sub::QUERY_PARAMS)?;
        NudgeParams::decode(&base)?;
        let data = params.encode_onto(&base);
        self.transact(cmd::NUDGE, &[sub::PUT_PARAMS], &data).map(|_| ())
    }

    /// Save the in-memory calibration data and parameters to flash.
    pub fn commit_nudge_settings(&mut self) -> Result<()> {
        self.nudge_simple(sub::COMMIT).map(|_| ())
    }

    /// Reload the calibration data and parameters from flash, discarding
    /// unsaved changes.
    pub fn revert_nudge_settings(&mut self) -> Result<()> {
        self.nudge_simple(sub::REVERT).map(|_| ())
    }

    /// Set the X/Y/Z center point now from the average of recent readings.
    pub fn set_nudge_center_point(&mut self) -> Result<()> {
        self.nudge_simple(sub::CENTER).map(|_| ())
    }

    /// Start the ~15-second noise calibration (the device must be left
    /// undisturbed). Progress shows in [`NudgeStatus::calibrating`]. With
    /// `auto_save`, the result is committed to flash when it finishes.
    pub fn start_nudge_calibration(&mut self, auto_save: bool) -> Result<()> {
        self.transact(cmd::NUDGE, &calibrate_args(auto_save), &[]).map(|_| ())
    }
}
