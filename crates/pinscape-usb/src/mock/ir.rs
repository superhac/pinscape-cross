//! Simulated-device handling for CMD_QUERY_IR (0x14), CMD_SEND_IR (0x0D), CMD_TVON (0x0A): received IR commands and raw pulses, sending IR, TV-ON state and relay. (Phase 3)
//!
//! The simulated receiver "hears" a short script of remote-control presses
//! every few seconds (NEC with dittos, Sony repeats, RC5 toggles, Samsung),
//! each with a plausible raw pulse train. Anything sent with SEND_IR is
//! looped back to the receiver, as if the IR LED were pointed at it. The TV
//! ON state machine runs through a power-on sequence after "boot" and then
//! cycles power periodically; the relay honors manual on/off/pulse requests.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use pinscape_protocol::ir::{
    pwr, IrCode, IrCommandReceived, IrRawPulse, SendIrArgs, TvOnState, FPRO_DITTOS, F_AUTOREPEAT, F_DITTO_FLAG,
    F_HAS_DITTO, F_HAS_TOGGLE, F_POS_FIRST, F_POS_LAST, F_POS_MIDDLE, F_TOGGLE_BIT, RELAY_STATE_MANUAL,
    RELAY_STATE_MANUAL_PULSE, RELAY_STATE_POWERON, SUBCMD_QUERY_IR_CMD, SUBCMD_QUERY_IR_RAW,
    SUBCMD_TVON_QUERY_STATE, SUBCMD_TVON_SET_RELAY, TVON_RELAY_OFF, TVON_RELAY_ON, TVON_RELAY_PULSE,
};
use pinscape_protocol::{cmd, Status};

use super::MockDevice;

/// Decoded-command ring buffer size (oldest entries are dropped).
const CMD_BUF: usize = 32;
/// Raw pulse buffer size (`irRx.bufferSize` default); later pulses are discarded.
const RAW_BUF: usize = 128;
/// TV relay manual pulse length.
const PULSE_TIME: Duration = Duration::from_millis(250);
/// Number of IR commands in the simulated `tvon.IR` list.
const TVON_IR_COUNT: u8 = 2;

/// Per-feature simulated state (lives in `MockState::ir`).
#[derive(Debug, Clone)]
pub struct State {
    /// `irRx` configured. When false, the receive queries return empty lists, as the firmware does.
    pub rx_configured: bool,
    /// `irTx` configured. When false, SEND_IR fails with `ERR_FAILED`, as the firmware does.
    pub tx_configured: bool,
    /// Periodically receive codes from a simulated remote.
    pub auto_receive: bool,
    /// Run the TV ON power-sense sequence; when false the state stays at Power Off.
    pub tvon_simulate: bool,
    /// Decoded commands waiting to be queried: (dt µs, code with protocol flags, cmdFlags).
    pub received: VecDeque<(u64, IrCode, u8)>,
    /// Raw pulses of the latest code, waiting to be queried.
    pub raw: Vec<IrRawPulse>,
    /// Commands accepted by SEND_IR, with their repeat counts (for assertions).
    pub sent: Vec<(IrCode, u8)>,
    /// Relay switched on manually.
    pub relay_manual: bool,
    pulse_until: Option<Instant>,
    last_rx: Option<Instant>,
    next_auto: Option<Instant>,
    script_pos: usize,
    toggle: bool,
    t0: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        State {
            rx_configured: true,
            tx_configured: true,
            auto_receive: true,
            tvon_simulate: true,
            received: VecDeque::new(),
            raw: Vec::new(),
            sent: Vec::new(),
            relay_manual: false,
            pulse_until: None,
            last_rx: None,
            next_auto: None,
            script_pos: 0,
            toggle: false,
            t0: None,
        }
    }
}

/// The simulated remote's script: (protocol, code, presses-with-repeats, use dittos).
const SCRIPT: &[(u8, u64, u8, bool)] = &[
    (1, 0x20DF_10EF, 1, true),  // NEC-32 (LG TV power)
    (1, 0x20DF_40BF, 4, true),  // NEC-32 held down: code + dittos
    (23, 0x0A90, 3, false),     // Sony-12, repeated frames
    (4, 0x100C, 2, false),      // Philips RC5 with toggle bit
    (2, 0xE0E0_40BF, 1, false), // NEC-32x (Samsung TV power)
    (27, 0x2C, 3, false),       // Ortek MCE with position codes
];

impl State {
    fn elapsed_us(&mut self, now: Instant) -> u64 {
        let base = self.last_rx.or(self.t0).unwrap_or(now);
        now.saturating_duration_since(base).as_micros() as u64
    }

    /// Simulate receiving `count` frames of `code` (a press held for `count - 1` repeats).
    fn receive(&mut self, now: Instant, protocol: u8, code: u64, count: u8, dittos: bool) {
        if !self.rx_configured {
            return;
        }
        let k = kind(protocol);
        let pro_flags = if k == Kind::NecDitto { FPRO_DITTOS } else { 0 };
        let rc = IrCode::new(protocol, pro_flags, code);
        let period_us = frame_period_us(protocol);
        let dittos = dittos && k == Kind::NecDitto;
        if count > 0 {
            self.toggle = !self.toggle; // a new key press flips the toggle bit
        }
        let mut dt = self.elapsed_us(now);
        for i in 0..count.max(1) {
            let repeat = i > 0;
            let mut f = 0u8;
            match k {
                Kind::NecDitto => {
                    f |= F_HAS_DITTO;
                    if repeat && dittos {
                        f |= F_DITTO_FLAG;
                    }
                }
                Kind::Toggle => {
                    f |= F_HAS_TOGGLE;
                    if self.toggle {
                        f |= F_TOGGLE_BIT;
                    }
                }
                Kind::Position => {
                    f |= match (i, count) {
                        (0, _) => F_POS_FIRST,
                        (i, n) if i + 1 == n => F_POS_LAST,
                        _ => F_POS_MIDDLE,
                    };
                }
                Kind::Plain => {}
            }
            if repeat {
                f |= F_AUTOREPEAT;
            }
            if self.received.len() >= CMD_BUF {
                self.received.pop_front();
            }
            self.received.push_back((dt, rc, f));
            dt = period_us;
        }
        self.raw = pulse_train(protocol, code, count.max(1), dittos);
        self.last_rx = Some(now);
    }

    /// Advance the simulated remote: deliver the next scripted press when due.
    fn tick(&mut self, now: Instant) {
        self.t0.get_or_insert(now);
        if !self.auto_receive {
            return;
        }
        match self.next_auto {
            None => self.next_auto = Some(now + Duration::from_millis(1500)),
            Some(t) if now >= t => {
                let (p, c, n, d) = SCRIPT[self.script_pos % SCRIPT.len()];
                self.script_pos += 1;
                self.receive(now, p, c, n, d);
                self.next_auto = Some(now + Duration::from_millis(3500));
            }
            Some(_) => {}
        }
    }

    /// (power state, gpio, extra relay bits, IR index) at `now`.
    fn tvon_phase(&mut self, now: Instant) -> (u8, u8, u8, u8) {
        let t0 = *self.t0.get_or_insert(now);
        if !self.tvon_simulate {
            return (pwr::OFF, 0, 0, 0);
        }
        let t = now.saturating_duration_since(t0).as_secs_f64();
        if t < 1.0 {
            return (pwr::PICOBOOT, 0, 0, 0);
        }
        // 50 s cycle: power-on sequence, on for a while, then off.
        let p = (t - 1.0) % 50.0;
        let ir_slot = 0.2 + 0.4 + 1.5; // ready, sending, waiting
        let ir_start = 7.1;
        let ir_end = ir_start + ir_slot * TVON_IR_COUNT as f64;
        match p {
            p if p < 0.3 => (pwr::PULSELATCH, 0, 0, 0),
            p if p < 0.6 => (pwr::TESTLATCH, 1, 0, 0),
            p if p < 6.6 => (pwr::COUNTDOWN, 1, 0, 0),
            p if p < ir_start => (pwr::RELAYON, 1, RELAY_STATE_POWERON, 0),
            p if p < ir_end => {
                let q = p - ir_start;
                let idx = (q / ir_slot) as u8;
                let r = q % ir_slot;
                let st = if r < 0.2 {
                    pwr::IRREADY
                } else if r < 0.6 {
                    pwr::IRSENDING
                } else {
                    pwr::IRWAITING
                };
                (st, 1, 0, idx)
            }
            p if p < 45.0 => (pwr::ON, 1, 0, 0),
            _ => (pwr::OFF, 0, 0, 0),
        }
    }

    fn tvon_args(&mut self, now: Instant) -> [u8; 5] {
        let (st, gpio, extra, idx) = self.tvon_phase(now);
        let mut relay = extra;
        if self.relay_manual {
            relay |= RELAY_STATE_MANUAL;
        }
        if self.pulse_until.is_some_and(|t| now < t) {
            relay |= RELAY_STATE_MANUAL_PULSE;
        }
        let count = if self.tvon_simulate { TVON_IR_COUNT } else { 0 };
        TvOnState::encode_args(st, gpio, relay, idx, count)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    NecDitto,
    Toggle,
    Position,
    Plain,
}

fn kind(protocol: u8) -> Kind {
    match protocol {
        1 | 3 => Kind::NecDitto,
        4 | 5 => Kind::Toggle,
        27 => Kind::Position,
        _ => Kind::Plain,
    }
}

fn frame_period_us(protocol: u8) -> u64 {
    match protocol {
        1..=3 | 19 | 29 => 108_000,
        22..=26 => 45_000,
        4 | 5 => 113_778,
        _ => 100_000,
    }
}

/// Build a plausible raw pulse train for `count` frames, capped at the buffer size.
fn pulse_train(protocol: u8, code: u64, count: u8, dittos: bool) -> Vec<IrRawPulse> {
    let mut v: Vec<(bool, u32)> = Vec::new();
    let mut push = |mark: bool, us: u32| match v.last_mut() {
        Some((m, t)) if *m == mark => *t += us,
        _ => v.push((mark, us)),
    };
    let bits_of = |n: u32| (0..n).rev().map(move |i| (code >> i) & 1 != 0);
    for i in 0..count {
        let mut len = 0u32;
        let mut emit = |mark: bool, us: u32| {
            len += us;
            push(mark, us);
        };
        match protocol {
            22..=26 => {
                // Sony SIRC: 2400 header, then (600 space, 1200/600 mark) per bit, LSB first
                let n = match protocol {
                    22 => 8,
                    23 => 12,
                    25 => 15,
                    _ => 20,
                };
                emit(true, 2400);
                for b in (0..n).map(|i| (code >> i) & 1 != 0) {
                    emit(false, 600);
                    emit(true, if b { 1200 } else { 600 });
                }
            }
            4 | 5 => {
                // RC5: Manchester, 889 µs half-bits; "1" = space then mark
                for b in bits_of(14) {
                    emit(!b, 889);
                    emit(b, 889);
                }
            }
            _ if i > 0 && dittos => {
                // NEC ditto
                emit(true, 9000);
                emit(false, 2250);
                emit(true, 560);
            }
            _ => {
                let n = if protocol == 3 { 48 } else if code > u32::MAX as u64 { 64 } else { 32 };
                emit(true, if protocol == 2 { 4500 } else { 9000 });
                emit(false, 4500);
                for b in bits_of(n) {
                    emit(true, 560);
                    emit(false, if b { 1690 } else { 560 });
                }
                emit(true, 560);
            }
        }
        if i + 1 < count {
            let gap = (frame_period_us(protocol) as u32).saturating_sub(len).max(5000);
            push(false, gap);
        }
    }
    // The receiver records the long quiet period before the code as an overflow
    // space (which absorbs a leading half-bit space, e.g. RC5's).
    if v.first().is_some_and(|(mark, _)| !mark) {
        v.remove(0);
    }
    let mut out = vec![IrRawPulse { mark: false, t_us: None }];
    out.extend(v.into_iter().map(|(mark, t)| IrRawPulse { mark, t_us: Some(t) }));
    out.truncate(RAW_BUF);
    out
}

/// Handle a request for this feature's command(s). `args` is the meaningful
/// part of the request's argument bytes; `xfer` is any extra OUT data.
pub(super) fn handle(dev: &mut MockDevice, token: u32, cmd: u8, args: &[u8], xfer: &[u8]) {
    let _ = xfer;
    let arg = |i: usize| args.get(i).copied().unwrap_or(0);
    let now = Instant::now();
    let mut st = dev.state.lock().unwrap();
    let ir = &mut st.ir;
    ir.tick(now);
    let (status, rargs, data): (Status, Vec<u8>, Vec<u8>) = match cmd {
        cmd::QUERY_IR => match arg(0) {
            SUBCMD_QUERY_IR_CMD => {
                let items: Vec<_> = ir.received.drain(..).collect();
                (Status::Ok, vec![], IrCommandReceived::encode_list(&items))
            }
            SUBCMD_QUERY_IR_RAW => (Status::Ok, vec![], IrRawPulse::encode_list(&std::mem::take(&mut ir.raw))),
            _ => (Status::BadSubcmd, vec![], vec![]),
        },
        cmd::SEND_IR => {
            if !ir.tx_configured {
                (Status::Failed, vec![], vec![])
            } else {
                let a = SendIrArgs::decode(args);
                ir.sent.push((a.code, a.count));
                // loopback: the receiver sees what the transmitter sends
                ir.receive(now, a.code.protocol, a.code.code, a.count.max(1), a.code.use_dittos());
                (Status::Ok, vec![], vec![])
            }
        }
        cmd::TVON => match arg(0) {
            SUBCMD_TVON_QUERY_STATE => (Status::Ok, ir.tvon_args(now).to_vec(), vec![]),
            SUBCMD_TVON_SET_RELAY => {
                match arg(1) {
                    TVON_RELAY_OFF => ir.relay_manual = false,
                    TVON_RELAY_ON => ir.relay_manual = true,
                    TVON_RELAY_PULSE => ir.pulse_until = Some(now + PULSE_TIME),
                    _ => {} // ignored, like the firmware
                }
                (Status::Ok, vec![], vec![])
            }
            _ => (Status::BadSubcmd, vec![], vec![]),
        },
        _ => (Status::BadCmd, vec![], vec![]),
    };
    drop(st);
    dev.reply(token, cmd, status, &rargs, &data);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pinscape_protocol::ir::{IrPosition, TvRelayAction};

    use super::*;
    use crate::mock::MockState;
    use crate::VendorClient;

    /// A client whose simulated remote and TV ON sequence are switched off (deterministic).
    fn quiet_client() -> (VendorClient, Arc<Mutex<MockState>>) {
        let dev = MockDevice::new();
        let h = dev.handle();
        {
            let mut st = h.lock().unwrap();
            st.ir.auto_receive = false;
            st.ir.tvon_simulate = false;
        }
        (VendorClient::with_transport(Box::new(dev)), h)
    }

    #[test]
    fn empty_queries() {
        let (mut c, _) = quiet_client();
        assert!(c.query_ir_commands().unwrap().is_empty());
        assert!(c.query_ir_raw().unwrap().is_empty());
    }

    #[test]
    fn send_loops_back_with_dittos() {
        let (mut c, h) = quiet_client();
        let code: IrCode = "01.02.20DF40BF".parse().unwrap();
        c.send_ir(code, 3).unwrap();
        assert_eq!(h.lock().unwrap().ir.sent, vec![(code, 3)]);
        let v = c.query_ir_commands().unwrap();
        assert_eq!(v.len(), 3);
        assert!(v.iter().all(|e| e.text == "01.02.20DF40BF" && e.protocol_name == "NEC-32"));
        assert!(!v[0].ditto && !v[0].auto_repeat);
        assert!(v[1].ditto && v[1].auto_repeat && v[2].ditto);
        assert_eq!(v[1].dt_us, 108_000);
        // the buffer is cleared by the query
        assert!(c.query_ir_commands().unwrap().is_empty());
        // raw pulses: overflow space, then the 9 ms NEC header
        let raw = c.query_ir_raw().unwrap();
        assert_eq!(raw[0], IrRawPulse { mark: false, t_us: None });
        assert_eq!(raw[1], IrRawPulse { mark: true, t_us: Some(9000) });
        assert_eq!(raw[2], IrRawPulse { mark: false, t_us: Some(4500) });
        assert!(raw.windows(2).all(|w| w[0].mark != w[1].mark), "marks and spaces alternate");
        assert!(c.query_ir_raw().unwrap().is_empty());
    }

    #[test]
    fn send_without_dittos_and_other_protocols() {
        let (mut c, _) = quiet_client();
        c.send_ir("17.00.0A90".parse().unwrap(), 2).unwrap();
        c.send_ir("04.00.100C".parse().unwrap(), 1).unwrap();
        c.send_ir("1B.00.002C".parse().unwrap(), 3).unwrap();
        let v = c.query_ir_commands().unwrap();
        assert_eq!(v.len(), 6);
        assert_eq!(v[0].text, "17.00.0A90");
        assert!(!v[0].has_ditto && !v[0].auto_repeat && v[1].auto_repeat);
        assert!(v[2].has_toggle);
        let pos: Vec<_> = v[3..].iter().map(|e| e.position).collect();
        assert_eq!(pos, [Some(IrPosition::First), Some(IrPosition::Middle), Some(IrPosition::Last)]);
        // raw buffer holds only the latest code
        let raw = c.query_ir_raw().unwrap();
        assert!(raw.len() > 10 && raw.len() <= RAW_BUF);
        assert!(raw.windows(2).all(|w| w[0].mark != w[1].mark));
        // RC5 too
        c.send_ir("04.00.100C".parse().unwrap(), 1).unwrap();
        let raw = c.query_ir_raw().unwrap();
        assert_eq!(raw[0].t_us, None);
        assert!(raw.windows(2).all(|w| w[0].mark != w[1].mark));
    }

    #[test]
    fn send_errors() {
        let (mut c, h) = quiet_client();
        let code = IrCode::new(1, 0, 0x1234);
        assert_eq!(c.send_ir(code, 0).unwrap_err().status(), Some(Status::OutOfBounds));
        h.lock().unwrap().ir.tx_configured = false;
        assert_eq!(c.send_ir(code, 1).unwrap_err().status(), Some(Status::Failed));
    }

    #[test]
    fn rx_not_configured_returns_empty() {
        let (mut c, h) = quiet_client();
        h.lock().unwrap().ir.rx_configured = false;
        c.send_ir(IrCode::new(1, 0, 0x1234), 1).unwrap();
        assert!(c.query_ir_commands().unwrap().is_empty());
        assert!(c.query_ir_raw().unwrap().is_empty());
    }

    #[test]
    fn auto_receive_script() {
        let dev = MockDevice::new();
        let h = dev.handle();
        let mut c = VendorClient::with_transport(Box::new(dev));
        assert!(c.query_ir_commands().unwrap().is_empty()); // schedules the first press
        h.lock().unwrap().ir.next_auto = Some(Instant::now() - Duration::from_millis(1));
        let v = c.query_ir_commands().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "01.02.20DF10EF");
        assert!(!c.query_ir_raw().unwrap().is_empty());
        h.lock().unwrap().ir.next_auto = Some(Instant::now() - Duration::from_millis(1));
        let v = c.query_ir_commands().unwrap();
        assert_eq!(v.len(), 4);
        assert!(v[3].ditto && v[3].auto_repeat);
    }

    #[test]
    fn tvon_relay_control() {
        let (mut c, _) = quiet_client();
        let t = c.query_tvon_state().unwrap();
        assert_eq!(t.power_state, pwr::OFF);
        assert!(!t.relay_on);
        c.set_tv_relay(TvRelayAction::On).unwrap();
        let t = c.query_tvon_state().unwrap();
        assert!(t.relay_on && t.relay_manual);
        c.set_tv_relay(TvRelayAction::Off).unwrap();
        assert!(!c.query_tvon_state().unwrap().relay_on);
        c.set_tv_relay(TvRelayAction::Pulse).unwrap();
        let t = c.query_tvon_state().unwrap();
        assert!(t.relay_manual_pulse && !t.relay_manual);
        // unknown relay values are ignored; unknown subcommands are errors
        c.transact(cmd::TVON, &[SUBCMD_TVON_SET_RELAY, 0x7F], &[]).unwrap();
        assert_eq!(c.transact(cmd::TVON, &[0x55], &[]).unwrap_err().status(), Some(Status::BadSubcmd));
        assert_eq!(c.transact(cmd::QUERY_IR, &[0x55], &[]).unwrap_err().status(), Some(Status::BadSubcmd));
    }

    #[test]
    fn tvon_sequence() {
        let mut s = State::default();
        let t0 = Instant::now();
        s.t0 = Some(t0);
        let at = |s: &mut State, secs: f64| s.tvon_phase(t0 + Duration::from_secs_f64(secs));
        assert_eq!(at(&mut s, 0.5).0, pwr::PICOBOOT);
        assert_eq!(at(&mut s, 3.0).0, pwr::COUNTDOWN);
        assert_eq!(at(&mut s, 7.8), (pwr::RELAYON, 1, RELAY_STATE_POWERON, 0));
        assert_eq!(at(&mut s, 8.3).0, pwr::IRSENDING);
        assert_eq!(at(&mut s, 9.0).0, pwr::IRWAITING);
        assert_eq!(at(&mut s, 10.6), (pwr::IRSENDING, 1, 0, 1));
        assert_eq!(at(&mut s, 20.0).0, pwr::ON);
        assert_eq!(at(&mut s, 48.0).0, pwr::OFF);
    }
}
