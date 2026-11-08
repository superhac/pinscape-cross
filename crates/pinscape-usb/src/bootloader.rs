//! RP2040 / RP2350 boot ROM access, for installing firmware.
//!
//! Two ways to get a UF2 image onto a Pico in boot loader (BOOTSEL) mode:
//!
//! 1. **PICOBOOT** (primary): the boot ROM's vendor-class USB interface. We
//!    erase and program exactly the flash sectors the image covers (see
//!    [`pinscape_protocol::uf2::FlashPlan`]), optionally read back to
//!    verify, and reboot. Works without a mounted drive and reports real
//!    progress. Protocol per pico-sdk `boot/picoboot.h` and picotool's
//!    `picoboot_connection.c`.
//! 2. **Mass storage** (fallback): copy the .uf2 file onto the boot ROM's
//!    virtual drive (`RPI-RP2` / `RP2350`, recognized by its `INFO_UF2.TXT`).
//!    Needed on Windows for the RP2040, whose boot ROM has no MS OS
//!    descriptors, so WinUSB can't open PICOBOOT without a manual driver install.
//!
//! The PICOBOOT client runs over [`PicobootTransport`], so the protocol and
//! the flashing sequence are unit-tested against [`sim::SimPicoboot`], a
//! simulated boot ROM with a flash array.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, Direction, In, Out, Recipient};
use nusb::{DeviceInfo, MaybeFuture};
use pinscape_protocol::uf2::{Chip, FlashPlan, Uf2Error, SECTOR_SIZE};
use serde::Serialize;

use crate::{check_completion, map_open_err, Error, RPI_VID};

pub const RP2040_BOOT_PID: u16 = 0x0003;
pub const RP2350_BOOT_PID: u16 = 0x000F;

/// PICOBOOT wire constants (pico-sdk `boot/picoboot.h`).
pub mod picoboot {
    pub const MAGIC: u32 = 0x431F_D10B;
    pub const CMD_LEN: usize = 32;
    pub const STATUS_LEN: usize = 16;

    /// Vendor control requests on the PICOBOOT interface.
    pub const IF_RESET: u8 = 0x41;
    pub const IF_CMD_STATUS: u8 = 0x42;

    // Command IDs; the top bit marks an IN (device-to-host) data phase.
    pub const EXCLUSIVE_ACCESS: u8 = 0x01;
    /// RP2040 only.
    pub const REBOOT: u8 = 0x02;
    pub const FLASH_ERASE: u8 = 0x03;
    pub const READ: u8 = 0x84;
    pub const WRITE: u8 = 0x05;
    pub const EXIT_XIP: u8 = 0x06;
    pub const ENTER_CMD_XIP: u8 = 0x07;
    /// RP2350 only.
    pub const REBOOT2: u8 = 0x0A;
    /// RP2350 only.
    pub const GET_INFO: u8 = 0x8B;

    // EXCLUSIVE_ACCESS argument
    pub const NOT_EXCLUSIVE: u8 = 0;
    /// Exclusive: the boot ROM stops accepting mass-storage writes meanwhile.
    pub const EXCLUSIVE: u8 = 1;
    pub const EXCLUSIVE_AND_EJECT: u8 = 2;

    /// REBOOT2 flag: normal boot (from flash).
    pub const REBOOT2_FLAG_REBOOT_TYPE_NORMAL: u32 = 0x0;
    /// RP2040 REBOOT stack pointer for a normal boot (SRAM_END), as picotool uses.
    pub const RP2040_SRAM_END: u32 = 0x2004_2000;

    pub fn status_name(code: u32) -> &'static str {
        match code {
            0 => "OK",
            1 => "unknown command",
            2 => "invalid command length",
            3 => "invalid transfer length",
            4 => "invalid address",
            5 => "bad alignment",
            6 => "interleaved write",
            7 => "rebooting",
            8 => "unknown error",
            9 => "invalid state",
            10 => "not permitted",
            11 => "invalid argument",
            12 => "buffer too small",
            13 => "precondition not met",
            14 => "modified data",
            15 => "invalid data",
            16 => "not found",
            17 => "unsupported modification",
            _ => "unrecognized status",
        }
    }
}

/// `struct picoboot_cmd` (32 bytes, little endian).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub token: u32,
    pub id: u8,
    /// Argument bytes (`bCmdSize` of them, at most 16).
    pub args: Vec<u8>,
    pub transfer_len: u32,
}

impl Command {
    pub fn encode(&self) -> [u8; picoboot::CMD_LEN] {
        let mut b = [0u8; picoboot::CMD_LEN];
        b[0..4].copy_from_slice(&picoboot::MAGIC.to_le_bytes());
        b[4..8].copy_from_slice(&self.token.to_le_bytes());
        b[8] = self.id;
        b[9] = self.args.len() as u8;
        b[12..16].copy_from_slice(&self.transfer_len.to_le_bytes());
        b[16..16 + self.args.len()].copy_from_slice(&self.args);
        b
    }

    pub fn decode(b: &[u8]) -> Option<Command> {
        if b.len() != picoboot::CMD_LEN || u32::from_le_bytes(b[0..4].try_into().ok()?) != picoboot::MAGIC {
            return None;
        }
        let size = b[9] as usize;
        if size > 16 {
            return None;
        }
        Some(Command {
            token: u32::from_le_bytes(b[4..8].try_into().ok()?),
            id: b[8],
            args: b[16..16 + size].to_vec(),
            transfer_len: u32::from_le_bytes(b[12..16].try_into().ok()?),
        })
    }

    pub fn is_in(&self) -> bool {
        self.id & 0x80 != 0
    }
}

fn range_args(addr: u32, len: u32) -> Vec<u8> {
    let mut v = addr.to_le_bytes().to_vec();
    v.extend_from_slice(&len.to_le_bytes());
    v
}

/// `struct picoboot_cmd_status` (16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmdStatus {
    pub token: u32,
    pub status: u32,
    pub cmd_id: u8,
    pub in_progress: bool,
}

impl CmdStatus {
    pub fn decode(b: &[u8]) -> Option<CmdStatus> {
        if b.len() < picoboot::STATUS_LEN {
            return None;
        }
        Some(CmdStatus {
            token: u32::from_le_bytes(b[0..4].try_into().ok()?),
            status: u32::from_le_bytes(b[4..8].try_into().ok()?),
            cmd_id: b[8],
            in_progress: b[9] != 0,
        })
    }

    pub fn encode(&self) -> [u8; picoboot::STATUS_LEN] {
        let mut b = [0u8; picoboot::STATUS_LEN];
        b[0..4].copy_from_slice(&self.token.to_le_bytes());
        b[4..8].copy_from_slice(&self.status.to_le_bytes());
        b[8] = self.cmd_id;
        b[9] = self.in_progress as u8;
        b
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BootError {
    #[error(transparent)]
    Usb(#[from] Error),
    #[error("boot loader rejected command {cmd:#04x}: {} (status {status})", picoboot::status_name(*.status))]
    Picoboot { cmd: u8, status: u32 },
    #[error("invalid firmware image: {0}")]
    Image(#[from] Uf2Error),
    #[error("verification failed: flash at {addr:#010x} doesn't match the image")]
    Verify { addr: u32 },
    #[error("this boot loader doesn't offer the PICOBOOT interface")]
    NoPicobootInterface,
    #[error("no Pico in boot loader mode found")]
    NotFound,
    #[error("{0}")]
    Io(String),
}

impl BootError {
    pub fn is_permission(&self) -> bool {
        matches!(self, BootError::Usb(Error::PermissionDenied))
    }
}

pub type BootResult<T> = std::result::Result<T, BootError>;

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// A Pico in boot loader mode (not yet opened).
#[derive(Clone)]
pub struct BootDevice {
    pub info: DeviceInfo,
    pub chip: Chip,
    /// The PICOBOOT (vendor-class) interface number, if the boot ROM exposes one.
    pub interface: Option<u8>,
}

/// Serializable summary of a boot-loader device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootDeviceSummary {
    /// Stable for as long as the device stays attached.
    pub key: String,
    pub chip: Chip,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub picoboot: bool,
}

impl BootDevice {
    pub fn key(&self) -> String {
        format!("boot:{:?}", self.info.id())
    }

    pub fn summary(&self) -> BootDeviceSummary {
        BootDeviceSummary {
            key: self.key(),
            chip: self.chip,
            vid: self.info.vendor_id(),
            pid: self.info.product_id(),
            serial: self.info.serial_number().map(str::to_owned),
            picoboot: self.interface.is_some(),
        }
    }

    /// Open the PICOBOOT interface (resets it and takes exclusive access).
    pub fn open(&self) -> BootResult<Picoboot> {
        let ifc = self.interface.ok_or(BootError::NoPicobootInterface)?;
        let t = UsbPicoboot::open(&self.info, ifc)?;
        Picoboot::connect(Box::new(t), self.chip)
    }
}

/// Which chip a boot ROM USB ID belongs to.
pub fn boot_chip(vid: u16, pid: u16) -> Option<Chip> {
    match (vid, pid) {
        (RPI_VID, RP2040_BOOT_PID) => Some(Chip::Rp2040),
        (RPI_VID, RP2350_BOOT_PID) => Some(Chip::Rp2350),
        _ => None,
    }
}

/// Enumerate Picos in boot loader mode. Only reads the bus's descriptor
/// cache; no device is opened.
pub fn find_boot_devices() -> crate::Result<Vec<BootDevice>> {
    let mut out = Vec::new();
    for info in nusb::list_devices().wait()? {
        let Some(chip) = boot_chip(info.vendor_id(), info.product_id()) else { continue };
        let interface = info.interfaces().find(|i| i.class() == 0xFF).map(|i| i.interface_number());
        out.push(BootDevice { info, chip, interface });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Byte-level link to a PICOBOOT interface: real USB, or [`sim::SimPicoboot`].
pub trait PicobootTransport: Send {
    fn bulk_out(&mut self, data: &[u8], timeout: Duration) -> crate::Result<()>;
    /// Read one IN transfer of up to `len` bytes.
    fn bulk_in(&mut self, len: usize, timeout: Duration) -> crate::Result<Vec<u8>>;
    /// `PICOBOOT_IF_RESET`: clear halts and reset the command state machine.
    fn interface_reset(&mut self) -> crate::Result<()>;
    /// `PICOBOOT_IF_CMD_STATUS`: status of the last command.
    fn cmd_status(&mut self) -> crate::Result<[u8; picoboot::STATUS_LEN]>;
}

struct UsbPicoboot {
    interface: nusb::Interface,
    ep_out: nusb::Endpoint<Bulk, Out>,
    ep_in: nusb::Endpoint<Bulk, In>,
}

impl UsbPicoboot {
    fn open(info: &DeviceInfo, ifc: u8) -> crate::Result<UsbPicoboot> {
        let device = info.open().wait().map_err(map_open_err)?;
        let interface = device.claim_interface(ifc).wait().map_err(map_open_err)?;
        let (mut out_addr, mut in_addr) = (None, None);
        if let Some(desc) = interface.descriptor() {
            for ep in desc.endpoints() {
                if ep.transfer_type() != TransferType::Bulk {
                    continue;
                }
                match ep.direction() {
                    Direction::Out => out_addr = out_addr.or(Some(ep.address())),
                    Direction::In => in_addr = in_addr.or(Some(ep.address())),
                }
            }
        }
        let (Some(o), Some(i)) = (out_addr, in_addr) else { return Err(Error::NoEndpoints) };
        let ep_out = interface.endpoint::<Bulk, Out>(o)?;
        let ep_in = interface.endpoint::<Bulk, In>(i)?;
        Ok(UsbPicoboot { interface, ep_out, ep_in })
    }
}

impl PicobootTransport for UsbPicoboot {
    fn bulk_out(&mut self, data: &[u8], timeout: Duration) -> crate::Result<()> {
        let c = self.ep_out.transfer_blocking(Buffer::from(data.to_vec()), timeout);
        check_completion(&c)
    }

    fn bulk_in(&mut self, len: usize, timeout: Duration) -> crate::Result<Vec<u8>> {
        // nusb requires IN requests in whole packets; a short packet ends the transfer.
        let mps = self.ep_in.max_packet_size().max(1);
        let req = len.max(1).div_ceil(mps) * mps;
        let c = self.ep_in.transfer_blocking(Buffer::new(req), timeout);
        check_completion(&c)?;
        let mut v = c.buffer.into_vec();
        v.truncate(c.actual_len.min(len));
        Ok(v)
    }

    fn interface_reset(&mut self) -> crate::Result<()> {
        let _ = self.ep_out.clear_halt().wait();
        let _ = self.ep_in.clear_halt().wait();
        let req = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: picoboot::IF_RESET,
            value: 0,
            index: self.interface.interface_number() as u16,
            data: &[],
        };
        self.interface.control_out(req, Duration::from_secs(1)).wait()?;
        Ok(())
    }

    fn cmd_status(&mut self) -> crate::Result<[u8; picoboot::STATUS_LEN]> {
        let req = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request: picoboot::IF_CMD_STATUS,
            value: 0,
            index: self.interface.interface_number() as u16,
            length: picoboot::STATUS_LEN as u16,
        };
        let v = self.interface.control_in(req, Duration::from_secs(1)).wait()?;
        let mut out = [0u8; picoboot::STATUS_LEN];
        let n = v.len().min(out.len());
        out[..n].copy_from_slice(&v[..n]);
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// PICOBOOT client
// ---------------------------------------------------------------------------

/// Timeout for commands without a data phase (erase can be slow), as picotool.
const CMD_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout for sending the command block and for the ack after a data phase.
const SHORT_TIMEOUT: Duration = Duration::from_secs(3);
/// Erase/write/read chunk: one 4 KB sector, the unit picotool uses.
pub const CHUNK: u32 = SECTOR_SIZE;

/// An open PICOBOOT connection.
pub struct Picoboot {
    t: Box<dyn PicobootTransport>,
    token: u32,
    chip: Chip,
    exclusive: bool,
}

impl Picoboot {
    /// Reset the interface (it may have been left mid-command) and take
    /// exclusive access, as picotool does on connect.
    pub fn connect(t: Box<dyn PicobootTransport>, chip: Chip) -> BootResult<Picoboot> {
        let mut pb = Picoboot { t, token: 0, chip, exclusive: false };
        pb.t.interface_reset()?;
        pb.exclusive_access(picoboot::EXCLUSIVE)?;
        pb.exclusive = true;
        Ok(pb)
    }

    pub fn chip(&self) -> Chip {
        self.chip
    }

    /// Run one command: command block → optional data phase → ack in the
    /// opposite direction. On failure the device stalls; we fetch the status
    /// code, reset the interface and report it.
    fn command(&mut self, id: u8, args: &[u8], data_out: &[u8], in_len: u32) -> BootResult<Vec<u8>> {
        self.token = self.token.wrapping_add(1);
        let cmd = Command {
            token: self.token,
            id,
            args: args.to_vec(),
            transfer_len: if id & 0x80 != 0 { in_len } else { data_out.len() as u32 },
        };
        match self.exchange(&cmd, data_out) {
            Ok(v) => Ok(v),
            Err(e) if e.is_disconnect() => Err(e.into()),
            Err(e) => {
                let status = self.t.cmd_status().ok().and_then(|b| CmdStatus::decode(&b));
                let _ = self.t.interface_reset();
                match status {
                    Some(s) if s.status != 0 => Err(BootError::Picoboot { cmd: id, status: s.status }),
                    _ => Err(e.into()),
                }
            }
        }
    }

    fn exchange(&mut self, cmd: &Command, data_out: &[u8]) -> crate::Result<Vec<u8>> {
        self.t.bulk_out(&cmd.encode(), SHORT_TIMEOUT)?;
        let mut data = Vec::new();
        if cmd.transfer_len > 0 {
            if cmd.is_in() {
                let n = cmd.transfer_len as usize;
                while data.len() < n {
                    let chunk = self.t.bulk_in(n - data.len(), CMD_TIMEOUT)?;
                    if chunk.is_empty() {
                        break;
                    }
                    data.extend_from_slice(&chunk);
                }
                if data.len() != n {
                    return Err(Error::Timeout);
                }
            } else {
                self.t.bulk_out(data_out, CMD_TIMEOUT)?;
            }
        }
        let ack_timeout = if cmd.transfer_len == 0 { CMD_TIMEOUT } else { SHORT_TIMEOUT };
        if cmd.is_in() {
            // picotool sends a 1-byte OUT transfer as the ack
            self.t.bulk_out(&[0], ack_timeout)?;
        } else {
            self.t.bulk_in(1, ack_timeout)?; // zero-length IN packet
        }
        Ok(data)
    }

    pub fn exclusive_access(&mut self, mode: u8) -> BootResult<()> {
        self.command(picoboot::EXCLUSIVE_ACCESS, &[mode], &[], 0).map(|_| ())
    }

    pub fn exit_xip(&mut self) -> BootResult<()> {
        self.command(picoboot::EXIT_XIP, &[], &[], 0).map(|_| ())
    }

    pub fn enter_cmd_xip(&mut self) -> BootResult<()> {
        self.command(picoboot::ENTER_CMD_XIP, &[], &[], 0).map(|_| ())
    }

    /// Erase `len` bytes at `addr` (both 4 KB aligned).
    pub fn flash_erase(&mut self, addr: u32, len: u32) -> BootResult<()> {
        self.command(picoboot::FLASH_ERASE, &range_args(addr, len), &[], 0).map(|_| ())
    }

    /// Program `data` at `addr` (256-byte aligned pages; flash must be erased).
    pub fn write(&mut self, addr: u32, data: &[u8]) -> BootResult<()> {
        self.command(picoboot::WRITE, &range_args(addr, data.len() as u32), data, 0).map(|_| ())
    }

    pub fn read(&mut self, addr: u32, len: u32) -> BootResult<Vec<u8>> {
        self.command(picoboot::READ, &range_args(addr, len), &[], len)
    }

    /// Reboot into the flash program after `delay_ms`. The device may drop
    /// off the bus before acknowledging, so transfer errors are ignored.
    pub fn reboot(&mut self, delay_ms: u32) -> BootResult<()> {
        let r = match self.chip {
            Chip::Rp2040 => {
                let mut a = 0u32.to_le_bytes().to_vec(); // dPC = 0: normal boot
                a.extend_from_slice(&picoboot::RP2040_SRAM_END.to_le_bytes());
                a.extend_from_slice(&delay_ms.to_le_bytes());
                self.command(picoboot::REBOOT, &a, &[], 0)
            }
            Chip::Rp2350 => {
                let mut a = picoboot::REBOOT2_FLAG_REBOOT_TYPE_NORMAL.to_le_bytes().to_vec();
                a.extend_from_slice(&delay_ms.to_le_bytes());
                a.extend_from_slice(&[0u8; 8]);
                self.command(picoboot::REBOOT2, &a, &[], 0)
            }
        };
        self.exclusive = false;
        match r {
            Err(BootError::Picoboot { .. }) => r.map(|_| ()),
            _ => Ok(()),
        }
    }
}

impl Drop for Picoboot {
    fn drop(&mut self) {
        if self.exclusive {
            let _ = self.exclusive_access(picoboot::NOT_EXCLUSIVE);
        }
    }
}

/// Stage of a firmware installation, for progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    EnterBootLoader,
    WaitBootLoader,
    Connecting,
    Erasing,
    Writing,
    Verifying,
    Copying,
    Rebooting,
    WaitDevice,
    Done,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct FlashOptions {
    pub verify: bool,
    pub reboot: bool,
}

/// Flash a plan: exit XIP → erase the plan's sectors → write its pages →
/// (verify) → (reboot). Nothing outside `plan.sectors` is erased or written.
pub fn flash(pb: &mut Picoboot, plan: &FlashPlan, opts: FlashOptions, progress: &mut dyn FnMut(Progress)) -> BootResult<()> {
    if plan.chip != pb.chip() {
        return Err(Uf2Error::NoBlocksForChip(pb.chip().name()).into());
    }
    pb.exit_xip()?;

    let erases = plan.erase_ranges(CHUNK);
    let total = erases.len() as u64 * CHUNK as u64;
    let mut done = 0;
    progress(Progress { phase: Phase::Erasing, done, total });
    for (addr, len) in erases {
        pb.flash_erase(addr, len)?;
        done += len as u64;
        progress(Progress { phase: Phase::Erasing, done, total });
    }

    let runs = plan.write_runs(CHUNK);
    let total = plan.total_write_bytes();
    let mut done = 0;
    progress(Progress { phase: Phase::Writing, done, total });
    for (addr, data) in &runs {
        pb.write(*addr, data)?;
        done += data.len() as u64;
        progress(Progress { phase: Phase::Writing, done, total });
    }

    if opts.verify {
        // flash reads need the XIP read path enabled again
        pb.enter_cmd_xip()?;
        let mut done = 0;
        progress(Progress { phase: Phase::Verifying, done, total });
        for (addr, data) in &runs {
            let got = pb.read(*addr, data.len() as u32)?;
            if let Some(i) = got.iter().zip(data).position(|(a, b)| a != b) {
                return Err(BootError::Verify { addr: addr + i as u32 });
            }
            if got.len() != data.len() {
                return Err(BootError::Verify { addr: addr + got.len() as u32 });
            }
            done += data.len() as u64;
            progress(Progress { phase: Phase::Verifying, done, total });
        }
    }

    if opts.reboot {
        progress(Progress { phase: Phase::Rebooting, done: 0, total: 0 });
        pb.reboot(500)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Mass-storage fallback
// ---------------------------------------------------------------------------

/// Parsed `INFO_UF2.TXT` from a boot ROM drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoUf2 {
    /// Boot loader version (after "UF2 Bootloader v").
    pub version: String,
    pub model: Option<String>,
    /// `RPI-RP2` (RP2040) or `RP2350`, without any version suffix.
    pub board_id: String,
    /// All `Name: Value` lines, names lower-cased.
    pub tags: BTreeMap<String, String>,
}

impl InfoUf2 {
    pub fn chip(&self) -> Chip {
        if self.board_id == "RP2350" {
            Chip::Rp2350
        } else {
            Chip::Rp2040
        }
    }
}

/// Parse INFO_UF2.TXT; `None` unless it's from a Raspberry Pi Pico boot ROM.
pub fn parse_info_uf2(text: &str) -> Option<InfoUf2> {
    let mut lines = text.lines();
    let first = lines.next()?.trim();
    if first.len() < 16 || !first[..16].eq_ignore_ascii_case("UF2 Bootloader v") {
        return None;
    }
    let version = first[16..].trim().to_string();
    let mut tags = BTreeMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim();
            if !k.is_empty() && !k.contains(char::is_whitespace) {
                tags.insert(k.to_ascii_lowercase(), v.trim().to_string());
            }
        }
    }
    let id = tags.get("board-id")?;
    let board_id = ["RPI-RP2", "RP2350"].into_iter().find(|t| {
        id.len() >= t.len()
            && id[..t.len()].eq_ignore_ascii_case(t)
            && (id.len() == t.len() || id.as_bytes()[t.len()] == b'-')
    })?;
    Some(InfoUf2 { version, model: tags.get("model").cloned(), board_id: board_id.to_string(), tags })
}

/// A mounted boot ROM drive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootVolume {
    pub path: PathBuf,
    pub chip: Chip,
    pub info: InfoUf2,
}

/// Check whether `root` is a boot ROM drive.
pub fn probe_volume(root: &Path) -> Option<BootVolume> {
    use std::io::Read;
    let f = std::fs::File::open(root.join("INFO_UF2.TXT")).ok()?;
    let mut text = String::new();
    f.take(4096).read_to_string(&mut text).ok()?;
    let info = parse_info_uf2(&text)?;
    Some(BootVolume { path: root.to_path_buf(), chip: info.chip(), info })
}

/// Mount points listed in `/proc/mounts` for FAT file systems (the boot ROM
/// drive is FAT), with octal escapes (`\040` = space) decoded.
pub fn parse_proc_mounts(text: &str) -> Vec<PathBuf> {
    fn unescape(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
                out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (_dev, mp, fs) = (f.next()?, f.next()?, f.next()?);
            matches!(fs, "vfat" | "msdos" | "fat" | "fuseblk" | "exfat").then(|| PathBuf::from(unescape(mp)))
        })
        .collect()
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/// Places a boot ROM drive may be mounted on this OS.
fn candidate_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "linux") {
        if let Ok(t) = std::fs::read_to_string("/proc/mounts") {
            v.extend(parse_proc_mounts(&t));
        }
        for base in ["/media", "/run/media"] {
            for d in subdirs(Path::new(base)) {
                v.push(d.clone());
                v.extend(subdirs(&d)); // /media/<user>/<label>
            }
        }
    } else if cfg!(target_os = "macos") {
        v.extend(subdirs(Path::new("/Volumes")));
    } else if cfg!(windows) {
        v.extend((b'C'..=b'Z').map(|c| PathBuf::from(format!("{}:\\", c as char))));
    }
    v.sort();
    v.dedup();
    v
}

/// Find mounted boot ROM drives (reads `INFO_UF2.TXT` on candidate volumes).
pub fn find_boot_volumes() -> Vec<BootVolume> {
    candidate_roots().iter().filter_map(|r| probe_volume(r)).collect()
}

/// Copy a UF2 image onto a boot ROM drive. The Pico reboots by itself once
/// it has received the last block, which can make the final flush fail;
/// that's reported as success when every byte was already written.
pub fn copy_to_volume(root: &Path, bytes: &[u8], progress: &mut dyn FnMut(Progress)) -> BootResult<()> {
    let path = root.join("PINSCAPE.UF2");
    let io = |e: std::io::Error| BootError::Io(format!("{}: {e}", path.display()));
    let mut f = std::fs::File::create(&path).map_err(io)?;
    let total = bytes.len() as u64;
    let mut done = 0u64;
    progress(Progress { phase: Phase::Copying, done, total });
    for chunk in bytes.chunks(32 * 1024) {
        f.write_all(chunk).map_err(io)?;
        done += chunk.len() as u64;
        progress(Progress { phase: Phase::Copying, done, total });
    }
    let _ = f.flush();
    let _ = f.sync_all(); // may fail if the Pico already rebooted
    Ok(())
}

// ---------------------------------------------------------------------------
// Simulated boot ROM
// ---------------------------------------------------------------------------

/// A simulated PICOBOOT interface with a flash array: enforces alignment,
/// erase-before-write (NOR semantics), XIP state and exclusive access, and
/// records every command, so the flashing sequence can be tested without
/// hardware (and the UI exercised with `PINSCAPE_MOCK_BOOTLOADER`).
pub mod sim {
    use super::*;
    use pinscape_protocol::uf2::FLASH_START;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Default)]
    enum Expect {
        #[default]
        Command,
        DataOut(Command),
        DataIn(Vec<u8>),
        AckIn,
        AckOut,
    }

    #[derive(Debug)]
    pub struct SimState {
        pub flash: Vec<u8>,
        /// Command IDs in the order received.
        pub log: Vec<u8>,
        /// Sector addresses erased, in order.
        pub erased: Vec<u32>,
        pub exclusive: u8,
        pub xip_exited: bool,
        pub rebooted: bool,
        /// Writes that tried to set a 0 bit back to 1 (flash not erased).
        pub dirty_writes: u32,
        /// Fail (stall with INVALID_ADDRESS) a WRITE to this address.
        pub fail_write_at: Option<u32>,
        /// Corrupt this flash address after writes (to test verification).
        pub corrupt_at: Option<u32>,
        /// Simulate a slow device (per command), for UI testing.
        pub delay: Duration,
        expect: Expect,
        stalled: bool,
        last: CmdStatus,
    }

    #[derive(Clone)]
    pub struct SimPicoboot(pub Arc<Mutex<SimState>>);

    impl SimPicoboot {
        /// A device with `flash_size` bytes of flash filled with `fill`.
        pub fn new(flash_size: usize, fill: u8) -> SimPicoboot {
            SimPicoboot(Arc::new(Mutex::new(SimState {
                flash: vec![fill; flash_size],
                log: Vec::new(),
                erased: Vec::new(),
                exclusive: 0,
                xip_exited: false,
                rebooted: false,
                dirty_writes: 0,
                fail_write_at: None,
                corrupt_at: None,
                delay: Duration::ZERO,
                expect: Expect::Command,
                stalled: false,
                last: CmdStatus { token: 0, status: 0, cmd_id: 0, in_progress: false },
            })))
        }

        pub fn state(&self) -> std::sync::MutexGuard<'_, SimState> {
            self.0.lock().unwrap()
        }
    }

    fn stall() -> Error {
        Error::Transfer(nusb::transfer::TransferError::Stall)
    }

    impl SimState {
        fn range(&self, c: &Command) -> Option<(usize, usize)> {
            if c.args.len() != 8 {
                return None;
            }
            let addr = u32::from_le_bytes(c.args[0..4].try_into().ok()?);
            let len = u32::from_le_bytes(c.args[4..8].try_into().ok()?);
            let ofs = addr.checked_sub(FLASH_START)? as usize;
            (ofs + len as usize <= self.flash.len()).then_some((ofs, len as usize))
        }

        fn fail(&mut self, c: &Command, status: u32) -> crate::Result<()> {
            self.last = CmdStatus { token: c.token, status, cmd_id: c.id, in_progress: false };
            self.stalled = true;
            self.expect = Expect::Command;
            Err(stall())
        }

        fn ok(&mut self, c: &Command) {
            self.last = CmdStatus { token: c.token, status: 0, cmd_id: c.id, in_progress: false };
        }

        fn start(&mut self, c: Command) -> crate::Result<()> {
            use picoboot::*;
            self.log.push(c.id);
            std::thread::sleep(self.delay);
            let expected_args = match c.id {
                EXCLUSIVE_ACCESS => 1,
                REBOOT => 12,
                REBOOT2 => 16,
                FLASH_ERASE | WRITE | READ => 8,
                EXIT_XIP | ENTER_CMD_XIP => 0,
                _ => return self.fail(&c, 1),
            };
            if c.args.len() != expected_args {
                return self.fail(&c, 2);
            }
            if c.transfer_len > 0 && !matches!(c.id, WRITE | READ) {
                return self.fail(&c, 3);
            }
            match c.id {
                EXCLUSIVE_ACCESS => self.exclusive = c.args[0],
                EXIT_XIP => self.xip_exited = true,
                ENTER_CMD_XIP => self.xip_exited = false,
                REBOOT | REBOOT2 => self.rebooted = true,
                FLASH_ERASE => {
                    let Some((ofs, len)) = self.range(&c) else { return self.fail(&c, 4) };
                    if ofs % 4096 != 0 || len % 4096 != 0 {
                        return self.fail(&c, 5);
                    }
                    if !self.xip_exited {
                        return self.fail(&c, 9);
                    }
                    self.flash[ofs..ofs + len].fill(0xFF);
                    for s in (ofs..ofs + len).step_by(4096) {
                        self.erased.push(FLASH_START + s as u32);
                    }
                }
                WRITE => {
                    let Some((ofs, len)) = self.range(&c) else { return self.fail(&c, 4) };
                    if ofs % 256 != 0 || len % 256 != 0 || len as u32 != c.transfer_len {
                        return self.fail(&c, 5);
                    }
                    if !self.xip_exited {
                        return self.fail(&c, 9);
                    }
                    if self.fail_write_at == Some(FLASH_START + ofs as u32) {
                        return self.fail(&c, 4);
                    }
                    self.ok(&c);
                    self.expect = Expect::DataOut(c);
                    return Ok(());
                }
                READ => {
                    let Some((ofs, len)) = self.range(&c) else { return self.fail(&c, 4) };
                    if len as u32 != c.transfer_len {
                        return self.fail(&c, 3);
                    }
                    if self.xip_exited {
                        // real RP2040 can't read flash with XIP off
                        return self.fail(&c, 9);
                    }
                    self.ok(&c);
                    self.expect = Expect::DataIn(self.flash[ofs..ofs + len].to_vec());
                    return Ok(());
                }
                _ => unreachable!(),
            }
            self.ok(&c);
            self.expect = if c.is_in() { Expect::AckOut } else { Expect::AckIn };
            Ok(())
        }
    }

    impl PicobootTransport for SimPicoboot {
        fn bulk_out(&mut self, data: &[u8], _timeout: Duration) -> crate::Result<()> {
            let mut s = self.0.lock().unwrap();
            if s.stalled {
                return Err(stall());
            }
            match std::mem::take(&mut s.expect) {
                Expect::Command => match Command::decode(data) {
                    Some(c) => s.start(c),
                    None => {
                        s.stalled = true;
                        Err(stall())
                    }
                },
                Expect::DataOut(c) => {
                    let (ofs, _) = s.range(&c).unwrap();
                    if data.len() != c.transfer_len as usize {
                        return s.fail(&c, 3);
                    }
                    for (i, &b) in data.iter().enumerate() {
                        let cur = s.flash[ofs + i];
                        if cur & b != b {
                            s.dirty_writes += 1;
                        }
                        s.flash[ofs + i] = cur & b;
                    }
                    if let Some(a) = s.corrupt_at {
                        let o = (a - FLASH_START) as usize;
                        if (ofs..ofs + data.len()).contains(&o) {
                            s.flash[o] ^= 0x01;
                        }
                    }
                    s.expect = Expect::AckIn;
                    Ok(())
                }
                Expect::AckOut => Ok(()),
                other => {
                    s.expect = other;
                    s.stalled = true;
                    Err(stall())
                }
            }
        }

        fn bulk_in(&mut self, len: usize, _timeout: Duration) -> crate::Result<Vec<u8>> {
            let mut s = self.0.lock().unwrap();
            if s.stalled {
                return Err(stall());
            }
            match std::mem::take(&mut s.expect) {
                Expect::DataIn(mut d) => {
                    let n = len.min(d.len());
                    let rest = d.split_off(n);
                    s.expect = if rest.is_empty() { Expect::AckOut } else { Expect::DataIn(rest) };
                    Ok(d)
                }
                Expect::AckIn => Ok(Vec::new()),
                other => {
                    s.expect = other;
                    Err(Error::Timeout)
                }
            }
        }

        fn interface_reset(&mut self) -> crate::Result<()> {
            let mut s = self.0.lock().unwrap();
            s.stalled = false;
            s.expect = Expect::Command;
            Ok(())
        }

        fn cmd_status(&mut self) -> crate::Result<[u8; picoboot::STATUS_LEN]> {
            Ok(self.0.lock().unwrap().last.encode())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sim::SimPicoboot;
    use super::*;
    use pinscape_protocol::uf2::{build_image, family, Uf2Image, FLASH_START, PAGE_SIZE};

    const MB2: usize = 2 << 20;

    fn connect(sim: &SimPicoboot, chip: Chip) -> Picoboot {
        Picoboot::connect(Box::new(sim.clone()), chip).unwrap()
    }

    #[test]
    fn command_encoding_matches_picoboot_h() {
        let c = Command { token: 0x11223344, id: picoboot::WRITE, args: range_args(0x1000_0100, 0x200), transfer_len: 0x200 };
        let b = c.encode();
        assert_eq!(&b[0..4], &[0x0B, 0xD1, 0x1F, 0x43]); // magic 0x431fd10b LE
        assert_eq!(&b[4..8], &[0x44, 0x33, 0x22, 0x11]);
        assert_eq!(b[8], 0x05);
        assert_eq!(b[9], 8);
        assert_eq!(&b[10..12], &[0, 0]);
        assert_eq!(&b[12..16], &0x200u32.to_le_bytes());
        assert_eq!(&b[16..20], &0x1000_0100u32.to_le_bytes());
        assert_eq!(&b[20..24], &0x200u32.to_le_bytes());
        assert!(b[24..].iter().all(|&x| x == 0));
        assert_eq!(Command::decode(&b), Some(c));
        assert!(Command { token: 0, id: picoboot::READ, args: vec![], transfer_len: 0 }.is_in());
        assert!(Command::decode(&[0u8; 32]).is_none());

        let s = CmdStatus { token: 7, status: 4, cmd_id: 5, in_progress: true };
        let e = s.encode();
        assert_eq!(&e[4..8], &[4, 0, 0, 0]);
        assert_eq!(CmdStatus::decode(&e), Some(s));
    }

    #[test]
    fn connect_resets_and_takes_exclusive_access_then_releases() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        let pb = connect(&sim, Chip::Rp2040);
        assert_eq!(sim.state().exclusive, picoboot::EXCLUSIVE);
        drop(pb);
        assert_eq!(sim.state().exclusive, picoboot::NOT_EXCLUSIVE);
        assert_eq!(sim.state().log, vec![picoboot::EXCLUSIVE_ACCESS, picoboot::EXCLUSIVE_ACCESS]);
    }

    fn image_with_config_area(sim: &SimPicoboot) -> (Uf2Image, Vec<u8>) {
        // existing "old firmware" + config at the top of the 2 MB flash
        {
            let mut s = sim.state();
            let n = s.flash.len();
            s.flash[..0x30000].fill(0x5A);
            s.flash[n - 4096..].fill(0xC3);
            s.flash[n - 3 * 4096..n - 2 * 4096].fill(0xC4);
        }
        let fw: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &fw)])).unwrap();
        (img, fw)
    }

    #[test]
    fn flash_erases_only_image_sectors_and_preserves_config() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        let (img, fw) = image_with_config_area(&sim);
        let before = sim.state().flash.clone();
        let plan = img.plan(Chip::Rp2040).unwrap();
        let mut pb = connect(&sim, Chip::Rp2040);
        let mut events = Vec::new();
        flash(&mut pb, &plan, FlashOptions { verify: true, reboot: true }, &mut |p| events.push(p)).unwrap();
        drop(pb);

        let s = sim.state();
        // 70,000 bytes → 18 sectors (0x10000000..0x10012000)
        let expect: Vec<u32> = (0..18).map(|i| FLASH_START + i * 4096).collect();
        assert_eq!(s.erased, expect);
        assert_eq!(&s.flash[..fw.len()], &fw[..]);
        // rest of the last image sector is erased (0xFF), as with the boot ROM's own UF2 handling
        assert!(s.flash[fw.len()..18 * 4096].iter().all(|&b| b == 0xFF));
        // everything after it untouched: old data and the config area
        assert_eq!(&s.flash[18 * 4096..], &before[18 * 4096..]);
        assert!(s.flash[MB2 - 4096..].iter().all(|&b| b == 0xC3));
        assert_eq!(s.dirty_writes, 0);
        assert!(s.rebooted);
        assert_eq!(s.exclusive, picoboot::EXCLUSIVE, "no release after reboot");

        // command order: exclusive, exit_xip, 18 erases, writes, enter_cmd_xip, reads, reboot
        use picoboot::*;
        let n_runs = plan.write_runs(CHUNK).len();
        assert_eq!(s.log[0], EXCLUSIVE_ACCESS);
        assert_eq!(s.log[1], EXIT_XIP);
        assert!(s.log[2..20].iter().all(|&c| c == FLASH_ERASE));
        assert!(s.log[20..20 + n_runs].iter().all(|&c| c == WRITE));
        assert_eq!(s.log[20 + n_runs], ENTER_CMD_XIP);
        assert!(s.log[21 + n_runs..21 + 2 * n_runs].iter().all(|&c| c == READ));
        assert_eq!(*s.log.last().unwrap(), REBOOT);

        // progress: monotonic per phase and ends complete
        let last = |ph: Phase| events.iter().rev().find(|p| p.phase == ph).unwrap().clone();
        assert_eq!(last(Phase::Erasing).done, last(Phase::Erasing).total);
        assert_eq!(last(Phase::Writing).done, plan.total_write_bytes());
        assert_eq!(last(Phase::Verifying).done, plan.total_write_bytes());
        assert_eq!(events.last().unwrap().phase, Phase::Rebooting);
    }

    #[test]
    fn rp2350_uses_reboot2() {
        let sim = SimPicoboot::new(4 << 20, 0xFF);
        let img = Uf2Image::parse(&build_image(family::ABSOLUTE, &[(FLASH_START, &[1u8; 512])])).unwrap();
        let mut pb = connect(&sim, Chip::Rp2350);
        flash(&mut pb, &img.plan(Chip::Rp2350).unwrap(), FlashOptions { verify: false, reboot: true }, &mut |_| {}).unwrap();
        let s = sim.state();
        assert_eq!(*s.log.last().unwrap(), picoboot::REBOOT2);
        assert!(!s.log.contains(&picoboot::READ));
        assert_eq!(&s.flash[..512], &[1u8; 512]);
    }

    #[test]
    fn chip_mismatch_is_refused_before_touching_flash() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &[1u8; 512])])).unwrap();
        let plan = img.plan(Chip::Rp2040).unwrap();
        let mut pb = connect(&sim, Chip::Rp2350);
        assert!(matches!(flash(&mut pb, &plan, FlashOptions { verify: false, reboot: false }, &mut |_| {}), Err(BootError::Image(_))));
        assert!(sim.state().erased.is_empty());
    }

    #[test]
    fn device_error_reports_status_and_recovers() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        let (img, _) = image_with_config_area(&sim);
        let plan = img.plan(Chip::Rp2040).unwrap();
        sim.state().fail_write_at = Some(FLASH_START + 0x2000);
        let mut pb = connect(&sim, Chip::Rp2040);
        let err = flash(&mut pb, &plan, FlashOptions { verify: false, reboot: true }, &mut |_| {}).unwrap_err();
        assert!(matches!(err, BootError::Picoboot { cmd: picoboot::WRITE, status: 4 }), "{err}");
        assert!(err.to_string().contains("invalid address"));
        assert!(!sim.state().rebooted);
        // the interface was reset, so the next command works
        pb.exit_xip().unwrap();
        // bad alignment from the device side
        let err = pb.flash_erase(FLASH_START + 0x100, 4096).unwrap_err();
        assert!(matches!(err, BootError::Picoboot { cmd: picoboot::FLASH_ERASE, status: 5 }));
    }

    #[test]
    fn verify_detects_mismatch() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        let (img, _) = image_with_config_area(&sim);
        sim.state().corrupt_at = Some(FLASH_START + 0x1234);
        let mut pb = connect(&sim, Chip::Rp2040);
        let err = flash(&mut pb, &img.plan(Chip::Rp2040).unwrap(), FlashOptions { verify: true, reboot: true }, &mut |_| {}).unwrap_err();
        assert!(matches!(err, BootError::Verify { addr } if addr == FLASH_START + 0x1234));
        assert!(!sim.state().rebooted, "never reboot into a bad image");
    }

    #[test]
    fn read_roundtrip() {
        let sim = SimPicoboot::new(MB2, 0xFF);
        sim.state().flash[0x100..0x104].copy_from_slice(&[1, 2, 3, 4]);
        let mut pb = connect(&sim, Chip::Rp2040);
        assert_eq!(pb.read(FLASH_START + 0x100, 4).unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn info_uf2_parsing() {
        let rp2040 = "UF2 Bootloader v3.0\r\nModel: Raspberry Pi RP2\r\nBoard-ID: RPI-RP2\r\n";
        let i = parse_info_uf2(rp2040).unwrap();
        assert_eq!(i.version, "3.0");
        assert_eq!(i.model.as_deref(), Some("Raspberry Pi RP2"));
        assert_eq!(i.chip(), Chip::Rp2040);
        let rp2350 = "UF2 Bootloader v1.0\nModel: Raspberry Pi RP2350\nBoard-ID: RP2350\n";
        assert_eq!(parse_info_uf2(rp2350).unwrap().chip(), Chip::Rp2350);
        assert_eq!(parse_info_uf2("UF2 Bootloader v1\nBoard-ID: RPI-RP2-B1\n").unwrap().board_id, "RPI-RP2");
        assert!(parse_info_uf2("UF2 Bootloader v1\nBoard-ID: SAMD21\n").is_none());
        assert!(parse_info_uf2("UF2 Bootloader v1\nBoard-ID: RPI-RP2X\n").is_none());
        assert!(parse_info_uf2("Hello\nBoard-ID: RPI-RP2\n").is_none());
        assert!(parse_info_uf2("").is_none());
    }

    #[test]
    fn proc_mounts_parsing() {
        let t = "sysfs /sys sysfs rw 0 0\n/dev/sda1 /media/me/RPI-RP2 vfat rw,nosuid 0 0\n/dev/sdb1 /media/me/My\\040Drive vfat rw 0 0\n/dev/nvme0n1p2 / ext4 rw 0 0\n";
        assert_eq!(parse_proc_mounts(t), vec![PathBuf::from("/media/me/RPI-RP2"), PathBuf::from("/media/me/My Drive")]);
    }

    #[test]
    fn volume_probe_and_copy_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("pinscape-bootvol-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(probe_volume(&dir).is_none());
        std::fs::write(dir.join("INFO_UF2.TXT"), "UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n").unwrap();
        let v = probe_volume(&dir).unwrap();
        assert_eq!(v.chip, Chip::Rp2040);
        let data = vec![7u8; 100_000];
        let mut last = None;
        copy_to_volume(&dir, &data, &mut |p| last = Some(p)).unwrap();
        assert_eq!(std::fs::read(dir.join("PINSCAPE.UF2")).unwrap(), data);
        let p = last.unwrap();
        assert_eq!((p.phase, p.done, p.total), (Phase::Copying, 100_000, 100_000));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn boot_ids() {
        assert_eq!(boot_chip(0x2E8A, 0x0003), Some(Chip::Rp2040));
        assert_eq!(boot_chip(0x2E8A, 0x000F), Some(Chip::Rp2350));
        assert_eq!(boot_chip(0x2E8A, 0x000A), None);
        assert_eq!(boot_chip(0x1209, 0x0003), None);
    }

    #[test]
    fn write_runs_are_page_aligned_and_within_chunk() {
        let fw = vec![0u8; 10_000];
        let img = Uf2Image::parse(&build_image(family::RP2040, &[(FLASH_START, &fw)])).unwrap();
        for (a, d) in img.plan(Chip::Rp2040).unwrap().write_runs(CHUNK) {
            assert_eq!(a % PAGE_SIZE, 0);
            assert_eq!(d.len() as u32 % PAGE_SIZE, 0);
            assert!(d.len() as u32 <= CHUNK);
            assert_eq!(a / SECTOR_SIZE, (a + d.len() as u32 - 1) / SECTOR_SIZE);
        }
    }
}
