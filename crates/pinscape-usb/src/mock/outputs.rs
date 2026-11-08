//! Simulated-device handling for CMD_OUTPUTS (0x11): logical ports, output
//! devices and device ports, levels, test mode, setting logical/physical
//! port levels, night mode.
//!
//! The default configuration mimics a typical cabinet: flipper/knocker
//! solenoids on Pico GPIOs, flashers on a TLC5940 chain, motors on a
//! PCA9685, button lamps on a 74HC595 chain, plus a virtual port. A few
//! ports animate on their own (an "attract mode" chase and a computed
//! blinker) so the UI has live data to show.

use std::time::{Duration, Instant};

use pinscape_protocol::outputs::{
    self as out, dev_type as dt, port_flags as pf, port_type as pt, sub, DevicePortArgs, LogicalLevel,
    LogicalLevels, RawDevDesc, TestModeArgs,
};
use pinscape_protocol::Status;

use super::MockDevice;

/// Flipper Logic: full power for this long, then the hold level.
const FLIPPER_TIME: Duration = Duration::from_millis(1500);
const FLIPPER_HOLD_PCT: u32 = 30;

/// One simulated logical port.
#[derive(Debug, Clone)]
pub struct MockPort {
    pub flags: u8,
    pub dev_id: u8,
    pub dev_type: u8,
    pub dev_port: u8,
    pub name: String,
    /// Host-commanded level.
    pub dof: u8,
    /// Animated by the simulated "attract mode" until the host sets it.
    pub demo: bool,
    on_since: Option<Instant>,
}

/// One simulated physical output device.
#[derive(Debug, Clone)]
pub struct MockOutDev {
    pub desc: RawDevDesc,
    pub port_types: Vec<u8>,
    /// Levels set directly in test mode (native scale).
    pub test_levels: Vec<u16>,
}

impl MockOutDev {
    fn new(desc: RawDevDesc, port_types: Vec<u8>) -> Self {
        let n = desc.num_ports as usize;
        MockOutDev { desc, port_types, test_levels: vec![0; n] }
    }

    /// Max native level for a port (digital ports are 0/1).
    fn max_level(&self, port: usize) -> u16 {
        if self.port_types.get(port) == Some(&pt::DIGITAL) || self.desc.pwm_res <= 2 {
            1
        } else {
            self.desc.pwm_res - 1
        }
    }

    /// Scale a 0..255 logical level to this port's native scale.
    fn scale(&self, port: usize, level: u8) -> u16 {
        match self.max_level(port) {
            1 => (level >= 128) as u16,
            max => ((level as u32 * max as u32 + 127) / 255) as u16,
        }
    }
}

/// Per-feature simulated state (lives in `MockState::outputs`).
#[derive(Debug, Clone)]
pub struct State {
    pub ports: Vec<MockPort>,
    pub devices: Vec<MockOutDev>,
    pub night_mode: bool,
    pub test_mode: bool,
    /// When test mode reverts automatically (`None` = indefinite).
    pub test_deadline: Option<Instant>,
    /// Timeout from the last TEST_MODE enable command.
    pub last_test_timeout_ms: Option<u32>,
    /// Run the attract-mode / computed animations.
    pub animate: bool,
    /// PWMWorker resets requested: (worker index, bootloader).
    pub worker_resets: Vec<(u8, bool)>,
    started: Instant,
}

fn port(flags: u8, dev_id: u8, dev_type: u8, dev_port: u8, name: &str, demo: bool) -> MockPort {
    let flags = if name.is_empty() { flags } else { flags | pf::NAMED };
    MockPort { flags, dev_id, dev_type, dev_port, name: name.into(), dof: 0, demo, on_since: None }
}

fn gpio_device(outputs: &[(usize, u8)]) -> MockOutDev {
    let mut types = vec![pt::UNUSED; 30];
    for &(gp, t) in outputs {
        types[gp] = t;
    }
    let desc = RawDevDesc { addr: 0, num_ports: 30, num_ports_per_chip: 30, pwm_res: 256, config_index: 0, dev_type: dt::GPIO };
    MockOutDev::new(desc, types)
}

impl Default for State {
    fn default() -> Self {
        let ports = vec![
            port(pf::FLIPPER_LOGIC | pf::NOISY, 0, dt::GPIO, 15, "Left Flipper", false),
            port(pf::FLIPPER_LOGIC | pf::NOISY, 0, dt::GPIO, 16, "Right Flipper", false),
            port(pf::FLIPPER_LOGIC | pf::NOISY, 0, dt::GPIO, 17, "Knocker", false),
            port(pf::GAMMA, 0, dt::TLC5940, 0, "Flasher Left", true),
            port(pf::GAMMA, 0, dt::TLC5940, 1, "Flasher Center", true),
            port(pf::GAMMA, 0, dt::TLC5940, 2, "", true),
            port(pf::NOISY, 0, dt::PCA9685, 0, "Shaker Motor", false),
            port(pf::NOISY | pf::INVERTED, 0, dt::PCA9685, 1, "Gear Motor", false),
            port(pf::COMPUTED, 0, dt::HC595, 0, "Start Button Lamp", false),
            port(0, 0, dt::HC595, 1, "Launch Button Lamp", false),
            port(0, 0, dt::VIRTUAL, 0, "", false),
        ];
        let devices = vec![
            gpio_device(&[(15, pt::PWM), (16, pt::PWM), (17, pt::DIGITAL)]),
            MockOutDev::new(
                RawDevDesc { addr: 0, num_ports: 32, num_ports_per_chip: 16, pwm_res: 4096, config_index: 0, dev_type: dt::TLC5940 },
                vec![pt::PWM; 32],
            ),
            MockOutDev::new(
                RawDevDesc { addr: 0x40, num_ports: 16, num_ports_per_chip: 16, pwm_res: 4096, config_index: 0, dev_type: dt::PCA9685 },
                vec![pt::PWM; 16],
            ),
            MockOutDev::new(
                RawDevDesc { addr: 0, num_ports: 16, num_ports_per_chip: 8, pwm_res: 2, config_index: 0, dev_type: dt::HC595 },
                vec![pt::DIGITAL; 16],
            ),
            // A PWMWorker Pico (workerPico[0]) on I2C1 0x30, with two ports in use.
            MockOutDev::new(
                RawDevDesc { addr: 0x130, num_ports: 24, num_ports_per_chip: 24, pwm_res: 256, config_index: 0, dev_type: dt::PWMWORKER },
                [vec![pt::PWM; 2], vec![pt::UNUSED; 22]].concat(),
            ),
        ];
        State {
            ports,
            devices,
            night_mode: false,
            test_mode: false,
            test_deadline: None,
            last_test_timeout_ms: None,
            animate: true,
            worker_resets: Vec::new(),
            started: Instant::now(),
        }
    }
}

impl State {
    /// A device with no outputs configured (the firmware still reports the GPIO bank).
    pub fn unconfigured() -> Self {
        State { ports: Vec::new(), devices: vec![gpio_device(&[])], ..Default::default() }
    }

    fn expire_test_mode(&mut self, now: Instant) {
        if self.test_mode && self.test_deadline.is_some_and(|d| now >= d) {
            self.test_mode = false;
            self.test_deadline = None;
        }
    }

    fn device_index(&self, dev_type: u8, config_index: u8) -> Option<usize> {
        self.devices.iter().position(|d| d.desc.dev_type == dev_type && d.desc.config_index == config_index)
    }

    /// Current logical levels: (dof, calc, out) per port.
    pub fn logical_levels(&self, now: Instant) -> Vec<LogicalLevel> {
        let t = now.duration_since(self.started).as_millis() as u64;
        self.ports
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let dof = if p.demo && self.animate {
                    // triangle wave, phase-shifted per port: a chase
                    let x = (t + i as u64 * 400) % 2400;
                    let tri = if x < 1200 { x } else { 2400 - x };
                    (tri * 255 / 1200) as u8
                } else {
                    p.dof
                };
                let calc = if p.flags & pf::COMPUTED != 0 && self.animate {
                    if (t / 500).is_multiple_of(2) { 255 } else { 0 }
                } else {
                    dof
                };
                let mut lvl = calc as u32;
                if p.flags & pf::FLIPPER_LOGIC != 0 && p.on_since.is_some_and(|s| now.duration_since(s) >= FLIPPER_TIME) {
                    lvl = lvl * FLIPPER_HOLD_PCT / 100;
                }
                if p.flags & pf::GAMMA != 0 {
                    lvl = ((lvl as f32 / 255.0).powf(2.2) * 255.0).round() as u32;
                }
                if self.night_mode && p.flags & pf::NOISY != 0 {
                    lvl = 0;
                }
                if p.flags & pf::INVERTED != 0 {
                    lvl = 255 - lvl;
                }
                LogicalLevel::from_raw(dof, calc, lvl as u8, 0, 0)
            })
            .collect()
    }

    /// Current physical levels, concatenated in device order.
    pub fn device_levels(&self, now: Instant) -> Vec<u16> {
        if self.test_mode {
            return self.devices.iter().flat_map(|d| d.test_levels.iter().copied()).collect();
        }
        let mut per_dev: Vec<Vec<u16>> = self.devices.iter().map(|d| vec![0; d.desc.num_ports as usize]).collect();
        for (p, l) in self.ports.iter().zip(self.logical_levels(now)) {
            if let Some(di) = self.device_index(p.dev_type, p.dev_id) {
                let dp = p.dev_port as usize;
                if let Some(slot) = per_dev[di].get_mut(dp) {
                    *slot = self.devices[di].scale(dp, l.out_level);
                }
            }
        }
        per_dev.concat()
    }

    fn set_port(&mut self, num: u8, level: u8, now: Instant) {
        let Some(p) = (num as usize).checked_sub(1).and_then(|i| self.ports.get_mut(i)) else {
            return; // firmware ignores out-of-range ports
        };
        p.demo = false;
        p.on_since = match (level, p.on_since) {
            (0, _) => None,
            (_, Some(s)) if p.dof > 0 => Some(s),
            _ => Some(now),
        };
        p.dof = level;
    }

    fn set_test_mode(&mut self, a: TestModeArgs, now: Instant) {
        if a.enable {
            if !self.test_mode {
                // entering test mode: physical outputs start from all-off
                for d in &mut self.devices {
                    d.test_levels.iter_mut().for_each(|l| *l = 0);
                }
            }
            self.test_mode = true;
            self.last_test_timeout_ms = Some(a.timeout_ms);
            self.test_deadline = (a.timeout_ms != 0).then(|| now + Duration::from_millis(a.timeout_ms as u64));
        } else {
            self.test_mode = false;
            self.test_deadline = None;
        }
    }

    fn set_device_port(&mut self, a: DevicePortArgs) -> Status {
        let Some(di) = self.device_index(a.dev_type, a.config_index) else {
            return Status::BadParams;
        };
        let dev = &mut self.devices[di];
        let p = a.port as usize;
        if p >= dev.test_levels.len() {
            return Status::BadParams;
        }
        if self.test_mode {
            dev.test_levels[p] = a.level.min(dev.max_level(p));
        }
        // outside test mode the Output Manager immediately countermands it
        Status::Ok
    }
}

/// Handle a request for this feature's command(s). `args` is the meaningful
/// part of the request's argument bytes; `xfer` is any extra OUT data.
pub(super) fn handle(dev: &mut MockDevice, token: u32, cmd: u8, args: &[u8], _xfer: &[u8]) {
    let arg = |i: usize| args.get(i).copied().unwrap_or(0);
    let now = Instant::now();
    let mut guard = dev.state.lock().unwrap();
    let st = &mut guard.outputs;
    st.expire_test_mode(now);
    let mut reply_args: Vec<u8> = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let status = match arg(0) {
        sub::SET_PORT => {
            st.set_port(arg(1), arg(2), now);
            Status::Ok
        }
        sub::TEST_MODE => {
            st.set_test_mode(TestModeArgs::decode(args), now);
            Status::Ok
        }
        sub::SET_DEVICE_PORT => st.set_device_port(DevicePortArgs::decode(args)),
        sub::SET_NIGHT_MODE => match arg(1) {
            0 | 1 => {
                st.night_mode = arg(1) == 1;
                Status::Ok
            }
            _ => Status::BadParams,
        },
        s @ (sub::PWMWORKER_RESET | sub::PWMWORKER_BOOTLOADER) => {
            st.worker_resets.push((arg(1), s == sub::PWMWORKER_BOOTLOADER));
            Status::Ok
        }
        sub::QUERY_LOGICAL_PORTS => {
            let raw: Vec<_> = st.ports.iter().map(|p| (p.flags, p.dev_id, p.dev_type, p.dev_port)).collect();
            data = out::encode_logical_ports(&raw);
            Status::Ok
        }
        sub::QUERY_LOGICAL_PORT_NAME => match (arg(1) as usize).checked_sub(1).and_then(|i| st.ports.get(i)) {
            Some(p) => {
                data = p.name.as_bytes().to_vec();
                data.push(0);
                Status::Ok
            }
            None => Status::Failed,
        },
        sub::QUERY_DEVICES => {
            let raw: Vec<_> = st.devices.iter().map(|d| d.desc).collect();
            data = out::encode_devices(&raw);
            Status::Ok
        }
        sub::QUERY_DEVICE_PORTS => {
            let types: Vec<u8> = st.devices.iter().flat_map(|d| d.port_types.iter().copied()).collect();
            data = out::encode_device_port_types(&types);
            Status::Ok
        }
        sub::QUERY_LOGICAL_PORT_LEVELS => {
            data = LogicalLevels { test_mode: st.test_mode, levels: st.logical_levels(now) }.encode();
            Status::Ok
        }
        sub::QUERY_DEVICE_PORT_LEVELS => {
            data = out::encode_device_levels(&st.device_levels(now));
            Status::Ok
        }
        sub::QUERY_NIGHT_MODE => {
            reply_args.push(st.night_mode as u8);
            Status::Ok
        }
        _ => Status::BadSubcmd,
    };
    drop(guard);
    dev.reply(token, cmd, status, &reply_args, &data);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use pinscape_protocol::outputs::{dev_type as dt, port_type as pt};
    use pinscape_protocol::Status;

    use crate::mock::{MockDevice, MockState};
    use crate::VendorClient;

    fn client() -> (VendorClient, Arc<Mutex<MockState>>) {
        let dev = MockDevice::new();
        let h = dev.handle();
        h.lock().unwrap().outputs.animate = false;
        (VendorClient::with_transport(Box::new(dev)), h)
    }

    #[test]
    fn config_query() {
        let (mut c, _) = client();
        let ports = c.query_logical_ports().unwrap();
        assert_eq!(ports.len(), 11);
        assert_eq!(ports[0].port, 1);
        assert_eq!(ports[0].name.as_deref(), Some("Left Flipper"));
        assert!(ports[0].flipper_logic && ports[0].noisy);
        assert_eq!((ports[0].dev_type, ports[0].dev_port), (dt::GPIO, 15));
        assert_eq!(ports[5].name, None);
        assert!(!ports[5].named);
        assert_eq!(ports[10].dev_type_name, "Virtual");
        assert_eq!(c.query_logical_port_name(3).unwrap(), "Knocker");

        let devs = c.query_output_devices_full().unwrap();
        assert_eq!(devs.len(), 5);
        assert_eq!(devs[0].dev_type_name, "GPIO");
        assert_eq!(devs[0].port_types[15], pt::PWM);
        assert_eq!(devs[0].port_types[17], pt::DIGITAL);
        assert_eq!(devs[0].port_types[0], pt::UNUSED);
        assert_eq!((devs[1].num_ports, devs[1].num_ports_per_chip, devs[1].pwm_res), (32, 16, 4096));
        assert_eq!(devs[1].first_port, 30);
        assert_eq!((devs[2].i2c_bus, devs[2].i2c_addr), (Some(0), Some(0x40)));
        assert_eq!(devs[3].dev_type_name, "74HC595");
        assert_eq!((devs[4].dev_type_name, devs[4].config_index), ("PWMWorker", 0));
        assert_eq!((devs[4].i2c_bus, devs[4].i2c_addr), (Some(1), Some(0x30)));
        let total: usize = devs.iter().map(|d| d.num_ports as usize).sum();
        assert_eq!(c.query_device_port_levels().unwrap().len(), total);
    }

    #[test]
    fn name_query_out_of_range_fails() {
        let (mut c, _) = client();
        let e = c.query_logical_port_name(200).unwrap_err();
        assert_eq!(e.status(), Some(Status::Failed));
    }

    #[test]
    fn set_logical_port_drives_physical_output() {
        let (mut c, _) = client();
        c.set_logical_port(7, 255).unwrap(); // shaker motor -> PCA9685 port 0
        c.set_logical_port(4, 128).unwrap(); // flasher (gamma) -> TLC5940 port 0
        let l = c.query_logical_port_levels().unwrap();
        assert!(!l.test_mode);
        assert_eq!(l.levels[6].dof_level, 255);
        assert_eq!(l.levels[6].out_level, 255);
        assert_eq!(l.levels[3].dof_level, 128);
        assert!(l.levels[3].out_level < 128, "gamma applied");
        // inverted port at 0 outputs 255
        assert_eq!(l.levels[7].out_level, 255);

        let devs = c.query_output_devices().unwrap();
        let dl = c.query_device_port_levels().unwrap();
        assert_eq!(dl[devs[2].first_port], 4095);
        assert!(dl[devs[1].first_port] > 0 && dl[devs[1].first_port] < 2048);
        // GPIO digital knocker
        c.set_logical_port(3, 255).unwrap();
        assert_eq!(c.query_device_port_levels().unwrap()[17], 1);
        // out-of-range port is ignored, like the firmware
        c.set_logical_port(99, 255).unwrap();
    }

    #[test]
    fn night_mode_silences_noisy_ports() {
        let (mut c, _) = client();
        assert!(!c.query_night_mode().unwrap());
        c.set_logical_port(7, 255).unwrap();
        c.set_night_mode(true).unwrap();
        assert!(c.query_night_mode().unwrap());
        let l = c.query_logical_port_levels().unwrap();
        assert_eq!(l.levels[6].dof_level, 255);
        assert_eq!(l.levels[6].out_level, 0);
        c.set_night_mode(false).unwrap();
        assert_eq!(c.query_logical_port_levels().unwrap().levels[6].out_level, 255);
    }

    #[test]
    fn test_mode_device_ports_and_timeout() {
        let (mut c, h) = client();
        let devs = c.query_output_devices_full().unwrap();
        let tlc = &devs[1];

        // not in test mode: device port writes are countermanded
        c.set_device_port(dt::TLC5940, 0, 5, 1000).unwrap();
        assert_eq!(c.query_device_port_levels().unwrap()[tlc.first_port + 5], 0);

        c.set_output_test_mode(true, 300).unwrap();
        assert!(c.query_logical_port_levels().unwrap().test_mode);
        assert_eq!(h.lock().unwrap().outputs.last_test_timeout_ms, Some(300));
        c.set_device_port(dt::TLC5940, 0, 5, 1000).unwrap();
        c.set_device_port(dt::TLC5940, 0, 6, 60000).unwrap(); // clipped
        c.set_device_port(dt::GPIO, 0, 17, 255).unwrap(); // digital: clipped to 1
        let dl = c.query_device_port_levels().unwrap();
        assert_eq!(dl[tlc.first_port + 5], 1000);
        assert_eq!(dl[tlc.first_port + 6], 4095);
        assert_eq!(dl[17], 1);
        // logical port writes don't reach the physical outputs in test mode
        c.set_logical_port(7, 255).unwrap();
        assert_eq!(c.query_device_port_levels().unwrap()[devs[2].first_port], 0);

        // bad device / port
        let e = c.set_device_port(dt::TLC59116, 0, 0, 1).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadParams));
        assert!(c.set_device_port(dt::TLC5940, 0, 32, 1).is_err());

        // keepalive extends the deadline
        std::thread::sleep(std::time::Duration::from_millis(200));
        c.set_output_test_mode(true, 300).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(c.query_logical_port_levels().unwrap().test_mode, "keepalive extended test mode");
        assert_eq!(c.query_device_port_levels().unwrap()[tlc.first_port + 5], 1000, "re-enable keeps levels");

        // ...and without one it expires
        std::thread::sleep(std::time::Duration::from_millis(400));
        let l = c.query_logical_port_levels().unwrap();
        assert!(!l.test_mode, "test mode timed out");
        let dl = c.query_device_port_levels().unwrap();
        assert_eq!(dl[tlc.first_port + 5], 0);
        assert_eq!(dl[devs[2].first_port], 4095, "logical control resumed");
    }

    #[test]
    fn test_mode_explicit_disable() {
        let (mut c, h) = client();
        c.set_output_test_mode(true, 0).unwrap();
        assert_eq!(h.lock().unwrap().outputs.test_deadline, None);
        assert!(c.query_logical_port_levels().unwrap().test_mode);
        c.set_output_test_mode(false, 0).unwrap();
        assert!(!c.query_logical_port_levels().unwrap().test_mode);
    }

    #[test]
    fn animation_changes_levels() {
        let (mut c, h) = client();
        h.lock().unwrap().outputs.animate = true;
        let a = c.query_logical_port_levels().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        let b = c.query_logical_port_levels().unwrap();
        let moved = (3..6).any(|i| a.levels[i].dof_level != b.levels[i].dof_level);
        assert!(moved, "attract chase moves");
        // host takes over a demo port
        c.set_logical_port(4, 10).unwrap();
        assert_eq!(c.query_logical_port_levels().unwrap().levels[3].dof_level, 10);
    }

    #[test]
    fn misc_subcommands() {
        let (mut c, h) = client();
        c.pwm_worker_reset(1, true).unwrap();
        assert_eq!(h.lock().unwrap().outputs.worker_resets, vec![(1, true)]);
        let e = c.transact(pinscape_protocol::cmd::OUTPUTS, &[0x55], &[]).unwrap_err();
        assert_eq!(e.status(), Some(Status::BadSubcmd));
    }

    #[test]
    fn unconfigured_device() {
        let st = MockState { outputs: super::State::unconfigured(), ..Default::default() };
        let mut c = VendorClient::with_transport(Box::new(MockDevice::with_state(st)));
        assert!(c.query_logical_ports().unwrap().is_empty());
        assert!(c.query_logical_port_levels().unwrap().levels.is_empty());
        assert_eq!(c.query_output_devices().unwrap().len(), 1);
    }
}
