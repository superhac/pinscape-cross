//! Simulated-device handling for CMD_BUTTONS (0x10): button descriptors,
//! logical button states, GPIO / PCA9555 / 74HC165 input states, shift state.
//!
//! The simulated cabinet has buttons on Pico GPIOs, one PCA9555 and a
//! two-chip 74HC165 chain, plus a shift button, BOOTSEL and an IR source.
//! "Presses" follow a deterministic schedule derived from elapsed time so a
//! UI polling the device sees activity.

use std::collections::HashMap;
use std::time::Instant;

use pinscape_protocol::buttons::{
    action as a, btn_type as t, encode_event_log, src, subcmd, ButtonConfig, ButtonDesc, ButtonDevice, ButtonEvent, FLAG_EVENTLOG, ALL_GPIOS,
};
use pinscape_protocol::Status;

use super::MockDevice;

/// Number of GPIO bytes the firmware reports (`gpio_get_all()`, 32 bits).
const NUM_GPIO_BYTES: usize = 32;

/// Per-feature simulated state (lives in `MockState::buttons`).
#[derive(Debug, Clone)]
pub struct State {
    pub config: ButtonConfig,
    /// Extra zero bytes appended to each descriptor (simulates newer firmware).
    pub desc_pad: usize,
    /// Fixed simulation time in ms (tests); `None` uses elapsed real time.
    pub time_ms: Option<u64>,
    /// Event log read position per GPIO (simulation ms up to which edges were reported).
    pub event_cursor: HashMap<u8, u64>,
    started: Instant,
}

/// The firmware's ring buffer holds `MaxEvents` (64) - 1 events.
pub const EVENT_LOG_CAPACITY: usize = 63;

fn btn(bt: u8, source: u8, unit: u8, port: u8, act: u8, detail: u8) -> ButtonDesc {
    ButtonDesc {
        btn_type: bt,
        source_type: source,
        source_unit: unit,
        source_port: port,
        source_active_high: false,
        action_type: act,
        action_detail: detail,
        ..Default::default()
    }
}

fn logged(mut d: ButtonDesc) -> ButtonDesc {
    d.flags |= FLAG_EVENTLOG;
    d
}

fn shifted(mut d: ButtonDesc, mask: u32, bits: u32) -> ButtonDesc {
    d.shift_mask = mask;
    d.shift_bits = bits;
    d
}

impl Default for State {
    fn default() -> Self {
        let buttons = vec![
            logged(btn(t::PUSH, src::GPIO, 0, 4, a::KEY, 0xE1)), // Left flipper: Left Shift (event-logged)
            logged(btn(t::PUSH, src::GPIO, 0, 5, a::KEY, 0xE5)), // Right flipper: Right Shift (event-logged)
            shifted(btn(t::PUSH, src::GPIO, 0, 6, a::KEY, 0x1E), 1, 0), // Start: "1" (unshifted)
            shifted(btn(t::PUSH, src::GPIO, 0, 6, a::KEY, 0x29), 1, 1), // Start+Shift: Esc
            btn(t::PUSH, src::GPIO, 0, 7, a::KEY, 0x22),  // Coin: "5"
            ButtonDesc { shift_bits: 1, ..btn(t::SHIFT, src::GPIO, 0, 8, a::NONE, 0) }, // Shift (ExtraBall)
            btn(t::PUSH, src::PCA9555, 0, 8, a::KEY, 0x28), // Launch: Enter (IO1_0)
            btn(t::PUSH, src::PCA9555, 0, 3, a::KEY, 0x3A), // F1 (IO0_3)
            ButtonDesc { source_active_high: true, ..btn(t::TOGGLE, src::PCA9555, 0, 13, a::NIGHTMODE, 0) },
            btn(t::PUSH, src::HC165, 0, 2, a::MEDIA, 0xE9),  // Volume up
            btn(t::PUSH, src::HC165, 0, 3, a::MEDIA, 0xEA),  // Volume down
            btn(t::PUSH, src::HC165, 0, 9, a::XINPUT, 12),   // XInput A
            btn(t::PULSE, src::HC165, 0, 12, a::GAMEPAD, 3), // Gamepad button 3
            btn(t::PUSH, src::BOOTSEL, 0, 0, a::RESET, 0),
            btn(t::PUSH, src::IR, 0, 0, a::KEY, 0x52), // IR remote: Up arrow
            btn(t::PUSH, src::ZBLAUNCH, 0, 0, a::GAMEPAD, 100), // ZB Launch: hat up
        ];
        let devices = vec![
            ButtonDevice { config_index: 0, dev_type: src::PCA9555, num_ports: 16, addr: 0x20 },
            ButtonDevice { config_index: 0, dev_type: src::HC165, num_ports: 16, addr: 0 },
        ];
        State { config: ButtonConfig { buttons, devices }, desc_pad: 0, time_ms: None, event_cursor: HashMap::new(), started: Instant::now() }
    }
}

impl State {
    fn now_ms(&self) -> u64 {
        self.time_ms.unwrap_or_else(|| self.started.elapsed().as_millis() as u64)
    }

    /// Whether the input `(source, unit, port)` is physically pressed at time `ms`.
    /// Each input gets its own period and phase; the shift button is held longer.
    pub fn pressed(source: u8, unit: u8, port: u8, ms: u64) -> bool {
        let k = (source as u64) * 37 + (unit as u64) * 11 + port as u64;
        let (period, hold) = match (source, port) {
            (src::GPIO, 8) => (7000, 2600),     // shift button
            (src::GPIO, 6) => (3100, 700),      // start (overlaps shift sometimes)
            (src::BOOTSEL, _) => (11000, 500),
            (src::IR, _) => (4300, 150),
            (src::ZBLAUNCH, _) => (5300, 400),
            _ => (1700 + (k % 13) * 230, 250 + (k % 5) * 90),
        };
        (ms + k * 397) % period < hold
    }

    fn raw_level(&self, source: u8, unit: u8, port: u8, ms: u64, idle: bool) -> bool {
        match self
            .config
            .buttons
            .iter()
            .find(|b| b.source_type == source && b.source_unit == unit && b.source_port == port)
        {
            Some(b) => Self::pressed(source, unit, port, ms) == b.source_active_high,
            None => idle,
        }
    }

    pub fn gpio_levels(&self, ms: u64) -> Vec<u8> {
        (0..NUM_GPIO_BYTES as u8).map(|p| self.raw_level(src::GPIO, 0, p, ms, false) as u8).collect()
    }

    fn device_levels(&self, dev_type: u8, ms: u64) -> Vec<u8> {
        let mut out = Vec::new();
        for (unit, d) in self.config.devices.iter().filter(|d| d.dev_type == dev_type).enumerate() {
            // unused inputs on these chips idle HIGH (pull-ups)
            out.extend((0..d.num_ports).map(|p| self.raw_level(dev_type, unit as u8, p as u8, ms, true) as u8));
        }
        out
    }

    /// True if a GPIO button on `gpio` has event logging enabled.
    pub fn event_logged(&self, gpio: u8) -> bool {
        self.config.buttons.iter().any(|b| b.source_type == src::GPIO && b.source_port == gpio && b.flags & FLAG_EVENTLOG != 0)
    }

    /// Press/release edges of `gpio` in the simulated-time window `(from, to]` ms,
    /// newest `EVENT_LOG_CAPACITY` only, timestamped in µs with sub-ms detail.
    pub fn edges(gpio: u8, from: u64, to: u64) -> Vec<ButtonEvent> {
        let from = from.max(to.saturating_sub(300_000)); // don't scan forever after a long idle
        let mut out = Vec::new();
        let mut prev = Self::pressed(src::GPIO, 0, gpio, from);
        for ms in from + 1..=to {
            let p = Self::pressed(src::GPIO, 0, gpio, ms);
            if p != prev {
                out.push(ButtonEvent { t: ms * 1000 + (ms * 37 + gpio as u64 * 101) % 1000, pressed: p });
                prev = p;
            }
        }
        let n = out.len().saturating_sub(EVENT_LOG_CAPACITY);
        out.split_off(n)
    }

    /// Global shift state: the bits of every pressed shift button.
    pub fn shift_state(&self, ms: u64) -> u32 {
        self.config
            .buttons
            .iter()
            .filter(|b| b.btn_type == t::SHIFT && Self::pressed(b.source_type, b.source_unit, b.source_port, ms))
            .fold(0, |s, b| s | b.shift_bits)
    }

    /// Logical states: physical press gated by the shift mask/bits.
    /// (Toggle buttons are approximated as following the toggle period.)
    pub fn logical_states(&self, ms: u64) -> Vec<u8> {
        let shift = self.shift_state(ms);
        self.config
            .buttons
            .iter()
            .map(|b| {
                let p = Self::pressed(b.source_type, b.source_unit, b.source_port, ms);
                let on = match b.btn_type {
                    t::SHIFT => p,
                    t::TOGGLE => (ms / 9000) % 2 == 1,
                    _ => p && (shift & b.shift_mask) == b.shift_bits,
                };
                on as u8
            })
            .collect()
    }
}

/// Handle a request for this feature's command(s). `args` is the meaningful
/// part of the request's argument bytes; `xfer` is any extra OUT data.
pub(super) fn handle(dev: &mut MockDevice, token: u32, cmd: u8, args: &[u8], xfer: &[u8]) {
    let _ = xfer;
    let st = dev.state.lock().unwrap().buttons.clone();
    let ms = st.now_ms();
    let (status, rargs, data) = match args.first().copied() {
        Some(subcmd::QUERY_DESCS) => (Status::Ok, vec![], st.config.encode(st.desc_pad)),
        Some(subcmd::QUERY_STATES) => {
            (Status::Ok, st.shift_state(ms).to_le_bytes().to_vec(), st.logical_states(ms))
        }
        Some(subcmd::QUERY_GPIO_STATES) => (Status::Ok, vec![], st.gpio_levels(ms)),
        Some(subcmd::QUERY_PCA9555_STATES) => (Status::Ok, vec![], st.device_levels(src::PCA9555, ms)),
        Some(subcmd::QUERY_74HC165_STATES) => (Status::Ok, vec![], st.device_levels(src::HC165, ms)),
        Some(subcmd::QUERY_EVENT_LOG) => {
            let gpio = args.get(1).copied().unwrap_or(0);
            if st.event_logged(gpio) {
                let from = st.event_cursor.get(&gpio).copied().unwrap_or(0);
                dev.state.lock().unwrap().buttons.event_cursor.insert(gpio, ms);
                (Status::Ok, vec![], encode_event_log(&State::edges(gpio, from, ms)))
            } else {
                (Status::Failed, vec![], vec![]) // the firmware returns no data -> ERR_FAILED
            }
        }
        Some(subcmd::CLEAR_EVENT_LOG) => {
            let gpio = args.get(1).copied().unwrap_or(0);
            let mut s = dev.state.lock().unwrap();
            let logged: Vec<u8> = (0..30u8).filter(|&g| st.event_logged(g) && (gpio == ALL_GPIOS || g == gpio)).collect();
            for g in logged {
                s.buttons.event_cursor.insert(g, ms);
            }
            // same quirk as the firmware: only the wildcard reports success
            (if gpio == ALL_GPIOS { Status::Ok } else { Status::Failed }, vec![], vec![])
        }
        _ => (Status::BadSubcmd, vec![], vec![]),
    };
    dev.reply(token, cmd, status, &rargs, &data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockState;
    use crate::VendorClient;
    use pinscape_protocol::cmd;

    fn client(st: State) -> VendorClient {
        let ms = MockState { buttons: st, ..Default::default() };
        VendorClient::with_transport(Box::new(MockDevice::with_state(ms)))
    }

    /// First time >= 0 at which `f` holds (searching 0..60 s in 10 ms steps).
    fn find_time(f: impl Fn(u64) -> bool) -> u64 {
        (0..6000).map(|i| i * 10).find(|&t| f(t)).expect("no matching time")
    }

    #[test]
    fn config_end_to_end() {
        let mut c = VendorClient::with_transport(Box::new(MockDevice::new()));
        let cfg = c.query_button_config().unwrap();
        assert_eq!(cfg, State::default().config);
        assert_eq!(cfg.devices.len(), 2);
        assert!(cfg.buttons.iter().any(|b| b.btn_type == t::SHIFT));
    }

    #[test]
    fn config_with_padded_descs() {
        let st = State { desc_pad: 12, ..Default::default() };
        let want = st.config.clone();
        assert_eq!(client(st).query_button_config().unwrap(), want);
    }

    #[test]
    fn empty_config() {
        let st = State { config: ButtonConfig::default(), time_ms: Some(0), ..Default::default() };
        let mut c = client(st);
        let cfg = c.query_button_config().unwrap();
        assert!(cfg.buttons.is_empty());
        let s = c.query_button_states().unwrap();
        assert!(s.states.is_empty());
        assert_eq!(s.shift_state, 0);
        assert!(c.query_button_pca9555_states().unwrap().is_empty());
        assert!(c.query_button_74hc165_states().unwrap().is_empty());
        assert_eq!(c.query_button_gpio_states().unwrap().len(), NUM_GPIO_BYTES);
    }

    #[test]
    fn physical_levels_follow_polarity() {
        // GP4 is active low: pressed => LOW
        let t_on = find_time(|t| State::pressed(src::GPIO, 0, 4, t));
        let t_off = find_time(|t| !State::pressed(src::GPIO, 0, 4, t));
        let mut c = client(State { time_ms: Some(t_on), ..Default::default() });
        let g = c.query_button_gpio_states().unwrap();
        assert!(!g[4]);
        assert!(!g[20], "unused GPIO idles low in the mock");
        let mut c = client(State { time_ms: Some(t_off), ..Default::default() });
        assert!(c.query_button_gpio_states().unwrap()[4]);

        // PCA9555 port 13 is active high
        let t13 = find_time(|t| State::pressed(src::PCA9555, 0, 13, t));
        let mut c = client(State { time_ms: Some(t13), ..Default::default() });
        let p = c.query_button_pca9555_states().unwrap();
        assert_eq!(p.len(), 16);
        assert!(p[13]);
        assert!(p[0], "unused PCA9555 inputs idle high");
        assert_eq!(c.query_button_74hc165_states().unwrap().len(), 16);
    }

    #[test]
    fn shift_gates_buttons() {
        // find a time where Start (GP6) is pressed with and without the shift button (GP8)
        let start = |t| State::pressed(src::GPIO, 0, 6, t);
        let shift = |t| State::pressed(src::GPIO, 0, 8, t);
        let t_unshifted = find_time(|t| start(t) && !shift(t));
        let t_shifted = find_time(|t| start(t) && shift(t));

        let mut c = client(State { time_ms: Some(t_unshifted), ..Default::default() });
        let s = c.query_button_states().unwrap();
        assert_eq!(s.shift_state, 0);
        assert!(s.states[2] && !s.states[3], "{s:?}");

        let mut c = client(State { time_ms: Some(t_shifted), ..Default::default() });
        let s = c.query_button_states().unwrap();
        assert_eq!(s.shift_state, 1);
        assert!(!s.states[2] && s.states[3] && s.states[5], "{s:?}");
        assert_eq!(s.states.len(), State::default().config.buttons.len());
    }

    #[test]
    fn states_change_over_time() {
        let st = State::default();
        let a = st.logical_states(0);
        assert!((1..200).any(|i| st.logical_states(i * 50) != a));
    }

    #[test]
    fn event_log_reports_each_edge_once() {
        let h = {
            let d = MockDevice::with_state(MockState { buttons: State { time_ms: Some(0), ..Default::default() }, ..Default::default() });
            let h = d.handle();
            (VendorClient::with_transport(Box::new(d)), h)
        };
        let (mut c, h) = h;
        let set_t = |t: u64| h.lock().unwrap().buttons.time_ms = Some(t);
        assert_eq!(c.query_button_event_log(4).unwrap(), Some(vec![]));
        set_t(10_000);
        let ev = c.query_button_event_log(4).unwrap().unwrap();
        assert!(ev.len() >= 4, "{ev:?}");
        assert!(ev.windows(2).all(|w| w[0].t < w[1].t && w[0].pressed != w[1].pressed), "alternating, ascending");
        assert!(ev.iter().all(|e| e.t <= 10_000_000));
        // matches the simulated schedule exactly
        for e in &ev {
            let ms = e.t / 1000;
            assert_eq!(State::pressed(src::GPIO, 0, 4, ms), e.pressed);
            assert_ne!(State::pressed(src::GPIO, 0, 4, ms - 1), e.pressed);
        }
        // read again at the same time: nothing new
        assert_eq!(c.query_button_event_log(4).unwrap(), Some(vec![]));
        // a GPIO without event logging, and one without any button
        assert_eq!(c.query_button_event_log(6).unwrap(), None);
        assert_eq!(c.query_button_event_log(20).unwrap(), None);
        // clear (single GPIO: firmware reports failure, client tolerates it), then nothing pending
        set_t(20_000);
        c.clear_button_event_log(Some(4)).unwrap();
        assert_eq!(c.query_button_event_log(4).unwrap(), Some(vec![]));
        assert!(!c.query_button_event_log(5).unwrap().unwrap().is_empty());
        set_t(30_000);
        c.clear_button_event_log(None).unwrap();
        assert_eq!(c.query_button_event_log(5).unwrap(), Some(vec![]));
        // long idle: capped to the firmware's buffer size
        set_t(300_000);
        assert_eq!(c.query_button_event_log(5).unwrap().unwrap().len(), EVENT_LOG_CAPACITY);
    }

    #[test]
    fn bad_subcommand() {
        let mut c = VendorClient::with_transport(Box::new(MockDevice::new()));
        let e = c.transact(cmd::BUTTONS, &[0x7E], &[]).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadSubcmd));
    }
}
