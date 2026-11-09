//! DOF config helper: the device's logical output ports as DOF (DirectOutput
//! Framework) sees them, plus problem checks and CSV / text exports.
//! (docs/PLAN.md §11, "DOF config helper".)
//!
//! What this is based on (upstream PinscapePico sources):
//! * `Firmware/JSONConfigRef.txt`, `outputs`: "Each logical output port has a
//!   number that DOF uses to address the port ... the first "outputs" array
//!   element is port 1, the second is port 2" and "it's how you identify the
//!   port when you're setting up devices in the DOF Config Tool".
//!   `outputs[].shareGroup`: the share-group ("virtual DOF") ports are the ones
//!   you assign "In your DOF Config Tool settings, under Cabinet > Devices >
//!   Port Assignments"; the physical pool ports aren't assigned in DOF.
//! * `Firmware/JSONConfigRef.txt`, `id.unitNum`: "The Unit Number is how DOF
//!   and other pinball software identifies the device ... Use the same number
//!   when you're setting up your DOF Config Tool settings for the device."
//!   Default 1; KL25Z Pinscape units are numbered separately.
//! * `GUIConfigTool/Help/NewDeviceSetup.htm`: "DOF expects that the Pinscape
//!   Picos are numbered sequentially starting at 1".
//! * `Firmware/JSONConfigRef.txt`, `feedbackController`: the Feedback
//!   Controller HID interface "is the interface that DOF uses"; it's enabled
//!   by default and normally only disabled for strict LedWiz emulation.
//! * `USBProtocol/FeedbackControllerProtocol.h`: SET OUTPUT PORT BLOCK / SET
//!   OUTPUT PORTS carry port numbers as one byte, so DOF can reach ports
//!   1..255 only.
//! * `Firmware/Outputs.cpp`: duplicate port names are ignored after the first
//!   (`portsByName.emplace`), so name references go to the first port.
//! * `GUIConfigTool/Help/OutputTester.htm`: computed ports "ignore" their DOF
//!   input unless the `source` formula uses it.
//!
//! The upstream docs don't describe a DOF `Cabinet.xml` format for Pinscape
//! Pico (DOF finds Picos itself by unit number; ports are assigned in the
//! online DOF Config Tool), so the exports here are a port table (CSV) and a
//! plain-text port list to work from, not DOF XML.
//!
//! Register the command in lib.rs `generate_handler!`.

use std::collections::BTreeMap;

use pinscape_protocol::outputs::{dev_type as dt, LogicalPort, OutputDevice};
use pinscape_usb as usb;
use serde::Serialize;
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// Highest port number the Feedback Controller protocol (one-byte port
/// numbers) lets DOF address.
pub const MAX_DOF_PORT: u32 = 255;

/// A Pinscape Pico as DOF identifies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DofUnit {
    pub unit_num: u8,
    pub unit_name: String,
    pub hwid: String,
}

/// One row of the mapping table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DofPort {
    /// DOF port number (1-based position in the `outputs` array).
    pub dof_port: u32,
    pub name: Option<String>,
    /// `physical`, `virtual`, `shareGroup`, `zbLaunch` or `unknown`.
    pub kind: &'static str,
    /// e.g. "Pico GPIO", "TLC5940[0]", "PCA9685[0] (I2C0 0x40)", "Share group #0".
    pub device: String,
    /// Pin on the device, e.g. "GP15", "OUT7"; empty for non-physical ports.
    pub pin: String,
    pub noisy: bool,
    pub gamma: bool,
    pub inverted: bool,
    pub flipper_logic: bool,
    pub computed: bool,
    /// DOF can address it (port number fits the protocol's one byte).
    pub addressable: bool,
}

impl DofPort {
    /// Flag words for display ("noisy", "gamma", ...).
    pub fn flag_words(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (on, w) in [
            (self.noisy, "noisy"),
            (self.gamma, "gamma"),
            (self.inverted, "inverted"),
            (self.flipper_logic, "flipper logic"),
            (self.computed, "computed"),
        ] {
            if on {
                v.push(w);
            }
        }
        v
    }

    /// "TLC5940[0] OUT7", "Pico GPIO GP15", "Virtual".
    pub fn output_label(&self) -> String {
        if self.pin.is_empty() {
            self.device.clone()
        } else {
            format!("{} {}", self.device, self.pin)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DofProblem {
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    /// Ports the problem concerns (empty for device-wide problems).
    pub ports: Vec<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DofReport {
    pub unit: DofUnit,
    /// Feedback Controller (DOF's) USB interface configured; `None` if unknown.
    pub feedback_enabled: Option<bool>,
    pub ports: Vec<DofPort>,
    pub problems: Vec<DofProblem>,
    /// The other Pinscape Picos currently connected.
    pub other_units: Vec<DofUnit>,
    pub csv: String,
    pub port_list: String,
}

/// Pin label on a device (same scheme as the Outputs tab, ui/src/lib/api-outputs.ts `pinLabel`).
pub fn pin_label(dev_type: u8, port: u8) -> String {
    match dev_type {
        dt::GPIO => format!("GP{port}"),
        dt::PCA9685 | dt::TLC59116 => format!("LED{port}"),
        dt::HC595 => format!("Q{port}"),
        dt::PCA9555 => format!("IO{}_{}", port >> 3, port & 7),
        _ => format!("OUT{port}"),
    }
}

fn kind_of(dev_type: u8) -> &'static str {
    match dev_type {
        dt::VIRTUAL => "virtual",
        dt::SHAREGROUP => "shareGroup",
        dt::ZBLAUNCH => "zbLaunch",
        dt::GPIO | dt::TLC59116 | dt::TLC5940 | dt::PCA9685 | dt::PCA9555 | dt::HC595 | dt::PWMWORKER | dt::TLC5947 => {
            "physical"
        }
        _ => "unknown",
    }
}

/// Device label (ui/src/lib/api-outputs.ts `deviceLabel`), with the I2C
/// address when the device list has it.
fn device_label(p: &LogicalPort, devices: &[OutputDevice]) -> String {
    match p.dev_type {
        dt::VIRTUAL => "Virtual".into(),
        dt::ZBLAUNCH => "ZB Launch".into(),
        dt::SHAREGROUP => format!("Share group #{}", p.dev_id),
        dt::GPIO => "Pico GPIO".into(),
        t => {
            let base = format!("{}[{}]", p.dev_type_name, p.dev_id);
            match devices.iter().find(|d| d.dev_type == t && d.config_index == p.dev_id) {
                Some(OutputDevice { i2c_bus: Some(b), i2c_addr: Some(a), .. }) => format!("{base} (I2C{b} 0x{a:02x})"),
                _ => base,
            }
        }
    }
}

/// Mapping rows. Port numbers come from list position (the device's own
/// one-byte numbering would wrap past 255).
pub fn build_ports(ports: &[LogicalPort], devices: &[OutputDevice]) -> Vec<DofPort> {
    ports
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let dof_port = i as u32 + 1;
            let kind = kind_of(p.dev_type);
            DofPort {
                dof_port,
                name: p.name.clone().filter(|n| !n.is_empty()),
                kind,
                device: device_label(p, devices),
                pin: if kind == "physical" { pin_label(p.dev_type, p.dev_port) } else { String::new() },
                noisy: p.noisy,
                gamma: p.gamma,
                inverted: p.inverted,
                flipper_logic: p.flipper_logic,
                computed: p.computed,
                addressable: dof_port <= MAX_DOF_PORT,
            }
        })
        .collect()
}

/// "#3", "#3 and #5", "#3, #5 and #9" (at most `MAX` listed, then "and N more").
fn port_refs(ports: &[u32]) -> String {
    const MAX: usize = 8;
    let mut v: Vec<String> = ports.iter().take(MAX).map(|p| format!("#{p}")).collect();
    if ports.len() > MAX {
        v.push(format!("{} more", ports.len() - MAX));
    }
    match v.len() {
        0 => String::new(),
        1 => v.remove(0),
        n => format!("{} and {}", v[..n - 1].join(", "), v[n - 1]),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Problems DOF users are likely to run into, most severe first.
pub fn find_problems(unit: &DofUnit, feedback_enabled: Option<bool>, ports: &[DofPort], others: &[DofUnit]) -> Vec<DofProblem> {
    let mut out = Vec::new();
    let mut add = |severity, ports: Vec<u32>, message: String| out.push(DofProblem { severity, ports, message });

    // --- Device-wide ---
    if feedback_enabled == Some(false) {
        add(
            "error",
            vec![],
            "The Feedback Controller USB interface is disabled (feedbackController.enable: false). That's the \
             interface DOF uses, so DOF can't see this Pico. Remove the setting unless you need strict LedWiz emulation."
                .into(),
        );
    }
    let same: Vec<&DofUnit> = others.iter().filter(|o| o.unit_num == unit.unit_num).collect();
    if !same.is_empty() {
        let names: Vec<String> = same.iter().map(|o| format!("\"{}\" ({})", o.unit_name, o.hwid)).collect();
        add(
            "error",
            vec![],
            format!(
                "Unit number conflict: {} also {} unit #{}. DOF tells Pinscape Picos apart by unit number, so give \
                 each one a different id.unitNum.",
                names.join(", "),
                if same.len() == 1 { "uses" } else { "use" },
                unit.unit_num
            ),
        );
    }
    if unit.unit_num == 0 {
        add("warning", vec![], "Unit number 0: number Pinscape Picos from 1 (id.unitNum: 1 for the first).".into());
    } else {
        let mut nums: Vec<u8> = others.iter().map(|o| o.unit_num).chain([unit.unit_num]).collect();
        nums.sort_unstable();
        nums.dedup();
        let sequential = nums.iter().enumerate().all(|(i, &n)| n as usize == i + 1);
        if !sequential {
            let list = nums.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
            if others.is_empty() {
                add(
                    "info",
                    vec![],
                    format!(
                        "This Pico is unit #{}. DOF expects Pinscape Picos to be numbered from 1; with a single Pico \
                         set id.unitNum: 1, or make sure the DOF Config Tool uses unit {} too.",
                        unit.unit_num, unit.unit_num
                    ),
                );
            } else {
                add(
                    "warning",
                    vec![],
                    format!(
                        "Connected Pinscape Picos use unit numbers {list}. DOF expects them numbered sequentially \
                         from 1 (1, 2, 3 …); KL25Z Pinscape units don't count."
                    ),
                );
            }
        }
    }
    if ports.is_empty() {
        add(
            "error",
            vec![],
            "No output ports are configured, so DOF has nothing to address. Add an outputs array to the config; \
             each entry is one DOF port, starting at #1."
                .into(),
        );
        return out;
    }

    // --- Per port ---
    let too_high: Vec<u32> = ports.iter().filter(|p| !p.addressable).map(|p| p.dof_port).collect();
    if !too_high.is_empty() {
        add(
            "error",
            too_high.clone(),
            format!(
                "{} beyond #{MAX_DOF_PORT} can't be addressed by DOF (the Feedback Controller protocol carries port \
                 numbers in one byte): {}.",
                plural(too_high.len(), "port is", "ports are"),
                port_refs(&too_high)
            ),
        );
    }

    let mut by_output: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for p in ports.iter().filter(|p| p.kind == "physical") {
        by_output.entry(p.output_label()).or_default().push(p.dof_port);
    }
    for (label, list) in by_output.iter().filter(|(_, l)| l.len() > 1) {
        add(
            "warning",
            list.clone(),
            format!(
                "Ports {} all drive {label}. DOF treats them as separate devices, so their effects will fight over \
                 the same output. (To share outputs on purpose, use a share group.)",
                port_refs(list)
            ),
        );
    }

    let mut by_name: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for p in ports {
        if let Some(n) = &p.name {
            by_name.entry(n.as_str()).or_default().push(p.dof_port);
        }
    }
    for (name, list) in by_name.iter().filter(|(_, l)| l.len() > 1) {
        add(
            "warning",
            list.clone(),
            format!(
                "Ports {} share the name \"{name}\". References to the name (e.g. in source formulas or button \
                 settings) reach only the first; give each port its own name.",
                port_refs(list)
            ),
        );
    }

    let unknown: Vec<u32> = ports.iter().filter(|p| p.kind == "unknown").map(|p| p.dof_port).collect();
    if !unknown.is_empty() {
        add(
            "warning",
            unknown.clone(),
            format!("Unrecognized output device type on {} (newer firmware?).", port_refs(&unknown)),
        );
    }

    let unnamed: Vec<u32> = ports.iter().filter(|p| p.name.is_none()).map(|p| p.dof_port).collect();
    if !unnamed.is_empty() {
        add(
            "info",
            unnamed.clone(),
            format!(
                "{} no name: {}. Names are optional, but a name: on each outputs entry makes it much easier to \
                 match ports up with the DOF Config Tool.",
                plural(unnamed.len(), "port has", "ports have"),
                port_refs(&unnamed)
            ),
        );
    }

    let computed: Vec<u32> = ports.iter().filter(|p| p.computed).map(|p| p.dof_port).collect();
    if !computed.is_empty() {
        add(
            "info",
            computed.clone(),
            format!(
                "Computed {} {}: a source formula sets the output, so DOF's level only matters if the formula uses \
                 it. Assign these in DOF only if that's intended.",
                if computed.len() == 1 { "port" } else { "ports" },
                port_refs(&computed)
            ),
        );
    }

    let shared: Vec<u32> = ports.iter().filter(|p| p.kind == "shareGroup").map(|p| p.dof_port).collect();
    if !shared.is_empty() {
        add(
            "info",
            shared.clone(),
            format!(
                "Share-group {} {}: assign these in the DOF Config Tool. The physical ports in the pool (outputs with \
                 a shareGroup property) don't need DOF assignments.",
                if shared.len() == 1 { "port" } else { "ports" },
                port_refs(&shared)
            ),
        );
    }

    let zb: Vec<u32> = ports.iter().filter(|p| p.kind == "zbLaunch").map(|p| p.dof_port).collect();
    if !zb.is_empty() {
        add(
            "info",
            zb.clone(),
            format!(
                "ZB Launch Mode {}: {}. Assign it to the ZB Launch output in the DOF Config Tool, so DOF turns it on \
                 for tables without a plunger.",
                if zb.len() == 1 { "port" } else { "ports" },
                port_refs(&zb)
            ),
        );
    }

    let rank = |s: &str| match s {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    };
    out.sort_by_key(|p| rank(p.severity)); // stable: keeps the order above within a severity
    out
}

/// One CSV field: quoted when it contains a comma, quote or line break
/// (same rule as ui/src/lib/api-export.ts `csvField`).
pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        ""
    }
}

/// The mapping table as CSV (LF line ends, trailing newline).
pub fn to_csv(unit: &DofUnit, ports: &[DofPort]) -> String {
    let header = [
        "Unit",
        "DOF port",
        "Name",
        "Type",
        "Device",
        "Pin",
        "Noisy (off in night mode)",
        "Gamma",
        "Inverted",
        "Flipper logic",
        "Computed",
        "DOF addressable",
    ];
    let mut s = header.join(",");
    s.push('\n');
    for p in ports {
        let row = [
            unit.unit_num.to_string(),
            p.dof_port.to_string(),
            p.name.clone().unwrap_or_default(),
            p.kind.to_string(),
            p.device.clone(),
            p.pin.clone(),
            yes(p.noisy).into(),
            yes(p.gamma).into(),
            yes(p.inverted).into(),
            yes(p.flipper_logic).into(),
            yes(p.computed).into(),
            if p.addressable { "yes" } else { "no" }.into(),
        ];
        s.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        s.push('\n');
    }
    s
}

/// Plain-text port list to work from in the DOF Config Tool.
pub fn to_port_list(unit: &DofUnit, ports: &[DofPort]) -> String {
    let mut s = format!(
        "Pinscape Pico unit #{} \"{}\" (hardware ID {})\n\
         DOF Config Tool: Cabinet > Devices > Port Assignments - select the Pinscape Pico with unit number {},\n\
         then assign each port below to its DOF device.\n\n",
        unit.unit_num, unit.unit_name, unit.hwid, unit.unit_num
    );
    let name_w = ports.iter().map(|p| p.name.as_deref().unwrap_or("(unnamed)").chars().count()).max().unwrap_or(0).max(4);
    let out_w = ports.iter().map(|p| p.output_label().chars().count()).max().unwrap_or(0).max(6);
    s.push_str(&format!("{:<5} {:<name_w$}  {:<out_w$}  Flags\n", "Port", "Name", "Output"));
    for p in ports {
        let mut flags = p.flag_words();
        if !p.addressable {
            flags.push("NOT ADDRESSABLE BY DOF");
        }
        let line = format!(
            "{:<5} {:<name_w$}  {:<out_w$}  {}",
            p.dof_port,
            p.name.as_deref().unwrap_or("(unnamed)"),
            p.output_label(),
            flags.join(", ")
        );
        s.push_str(line.trim_end());
        s.push('\n');
    }
    s
}

/// Assemble the report from query results.
pub fn build_report(
    unit: DofUnit,
    feedback_enabled: Option<bool>,
    ports: &[LogicalPort],
    devices: &[OutputDevice],
    other_units: Vec<DofUnit>,
) -> DofReport {
    let rows = build_ports(ports, devices);
    let problems = find_problems(&unit, feedback_enabled, &rows, &other_units);
    let csv = to_csv(&unit, &rows);
    let port_list = to_port_list(&unit, &rows);
    DofReport { unit, feedback_enabled, ports: rows, problems, other_units, csv, port_list }
}

/// Query the device and build the report (runs on the device worker).
fn query(c: &mut usb::VendorClient, other_units: Vec<DofUnit>) -> usb::Result<DofReport> {
    let ids = c.query_ids()?;
    let ports = c.query_logical_ports()?;
    let devices = c.query_output_devices()?;
    // Older firmware may not answer this; don't fail the whole report.
    let feedback = match c.query_usb_interfaces() {
        Ok(u) => Some(u.feedback_configured),
        Err(e) if e.is_disconnect() => return Err(e),
        Err(_) => None,
    };
    let unit = DofUnit { unit_num: ids.unit_num, unit_name: ids.unit_name, hwid: ids.hwid };
    Ok(build_report(unit, feedback, &ports, &devices, other_units))
}

/// DOF port mapping for one device, checked against the other connected Picos.
#[tauri::command]
pub async fn dof_report(mgr: State<'_, Manager>, key: String) -> Res<DofReport> {
    let others: Vec<DofUnit> = mgr
        .snapshot()
        .into_iter()
        .filter(|d| d.key != key)
        .filter_map(|d| d.ids)
        .map(|i| DofUnit { unit_num: i.unit_num, unit_name: i.unit_name, hwid: i.hwid })
        .collect();
    mgr.run(&key, move |c| query(c, others)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinscape_protocol::outputs::port_flags as pf;
    use pinscape_usb::mock::{MockDevice, MockState};

    fn unit(n: u8) -> DofUnit {
        DofUnit { unit_num: n, unit_name: format!("Pico {n}"), hwid: format!("HWID{n}") }
    }

    fn lp(i: usize, flags: u8, dev_type: u8, dev_id: u8, dev_port: u8, name: &str) -> LogicalPort {
        let mut p = LogicalPort::from_raw(i as u8, flags, dev_id, dev_type, dev_port);
        p.name = (!name.is_empty()).then(|| name.to_string());
        p
    }

    fn row(n: u32, name: Option<&str>, kind: &'static str, device: &str, pin: &str) -> DofPort {
        DofPort {
            dof_port: n,
            name: name.map(String::from),
            kind,
            device: device.into(),
            pin: pin.into(),
            noisy: false,
            gamma: false,
            inverted: false,
            flipper_logic: false,
            computed: false,
            addressable: n <= MAX_DOF_PORT,
        }
    }

    fn mock_report(state: MockState, others: Vec<DofUnit>) -> DofReport {
        let mut c = usb::VendorClient::with_transport(Box::new(MockDevice::with_state(state)));
        query(&mut c, others).unwrap()
    }

    #[test]
    fn mock_device_report() {
        let r = mock_report(MockState::default(), vec![]);
        assert_eq!(r.unit.unit_num, 9);
        assert_eq!(r.unit.unit_name, "Mock Pico");
        assert_eq!(r.feedback_enabled, Some(true));
        assert!(r.ports.len() >= 11);
        let p1 = &r.ports[0];
        assert_eq!((p1.dof_port, p1.name.as_deref(), p1.kind), (1, Some("Left Flipper"), "physical"));
        assert_eq!((p1.device.as_str(), p1.pin.as_str()), ("Pico GPIO", "GP15"));
        assert!(p1.noisy && p1.flipper_logic && !p1.gamma);
        assert_eq!(r.ports[3].output_label(), "TLC5940[0] OUT0");
        assert_eq!(r.ports[6].output_label(), "PCA9685[0] (I2C0 0x40) LED0");
        assert_eq!(r.ports[8].output_label(), "74HC595[0] Q0");
        assert!(r.ports[7].inverted && r.ports[8].computed);
        assert_eq!((r.ports[10].kind, r.ports[10].output_label().as_str()), ("virtual", "Virtual"));
        assert!(r.ports.iter().any(|p| p.kind == "shareGroup"), "mock has share-group ports");
        assert!(r.ports.iter().any(|p| p.kind == "zbLaunch"), "mock has a ZB Launch port");
        // problems: unit #9 alone -> info; unnamed #6/#11; computed #9; share group; no errors
        assert!(r.problems.iter().all(|p| p.severity != "error"), "{:?}", r.problems);
        let unnamed = r.problems.iter().find(|p| p.message.contains("no name")).unwrap();
        assert_eq!(unnamed.ports, vec![6, 11]);
        assert!(r.problems.iter().any(|p| p.message.contains("unit #9")));
        assert!(r.problems.iter().any(|p| p.ports == vec![9] && p.message.contains("Computed")));
        // exports
        assert!(r.csv.starts_with("Unit,DOF port,Name,"));
        assert_eq!(r.csv.lines().count(), r.ports.len() + 1);
        assert!(r.csv.contains("\n9,1,Left Flipper,physical,Pico GPIO,GP15,yes,,,yes,,yes\n"));
        assert!(r.csv.contains("\n9,7,Shaker Motor,physical,PCA9685[0] (I2C0 0x40),LED0,yes,"), "{}", r.csv);
        assert!(r.port_list.contains("unit #9 \"Mock Pico\""));
        assert!(r.port_list.contains("Cabinet > Devices > Port Assignments"));
        // camelCase for the UI
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["ports"][0]["dofPort"], 1);
        assert_eq!(j["ports"][0]["flipperLogic"], true);
        assert_eq!(j["unit"]["unitNum"], 9);
        assert!(j["feedbackEnabled"].is_boolean() && j["portList"].is_string() && j["otherUnits"].is_array());
    }

    #[test]
    fn mock_unit_conflict_and_numbering() {
        // another Pico with the same unit number
        let r = mock_report(MockState::default(), vec![unit(9)]);
        let e = r.problems.iter().find(|p| p.severity == "error").unwrap();
        assert!(e.message.contains("Unit number conflict") && e.message.contains("HWID9"), "{}", e.message);
        assert_eq!(r.problems[0].severity, "error", "errors sorted first");
        // units 1 and 9: not sequential
        let r = mock_report(MockState::default(), vec![unit(1)]);
        assert!(r.problems.iter().any(|p| p.severity == "warning" && p.message.contains("1, 9")));
        // unit 1, alone: no numbering note
        let mut st = MockState::default();
        st.unit_num = 1;
        let r = mock_report(st, vec![]);
        assert!(!r.problems.iter().any(|p| p.message.contains("unit #") || p.message.contains("unit numbers")));
    }

    #[test]
    fn mock_unconfigured_outputs() {
        let mut st = MockState::default();
        st.unit_num = 1;
        st.outputs.ports.clear();
        let r = mock_report(st, vec![]);
        assert!(r.ports.is_empty());
        assert_eq!(r.problems.len(), 1);
        assert!(r.problems[0].severity == "error" && r.problems[0].message.contains("No output ports"));
        assert_eq!(r.csv.lines().count(), 1);
    }

    #[test]
    fn numbering_by_position_and_255_limit() {
        // 300 virtual ports: the device's u8 port field wraps; we number by position.
        let ports: Vec<LogicalPort> = (0..300).map(|i| lp(i + 1, 0, dt::VIRTUAL, 0, 0, &format!("V{i}"))).collect();
        let rows = build_ports(&ports, &[]);
        assert_eq!(rows[255].dof_port, 256);
        assert!(rows[254].addressable && !rows[255].addressable);
        let probs = find_problems(&unit(1), Some(true), &rows, &[]);
        let e = &probs[0];
        assert_eq!(e.severity, "error");
        assert_eq!(e.ports.len(), 45);
        assert!(e.message.contains("45 ports are beyond #255") && e.message.contains("and 37 more"), "{}", e.message);
        assert!(to_port_list(&unit(1), &rows).contains("NOT ADDRESSABLE BY DOF"));
        assert!(to_csv(&unit(1), &rows).contains("\n1,256,V255,virtual,Virtual,,,,,,,no\n"));
    }

    #[test]
    fn duplicate_outputs_and_names() {
        let ports = vec![
            lp(1, 0, dt::GPIO, 0, 5, "Shaker"),
            lp(2, 0, dt::GPIO, 0, 5, "Shaker"),
            lp(3, 0, dt::GPIO, 0, 6, "Gear"),
            // virtual / share-group ports never count as duplicate outputs
            lp(4, 0, dt::VIRTUAL, 0, 0, "A"),
            lp(5, 0, dt::VIRTUAL, 0, 0, "B"),
            lp(6, 0, dt::SHAREGROUP, 0, 0, "C"),
            lp(7, 0, dt::SHAREGROUP, 0, 0, "D"),
        ];
        let rows = build_ports(&ports, &[]);
        let probs = find_problems(&unit(1), Some(true), &rows, &[]);
        let dup_out: Vec<_> = probs.iter().filter(|p| p.message.contains("all drive")).collect();
        assert_eq!(dup_out.len(), 1);
        assert_eq!(dup_out[0].ports, vec![1, 2]);
        assert!(dup_out[0].message.contains("Pico GPIO GP5") && dup_out[0].message.contains("#1 and #2"));
        let dup_name: Vec<_> = probs.iter().filter(|p| p.message.contains("share the name")).collect();
        assert_eq!(dup_name.len(), 1);
        assert_eq!(dup_name[0].ports, vec![1, 2]);
        let sg = probs.iter().find(|p| p.message.starts_with("Share-group")).unwrap();
        assert_eq!(sg.ports, vec![6, 7]);
        assert!(probs.iter().all(|p| p.severity != "error"));
    }

    #[test]
    fn feedback_disabled_and_unit_zero() {
        let rows = vec![row(1, Some("x"), "physical", "Pico GPIO", "GP1")];
        let probs = find_problems(&unit(0), Some(false), &rows, &[]);
        assert_eq!(probs[0].severity, "error");
        assert!(probs[0].message.contains("Feedback Controller"));
        assert!(probs.iter().any(|p| p.severity == "warning" && p.message.contains("Unit number 0")));
        // unknown: no complaint
        assert!(find_problems(&unit(1), None, &rows, &[]).is_empty());
    }

    #[test]
    fn sequential_units_are_fine() {
        let rows = vec![row(1, Some("x"), "physical", "Pico GPIO", "GP1")];
        assert!(find_problems(&unit(2), Some(true), &rows, &[unit(1), unit(3)]).is_empty());
        let p = find_problems(&unit(2), Some(true), &rows, &[unit(3)]);
        assert!(p[0].message.contains("2, 3"));
    }

    #[test]
    fn device_labels_and_pins() {
        let devs = OutputDevice::decode_all(&pinscape_protocol::outputs::encode_devices(&[
            pinscape_protocol::outputs::RawDevDesc {
                addr: 0x120,
                num_ports: 16,
                num_ports_per_chip: 16,
                pwm_res: 2,
                config_index: 1,
                dev_type: dt::PCA9555,
            },
        ]))
        .unwrap();
        let ports = vec![
            lp(1, pf::NOISY, dt::PCA9555, 1, 11, "Coin"),
            lp(2, 0, dt::PCA9555, 0, 3, "Other chip"),
            lp(3, 0, dt::ZBLAUNCH, 0, 0, "ZB"),
            lp(4, 0, dt::TLC5947, 2, 23, ""),
            lp(5, 0, 99, 0, 0, "Future"),
        ];
        let rows = build_ports(&ports, &devs);
        assert_eq!(rows[0].output_label(), "PCA9555[1] (I2C1 0x20) IO1_3");
        assert_eq!(rows[1].output_label(), "PCA9555[0] IO0_3", "no device entry -> no address");
        assert_eq!((rows[2].kind, rows[2].pin.as_str()), ("zbLaunch", ""));
        assert_eq!(rows[3].output_label(), "TLC5947[2] OUT23");
        assert_eq!(rows[4].kind, "unknown");
        let probs = find_problems(&unit(1), Some(true), &rows, &[]);
        assert!(probs.iter().any(|p| p.ports == vec![5] && p.severity == "warning"));
        assert!(probs.iter().any(|p| p.ports == vec![3] && p.message.contains("ZB Launch")));
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field(""), "");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_field("cr\r"), "\"cr\r\"");
        let mut r = row(1, Some("Flasher, \"left\""), "physical", "Pico GPIO", "GP1");
        r.gamma = true;
        let csv = to_csv(&unit(2), &[r]);
        assert_eq!(csv.lines().nth(1).unwrap(), "2,1,\"Flasher, \"\"left\"\"\",physical,Pico GPIO,GP1,,yes,,,,yes");
        assert!(csv.ends_with('\n'));
    }

    #[test]
    fn port_list_layout() {
        let mut a = row(1, Some("Left Flipper"), "physical", "Pico GPIO", "GP15");
        a.noisy = true;
        a.flipper_logic = true;
        let b = row(12, None, "virtual", "Virtual", "");
        let t = to_port_list(&unit(1), &[a, b]);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "Pinscape Pico unit #1 \"Pico 1\" (hardware ID HWID1)");
        assert!(lines[1].contains("select the Pinscape Pico with unit number 1"));
        assert_eq!(lines[4], "Port  Name          Output          Flags");
        assert_eq!(lines[5], "1     Left Flipper  Pico GPIO GP15  noisy, flipper logic");
        assert_eq!(lines[6], "12    (unnamed)     Virtual");
    }

    #[test]
    fn port_refs_text() {
        assert_eq!(port_refs(&[3]), "#3");
        assert_eq!(port_refs(&[3, 5]), "#3 and #5");
        assert_eq!(port_refs(&[1, 2, 3]), "#1, #2 and #3");
    }
}
