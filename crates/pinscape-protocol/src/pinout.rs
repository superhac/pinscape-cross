//! Pico board pinout and GPIO-usage classification, for the visual pinout
//! view. Pure logic on top of [`GpioPort`] (from `QUERY_GPIO_CONFIG`) and
//! [`DeviceIds`]; no I/O.
//!
//! The standard Raspberry Pi Pico, Pico W, Pico 2 and Pico 2 W share one
//! 40-pin header layout, which [`HEADER`] describes. Any other board (e.g.
//! an RP2350B board with 48 GPIOs, or a third-party layout) gets no header
//! diagram: [`Pinout::header`] is empty and the UI shows a plain GPIO list.

use serde::Serialize;

use crate::{DeviceIds, GpioPort};

/// What a GPIO is used for, as shown by the pinout view's colours/legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Button / input: GPIO buttons, input shift registers and expanders, gamepad axis inputs.
    Button,
    /// Output port or output controller chip (PWM, shift register, LED driver).
    Output,
    I2c,
    Spi,
    Uart,
    /// IR receiver or transmitter.
    Ir,
    /// Plunger sensor or analog input (Pico ADC, ADS1115, image/distance/quadrature sensors).
    Plunger,
    /// Accelerometer (nudge) interrupt line.
    Accel,
    /// Board/firmware internal: on-board LED, status LED, power control, or a
    /// GPIO the board wires to something other than the header.
    System,
    /// In use, but not recognised (or no Pinscape label).
    Other,
    Free,
}

/// Kind of a header pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PinKind {
    Gpio,
    Ground,
    Power,
    Control,
}

/// One pin of the 40-pin header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderPin {
    /// Physical pin number, 1..=40 (1-20 down the left side with the USB
    /// connector at the top, 21-40 up the right side).
    pub pin: u8,
    pub label: &'static str,
    pub kind: PinKind,
    pub gpio: Option<u8>,
    /// ADC channel, for GP26-GP28.
    pub adc_channel: Option<u8>,
}

/// Header labels by physical pin number (index 0 = pin 1). Identical for
/// Pico, Pico W, Pico 2 and Pico 2 W.
const HEADER_LABELS: [&str; 40] = [
    "GP0", "GP1", "GND", "GP2", "GP3", "GP4", "GP5", "GND", "GP6", "GP7", // 1-10
    "GP8", "GP9", "GND", "GP10", "GP11", "GP12", "GP13", "GND", "GP14", "GP15", // 11-20
    "GP16", "GP17", "GND", "GP18", "GP19", "GP20", "GP21", "GND", "GP22", "RUN", // 21-30
    "GP26", "GP27", "GND", "GP28", "ADC_VREF", "3V3(OUT)", "3V3_EN", "GND", "VSYS", "VBUS", // 31-40
];

/// The standard Pico 40-pin header, in physical pin order.
pub fn header() -> Vec<HeaderPin> {
    HEADER_LABELS
        .iter()
        .enumerate()
        .map(|(i, &label)| {
            let gpio = label.strip_prefix("GP").and_then(|n| n.parse::<u8>().ok());
            let kind = match label {
                _ if gpio.is_some() => PinKind::Gpio,
                "GND" => PinKind::Ground,
                "RUN" | "3V3_EN" => PinKind::Control,
                _ => PinKind::Power,
            };
            HeaderPin { pin: i as u8 + 1, label, kind, gpio, adc_channel: gpio.and_then(adc_channel) }
        })
        .collect()
}

/// ADC channel for a GPIO on RP2040 / RP2350A (GP26-GP29 = ADC0-ADC3).
pub fn adc_channel(gpio: u8) -> Option<u8> {
    (26..=29).contains(&gpio).then(|| gpio - 26)
}

/// A board with the standard Pico header layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    Pico,
    PicoW,
    Pico2,
    Pico2W,
}

impl Board {
    /// Identify a standard Pico from the firmware's build target (`PICO_BOARD`)
    /// and CPU type. Returns `None` for anything else, including a known name
    /// with the wrong CPU.
    pub fn detect(target_board: &str, cpu_type: u16) -> Option<Board> {
        let b = target_board.trim().to_ascii_lowercase().replace('-', "_");
        match (b.as_str(), cpu_type) {
            ("pico", 2040) => Some(Board::Pico),
            ("pico_w", 2040) => Some(Board::PicoW),
            ("pico2", 2350) => Some(Board::Pico2),
            ("pico2_w", 2350) => Some(Board::Pico2W),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Board::Pico => "Raspberry Pi Pico",
            Board::PicoW => "Raspberry Pi Pico W",
            Board::Pico2 => "Raspberry Pi Pico 2",
            Board::Pico2W => "Raspberry Pi Pico 2 W",
        }
    }

    fn wireless(self) -> bool {
        matches!(self, Board::PicoW | Board::Pico2W)
    }

    /// What the board itself wires a GPIO to, for GPIOs not on the header.
    pub fn internal_use(self, gpio: u8) -> Option<&'static str> {
        match (gpio, self.wireless()) {
            (23, false) => Some("SMPS power-save control (PS)"),
            (24, false) => Some("VBUS sense"),
            (25, false) => Some("on-board LED"),
            (29, false) => Some("VSYS/3 voltage measurement (ADC3)"),
            (23, true) => Some("wireless chip power (WL_ON)"),
            (24, true) => Some("wireless chip SPI data (WL_D)"),
            (25, true) => Some("wireless chip SPI chip select (WL_CS)"),
            (29, true) => Some("wireless chip SPI clock (WL_CLK) / VSYS/3 (ADC3)"),
            _ => None,
        }
    }
}

const FUNC_SPI: u8 = 1;
const FUNC_UART: u8 = 2;
const FUNC_I2C: u8 = 3;
const FUNC_PWM: u8 = 4;
const FUNC_SIO: u8 = 5;
const FUNC_NULL: u8 = 0x1F;

/// Category from the firmware's usage string (the names its GPIO manager
/// assigns, e.g. "Button #3", "Output #2 (Knocker)", "I2C0 SDA",
/// "TLC5940[0] (SCLK)", "MMA8451Q (Interrupt)", "Pico ADC"), falling back on
/// the pin's hardware function when the string isn't recognised.
pub fn classify(port: &GpioPort) -> Category {
    let Some(usage) = port.usage.as_deref().map(str::to_ascii_lowercase) else {
        return match port.func {
            _ if port.adc => Category::Plunger,
            FUNC_NULL => Category::Free,
            f => by_func(f).unwrap_or(Category::Other),
        };
    };
    let u = usage.as_str();
    let has = |keys: &[&str]| keys.iter().any(|k| u.contains(k));
    if u.starts_with("button") || has(&["74hc165", "pca9555", "gpio() axis"]) {
        Category::Button
    } else if u.starts_with("output") || has(&["tlc5940", "tlc5947", "tlc59116", "74hc595", "pca9685"]) {
        Category::Output
    } else if u.starts_with("ir rx") || u.starts_with("ir tx") {
        Category::Ir
    } else if has(&["mma8451q", "lis3dh", "lis3dsh", "mxc6655", "mc3416", "mpu6050", "accel"]) {
        Category::Accel
    } else if has(&[
        "pico adc", "ads1115", "tcd1103", "tsl1410", "tsl1412", "plunger", "vl6180x", "vcnl4010", "aedr8300", "(cha)",
        "(chb)",
    ]) {
        Category::Plunger
    } else if has(&["pico led", "status led", "power enable"]) {
        Category::System
    } else if u.starts_with("i2c") {
        Category::I2c
    } else if u.starts_with("spi") {
        Category::Spi
    } else if u.starts_with("uart") {
        Category::Uart
    } else if port.adc {
        Category::Plunger
    } else {
        by_func(port.func).unwrap_or(Category::Other)
    }
}

fn by_func(func: u8) -> Option<Category> {
    match func {
        FUNC_SPI => Some(Category::Spi),
        FUNC_UART => Some(Category::Uart),
        FUNC_I2C => Some(Category::I2c),
        FUNC_PWM => Some(Category::Output),
        _ => None,
    }
}

/// Short description of the pin's hardware state, e.g. "GPIO input", "PWM", "ADC input".
pub fn func_detail(port: &GpioPort) -> String {
    match port.func {
        _ if port.adc => "ADC input".into(),
        FUNC_SIO if port.output => "GPIO output".into(),
        FUNC_SIO => "GPIO input".into(),
        FUNC_NULL => "no function selected".into(),
        _ => port.func_name.to_string(),
    }
}

/// One GPIO with everything the pinout view shows about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinInfo {
    pub gpio: u8,
    pub func: u8,
    pub func_name: &'static str,
    pub output: bool,
    pub adc: bool,
    pub usage: Option<String>,
    pub category: Category,
    /// Physical header pin, when the board has the standard layout.
    pub header_pin: Option<u8>,
    pub adc_channel: Option<u8>,
    /// What the board wires this GPIO to, if it isn't on the header.
    pub board_use: Option<&'static str>,
    /// Hardware state, e.g. "GPIO input".
    pub detail: String,
    /// Full one-line description (tooltips, screen readers).
    pub summary: String,
    /// Something looks wrong with this assignment.
    pub warning: Option<String>,
}

/// The pinout view's model: board, header, and every reported GPIO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pinout {
    /// "RP2040" / "RP2350".
    pub chip: String,
    /// Firmware build target as reported (`PICO_BOARD`).
    pub target_board: String,
    /// Board name, when it has the standard Pico header.
    pub board: Option<&'static str>,
    /// The 40-pin header; empty when the board isn't a standard Pico layout.
    pub header: Vec<HeaderPin>,
    pub pins: Vec<PinInfo>,
    /// Unassigned GPIOs a user can wire to (header pins only, on a standard board).
    pub free: Vec<u8>,
    /// Why no diagram is shown, if not.
    pub fallback_reason: Option<String>,
}

impl Pinout {
    pub fn build(ids: &DeviceIds, ports: &[GpioPort]) -> Pinout {
        let chip = format!("RP{}", ids.cpu_type);
        let mut board = Board::detect(&ids.target_board, ids.cpu_type);
        let mut fallback_reason = None;
        if board.is_none() {
            fallback_reason = Some(format!(
                "Board \"{}\" ({chip}) isn't a standard Pico / Pico 2 layout, so no header diagram is shown.",
                ids.target_board
            ));
        } else if ports.len() != 30 {
            fallback_reason =
                Some(format!("The device reported {} GPIOs instead of 30, so no header diagram is shown.", ports.len()));
            board = None;
        }
        let header = if board.is_some() { header() } else { Vec::new() };
        let header_pin = |gpio: u8| header.iter().find(|h| h.gpio == Some(gpio)).map(|h| h.pin);
        // ADC pins are fixed on RP2040; an unknown RP2350 board could be an
        // RP2350B, whose ADC pins are GP40-GP47.
        let adc_known = board.is_some() || ids.cpu_type == 2040;

        let pins: Vec<PinInfo> = ports
            .iter()
            .map(|p| {
                let board_use = board.and_then(|b| b.internal_use(p.gpio));
                let hp = header_pin(p.gpio);
                let mut category = classify(p);
                let mut warning = None;
                if let Some(bu) = board_use {
                    let expected = p.usage.is_none() || (bu == "on-board LED" && category == Category::System);
                    if !expected {
                        warning = Some(format!(
                            "GP{} isn't on the header: the {} wires it to the {bu}.",
                            p.gpio,
                            board.map(Board::name).unwrap_or("board")
                        ));
                    } else if category == Category::Free || category == Category::Other {
                        category = Category::System;
                    }
                }
                let adc_channel = if adc_known { adc_channel(p.gpio) } else { None };
                let detail = func_detail(p);
                let mut summary = format!("GP{}", p.gpio);
                if let Some(n) = hp {
                    summary += &format!(" (pin {n})");
                }
                summary += ": ";
                match (&p.usage, board_use) {
                    (Some(u), _) => summary += u,
                    (None, Some(bu)) => summary += &format!("board internal, {bu}"),
                    (None, None) if category == Category::Free => summary += "free",
                    (None, None) => summary += "in use, no Pinscape label",
                }
                if category != Category::Free {
                    summary += &format!(" · {detail}");
                }
                if let Some(ch) = adc_channel {
                    summary += &format!(" · ADC{ch}-capable");
                }
                if let Some(w) = &warning {
                    summary += &format!(" · Warning: {w}");
                }
                PinInfo {
                    gpio: p.gpio,
                    func: p.func,
                    func_name: p.func_name,
                    output: p.output,
                    adc: p.adc,
                    usage: p.usage.clone(),
                    category,
                    header_pin: hp,
                    adc_channel,
                    board_use,
                    detail,
                    summary,
                    warning,
                }
            })
            .collect();
        let free = pins
            .iter()
            .filter(|p| p.category == Category::Free && (board.is_none() || p.header_pin.is_some()))
            .map(|p| p.gpio)
            .collect();
        Pinout {
            chip,
            target_board: ids.target_board.clone(),
            board: board.map(Board::name),
            header,
            pins,
            free,
            fallback_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(gpio: u8, func: u8, output: bool, adc: bool, usage: Option<&str>) -> GpioPort {
        GpioPort { gpio, func, func_name: GpioPort::func_name(func), output, adc, usage: usage.map(Into::into) }
    }

    fn used(usage: &str, func: u8) -> Category {
        classify(&port(0, func, false, false, Some(usage)))
    }

    fn ids(board: &str, cpu: u16) -> DeviceIds {
        DeviceIds {
            hwid: "0".into(),
            cpu_type: cpu,
            cpu_version: 2,
            rom_version: 3,
            unit_num: 1,
            xinput_player_index: None,
            led_wiz_unit_mask: 0,
            unit_name: "t".into(),
            target_board: board.into(),
            pico_sdk_version: String::new(),
            tinyusb_version: String::new(),
            compiler_version: String::new(),
        }
    }

    fn all_free() -> Vec<GpioPort> {
        (0..30).map(|g| port(g, FUNC_NULL, false, false, None)).collect()
    }

    #[test]
    fn header_layout() {
        let h = header();
        assert_eq!(h.len(), 40);
        assert!(h.iter().enumerate().all(|(i, p)| p.pin as usize == i + 1));
        let gpios: Vec<u8> = h.iter().filter_map(|p| p.gpio).collect();
        let mut sorted = gpios.clone();
        sorted.sort();
        // GP0-GP22 and GP26-GP28, each exactly once
        let want: Vec<u8> = (0..=22).chain(26..=28).collect();
        assert_eq!(sorted, want);
        assert_eq!(h.iter().filter(|p| p.kind == PinKind::Ground).count(), 8);
        let at = |n: usize| &h[n - 1];
        assert_eq!((at(1).label, at(20).label, at(21).label, at(40).label), ("GP0", "GP15", "GP16", "VBUS"));
        assert_eq!(at(30).kind, PinKind::Control);
        assert_eq!(at(36).label, "3V3(OUT)");
        assert_eq!(at(36).kind, PinKind::Power);
        assert_eq!((at(31).adc_channel, at(32).adc_channel, at(34).adc_channel), (Some(0), Some(1), Some(2)));
        assert_eq!(at(29).adc_channel, None);
    }

    #[test]
    fn board_detection() {
        assert_eq!(Board::detect("pico", 2040), Some(Board::Pico));
        assert_eq!(Board::detect("PICO_W", 2040), Some(Board::PicoW));
        assert_eq!(Board::detect("pico2", 2350), Some(Board::Pico2));
        assert_eq!(Board::detect("pico2_w", 2350), Some(Board::Pico2W));
        assert_eq!(Board::detect("pico2", 2040), None, "name/CPU mismatch");
        assert_eq!(Board::detect("pimoroni_pico_plus2_rp2350", 2350), None);
        assert_eq!(Board::detect("", 2040), None);
    }

    #[test]
    fn classify_firmware_usage_strings() {
        assert_eq!(used("Button #3", FUNC_SIO), Category::Button);
        assert_eq!(used("74HC165[0] (QH)", FUNC_SIO), Category::Button);
        assert_eq!(used("PCA9555[1] (Intr)", FUNC_SIO), Category::Button);
        assert_eq!(used("gamepad.x gpio() axis", FUNC_SIO), Category::Button);
        assert_eq!(used("Output #7", FUNC_PWM), Category::Output);
        assert_eq!(used("Output #2 (Knocker)", FUNC_SIO), Category::Output);
        assert_eq!(used("TLC5940[0] (SCLK)", 6), Category::Output);
        assert_eq!(used("74HC595[1] (LATCH)", FUNC_SIO), Category::Output);
        assert_eq!(used("TLC59116 (RESET)", FUNC_SIO), Category::Output);
        assert_eq!(used("I2C0 SDA", FUNC_I2C), Category::I2c);
        assert_eq!(used("I2C1 SCL", FUNC_I2C), Category::I2c);
        assert_eq!(used("UART0 TX", FUNC_UART), Category::Uart);
        assert_eq!(used("SPI0 SCK", FUNC_SPI), Category::Spi);
        assert_eq!(used("IR RX", FUNC_SIO), Category::Ir);
        assert_eq!(used("IR TX", FUNC_PWM), Category::Ir);
        assert_eq!(used("MMA8451Q (Interrupt)", FUNC_SIO), Category::Accel);
        assert_eq!(used("LIS3DSH (Interrupt)", FUNC_SIO), Category::Accel);
        assert_eq!(used("MXC6655XA (Interrupt)", FUNC_SIO), Category::Accel);
        assert_eq!(used("MPU6050 (Interrupt)", FUNC_SIO), Category::Accel);
        assert_eq!(used("TCD1103 (OS)", FUNC_NULL), Category::Plunger);
        assert_eq!(used("TSL1410R (SI)", FUNC_SIO), Category::Plunger);
        assert_eq!(used("AEDR8300 (ChA)", FUNC_SIO), Category::Plunger);
        assert_eq!(used("VL6180X (Chip Enable)", FUNC_SIO), Category::Plunger);
        assert_eq!(used("ADS1115 (ALERT/RDY)", FUNC_SIO), Category::Plunger);
        assert_eq!(classify(&port(26, FUNC_NULL, false, true, Some("Pico ADC"))), Category::Plunger);
        assert_eq!(used("Pico LED", FUNC_SIO), Category::System);
        assert_eq!(used("Status LED (G)", FUNC_PWM), Category::System);
        assert_eq!(used("Peripheral Power Enable", FUNC_SIO), Category::System);
        // unknown labels fall back on the hardware function
        assert_eq!(used("Mystery", FUNC_I2C), Category::I2c);
        assert_eq!(used("Mystery", FUNC_PWM), Category::Output);
        assert_eq!(used("Mystery", FUNC_SIO), Category::Other);
        assert_eq!(classify(&port(27, FUNC_NULL, false, true, Some("Mystery"))), Category::Plunger);
    }

    #[test]
    fn classify_unlabelled() {
        assert_eq!(classify(&port(3, FUNC_NULL, false, false, None)), Category::Free);
        assert_eq!(classify(&port(3, FUNC_SIO, true, false, None)), Category::Other);
        assert_eq!(classify(&port(3, FUNC_UART, false, false, None)), Category::Uart);
        assert_eq!(classify(&port(26, FUNC_NULL, false, true, None)), Category::Plunger);
    }

    #[test]
    fn func_details() {
        assert_eq!(func_detail(&port(0, FUNC_SIO, true, false, None)), "GPIO output");
        assert_eq!(func_detail(&port(0, FUNC_SIO, false, false, None)), "GPIO input");
        assert_eq!(func_detail(&port(26, FUNC_NULL, false, true, None)), "ADC input");
        assert_eq!(func_detail(&port(0, 6, false, false, None)), "PIO0");
    }

    #[test]
    fn build_standard_pico() {
        let mut ports = all_free();
        ports[4] = port(4, FUNC_SIO, false, false, Some("Button #0"));
        ports[25] = port(25, FUNC_SIO, true, false, Some("Pico LED"));
        ports[24] = port(24, FUNC_SIO, false, false, Some("Button #9"));
        ports[29] = port(29, FUNC_NULL, false, false, None);
        let p = Pinout::build(&ids("pico", 2040), &ports);
        assert_eq!(p.board, Some("Raspberry Pi Pico"));
        assert_eq!(p.chip, "RP2040");
        assert_eq!(p.header.len(), 40);
        assert_eq!(p.fallback_reason, None);
        assert_eq!(p.pins.len(), 30);

        let gp4 = &p.pins[4];
        assert_eq!(gp4.category, Category::Button);
        assert_eq!(gp4.header_pin, Some(6));
        assert_eq!(gp4.summary, "GP4 (pin 6): Button #0 · GPIO input");
        assert_eq!(gp4.warning, None);

        // the LED is expected on GP25; GP29 unlabelled is board-internal, not free
        assert_eq!(p.pins[25].category, Category::System);
        assert_eq!(p.pins[25].warning, None);
        assert_eq!(p.pins[25].header_pin, None);
        assert_eq!(p.pins[29].category, Category::System);
        assert_eq!(p.pins[29].board_use, Some("VSYS/3 voltage measurement (ADC3)"));
        assert!(p.pins[29].summary.contains("board internal"));

        // a button on VBUS sense is flagged
        let w = p.pins[24].warning.as_deref().unwrap();
        assert!(w.contains("VBUS sense"), "{w}");
        assert_eq!(p.pins[24].category, Category::Button);

        // free list: header GPIOs only, without GP4
        assert!(!p.free.contains(&4));
        assert!(!p.free.contains(&23) && !p.free.contains(&29));
        assert_eq!(p.free.len(), 26 - 1);
        assert_eq!(p.pins[26].adc_channel, Some(0));
        assert!(p.pins[26].summary.ends_with("free · ADC0-capable"), "{}", p.pins[26].summary);
    }

    #[test]
    fn build_pico2_w_internal_pins() {
        let p = Pinout::build(&ids("pico2_w", 2350), &all_free());
        assert_eq!(p.board, Some("Raspberry Pi Pico 2 W"));
        assert_eq!(p.chip, "RP2350");
        assert_eq!(p.pins[25].board_use, Some("wireless chip SPI chip select (WL_CS)"));
        assert_eq!(p.pins[25].category, Category::System);
    }

    #[test]
    fn build_unknown_board_falls_back() {
        let p = Pinout::build(&ids("pimoroni_pico_plus2_rp2350", 2350), &all_free());
        assert!(p.header.is_empty());
        assert_eq!(p.board, None);
        assert!(p.fallback_reason.as_deref().unwrap().contains("pimoroni"));
        assert!(p.pins.iter().all(|x| x.header_pin.is_none() && x.board_use.is_none()));
        // could be an RP2350B: don't claim GP26-29 are ADC pins
        assert_eq!(p.pins[26].adc_channel, None);
        assert_eq!(p.free.len(), 30);

        // an unknown RP2040 board still has ADC on GP26-29
        let p = Pinout::build(&ids("adafruit_kb2040", 2040), &all_free());
        assert!(p.header.is_empty());
        assert_eq!(p.pins[28].adc_channel, Some(2));
    }

    #[test]
    fn build_wrong_gpio_count_falls_back() {
        let ports: Vec<GpioPort> = (0..48).map(|g| port(g, FUNC_NULL, false, false, None)).collect();
        let p = Pinout::build(&ids("pico2", 2350), &ports);
        assert!(p.header.is_empty());
        assert!(p.fallback_reason.as_deref().unwrap().contains("48"));
        assert_eq!(p.pins.len(), 48);
    }
}
