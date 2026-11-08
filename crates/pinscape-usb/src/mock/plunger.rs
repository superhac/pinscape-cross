//! Simulated-device handling for CMD_PLUNGER (0x0F): plunger configuration,
//! live readings (incl. image-sensor pixels), calibration, settings,
//! commit/revert.
//!
//! The simulated plunger rests at about 1/6 of its travel and, every few
//! seconds, is pulled back, held, and released: a fast forward stroke past
//! the rest point followed by a damped bounce. The default sensor is a
//! potentiometer; [`State::with_sensor`] selects an imaging sensor
//! (TCD1103/TSL1410R, with a pixel snapshot whose shadow edge moves), a
//! quadrature encoder, a VCNL4010, or no plunger at all.
//!
//! The device has no background thread, so the simulation is advanced
//! lazily: each request replays the samples since the last one at a 2 ms
//! interval, which is what calibration and the jitter filter see.

use std::time::{Duration, Instant};

use pinscape_protocol::plunger::{
    firing_state as fs, reading_flags as rf, sensor_type as st, PlungerCal, PlungerConfig, PlungerReading, Request,
    SensorData,
};
use pinscape_protocol::Status;

use super::MockDevice;

/// Physical positions as a fraction of the travel (0 = pushed fully forward, 1 = fully retracted).
const REST: f64 = 0.17;
const PULL: f64 = 0.95;
const FORWARD: f64 = 0.05;
/// Animation cycle and phase boundaries, milliseconds.
const CYCLE_MS: f64 = 7000.0;
const PULL_START: f64 = 2000.0;
const HOLD_START: f64 = 3200.0;
const RELEASE_START: f64 = 4000.0;
const RELEASE_MS: f64 = 40.0;
const BOUNCE_END: f64 = 5000.0;
/// How long the firmware stays in the Fired state.
const FIRED_MS: f64 = 50.0;
const SAMPLE_STEP: Duration = Duration::from_millis(2);
/// The firmware's calibration period.
pub const CAL_DURATION: Duration = Duration::from_secs(15);

/// Simulated plunger sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockSensor {
    /// No plunger configured (the firmware reports its null sensor).
    None,
    Pot,
    Tcd1103,
    Tsl1410r,
    Aedr8300,
    Vcnl4010,
}

impl MockSensor {
    pub fn type_code(self) -> u16 {
        match self {
            MockSensor::None => st::NONE,
            MockSensor::Pot => st::POT,
            MockSensor::Tcd1103 => st::TCD1103,
            MockSensor::Tsl1410r => st::TSL1410R,
            MockSensor::Aedr8300 => st::AEDR8300,
            MockSensor::Vcnl4010 => st::VCNL4010,
        }
    }

    pub fn native_scale(self) -> u32 {
        match self {
            MockSensor::None => 65535,
            MockSensor::Pot => 4095,
            MockSensor::Tcd1103 => 1546,
            MockSensor::Tsl1410r => 1280,
            MockSensor::Aedr8300 => 6000,
            MockSensor::Vcnl4010 => 39320,
        }
    }

    /// Analog noise amplitude in native units.
    fn noise(self) -> f64 {
        match self {
            MockSensor::Pot => 4.0,
            MockSensor::Vcnl4010 => 30.0,
            MockSensor::Tcd1103 | MockSensor::Tsl1410r => 0.8,
            _ => 0.0,
        }
    }
}

/// The adjustable settings (live copy and the copy "saved in flash").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub cal: PlungerCal,
    pub jitter_window: u16,
    pub firing_time_limit: u32,
    pub integration_time: u32,
    pub reverse: bool,
    pub scan_mode: u8,
    pub scaling_factor: u32,
}

impl Settings {
    /// The firmware's defaults when nothing is saved.
    pub fn defaults(native_scale: u32) -> Self {
        Settings {
            cal: PlungerCal::defaults(native_scale),
            jitter_window: 0,
            firing_time_limit: 50_000,
            integration_time: 0,
            reverse: false,
            scan_mode: 0,
            scaling_factor: 100,
        }
    }
}

#[derive(Debug, Clone)]
struct CalRun {
    start: Instant,
    auto_save: bool,
    zero_sum: u64,
    zero_n: u64,
    release_sum: u64,
    release_n: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Jitter {
    lo: u32,
    hi: u32,
    last_pre: u32,
    last_post: u32,
}

/// Animation phase of the simulated plunger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Rest,
    Pull,
    Hold,
    Release,
    Bounce,
}

/// Per-feature simulated state (lives in `MockState::plunger`).
#[derive(Debug, Clone)]
pub struct State {
    pub sensor: MockSensor,
    pub live: Settings,
    pub saved: Settings,
    /// Run the pull-and-release animation.
    pub animate: bool,
    /// Hold the plunger at this physical position (0 = fully forward,
    /// 1 = fully retracted) instead of animating.
    pub manual_position: Option<f64>,
    /// Calibration period (the firmware uses 15 s; tests shorten it).
    pub cal_duration: Duration,
    /// Number of COMMIT_SETTINGS (including auto-saves) performed.
    pub commits: u32,
    /// Add analog noise to the readings.
    pub noise: bool,
    cal: Option<CalRun>,
    jf: Jitter,
    last_raw: u32,
    last_t: Option<Instant>,
    started: Instant,
}

impl Default for State {
    fn default() -> Self {
        Self::with_sensor(MockSensor::Pot)
    }
}

impl State {
    /// A plunger with this sensor, with a plausible saved calibration.
    pub fn with_sensor(sensor: MockSensor) -> Self {
        let scale = sensor.native_scale();
        let mut saved = Settings::defaults(scale);
        if sensor != MockSensor::None {
            let at = |f: f64| (f * scale as f64).round() as u32;
            saved.cal = PlungerCal {
                calibrated: true,
                cal_min: at(FORWARD),
                cal_zero: at(REST),
                cal_max: at(PULL),
                firing_time_measured: 37_000,
                sensor_data: [0; 8],
            };
            if matches!(sensor, MockSensor::Pot | MockSensor::Vcnl4010) {
                saved.jitter_window = (sensor.noise() * 2.5) as u16;
            }
        }
        State {
            sensor,
            live: saved,
            saved,
            animate: true,
            manual_position: None,
            cal_duration: CAL_DURATION,
            commits: 0,
            noise: true,
            cal: None,
            jf: Jitter::default(),
            last_raw: 0,
            last_t: None,
            started: Instant::now(),
        }
    }

    /// No plunger configured.
    pub fn unconfigured() -> Self {
        Self::with_sensor(MockSensor::None)
    }

    pub fn calibrating(&self) -> bool {
        self.cal.is_some()
    }

    fn scale(&self) -> u32 {
        self.sensor.native_scale()
    }

    fn ms(&self, t: Instant) -> f64 {
        t.saturating_duration_since(self.started).as_secs_f64() * 1000.0
    }

    /// Physical position and phase at `ms` since start (noise-free).
    fn physical(&self, ms: f64) -> (f64, Phase, f64) {
        if let Some(p) = self.manual_position {
            return (p.clamp(0.0, 1.0), Phase::Rest, 0.0);
        }
        if !self.animate {
            return (REST, Phase::Rest, 0.0);
        }
        let c = ms % CYCLE_MS;
        if c < PULL_START {
            (REST, Phase::Rest, c)
        } else if c < HOLD_START {
            let u = (c - PULL_START) / (HOLD_START - PULL_START);
            let s = u * u * (3.0 - 2.0 * u);
            (REST + (PULL - REST) * s, Phase::Pull, c)
        } else if c < RELEASE_START {
            (PULL, Phase::Hold, c)
        } else if c < RELEASE_START + RELEASE_MS {
            let u = (c - RELEASE_START) / RELEASE_MS;
            (PULL - (PULL - FORWARD) * u * u, Phase::Release, c)
        } else if c < BOUNCE_END {
            let s = (c - RELEASE_START - RELEASE_MS) / 1000.0;
            let amp = (FORWARD - REST) * (-5.0 * s).exp();
            (REST + amp * (2.0 * std::f64::consts::PI * 6.0 * s).cos(), Phase::Bounce, c)
        } else {
            (REST, Phase::Rest, c)
        }
    }

    /// Time from release to crossing the rest point, ms.
    fn crossing_ms() -> f64 {
        RELEASE_MS * ((PULL - REST) / (PULL - FORWARD)).sqrt()
    }

    /// Deterministic pseudo-noise in -1..1 for a sample time.
    fn noise_at(ms: f64) -> f64 {
        let mut x = (ms / 2.0) as u64 ^ 0x9E37_79B9_7F4A_7C15;
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        (x % 20001) as f64 / 10000.0 - 1.0
    }

    /// Unfiltered raw reading (native units, after orientation) at `ms`.
    fn raw_at(&self, ms: f64, noisy: bool) -> u32 {
        if self.sensor == MockSensor::None {
            return 9830; // the firmware's null sensor reports a fixed value
        }
        let scale = self.scale() as f64;
        let (p, _, _) = self.physical(ms);
        let n = if noisy && self.noise { Self::noise_at(ms) * self.sensor.noise() } else { 0.0 };
        let raw = (p * scale + n).round().clamp(0.0, scale) as u32;
        if self.live.reverse {
            self.scale() - raw
        } else {
            raw
        }
    }

    fn jitter(&mut self, pos: u32) -> u32 {
        let w = self.live.jitter_window as u32;
        let j = &mut self.jf;
        j.last_pre = pos;
        if pos < j.lo {
            j.lo = pos;
            j.hi = pos + w;
            j.last_post = (j.lo + j.hi) / 2;
        } else if pos > j.hi {
            j.hi = pos;
            j.lo = pos.saturating_sub(w);
            j.last_post = (j.lo + j.hi) / 2;
        }
        j.last_post
    }

    /// Calibrated Z axis value for a raw reading (the firmware's ApplyCalibration).
    fn z_of(&self, raw: u32) -> i16 {
        let c = &self.live.cal;
        let k: i64 = if c.cal_max > c.cal_zero { 32767 * 65536 / (c.cal_max - c.cal_zero) as i64 } else { 32767 };
        let pos = (raw as i64 - c.cal_zero as i64) * k / 65536 * self.live.scaling_factor as i64 / 100;
        pos.clamp(-32768, 32767) as i16
    }

    /// Replay samples up to `now` (at most the last 30 s), then end a
    /// calibration run whose time is up.
    fn advance(&mut self, now: Instant) {
        let from = match self.last_t {
            Some(l) if now <= l => None,
            Some(l) => Some(now.checked_sub(Duration::from_secs(30)).map_or(l, |m| l.max(m))),
            None => Some(now),
        };
        if let Some(mut t) = from {
            loop {
                t = (t + SAMPLE_STEP).min(now);
                self.sample(t);
                if t >= now {
                    break;
                }
            }
            self.last_t = Some(now);
        }
        if let Some(run) = &self.cal {
            if now.saturating_duration_since(run.start) >= self.cal_duration {
                self.end_calibration();
            }
        }
    }

    fn sample(&mut self, t: Instant) {
        let ms = self.ms(t);
        let raw = self.raw_at(ms, true);
        let prev = self.last_raw;
        let raw = self.jitter(raw);
        self.last_raw = raw;
        let (_, phase, _) = self.physical(ms);
        let (_, prev_phase, _) = self.physical(ms - SAMPLE_STEP.as_secs_f64() * 1000.0);
        let still = raw.abs_diff(prev) <= self.live.jitter_window as u32 + 2 * self.sensor.noise().ceil() as u32 + 1;
        let Some(run) = self.cal.as_mut() else { return };
        let cal = &mut self.live.cal;
        cal.cal_max = cal.cal_max.max(raw);
        cal.cal_min = cal.cal_min.min(raw);
        // average the rest position over still samples in the lower part of the range seen
        let low = cal.cal_min + (cal.cal_max - cal.cal_min) / 4;
        if still && raw <= low.max(cal.cal_zero) {
            run.zero_sum += raw as u64;
            run.zero_n += 1;
        }
        if phase == Phase::Bounce && prev_phase == Phase::Release {
            run.release_sum += (Self::crossing_ms() * 1000.0) as u64;
            run.release_n += 1;
        }
    }

    fn start_calibration(&mut self, now: Instant, auto_save: bool) {
        if self.cal.is_some() {
            return; // already running (the firmware ignores the request)
        }
        let raw = self.last_raw;
        self.live.cal.cal_min = raw;
        self.live.cal.cal_zero = raw;
        self.live.cal.cal_max = raw;
        self.cal = Some(CalRun { start: now, auto_save, zero_sum: raw as u64, zero_n: 1, release_sum: 0, release_n: 0 });
    }

    fn end_calibration(&mut self) {
        let Some(run) = self.cal.take() else { return };
        let scale = self.scale();
        let cal = &mut self.live.cal;
        cal.cal_zero = (run.zero_sum / run.zero_n.max(1)) as u32;
        if cal.cal_max <= cal.cal_zero {
            *cal = PlungerCal::defaults(scale);
        } else {
            cal.calibrated = true;
        }
        cal.firing_time_measured = run.release_sum.checked_div(run.release_n).unwrap_or(0) as u32;
        if run.auto_save {
            self.commit();
        }
    }

    fn commit(&mut self) {
        self.saved = self.live;
        self.commits += 1;
    }

    /// Firing state and the Z value the firmware would report, from the animation phase.
    fn firing(&self, ms: f64, z0: i16) -> (u16, i16) {
        if self.live.reverse || self.sensor == MockSensor::None {
            return (fs::NONE, z0); // the motion looks backwards: no firing events
        }
        let (_, phase, c) = self.physical(ms);
        let cross = Self::crossing_ms();
        let fires = cross * 1000.0 <= self.live.firing_time_limit as f64;
        let since = c - RELEASE_START;
        let held = self.z_of((PULL * self.scale() as f64) as u32);
        match phase {
            Phase::Release if since < cross => (fs::MOVING, held),
            Phase::Release | Phase::Bounce if fires && since < cross + FIRED_MS => (fs::FIRED, z0.min(0)),
            Phase::Bounce if fires => (fs::SETTLING, 0),
            _ => (fs::NONE, z0),
        }
    }

    fn sensor_data(&self, ms: f64, timestamp: u64) -> Option<SensorData> {
        let (p, _, _) = self.physical(ms);
        let ty = self.sensor.type_code();
        match self.sensor {
            MockSensor::None | MockSensor::Pot => None,
            MockSensor::Aedr8300 => {
                let count = (p * self.scale() as f64) as u32;
                let state = [0u8, 1, 3, 2][(count % 4) as usize];
                Some(SensorData::Quadrature { sensor_type: ty, state, a: state & 1 != 0, b: state & 2 != 0 })
            }
            MockSensor::Vcnl4010 => {
                let prox = 60000.0 * (0.15 / (0.15 + p)).powi(2) + Self::noise_at(ms) * 40.0;
                Some(SensorData::Vcnl4010 { sensor_type: ty, prox_count: prox.clamp(0.0, 65535.0) as u16 })
            }
            MockSensor::Tcd1103 | MockSensor::Tsl1410r => {
                let n = self.scale() as usize;
                let edge = p * n as f64;
                // brighter with longer exposure (default ~2.5 ms)
                let exposure = if self.live.integration_time == 0 { 1.0 } else { self.live.integration_time as f64 / 2500.0 };
                let gain = exposure.clamp(0.1, 2.0);
                let negative = self.sensor == MockSensor::Tcd1103;
                let pixels = (0..n)
                    .map(|i| {
                        // lit region in front of the plunger tip, shadow beyond it, soft edge
                        let lit = 1.0 / (1.0 + ((i as f64 - edge) / 2.0).exp());
                        let mut light = (lit * 0.85 + 0.05) * gain + Self::noise_at(ms + i as f64 * 7.3) * 0.02;
                        if negative && (16..=28).contains(&i) {
                            light = 0.0; // shielded dark-reference pixels
                        }
                        let light = light.clamp(0.0, 1.0);
                        if negative {
                            (195.0 - light * 140.0) as u8
                        } else {
                            (20.0 + light * 225.0) as u8
                        }
                    })
                    .collect();
                Some(SensorData::ImageSensor { sensor_type: ty, timestamp, n_pix: n as u16, pixels })
            }
        }
    }

    /// Build the current reading.
    pub fn reading(&mut self, now: Instant) -> PlungerReading {
        self.advance(now);
        let ms = self.ms(now);
        let timestamp = (ms * 1000.0) as u64;
        let raw = self.last_raw;
        let z0 = self.z_of(raw);
        let step = 2.5;
        let z0_prv = self.z_of(self.raw_at(ms - step, false));
        let z0_nxt = self.z_of(self.raw_at(ms + step, false));
        let dt = (2.0 * step * 1000.0) as i64;
        let speed = ((z0_nxt as i64 - z0_prv as i64) * 10_000 / dt).clamp(-32768, 32767) as i16;
        let (firing_state, z) = self.firing(ms, z0);
        let mut flags = 0;
        let l = &self.live;
        if l.reverse {
            flags |= rf::REVERSE;
        }
        if l.cal.calibrated {
            flags |= rf::CALIBRATED;
        }
        if self.cal.is_some() {
            flags |= rf::CALIBRATING;
        }
        if self.live != self.saved {
            flags |= rf::MODIFIED;
        }
        if firing_state == fs::FIRED {
            flags |= rf::ZBLAUNCH;
        }
        PlungerReading {
            flags,
            raw_pos: raw,
            timestamp,
            z,
            z0,
            z0_prv,
            z0_nxt,
            dt,
            speed,
            firing_state,
            cal_min: l.cal.cal_min,
            cal_zero: l.cal.cal_zero,
            cal_max: l.cal.cal_max,
            cal_sensor_data: l.cal.sensor_data,
            firing_time_measured: l.cal.firing_time_measured,
            firing_time_limit: l.firing_time_limit,
            integration_time: l.integration_time,
            manual_scaling_factor: l.scaling_factor,
            jf_window: l.jitter_window as u32,
            jf_lo: self.jf.lo,
            jf_hi: self.jf.hi,
            jf_last_pre: self.jf.last_pre,
            jf_last_post: self.jf.last_post,
            scan_mode: l.scan_mode,
            sensor: self.sensor_data(ms, timestamp),
            ..Default::default()
        }
        .with_derived()
    }
}

/// Handle a request for this feature's command. `args` is the meaningful
/// part of the request's argument bytes; `xfer` is any extra OUT data.
pub(super) fn handle(dev: &mut MockDevice, token: u32, cmd: u8, args: &[u8], xfer: &[u8]) {
    let now = Instant::now();
    let mut data = Vec::new();
    let status = {
        let mut ms = dev.state.lock().unwrap();
        let s = &mut ms.plunger;
        s.advance(now);
        match Request::decode(args) {
            Request::QueryConfig => {
                data = PlungerConfig::new(s.sensor.type_code(), s.scale(), 0).encode();
                Status::Ok
            }
            Request::QueryReading => {
                data = s.reading(now).encode();
                Status::Ok
            }
            Request::Calibrate { auto_save } => {
                s.start_calibration(now, auto_save);
                Status::Ok
            }
            Request::SetJitterFilter(w) => {
                s.live.jitter_window = w;
                s.jf.lo = s.jf.last_post;
                s.jf.hi = s.jf.last_post;
                Status::Ok
            }
            Request::SetFiringTimeLimit(us) => {
                s.live.firing_time_limit = us;
                Status::Ok
            }
            Request::SetIntegrationTime(us) => {
                s.live.integration_time = us;
                Status::Ok
            }
            Request::SetOrientation { reverse } => {
                s.live.reverse = reverse;
                Status::Ok
            }
            Request::SetScalingFactor(pct) => {
                s.live.scaling_factor = pct;
                Status::Ok
            }
            Request::SetScanMode(m) => {
                s.live.scan_mode = m;
                Status::Ok
            }
            Request::SetCalData => match PlungerCal::decode(xfer) {
                Ok(c) => {
                    s.live.cal = c;
                    Status::Ok
                }
                Err(_) => Status::Failed,
            },
            Request::Commit => {
                s.commit();
                Status::Ok
            }
            Request::Revert => {
                s.live = s.saved;
                Status::Ok
            }
            Request::Unknown(_) => Status::BadSubcmd,
        }
    };
    dev.reply(token, cmd, status, &[], &data);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::mock::MockState;
    use crate::VendorClient;

    fn client(p: State) -> (VendorClient, Arc<Mutex<MockState>>) {
        let dev = MockDevice::with_state(MockState { plunger: p, ..Default::default() });
        let h = dev.handle();
        (VendorClient::with_transport(Box::new(dev)), h)
    }

    fn manual(h: &Arc<Mutex<MockState>>, pos: f64) {
        h.lock().unwrap().plunger.manual_position = Some(pos);
    }

    #[test]
    fn config_and_reading_pot() {
        let (mut c, h) = client(State::default());
        manual(&h, REST);
        let cfg = c.query_plunger_config().unwrap();
        assert!(cfg.configured);
        assert_eq!(cfg.sensor_type_name, "Potentiometer");
        assert_eq!(cfg.native_scale, 4095);
        assert!(!cfg.image_sensor);
        let r = c.query_plunger_reading().unwrap();
        assert!(r.calibrated && !r.calibrating && !r.modified);
        assert!(r.raw_pos.abs_diff(696) < 10, "{}", r.raw_pos);
        assert!(r.z0.abs() < 200, "{}", r.z0);
        assert_eq!(r.sensor, None);
        assert_eq!(r.manual_scaling_factor, 100);
        manual(&h, PULL);
        let r = c.query_plunger_reading().unwrap();
        assert!(r.z0 > 32000, "{}", r.z0);
    }

    #[test]
    fn not_configured() {
        let (mut c, _) = client(State::unconfigured());
        let cfg = c.query_plunger_config().unwrap();
        assert!(!cfg.configured);
        assert_eq!(cfg.sensor_type, st::NONE);
        let r = c.query_plunger_reading().unwrap();
        assert_eq!(r.raw_pos, 9830);
        assert!(!r.calibrated);
        assert_eq!(r.sensor, None);
    }

    #[test]
    fn image_sensor_snapshot() {
        let (mut c, h) = client(State::with_sensor(MockSensor::Tcd1103));
        manual(&h, 0.5);
        let cfg = c.query_plunger_config().unwrap();
        assert!(cfg.image_sensor && cfg.has_integration_time);
        assert_eq!(cfg.native_scale, 1546);
        let r = c.query_plunger_reading().unwrap();
        assert!(r.raw_pos.abs_diff(773) <= 2, "{}", r.raw_pos);
        match r.sensor {
            Some(SensorData::ImageSensor { sensor_type, n_pix, pixels, .. }) => {
                assert_eq!(sensor_type, st::TCD1103);
                assert_eq!(n_pix, 1546);
                assert_eq!(pixels.len(), 1546);
                // negative image: lit side (before the edge) reads lower than the shadow
                assert!(pixels[300] < pixels[1200], "{} {}", pixels[300], pixels[1200]);
            }
            other => panic!("{other:?}"),
        }
        // the edge moves with the plunger
        manual(&h, 0.2);
        let r2 = c.query_plunger_reading().unwrap();
        assert!(r2.raw_pos < 400, "{}", r2.raw_pos);
        c.set_plunger_integration_time(4000).unwrap();
        assert_eq!(c.query_plunger_reading().unwrap().integration_time, 4000);
    }

    #[test]
    fn other_sensor_trailers() {
        let (mut c, _) = client(State::with_sensor(MockSensor::Aedr8300));
        assert!(matches!(c.query_plunger_reading().unwrap().sensor, Some(SensorData::Quadrature { .. })));
        let (mut c, _) = client(State::with_sensor(MockSensor::Vcnl4010));
        assert!(matches!(c.query_plunger_reading().unwrap().sensor, Some(SensorData::Vcnl4010 { .. })));
        let (mut c, _) = client(State::with_sensor(MockSensor::Tsl1410r));
        let cfg = c.query_plunger_config().unwrap();
        assert_eq!(cfg.scan_modes, vec!["Steady Slope", "Steepest Slope", "Speed Gap"]);
        c.set_plunger_scan_mode(2).unwrap();
        assert_eq!(c.query_plunger_reading().unwrap().scan_mode, 2);
    }

    #[test]
    fn settings_modify_commit_revert() {
        let (mut c, h) = client(State::default());
        manual(&h, REST);
        assert!(!c.query_plunger_reading().unwrap().modified);
        c.set_plunger_jitter_filter(25).unwrap();
        c.set_plunger_firing_time_limit(80_000).unwrap();
        c.set_plunger_scaling_factor(120).unwrap();
        c.set_plunger_orientation(true).unwrap();
        let r = c.query_plunger_reading().unwrap();
        assert!(r.modified && r.reverse);
        assert_eq!((r.jf_window, r.firing_time_limit, r.manual_scaling_factor), (25, 80_000, 120));
        // reversed: the raw reading is mirrored
        assert!(r.raw_pos > 3000, "{}", r.raw_pos);
        c.revert_plunger_settings().unwrap();
        let r = c.query_plunger_reading().unwrap();
        assert!(!r.modified && !r.reverse);
        assert_eq!(r.manual_scaling_factor, 100);
        c.set_plunger_jitter_filter(30).unwrap();
        c.commit_plunger_settings().unwrap();
        let r = c.query_plunger_reading().unwrap();
        assert!(!r.modified);
        assert_eq!(r.jf_window, 30);
        let st = h.lock().unwrap();
        assert_eq!(st.plunger.commits, 1);
        assert_eq!(st.plunger.saved.jitter_window, 30);
    }

    #[test]
    fn jitter_filter_window_tracks() {
        let (mut c, h) = client(State::default());
        manual(&h, 0.5);
        c.set_plunger_jitter_filter(40).unwrap();
        let r = c.query_plunger_reading().unwrap();
        assert!(r.jf_hi - r.jf_lo <= 40);
        assert!(r.jf_lo <= r.jf_last_post && r.jf_last_post <= r.jf_hi);
    }

    #[test]
    fn calibration_captures_range() {
        let (mut c, h) = client(State::default());
        manual(&h, REST);
        c.query_plunger_reading().unwrap();
        c.start_plunger_calibration(false).unwrap();
        let r = c.query_plunger_reading().unwrap();
        assert!(r.calibrating);
        manual(&h, PULL);
        c.query_plunger_reading().unwrap();
        manual(&h, FORWARD);
        c.query_plunger_reading().unwrap();
        manual(&h, REST);
        for _ in 0..5 {
            c.query_plunger_reading().unwrap();
        }
        h.lock().unwrap().plunger.cal_duration = Duration::ZERO;
        let r = c.query_plunger_reading().unwrap();
        assert!(!r.calibrating && r.calibrated && r.modified);
        assert!(r.cal_max.abs_diff(3890) < 15, "{}", r.cal_max);
        assert!(r.cal_min.abs_diff(205) < 15, "{}", r.cal_min);
        assert!(r.cal_zero.abs_diff(696) < 15, "{}", r.cal_zero);
        assert_eq!(h.lock().unwrap().plunger.commits, 0);
        // save the result, then restore a host-side copy after a revert
        let saved = r.calibration();
        c.set_plunger_calibration(&PlungerCal::defaults(4095)).unwrap();
        let d = c.query_plunger_reading().unwrap();
        assert!(!d.calibrated);
        assert_eq!((d.cal_min, d.cal_zero, d.cal_max), (0, 682, 4095));
        c.set_plunger_calibration(&saved).unwrap();
        c.commit_plunger_settings().unwrap();
        let r2 = c.query_plunger_reading().unwrap();
        assert_eq!(r2.calibration(), saved);
        assert!(!r2.modified);
    }

    #[test]
    fn calibration_auto_save_and_failure() {
        // no motion during calibration: max <= zero -> defaults, not calibrated
        let (mut c, h) = client(State { noise: false, ..State::default() });
        manual(&h, REST);
        c.start_plunger_calibration(true).unwrap();
        h.lock().unwrap().plunger.cal_duration = Duration::ZERO;
        let r = c.query_plunger_reading().unwrap();
        assert!(!r.calibrating && !r.calibrated);
        assert_eq!(r.cal_max, 4095);
        assert_eq!(h.lock().unwrap().plunger.commits, 1);
        assert!(!r.modified);
    }

    #[test]
    fn animated_release_fires() {
        let ago = |ms: f64| Instant::now() - Duration::from_millis((RELEASE_START + ms) as u64);
        let mut s = State { started: ago(20.0), ..State::default() };
        let r = s.reading(Instant::now());
        assert_eq!(r.firing_state_name, "Moving");
        assert!(r.speed < 0);
        let mut s = State { started: ago(60.0), ..State::default() };
        let r = s.reading(Instant::now());
        assert_eq!(r.firing_state, fs::FIRED);
        assert!(r.zb_launch);
        // too short a time limit: no firing event
        let mut s = State { started: ago(60.0), ..State::default() };
        s.live.firing_time_limit = 10_000;
        assert_eq!(s.reading(Instant::now()).firing_state, fs::NONE);
    }

    #[test]
    fn bad_cal_data_and_subcommand() {
        let (mut c, _) = client(State::default());
        let e = c.transact(pinscape_protocol::cmd::PLUNGER, &[0x07], &[1, 2, 3]).unwrap_err();
        assert_eq!(e.status(), Some(Status::Failed));
        let e = c.transact(pinscape_protocol::cmd::PLUNGER, &[0x55], &[]).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadSubcmd));
    }
}
