//! Simulated-device handling for CMD_NUDGE (0x07): accelerometer nudge
//! status, parameters, calibration, centering, commit/revert.
//!
//! A small model of the firmware's nudge device (`Firmware/Nudge.cpp`): a
//! 400 Hz accelerometer with a fixed tilt (DC offset), sensor noise, and
//! an occasional simulated nudge, run through the same stages as the
//! firmware (centering, DC blocker, low-pass, jitter window, velocity
//! integration with decay, auto-centering on stillness, noise calibration).
//!
//! Simulated time advances with the wall clock when the host queries the
//! status (`realtime`), or explicitly through [`State::advance_ms`] in tests.
//! Parameters have firmware-like put/commit/revert semantics: PUT changes
//! the live set only, COMMIT copies live to "flash", REVERT copies back.

use std::time::Instant;

use pinscape_protocol::nudge::{status_flags, sub, NudgeParams, NudgeStatus, CALIBRATION_MS};
use pinscape_protocol::Status;

use super::MockDevice;

const SAMPLE_RATE: f32 = 400.0;
const SAMPLE_US: u64 = 2500;

/// Per-axis filter state (DC blocker -> low-pass -> jitter window).
#[derive(Debug, Clone, Default)]
struct AxisFilter {
    dc_in: f32,
    dc_out: f32,
    lp: f32,
    win_min: f32,
    win_max: f32,
}

impl AxisFilter {
    fn apply(&mut self, x: f32, dc_time_ms: u16, lp_hz: u16, window: u16) -> f32 {
        let mut v = x;
        if dc_time_ms > 0 {
            let a = (-1000.0 / (dc_time_ms as f32 * SAMPLE_RATE)).exp();
            let out = v - self.dc_in + a * self.dc_out;
            self.dc_in = v;
            self.dc_out = out;
            v = out;
        } else {
            self.dc_in = v;
            self.dc_out = 0.0;
        }
        if lp_hz > 0 {
            let a = (-2.0 * std::f32::consts::PI * lp_hz as f32 / SAMPLE_RATE).exp();
            self.lp = a * self.lp + (1.0 - a) * v;
            v = self.lp;
        } else {
            self.lp = v;
        }
        let w = window as f32;
        if w > 0.0 {
            if v > self.win_max {
                self.win_max = v;
                self.win_min = v - w;
            } else if v < self.win_min {
                self.win_min = v;
                self.win_max = v + w;
            }
            v = (self.win_min + self.win_max) / 2.0;
        } else {
            self.win_min = v;
            self.win_max = v;
        }
        v
    }
}

/// Per-feature simulated state (lives in `MockState::nudge`).
#[derive(Debug, Clone)]
pub struct State {
    /// `false` simulates firmware with no accelerometer (null device):
    /// all-zero readings and a zero timestamp.
    pub configured: bool,
    /// Advance simulated time with the wall clock on each status query.
    pub realtime: bool,
    /// Sensor noise and random nudges (off gives a perfectly still sensor).
    pub animate: bool,
    pub g_range: u8,
    /// Fixed tilt in raw units (Z includes 1g).
    pub tilt: [f32; 3],
    /// Live (in-memory) parameters.
    pub live: NudgeParams,
    /// Parameters saved in "flash".
    pub saved: NudgeParams,
    pub commits: u32,
    /// Simulated microseconds since startup.
    pub now_us: u64,
    pub center: [i32; 3],
    pub velocity: [f32; 3],
    /// Last raw reading (after the axis mapping).
    pub raw: [i16; 3],
    pub filtered: [f32; 3],
    /// Queue a nudge of this (x, y) peak amplitude at the next sample.
    pub pending_nudge: Option<(f32, f32)>,
    filters: [AxisFilter; 3],
    avg_long: [f32; 3],
    avg_short: [f32; 3],
    quiet_end_us: u64,
    last_center_us: u64,
    manual_center: bool,
    cal_end_us: Option<u64>,
    cal_auto_save: bool,
    cal_n: f64,
    cal_sum: [f64; 3],
    cal_sum2: [f64; 3],
    rng: u64,
    next_nudge_us: u64,
    nudge: Option<(u64, f32, f32)>,
    last_wall: Option<Instant>,
    timestamp: u64,
}

impl Default for State {
    fn default() -> Self {
        let tilt = [900.0, -650.0, 16384.0 + 250.0];
        State {
            configured: true,
            realtime: true,
            animate: true,
            g_range: 2,
            tilt,
            live: NudgeParams::default(),
            saved: NudgeParams::default(),
            commits: 0,
            now_us: 1_000_000,
            center: [0; 3],
            velocity: [0.0; 3],
            raw: [0; 3],
            filtered: [0.0; 3],
            pending_nudge: None,
            filters: Default::default(),
            avg_long: tilt,
            avg_short: tilt,
            quiet_end_us: 6_000_000,
            last_center_us: 1_000_000,
            manual_center: false,
            cal_end_us: None,
            cal_auto_save: false,
            cal_n: 0.0,
            cal_sum: [0.0; 3],
            cal_sum2: [0.0; 3],
            rng: 0x2545_F491_4F6C_DD1D,
            next_nudge_us: 4_000_000,
            nudge: None,
            last_wall: None,
            timestamp: 0,
        }
    }
}

impl State {
    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Roughly Gaussian noise with standard deviation `sd`.
    fn noise(&mut self, sd: f32) -> f32 {
        let s: f32 = (0..4).map(|_| self.rand()).sum::<f32>() - 2.0;
        s * sd * 1.73
    }

    fn one_g(&self) -> f32 {
        32768.0 / self.g_range.max(1) as f32
    }

    pub fn calibrating(&self) -> bool {
        self.cal_end_us.is_some()
    }

    /// Advance simulated time, taking samples at the sensor rate.
    pub fn advance_ms(&mut self, ms: u64) {
        if !self.configured {
            self.now_us += ms * 1000;
            return;
        }
        let end = self.now_us + ms * 1000;
        while self.now_us + SAMPLE_US <= end {
            self.now_us += SAMPLE_US;
            self.sample();
        }
        self.now_us = end;
    }

    fn sample(&mut self) {
        let now = self.now_us;
        // raw reading: tilt + noise + nudge impulse (damped 4 Hz sine)
        let mut r = self.tilt;
        if let Some((ax, ay)) = self.pending_nudge.take() {
            self.nudge = Some((now, ax, ay));
        }
        if self.animate {
            for (i, sd) in [45.0, 45.0, 90.0].into_iter().enumerate() {
                r[i] += self.noise(sd);
            }
            if now >= self.next_nudge_us && self.nudge.is_none() {
                let ang = self.rand() * std::f32::consts::TAU;
                let amp = 3000.0 + self.rand() * 7000.0;
                self.nudge = Some((now, amp * ang.cos(), amp * ang.sin()));
                self.next_nudge_us = now + 4_000_000 + (self.rand() * 6_000_000.0) as u64;
            }
        }
        if let Some((t0, ax, ay)) = self.nudge {
            let t = (now - t0) as f32 / 1e6;
            if t > 1.5 {
                self.nudge = None;
            } else {
                let k = (-t / 0.25).exp() * (std::f32::consts::TAU * 4.0 * t).sin();
                r[0] += ax * k;
                r[1] += ay * k;
                r[2] += 0.2 * (ax.abs() + ay.abs()) * k;
            }
        }
        let raw = r.map(|v| v.round().clamp(-32768.0, 32767.0) as i16);
        let [x, y, z] = raw.map(|v| v as f32);

        // calibration
        if let Some(end) = self.cal_end_us {
            self.cal_n += 1.0;
            for (i, v) in [x, y, z].into_iter().enumerate() {
                self.cal_sum[i] += v as f64;
                self.cal_sum2[i] += (v as f64) * (v as f64);
            }
            if now > end {
                self.cal_end_us = None;
                let n = self.cal_n.max(1.0);
                let th = |i: usize| {
                    let var = (self.cal_sum2[i] / n - (self.cal_sum[i] / n).powi(2)).max(0.0);
                    (var.sqrt() * 4.0).round() as u16
                };
                let t = [th(0), th(1), th(2)];
                (self.live.x_threshold, self.live.y_threshold, self.live.z_threshold) = (t[0], t[1], t[2]);
                if self.cal_auto_save {
                    self.commit();
                }
            }
        }

        // centering and filters
        let p = self.live.clone();
        let one_g = self.one_g();
        let c = self.center.map(|v| v as f32);
        let fx = self.filters[0].apply(x - c[0], p.dc_time_ms, p.lp_freq_hz, p.x_jitter_window);
        let fy = self.filters[1].apply(y - c[1], p.dc_time_ms, p.lp_freq_hz, p.y_jitter_window);
        let fz = self.filters[2].apply(z - c[2] - one_g, p.dc_time_ms, p.lp_freq_hz, p.z_jitter_window) + one_g;
        self.filtered = [fx, fy, fz];

        // velocity, mm/s
        let decay = if p.velocity_decay_ms > 0 {
            0.5f32.powf(1000.0 / (p.velocity_decay_ms as f32 * SAMPLE_RATE))
        } else {
            0.0
        };
        let conv = self.g_range as f32 / 32768.0 * 9806.65 / SAMPLE_RATE;
        for (i, a) in [fx, fy, fz - one_g].into_iter().enumerate() {
            self.velocity[i] = self.velocity[i] * decay + a * conv;
        }

        // rolling averages (exponential, ~4 s and ~0.5 s)
        let (al, as_) = (1.0 / (4.0 * SAMPLE_RATE), 1.0 / (0.5 * SAMPLE_RATE));
        for (i, v) in [x, y, z].into_iter().enumerate() {
            self.avg_long[i] += (v - self.avg_long[i]) * al;
            self.avg_short[i] += (v - self.avg_short[i]) * as_;
        }

        // auto-centering on stillness
        let th = [p.x_threshold, p.y_threshold, p.z_threshold].map(|t| t as f32);
        let quiet = (0..3).all(|i| ([x, y, z][i] - self.avg_long[i]).abs() < th[i]);
        let interval = (p.auto_center_interval_ms.max(1) as u64) * 1000;
        if quiet {
            if p.auto_center && now > self.quiet_end_us {
                self.center_on(self.avg_long);
                self.quiet_end_us = now + interval;
            }
        } else {
            self.quiet_end_us = now + interval;
        }
        if std::mem::take(&mut self.manual_center) {
            self.center_on(self.avg_short);
        }

        self.raw = raw;
        self.timestamp = now;
    }

    fn center_on(&mut self, avg: [f32; 3]) {
        self.center = [avg[0].round() as i32, avg[1].round() as i32, (avg[2] - self.one_g()).round() as i32];
        self.last_center_us = self.now_us;
    }

    fn commit(&mut self) {
        self.saved = self.live.clone();
        self.commits += 1;
    }

    fn put(&mut self, p: NudgeParams) {
        if !p.auto_center {
            self.center = [0; 3];
        }
        let mut p = p;
        p.auto_center_interval_ms = p.auto_center_interval_ms.max(1);
        if !p.has_lp_freq {
            p.lp_freq_hz = self.live.lp_freq_hz;
        }
        p.has_lp_freq = true;
        self.live = p;
    }

    fn revert(&mut self) {
        self.live = self.saved.clone();
    }

    pub fn status(&self) -> NudgeStatus {
        let clip = |v: f32| v.round().clamp(-32768.0, 32767.0) as i16;
        let modified = self.live != self.saved;
        let flags = if modified { status_flags::MODIFIED } else { 0 }
            | if self.calibrating() { status_flags::CALIBRATING } else { 0 };
        if !self.configured {
            // null accelerometer: no readings, but the rest of the machinery runs
            let p = &self.live;
            return NudgeStatus {
                cb: 56,
                flags,
                modified,
                calibrating: self.calibrating(),
                g_range: 2,
                last_centering_age_us: self.now_us,
                x_threshold: p.x_threshold as i16,
                y_threshold: p.y_threshold as i16,
                z_threshold: p.z_threshold as i16,
                ..Default::default()
            };
        }
        let scale = self.live.velocity_scaling_factor as f32;
        let i16c = |v: i32| v.clamp(-32768, 32767) as i16;
        NudgeStatus {
            cb: 56,
            flags,
            modified,
            calibrating: self.calibrating(),
            g_range: self.g_range,
            x_raw: self.raw[0],
            y_raw: self.raw[1],
            z_raw: self.raw[2],
            x_filtered: clip(self.filtered[0]),
            y_filtered: clip(self.filtered[1]),
            z_filtered: clip(self.filtered[2]),
            timestamp: self.timestamp,
            last_centering_age_us: self.now_us - self.last_center_us,
            x_center: i16c(self.center[0]),
            y_center: i16c(self.center[1]),
            z_center: i16c(self.center[2]),
            x_threshold: self.live.x_threshold as i16,
            y_threshold: self.live.y_threshold as i16,
            z_threshold: self.live.z_threshold as i16,
            x_avg: clip(self.avg_long[0]),
            y_avg: clip(self.avg_long[1]),
            z_avg: clip(self.avg_long[2]),
            vx: clip(self.velocity[0] * scale),
            vy: clip(self.velocity[1] * scale),
            vz: clip(self.velocity[2] * scale),
            has_accelerometer: self.timestamp != 0,
        }
    }

    /// Catch simulated time up with the wall clock (at most 1 s per call).
    fn sync_wall_clock(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_wall {
            let ms = now.duration_since(last).as_millis().min(1000) as u64;
            if ms > 0 {
                self.advance_ms(ms);
                self.last_wall = Some(now);
            }
        } else {
            self.advance_ms(10);
            self.last_wall = Some(now);
        }
    }
}

/// Handle a request for this feature's command. `args` is the meaningful
/// part of the request's argument bytes; `xfer` is any extra OUT data.
pub(super) fn handle(dev: &mut MockDevice, token: u32, cmd: u8, args: &[u8], xfer: &[u8]) {
    let subcmd = args.first().copied().unwrap_or(0);
    let mut data = Vec::new();
    let status = {
        let mut st = dev.state.lock().unwrap();
        let n = &mut st.nudge;
        match subcmd {
            sub::CALIBRATE => {
                n.cal_end_us = Some(n.now_us + CALIBRATION_MS as u64 * 1000);
                n.cal_auto_save = args.get(1) == Some(&1);
                (n.cal_n, n.cal_sum, n.cal_sum2) = (0.0, [0.0; 3], [0.0; 3]);
                Status::Ok
            }
            sub::CENTER => {
                n.manual_center = true;
                if !n.configured {
                    n.center = [0; 3];
                    n.last_center_us = n.now_us;
                }
                Status::Ok
            }
            sub::QUERY_STATUS => {
                if n.realtime {
                    n.sync_wall_clock();
                }
                data = n.status().encode();
                Status::Ok
            }
            sub::QUERY_PARAMS => {
                data = n.live.encode();
                Status::Ok
            }
            sub::PUT_PARAMS => match NudgeParams::decode(xfer) {
                // like the firmware, require at least the full current struct
                Ok(p) if xfer.len() >= 26 => {
                    n.put(p);
                    Status::Ok
                }
                _ => Status::Failed,
            },
            sub::COMMIT => {
                n.commit();
                Status::Ok
            }
            sub::REVERT => {
                n.revert();
                Status::Ok
            }
            _ => Status::BadSubcmd,
        }
    };
    dev.reply(token, cmd, status, &[], &data);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pinscape_protocol::nudge::NudgeParams;
    use pinscape_protocol::Status;

    use crate::mock::{MockDevice, MockState};
    use crate::VendorClient;

    /// A client on a still (noise-free), manually clocked mock.
    fn client() -> (VendorClient, Arc<Mutex<MockState>>) {
        let dev = MockDevice::new();
        let h = dev.handle();
        {
            let mut st = h.lock().unwrap();
            st.nudge.realtime = false;
            st.nudge.animate = false;
        }
        (VendorClient::with_transport(Box::new(dev)), h)
    }

    fn advance(h: &Arc<Mutex<MockState>>, ms: u64) {
        h.lock().unwrap().nudge.advance_ms(ms);
    }

    #[test]
    fn status_shows_raw_tilt_until_centered() {
        let (mut c, h) = client();
        // DC blocker off so the tilt shows in the filtered output
        let p = NudgeParams { dc_time_ms: 0, auto_center: true, ..c.query_nudge_params().unwrap() };
        c.put_nudge_params(&p).unwrap();
        advance(&h, 500);
        let s = c.query_nudge_status().unwrap();
        assert!(s.has_accelerometer);
        assert_eq!(s.g_range, 2);
        assert_eq!((s.x_raw, s.y_raw, s.z_raw), (900, -650, 16634));
        assert_eq!((s.x_filtered, s.y_filtered), (900, -650));
        assert!(s.modified);

        c.set_nudge_center_point().unwrap();
        advance(&h, 10);
        let s = c.query_nudge_status().unwrap();
        assert_eq!((s.x_center, s.y_center, s.z_center), (900, -650, 250));
        assert_eq!((s.x_filtered, s.y_filtered, s.z_filtered), (0, 0, 16384));
        assert!(s.last_centering_age_us < 20_000);
    }

    #[test]
    fn auto_centering_after_quiet_period() {
        let (mut c, h) = client();
        c.put_nudge_params(&NudgeParams { dc_time_ms: 0, auto_center_interval_ms: 1000, ..Default::default() })
            .unwrap();
        advance(&h, 8000);
        let s = c.query_nudge_status().unwrap();
        assert_eq!((s.x_center, s.y_center), (900, -650));
        assert!(s.last_centering_age_us <= 1_100_000);

        // disabling auto-centering zeroes the center point
        c.put_nudge_params(&NudgeParams { auto_center: false, dc_time_ms: 0, ..Default::default() }).unwrap();
        advance(&h, 10);
        let s = c.query_nudge_status().unwrap();
        assert_eq!((s.x_center, s.y_center, s.z_center), (0, 0, 0));
        assert_eq!(s.x_filtered, 900);
    }

    #[test]
    fn nudge_impulse_moves_filtered_and_velocity() {
        let (mut c, h) = client();
        c.set_nudge_center_point().unwrap();
        advance(&h, 3000);
        h.lock().unwrap().nudge.pending_nudge = Some((8000.0, 0.0));
        advance(&h, 40);
        let s = c.query_nudge_status().unwrap();
        assert!(s.x_raw > 2000, "raw x {}", s.x_raw);
        assert!(s.x_filtered > 1000, "filtered x {}", s.x_filtered);
        assert!(s.vx > 0, "vx {}", s.vx);
        assert!(s.vy.abs() < s.vx / 10, "vy {}", s.vy);
    }

    #[test]
    fn params_put_commit_revert() {
        let (mut c, h) = client();
        let orig = c.query_nudge_params().unwrap();
        assert_eq!(orig, NudgeParams::default());
        assert!(!c.query_nudge_status().unwrap().modified);

        let edited = NudgeParams {
            dc_time_ms: 350,
            lp_freq_hz: 25,
            x_jitter_window: 120,
            velocity_scaling_factor: 109,
            auto_center: false,
            ..orig.clone()
        };
        c.put_nudge_params(&edited).unwrap();
        assert_eq!(c.query_nudge_params().unwrap(), edited);
        assert!(c.query_nudge_status().unwrap().modified);

        c.revert_nudge_settings().unwrap();
        assert_eq!(c.query_nudge_params().unwrap(), orig);
        assert!(!c.query_nudge_status().unwrap().modified);

        c.put_nudge_params(&edited).unwrap();
        c.commit_nudge_settings().unwrap();
        assert!(!c.query_nudge_status().unwrap().modified);
        assert_eq!(h.lock().unwrap().nudge.commits, 1);
        c.put_nudge_params(&NudgeParams { dc_time_ms: 1, ..edited.clone() }).unwrap();
        c.revert_nudge_settings().unwrap();
        assert_eq!(c.query_nudge_params().unwrap(), edited);
    }

    #[test]
    fn short_put_is_rejected() {
        let (mut c, _) = client();
        let mut b = NudgeParams::default().encode();
        b.truncate(24);
        b[0] = 24;
        let e = c.transact(pinscape_protocol::cmd::NUDGE, &[0x05], &b).unwrap_err();
        assert_eq!(e.status(), Some(Status::Failed));
        let e = c.transact(pinscape_protocol::cmd::NUDGE, &[0x55], &[]).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadSubcmd));
    }

    #[test]
    fn calibration_sets_thresholds() {
        let (mut c, h) = client();
        h.lock().unwrap().nudge.animate = true;
        h.lock().unwrap().nudge.next_nudge_us = u64::MAX; // noise only
        c.start_nudge_calibration(false).unwrap();
        advance(&h, 1000);
        assert!(c.query_nudge_status().unwrap().calibrating);
        advance(&h, 15_000);
        let s = c.query_nudge_status().unwrap();
        assert!(!s.calibrating);
        // 4 standard deviations of the simulated noise (45, 45, 90)
        assert!((120..=240).contains(&s.x_threshold), "x threshold {}", s.x_threshold);
        assert!((240..=480).contains(&s.z_threshold), "z threshold {}", s.z_threshold);
        let p = c.query_nudge_params().unwrap();
        assert_eq!(p.x_threshold as i16, s.x_threshold);
        assert!(s.modified);
        assert_eq!(h.lock().unwrap().nudge.commits, 0);

        // auto-save variant commits at the end
        c.start_nudge_calibration(true).unwrap();
        advance(&h, 15_100);
        assert!(!c.query_nudge_status().unwrap().modified);
        assert_eq!(h.lock().unwrap().nudge.commits, 1);
    }

    #[test]
    fn not_configured_reports_null_device() {
        let (mut c, h) = client();
        h.lock().unwrap().nudge.configured = false;
        advance(&h, 1000);
        let s = c.query_nudge_status().unwrap();
        assert!(!s.has_accelerometer);
        assert_eq!(s.timestamp, 0);
        assert_eq!((s.x_raw, s.z_raw, s.vx), (0, 0, 0));
        // parameters still work, as on the firmware
        let p = c.query_nudge_params().unwrap();
        c.put_nudge_params(&NudgeParams { dc_time_ms: 0, ..p }).unwrap();
        assert!(c.query_nudge_status().unwrap().modified);
    }

    #[test]
    fn realtime_clock_advances() {
        let dev = MockDevice::new();
        let mut c = VendorClient::with_transport(Box::new(dev));
        let a = c.query_nudge_status().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let b = c.query_nudge_status().unwrap();
        assert!(a.has_accelerometer);
        assert!(b.timestamp > a.timestamp);
    }
}
