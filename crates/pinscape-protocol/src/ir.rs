//! IR: wire structs and constants for CMD_QUERY_IR (0x14), CMD_SEND_IR (0x0D), CMD_TVON (0x0A): received IR commands and raw pulses, sending IR, TV-ON state and relay.
//! See USBProtocol/VendorIfcProtocol.h upstream (`IRCommandList`, `IRRawList`,
//! `args.sendIR`, `args.tvon`), and Firmware/IRRemote/IRCommand.h for the
//! universal IR command code format.

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use super::{DecodeError, Le, Response};

/// [`crate::cmd::QUERY_IR`] subcommand: decoded commands received since the last query (clears the buffer).
pub const SUBCMD_QUERY_IR_CMD: u8 = 0x01;
/// [`crate::cmd::QUERY_IR`] subcommand: raw pulses of the latest code (clears the buffer).
pub const SUBCMD_QUERY_IR_RAW: u8 = 0x02;

/// [`crate::cmd::TVON`] subcommand: query the TV ON state (reply in `args.tvon`).
pub const SUBCMD_TVON_QUERY_STATE: u8 = 0x01;
/// [`crate::cmd::TVON`] subcommand: set the TV relay; argument byte 1 is a `TVON_RELAY_*` value.
pub const SUBCMD_TVON_SET_RELAY: u8 = 0x02;
pub const TVON_RELAY_OFF: u8 = 0x01;
pub const TVON_RELAY_ON: u8 = 0x02;
pub const TVON_RELAY_PULSE: u8 = 0x03;

/// Universal code flag: the code uses dittos for auto-repeat (`IRCommandDesc::useDittos`).
pub const FLAG_USE_DITTOS: u8 = 0x02;

/// `IRCommandListEle::proFlags`: the protocol has a ditto format.
pub const FPRO_DITTOS: u8 = 0x02;
/// `IRCommandListEle::cmdFlags` bits.
pub const F_HAS_TOGGLE: u8 = 0x01;
pub const F_TOGGLE_BIT: u8 = 0x02;
pub const F_HAS_DITTO: u8 = 0x04;
pub const F_DITTO_FLAG: u8 = 0x08;
pub const F_POS_MASK: u8 = 0x30;
pub const F_POS_FIRST: u8 = 0x10;
pub const F_POS_MIDDLE: u8 = 0x20;
pub const F_POS_LAST: u8 = 0x30;
pub const F_AUTOREPEAT: u8 = 0x40;

/// `IRRaw::t` value meaning "longer than 131.068 ms".
pub const RAW_T_OVERFLOW: u16 = 0xFFFF;

/// Wire sizes of the current structs (newer firmware may send larger ones).
pub const IR_LIST_HEADER_LEN: usize = 8;
pub const IR_COMMAND_ELE_LEN: usize = 24;
pub const IR_RAW_ELE_LEN: usize = 4;
pub const SEND_IR_ARGS_LEN: usize = 11;
pub const TVON_ARGS_LEN: usize = 5;

/// Protocol IDs (Firmware/IRRemote/IRProtocolID.h) and display names.
pub const PROTOCOLS: &[(u8, &str)] = &[
    (1, "NEC-32"),
    (2, "NEC-32x"),
    (3, "NEC-48"),
    (4, "Philips RC5"),
    (5, "Philips RC6"),
    (6, "Kaseikyo-48"),
    (7, "Kaseikyo-56"),
    (8, "Denon-K"),
    (9, "Fujitsu-48"),
    (10, "Fujitsu-56"),
    (11, "JVC-48"),
    (12, "JVC-56"),
    (13, "Mitsubishi-K"),
    (14, "Panasonic-48"),
    (15, "Panasonic-56"),
    (16, "Sharp-K"),
    (17, "Teac-K"),
    (18, "Denon"),
    (19, "Pioneer"),
    (20, "Samsung-20"),
    (21, "Samsung-36"),
    (22, "Sony-8"),
    (23, "Sony-12"),
    (25, "Sony-15"),
    (26, "Sony-20"),
    (27, "Ortek MCE"),
    (28, "Lutron"),
    (29, "TCL Roku"),
    (30, "JVC-16"),
    (31, "TCL/JVC-16"),
    (32, "TCL/JVC-24"),
];

/// Display name for a protocol ID ("Unknown" if not in [`PROTOCOLS`]).
pub fn protocol_name(id: u8) -> &'static str {
    PROTOCOLS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n).unwrap_or("Unknown")
}

fn ser_hex_u64<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    // u64 doesn't survive a trip through a JavaScript number; send it as hex.
    s.serialize_str(&format!("{v:X}"))
}

// ---------------------------------------------------------------------------
// Universal IR command code ("PP.FF.CCCC…")
// ---------------------------------------------------------------------------

/// An IR command in the Pinscape universal format: protocol ID, flags and
/// command code, written as `"<protocol>.<flags>.<code>"` in hex — e.g.
/// `01.02.20DF10EF`. This is the same text the config file accepts
/// (`irTx.commands[].code`, `tvon.IR[]`, button/output IR sources).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrCode {
    pub protocol: u8,
    /// [`FLAG_USE_DITTOS`] is the only flag currently defined.
    pub flags: u8,
    #[serde(serialize_with = "ser_hex_u64")]
    pub code: u64,
}

impl IrCode {
    pub fn new(protocol: u8, flags: u8, code: u64) -> Self {
        IrCode { protocol, flags, code }
    }

    pub fn use_dittos(&self) -> bool {
        self.flags & FLAG_USE_DITTOS != 0
    }
}

/// Formats like the firmware's `IRCommandDesc::ToString()`: two hex digits
/// each for protocol and flags, then the code padded to 16, 32, 48, 56 or
/// 64 bits depending on its magnitude.
impl fmt::Display for IrCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code;
        let bits = if c >= 1 << 56 {
            64
        } else if c >= 1 << 48 {
            56
        } else if c >= 1 << 32 {
            48
        } else if c >= 1 << 16 {
            32
        } else {
            16
        };
        write!(f, "{:02X}.{:02X}.{:0w$X}", self.protocol, self.flags, c, w = bits / 4)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid IR code '{0}': expected <protocol>.<flags>.<code> in hex, e.g. 01.02.20DF10EF")]
pub struct IrCodeParseError(pub String);

/// Parses like the firmware's `IRCommandDesc::Parse()`: optional surrounding
/// whitespace, exactly two hex digits for protocol and flags, then 1 to 16
/// hex digits for the code.
impl FromStr for IrCode {
    type Err = IrCodeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || IrCodeParseError(s.trim().to_string());
        let t = s.trim();
        let mut parts = t.split('.');
        let (Some(p), Some(f), Some(c), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(err());
        };
        let hex = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_hexdigit());
        if p.len() != 2 || f.len() != 2 || c.len() > 16 || !hex(p) || !hex(f) || !hex(c) {
            return Err(err());
        }
        Ok(IrCode {
            protocol: u8::from_str_radix(p, 16).map_err(|_| err())?,
            flags: u8::from_str_radix(f, 16).map_err(|_| err())?,
            code: u64::from_str_radix(c, 16).map_err(|_| err())?,
        })
    }
}

/// Arguments for [`crate::cmd::SEND_IR`] (`args.sendIR`, 11 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendIrArgs {
    pub code: IrCode,
    /// Number of times to send (1..=255). With [`FLAG_USE_DITTOS`], repeats are sent as dittos.
    pub count: u8,
}

impl SendIrArgs {
    pub fn encode(&self) -> [u8; SEND_IR_ARGS_LEN] {
        let mut b = [0u8; SEND_IR_ARGS_LEN];
        b[0..8].copy_from_slice(&self.code.code.to_le_bytes());
        b[8] = self.code.protocol;
        b[9] = self.code.flags;
        b[10] = self.count;
        b
    }

    pub fn decode(a: &[u8]) -> Self {
        let r = Le(a);
        SendIrArgs { code: IrCode::new(r.u8(8), r.u8(9), r.u64(0)), count: r.u8(10) }
    }
}

// ---------------------------------------------------------------------------
// Received commands
// ---------------------------------------------------------------------------

/// Position marker in protocols that encode one (e.g. Ortek MCE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IrPosition {
    First,
    Middle,
    Last,
}

/// One `IRCommandListEle` from [`SUBCMD_QUERY_IR_CMD`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrCommandReceived {
    /// Microseconds since the previous command was received.
    pub dt_us: u64,
    /// Protocol, protocol flags and command code. As in the upstream API,
    /// the flags are the *protocol* flags, so `code.to_string()` gives a
    /// code that can be pasted into the config or sent back.
    pub code: IrCode,
    pub protocol_name: &'static str,
    /// Universal text format of `code`, e.g. `01.02.20DF10EF`.
    pub text: String,
    pub pro_flags: u8,
    pub cmd_flags: u8,
    /// The protocol has a ditto format.
    pub pro_has_dittos: bool,
    pub has_toggle: bool,
    pub toggle: bool,
    pub has_ditto: bool,
    pub ditto: bool,
    pub position: Option<IrPosition>,
    /// The firmware judged this an auto-repeat (button held down).
    pub auto_repeat: bool,
}

impl IrCommandReceived {
    fn from_ele(e: &[u8]) -> Self {
        let r = Le(e);
        let (protocol, pro_flags, cmd_flags) = (r.u8(16), r.u8(17), r.u8(18));
        let code = IrCode::new(protocol, pro_flags, r.u64(8));
        let has = |bit: u8| cmd_flags & bit != 0;
        IrCommandReceived {
            dt_us: r.u64(0),
            code,
            protocol_name: protocol_name(protocol),
            text: code.to_string(),
            pro_flags,
            cmd_flags,
            pro_has_dittos: pro_flags & FPRO_DITTOS != 0,
            has_toggle: has(F_HAS_TOGGLE),
            toggle: has(F_TOGGLE_BIT),
            has_ditto: has(F_HAS_DITTO),
            ditto: has(F_DITTO_FLAG),
            position: match cmd_flags & F_POS_MASK {
                F_POS_FIRST => Some(IrPosition::First),
                F_POS_MIDDLE => Some(IrPosition::Middle),
                F_POS_LAST => Some(IrPosition::Last),
                _ => None,
            },
            auto_repeat: has(F_AUTOREPEAT),
        }
    }

    /// Decode an `IRCommandList` transfer (header + packed elements). The
    /// header's `cb` and `cbEle` are honored, so newer firmware with larger
    /// structs still decodes; fields missing from shorter elements read as 0.
    pub fn decode_list(xfer: &[u8]) -> Result<Vec<Self>, DecodeError> {
        decode_list(xfer, 16).map(|eles| eles.map(Self::from_ele).collect())
    }

    /// Encode a list as the firmware does (used by the simulated device and tests).
    pub fn encode_list(items: &[(u64, IrCode, u8)]) -> Vec<u8> {
        let mut b = list_header(IR_COMMAND_ELE_LEN, items.len());
        for (dt, code, cmd_flags) in items {
            b.extend_from_slice(&dt.to_le_bytes());
            b.extend_from_slice(&code.code.to_le_bytes());
            b.extend_from_slice(&[code.protocol, code.flags, *cmd_flags, 0, 0, 0, 0, 0]);
        }
        b
    }
}

// ---------------------------------------------------------------------------
// Raw pulses
// ---------------------------------------------------------------------------

/// One `IRRaw` element from [`SUBCMD_QUERY_IR_RAW`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrRawPulse {
    /// true = mark (IR on), false = space (IR off).
    pub mark: bool,
    /// Duration in microseconds; `None` if longer than 131.068 ms.
    pub t_us: Option<u32>,
}

impl IrRawPulse {
    fn from_ele(e: &[u8]) -> Self {
        let r = Le(e);
        let t = r.u16(0);
        IrRawPulse { mark: r.u8(2) != 0, t_us: (t != RAW_T_OVERFLOW).then_some(t as u32 * 2) }
    }

    /// Wire `t` value: duration in 2 µs units, saturating to [`RAW_T_OVERFLOW`].
    pub fn wire_t(t_us: Option<u32>) -> u16 {
        match t_us {
            Some(us) => (us / 2).min(RAW_T_OVERFLOW as u32 - 1) as u16,
            None => RAW_T_OVERFLOW,
        }
    }

    /// Decode an `IRRawList` transfer (header + packed elements), honoring `cb`/`cbRaw`.
    pub fn decode_list(xfer: &[u8]) -> Result<Vec<Self>, DecodeError> {
        decode_list(xfer, 3).map(|eles| eles.map(Self::from_ele).collect())
    }

    /// Encode a list as the firmware does (used by the simulated device and tests).
    pub fn encode_list(items: &[IrRawPulse]) -> Vec<u8> {
        let mut b = list_header(IR_RAW_ELE_LEN, items.len());
        for p in items {
            b.extend_from_slice(&Self::wire_t(p.t_us).to_le_bytes());
            b.extend_from_slice(&[p.mark as u8, 0]);
        }
        b
    }
}

fn list_header(cb_ele: usize, n: usize) -> Vec<u8> {
    let mut b = Vec::with_capacity(IR_LIST_HEADER_LEN + cb_ele * n);
    b.extend_from_slice(&(IR_LIST_HEADER_LEN as u16).to_le_bytes());
    b.extend_from_slice(&(cb_ele as u16).to_le_bytes());
    b.extend_from_slice(&(n as u16).to_le_bytes());
    b.extend_from_slice(&[0, 0]);
    b
}

/// Shared `{cb, cbEle, numEle}` list walker. `min_ele` is the smallest
/// element size that still carries the essential fields.
fn decode_list(xfer: &[u8], min_ele: usize) -> Result<impl Iterator<Item = &[u8]>, DecodeError> {
    if xfer.len() < 6 {
        return Err(DecodeError::Length { expected: IR_LIST_HEADER_LEN, got: xfer.len() });
    }
    let r = Le(xfer);
    let cb = r.u16(0) as usize;
    let cb_ele = r.u16(2) as usize;
    let n = r.u16(4) as usize;
    let need = cb + n * cb_ele;
    if cb < 6 || (n > 0 && cb_ele < min_ele) || need > xfer.len() {
        return Err(DecodeError::Length { expected: need.max(cb + n * min_ele), got: xfer.len() });
    }
    Ok((0..n).map(move |i| &xfer[cb + i * cb_ele..cb + (i + 1) * cb_ele]))
}

// ---------------------------------------------------------------------------
// TV ON
// ---------------------------------------------------------------------------

/// `args.tvon` power states.
pub mod pwr {
    pub const OFF: u8 = 0;
    pub const PULSELATCH: u8 = 1;
    pub const TESTLATCH: u8 = 2;
    pub const COUNTDOWN: u8 = 3;
    pub const RELAYON: u8 = 4;
    pub const IRREADY: u8 = 5;
    pub const IRWAITING: u8 = 6;
    pub const IRSENDING: u8 = 7;
    pub const ON: u8 = 8;
    pub const PICOBOOT: u8 = 9;
}

/// `args.tvon.relayState` bits.
pub const RELAY_STATE_POWERON: u8 = 0x01;
pub const RELAY_STATE_MANUAL: u8 = 0x02;
pub const RELAY_STATE_MANUAL_PULSE: u8 = 0x04;

/// Name (as in the upstream Windows tools) and description of a TV ON power state.
pub fn power_state_info(s: u8) -> (String, &'static str) {
    let (name, desc) = match s {
        pwr::OFF => ("Power Off", "Power was off at the last check; watching for the power-sense circuit to report power on."),
        pwr::PULSELATCH => ("Pulsing Latch", "Power was off; pulsing the power-sense latch to test for a new state."),
        pwr::TESTLATCH => ("Testing Latch", "Power was off; reading the latch to see whether power has come on."),
        pwr::COUNTDOWN => ("Countdown", "Off-to-on power transition detected; waiting out the configured delay before switching the TV on."),
        pwr::RELAYON => ("Pulsing Relay", "Pulsing the TV relay (hard-wired to the TV's power button)."),
        pwr::IRREADY => ("IR Ready", "Ready to send the next TV ON IR command."),
        pwr::IRWAITING => ("IR Waiting", "Waiting for the configured pause between TV ON IR commands."),
        pwr::IRSENDING => ("IR Sending", "IR command sent; waiting for the transmission to complete."),
        pwr::ON => ("Power On", "Power was on at the last check; the TV ON sequence is complete."),
        pwr::PICOBOOT => ("Pico Boot", "The Pico just rebooted; the external power state is not yet known."),
        _ => return (format!("State #{s}"), "Unknown state (newer firmware?)."),
    };
    (name.to_string(), desc)
}

/// Reply to [`crate::cmd::TVON`] / [`SUBCMD_TVON_QUERY_STATE`] (`args.tvon`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvOnState {
    pub power_state: u8,
    pub power_state_name: String,
    pub power_state_description: &'static str,
    /// Power-sense GPIO reads high.
    pub gpio_high: bool,
    pub relay_state: u8,
    /// Relay is energized for any reason.
    pub relay_on: bool,
    pub relay_power_on: bool,
    pub relay_manual: bool,
    pub relay_manual_pulse: bool,
    /// Index of the IR command being sent (valid during the IR states).
    pub ir_command_index: u8,
    /// Number of IR commands configured to send at power-on.
    pub ir_command_count: u8,
}

impl TvOnState {
    pub fn decode(resp: &Response) -> Result<Self, DecodeError> {
        Ok(Self::from_args(resp.args_checked(1)?))
    }

    pub fn from_args(a: &[u8]) -> Self {
        let r = Le(a);
        let power_state = r.u8(0);
        let relay_state = r.u8(2);
        let (power_state_name, power_state_description) = power_state_info(power_state);
        TvOnState {
            power_state,
            power_state_name,
            power_state_description,
            gpio_high: r.u8(1) != 0,
            relay_state,
            relay_on: relay_state != 0,
            relay_power_on: relay_state & RELAY_STATE_POWERON != 0,
            relay_manual: relay_state & RELAY_STATE_MANUAL != 0,
            relay_manual_pulse: relay_state & RELAY_STATE_MANUAL_PULSE != 0,
            ir_command_index: r.u8(3),
            ir_command_count: r.u8(4),
        }
    }

    /// Encode `args.tvon` (used by the simulated device and tests).
    pub fn encode_args(power_state: u8, gpio: u8, relay: u8, ir_index: u8, ir_count: u8) -> [u8; TVON_ARGS_LEN] {
        [power_state, gpio, relay, ir_index, ir_count]
    }
}

/// Manual TV relay actions for [`SUBCMD_TVON_SET_RELAY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TvRelayAction {
    Off,
    On,
    Pulse,
}

impl TvRelayAction {
    pub fn wire(self) -> u8 {
        match self {
            TvRelayAction::Off => TVON_RELAY_OFF,
            TvRelayAction::On => TVON_RELAY_ON,
            TvRelayAction::Pulse => TVON_RELAY_PULSE,
        }
    }

    pub fn encode(self) -> [u8; 2] {
        [SUBCMD_TVON_SET_RELAY, self.wire()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cmd, RESPONSE_LEN};

    #[test]
    fn code_format_matches_firmware() {
        assert_eq!(IrCode::new(1, 2, 0x20DF10EF).to_string(), "01.02.20DF10EF");
        assert_eq!(IrCode::new(0x17, 0, 0x95).to_string(), "17.00.0095");
        assert_eq!(IrCode::new(4, 0, 0xFFFF).to_string(), "04.00.FFFF");
        assert_eq!(IrCode::new(4, 0, 0x1_0000).to_string(), "04.00.00010000");
        assert_eq!(IrCode::new(6, 0, 0x4004_0100_BCBD).to_string(), "06.00.40040100BCBD");
        assert_eq!(IrCode::new(7, 0, 0x01_0000_0000_0000).to_string(), "07.00.01000000000000");
        assert_eq!(IrCode::new(7, 0, u64::MAX).to_string(), "07.00.FFFFFFFFFFFFFFFF");
    }

    #[test]
    fn code_parse_round_trip() {
        for c in [
            IrCode::new(1, 2, 0x20DF10EF),
            IrCode::new(0x20, 0, 0),
            IrCode::new(0xFF, 0xFF, u64::MAX),
            IrCode::new(5, 0, 0x1_0000_0000),
            IrCode::new(23, 0, 0xA90),
        ] {
            assert_eq!(c.to_string().parse::<IrCode>().unwrap(), c, "{c}");
        }
        assert_eq!(" 02.10.e0e040bf\t".parse::<IrCode>().unwrap(), IrCode::new(2, 0x10, 0xE0E040BF));
        assert_eq!("01.00.7".parse::<IrCode>().unwrap(), IrCode::new(1, 0, 7));
    }

    #[test]
    fn code_parse_rejects_malformed() {
        for s in [
            "", "1.02.1234", "001.02.1234", "01.2.1234", "01.02.", "01.02.12345678901234567",
            "01.02.12G4", "01-02-1234", "01.02.1234.5", "01.02.12 34", "0x.02.1234", "01.02.+123",
        ] {
            assert!(s.parse::<IrCode>().is_err(), "{s:?} should not parse");
        }
    }

    #[test]
    fn send_args_layout() {
        let a = SendIrArgs { code: IrCode::new(1, FLAG_USE_DITTOS, 0x0102_0304_0506_0708), count: 3 };
        let b = a.encode();
        assert_eq!(b, [8, 7, 6, 5, 4, 3, 2, 1, 1, 2, 3]);
        assert_eq!(SendIrArgs::decode(&b), a);
    }

    #[test]
    fn command_list_decode() {
        let items = [
            (1_234_567u64, IrCode::new(1, FPRO_DITTOS, 0x20DF10EF), F_HAS_DITTO),
            (108_000, IrCode::new(1, FPRO_DITTOS, 0), F_HAS_DITTO | F_DITTO_FLAG | F_AUTOREPEAT),
            (5, IrCode::new(27, 0, 0x1234), F_HAS_TOGGLE | F_TOGGLE_BIT | F_POS_LAST),
        ];
        let b = IrCommandReceived::encode_list(&items);
        assert_eq!(b.len(), 8 + 3 * 24);
        let v = IrCommandReceived::decode_list(&b).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].dt_us, 1_234_567);
        assert_eq!(v[0].text, "01.02.20DF10EF");
        assert_eq!(v[0].protocol_name, "NEC-32");
        assert!(v[0].pro_has_dittos && v[0].has_ditto && !v[0].ditto && !v[0].auto_repeat);
        assert!(v[1].ditto && v[1].auto_repeat);
        assert_eq!(v[2].position, Some(IrPosition::Last));
        assert!(v[2].has_toggle && v[2].toggle);
        assert_eq!(v[2].protocol_name, "Ortek MCE");
        // empty list
        assert!(IrCommandReceived::decode_list(&IrCommandReceived::encode_list(&[])).unwrap().is_empty());
    }

    #[test]
    fn command_list_tolerates_larger_structs() {
        // header cb = 12, cbEle = 32 (newer firmware with extra fields)
        let mut b = vec![12, 0, 32, 0, 2, 0, 0, 0, 0xEE, 0xEE, 0xEE, 0xEE];
        for (dt, code) in [(10u64, 0xAAu64), (20, 0xBB)] {
            let mut e = vec![0u8; 32];
            e[0..8].copy_from_slice(&dt.to_le_bytes());
            e[8..16].copy_from_slice(&code.to_le_bytes());
            e[16] = 23;
            e[18] = F_AUTOREPEAT;
            e[24..32].fill(0x55); // unknown trailing fields
            b.extend(e);
        }
        let v = IrCommandReceived::decode_list(&b).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].dt_us, v[0].code.code), (10, 0xAA));
        assert_eq!((v[1].dt_us, v[1].code.code), (20, 0xBB));
        assert_eq!(v[1].text, "17.00.00BB");
        assert!(v[1].auto_repeat);
    }

    #[test]
    fn command_list_shorter_struct_and_errors() {
        // cbEle = 17: dt, cmd, protocol only -> flags read as 0
        let mut b = vec![8, 0, 17, 0, 1, 0, 0, 0];
        b.extend_from_slice(&7u64.to_le_bytes());
        b.extend_from_slice(&0x1234u64.to_le_bytes());
        b.push(1);
        let v = IrCommandReceived::decode_list(&b).unwrap();
        assert_eq!((v[0].code.protocol, v[0].cmd_flags), (1, 0));
        // truncated
        assert!(IrCommandReceived::decode_list(&b[..b.len() - 1]).is_err());
        assert!(IrCommandReceived::decode_list(&[8, 0]).is_err());
        // element too small to hold the essentials
        assert!(IrCommandReceived::decode_list(&[8, 0, 4, 0, 1, 0, 0, 0, 1, 2, 3, 4]).is_err());
    }

    #[test]
    fn raw_list_round_trip_and_overflow() {
        let pulses = [
            IrRawPulse { mark: false, t_us: None },
            IrRawPulse { mark: true, t_us: Some(9000) },
            IrRawPulse { mark: false, t_us: Some(4500) },
            IrRawPulse { mark: true, t_us: Some(562) },
        ];
        let b = IrRawPulse::encode_list(&pulses);
        assert_eq!(&b[..8], &[8, 0, 4, 0, 4, 0, 0, 0]);
        assert_eq!(&b[8..12], &[0xFF, 0xFF, 0, 0]);
        assert_eq!(&b[12..16], &[0x94, 0x11, 1, 0]); // 4500 * 2us = 9000us
        assert_eq!(IrRawPulse::decode_list(&b).unwrap(), pulses);
        // larger element stride
        let b = [8, 0, 6, 0, 2, 0, 0, 0, 100, 0, 1, 0, 9, 9, 50, 0, 0, 0, 9, 9];
        let v = IrRawPulse::decode_list(&b).unwrap();
        assert_eq!(v, [IrRawPulse { mark: true, t_us: Some(200) }, IrRawPulse { mark: false, t_us: Some(100) }]);
    }

    #[test]
    fn tvon_decode() {
        let mut b = [0u8; RESPONSE_LEN];
        b[4] = cmd::TVON;
        b[5] = 5;
        b[12..17].copy_from_slice(&TvOnState::encode_args(pwr::IRWAITING, 1, RELAY_STATE_MANUAL | RELAY_STATE_POWERON, 1, 3));
        let r = Response::decode(&b).unwrap();
        let t = TvOnState::decode(&r).unwrap();
        assert_eq!(t.power_state_name, "IR Waiting");
        assert!(t.gpio_high && t.relay_on && t.relay_manual && t.relay_power_on && !t.relay_manual_pulse);
        assert_eq!((t.ir_command_index, t.ir_command_count), (1, 3));
        // unknown state, short args
        let t = TvOnState::from_args(&[42]);
        assert_eq!(t.power_state_name, "State #42");
        assert!(!t.relay_on);
    }

    #[test]
    fn relay_args() {
        assert_eq!(TvRelayAction::On.encode(), [0x02, 0x02]);
        assert_eq!(TvRelayAction::Off.encode(), [0x02, 0x01]);
        assert_eq!(TvRelayAction::Pulse.encode(), [0x02, 0x03]);
    }
}
