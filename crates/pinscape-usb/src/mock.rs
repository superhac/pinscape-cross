//! A simulated Pinscape Pico, for tests and for UI development without
//! hardware (`PINSCAPE_MOCK=1` makes the app show one; `PINSCAPE_MOCK=N`
//! shows a set of `N` from [`fleet`], for the multi-device dashboard).
//!
//! It implements the vendor protocol at the byte level: requests are
//! decoded from the OUT stream, and replies (header + extra data) are
//! appended to a single IN byte stream, the same way the real firmware
//! can pack them. Tests can inspect and manipulate [`MockState`] through
//! the shared handle.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pinscape_protocol::{self as proto, cmd, Request, Status, ARGS_LEN, REQUEST_LEN, RESPONSE_LEN};

use crate::{Error, Result, Transport};

mod buttons;
mod ir;
mod nudge;
mod outputs;
mod plunger;

/// Simulated device state, shared between the transport and the test.
#[derive(Debug, Clone)]
pub struct MockState {
    pub hwid: [u8; 8],
    pub unit_num: u8,
    pub unit_name: String,
    pub version: (u8, u8, u8),
    /// XInput player index reported by QUERY_IDS (`None` = 0xFF, not assigned).
    pub xinput_player_index: Option<u8>,
    /// LedWiz unit mask reported by QUERY_IDS (bit 0 = LedWiz unit 1).
    pub led_wiz_unit_mask: u16,
    /// `USBInterfaces::flags` reported by QUERY_USBIFCS.
    pub usb_flags: u16,
    /// Stored config files by file ID.
    pub configs: HashMap<u8, Vec<u8>>,
    /// Pending log text (drained by QUERY_LOG).
    pub log: VecDeque<u8>,
    /// Last wall-clock time set by the host.
    pub clock: Option<proto::ClockArgs>,
    /// Last reset mode requested; the device "disconnects" after a reset.
    pub last_reset: Option<u8>,
    pub disconnected: bool,
    /// When set, the next reply is preceded by a stale reply with a wrong token
    /// (and some extra data), to exercise the client's resync logic.
    pub inject_stale_reply: bool,
    /// Requests received, by command code (for assertions).
    pub requests: Vec<u8>,
    started: Instant,
    // config PUT in progress: (file id, total pages, crc, received pages)
    put: Option<(u8, u16, u32, Vec<u8>, u16)>,
    /// Phase 3 feature state (see mock/buttons.rs, mock/outputs.rs, mock/ir.rs).
    pub buttons: buttons::State,
    pub outputs: outputs::State,
    pub ir: ir::State,
    pub nudge: nudge::State,
    pub plunger: plunger::State,
}

impl Default for MockState {
    fn default() -> Self {
        let mut log = VecDeque::new();
        log.extend(b"\x1b[36mConfig:\x1b[0m  Mock device ready\nInfo:    Simulated Pinscape Pico\n".iter());
        MockState {
            hwid: [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x01],
            unit_num: 9,
            unit_name: "Mock Pico".into(),
            version: (1, 0, 2),
            xinput_player_index: None,
            led_wiz_unit_mask: 0x0001,
            usb_flags: 0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0100 | 0x1000,
            configs: HashMap::new(),
            log,
            clock: None,
            last_reset: None,
            disconnected: false,
            inject_stale_reply: false,
            requests: Vec::new(),
            started: Instant::now(),
            put: None,
            buttons: Default::default(),
            outputs: Default::default(),
            ir: Default::default(),
            nudge: Default::default(),
            plunger: Default::default(),
        }
    }
}

/// A set of `n` simulated devices (1..=8) for exercising the multi-device
/// dashboard (`PINSCAPE_MOCK=N`). `fleet(1)` is the single default device that
/// `PINSCAPE_MOCK=1` has always shown. From two devices on, the set contains
/// the cross-device problems the dashboard flags:
///
/// | # | unit | name            | firmware | notes |
/// |---|------|-----------------|----------|-------|
/// | 0 | 9    | Mock Pico       | 1.0.2    | the default device |
/// | 1 | 1    | Mock Flippers   | 1.0.2    | config stored, XInput player 1, LedWiz units 1+2 |
/// | 2 | 9    | Mock Spare      | 1.0.1    | duplicate unit 9, older firmware, output test mode on, LedWiz unit 2 (overlap) |
/// | 3 | 1    | Pinscape        | 1.0.2    | config stored but default identity (looks like Safe Mode), duplicate unit 1 |
/// | 4+| 10+  | Mock Pico N     | 1.0.2    | unique, no problems |
pub fn fleet(n: usize) -> Vec<MockState> {
    const LEDWIZ_CONF: u16 = 0x0200;
    const XINPUT: u16 = 0x0010 | 0x0020;
    let base = MockState::default();
    let hwid = |i: usize| {
        let mut h = base.hwid;
        h[7] = (i + 1) as u8;
        h
    };
    let config = |unit: u8, name: &str| format!("{{ id: {{ unitNum: {unit}, unitName: \"{name}\" }} }}\n").into_bytes();
    (0..n.clamp(1, 8))
        .map(|i| {
            let mut s = MockState { hwid: hwid(i), ..MockState::default() };
            match i {
                0 => {}
                1 => {
                    s.unit_num = 1;
                    s.unit_name = "Mock Flippers".into();
                    s.xinput_player_index = Some(0);
                    s.led_wiz_unit_mask = 0x0003;
                    s.usb_flags |= XINPUT | LEDWIZ_CONF;
                    s.configs.insert(proto::config::FILE_MAIN, config(1, "Mock Flippers"));
                    s.outputs = outputs::State::unconfigured();
                }
                2 => {
                    s.unit_name = "Mock Spare".into();
                    s.version = (1, 0, 1);
                    s.led_wiz_unit_mask = 0x0002;
                    s.usb_flags |= LEDWIZ_CONF;
                    s.outputs.test_mode = true;
                }
                3 => {
                    s.unit_num = 1;
                    s.unit_name = "Pinscape".into();
                    s.configs.insert(proto::config::FILE_MAIN, config(3, "Mock Backbox"));
                }
                _ => {
                    s.unit_num = 6 + i as u8;
                    s.unit_name = format!("Mock Pico {}", i + 1);
                }
            }
            s
        })
        .collect()
}

/// Transport implementation backed by [`MockState`].
pub struct MockDevice {
    pub state: Arc<Mutex<MockState>>,
    out_buf: Vec<u8>,
    in_buf: VecDeque<u8>,
}

impl MockDevice {
    pub fn new() -> Self {
        Self::with_state(MockState::default())
    }

    pub fn with_state(state: MockState) -> Self {
        MockDevice { state: Arc::new(Mutex::new(state)), out_buf: Vec::new(), in_buf: VecDeque::new() }
    }

    /// Handle for inspecting/mutating state after the device is boxed into a client.
    pub fn handle(&self) -> Arc<Mutex<MockState>> {
        self.state.clone()
    }

    pub(crate) fn reply(&mut self, token: u32, cmd: u8, status: Status, args: &[u8], data: &[u8]) {
        let mut b = [0u8; RESPONSE_LEN];
        b[0..4].copy_from_slice(&token.to_le_bytes());
        b[4] = cmd;
        b[5] = args.len() as u8;
        b[6..8].copy_from_slice(&(status as u16).to_le_bytes());
        b[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
        b[12..12 + args.len()].copy_from_slice(args);
        self.in_buf.extend(b);
        self.in_buf.extend(data);
    }

    /// Try to process one complete request from the OUT buffer.
    fn process(&mut self) {
        loop {
            if self.out_buf.len() < REQUEST_LEN {
                return;
            }
            let h = &self.out_buf[..REQUEST_LEN];
            let req = Request {
                token: u32::from_le_bytes(h[0..4].try_into().unwrap()),
                cmd: h[8],
                args_size: h[9],
                xfer_bytes: u16::from_le_bytes([h[10], h[11]]),
                args: h[12..28].try_into().unwrap(),
            };
            let checksum = u32::from_le_bytes(h[4..8].try_into().unwrap());
            let total = REQUEST_LEN + req.xfer_bytes as usize;
            if self.out_buf.len() < total {
                return; // wait for the extra data
            }
            let xfer: Vec<u8> = self.out_buf[REQUEST_LEN..total].to_vec();
            self.out_buf.drain(..total);
            if checksum != req.checksum() {
                continue; // firmware ignores malformed requests
            }
            self.handle_request(&req, &xfer);
        }
    }

    fn handle_request(&mut self, req: &Request, xfer: &[u8]) {
        let a = &req.args[..(req.args_size as usize).min(ARGS_LEN)];
        let arg = |i: usize| a.get(i).copied().unwrap_or(0);
        let stale = {
            let mut st = self.state.lock().unwrap();
            st.requests.push(req.cmd);
            std::mem::take(&mut st.inject_stale_reply)
        };
        if stale {
            self.reply(req.token.wrapping_sub(1000), req.cmd, Status::Ok, &[], b"stale!");
        }
        let (t, c) = (req.token, req.cmd);
        match req.cmd {
            cmd::PING => self.reply(t, c, Status::Ok, &[], &[]),
            cmd::QUERY_VERSION => {
                let st = self.state.lock().unwrap();
                let mut args = vec![st.version.0, st.version.1, st.version.2];
                args.extend_from_slice(b"202611080000");
                drop(st);
                self.reply(t, c, Status::Ok, &args, &[]);
            }
            cmd::QUERY_IDS => {
                let st = self.state.lock().unwrap();
                let mut args = st.hwid.to_vec();
                args.extend_from_slice(&2040u16.to_le_bytes());
                args.extend_from_slice(&[2, 3, st.unit_num, st.xinput_player_index.unwrap_or(0xFF)]);
                args.extend_from_slice(&st.led_wiz_unit_mask.to_le_bytes());
                let data = format!("{}\0pico\x002.2.0\x000.18.0\0GNUC 14.3.1\0", st.unit_name).into_bytes();
                drop(st);
                self.reply(t, c, Status::Ok, &args, &data);
            }
            cmd::QUERY_USBIFCS => {
                let flags = self.state.lock().unwrap().usb_flags;
                let mut d = vec![8, 0];
                d.extend_from_slice(&proto::DEFAULT_VID.to_le_bytes());
                d.extend_from_slice(&proto::DEFAULT_PIDS[1].to_le_bytes());
                d.extend_from_slice(&flags.to_le_bytes());
                self.reply(t, c, Status::Ok, &[], &d);
            }
            cmd::STATS if arg(0) == proto::stats::QUERY_STATS => {
                let up = self.state.lock().unwrap().started.elapsed().as_micros() as u64;
                let mut d = vec![0u8; 84];
                d[0..2].copy_from_slice(&84u16.to_le_bytes());
                d[8..16].copy_from_slice(&up.to_le_bytes());
                d[24..32].copy_from_slice(&(up / 40).to_le_bytes());
                d[32..36].copy_from_slice(&40u32.to_le_bytes());
                d[36..40].copy_from_slice(&900u32.to_le_bytes());
                d[64..68].copy_from_slice(&209_556u32.to_le_bytes());
                d[68..72].copy_from_slice(&163_840u32.to_le_bytes());
                self.reply(t, c, Status::Ok, &[], &d);
            }
            cmd::QUERY_GPIO_CONFIG => {
                let d = gpio_config_reply();
                self.reply(t, c, Status::Ok, &[], &d);
            }
            cmd::SET_CLOCK => {
                let y = i16::from_le_bytes([arg(0), arg(1)]);
                self.state.lock().unwrap().clock = Some(proto::ClockArgs {
                    year: y,
                    month: arg(2),
                    day: arg(3),
                    hour: arg(4),
                    minute: arg(5),
                    second: arg(6),
                });
                self.reply(t, c, Status::Ok, &[], &[]);
            }
            cmd::QUERY_LOG => {
                let mut st = self.state.lock().unwrap();
                if st.log.is_empty() {
                    drop(st);
                    self.reply(t, c, Status::Eof, &[], &[]);
                } else {
                    let n = st.log.len().min(1000);
                    let chunk: Vec<u8> = st.log.drain(..n).collect();
                    let avail = st.log.len() as u32;
                    drop(st);
                    self.reply(t, c, Status::Ok, &avail.to_le_bytes(), &chunk);
                }
            }
            cmd::CONFIG => self.handle_config(t, a, xfer),
            cmd::BUTTONS => buttons::handle(self, t, c, a, xfer),
            cmd::OUTPUTS => outputs::handle(self, t, c, a, xfer),
            cmd::QUERY_IR | cmd::SEND_IR | cmd::TVON => ir::handle(self, t, c, a, xfer),
            cmd::NUDGE => nudge::handle(self, t, c, a, xfer),
            cmd::PLUNGER => plunger::handle(self, t, c, a, xfer),
            cmd::RESET => {
                let mut st = self.state.lock().unwrap();
                st.last_reset = Some(arg(0));
                st.disconnected = true; // device drops off the bus; no reply
            }
            _ => self.reply(t, c, Status::BadCmd, &[], &[]),
        }
    }

    fn handle_config(&mut self, t: u32, a: &[u8], xfer: &[u8]) {
        let c = cmd::CONFIG;
        let g = |i: usize| a.get(i).copied().unwrap_or(0);
        let (subcmd, fid) = (g(0), g(1));
        let page = u16::from_le_bytes([g(2), g(3)]);
        let n_pages = u16::from_le_bytes([g(4), g(5)]);
        let crc = u32::from_le_bytes([g(8), g(9), g(10), g(11)]);
        let mut st = self.state.lock().unwrap();
        let status = match subcmd {
            proto::config::GET => match st.configs.get(&fid) {
                None => Status::NotFound,
                Some(file) => {
                    let start = page as usize * proto::config::PAGE_SIZE;
                    if start >= file.len() {
                        Status::Eof
                    } else {
                        let end = (start + proto::config::PAGE_SIZE).min(file.len());
                        let chunk = file[start..end].to_vec();
                        drop(st);
                        self.reply(t, c, Status::Ok, &[], &chunk);
                        return;
                    }
                }
            },
            proto::config::PUT if page == 0xFFFF => {
                st.put = Some((fid, n_pages, crc, Vec::new(), 0));
                Status::Ok
            }
            proto::config::PUT => match st.put.as_mut() {
                Some((pf, pn, pcrc, buf, next)) if *pf == fid && *pn == n_pages && *pcrc == crc => {
                    if page + 1 == *next {
                        Status::RetryOk
                    } else if page != *next {
                        Status::BadParams
                    } else {
                        buf.extend_from_slice(xfer);
                        *next += 1;
                        if *next == *pn {
                            let (fid, _, crc, data, _) = st.put.take().unwrap();
                            if proto::crc32(&data) == crc {
                                st.configs.insert(fid, data);
                                Status::Ok
                            } else {
                                Status::ConfigInvalid
                            }
                        } else {
                            Status::Ok
                        }
                    }
                }
                _ => Status::BadParams,
            },
            proto::config::EXISTS => {
                if st.configs.contains_key(&fid) {
                    Status::Ok
                } else {
                    Status::NotFound
                }
            }
            proto::config::ERASE => {
                if fid == proto::config::FILE_ALL {
                    st.configs.clear();
                } else {
                    st.configs.remove(&fid);
                }
                Status::Ok
            }
            proto::config::RESET => {
                st.configs.clear();
                Status::Ok
            }
            _ => Status::BadSubcmd,
        };
        drop(st);
        self.reply(t, c, status, &[], &[]);
    }
}

impl Default for MockDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// Simulated GPIO assignments (gpio, function, flags, usage), matching the
/// simulated devices: buttons on GP4-GP8 and a 74HC165 chain (mock/buttons.rs),
/// IR receiver/transmitter (mock/ir.rs), output ports on GP15-GP17 and a
/// TLC5940 chain (mock/outputs.rs), the potentiometer plunger on the Pico ADC
/// (mock/plunger.rs), an I2C bus with the accelerometer's interrupt line
/// (mock/nudge.rs), a logger UART, and the Pico LED. GP14, GP27 and GP28 stay
/// free. Usage names are the firmware GPIO manager's. Function codes: 2 UART,
/// 3 I2C, 4 PWM, 5 SIO, 6 PIO0; flags: 0x01 output, 0x02 ADC.
const MOCK_GPIOS: [(usize, u8, u8, &str); 24] = [
    (0, 2, 0, "UART0 TX"),
    (1, 5, 0, "MMA8451Q (Interrupt)"),
    (2, 3, 0, "I2C1 SDA"),
    (3, 3, 0, "I2C1 SCL"),
    (4, 5, 0, "Button #0"),
    (5, 5, 0, "Button #1"),
    (6, 5, 0, "Button #2"),
    (7, 5, 0, "Button #4"),
    (8, 5, 0, "Button #5"),
    (9, 5, 0, "IR RX"),
    (10, 4, 0, "IR TX"),
    (11, 5, 1, "74HC165[0] (SH/LD)"),
    (12, 5, 1, "74HC165[0] (CLK)"),
    (13, 5, 0, "74HC165[0] (QH)"),
    (15, 4, 0, "Output #1 (Left Flipper)"),
    (16, 4, 0, "Output #2 (Right Flipper)"),
    (17, 4, 0, "Output #3 (Knocker)"),
    (18, 5, 1, "TLC5940[0] (BLANK)"),
    (19, 4, 0, "TLC5940[0] (GSCLK)"),
    (20, 6, 0, "TLC5940[0] (SCLK)"),
    (21, 5, 1, "TLC5940[0] (XLAT)"),
    (22, 6, 0, "TLC5940[0] (SIN)"),
    (25, 5, 1, "Pico LED"),
    (26, 0x1F, 2, "Pico ADC"),
];

/// `GPIOConfig` reply for QUERY_GPIO_CONFIG: header, 30 ports, string pool.
fn gpio_config_reply() -> Vec<u8> {
    let n = 30usize;
    let pool_at = 6 + n * 4;
    let mut ports: Vec<u8> = Vec::new();
    let mut pool: Vec<u8> = Vec::new();
    for i in 0..n {
        let (func, flags, ofs) = match MOCK_GPIOS.iter().find(|(g, ..)| *g == i) {
            Some(&(_, func, flags, name)) => {
                let ofs = (pool_at + pool.len()) as u16;
                pool.extend_from_slice(name.as_bytes());
                pool.push(0);
                (func, flags, ofs)
            }
            None => (0x1F, 0, 0),
        };
        ports.extend_from_slice(&[func, flags]);
        ports.extend_from_slice(&ofs.to_le_bytes());
    }
    let mut d = vec![6, 0, 4, 0, n as u8, 0];
    d.extend_from_slice(&ports);
    d.extend_from_slice(&pool);
    d
}

fn disconnected() -> Error {
    Error::Transfer(nusb::transfer::TransferError::Disconnected)
}

impl Transport for MockDevice {
    fn write(&mut self, data: &[u8]) -> Result<()> {
        if self.state.lock().unwrap().disconnected {
            return Err(disconnected());
        }
        self.out_buf.extend_from_slice(data);
        self.process();
        Ok(())
    }

    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>> {
        if self.in_buf.len() < n {
            return Err(if self.state.lock().unwrap().disconnected { disconnected() } else { Error::Timeout });
        }
        Ok(self.in_buf.drain(..n).collect())
    }

    fn set_timeout(&mut self, _timeout: Duration) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VendorClient;
    use proto::config::{FILE_MAIN, FILE_SAFE_MODE};

    fn client() -> (VendorClient, Arc<Mutex<MockState>>) {
        let dev = MockDevice::new();
        let h = dev.handle();
        (VendorClient::with_transport(Box::new(dev)), h)
    }

    #[test]
    fn identify() {
        let (mut c, _) = client();
        let ids = c.query_ids().unwrap();
        assert_eq!(ids.hwid, "DEADBEEF00000001");
        assert_eq!(ids.unit_name, "Mock Pico");
        assert_eq!(c.query_version().unwrap().to_string(), "1.0.2 (build 202611080000)");
    }

    #[test]
    fn config_roundtrip_multi_page() {
        let (mut c, h) = client();
        assert_eq!(c.get_config(FILE_MAIN).unwrap(), None);
        assert!(!c.config_exists(FILE_MAIN).unwrap());
        // 65155 bytes = 16 pages, last one partial (same size as the Starter template)
        let data: Vec<u8> = (0..65155u32).map(|i| b"{ abc: 123, }\n"[(i % 14) as usize]).collect();
        c.put_config(FILE_MAIN, &data).unwrap();
        assert!(c.config_exists(FILE_MAIN).unwrap());
        assert_eq!(c.get_config(FILE_MAIN).unwrap().as_deref(), Some(&data[..]));
        assert_eq!(c.get_config(FILE_SAFE_MODE).unwrap(), None);
        assert_eq!(h.lock().unwrap().configs[&FILE_MAIN].len(), 65155);
    }

    #[test]
    fn config_exact_page_multiple() {
        let (mut c, _) = client();
        let data = vec![b'x'; 8192];
        c.put_config(FILE_MAIN, &data).unwrap();
        assert_eq!(c.get_config(FILE_MAIN).unwrap().unwrap().len(), 8192);
    }

    #[test]
    fn erase() {
        let (mut c, _) = client();
        c.put_config(FILE_MAIN, b"{}").unwrap();
        c.put_config(FILE_SAFE_MODE, b"{}").unwrap();
        c.erase_config(proto::config::FILE_ALL).unwrap();
        assert!(!c.config_exists(FILE_MAIN).unwrap());
        assert!(!c.config_exists(FILE_SAFE_MODE).unwrap());
    }

    #[test]
    fn skips_stale_replies() {
        let (mut c, h) = client();
        h.lock().unwrap().inject_stale_reply = true;
        c.ping().unwrap();
        h.lock().unwrap().inject_stale_reply = true;
        assert_eq!(c.query_ids().unwrap().unit_num, 9);
    }

    #[test]
    fn log_drain() {
        let (mut c, _) = client();
        let (text, _) = c.query_log().unwrap().unwrap();
        assert!(String::from_utf8_lossy(&text).contains("Mock device ready"));
        assert_eq!(c.query_log().unwrap(), None);
    }

    #[test]
    fn stats_usb_gpio_clock() {
        let (mut c, h) = client();
        assert_eq!(c.query_stats(false).unwrap().heap_size, 209_556);
        assert!(c.query_usb_interfaces().unwrap().keyboard_enabled);
        let gpio = c.query_gpio_config().unwrap();
        assert_eq!(gpio.len(), 30);
        assert_eq!(gpio[4].usage.as_deref(), Some("Button #0"));
        assert_eq!(gpio[8].usage.as_deref(), Some("Button #5"));
        assert_eq!(gpio[26].usage.as_deref(), Some("Pico ADC"));
        assert!(gpio[26].adc && gpio[25].output);
        assert_eq!(gpio[14].usage, None);
        let t = proto::ClockArgs { year: 2026, month: 11, day: 8, hour: 1, minute: 2, second: 3 };
        c.set_clock(t).unwrap();
        assert_eq!(h.lock().unwrap().clock, Some(t));
    }

    #[test]
    fn gpio_pinout_covers_every_category() {
        use proto::pinout::{Category as C, Pinout};
        let (mut c, _) = client();
        let p = Pinout::build(&c.query_ids().unwrap(), &c.query_gpio_config().unwrap());
        assert_eq!(p.board, Some("Raspberry Pi Pico"));
        assert_eq!(p.header.len(), 40);
        let cat = |g: usize| p.pins[g].category;
        assert_eq!(
            [cat(0), cat(1), cat(2), cat(4), cat(9), cat(13), cat(15), cat(20), cat(25), cat(26), cat(14)],
            [C::Uart, C::Accel, C::I2c, C::Button, C::Ir, C::Button, C::Output, C::Output, C::System, C::Plunger, C::Free]
        );
        assert_eq!(p.free, vec![14, 27, 28]);
        assert!(p.pins.iter().all(|x| x.warning.is_none()));
    }

    #[test]
    fn reset_disconnects() {
        let (mut c, h) = client();
        c.reset(proto::reset::NORMAL).unwrap();
        assert_eq!(h.lock().unwrap().last_reset, Some(proto::reset::NORMAL));
        let e = c.ping().unwrap_err();
        assert!(e.is_disconnect(), "{e:?}");
    }

    #[test]
    fn fleet_of_one_is_the_default_device() {
        let f = fleet(1);
        assert_eq!(f.len(), 1);
        let mut c = VendorClient::with_transport(Box::new(MockDevice::with_state(f.into_iter().next().unwrap())));
        let ids = c.query_ids().unwrap();
        assert_eq!((ids.hwid.as_str(), ids.unit_num, ids.unit_name.as_str()), ("DEADBEEF00000001", 9, "Mock Pico"));
        assert_eq!(ids.xinput_player_index, None);
        assert_eq!(ids.led_wiz_unit_mask, 1);
        assert!(!c.query_usb_interfaces().unwrap().ledwiz_configured);
    }

    #[test]
    fn fleet_devices_differ_and_include_problem_cases() {
        assert_eq!(fleet(0).len(), 1);
        assert_eq!(fleet(99).len(), 8);
        let ids: Vec<_> = fleet(8)
            .into_iter()
            .map(|s| {
                let mut c = VendorClient::with_transport(Box::new(MockDevice::with_state(s)));
                (c.query_ids().unwrap(), c.query_version().unwrap(), c.query_usb_interfaces().unwrap(), c)
            })
            .collect();
        let hwids: std::collections::HashSet<_> = ids.iter().map(|(i, ..)| i.hwid.clone()).collect();
        assert_eq!(hwids.len(), 8, "hardware IDs must be unique");
        assert_eq!(ids[0].0.unit_num, ids[2].0.unit_num, "duplicate unit number case");
        assert_ne!(ids[0].1, ids[2].1, "firmware mismatch case");
        assert_eq!(ids[1].0.xinput_player_index, Some(0));
        assert!(ids[1].2.xinput_configured && ids[1].2.ledwiz_configured);
        assert_eq!(ids[1].0.led_wiz_unit_mask & ids[2].0.led_wiz_unit_mask, 0x0002);
        assert_eq!(ids[4].0.unit_num, 10);
        let (.., mut c2) = ids.into_iter().nth(2).unwrap();
        assert!(c2.query_logical_port_levels().unwrap().test_mode);
    }

    #[test]
    fn unknown_command_is_status_error() {
        let (mut c, _) = client();
        let e = c.transact(0x7F, &[], &[]).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadCmd));
    }
}
