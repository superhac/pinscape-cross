//! Wire-format definitions for the Pinscape Pico USB **vendor interface**.
//!
//! This crate is a pure, I/O-free Rust rendering of the upstream protocol
//! specification `PinscapePico/USBProtocol/VendorIfcProtocol.h`. All
//! multi-byte fields are little-endian and structs are packed.
//!
//! Every request is a fixed 28-byte [`Request`] written to the bulk OUT
//! endpoint, optionally followed by `xfer_bytes` of extra data. The device
//! answers each request with a fixed 28-byte [`Response`] on the bulk IN
//! endpoint, optionally followed by `xfer_bytes` of extra data.

use serde::Serialize;

/// Size in bytes of the fixed-length argument union in requests and responses.
pub const ARGS_LEN: usize = 16;
/// Encoded size of a [`Request`] on the wire.
pub const REQUEST_LEN: usize = 28;
/// Encoded size of a [`Response`] on the wire.
pub const RESPONSE_LEN: usize = 28;

/// USB interface string the firmware assigns to the vendor interface.
pub const VENDOR_INTERFACE_STRING: &str = "PinscapePicoControl";
/// Windows WinUSB device interface GUID published via MS OS 2.0 descriptors.
pub const WINUSB_GUID: &str = "{D3057FB3-8F4C-4AF9-9440-B220C3B2BA23}";
/// Default USB vendor ID (pid.codes open-source VID).
pub const DEFAULT_VID: u16 = 0x1209;
/// Default USB product IDs used by Pinscape Pico builds.
pub const DEFAULT_PIDS: [u16; 2] = [0xEAEA, 0xEAEB];

/// Command codes (`VendorRequest::CMD_*`).
pub mod cmd {
    pub const NULL: u8 = 0x00;
    pub const QUERY_VERSION: u8 = 0x01;
    pub const QUERY_IDS: u8 = 0x02;
    pub const QUERY_USBIFCS: u8 = 0x03;
    pub const RESET: u8 = 0x04;
    pub const SET_CLOCK: u8 = 0x05;
    pub const CONFIG: u8 = 0x06;
    pub const NUDGE: u8 = 0x07;
    pub const TVON: u8 = 0x0A;
    pub const STATS: u8 = 0x0B;
    pub const QUERY_LOG: u8 = 0x0C;
    pub const SEND_IR: u8 = 0x0D;
    pub const FLASH_STORAGE: u8 = 0x0E;
    pub const PLUNGER: u8 = 0x0F;
    pub const BUTTONS: u8 = 0x10;
    pub const OUTPUTS: u8 = 0x11;
    pub const PING: u8 = 0x12;
    pub const QUERY_GPIO_CONFIG: u8 = 0x13;
    pub const QUERY_IR: u8 = 0x14;
    pub const SYNC_CLOCKS: u8 = 0x15;
    pub const DEBUG: u8 = 0x16;
}

/// Subcommands for [`cmd::RESET`].
pub mod reset {
    pub const NORMAL: u8 = 0x01;
    pub const SAFE_MODE: u8 = 0x02;
    pub const BOOTLOADER: u8 = 0x03;
    pub const FACTORY: u8 = 0x04;
}

/// Subcommands and file IDs for [`cmd::CONFIG`].
pub mod config {
    pub const GET: u8 = 0x01;
    pub const PUT: u8 = 0x02;
    pub const EXISTS: u8 = 0x03;
    pub const ERASE: u8 = 0x04;
    pub const TEST_CHECKSUM: u8 = 0x05;
    pub const RESET: u8 = 0x06;

    pub const FILE_MAIN: u8 = 0x00;
    pub const FILE_SAFE_MODE: u8 = 0x01;
    pub const FILE_ALL: u8 = 0xFF;

    /// Config files are transferred in pages of this size.
    pub const PAGE_SIZE: usize = 4096;
}

/// Response status codes (`VendorResponse::OK`, `ERR_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[repr(u16)]
pub enum Status {
    Ok = 0,
    Failed = 1,
    Timeout = 2,
    BadXferLen = 3,
    UsbXferFailed = 4,
    BadParams = 5,
    BadCmd = 6,
    BadSubcmd = 7,
    ReplyMismatch = 8,
    ConfigTimeout = 9,
    ConfigInvalid = 10,
    OutOfBounds = 11,
    NotReady = 12,
    Eof = 13,
    BadRequestData = 14,
    BadReplyData = 15,
    NotFound = 16,
    RetryOk = 17,
}

impl Status {
    pub fn from_u16(v: u16) -> Option<Status> {
        use Status::*;
        Some(match v {
            0 => Ok,
            1 => Failed,
            2 => Timeout,
            3 => BadXferLen,
            4 => UsbXferFailed,
            5 => BadParams,
            6 => BadCmd,
            7 => BadSubcmd,
            8 => ReplyMismatch,
            9 => ConfigTimeout,
            10 => ConfigInvalid,
            11 => OutOfBounds,
            12 => NotReady,
            13 => Eof,
            14 => BadRequestData,
            15 => BadReplyData,
            16 => NotFound,
            17 => RetryOk,
            _ => return None,
        })
    }

    pub fn description(self) -> &'static str {
        use Status::*;
        match self {
            Ok => "success",
            Failed => "request failed",
            Timeout => "timed out",
            BadXferLen => "bad transfer length",
            UsbXferFailed => "USB transfer failed",
            BadParams => "invalid parameters",
            BadCmd => "unknown command",
            BadSubcmd => "unknown subcommand",
            ReplyMismatch => "reply did not match request",
            ConfigTimeout => "configuration transfer timed out",
            ConfigInvalid => "configuration data invalid",
            OutOfBounds => "value out of bounds",
            NotReady => "device not ready",
            Eof => "end of file",
            BadRequestData => "bad request data",
            BadReplyData => "bad reply data",
            NotFound => "not found",
            RetryOk => "retry is OK",
        }
    }
}

/// A vendor-interface request (host to device).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub token: u32,
    pub cmd: u8,
    /// Number of meaningful bytes in `args`.
    pub args_size: u8,
    /// Length of the extra OUT data packet that follows the request.
    pub xfer_bytes: u16,
    pub args: [u8; ARGS_LEN],
}

impl Request {
    /// A request with no arguments and no extra data.
    pub fn new(token: u32, cmd: u8) -> Self {
        Request { token, cmd, args_size: 0, xfer_bytes: 0, args: [0; ARGS_LEN] }
    }

    /// A request with argument bytes (at most [`ARGS_LEN`]).
    pub fn with_args(token: u32, cmd: u8, args: &[u8]) -> Self {
        assert!(args.len() <= ARGS_LEN, "args too long");
        let mut r = Request::new(token, cmd);
        r.args[..args.len()].copy_from_slice(args);
        r.args_size = args.len() as u8;
        r
    }

    /// `~(token + cmd + xferBytes)`, wrapping, as specified by the protocol.
    pub fn checksum(&self) -> u32 {
        !(self
            .token
            .wrapping_add(self.cmd as u32)
            .wrapping_add(self.xfer_bytes as u32))
    }

    pub fn encode(&self) -> [u8; REQUEST_LEN] {
        let mut b = [0u8; REQUEST_LEN];
        b[0..4].copy_from_slice(&self.token.to_le_bytes());
        b[4..8].copy_from_slice(&self.checksum().to_le_bytes());
        b[8] = self.cmd;
        b[9] = self.args_size;
        b[10..12].copy_from_slice(&self.xfer_bytes.to_le_bytes());
        b[12..28].copy_from_slice(&self.args);
        b
    }
}

/// A vendor-interface response (device to host).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub token: u32,
    pub cmd: u8,
    pub args_size: u8,
    pub status: u16,
    /// Length of the extra IN data packet that follows the response.
    pub xfer_bytes: u16,
    pub args: [u8; ARGS_LEN],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("expected {expected} bytes, got {got}")]
    Length { expected: usize, got: usize },
    #[error("reply arguments too short: need {need} bytes, got {got}")]
    ArgsTooShort { need: usize, got: usize },
}

impl Response {
    pub fn decode(b: &[u8]) -> Result<Self, DecodeError> {
        if b.len() != RESPONSE_LEN {
            return Err(DecodeError::Length { expected: RESPONSE_LEN, got: b.len() });
        }
        let mut args = [0u8; ARGS_LEN];
        args.copy_from_slice(&b[12..28]);
        Ok(Response {
            token: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            cmd: b[4],
            args_size: b[5],
            status: u16::from_le_bytes(b[6..8].try_into().unwrap()),
            xfer_bytes: u16::from_le_bytes(b[8..10].try_into().unwrap()),
            args,
        })
    }

    pub fn status(&self) -> Option<Status> {
        Status::from_u16(self.status)
    }

    /// The meaningful portion of the argument union, after checking it's at least `need` bytes.
    pub fn args_checked(&self, need: usize) -> Result<&[u8], DecodeError> {
        let got = (self.args_size as usize).min(ARGS_LEN);
        if got < need {
            return Err(DecodeError::ArgsTooShort { need, got });
        }
        Ok(&self.args[..got])
    }
}

// ---------------------------------------------------------------------------
// Typed reply payloads
// ---------------------------------------------------------------------------

/// Reply to [`cmd::QUERY_VERSION`] (`args.version`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    /// Build timestamp, `YYYYMMDDhhmm`.
    pub build_date: String,
}

impl Version {
    pub fn decode(resp: &Response) -> Result<Self, DecodeError> {
        let a = resp.args_checked(15)?;
        Ok(Version {
            major: a[0],
            minor: a[1],
            patch: a[2],
            build_date: String::from_utf8_lossy(&a[3..15]).trim_end_matches('\0').to_string(),
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{} (build {})", self.major, self.minor, self.patch, self.build_date)
    }
}

/// Reply to [`cmd::QUERY_IDS`] (`args.id` plus the null-separated string block).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceIds {
    /// The Pico's permanent, unique 64-bit hardware ID, as 16 hex digits.
    pub hwid: String,
    /// 2040 for RP2040, 2350 for RP2350.
    pub cpu_type: u16,
    pub cpu_version: u8,
    pub rom_version: u8,
    pub unit_num: u8,
    /// XInput player index, `None` if unknown/inactive.
    pub xinput_player_index: Option<u8>,
    pub led_wiz_unit_mask: u16,
    pub unit_name: String,
    pub target_board: String,
    pub pico_sdk_version: String,
    pub tinyusb_version: String,
    pub compiler_version: String,
}

impl DeviceIds {
    pub fn decode(resp: &Response, xfer: &[u8]) -> Result<Self, DecodeError> {
        let a = resp.args_checked(16)?;
        let hwid = a[0..8].iter().map(|b| format!("{b:02X}")).collect::<String>();
        let mut strings = xfer
            .split(|&c| c == 0)
            .map(|s| String::from_utf8_lossy(s).into_owned());
        let mut next = || strings.next().unwrap_or_default();
        Ok(DeviceIds {
            hwid,
            cpu_type: u16::from_le_bytes([a[8], a[9]]),
            cpu_version: a[10],
            rom_version: a[11],
            unit_num: a[12],
            xinput_player_index: (a[13] != 0xFF).then_some(a[13]),
            led_wiz_unit_mask: u16::from_le_bytes([a[14], a[15]]),
            unit_name: next(),
            target_board: next(),
            pico_sdk_version: next(),
            tinyusb_version: next(),
            compiler_version: next(),
        })
    }

    pub fn cpu_name(&self) -> String {
        format!("RP{}", self.cpu_type)
    }
}

/// Arguments for [`cmd::CONFIG`] requests (`args.config`, 12 bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigArgs {
    pub subcmd: u8,
    pub file_id: u8,
    pub page: u16,
    pub n_pages: u16,
    pub crc: u32,
}

impl ConfigArgs {
    pub fn encode(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0] = self.subcmd;
        b[1] = self.file_id;
        b[2..4].copy_from_slice(&self.page.to_le_bytes());
        b[4..6].copy_from_slice(&self.n_pages.to_le_bytes());
        // b[6..8] reserved
        b[8..12].copy_from_slice(&self.crc.to_le_bytes());
        b
    }
}

/// Subcommands for [`cmd::STATS`].
pub mod stats {
    pub const QUERY_STATS: u8 = 0x01;
    pub const QUERY_CLOCK: u8 = 0x02;
    pub const PREP_QUERY_CLOCK: u8 = 0x03;
    pub const FLAG_RESET_COUNTERS: u8 = 0x01;
}

/// Little-endian field reader over a reply buffer that tolerates short
/// (older-version) structs by returning zero for missing fields.
struct Le<'a>(&'a [u8]);

impl Le<'_> {
    fn u8(&self, o: usize) -> u8 {
        self.0.get(o).copied().unwrap_or(0)
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.u8(o), self.u8(o + 1)])
    }
    fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.u8(o), self.u8(o + 1), self.u8(o + 2), self.u8(o + 3)])
    }
    fn u64(&self, o: usize) -> u64 {
        (self.u32(o) as u64) | ((self.u32(o + 4) as u64) << 32)
    }
}

/// `Statistics` struct returned by [`cmd::STATS`] / [`stats::QUERY_STATS`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub up_time_us: u64,
    pub n_loops: u64,
    pub n_loops_ever: u64,
    pub avg_loop_time_us: u32,
    pub max_loop_time_us: u32,
    pub n_loops2: u64,
    pub n_loops_ever2: u64,
    pub avg_loop_time2_us: u32,
    pub max_loop_time2_us: u32,
    pub heap_size: u32,
    pub heap_unused: u32,
    pub arena_size: u32,
    pub arena_alloc: u32,
    pub arena_free: u32,
}

impl Statistics {
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < 40 {
            return Err(DecodeError::Length { expected: 40, got: xfer.len() });
        }
        let r = Le(xfer);
        Ok(Statistics {
            up_time_us: r.u64(8),
            n_loops: r.u64(16),
            n_loops_ever: r.u64(24),
            avg_loop_time_us: r.u32(32),
            max_loop_time_us: r.u32(36),
            n_loops2: r.u64(40),
            n_loops_ever2: r.u64(48),
            avg_loop_time2_us: r.u32(56),
            max_loop_time2_us: r.u32(60),
            heap_size: r.u32(64),
            heap_unused: r.u32(68),
            arena_size: r.u32(72),
            arena_alloc: r.u32(76),
            arena_free: r.u32(80),
        })
    }
}

/// `USBInterfaces` struct returned by [`cmd::QUERY_USBIFCS`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbInterfaces {
    pub vid: u16,
    pub pid: u16,
    pub flags: u16,
    pub keyboard_configured: bool,
    pub keyboard_enabled: bool,
    pub gamepad_configured: bool,
    pub gamepad_enabled: bool,
    pub xinput_configured: bool,
    pub xinput_enabled: bool,
    pub pindev_configured: bool,
    pub pindev_enabled: bool,
    pub feedback_configured: bool,
    pub ledwiz_configured: bool,
    pub cdc_configured: bool,
}

impl UsbInterfaces {
    pub fn decode(xfer: &[u8]) -> Result<Self, DecodeError> {
        if xfer.len() < 8 {
            return Err(DecodeError::Length { expected: 8, got: xfer.len() });
        }
        let r = Le(xfer);
        let flags = r.u16(6);
        let f = |bit: u16| flags & bit != 0;
        Ok(UsbInterfaces {
            vid: r.u16(2),
            pid: r.u16(4),
            flags,
            keyboard_configured: f(0x0001),
            keyboard_enabled: f(0x0002),
            gamepad_configured: f(0x0004),
            gamepad_enabled: f(0x0008),
            xinput_configured: f(0x0010),
            xinput_enabled: f(0x0020),
            pindev_configured: f(0x0040),
            pindev_enabled: f(0x0080),
            feedback_configured: f(0x0100),
            ledwiz_configured: f(0x0200),
            cdc_configured: f(0x1000),
        })
    }
}

/// One GPIO pin's assignment, from [`cmd::QUERY_GPIO_CONFIG`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpioPort {
    pub gpio: u8,
    /// RP2040 function selector (0 XIP, 1 SPI, 2 UART, 3 I2C, 4 PWM, 5 SIO, 6 PIO0, 7 PIO1, 8 GPCK, 9 USB, 0x1F none).
    pub func: u8,
    pub func_name: &'static str,
    pub output: bool,
    pub adc: bool,
    /// Pinscape's description of the pin's use, e.g. "Button #3".
    pub usage: Option<String>,
}

impl GpioPort {
    pub fn func_name(func: u8) -> &'static str {
        match func {
            0 => "XIP",
            1 => "SPI",
            2 => "UART",
            3 => "I2C",
            4 => "PWM",
            5 => "SIO",
            6 => "PIO0",
            7 => "PIO1",
            8 => "GPCK",
            9 => "USB",
            0x1F => "NULL",
            _ => "?",
        }
    }

    /// Decode the `GPIOConfig` struct (header + port array + string pool).
    pub fn decode_all(xfer: &[u8]) -> Result<Vec<GpioPort>, DecodeError> {
        if xfer.len() < 6 {
            return Err(DecodeError::Length { expected: 6, got: xfer.len() });
        }
        let r = Le(xfer);
        let cb_port = r.u16(2) as usize;
        let n = r.u16(4) as usize;
        let base = 6usize;
        if cb_port < 4 || base + n * cb_port > xfer.len() {
            return Err(DecodeError::Length { expected: base + n * cb_port, got: xfer.len() });
        }
        let cstr = |ofs: usize| -> Option<String> {
            if ofs == 0 || ofs >= xfer.len() {
                return None;
            }
            let s = &xfer[ofs..];
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            Some(String::from_utf8_lossy(&s[..end]).into_owned())
        };
        Ok((0..n)
            .map(|i| {
                let o = base + i * cb_port;
                let func = r.u8(o);
                let flags = r.u8(o + 1);
                GpioPort {
                    gpio: i as u8,
                    func,
                    func_name: Self::func_name(func),
                    output: flags & 0x01 != 0,
                    adc: flags & 0x02 != 0,
                    usage: cstr(r.u16(o + 2) as usize),
                }
            })
            .collect())
    }
}

/// Arguments for [`cmd::SET_CLOCK`] (`args.clock`, 7 bytes), local wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockArgs {
    pub year: i16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl ClockArgs {
    pub fn encode(&self) -> [u8; 7] {
        let y = self.year.to_le_bytes();
        [y[0], y[1], self.month, self.day, self.hour, self.minute, self.second]
    }
}

/// `args.log.avail` in a [`cmd::QUERY_LOG`] reply: bytes still available on the device.
pub fn decode_log_avail(resp: &Response) -> u32 {
    resp.args_checked(4).map(|a| Le(a).u32(0)).unwrap_or(0)
}

/// Standard CRC-32 (IEEE 802.3), as used by the config PUT protocol.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn clock_args_layout() {
        let c = ClockArgs { year: 2026, month: 11, day: 8, hour: 7, minute: 5, second: 9 };
        assert_eq!(c.encode(), [0xEA, 0x07, 11, 8, 7, 5, 9]);
    }

    #[test]
    fn gpio_decode() {
        // header: cb=6, cbPort=4, numPorts=2; port0 SIO out "Out", port1 NULL
        let mut b = vec![6, 0, 4, 0, 2, 0];
        b.extend_from_slice(&[5, 0x01, 14, 0]);
        b.extend_from_slice(&[0x1F, 0, 0, 0]);
        b.extend_from_slice(b"Out\0");
        let p = GpioPort::decode_all(&b).unwrap();
        assert_eq!(p[0].func_name, "SIO");
        assert!(p[0].output);
        assert_eq!(p[0].usage.as_deref(), Some("Out"));
        assert_eq!(p[1].usage, None);
    }

    #[test]
    fn request_layout_and_checksum() {
        let mut r = Request::new(0x0102_0304, cmd::QUERY_IDS);
        r.xfer_bytes = 0x10;
        let b = r.encode();
        assert_eq!(b.len(), 28);
        assert_eq!(&b[0..4], &[0x04, 0x03, 0x02, 0x01]);
        let expect = !(0x0102_0304u32 + 2 + 0x10);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), expect);
        assert_eq!(b[8], cmd::QUERY_IDS);
        assert_eq!(b[9], 0);
        assert_eq!(&b[10..12], &[0x10, 0x00]);
    }

    #[test]
    fn checksum_wraps() {
        let r = Request::new(u32::MAX, 0x05);
        assert_eq!(r.checksum(), !(4u32));
    }

    #[test]
    fn response_decode() {
        let mut b = [0u8; 28];
        b[0..4].copy_from_slice(&7u32.to_le_bytes());
        b[4] = cmd::QUERY_VERSION;
        b[5] = 15;
        b[6..8].copy_from_slice(&0u16.to_le_bytes());
        b[8..10].copy_from_slice(&0u16.to_le_bytes());
        b[12] = 1;
        b[13] = 0;
        b[14] = 2;
        b[15..27].copy_from_slice(b"202605011342");
        let r = Response::decode(&b).unwrap();
        assert_eq!(r.status(), Some(Status::Ok));
        let v = Version::decode(&r).unwrap();
        assert_eq!(v.to_string(), "1.0.2 (build 202605011342)");
    }

    #[test]
    fn ids_decode() {
        let mut b = [0u8; 28];
        b[5] = 16;
        b[12..20].copy_from_slice(&[0xE6, 0x64, 0x80, 0x45, 0x4F, 0x0A, 0x58, 0x34]);
        b[20..22].copy_from_slice(&2040u16.to_le_bytes());
        b[22] = 2;
        b[23] = 3;
        b[24] = 1;
        b[25] = 0xFF;
        b[26..28].copy_from_slice(&1u16.to_le_bytes());
        let r = Response::decode(&b).unwrap();
        let ids = DeviceIds::decode(&r, b"Main Pico\0pico\x002.2.0\x000.18.0\0GNUC 14.3.1\0").unwrap();
        assert_eq!(ids.hwid, "E66480454F0A5834");
        assert_eq!(ids.cpu_name(), "RP2040");
        assert_eq!(ids.unit_name, "Main Pico");
        assert_eq!(ids.compiler_version, "GNUC 14.3.1");
        assert_eq!(ids.xinput_player_index, None);
    }

    #[test]
    fn config_args_layout() {
        let a = ConfigArgs { subcmd: config::PUT, file_id: 0, page: 2, n_pages: 16, crc: 0xAABBCCDD };
        let b = a.encode();
        assert_eq!(b, [2, 0, 2, 0, 16, 0, 0, 0, 0xDD, 0xCC, 0xBB, 0xAA]);
    }
}
