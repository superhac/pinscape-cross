//! Outputs: wire structs and constants for [`cmd::OUTPUTS`](crate::cmd::OUTPUTS)
//! (0x11): logical ports, output devices and device ports, levels, test mode,
//! setting logical/physical port levels, night mode.
//!
//! See `USBProtocol/VendorIfcProtocol.h` upstream (`OutputPortList`,
//! `OutputDevList`, `OutputDevPortList`, `OutputLevelList`,
//! `OutputDevLevelList`, `Args::OutputTestMode`, `Args::OutputDevPort`).
//!
//! The list replies all start with a small header carrying its own size
//! (`cb`) and the element size (`cbDesc`/`cbLevel`); the decoders here use
//! those, as the upstream Windows code does, so newer firmware that appends
//! fields to either struct still decodes, and older firmware with shorter
//! structs decodes with zeroes for the missing fields.

use serde::Serialize;

use crate::{DecodeError, Le, Response};

/// Subcommands (first argument byte) for [`cmd::OUTPUTS`](crate::cmd::OUTPUTS).
pub mod sub {
    pub const SET_PORT: u8 = 0x01;
    pub const TEST_MODE: u8 = 0x02;
    pub const SET_DEVICE_PORT: u8 = 0x03;
    pub const SET_NIGHT_MODE: u8 = 0x04;
    pub const PWMWORKER_RESET: u8 = 0x70;
    pub const PWMWORKER_BOOTLOADER: u8 = 0x71;
    pub const QUERY_LOGICAL_PORTS: u8 = 0x81;
    pub const QUERY_DEVICES: u8 = 0x82;
    pub const QUERY_DEVICE_PORTS: u8 = 0x83;
    pub const QUERY_LOGICAL_PORT_LEVELS: u8 = 0x84;
    pub const QUERY_DEVICE_PORT_LEVELS: u8 = 0x85;
    pub const QUERY_LOGICAL_PORT_NAME: u8 = 0x86;
    pub const QUERY_NIGHT_MODE: u8 = 0x87;
}

/// `OutputPortDesc::F_*` flags.
pub mod port_flags {
    /// Designated "noisy" port, disabled in Night Mode.
    pub const NOISY: u8 = 0x01;
    pub const GAMMA: u8 = 0x02;
    pub const INVERTED: u8 = 0x04;
    /// Flipper/Chime Logic time limiter enabled.
    pub const FLIPPER_LOGIC: u8 = 0x08;
    /// Computed data source instead of direct host control.
    pub const COMPUTED: u8 = 0x10;
    /// The port has a name (query it with QUERY_LOGICAL_PORT_NAME).
    pub const NAMED: u8 = 0x20;
}

/// `OutputPortDesc::DEV_*` device types (shared by the device list).
pub mod dev_type {
    pub const VIRTUAL: u8 = 1;
    pub const GPIO: u8 = 2;
    pub const TLC59116: u8 = 3;
    pub const TLC5940: u8 = 4;
    pub const PCA9685: u8 = 5;
    pub const PCA9555: u8 = 6;
    pub const HC595: u8 = 7;
    pub const ZBLAUNCH: u8 = 8;
    pub const PWMWORKER: u8 = 9;
    pub const TLC5947: u8 = 10;
    pub const SHAREGROUP: u8 = 11;
}

/// `OutputDevPortDesc::TYPE_*` physical port usage.
pub mod port_type {
    pub const UNUSED: u8 = 0;
    pub const DIGITAL: u8 = 1;
    pub const PWM: u8 = 2;
}

/// `OutputLevelList::F_TEST_MODE`.
pub const LEVELS_F_TEST_MODE: u16 = 0x0001;

/// `OutputLevel::LWSTATE_*` bits.
pub mod lw_state {
    pub const PERIOD_MASK: u8 = 0x07;
    pub const ON: u8 = 0x40;
    pub const MODE: u8 = 0x80;
}

/// Display name for a `DEV_*` code.
pub fn dev_type_name(t: u8) -> &'static str {
    match t {
        dev_type::VIRTUAL => "Virtual",
        dev_type::GPIO => "GPIO",
        dev_type::TLC59116 => "TLC59116",
        dev_type::TLC5940 => "TLC5940",
        dev_type::PCA9685 => "PCA9685",
        dev_type::PCA9555 => "PCA9555",
        dev_type::HC595 => "74HC595",
        dev_type::ZBLAUNCH => "ZB Launch",
        dev_type::PWMWORKER => "PWMWorker",
        dev_type::TLC5947 => "TLC5947",
        dev_type::SHAREGROUP => "Share group",
        _ => "Unknown",
    }
}

/// True for device types addressed over I2C (`addr` = `(bus << 8) | addr7`).
pub fn is_i2c(t: u8) -> bool {
    // PWMWorker too: the firmware fills addr as (bus << 8) | addr (Devices/PWM/PWMWorker.cpp)
    matches!(t, dev_type::TLC59116 | dev_type::PCA9685 | dev_type::PCA9555 | dev_type::PWMWORKER)
}

/// Header sizes (the minimum we need to read the fields we use).
const LIST_HDR_MIN: usize = 6;

/// Validate a list header and return `(data offset, count, element stride)`.
/// `count_at`/`size_at` are the field offsets, which differ between lists.
fn list_layout(xfer: &[u8], count_at: usize, size_at: usize) -> Result<(usize, usize, usize), DecodeError> {
    if xfer.len() < LIST_HDR_MIN {
        return Err(DecodeError::Length { expected: LIST_HDR_MIN, got: xfer.len() });
    }
    let r = Le(xfer);
    let cb = (r.u16(0) as usize).max(LIST_HDR_MIN);
    let n = r.u16(count_at) as usize;
    let stride = r.u16(size_at) as usize;
    let need = cb + n * stride;
    if (n > 0 && stride == 0) || need > xfer.len() {
        return Err(DecodeError::Length { expected: need.max(cb + n), got: xfer.len() });
    }
    Ok((cb, n, stride))
}

/// Iterate over the element slices of a validated list.
fn elements(xfer: &[u8], (cb, n, stride): (usize, usize, usize)) -> impl Iterator<Item = Le<'_>> {
    (0..n).map(move |i| Le(&xfer[cb + i * stride..cb + (i + 1) * stride]))
}

/// One logical output port (`OutputPortDesc`), as DOF sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalPort {
    /// Port number, starting at 1, in list order.
    pub port: u8,
    pub flags: u8,
    /// Config index of the chip, or share-group id.
    pub dev_id: u8,
    pub dev_type: u8,
    pub dev_type_name: &'static str,
    /// GP number for GPIO; chip pin number otherwise.
    pub dev_port: u8,
    pub noisy: bool,
    pub gamma: bool,
    pub inverted: bool,
    pub flipper_logic: bool,
    pub computed: bool,
    pub named: bool,
    /// Filled in by the client from QUERY_LOGICAL_PORT_NAME for named ports.
    pub name: Option<String>,
}

impl LogicalPort {
    pub fn from_raw(port: u8, flags: u8, dev_id: u8, dev_type: u8, dev_port: u8) -> Self {
        let f = |b: u8| flags & b != 0;
        LogicalPort {
            port,
            flags,
            dev_id,
            dev_type,
            dev_type_name: dev_type_name(dev_type),
            dev_port,
            noisy: f(port_flags::NOISY),
            gamma: f(port_flags::GAMMA),
            inverted: f(port_flags::INVERTED),
            flipper_logic: f(port_flags::FLIPPER_LOGIC),
            computed: f(port_flags::COMPUTED),
            named: f(port_flags::NAMED),
            name: None,
        }
    }

    /// Decode the QUERY_LOGICAL_PORTS reply (`OutputPortList` + descs).
    pub fn decode_all(xfer: &[u8]) -> Result<Vec<LogicalPort>, DecodeError> {
        // OutputPortList { cb, cbDesc, numDescs, reserved }
        let layout = list_layout(xfer, 4, 2)?;
        Ok(elements(xfer, layout)
            .enumerate()
            .map(|(i, d)| LogicalPort::from_raw((i + 1) as u8, d.u8(0), d.u8(1), d.u8(2), d.u8(3)))
            .collect())
    }
}

/// Encode a QUERY_LOGICAL_PORTS reply from `(flags, devId, devType, devPort)` tuples.
pub fn encode_logical_ports(ports: &[(u8, u8, u8, u8)]) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + ports.len() * 4);
    for v in [8u16, 4, ports.len() as u16, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &(f, id, t, p) in ports {
        b.extend_from_slice(&[f, id, t, p]);
    }
    b
}

/// The null-terminated name in a QUERY_LOGICAL_PORT_NAME reply.
pub fn decode_port_name(xfer: &[u8]) -> String {
    let end = xfer.iter().position(|&c| c == 0).unwrap_or(xfer.len());
    String::from_utf8_lossy(&xfer[..end]).into_owned()
}

/// One physical output device (`OutputDevDesc`): a chip, chain, or the Pico's GPIOs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    /// `(bus << 8) | addr7` for I2C chips; unused otherwise.
    pub addr: u32,
    /// Total ports (whole chain for daisy chains).
    pub num_ports: u16,
    pub num_ports_per_chip: u16,
    /// Number of PWM steps; levels are `0..pwm_res-1`. Digital reports 2.
    pub pwm_res: u16,
    pub config_index: u8,
    pub dev_type: u8,
    pub dev_type_name: &'static str,
    /// I2C bus number and 7-bit address, for I2C chips.
    pub i2c_bus: Option<u8>,
    pub i2c_addr: Option<u8>,
    /// Per-port usage (`port_type::*`), filled in from QUERY_DEVICE_PORTS.
    pub port_types: Vec<u8>,
    /// Index of this device's first port in the device port/level arrays.
    pub first_port: usize,
}

impl OutputDevice {
    /// Highest native level (`pwm_res - 1`, at least 1).
    pub fn max_level(&self) -> u16 {
        self.pwm_res.saturating_sub(1).max(1)
    }

    /// Decode the QUERY_DEVICES reply (`OutputDevList` + descs).
    pub fn decode_all(xfer: &[u8]) -> Result<Vec<OutputDevice>, DecodeError> {
        // OutputDevList { cb, numDescs, cbDesc, reserved }
        let layout = list_layout(xfer, 2, 4)?;
        let mut first = 0usize;
        Ok(elements(xfer, layout)
            .map(|d| {
                let addr = d.u32(0);
                let dev_type = d.u8(11);
                let i2c = is_i2c(dev_type);
                let dev = OutputDevice {
                    addr,
                    num_ports: d.u16(4),
                    num_ports_per_chip: d.u16(6),
                    pwm_res: d.u16(8),
                    config_index: d.u8(10),
                    dev_type,
                    dev_type_name: dev_type_name(dev_type),
                    i2c_bus: i2c.then_some((addr >> 8) as u8),
                    i2c_addr: i2c.then_some((addr & 0x7F) as u8),
                    port_types: Vec::new(),
                    first_port: first,
                };
                first += dev.num_ports as usize;
                dev
            })
            .collect())
    }

    /// Distribute the flat QUERY_DEVICE_PORTS list over the devices, in order.
    /// Missing entries read as [`port_type::UNUSED`].
    pub fn attach_port_types(devices: &mut [OutputDevice], types: &[u8]) {
        for d in devices {
            d.port_types = (0..d.num_ports as usize)
                .map(|i| types.get(d.first_port + i).copied().unwrap_or(port_type::UNUSED))
                .collect();
        }
    }
}

/// Raw `OutputDevDesc` fields for [`encode_devices`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawDevDesc {
    pub addr: u32,
    pub num_ports: u16,
    pub num_ports_per_chip: u16,
    pub pwm_res: u16,
    pub config_index: u8,
    pub dev_type: u8,
}

/// Encode a QUERY_DEVICES reply.
pub fn encode_devices(devs: &[RawDevDesc]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [8u16, devs.len() as u16, 12, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for d in devs {
        b.extend_from_slice(&d.addr.to_le_bytes());
        b.extend_from_slice(&d.num_ports.to_le_bytes());
        b.extend_from_slice(&d.num_ports_per_chip.to_le_bytes());
        b.extend_from_slice(&d.pwm_res.to_le_bytes());
        b.extend_from_slice(&[d.config_index, d.dev_type]);
    }
    b
}

/// Decode the QUERY_DEVICE_PORTS reply (`OutputDevPortList` + one type byte per port).
pub fn decode_device_port_types(xfer: &[u8]) -> Result<Vec<u8>, DecodeError> {
    // OutputDevPortList { cb, numDescs, cbDesc, reserved }
    let layout = list_layout(xfer, 2, 4)?;
    Ok(elements(xfer, layout).map(|d| d.u8(0)).collect())
}

/// Encode a QUERY_DEVICE_PORTS reply.
pub fn encode_device_port_types(types: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [8u16, types.len() as u16, 1, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(types);
    b
}

/// One logical port's levels (`OutputLevel`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalLevel {
    /// Host-commanded level (DOF / feedback controller), 0..255.
    pub dof_level: u8,
    /// After computed data sources.
    pub calc_level: u8,
    /// Final level sent to the device (after flipper logic, inversion, ...).
    pub out_level: u8,
    pub lw_state: u8,
    pub lw_profile: u8,
    /// Port is in LedWiz mode (expressing its LedWiz state).
    pub lw_mode: bool,
    pub lw_on: bool,
    pub lw_period: u8,
}

impl LogicalLevel {
    pub fn from_raw(dof: u8, calc: u8, out: u8, lw_state: u8, lw_profile: u8) -> Self {
        LogicalLevel {
            dof_level: dof,
            calc_level: calc,
            out_level: out,
            lw_state,
            lw_profile,
            lw_mode: lw_state & lw_state::MODE != 0,
            lw_on: lw_state & lw_state::ON != 0,
            lw_period: lw_state & lw_state::PERIOD_MASK,
        }
    }
}

/// QUERY_LOGICAL_PORT_LEVELS reply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogicalLevels {
    pub test_mode: bool,
    /// One per logical port, in port order (index 0 = port #1).
    pub levels: Vec<LogicalLevel>,
}

impl LogicalLevels {
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        // OutputLevelList { cb, cbLevel, numLevels, flags }
        let layout = list_layout(xfer, 4, 2)?;
        let flags = Le(xfer).u16(6);
        Ok(LogicalLevels {
            test_mode: flags & LEVELS_F_TEST_MODE != 0,
            levels: elements(xfer, layout)
                .map(|l| LogicalLevel::from_raw(l.u8(0), l.u8(1), l.u8(2), l.u8(3), l.u8(4)))
                .collect(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        let flags = if self.test_mode { LEVELS_F_TEST_MODE } else { 0 };
        for v in [8u16, 5, self.levels.len() as u16, flags] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for l in &self.levels {
            b.extend_from_slice(&[l.dof_level, l.calc_level, l.out_level, l.lw_state, l.lw_profile]);
        }
        b
    }
}

/// Decode the QUERY_DEVICE_PORT_LEVELS reply (`OutputDevLevelList` + u16 per port).
pub fn decode_device_levels(xfer: &[u8]) -> Result<Vec<u16>, DecodeError> {
    // OutputDevLevelList { cb, cbLevel, numLevels }
    let layout = list_layout(xfer, 4, 2)?;
    Ok(elements(xfer, layout).map(|l| l.u16(0)).collect())
}

/// Encode a QUERY_DEVICE_PORT_LEVELS reply (6-byte header, as the firmware sends).
pub fn encode_device_levels(levels: &[u16]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [6u16, 2, levels.len() as u16] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for l in levels {
        b.extend_from_slice(&l.to_le_bytes());
    }
    b
}

/// Night Mode status from a QUERY_NIGHT_MODE reply (`args[0]`: 0 off, 1 on).
pub fn decode_night_mode(resp: &Response) -> Result<bool, DecodeError> {
    let a = resp.args_checked(1)?;
    Ok(a[0] == 1)
}

/// SET_PORT arguments: logical port (1-based) and level 0..255.
pub fn set_port_args(port: u8, level: u8) -> [u8; 3] {
    [sub::SET_PORT, port, level]
}

/// SET_NIGHT_MODE arguments.
pub fn set_night_mode_args(on: bool) -> [u8; 2] {
    [sub::SET_NIGHT_MODE, on as u8]
}

/// `Args::OutputTestMode` (8 bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestModeArgs {
    pub enable: bool,
    /// Auto-revert timeout when enabling; 0 = indefinite.
    pub timeout_ms: u32,
}

impl TestModeArgs {
    pub fn encode(&self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0] = sub::TEST_MODE;
        b[1] = self.enable as u8;
        // b[2..4] reserved
        b[4..8].copy_from_slice(&self.timeout_ms.to_le_bytes());
        b
    }

    pub fn decode(a: &[u8]) -> Self {
        let r = Le(a);
        TestModeArgs { enable: r.u8(1) == 1, timeout_ms: r.u32(4) }
    }
}

/// `Args::OutputDevPort` (6 bytes): set a physical port (test mode only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DevicePortArgs {
    pub dev_type: u8,
    pub config_index: u8,
    pub port: u8,
    /// Native device scale (see [`OutputDevice::pwm_res`]); clipped by the firmware.
    pub level: u16,
}

impl DevicePortArgs {
    pub fn encode(&self) -> [u8; 6] {
        let l = self.level.to_le_bytes();
        [sub::SET_DEVICE_PORT, self.dev_type, self.config_index, self.port, l[0], l[1]]
    }

    pub fn decode(a: &[u8]) -> Self {
        let r = Le(a);
        DevicePortArgs { dev_type: r.u8(1), config_index: r.u8(2), port: r.u8(3), level: r.u16(4) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_ports_roundtrip() {
        let b = encode_logical_ports(&[
            (port_flags::NAMED | port_flags::FLIPPER_LOGIC, 0, dev_type::GPIO, 15),
            (port_flags::GAMMA, 1, dev_type::TLC5940, 7),
        ]);
        let p = LogicalPort::decode_all(&b).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].port, 1);
        assert!(p[0].named && p[0].flipper_logic && !p[0].gamma);
        assert_eq!(p[0].dev_type_name, "GPIO");
        assert_eq!(p[0].dev_port, 15);
        assert_eq!(p[1].port, 2);
        assert_eq!((p[1].dev_id, p[1].dev_type, p[1].dev_port), (1, dev_type::TLC5940, 7));
        assert!(p[1].gamma);
    }

    #[test]
    fn logical_ports_tolerate_larger_header_and_desc() {
        // cb = 10 (2 extra header bytes), cbDesc = 6 (2 extra bytes per desc)
        let mut b = vec![10, 0, 6, 0, 2, 0, 0, 0, 0xAA, 0xBB];
        b.extend_from_slice(&[0x01, 3, dev_type::PCA9685, 4, 0xEE, 0xEE]);
        b.extend_from_slice(&[0x04, 0, dev_type::VIRTUAL, 0, 0xEE, 0xEE]);
        let p = LogicalPort::decode_all(&b).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p[0].noisy);
        assert_eq!((p[0].dev_id, p[0].dev_type, p[0].dev_port), (3, dev_type::PCA9685, 4));
        assert!(p[1].inverted);
        assert_eq!(p[1].dev_type_name, "Virtual");
    }

    #[test]
    fn short_desc_reads_zero_for_missing_fields() {
        // cbDesc = 3: devPort missing; must not bleed into the next descriptor
        let mut b = vec![8, 0, 3, 0, 2, 0, 0, 0];
        b.extend_from_slice(&[0, 1, dev_type::GPIO]);
        b.extend_from_slice(&[0, 2, dev_type::GPIO]);
        let p = LogicalPort::decode_all(&b).unwrap();
        assert_eq!(p[0].dev_port, 0);
        assert_eq!(p[1].dev_id, 2);
    }

    #[test]
    fn truncated_list_is_error() {
        let mut b = encode_logical_ports(&[(0, 0, 1, 0), (0, 0, 1, 0)]);
        b.pop();
        assert!(LogicalPort::decode_all(&b).is_err());
        assert!(LogicalPort::decode_all(&[8, 0]).is_err());
        // zero stride with nonzero count
        assert!(LogicalPort::decode_all(&[8, 0, 0, 0, 3, 0, 0, 0]).is_err());
        // empty list is fine
        assert_eq!(LogicalPort::decode_all(&encode_logical_ports(&[])).unwrap(), vec![]);
    }

    #[test]
    fn devices_roundtrip_and_i2c_addr() {
        let raw = [
            RawDevDesc { addr: 0, num_ports: 30, num_ports_per_chip: 30, pwm_res: 256, config_index: 0, dev_type: dev_type::GPIO },
            RawDevDesc { addr: 0x0140, num_ports: 16, num_ports_per_chip: 16, pwm_res: 4096, config_index: 2, dev_type: dev_type::PCA9685 },
        ];
        let mut d = OutputDevice::decode_all(&encode_devices(&raw)).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].dev_type_name, "GPIO");
        assert_eq!(d[0].i2c_addr, None);
        assert_eq!(d[0].max_level(), 255);
        assert_eq!((d[1].i2c_bus, d[1].i2c_addr), (Some(1), Some(0x40)));
        assert_eq!(d[1].config_index, 2);
        assert_eq!(d[1].first_port, 30);
        assert_eq!(d[1].max_level(), 4095);

        let mut types = vec![port_type::UNUSED; 30];
        types[15] = port_type::PWM;
        types.extend(std::iter::repeat_n(port_type::PWM, 15)); // one short
        let t = decode_device_port_types(&encode_device_port_types(&types)).unwrap();
        OutputDevice::attach_port_types(&mut d, &t);
        assert_eq!(d[0].port_types[15], port_type::PWM);
        assert_eq!(d[1].port_types.len(), 16);
        assert_eq!(d[1].port_types[15], port_type::UNUSED);
    }

    #[test]
    fn devices_tolerate_larger_desc() {
        // header cb=8, numDescs=1, cbDesc=16 (4 extra bytes)
        let mut b = vec![8, 0, 1, 0, 16, 0, 0, 0];
        b.extend_from_slice(&0x0060u32.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        b.extend_from_slice(&[1, dev_type::TLC59116, 9, 9, 9, 9]);
        let d = OutputDevice::decode_all(&b).unwrap();
        assert_eq!(d[0].dev_type, dev_type::TLC59116);
        assert_eq!(d[0].i2c_addr, Some(0x60));
        assert_eq!(d[0].pwm_res, 256);
    }

    #[test]
    fn levels_roundtrip() {
        let l = LogicalLevels {
            test_mode: true,
            levels: vec![
                LogicalLevel::from_raw(255, 200, 100, lw_state::MODE | lw_state::ON | 3, 48),
                LogicalLevel::from_raw(0, 0, 255, 0, 0),
            ],
        };
        let d = LogicalLevels::decode(&l.encode()).unwrap();
        assert_eq!(d, l);
        assert!(d.levels[0].lw_mode && d.levels[0].lw_on);
        assert_eq!(d.levels[0].lw_period, 3);

        let dl = decode_device_levels(&encode_device_levels(&[0, 1, 4095])).unwrap();
        assert_eq!(dl, vec![0, 1, 4095]);
    }

    #[test]
    fn levels_tolerate_larger_level_struct() {
        // cb=8, cbLevel=7, n=1, flags=0
        let b = [8, 0, 7, 0, 1, 0, 0, 0, 10, 20, 30, 0, 0, 0xFF, 0xFF];
        let d = LogicalLevels::decode(&b).unwrap();
        assert!(!d.test_mode);
        assert_eq!((d.levels[0].dof_level, d.levels[0].calc_level, d.levels[0].out_level), (10, 20, 30));
        // device levels: cb=8 (padded header), cbLevel=4
        let b = [8, 0, 4, 0, 2, 0, 0, 0, 0x34, 0x12, 0, 0, 1, 0, 0, 0];
        assert_eq!(decode_device_levels(&b).unwrap(), vec![0x1234, 1]);
    }

    #[test]
    fn arg_layouts() {
        assert_eq!(set_port_args(3, 200), [0x01, 3, 200]);
        assert_eq!(set_night_mode_args(true), [0x04, 1]);
        let t = TestModeArgs { enable: true, timeout_ms: 3000 };
        assert_eq!(t.encode(), [0x02, 1, 0, 0, 0xB8, 0x0B, 0, 0]);
        assert_eq!(TestModeArgs::decode(&t.encode()), t);
        let d = DevicePortArgs { dev_type: dev_type::TLC5940, config_index: 1, port: 17, level: 4095 };
        assert_eq!(d.encode(), [0x03, 4, 1, 17, 0xFF, 0x0F]);
        assert_eq!(DevicePortArgs::decode(&d.encode()), d);
    }

    #[test]
    fn port_name_and_night_mode() {
        assert_eq!(decode_port_name(b"Knocker\0junk"), "Knocker");
        assert_eq!(decode_port_name(b"\0"), "");
        assert_eq!(decode_port_name(b""), "");
        let mut r = Response { token: 0, cmd: 0x11, args_size: 1, status: 0, xfer_bytes: 0, args: [0; 16] };
        r.args[0] = 1;
        assert!(decode_night_mode(&r).unwrap());
        r.args_size = 0;
        assert!(decode_night_mode(&r).is_err());
    }
}
