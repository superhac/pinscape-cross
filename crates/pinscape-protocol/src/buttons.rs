//! Buttons: wire structs and constants for CMD_BUTTONS (0x10): button descriptors,
//! logical button states, GPIO / PCA9555 / 74HC165 input states, shift state.
//! See USBProtocol/VendorIfcProtocol.h upstream (`ButtonList`, `ButtonDesc`,
//! `ButtonDevice`, `Args::ButtonState`).

use serde::Serialize;

use super::{DecodeError, Le, Response};

/// Subcommands for [`crate::cmd::BUTTONS`] (first argument byte).
pub mod subcmd {
    pub const QUERY_DESCS: u8 = 0x81;
    pub const QUERY_STATES: u8 = 0x82;
    pub const QUERY_GPIO_STATES: u8 = 0x83;
    pub const QUERY_PCA9555_STATES: u8 = 0x84;
    pub const QUERY_74HC165_STATES: u8 = 0x85;
    pub const QUERY_EVENT_LOG: u8 = 0xA0;
    pub const CLEAR_EVENT_LOG: u8 = 0xA1;
}

/// `ButtonDesc::TYPE_*`: logical button type.
pub mod btn_type {
    pub const PUSH: u8 = 0x01;
    pub const HOLD: u8 = 0x02;
    pub const PULSE: u8 = 0x03;
    pub const TOGGLE: u8 = 0x04;
    pub const ONOFF: u8 = 0x05;
    pub const SHIFT: u8 = 0x06;
}

/// `ButtonDesc::SRC_*`: input source type (also `ButtonDevice::type`).
pub mod src {
    pub const GPIO: u8 = 0x01;
    pub const BOOTSEL: u8 = 0x02;
    pub const PCA9555: u8 = 0x03;
    pub const HC165: u8 = 0x04;
    pub const ACCEL: u8 = 0x05;
    pub const PLUNGER: u8 = 0x06;
    pub const ZBLAUNCH: u8 = 0x07;
    pub const IR: u8 = 0x08;
    pub const CLOCK: u8 = 0x09;
    pub const OUTPORT: u8 = 0x0A;
    pub const NULL: u8 = 0x0B;
    pub const ADC: u8 = 0x0C;
}

/// `ButtonDesc::ACTION_*`: what the button does.
pub mod action {
    pub const NONE: u8 = 0x01;
    pub const KEY: u8 = 0x02;
    pub const MEDIA: u8 = 0x03;
    pub const GAMEPAD: u8 = 0x04;
    pub const XINPUT: u8 = 0x05;
    pub const RESET: u8 = 0x06;
    pub const NIGHTMODE: u8 = 0x07;
    pub const PLUNGERCAL: u8 = 0x08;
    pub const IR: u8 = 0x09;
    pub const MACRO: u8 = 0x0A;
    pub const OPENPINDEV: u8 = 0x0B;
}

/// `ButtonDesc::FLAG_EVENTLOG`.
pub const FLAG_EVENTLOG: u8 = 0x01;

/// Size of the `ButtonList` header as defined by the current protocol.
pub const BUTTON_LIST_LEN: usize = 16;
/// Size of `ButtonDesc` as defined by the current protocol.
pub const BUTTON_DESC_LEN: usize = 16;
/// Size of `ButtonDevice` as defined by the current protocol.
pub const BUTTON_DEVICE_LEN: usize = 8;
/// Minimum header bytes needed to locate the arrays (through `numDevices`).
const BUTTON_LIST_MIN: usize = 14;

/// One logical button (`ButtonDesc`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonDesc {
    /// [`btn_type`] constant.
    #[serde(rename = "type")]
    pub btn_type: u8,
    /// [`src`] constant.
    pub source_type: u8,
    /// PCA9555: chip index; 74HC165: chain index; otherwise 0.
    pub source_unit: u8,
    /// GPIO number, PCA9555 port 0..15, 74HC165 port across the chain, output port, ADC channel.
    pub source_port: u8,
    /// Polarity for physical sources (GPIO, PCA9555, 74HC165).
    pub source_active_high: bool,
    /// [`action`] constant.
    pub action_type: u8,
    /// Key usage, media usage, gamepad button, XInput button, OPD button.
    pub action_detail: u8,
    pub flags: u8,
    pub shift_mask: u32,
    pub shift_bits: u32,
}

impl ButtonDesc {
    /// Decode one descriptor from `b`; missing trailing fields read as zero.
    pub fn decode(b: &[u8]) -> Self {
        let r = Le(b);
        ButtonDesc {
            btn_type: r.u8(0),
            source_type: r.u8(1),
            source_unit: r.u8(2),
            source_port: r.u8(3),
            source_active_high: r.u8(4) != 0,
            action_type: r.u8(5),
            action_detail: r.u8(6),
            flags: r.u8(7),
            shift_mask: r.u32(8),
            shift_bits: r.u32(12),
        }
    }

    pub fn encode(&self) -> [u8; BUTTON_DESC_LEN] {
        let mut b = [0u8; BUTTON_DESC_LEN];
        b[0] = self.btn_type;
        b[1] = self.source_type;
        b[2] = self.source_unit;
        b[3] = self.source_port;
        b[4] = self.source_active_high as u8;
        b[5] = self.action_type;
        b[6] = self.action_detail;
        b[7] = self.flags;
        b[8..12].copy_from_slice(&self.shift_mask.to_le_bytes());
        b[12..16].copy_from_slice(&self.shift_bits.to_le_bytes());
        b
    }

    /// True for inputs read from a pin (GPIO, PCA9555, 74HC165), where polarity applies.
    pub fn is_physical(&self) -> bool {
        matches!(self.source_type, src::GPIO | src::PCA9555 | src::HC165)
    }

    pub fn type_name(t: u8) -> &'static str {
        match t {
            btn_type::PUSH => "Push",
            btn_type::HOLD => "Hold",
            btn_type::PULSE => "Pulse",
            btn_type::TOGGLE => "Toggle",
            btn_type::ONOFF => "On/Off",
            btn_type::SHIFT => "Shift",
            _ => "?",
        }
    }

    pub fn source_name(t: u8) -> &'static str {
        match t {
            src::GPIO => "GPIO",
            src::BOOTSEL => "BOOTSEL",
            src::PCA9555 => "PCA9555",
            src::HC165 => "74HC165",
            src::ACCEL => "Accelerometer",
            src::PLUNGER => "Plunger",
            src::ZBLAUNCH => "ZB Launch",
            src::IR => "IR",
            src::CLOCK => "Clock",
            src::OUTPORT => "Output port",
            src::NULL => "None",
            src::ADC => "ADC",
            _ => "?",
        }
    }

    pub fn action_name(t: u8) -> &'static str {
        match t {
            action::NONE => "None",
            action::KEY => "Key",
            action::MEDIA => "Media",
            action::GAMEPAD => "Gamepad",
            action::XINPUT => "XInput",
            action::RESET => "Reset",
            action::NIGHTMODE => "Night Mode",
            action::PLUNGERCAL => "Plunger calibration",
            action::IR => "IR",
            action::MACRO => "Macro",
            action::OPENPINDEV => "Open Pinball Device",
            _ => "?",
        }
    }
}

/// One physical input device (`ButtonDevice`): a PCA9555 chip or a 74HC165 chain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonDevice {
    /// Index in the config section for this device type.
    pub config_index: u8,
    /// [`src::PCA9555`] or [`src::HC165`].
    #[serde(rename = "type")]
    pub dev_type: u8,
    /// 16 for a PCA9555; 8 x chips for a 74HC165 chain.
    pub num_ports: u16,
    /// PCA9555: `(bus << 8) | i2c_addr`; unused for 74HC165.
    pub addr: u32,
}

impl ButtonDevice {
    pub fn decode(b: &[u8]) -> Self {
        let r = Le(b);
        ButtonDevice { config_index: r.u8(0), dev_type: r.u8(1), num_ports: r.u16(2), addr: r.u32(4) }
    }

    pub fn encode(&self) -> [u8; BUTTON_DEVICE_LEN] {
        let mut b = [0u8; BUTTON_DEVICE_LEN];
        b[0] = self.config_index;
        b[1] = self.dev_type;
        b[2..4].copy_from_slice(&self.num_ports.to_le_bytes());
        b[4..8].copy_from_slice(&self.addr.to_le_bytes());
        b
    }
}

/// Reply to [`subcmd::QUERY_DESCS`]: the logical buttons and physical input devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonConfig {
    pub buttons: Vec<ButtonDesc>,
    pub devices: Vec<ButtonDevice>,
}

impl ButtonConfig {
    /// Decode a `ButtonList` header plus its two arrays, honoring the header's
    /// offsets and element sizes so larger (newer) structs still decode.
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < BUTTON_LIST_MIN {
            return Err(DecodeError::Length { expected: BUTTON_LIST_MIN, got: xfer.len() });
        }
        let r = Le(xfer);
        let (ofs_desc, cb_desc, n_desc) = (r.u16(2) as usize, r.u16(4) as usize, r.u16(6) as usize);
        let (ofs_dev, cb_dev, n_dev) = (r.u16(8) as usize, r.u16(10) as usize, r.u16(12) as usize);
        let array = |ofs: usize, cb: usize, n: usize| -> Result<Vec<&[u8]>, DecodeError> {
            let end = ofs + cb * n;
            if end > xfer.len() || (n > 0 && cb == 0) {
                return Err(DecodeError::Length { expected: end, got: xfer.len() });
            }
            Ok((0..n).map(|i| &xfer[ofs + i * cb..ofs + (i + 1) * cb]).collect())
        };
        Ok(ButtonConfig {
            buttons: array(ofs_desc, cb_desc, n_desc)?.into_iter().map(ButtonDesc::decode).collect(),
            devices: array(ofs_dev, cb_dev, n_dev)?.into_iter().map(ButtonDevice::decode).collect(),
        })
    }

    /// Encode with the current struct sizes, padding each descriptor with
    /// `desc_pad` extra zero bytes (to simulate newer firmware).
    pub fn encode(&self, desc_pad: usize) -> Vec<u8> {
        let cb_desc = BUTTON_DESC_LEN + desc_pad;
        let ofs_desc = BUTTON_LIST_LEN;
        let ofs_dev = ofs_desc + cb_desc * self.buttons.len();
        let mut b = Vec::with_capacity(ofs_dev + BUTTON_DEVICE_LEN * self.devices.len());
        for v in [
            BUTTON_LIST_LEN,
            ofs_desc,
            cb_desc,
            self.buttons.len(),
            ofs_dev,
            BUTTON_DEVICE_LEN,
            self.devices.len(),
            0,
        ] {
            b.extend_from_slice(&(v as u16).to_le_bytes());
        }
        for d in &self.buttons {
            b.extend_from_slice(&d.encode());
            b.extend(std::iter::repeat_n(0u8, desc_pad));
        }
        for d in &self.devices {
            b.extend_from_slice(&d.encode());
        }
        b
    }
}

/// Reply to [`subcmd::QUERY_STATES`]: logical button states and the global shift state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonStates {
    /// One entry per logical button, in descriptor order.
    pub states: Vec<bool>,
    /// Bits of the shift buttons currently pressed (0 if the firmware didn't report it).
    pub shift_state: u32,
}

impl ButtonStates {
    pub fn decode(resp: &Response, xfer: &[u8]) -> Self {
        ButtonStates {
            states: xfer.iter().map(|&b| b != 0).collect(),
            shift_state: resp.args_checked(4).map(|a| Le(a).u32(0)).unwrap_or(0),
        }
    }
}

/// Raw port levels (one byte per port, 0 LOW / 1 HIGH) as booleans.
pub fn decode_levels(xfer: &[u8]) -> Vec<bool> {
    xfer.iter().map(|&b| b != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cmd, ARGS_LEN, RESPONSE_LEN};

    fn sample() -> ButtonConfig {
        ButtonConfig {
            buttons: vec![
                ButtonDesc {
                    btn_type: btn_type::PUSH,
                    source_type: src::GPIO,
                    source_port: 5,
                    action_type: action::KEY,
                    action_detail: 0x04,
                    ..Default::default()
                },
                ButtonDesc {
                    btn_type: btn_type::PUSH,
                    source_type: src::PCA9555,
                    source_unit: 1,
                    source_port: 11,
                    source_active_high: true,
                    action_type: action::GAMEPAD,
                    action_detail: 3,
                    flags: FLAG_EVENTLOG,
                    shift_mask: 0x01,
                    shift_bits: 0x01,
                },
            ],
            devices: vec![
                ButtonDevice { config_index: 0, dev_type: src::PCA9555, num_ports: 16, addr: 0x0120 },
                ButtonDevice { config_index: 0, dev_type: src::HC165, num_ports: 24, addr: 0 },
            ],
        }
    }

    #[test]
    fn desc_layout() {
        let d = &sample().buttons[1];
        let b = d.encode();
        assert_eq!(b, [1, 3, 1, 11, 1, 4, 3, 1, 1, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(&ButtonDesc::decode(&b), d);
    }

    #[test]
    fn config_roundtrip() {
        let c = sample();
        let b = c.encode(0);
        assert_eq!(b.len(), 16 + 2 * 16 + 2 * 8);
        assert_eq!(ButtonConfig::decode(&b).unwrap(), c);
    }

    #[test]
    fn config_tolerates_larger_structs() {
        let c = sample();
        // newer firmware: 8 extra bytes per desc, a larger header and a larger device struct
        let mut b = Vec::new();
        let (cb, cb_desc, cb_dev) = (20usize, 24usize, 12usize);
        let ofs_dev = cb + cb_desc * 2;
        for v in [cb, cb, cb_desc, 2, ofs_dev, cb_dev, 2, 0] {
            b.extend_from_slice(&(v as u16).to_le_bytes());
        }
        b.extend_from_slice(&[0xEE; 4]); // unknown header fields
        for d in &c.buttons {
            b.extend_from_slice(&d.encode());
            b.extend_from_slice(&[0xAA; 8]);
        }
        for d in &c.devices {
            b.extend_from_slice(&d.encode());
            b.extend_from_slice(&[0xBB; 4]);
        }
        assert_eq!(ButtonConfig::decode(&b).unwrap(), c);
        // and the padded encoder produces something the decoder accepts too
        assert_eq!(ButtonConfig::decode(&c.encode(8)).unwrap(), c);
    }

    #[test]
    fn config_tolerates_smaller_desc() {
        // older firmware without shift fields: cbDesc = 8
        let mut b = Vec::new();
        for v in [16u16, 16, 8, 1, 24, 8, 0, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[1, 1, 0, 7, 0, 2, 0x28, 0]);
        let c = ButtonConfig::decode(&b).unwrap();
        assert_eq!(c.buttons[0].source_port, 7);
        assert_eq!(c.buttons[0].action_detail, 0x28);
        assert_eq!(c.buttons[0].shift_mask, 0);
        assert!(c.devices.is_empty());
    }

    #[test]
    fn config_empty_and_errors() {
        let c = ButtonConfig::decode(&ButtonConfig::default().encode(0)).unwrap();
        assert!(c.buttons.is_empty() && c.devices.is_empty());
        assert!(ButtonConfig::decode(&[16, 0, 16]).is_err());
        // array runs past the end of the buffer
        let mut b = sample().encode(0);
        b.truncate(b.len() - 1);
        assert!(ButtonConfig::decode(&b).is_err());
    }

    #[test]
    fn states_decode() {
        let mut raw = [0u8; RESPONSE_LEN];
        raw[4] = cmd::BUTTONS;
        raw[5] = 4;
        raw[12..16].copy_from_slice(&0x05u32.to_le_bytes());
        let r = Response::decode(&raw).unwrap();
        let s = ButtonStates::decode(&r, &[0, 1, 1, 0]);
        assert_eq!(s.states, vec![false, true, true, false]);
        assert_eq!(s.shift_state, 5);
        // older firmware without the shift-state argument
        let r2 = Response { args_size: 0, args: [0; ARGS_LEN], ..r };
        assert_eq!(ButtonStates::decode(&r2, &[]).shift_state, 0);
    }

    #[test]
    fn names() {
        assert_eq!(ButtonDesc::source_name(src::HC165), "74HC165");
        assert_eq!(ButtonDesc::type_name(btn_type::SHIFT), "Shift");
        assert_eq!(ButtonDesc::action_name(action::XINPUT), "XInput");
        assert!(sample().buttons[0].is_physical());
    }
}
