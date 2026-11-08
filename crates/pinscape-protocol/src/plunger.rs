//! Plunger: wire structs and constants for [`cmd::PLUNGER`](crate::cmd::PLUNGER)
//! (0x0F): plunger configuration, live readings (incl. image-sensor pixels),
//! calibration, settings, commit/revert.
//!
//! See `USBProtocol/VendorIfcProtocol.h` upstream (`PlungerConfig`,
//! `PlungerReading`, `PlungerReadingQuadrature`, `PlungerReadingImageSensor`,
//! `PlungerReadingVCNL4010`, `PlungerCal`, `Args::JitterFilter`,
//! `Args::PlungerInt`, `Args::PlungerByte`).
//!
//! The reply structs start with their own size (`cb`). As in the upstream
//! Windows API, the decoders copy only the fields the device actually sent
//! (missing fields read as zero), and a larger struct from newer firmware is
//! skipped using its `cb`. A `PlungerReading` may be followed by a
//! sensor-specific struct that starts with its own `cb` and sensor type code;
//! known ones are decoded, unknown ones are kept as opaque bytes.

use serde::Serialize;

use crate::{DecodeError, Le};

/// Subcommands (first argument byte) for [`cmd::PLUNGER`](crate::cmd::PLUNGER).
pub mod sub {
    pub const CALIBRATE: u8 = 0x01;
    pub const SET_JITTER_FILTER: u8 = 0x02;
    pub const SET_FIRING_TIME_LIMIT: u8 = 0x03;
    pub const SET_INTEGRATION_TIME: u8 = 0x04;
    pub const SET_ORIENTATION: u8 = 0x05;
    pub const SET_SCALING_FACTOR: u8 = 0x06;
    pub const SET_CAL_DATA: u8 = 0x07;
    pub const SET_SCAN_MODE: u8 = 0x08;
    pub const COMMIT_SETTINGS: u8 = 0x40;
    pub const REVERT_SETTINGS: u8 = 0x41;
    pub const QUERY_READING: u8 = 0x81;
    pub const QUERY_CONFIG: u8 = 0x82;
}

/// Plunger sensor type codes (`FeedbackControllerReport::PLUNGER_*`).
pub mod sensor_type {
    pub const NONE: u16 = 0;
    pub const POT: u16 = 1;
    pub const AEDR8300: u16 = 2;
    pub const VCNL4010: u16 = 3;
    pub const VL6180X: u16 = 4;
    pub const TCD1103: u16 = 5;
    pub const TSL1410R: u16 = 6;
    pub const TSL1412S: u16 = 7;
}

/// `PlungerReading::F_*` flags.
pub mod reading_flags {
    /// Reverse orientation in effect.
    pub const REVERSE: u16 = 0x0001;
    /// Calibration data is valid.
    pub const CALIBRATED: u16 = 0x0002;
    /// Calibration mode is running.
    pub const CALIBRATING: u16 = 0x0004;
    /// Live settings/calibration differ from the ones saved in flash.
    pub const MODIFIED: u16 = 0x0008;
    /// A ZB Launch event is active.
    pub const ZBLAUNCH: u16 = 0x0010;
}

/// `PlungerCal::F_CALIBRATED`.
pub const CAL_F_CALIBRATED: u16 = 0x0001;

/// `PlungerFiringState` values.
pub mod firing_state {
    pub const NONE: u16 = 0;
    pub const MOVING: u16 = 1;
    pub const FIRED: u16 = 2;
    pub const SETTLING: u16 = 3;
}

/// Display name for a `PLUNGER_*` sensor type code.
pub fn sensor_type_name(t: u16) -> &'static str {
    match t {
        sensor_type::NONE => "None",
        sensor_type::POT => "Potentiometer",
        sensor_type::AEDR8300 => "AEDR-8300",
        sensor_type::VCNL4010 => "VCNL4010",
        sensor_type::VL6180X => "VL6180X",
        sensor_type::TCD1103 => "TCD1103",
        sensor_type::TSL1410R => "TSL1410R",
        sensor_type::TSL1412S => "TSL1412S",
        _ => "Unknown",
    }
}

/// True for the linear imaging sensors (TCD1103, TSL1410R, TSL1412S).
pub fn is_image_sensor(t: u16) -> bool {
    matches!(t, sensor_type::TCD1103 | sensor_type::TSL1410R | sensor_type::TSL1412S)
}

/// True for sensors that use the integration-time (exposure) setting.
pub fn has_integration_time(t: u16) -> bool {
    is_image_sensor(t)
}

/// Scan-mode (edge detection algorithm) names for sensors that have them;
/// empty if the sensor has no selectable modes. Index = mode byte.
pub fn scan_mode_names(t: u16) -> &'static [&'static str] {
    match t {
        sensor_type::TSL1410R | sensor_type::TSL1412S => &["Steady Slope", "Steepest Slope", "Speed Gap"],
        _ => &[],
    }
}

/// Display name for a `PlungerFiringState` value.
pub fn firing_state_name(s: u16) -> &'static str {
    match s {
        firing_state::NONE => "None",
        firing_state::MOVING => "Moving",
        firing_state::FIRED => "Fired",
        firing_state::SETTLING => "Settling",
        _ => "Unknown",
    }
}

/// Size of the `PlungerConfig` struct this crate knows.
pub const CONFIG_LEN: usize = 12;
/// Size of the `PlungerReading` struct this crate knows.
pub const READING_LEN: usize = 117;
/// Size of the `PlungerCal` struct this crate knows (and the minimum the firmware accepts).
pub const CAL_LEN: usize = 52;
/// Offset of the pixel array in `PlungerReadingImageSensor`.
const IMAGE_HDR_LEN: usize = 16;

/// Copy of a reply struct limited to its own `cb` (and to the data present),
/// so fields beyond either read as zero. Returns the struct bytes and `cb`.
fn sized(xfer: &[u8]) -> Result<(&[u8], usize), DecodeError> {
    if xfer.len() < 2 {
        return Err(DecodeError::Length { expected: 2, got: xfer.len() });
    }
    let cb = Le(xfer).u16(0) as usize;
    let cb = cb.clamp(2, xfer.len());
    Ok((&xfer[..cb], cb))
}

/// `PlungerConfig` (reply to QUERY_CONFIG).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlungerConfig {
    pub sensor_type: u16,
    pub sensor_type_name: &'static str,
    /// Maximum raw reading in the sensor's native units (minimum is always 0).
    pub native_scale: u32,
    pub flags: u32,
    /// A plunger sensor is configured (`sensor_type != NONE`).
    pub configured: bool,
    pub image_sensor: bool,
    pub has_integration_time: bool,
    /// Scan-mode names for sensors with selectable modes (index = mode byte).
    pub scan_modes: Vec<&'static str>,
}

impl PlungerConfig {
    pub fn new(sensor_type: u16, native_scale: u32, flags: u32) -> Self {
        PlungerConfig {
            sensor_type,
            sensor_type_name: sensor_type_name(sensor_type),
            native_scale,
            flags,
            configured: sensor_type != sensor_type::NONE,
            image_sensor: is_image_sensor(sensor_type),
            has_integration_time: has_integration_time(sensor_type),
            scan_modes: scan_mode_names(sensor_type).to_vec(),
        }
    }

    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        let (b, _) = sized(xfer)?;
        let r = Le(b);
        Ok(Self::new(r.u16(2), r.u32(4), r.u32(8)))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(CONFIG_LEN);
        b.extend_from_slice(&(CONFIG_LEN as u16).to_le_bytes());
        b.extend_from_slice(&self.sensor_type.to_le_bytes());
        b.extend_from_slice(&self.native_scale.to_le_bytes());
        b.extend_from_slice(&self.flags.to_le_bytes());
        b
    }
}

/// Sensor-specific data appended to a `PlungerReading`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SensorData {
    /// `PlungerReadingQuadrature`: current A/B channel states.
    Quadrature { sensor_type: u16, state: u8, a: bool, b: bool },
    /// `PlungerReadingImageSensor`: snapshot of the latest image frame.
    /// Most sensors are linear grayscale; TCD1103 is a negative image
    /// (lower number = brighter).
    ImageSensor { sensor_type: u16, timestamp: u64, n_pix: u16, pixels: Vec<u8> },
    /// `PlungerReadingVCNL4010`: last raw proximity count.
    Vcnl4010 { sensor_type: u16, prox_count: u16 },
    /// A struct this crate doesn't know (or a malformed one), as raw bytes.
    Opaque { sensor_type: u16, data: Vec<u8> },
}

impl SensorData {
    pub fn sensor_type(&self) -> u16 {
        match self {
            SensorData::Quadrature { sensor_type, .. }
            | SensorData::ImageSensor { sensor_type, .. }
            | SensorData::Vcnl4010 { sensor_type, .. }
            | SensorData::Opaque { sensor_type, .. } => *sensor_type,
        }
    }

    /// Decode one trailer struct (`cb`, `sensorType`, ...). `b` is exactly the struct.
    pub fn decode(b: &[u8]) -> Self {
        let r = Le(b);
        let st = r.u16(2);
        match st {
            sensor_type::AEDR8300 if b.len() >= 5 => {
                let state = r.u8(4) & 0x03;
                SensorData::Quadrature { sensor_type: st, state, a: state & 0x01 != 0, b: state & 0x02 != 0 }
            }
            sensor_type::VCNL4010 if b.len() >= 6 => SensorData::Vcnl4010 { sensor_type: st, prox_count: r.u16(4) },
            t if is_image_sensor(t) && b.len() >= IMAGE_HDR_LEN => {
                let n = (r.u16(14) as usize).min(b.len() - IMAGE_HDR_LEN);
                SensorData::ImageSensor {
                    sensor_type: st,
                    timestamp: r.u64(6),
                    n_pix: n as u16,
                    pixels: b[IMAGE_HDR_LEN..IMAGE_HDR_LEN + n].to_vec(),
                }
            }
            _ => SensorData::Opaque { sensor_type: st, data: b.to_vec() },
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        let hdr = |b: &mut Vec<u8>, cb: usize, st: u16| {
            b.extend_from_slice(&(cb as u16).to_le_bytes());
            b.extend_from_slice(&st.to_le_bytes());
        };
        match self {
            SensorData::Quadrature { sensor_type, state, .. } => {
                hdr(&mut b, 5, *sensor_type);
                b.push(*state);
            }
            SensorData::ImageSensor { sensor_type, timestamp, pixels, .. } => {
                hdr(&mut b, IMAGE_HDR_LEN + pixels.len(), *sensor_type);
                b.extend_from_slice(&0u16.to_le_bytes());
                b.extend_from_slice(&timestamp.to_le_bytes());
                b.extend_from_slice(&(pixels.len() as u16).to_le_bytes());
                b.extend_from_slice(pixels);
            }
            SensorData::Vcnl4010 { sensor_type, prox_count } => {
                hdr(&mut b, 6, *sensor_type);
                b.extend_from_slice(&prox_count.to_le_bytes());
            }
            SensorData::Opaque { data, .. } => b.extend_from_slice(data),
        }
        b
    }
}

/// `PlungerReading` (reply to QUERY_READING), plus any sensor-specific trailer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlungerReading {
    pub flags: u16,
    pub reverse: bool,
    pub calibrated: bool,
    pub calibrating: bool,
    pub modified: bool,
    pub zb_launch: bool,
    /// Raw sensor reading, native units (after orientation and jitter filter).
    pub raw_pos: u32,
    /// Reading time, microseconds since an arbitrary epoch (usually Pico boot).
    pub timestamp: u64,
    /// Joystick Z axis with firing-event processing (0 = rest, 32767 = full retraction).
    pub z: i16,
    /// Instantaneous (unprocessed) Z axis.
    pub z0: i16,
    pub z0_prv: i16,
    pub z0_nxt: i16,
    /// Time between `z0_prv` and `z0_nxt`, microseconds.
    pub dt: i64,
    /// Speed in Z units per centisecond; negative when moving forward.
    pub speed: i16,
    pub firing_state: u16,
    pub firing_state_name: &'static str,
    /// Calibration points, native sensor units.
    pub cal_min: u32,
    pub cal_zero: u32,
    pub cal_max: u32,
    /// Opaque sensor-specific calibration data (save/restore only).
    pub cal_sensor_data: [u32; 8],
    /// Average firing time measured in the last calibration, microseconds.
    pub firing_time_measured: u32,
    /// Firing state time limit, microseconds.
    pub firing_time_limit: u32,
    /// Imaging-sensor integration time, microseconds (0 = sensor default).
    pub integration_time: u32,
    /// Manual scaling factor, percent (100 = 1.0).
    pub manual_scaling_factor: u32,
    /// Jitter filter window, native units (0 = off).
    pub jf_window: u32,
    pub jf_lo: u32,
    pub jf_hi: u32,
    /// Last reading before / after the jitter filter.
    pub jf_last_pre: u32,
    pub jf_last_post: u32,
    pub scan_mode: u8,
    pub sensor: Option<SensorData>,
}

impl PlungerReading {
    /// Fill in the derived fields (flag booleans, state name) from the raw ones.
    pub fn with_derived(mut self) -> Self {
        let f = |b: u16| self.flags & b != 0;
        self.reverse = f(reading_flags::REVERSE);
        self.calibrated = f(reading_flags::CALIBRATED);
        self.calibrating = f(reading_flags::CALIBRATING);
        self.modified = f(reading_flags::MODIFIED);
        self.zb_launch = f(reading_flags::ZBLAUNCH);
        self.firing_state_name = firing_state_name(self.firing_state);
        self
    }

    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        let (b, cb) = sized(xfer)?;
        let r = Le(b);
        let i16_at = |o: usize| r.u16(o) as i16;
        let mut cal_sensor_data = [0u32; 8];
        for (i, v) in cal_sensor_data.iter_mut().enumerate() {
            *v = r.u32(48 + i * 4);
        }
        let mut rd = PlungerReading {
            flags: r.u16(2),
            raw_pos: r.u32(4),
            timestamp: r.u64(8),
            z: i16_at(16),
            z0: i16_at(18),
            z0_prv: i16_at(20),
            z0_nxt: i16_at(22),
            dt: r.u64(24) as i64,
            speed: i16_at(32),
            firing_state: r.u16(34),
            cal_min: r.u32(36),
            cal_zero: r.u32(40),
            cal_max: r.u32(44),
            cal_sensor_data,
            firing_time_measured: r.u32(80),
            firing_time_limit: r.u32(84),
            integration_time: r.u32(88),
            manual_scaling_factor: r.u32(92),
            jf_window: r.u32(96),
            jf_lo: r.u32(100),
            jf_hi: r.u32(104),
            jf_last_pre: r.u32(108),
            jf_last_post: r.u32(112),
            scan_mode: r.u8(116),
            ..Default::default()
        };
        // sensor-specific trailer: { u16 cb; u16 sensorType; ... }
        let rest = &xfer[cb..];
        if rest.len() >= 4 {
            let tcb = (Le(rest).u16(0) as usize).clamp(4, rest.len());
            rd.sensor = Some(SensorData::decode(&rest[..tcb]));
        }
        Ok(rd.with_derived())
    }

    /// Encode as the firmware would (used by the simulated device and tests).
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(READING_LEN);
        b.extend_from_slice(&(READING_LEN as u16).to_le_bytes());
        b.extend_from_slice(&self.flags.to_le_bytes());
        b.extend_from_slice(&self.raw_pos.to_le_bytes());
        b.extend_from_slice(&self.timestamp.to_le_bytes());
        for v in [self.z, self.z0, self.z0_prv, self.z0_nxt] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&self.dt.to_le_bytes());
        b.extend_from_slice(&self.speed.to_le_bytes());
        b.extend_from_slice(&self.firing_state.to_le_bytes());
        for v in [self.cal_min, self.cal_zero, self.cal_max] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.cal_sensor_data {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [
            self.firing_time_measured,
            self.firing_time_limit,
            self.integration_time,
            self.manual_scaling_factor,
            self.jf_window,
            self.jf_lo,
            self.jf_hi,
            self.jf_last_pre,
            self.jf_last_post,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(self.scan_mode);
        debug_assert_eq!(b.len(), READING_LEN);
        if let Some(s) = &self.sensor {
            b.extend_from_slice(&s.encode());
        }
        b
    }

    /// The calibration data in this reading, in `PlungerCal` form (for saving
    /// on the host and restoring later with SET_CAL_DATA).
    pub fn calibration(&self) -> PlungerCal {
        PlungerCal {
            calibrated: self.calibrated,
            cal_min: self.cal_min,
            cal_zero: self.cal_zero,
            cal_max: self.cal_max,
            firing_time_measured: self.firing_time_measured,
            sensor_data: self.cal_sensor_data,
        }
    }
}

/// `PlungerCal`: calibration data for SET_CAL_DATA (sent as extra OUT data).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlungerCal {
    pub calibrated: bool,
    pub cal_min: u32,
    pub cal_zero: u32,
    pub cal_max: u32,
    pub firing_time_measured: u32,
    /// Opaque sensor-specific data, as reported in `PlungerReading::cal_sensor_data`.
    pub sensor_data: [u32; 8],
}

impl PlungerCal {
    /// The firmware's defaults when no calibration is saved: the full native
    /// range, with the rest position at 1/6 of it, not calibrated.
    pub fn defaults(native_scale: u32) -> Self {
        PlungerCal { cal_min: 0, cal_max: native_scale, cal_zero: native_scale / 6, ..Default::default() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(CAL_LEN);
        b.extend_from_slice(&(CAL_LEN as u16).to_le_bytes());
        let flags = if self.calibrated { CAL_F_CALIBRATED } else { 0 };
        b.extend_from_slice(&flags.to_le_bytes());
        for v in [self.cal_min, self.cal_zero, self.cal_max, self.firing_time_measured] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.sensor_data {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    /// Decode as the firmware does: requires at least the v1 struct size.
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < CAL_LEN {
            return Err(DecodeError::Length { expected: CAL_LEN, got: xfer.len() });
        }
        let r = Le(xfer);
        let mut sensor_data = [0u32; 8];
        for (i, v) in sensor_data.iter_mut().enumerate() {
            *v = r.u32(20 + i * 4);
        }
        Ok(PlungerCal {
            calibrated: r.u16(2) & CAL_F_CALIBRATED != 0,
            cal_min: r.u32(4),
            cal_zero: r.u32(8),
            cal_max: r.u32(12),
            firing_time_measured: r.u32(16),
            sensor_data,
        })
    }
}

/// `Args::JitterFilter` (4 bytes) for SET_JITTER_FILTER.
pub fn jitter_args(window: u16) -> [u8; 4] {
    let w = window.to_le_bytes();
    [sub::SET_JITTER_FILTER, 0, w[0], w[1]]
}

/// `Args::PlungerInt` (8 bytes) for the u32-valued setters.
pub fn int_args(subcmd: u8, value: u32) -> [u8; 8] {
    let v = value.to_le_bytes();
    [subcmd, 0, 0, 0, v[0], v[1], v[2], v[3]]
}

/// `Args::PlungerByte` (2 bytes) for CALIBRATE, SET_ORIENTATION, SET_SCAN_MODE.
pub fn byte_args(subcmd: u8, value: u8) -> [u8; 2] {
    [subcmd, value]
}

/// Decoded request arguments (used by the simulated device).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Calibrate { auto_save: bool },
    SetJitterFilter(u16),
    SetFiringTimeLimit(u32),
    SetIntegrationTime(u32),
    SetOrientation { reverse: bool },
    SetScalingFactor(u32),
    SetCalData,
    SetScanMode(u8),
    Commit,
    Revert,
    QueryReading,
    QueryConfig,
    Unknown(u8),
}

impl Request {
    pub fn decode(args: &[u8]) -> Self {
        let r = Le(args);
        let sub = r.u8(0);
        match sub {
            sub::CALIBRATE => Request::Calibrate { auto_save: r.u8(1) & 0x01 != 0 },
            sub::SET_JITTER_FILTER => Request::SetJitterFilter(r.u16(2)),
            sub::SET_FIRING_TIME_LIMIT => Request::SetFiringTimeLimit(r.u32(4)),
            sub::SET_INTEGRATION_TIME => Request::SetIntegrationTime(r.u32(4)),
            sub::SET_ORIENTATION => Request::SetOrientation { reverse: r.u8(1) != 0 },
            sub::SET_SCALING_FACTOR => Request::SetScalingFactor(r.u32(4)),
            sub::SET_CAL_DATA => Request::SetCalData,
            sub::SET_SCAN_MODE => Request::SetScanMode(r.u8(1)),
            sub::COMMIT_SETTINGS => Request::Commit,
            sub::REVERT_SETTINGS => Request::Revert,
            sub::QUERY_READING => Request::QueryReading,
            sub::QUERY_CONFIG => Request::QueryConfig,
            other => Request::Unknown(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PlungerReading {
        PlungerReading {
            flags: reading_flags::CALIBRATED | reading_flags::MODIFIED,
            raw_pos: 1234,
            timestamp: 0x0102_0304_0506_0708,
            z: 1000,
            z0: -200,
            z0_prv: -150,
            z0_nxt: -250,
            dt: 2500,
            speed: -12000,
            firing_state: firing_state::FIRED,
            cal_min: 10,
            cal_zero: 700,
            cal_max: 4000,
            cal_sensor_data: [1, 2, 3, 4, 5, 6, 7, 8],
            firing_time_measured: 41_000,
            firing_time_limit: 50_000,
            integration_time: 2500,
            manual_scaling_factor: 100,
            jf_window: 8,
            jf_lo: 1230,
            jf_hi: 1238,
            jf_last_pre: 1236,
            jf_last_post: 1234,
            scan_mode: 2,
            ..Default::default()
        }
        .with_derived()
    }

    #[test]
    fn reading_layout() {
        let b = sample().encode();
        assert_eq!(b.len(), READING_LEN);
        assert_eq!(&b[0..2], &[117, 0]);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 1234);
        assert_eq!(i16::from_le_bytes([b[32], b[33]]), -12000);
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), firing_state::FIRED);
        assert_eq!(u32::from_le_bytes(b[44..48].try_into().unwrap()), 4000);
        assert_eq!(u32::from_le_bytes(b[80..84].try_into().unwrap()), 41_000);
        assert_eq!(u32::from_le_bytes(b[96..100].try_into().unwrap()), 8);
        assert_eq!(b[116], 2);
    }

    #[test]
    fn reading_roundtrip() {
        let s = sample();
        let d = PlungerReading::decode(&s.encode()).unwrap();
        assert_eq!(d, s);
        assert!(d.calibrated && d.modified && !d.calibrating && !d.reverse);
        assert_eq!(d.firing_state_name, "Fired");
        assert_eq!(d.calibration().cal_zero, 700);
    }

    #[test]
    fn reading_larger_struct_with_trailer() {
        // newer firmware: 8 extra bytes in PlungerReading, then a VCNL4010 trailer
        let mut s = sample();
        s.sensor = Some(SensorData::Vcnl4010 { sensor_type: sensor_type::VCNL4010, prox_count: 4321 });
        let enc = s.encode();
        let mut b = enc[..READING_LEN].to_vec();
        b[0..2].copy_from_slice(&((READING_LEN + 8) as u16).to_le_bytes());
        b.extend_from_slice(&[0xEE; 8]);
        b.extend_from_slice(&enc[READING_LEN..]);
        let d = PlungerReading::decode(&b).unwrap();
        assert_eq!(d.raw_pos, 1234);
        assert_eq!(d.scan_mode, 2);
        assert_eq!(d.sensor, s.sensor);
    }

    #[test]
    fn reading_shorter_struct() {
        // older firmware: struct ends before the scan mode / jitter fields
        let mut b = sample().encode();
        b.truncate(96);
        b[0..2].copy_from_slice(&96u16.to_le_bytes());
        let d = PlungerReading::decode(&b).unwrap();
        assert_eq!(d.manual_scaling_factor, 100);
        assert_eq!(d.jf_window, 0);
        assert_eq!(d.scan_mode, 0);
        assert_eq!(d.sensor, None);
        // and a cb that claims more than was sent is clamped
        let mut b = sample().encode();
        b.truncate(40);
        let d = PlungerReading::decode(&b).unwrap();
        assert_eq!(d.cal_min, 10); // cal_min at 36..40 present
        assert_eq!(d.cal_zero, 0);
        assert!(PlungerReading::decode(&[1]).is_err());
    }

    #[test]
    fn image_sensor_trailer() {
        let pixels: Vec<u8> = (0..200u32).map(|i| (i % 256) as u8).collect();
        let sd = SensorData::ImageSensor { sensor_type: sensor_type::TCD1103, timestamp: 99, n_pix: 200, pixels };
        let enc = sd.encode();
        assert_eq!(enc.len(), 216);
        assert_eq!(&enc[14..16], &[200, 0]);
        let mut s = sample();
        s.sensor = Some(sd.clone());
        let d = PlungerReading::decode(&s.encode()).unwrap();
        assert_eq!(d.sensor, Some(sd));
        // nPix larger than the struct: clamp to the bytes present
        let mut bad = enc.clone();
        bad[14..16].copy_from_slice(&5000u16.to_le_bytes());
        match SensorData::decode(&bad) {
            SensorData::ImageSensor { n_pix, pixels, .. } => {
                assert_eq!(n_pix, 200);
                assert_eq!(pixels.len(), 200);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quadrature_and_unknown_trailers() {
        let q = SensorData::decode(&[5, 0, 2, 0, 0x03]);
        assert_eq!(q, SensorData::Quadrature { sensor_type: sensor_type::AEDR8300, state: 3, a: true, b: true });
        // unknown sensor type: opaque, sized by its cb
        let mut s = sample();
        s.sensor = Some(SensorData::Opaque { sensor_type: 0x77, data: vec![6, 0, 0x77, 0, 0xAA, 0xBB] });
        let mut b = s.encode();
        b.extend_from_slice(&[0xFF; 3]); // trailing junk beyond the trailer's cb
        let d = PlungerReading::decode(&b).unwrap();
        assert_eq!(d.sensor, s.sensor);
        assert_eq!(d.sensor.unwrap().sensor_type(), 0x77);
        // a truncated known struct falls back to opaque
        assert!(matches!(SensorData::decode(&[4, 0, 3, 0]), SensorData::Opaque { .. }));
    }

    #[test]
    fn config_decode() {
        let c = PlungerConfig::new(sensor_type::TSL1410R, 1280, 0);
        let b = c.encode();
        assert_eq!(b, [12, 0, 6, 0, 0x00, 0x05, 0, 0, 0, 0, 0, 0]);
        let d = PlungerConfig::decode(&b).unwrap();
        assert_eq!(d, c);
        assert!(d.configured && d.image_sensor && d.has_integration_time);
        assert_eq!(d.scan_modes.len(), 3);
        // larger struct
        let mut big = b.clone();
        big[0] = 20;
        big.extend_from_slice(&[9; 8]);
        assert_eq!(PlungerConfig::decode(&big).unwrap(), c);
        // not configured
        let n = PlungerConfig::decode(&PlungerConfig::new(0, 65535, 0).encode()).unwrap();
        assert!(!n.configured);
        assert_eq!(n.sensor_type_name, "None");
    }

    #[test]
    fn cal_layout_and_roundtrip() {
        let c = PlungerCal {
            calibrated: true,
            cal_min: 1,
            cal_zero: 2,
            cal_max: 3,
            firing_time_measured: 4,
            sensor_data: [5, 6, 7, 8, 9, 10, 11, 12],
        };
        let b = c.encode();
        assert_eq!(b.len(), CAL_LEN);
        assert_eq!(&b[0..8], &[52, 0, 1, 0, 1, 0, 0, 0]);
        assert_eq!(&b[48..52], &[12, 0, 0, 0]);
        assert_eq!(PlungerCal::decode(&b).unwrap(), c);
        assert!(PlungerCal::decode(&b[..40]).is_err());
        assert_eq!(PlungerCal::defaults(600), PlungerCal { cal_max: 600, cal_zero: 100, ..Default::default() });
    }

    #[test]
    fn arg_layouts() {
        assert_eq!(jitter_args(0x1234), [0x02, 0, 0x34, 0x12]);
        assert_eq!(int_args(sub::SET_FIRING_TIME_LIMIT, 50_000), [0x03, 0, 0, 0, 0x50, 0xC3, 0, 0]);
        assert_eq!(byte_args(sub::CALIBRATE, 1), [0x01, 1]);
        assert_eq!(Request::decode(&jitter_args(7)), Request::SetJitterFilter(7));
        assert_eq!(Request::decode(&int_args(sub::SET_SCALING_FACTOR, 120)), Request::SetScalingFactor(120));
        assert_eq!(Request::decode(&byte_args(sub::SET_ORIENTATION, 1)), Request::SetOrientation { reverse: true });
        assert_eq!(Request::decode(&[sub::QUERY_READING]), Request::QueryReading);
        assert_eq!(Request::decode(&[0x55]), Request::Unknown(0x55));
    }
}
