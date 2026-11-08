//! Outputs: VendorClient methods for CMD_OUTPUTS (0x11): logical ports,
//! output devices and device ports, levels, test mode, setting
//! logical/physical port levels, night mode.

use pinscape_protocol::cmd;
use pinscape_protocol::outputs::{self as out, sub, DevicePortArgs, LogicalLevels, LogicalPort, OutputDevice, TestModeArgs};

use crate::{Result, VendorClient};

impl VendorClient {
    fn outputs_query(&mut self, subcmd: u8) -> Result<Vec<u8>> {
        Ok(self.transact(cmd::OUTPUTS, &[subcmd], &[])?.1)
    }

    /// Logical ports (as DOF sees them), with names fetched for named ports.
    pub fn query_logical_ports(&mut self) -> Result<Vec<LogicalPort>> {
        let data = self.outputs_query(sub::QUERY_LOGICAL_PORTS)?;
        let mut ports = LogicalPort::decode_all(&data)?;
        for p in ports.iter_mut().filter(|p| p.named) {
            let name = self.query_logical_port_name(p.port)?;
            p.name = (!name.is_empty()).then_some(name);
        }
        Ok(ports)
    }

    /// Name of a logical port (1-based); empty if it has none.
    pub fn query_logical_port_name(&mut self, port: u8) -> Result<String> {
        let (_, data) = self.transact(cmd::OUTPUTS, &[sub::QUERY_LOGICAL_PORT_NAME, port], &[])?;
        Ok(out::decode_port_name(&data))
    }

    /// Physical output devices, without port types (see [`Self::query_output_devices_full`]).
    pub fn query_output_devices(&mut self) -> Result<Vec<OutputDevice>> {
        let data = self.outputs_query(sub::QUERY_DEVICES)?;
        Ok(OutputDevice::decode_all(&data)?)
    }

    /// Per-port usage types for all device ports, concatenated in device order.
    pub fn query_output_device_port_types(&mut self) -> Result<Vec<u8>> {
        let data = self.outputs_query(sub::QUERY_DEVICE_PORTS)?;
        Ok(out::decode_device_port_types(&data)?)
    }

    /// Physical output devices with each device's `port_types` filled in.
    pub fn query_output_devices_full(&mut self) -> Result<Vec<OutputDevice>> {
        let mut devs = self.query_output_devices()?;
        let types = self.query_output_device_port_types()?;
        OutputDevice::attach_port_types(&mut devs, &types);
        Ok(devs)
    }

    /// Logical port levels plus the test-mode flag.
    pub fn query_logical_port_levels(&mut self) -> Result<LogicalLevels> {
        let data = self.outputs_query(sub::QUERY_LOGICAL_PORT_LEVELS)?;
        Ok(LogicalLevels::decode(&data)?)
    }

    /// Physical port levels on each device's native scale, in device order.
    pub fn query_device_port_levels(&mut self) -> Result<Vec<u16>> {
        let data = self.outputs_query(sub::QUERY_DEVICE_PORT_LEVELS)?;
        Ok(out::decode_device_levels(&data)?)
    }

    /// Set a logical port (1-based) as DOF would. No effect on the physical
    /// outputs while test mode is on.
    pub fn set_logical_port(&mut self, port: u8, level: u8) -> Result<()> {
        self.transact(cmd::OUTPUTS, &out::set_port_args(port, level), &[]).map(|_| ())
    }

    /// Enter or leave output test mode. Test mode disables the firmware's
    /// protective timers, so callers should use a short `timeout_ms` and
    /// repeat the command while they want to stay in test mode.
    pub fn set_output_test_mode(&mut self, enable: bool, timeout_ms: u32) -> Result<()> {
        let a = TestModeArgs { enable, timeout_ms };
        self.transact(cmd::OUTPUTS, &a.encode(), &[]).map(|_| ())
    }

    /// Set a physical device port directly (test mode only), native scale.
    pub fn set_device_port(&mut self, dev_type: u8, config_index: u8, port: u8, level: u16) -> Result<()> {
        let a = DevicePortArgs { dev_type, config_index, port, level };
        self.transact(cmd::OUTPUTS, &a.encode(), &[]).map(|_| ())
    }

    pub fn query_night_mode(&mut self) -> Result<bool> {
        let (r, _) = self.transact(cmd::OUTPUTS, &[sub::QUERY_NIGHT_MODE], &[])?;
        Ok(out::decode_night_mode(&r)?)
    }

    pub fn set_night_mode(&mut self, on: bool) -> Result<()> {
        self.transact(cmd::OUTPUTS, &out::set_night_mode_args(on), &[]).map(|_| ())
    }

    /// Reboot a PWMWorker Pico (index into the `workerPico` config array),
    /// optionally into its boot loader.
    pub fn pwm_worker_reset(&mut self, worker: u8, bootloader: bool) -> Result<()> {
        let s = if bootloader { sub::PWMWORKER_BOOTLOADER } else { sub::PWMWORKER_RESET };
        self.transact(cmd::OUTPUTS, &[s, worker], &[]).map(|_| ())
    }
}
