//! IR: Tauri commands for CMD_QUERY_IR (0x14), CMD_SEND_IR (0x0D), CMD_TVON (0x0A): received IR commands and raw pulses, sending IR, TV-ON state and relay. (Phase 3)
//! Register new commands in lib.rs `generate_handler!`.

use pinscape_protocol::ir::{self, IrCode, IrCommandReceived, IrRawPulse, TvOnState, TvRelayAction};
use pinscape_usb as usb;
use serde::Serialize;
use tauri::State;

use crate::session::Manager;

type Res<T> = Result<T, String>;

/// One poll of the IR tester: new decoded commands, new raw pulses and the
/// TV ON state. A failure in one part doesn't hide the others.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IrPoll {
    pub commands: Vec<IrCommandReceived>,
    pub raw: Vec<IrRawPulse>,
    pub ir_error: Option<String>,
    pub tvon: Option<TvOnState>,
    pub tvon_error: Option<String>,
}

/// Keep a per-query error as text, but let disconnects through so the
/// session worker notices the device is gone.
fn soft<T>(r: usb::Result<T>) -> usb::Result<Result<T, String>> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) if e.is_disconnect() => Err(e),
        Err(e) => Ok(Err(e.to_string())),
    }
}

fn poll(c: &mut usb::VendorClient) -> usb::Result<IrPoll> {
    let mut p = IrPoll::default();
    match soft(c.query_ir_commands())? {
        Ok(v) => p.commands = v,
        Err(e) => p.ir_error = Some(e),
    }
    if p.ir_error.is_none() {
        match soft(c.query_ir_raw())? {
            Ok(v) => p.raw = v,
            Err(e) => p.ir_error = Some(e),
        }
    }
    match soft(c.query_tvon_state())? {
        Ok(t) => p.tvon = Some(t),
        Err(e) => p.tvon_error = Some(e),
    }
    Ok(p)
}

/// Drain received commands and raw pulses, and read the TV ON state.
#[tauri::command]
pub async fn ir_poll(mgr: State<'_, Manager>, key: String) -> Res<IrPoll> {
    mgr.run(&key, poll).await
}

/// Send a code in the universal format (`PP.FF.CCCC…`) `count` times (1..=255).
/// Returns the parsed code.
#[tauri::command]
pub async fn ir_send(mgr: State<'_, Manager>, key: String, code: String, count: u8) -> Res<IrCode> {
    let c: IrCode = code.parse().map_err(|e: ir::IrCodeParseError| e.to_string())?;
    if count == 0 {
        return Err("repeat count must be 1 to 255".into());
    }
    mgr.run(&key, move |cl| cl.send_ir(c, count)).await.map_err(|e| {
        if e.contains("status 1:") {
            format!("{e} — is an IR transmitter (irTx) configured? The firmware also refuses while its send queue is full.")
        } else {
            e
        }
    })?;
    Ok(c)
}

/// Manual TV relay control: `on`, `off` or `pulse`.
#[tauri::command]
pub async fn ir_set_tv_relay(mgr: State<'_, Manager>, key: String, action: String) -> Res<()> {
    let a = match action.as_str() {
        "on" => TvRelayAction::On,
        "off" => TvRelayAction::Off,
        "pulse" => TvRelayAction::Pulse,
        other => return Err(format!("unknown relay action '{other}'")),
    };
    mgr.run(&key, move |c| c.set_tv_relay(a)).await
}

/// Which GPIOs the firmware has assigned to the IR receiver and transmitter
/// (from the GPIO usage strings "IR RX" / "IR TX"); `None` if not configured.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IrConfigStatus {
    pub rx_gpio: Option<u8>,
    pub tx_gpio: Option<u8>,
}

fn config_status(gpio: &[pinscape_protocol::GpioPort]) -> IrConfigStatus {
    let find = |tag: &str| {
        gpio.iter()
            .find(|p| p.usage.as_deref().is_some_and(|u| u.to_ascii_uppercase().contains(tag)))
            .map(|p| p.gpio)
    };
    IrConfigStatus { rx_gpio: find("IR RX"), tx_gpio: find("IR TX") }
}

#[tauri::command]
pub async fn ir_config_status(mgr: State<'_, Manager>, key: String) -> Res<IrConfigStatus> {
    mgr.run(&key, |c| Ok(config_status(&c.query_gpio_config()?))).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedIrCode {
    pub code: IrCode,
    /// Canonical text (upper-case, standard padding).
    pub text: String,
    pub protocol_name: &'static str,
    pub use_dittos: bool,
}

/// Validate a code typed by the user (no device access).
#[tauri::command]
pub fn ir_parse_code(text: String) -> Res<ParsedIrCode> {
    let code: IrCode = text.parse().map_err(|e: ir::IrCodeParseError| e.to_string())?;
    Ok(ParsedIrCode {
        text: code.to_string(),
        protocol_name: ir::protocol_name(code.protocol),
        use_dittos: code.use_dittos(),
        code,
    })
}

#[derive(Debug, Serialize)]
pub struct IrProtocolInfo {
    pub id: u8,
    pub name: &'static str,
}

/// Known IR protocol IDs and names.
#[tauri::command]
pub fn ir_protocols() -> Vec<IrProtocolInfo> {
    ir::PROTOCOLS.iter().map(|&(id, name)| IrProtocolInfo { id, name }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinscape_usb::mock::MockDevice;

    #[test]
    fn poll_against_mock_serializes_camel_case() {
        let dev = MockDevice::new();
        let h = dev.handle();
        h.lock().unwrap().ir.auto_receive = false;
        let mut c = usb::VendorClient::with_transport(Box::new(dev));
        c.send_ir("01.02.FFFFFFFFFFFFFFFF".parse().unwrap(), 2).unwrap();
        let p = poll(&mut c).unwrap();
        assert_eq!(p.commands.len(), 2);
        assert!(p.tvon.is_some() && p.ir_error.is_none());
        let j = serde_json::to_value(&p).unwrap();
        let e = &j["commands"][0];
        assert_eq!(e["code"]["code"], "FFFFFFFFFFFFFFFF", "u64 codes go to JS as hex strings");
        assert_eq!(e["text"], "01.02.FFFFFFFFFFFFFFFF");
        assert_eq!(e["protocolName"], "NEC-32");
        assert_eq!(j["commands"][1]["autoRepeat"], true);
        assert!(j["raw"][0]["tUs"].is_null());
        assert!(j["tvon"]["powerStateName"].is_string());
    }

    #[test]
    fn parse_code_command() {
        let p = ir_parse_code("1b.0.2c".into()).unwrap_err();
        assert!(p.contains("invalid IR code"));
        let p = ir_parse_code(" 1b.00.2c ".into()).unwrap();
        assert_eq!(p.text, "1B.00.002C");
        assert_eq!(p.protocol_name, "Ortek MCE");
        assert!(ir_protocols().iter().any(|p| p.id == 1));
    }

    #[test]
    fn gpio_status() {
        let port = |gpio: u8, usage: Option<&str>| pinscape_protocol::GpioPort {
            gpio,
            func: 5,
            func_name: "SIO",
            output: false,
            adc: false,
            usage: usage.map(str::to_owned),
        };
        let s = config_status(&[port(0, Some("Button #1")), port(4, Some("IR TX")), port(9, Some("IR RX"))]);
        assert_eq!(s, IrConfigStatus { rx_gpio: Some(9), tx_gpio: Some(4) });
        assert_eq!(config_status(&[port(0, None)]), IrConfigStatus::default());
    }
}
