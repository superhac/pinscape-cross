//! Cross-platform USB access to Pinscape Pico devices.
//!
//! Built on [`nusb`], which uses usbfs on Linux, WinUSB on Windows and IOKit
//! on macOS, so this one code path serves all three platforms.
//!
//! Discovery does **not** rely on VID/PID (users can change those in the
//! config). Instead we look for a vendor-class (0xFF) interface whose string
//! descriptor is `PinscapePicoControl`, and find its bulk endpoints from the
//! interface descriptor.

use std::io::Read;
use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, Direction, In, Out};
use nusb::{DeviceInfo, MaybeFuture};
use pinscape_protocol::{self as proto, cmd, DeviceIds, Request, Response, Status, Version};
use serde::Serialize;

pub use nusb::DeviceId;

pub mod mock;

/// Default timeout for a single USB transfer.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

/// RP2040 / RP2350 boot ROM USB IDs (excluded from Pinscape discovery).
const RPI_VID: u16 = 0x2E8A;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USB error: {0}")]
    Usb(#[from] nusb::Error),
    #[error("USB transfer error: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("transfer timed out")]
    Timeout,
    #[error("permission denied opening the device (on Linux, install the udev rule: packaging/udev/99-pinscape.rules)")]
    PermissionDenied,
    #[error("device is busy (another program, such as a config tool, has it open)")]
    Busy,
    #[error("vendor interface has no bulk IN/OUT endpoint pair")]
    NoEndpoints,
    #[error("device returned status {code}: {}", Status::from_u16(*.code).map(|s| s.description()).unwrap_or("unknown"))]
    Status { code: u16 },
    #[error("reply did not match request")]
    ReplyMismatch,
    #[error(transparent)]
    Decode(#[from] proto::DecodeError),
    #[error("no matching Pinscape Pico device found")]
    NotFound,
}

impl Error {
    pub fn status(&self) -> Option<Status> {
        match self {
            Error::Status { code } => Status::from_u16(*code),
            _ => None,
        }
    }

    /// The underlying USB transfer error, if any (including one wrapped in an I/O error).
    pub fn transfer_error(&self) -> Option<nusb::transfer::TransferError> {
        match self {
            Error::Transfer(t) => Some(*t),
            Error::Io(io) => io.get_ref()?.downcast_ref::<nusb::transfer::TransferError>().copied(),
            _ => None,
        }
    }

    /// True if the error means the device has gone away (unplugged or rebooting).
    pub fn is_disconnect(&self) -> bool {
        use nusb::transfer::TransferError as T;
        match self {
            Error::Usb(u) => u.kind() == nusb::ErrorKind::Disconnected,
            _ => matches!(self.transfer_error(), Some(T::Disconnected | T::Fault)),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A Pinscape Pico found on the bus (not yet opened).
#[derive(Clone)]
pub struct Candidate {
    pub info: DeviceInfo,
    pub interface: u8,
}

/// Serializable summary of a discovered device.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceSummary {
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub product: Option<String>,
    pub interface: u8,
}

impl Candidate {
    pub fn summary(&self) -> DeviceSummary {
        DeviceSummary {
            vid: self.info.vendor_id(),
            pid: self.info.product_id(),
            serial: self.info.serial_number().map(str::to_owned),
            product: self.info.product_string().map(str::to_owned),
            interface: self.interface,
        }
    }

    pub fn open(&self) -> Result<VendorClient> {
        VendorClient::open(self)
    }
}

/// Enumerate attached Pinscape Pico devices (normal firmware mode).
pub fn find_devices() -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for info in nusb::list_devices().wait()? {
        if info.vendor_id() == RPI_VID {
            continue; // boot ROM, not Pinscape firmware
        }
        let default_ids = info.vendor_id() == proto::DEFAULT_VID
            && proto::DEFAULT_PIDS.contains(&info.product_id());
        let ifc = info.interfaces().find(|i| {
            i.class() == 0xFF
                && match i.interface_string() {
                    Some(s) => s == proto::VENDOR_INTERFACE_STRING,
                    // Some platforms don't cache interface strings at
                    // enumeration time; fall back to the default IDs.
                    None => default_ids,
                }
        });
        if let Some(ifc) = ifc {
            out.push(Candidate { interface: ifc.interface_number(), info: info.clone() });
        }
    }
    Ok(out)
}

fn map_open_err(e: nusb::Error) -> Error {
    match e.kind() {
        nusb::ErrorKind::PermissionDenied => Error::PermissionDenied,
        nusb::ErrorKind::Busy => Error::Busy,
        _ => Error::Usb(e),
    }
}

/// Byte-level link to a device's vendor interface: the real USB endpoints,
/// or a simulated device ([`mock::MockDevice`]) for tests and UI development.
pub trait Transport: Send {
    /// Write one OUT transfer.
    fn write(&mut self, data: &[u8]) -> Result<()>;
    /// Read exactly `n` bytes from the IN byte stream.
    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>>;
    fn set_timeout(&mut self, timeout: Duration);
}

/// Real USB transport over the vendor interface's bulk endpoints.
struct UsbTransport {
    ep_out: nusb::Endpoint<Bulk, Out>,
    /// The IN pipe is read as a byte stream: the device may pack a reply
    /// header and its data into the same packets, and terminates full-size
    /// transfers with zero-length packets. (WinUSB on Windows behaves the
    /// same way, which is what the upstream Windows API relies on.)
    ep_in: nusb::io::EndpointRead<Bulk>,
    timeout: Duration,
}

impl UsbTransport {
    fn open(c: &Candidate) -> Result<Self> {
        let device = c.info.open().wait().map_err(map_open_err)?;
        let interface = device.claim_interface(c.interface).wait().map_err(map_open_err)?;

        let (mut ep_out_addr, mut ep_in_addr) = (None, None);
        if let Some(desc) = interface.descriptor() {
            for ep in desc.endpoints() {
                if ep.transfer_type() != TransferType::Bulk {
                    continue;
                }
                match ep.direction() {
                    Direction::Out => ep_out_addr = ep_out_addr.or(Some(ep.address())),
                    Direction::In => ep_in_addr = ep_in_addr.or(Some(ep.address())),
                }
            }
        }
        let (Some(out_addr), Some(in_addr)) = (ep_out_addr, ep_in_addr) else {
            return Err(Error::NoEndpoints);
        };
        let ep_out = interface.endpoint::<Bulk, Out>(out_addr)?;
        let mut ep_in = interface
            .endpoint::<Bulk, In>(in_addr)?
            .reader(4096)
            .with_num_transfers(2)
            .with_read_timeout(Duration::from_millis(30));

        // Drain anything left in the IN pipe from a previous session.
        let mut junk = [0u8; 4096];
        while matches!(ep_in.read(&mut junk), Ok(n) if n > 0) {}
        ep_in.set_read_timeout(DEFAULT_TIMEOUT);

        Ok(UsbTransport { ep_out, ep_in, timeout: DEFAULT_TIMEOUT })
    }
}

impl Transport for UsbTransport {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        let c = self.ep_out.transfer_blocking(data.to_vec().into(), self.timeout);
        check_completion(&c)
    }

    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut out = vec![0u8; n];
        self.ep_in.read_exact(&mut out).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => Error::Timeout,
            _ => Error::Io(e),
        })?;
        if std::env::var_os("PINSCAPE_TRACE").is_some() {
            eprintln!("[usb] read {n} bytes");
        }
        Ok(out)
    }

    fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
        self.ep_in.set_read_timeout(timeout);
    }
}

/// An open connection to a device's vendor interface.
///
/// The protocol is strictly request/response, so a client is used from one
/// thread at a time (the app wraps it in a per-device worker).
pub struct VendorClient {
    transport: Box<dyn Transport>,
    token: u32,
    timeout: Duration,
}

impl VendorClient {
    pub fn open(c: &Candidate) -> Result<Self> {
        Ok(Self::with_transport(Box::new(UsbTransport::open(c)?)))
    }

    /// Build a client over any transport (e.g. [`mock::MockDevice`]).
    pub fn with_transport(transport: Box<dyn Transport>) -> Self {
        VendorClient {
            transport,
            token: std::process::id().wrapping_mul(2_654_435_761),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
        self.transport.set_timeout(timeout);
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.transport.write(data)
    }

    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>> {
        self.transport.read_exact(n)
    }

    fn next_token(&mut self) -> u32 {
        self.token = self.token.wrapping_add(1);
        self.token
    }

    /// Send a request (plus optional OUT data) and return the response and any IN data.
    ///
    /// Mirrors `VendorInterface::SendRequest` in the upstream Windows API:
    /// stale replies with a different token are skipped (including their
    /// extra data), and a non-OK status is returned as [`Error::Status`].
    pub fn transact(
        &mut self,
        cmd: u8,
        args: &[u8],
        xfer_out: &[u8],
    ) -> Result<(Response, Vec<u8>)> {
        let mut req = Request::with_args(self.next_token(), cmd, args);
        req.xfer_bytes = u16::try_from(xfer_out.len()).map_err(|_| Error::Status {
            code: Status::BadXferLen as u16,
        })?;
        self.write(&req.encode())?;
        if !xfer_out.is_empty() {
            self.write(xfer_out)?;
        }

        let resp = loop {
            let pkt = self.read_exact(proto::RESPONSE_LEN)?;
            let resp = Response::decode(&pkt)?;
            if resp.token == req.token {
                break resp;
            }
            // stale reply from an earlier request: discard its data too
            let _ = self.read_exact(resp.xfer_bytes as usize)?;
        };
        if resp.cmd != req.cmd {
            return Err(Error::ReplyMismatch);
        }
        let data = self.read_exact(resp.xfer_bytes as usize)?;
        if resp.status != Status::Ok as u16 {
            return Err(Error::Status { code: resp.status });
        }
        Ok((resp, data))
    }

    // ---------------------------------------------------------------
    // High-level commands
    // ---------------------------------------------------------------

    pub fn ping(&mut self) -> Result<()> {
        self.transact(cmd::PING, &[], &[]).map(|_| ())
    }

    pub fn query_version(&mut self) -> Result<Version> {
        let (r, _) = self.transact(cmd::QUERY_VERSION, &[], &[])?;
        Ok(Version::decode(&r)?)
    }

    pub fn query_ids(&mut self) -> Result<DeviceIds> {
        let (r, data) = self.transact(cmd::QUERY_IDS, &[], &[])?;
        Ok(DeviceIds::decode(&r, &data)?)
    }

    /// Reset the device into the given mode (see [`proto::reset`]).
    /// The device disconnects immediately, so a missing reply is not an error.
    pub fn reset(&mut self, mode: u8) -> Result<()> {
        match self.transact(cmd::RESET, &[mode], &[]) {
            Ok(_) | Err(Error::Timeout) | Err(Error::Transfer(_)) | Err(Error::Io(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Download a config file. Returns `Ok(None)` if no such file is stored.
    pub fn get_config(&mut self, file_id: u8) -> Result<Option<Vec<u8>>> {
        let mut out = Vec::new();
        for page in 0u16.. {
            let args = proto::ConfigArgs { subcmd: proto::config::GET, file_id, page, ..Default::default() };
            match self.transact(cmd::CONFIG, &args.encode(), &[]) {
                Ok((_, data)) => out.extend_from_slice(&data),
                Err(e) if e.status() == Some(Status::Eof) => break,
                Err(e) if e.status() == Some(Status::NotFound) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
        Ok(Some(out))
    }

    /// Upload a config file using the page protocol (start-of-file marker,
    /// then 4K pages in order, CRC-32 over the whole file). Each page is
    /// retried a few times; `ERR_RETRY_OK` on a retry counts as success.
    /// The new config takes effect after the next reset.
    pub fn put_config(&mut self, file_id: u8, data: &[u8]) -> Result<()> {
        use proto::config::{PAGE_SIZE, PUT};
        if data.is_empty() {
            return Err(Error::Status { code: Status::BadParams as u16 });
        }
        let n_pages = u16::try_from(data.len().div_ceil(PAGE_SIZE))
            .map_err(|_| Error::Status { code: Status::OutOfBounds as u16 })?;
        let crc = proto::crc32(data);
        let start = proto::ConfigArgs { subcmd: PUT, file_id, page: 0xFFFF, n_pages, crc };
        self.transact(cmd::CONFIG, &start.encode(), &[])?;

        let saved = self.timeout;
        self.set_timeout(saved.max(Duration::from_secs(5))); // flash writes are slow
        let result = (|| {
            for (page, chunk) in data.chunks(PAGE_SIZE).enumerate() {
                let args = proto::ConfigArgs { subcmd: PUT, file_id, page: page as u16, n_pages, crc };
                let mut tries = 0;
                loop {
                    match self.transact(cmd::CONFIG, &args.encode(), chunk) {
                        Ok(_) => break,
                        Err(e) if tries > 0 && e.status() == Some(Status::RetryOk) => break,
                        Err(e) if tries >= 3 => return Err(e),
                        Err(_) => tries += 1,
                    }
                }
            }
            Ok(())
        })();
        self.set_timeout(saved);
        result
    }

    /// Does the given config file exist on the device?
    pub fn config_exists(&mut self, file_id: u8) -> Result<bool> {
        let args = proto::ConfigArgs { subcmd: proto::config::EXISTS, file_id, ..Default::default() };
        match self.transact(cmd::CONFIG, &args.encode(), &[]) {
            Ok(_) => Ok(true),
            Err(e) if e.status() == Some(Status::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Erase a config file (`config::FILE_ALL` erases all of them).
    pub fn erase_config(&mut self, file_id: u8) -> Result<()> {
        let args = proto::ConfigArgs { subcmd: proto::config::ERASE, file_id, ..Default::default() };
        self.transact(cmd::CONFIG, &args.encode(), &[]).map(|_| ())
    }

    /// Factory reset: clear the whole config area in flash.
    pub fn factory_reset_config(&mut self) -> Result<()> {
        let args = proto::ConfigArgs { subcmd: proto::config::RESET, ..Default::default() };
        self.transact(cmd::CONFIG, &args.encode(), &[]).map(|_| ())
    }

    pub fn query_stats(&mut self, reset_counters: bool) -> Result<proto::Statistics> {
        let flags = if reset_counters { proto::stats::FLAG_RESET_COUNTERS } else { 0 };
        let (_, data) = self.transact(cmd::STATS, &[proto::stats::QUERY_STATS, flags], &[])?;
        Ok(proto::Statistics::decode(&data)?)
    }

    pub fn query_usb_interfaces(&mut self) -> Result<proto::UsbInterfaces> {
        let (_, data) = self.transact(cmd::QUERY_USBIFCS, &[], &[])?;
        Ok(proto::UsbInterfaces::decode(&data)?)
    }

    pub fn query_gpio_config(&mut self) -> Result<Vec<proto::GpioPort>> {
        let (_, data) = self.transact(cmd::QUERY_GPIO_CONFIG, &[], &[])?;
        Ok(proto::GpioPort::decode_all(&data)?)
    }

    /// Send the host's local wall-clock time (used for device log timestamps
    /// and timed events).
    pub fn set_clock(&mut self, t: proto::ClockArgs) -> Result<()> {
        self.transact(cmd::SET_CLOCK, &t.encode(), &[]).map(|_| ())
    }

    /// Fetch the next chunk of the device's in-memory log. Returns `None`
    /// when no log text is waiting; otherwise the text and the number of
    /// bytes still available on the device.
    pub fn query_log(&mut self) -> Result<Option<(Vec<u8>, u32)>> {
        match self.transact(cmd::QUERY_LOG, &[], &[]) {
            Ok((resp, data)) => Ok(Some((data, proto::decode_log_avail(&resp)))),
            Err(e) if e.status() == Some(Status::Eof) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn check_completion(c: &nusb::transfer::Completion) -> Result<()> {
    match c.status {
        Ok(()) => Ok(()),
        Err(nusb::transfer::TransferError::Cancelled) => Err(Error::Timeout),
        Err(e) => Err(Error::Transfer(e)),
    }
}

/// Find exactly one device, optionally matching `id` against the hardware ID
/// (full or ≥4-char substring), unit number, or unit name — the same rules
/// as the upstream config tools' `--id` option.
pub fn open_device(id: Option<&str>) -> Result<(VendorClient, DeviceIds)> {
    let mut last_err = None;
    for c in find_devices()? {
        let mut client = match c.open() {
            Ok(cl) => cl,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        let ids = client.query_ids()?;
        let matches = match id {
            None => true,
            Some(id) => {
                ids.hwid.eq_ignore_ascii_case(id)
                    || id.parse::<u8>().ok() == Some(ids.unit_num)
                    || ids.unit_name.eq_ignore_ascii_case(id)
                    || (id.len() >= 4 && ids.hwid.to_ascii_uppercase().contains(&id.to_ascii_uppercase()))
            }
        };
        if matches {
            return Ok((client, ids));
        }
    }
    Err(last_err.unwrap_or(Error::NotFound))
}
