//! Nudge: wire structs and constants for [`cmd::NUDGE`](crate::cmd::NUDGE)
//! (0x07): accelerometer nudge status, parameters, noise calibration,
//! manual centering, commit/revert.
//!
//! See `USBProtocol/VendorIfcProtocol.h` upstream (`NudgeStatus`,
//! `NudgeParams`) and `Firmware/Nudge.cpp` for the semantics.
//!
//! Both structs start with their own size (`cb`). The decoders use it the
//! way the upstream Windows code does: newer firmware that appends fields
//! still decodes, and fields missing from a shorter (older) struct read as
//! zero.
//!
//! "No accelerometer configured" isn't reported explicitly. In that case the
//! firmware's nudge device reads from its internal null accelerometer, which
//! never produces a sample, so every reading stays zero and the status
//! `timestamp` stays 0 (see [`NudgeStatus::has_accelerometer`]). The
//! parameter commands still work (they apply to a `null.acc` settings file).

use serde::{Deserialize, Serialize};

use crate::{DecodeError, Le};

/// Subcommands (first argument byte) for [`cmd::NUDGE`](crate::cmd::NUDGE).
pub mod sub {
    /// Start the timed noise calibration; second arg byte: 1 = save to flash at the end.
    pub const CALIBRATE: u8 = 0x01;
    /// Set the center point now from the average of the last half second.
    pub const CENTER: u8 = 0x02;
    pub const QUERY_STATUS: u8 = 0x03;
    pub const QUERY_PARAMS: u8 = 0x04;
    /// Takes effect immediately, in RAM only (lost on reboot/revert).
    pub const PUT_PARAMS: u8 = 0x05;
    /// Save the in-memory calibration and parameters to flash.
    pub const COMMIT: u8 = 0x06;
    /// Reload the calibration and parameters from flash.
    pub const REVERT: u8 = 0x07;
}

/// `NudgeStatus::F_*` flags.
pub mod status_flags {
    /// In-memory parameters differ from the last flash save/load.
    pub const MODIFIED: u8 = 0x01;
    /// Noise calibration in progress.
    pub const CALIBRATING: u8 = 0x02;
}

/// `NudgeParams::F_*` flags.
pub mod params_flags {
    pub const AUTOCENTER: u16 = 0x0001;
}

/// Size of the `NudgeStatus` struct this crate knows.
pub const STATUS_LEN: usize = 56;
/// Size of the `NudgeParams` struct this crate knows (through `lpFreq`).
pub const PARAMS_LEN: usize = 26;
/// Smallest `NudgeParams` reply accepted (through `velocityScalingFactor`),
/// matching the upstream Windows API's check.
pub const PARAMS_MIN_LEN: usize = 24;
/// Length of the firmware's noise calibration run (`CAL_MODE_INTERVAL`).
pub const CALIBRATION_MS: u32 = 15_000;

/// Decoded `NudgeStatus` (reply to QUERY_STATUS). All axis values are
/// normalized INT16 units, where 32768 = `g_range` g.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeStatus {
    /// Size of the firmware's struct.
    pub cb: u16,
    pub flags: u8,
    /// [`status_flags::MODIFIED`]: settings changed since the last save/load.
    pub modified: bool,
    /// [`status_flags::CALIBRATING`].
    pub calibrating: bool,
    /// Dynamic range of the sensor, in g.
    pub g_range: u8,
    /// Last raw reading, mapped to the nudge axes but not centered/filtered.
    pub x_raw: i16,
    pub y_raw: i16,
    pub z_raw: i16,
    /// Last reading after centering, DC blocker, low-pass and jitter filters.
    pub x_filtered: i16,
    pub y_filtered: i16,
    pub z_filtered: i16,
    /// Reading timestamp, microseconds since device startup (0 = no reading ever taken).
    pub timestamp: u64,
    /// Microseconds since the center point was last set (auto or manual).
    pub last_centering_age_us: u64,
    pub x_center: i16,
    pub y_center: i16,
    pub z_center: i16,
    /// Auto-centering quiet thresholds (set by calibration).
    pub x_threshold: i16,
    pub y_threshold: i16,
    pub z_threshold: i16,
    /// Trailing average of the raw readings over the last few seconds.
    pub x_avg: i16,
    pub y_avg: i16,
    pub z_avg: i16,
    /// Integrated velocity, scaled by the velocity scaling factor.
    pub vx: i16,
    pub vy: i16,
    pub vz: i16,
    /// Derived: the device has an accelerometer producing readings.
    pub has_accelerometer: bool,
}

impl NudgeStatus {
    /// Decode a QUERY_STATUS reply. Only the `cb` field is required.
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < 2 {
            return Err(DecodeError::Length { expected: 2, got: xfer.len() });
        }
        let cb = u16::from_le_bytes([xfer[0], xfer[1]]);
        let r = Le(&xfer[..(cb as usize).clamp(2, xfer.len())]);
        let i = |o: usize| r.u16(o) as i16;
        let flags = r.u8(2);
        let timestamp = r.u64(16);
        Ok(NudgeStatus {
            cb,
            flags,
            modified: flags & status_flags::MODIFIED != 0,
            calibrating: flags & status_flags::CALIBRATING != 0,
            g_range: r.u8(3),
            x_raw: i(4),
            y_raw: i(6),
            z_raw: i(8),
            x_filtered: i(10),
            y_filtered: i(12),
            z_filtered: i(14),
            timestamp,
            last_centering_age_us: r.u64(24),
            x_center: i(32),
            y_center: i(34),
            z_center: i(36),
            x_threshold: i(38),
            y_threshold: i(40),
            z_threshold: i(42),
            x_avg: i(44),
            y_avg: i(46),
            z_avg: i(48),
            vx: i(50),
            vy: i(52),
            vz: i(54),
            has_accelerometer: timestamp != 0,
        })
    }

    /// Encode as the firmware would (for the mock and tests). `flags` and
    /// the scalar fields are taken as-is; `cb` is always [`STATUS_LEN`].
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(STATUS_LEN);
        b.extend_from_slice(&(STATUS_LEN as u16).to_le_bytes());
        b.push(self.flags);
        b.push(self.g_range);
        for v in [self.x_raw, self.y_raw, self.z_raw, self.x_filtered, self.y_filtered, self.z_filtered] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&self.timestamp.to_le_bytes());
        b.extend_from_slice(&self.last_centering_age_us.to_le_bytes());
        for v in [
            self.x_center,
            self.y_center,
            self.z_center,
            self.x_threshold,
            self.y_threshold,
            self.z_threshold,
            self.x_avg,
            self.y_avg,
            self.z_avg,
            self.vx,
            self.vy,
            self.vz,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        debug_assert_eq!(b.len(), STATUS_LEN);
        b
    }
}

/// `NudgeParams` (QUERY_PARAMS reply / PUT_PARAMS data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeParams {
    /// [`params_flags::AUTOCENTER`].
    pub auto_center: bool,
    /// Flag bits other than AUTOCENTER, preserved on round trips.
    #[serde(default)]
    pub other_flags: u16,
    /// Required stillness before auto-centering, ms (firmware minimum 1).
    pub auto_center_interval_ms: u16,
    /// Auto-centering quiet thresholds, INT16 axis units.
    pub x_threshold: u16,
    pub y_threshold: u16,
    pub z_threshold: u16,
    /// DC blocker time constant, ms; 0 disables the DC blocker.
    pub dc_time_ms: u16,
    /// Jitter (hysteresis) filter windows, INT16 axis units; 0 disables.
    pub x_jitter_window: u16,
    pub y_jitter_window: u16,
    pub z_jitter_window: u16,
    /// Velocity half-life, ms.
    pub velocity_decay_ms: u16,
    /// INT16 report units per mm/s.
    pub velocity_scaling_factor: u16,
    /// Low-pass filter corner frequency, Hz; 0 disables.
    pub lp_freq_hz: u16,
    /// Derived: the firmware's struct includes `lpFreq` (older versions don't).
    #[serde(default = "yes")]
    pub has_lp_freq: bool,
}

fn yes() -> bool {
    true
}

impl Default for NudgeParams {
    /// The firmware's defaults (when no settings file exists).
    fn default() -> Self {
        NudgeParams {
            auto_center: true,
            other_flags: 0,
            auto_center_interval_ms: 4000,
            x_threshold: 328,
            y_threshold: 328,
            z_threshold: 984,
            dc_time_ms: 200,
            x_jitter_window: 0,
            y_jitter_window: 0,
            z_jitter_window: 0,
            velocity_decay_ms: 2000,
            velocity_scaling_factor: 100,
            lp_freq_hz: 0,
            has_lp_freq: true,
        }
    }
}

impl NudgeParams {
    /// Decode a QUERY_PARAMS reply (or PUT_PARAMS data, in the mock).
    /// Requires at least the original struct (through `velocityScalingFactor`)
    /// and a `cb` that fits in the data.
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < PARAMS_MIN_LEN {
            return Err(DecodeError::Length { expected: PARAMS_MIN_LEN, got: xfer.len() });
        }
        let cb = u16::from_le_bytes([xfer[0], xfer[1]]) as usize;
        if cb > xfer.len() || cb < PARAMS_MIN_LEN {
            return Err(DecodeError::Length { expected: cb.max(PARAMS_MIN_LEN), got: xfer.len() });
        }
        let r = Le(&xfer[..cb]);
        let flags = r.u16(2);
        Ok(NudgeParams {
            auto_center: flags & params_flags::AUTOCENTER != 0,
            other_flags: flags & !params_flags::AUTOCENTER,
            auto_center_interval_ms: r.u16(4),
            x_threshold: r.u16(6),
            y_threshold: r.u16(8),
            z_threshold: r.u16(10),
            dc_time_ms: r.u16(12),
            x_jitter_window: r.u16(14),
            y_jitter_window: r.u16(16),
            z_jitter_window: r.u16(18),
            velocity_decay_ms: r.u16(20),
            velocity_scaling_factor: r.u16(22),
            lp_freq_hz: r.u16(24),
            has_lp_freq: cb >= PARAMS_LEN,
        })
    }

    pub fn flags(&self) -> u16 {
        (self.other_flags & !params_flags::AUTOCENTER) | if self.auto_center { params_flags::AUTOCENTER } else { 0 }
    }

    /// Encode the known struct ([`PARAMS_LEN`] bytes).
    pub fn encode(&self) -> Vec<u8> {
        self.encode_onto(&[])
    }

    /// Encode over a previous QUERY_PARAMS reply from the device, so that
    /// fields this crate doesn't know (appended by newer firmware) are sent
    /// back unchanged and `cb` matches the device's struct size. The firmware
    /// rejects a PUT shorter than its own struct.
    pub fn encode_onto(&self, base: &[u8]) -> Vec<u8> {
        let mut b = if base.len() > PARAMS_LEN { base.to_vec() } else { vec![0u8; PARAMS_LEN] };
        let cb = b.len() as u16;
        let fields = [
            cb,
            self.flags(),
            self.auto_center_interval_ms,
            self.x_threshold,
            self.y_threshold,
            self.z_threshold,
            self.dc_time_ms,
            self.x_jitter_window,
            self.y_jitter_window,
            self.z_jitter_window,
            self.velocity_decay_ms,
            self.velocity_scaling_factor,
            self.lp_freq_hz,
        ];
        for (i, v) in fields.iter().enumerate() {
            b[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        b
    }
}

/// Arguments for SUBCMD_NUDGE_CALIBRATE.
pub fn calibrate_args(auto_save: bool) -> [u8; 2] {
    [sub::CALIBRATE, auto_save as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_status() -> NudgeStatus {
        let mut s = NudgeStatus {
            cb: STATUS_LEN as u16,
            flags: status_flags::MODIFIED | status_flags::CALIBRATING,
            g_range: 2,
            x_raw: -1234,
            y_raw: 567,
            z_raw: 16384,
            x_filtered: -5,
            y_filtered: 6,
            z_filtered: 16390,
            timestamp: 123_456_789_012,
            last_centering_age_us: 2_500_000,
            x_center: -1200,
            y_center: 560,
            z_center: 10,
            x_threshold: 328,
            y_threshold: 329,
            z_threshold: 984,
            x_avg: -1201,
            y_avg: 561,
            z_avg: 16390,
            vx: -32768,
            vy: 32767,
            vz: 1,
            ..Default::default()
        };
        s.modified = true;
        s.calibrating = true;
        s.has_accelerometer = true;
        s
    }

    #[test]
    fn status_layout_and_round_trip() {
        let s = sample_status();
        let b = s.encode();
        assert_eq!(b.len(), STATUS_LEN);
        // spot-check offsets against the C struct
        assert_eq!(&b[0..4], &[56, 0, 3, 2]);
        assert_eq!(i16::from_le_bytes([b[4], b[5]]), -1234);
        assert_eq!(u64::from_le_bytes(b[16..24].try_into().unwrap()), 123_456_789_012);
        assert_eq!(i16::from_le_bytes([b[54], b[55]]), 1);
        assert_eq!(NudgeStatus::decode(&b).unwrap(), s);
    }

    #[test]
    fn status_larger_and_shorter_structs() {
        let s = sample_status();
        // newer firmware: bigger struct with extra trailing fields
        let mut b = s.encode();
        b.extend_from_slice(&[0xAA; 10]);
        b[0..2].copy_from_slice(&66u16.to_le_bytes());
        let d = NudgeStatus::decode(&b).unwrap();
        assert_eq!(d.cb, 66);
        assert_eq!((d.vx, d.vy, d.vz), (s.vx, s.vy, s.vz));

        // older/shorter struct: missing fields read as zero
        let mut b = s.encode();
        b.truncate(32);
        b[0..2].copy_from_slice(&32u16.to_le_bytes());
        let d = NudgeStatus::decode(&b).unwrap();
        assert_eq!(d.x_raw, -1234);
        assert_eq!(d.last_centering_age_us, 2_500_000);
        assert_eq!((d.x_center, d.vx, d.z_avg), (0, 0, 0));

        // cb smaller than the data: bytes past cb are ignored
        let mut b = s.encode();
        b[0..2].copy_from_slice(&10u16.to_le_bytes());
        let d = NudgeStatus::decode(&b).unwrap();
        assert_eq!((d.x_filtered, d.timestamp), (0, 0));
        assert!(!d.has_accelerometer);

        assert!(NudgeStatus::decode(&[1]).is_err());
    }

    #[test]
    fn status_no_accelerometer() {
        // null device: all zero, timestamp 0, gRange 2
        let s = NudgeStatus { g_range: 2, last_centering_age_us: 99, ..Default::default() };
        let d = NudgeStatus::decode(&s.encode()).unwrap();
        assert!(!d.has_accelerometer);
        assert_eq!(d.g_range, 2);
    }

    fn sample_params() -> NudgeParams {
        NudgeParams {
            auto_center: false,
            other_flags: 0x0100,
            auto_center_interval_ms: 5000,
            x_threshold: 11,
            y_threshold: 12,
            z_threshold: 13,
            dc_time_ms: 250,
            x_jitter_window: 21,
            y_jitter_window: 22,
            z_jitter_window: 23,
            velocity_decay_ms: 1500,
            velocity_scaling_factor: 109,
            lp_freq_hz: 30,
            has_lp_freq: true,
        }
    }

    #[test]
    fn params_layout_and_round_trip() {
        let p = sample_params();
        let b = p.encode();
        assert_eq!(b.len(), PARAMS_LEN);
        assert_eq!(&b[0..6], &[26, 0, 0x00, 0x01, 0x88, 0x13]);
        assert_eq!(u16::from_le_bytes([b[24], b[25]]), 30);
        assert_eq!(NudgeParams::decode(&b).unwrap(), p);

        let p2 = NudgeParams { auto_center: true, ..p };
        let b = p2.encode();
        assert_eq!(u16::from_le_bytes([b[2], b[3]]), 0x0101);
        assert_eq!(NudgeParams::decode(&b).unwrap(), p2);
    }

    #[test]
    fn params_older_struct_without_lp_freq() {
        let p = sample_params();
        let mut b = p.encode();
        b.truncate(PARAMS_MIN_LEN);
        b[0..2].copy_from_slice(&(PARAMS_MIN_LEN as u16).to_le_bytes());
        let d = NudgeParams::decode(&b).unwrap();
        assert!(!d.has_lp_freq);
        assert_eq!(d.lp_freq_hz, 0);
        assert_eq!(d.velocity_scaling_factor, 109);
        // too short / bad cb
        assert!(NudgeParams::decode(&b[..20]).is_err());
        let mut bad = p.encode();
        bad[0..2].copy_from_slice(&40u16.to_le_bytes());
        assert!(NudgeParams::decode(&bad).is_err());
    }

    #[test]
    fn params_newer_struct_preserves_unknown_fields() {
        let p = sample_params();
        let mut dev = p.encode();
        dev.extend_from_slice(&[0x5A, 0xA5, 0x01, 0x02]);
        dev[0..2].copy_from_slice(&30u16.to_le_bytes());
        let d = NudgeParams::decode(&dev).unwrap();
        assert_eq!(d, p);

        let edited = NudgeParams { dc_time_ms: 400, ..d };
        let out = edited.encode_onto(&dev);
        assert_eq!(out.len(), 30);
        assert_eq!(u16::from_le_bytes([out[0], out[1]]), 30);
        assert_eq!(u16::from_le_bytes([out[12], out[13]]), 400);
        assert_eq!(&out[26..], &[0x5A, 0xA5, 0x01, 0x02]);
        // a shorter base still produces the full known struct
        assert_eq!(edited.encode_onto(&dev[..24]).len(), PARAMS_LEN);
    }

    #[test]
    fn calibrate_arg_bytes() {
        assert_eq!(calibrate_args(true), [0x01, 1]);
        assert_eq!(calibrate_args(false), [0x01, 0]);
    }
}
