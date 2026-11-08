//! `pinscape` — cross-platform command-line tool for Pinscape Pico.
//!
//! Covers the upstream CmdLineConfigTool's config/reset/stats functions,
//! plus log, GPIO and USB-interface queries.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{Datelike, Timelike};
use clap::{Parser, Subcommand};
use pinscape_protocol::{config, reset, ClockArgs};
use pinscape_usb as usb;

#[derive(Parser)]
#[command(name = "pinscape", version, about = "Pinscape Pico command-line tool")]
struct Cli {
    /// Select a device by hardware ID (or ≥4-char fragment), unit number, or unit name
    #[arg(long, global = true)]
    id: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List attached Pinscape Pico devices
    List,
    /// Show firmware and hardware information
    Info,
    /// Show run-time statistics (uptime, loop timing, memory)
    Stats {
        /// Reset the rolling-average counters after reading
        #[arg(long)]
        reset_counters: bool,
    },
    /// Show the USB interfaces the device exposes
    Usb,
    /// Show GPIO pin assignments
    Gpio {
        /// Include unassigned pins
        #[arg(long)]
        all: bool,
    },
    /// Download a config file (stdout if no file given)
    GetConfig {
        file: Option<PathBuf>,
        /// Use the Safe Mode config instead of the main config
        #[arg(long)]
        safe_mode: bool,
    },
    /// Upload a config file (takes effect after a reset)
    PutConfig {
        file: PathBuf,
        /// Use the Safe Mode config instead of the main config
        #[arg(long)]
        safe_mode: bool,
        /// Reset the device afterwards so the new config takes effect
        #[arg(long)]
        reset: bool,
    },
    /// Erase a config file (restores factory defaults on next reset)
    EraseConfig {
        /// main | safe-mode | all
        #[arg(default_value = "main")]
        which: String,
    },
    /// Read the device's in-memory log
    Log {
        /// Keep polling for new messages until Ctrl+C
        #[arg(short, long)]
        follow: bool,
    },
    /// Set the device's wall clock to this computer's local time
    SetClock,
    /// Install firmware from a .uf2 file (reboots into the boot loader, flashes
    /// over USB with verification, and waits for Pinscape to come back).
    /// Only the flash sectors the image covers are erased; the config is kept.
    Update {
        file: PathBuf,
        /// Show the plan without touching the device
        #[arg(long)]
        dry_run: bool,
        /// Skip read-back verification
        #[arg(long)]
        no_verify: bool,
    },
    /// Check a config file offline (same parser and rules check as the editor)
    Validate { file: PathBuf },
    /// Reboot the device
    Reset {
        /// normal | safe-mode | bootloader | factory
        #[arg(default_value = "normal")]
        mode: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type Res = Result<(), Box<dyn std::error::Error>>;

fn file_id(safe_mode: bool) -> u8 {
    if safe_mode {
        config::FILE_SAFE_MODE
    } else {
        config::FILE_MAIN
    }
}

fn local_clock() -> ClockArgs {
    let t = chrono::Local::now();
    ClockArgs {
        year: t.year() as i16,
        month: t.month() as u8,
        day: t.day() as u8,
        hour: t.hour() as u8,
        minute: t.minute() as u8,
        second: t.second() as u8,
    }
}

fn fmt_uptime(us: u64) -> String {
    let s = us / 1_000_000;
    let (d, h, m, s) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d}d {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

fn run(cli: Cli) -> Res {
    let open = || usb::open_device(cli.id.as_deref());
    match cli.command {
        Command::List => {
            let devices = usb::find_devices()?;
            if devices.is_empty() {
                println!("No Pinscape Pico devices found.");
                return Ok(());
            }
            println!("{:>4}  {:<16}  {:<20}  {:<9}  Firmware", "Unit", "Hardware ID", "Name", "USB");
            for c in devices {
                let s = c.summary();
                let usb_id = format!("{:04x}:{:04x}", s.vid, s.pid);
                match c.open().and_then(|mut cl| Ok((cl.query_ids()?, cl.query_version()?))) {
                    Ok((ids, v)) => println!(
                        "{:>4}  {:<16}  {:<20}  {:<9}  {}",
                        ids.unit_num, ids.hwid, ids.unit_name, usb_id, v
                    ),
                    Err(e) => println!("{:>4}  {:<16}  {:<20}  {:<9}  ({e})", "?", "?", "?", usb_id),
                }
            }
        }
        Command::Info => {
            let (mut cl, ids) = open()?;
            let v = cl.query_version()?;
            let has_main = cl.config_exists(config::FILE_MAIN)?;
            let has_safe = cl.config_exists(config::FILE_SAFE_MODE)?;
            println!("Unit:            #{} \"{}\"", ids.unit_num, ids.unit_name);
            println!("Firmware:        v{v}");
            println!("Hardware ID:     {}", ids.hwid);
            println!("CPU:             {} (version {}, ROM {})", ids.cpu_name(), ids.cpu_version, ids.rom_version);
            println!("Target board:    {}", ids.target_board);
            println!("Pico SDK:        {}", ids.pico_sdk_version);
            println!("TinyUSB:         {}", ids.tinyusb_version);
            println!("Compiler:        {}", ids.compiler_version);
            println!("Main config:     {}", if has_main { "stored" } else { "none (factory defaults)" });
            println!("Safe-mode config:{}", if has_safe { " stored" } else { " none" });
        }
        Command::Stats { reset_counters } => {
            let (mut cl, _) = open()?;
            let s = cl.query_stats(reset_counters)?;
            println!("Uptime:          {}", fmt_uptime(s.up_time_us));
            println!("Main loop:       avg {} µs, max {} µs ({} iterations)", s.avg_loop_time_us, s.max_loop_time_us, s.n_loops_ever);
            if s.n_loops_ever2 > 0 {
                println!("Core 1 loop:     avg {} µs, max {} µs ({} iterations)", s.avg_loop_time2_us, s.max_loop_time2_us, s.n_loops_ever2);
            }
            println!("Heap:            {} of {} bytes free", s.heap_unused, s.heap_size);
            println!("Arena:           {} allocated, {} free, {} total", s.arena_alloc, s.arena_free, s.arena_size);
        }
        Command::Usb => {
            let (mut cl, _) = open()?;
            let u = cl.query_usb_interfaces()?;
            println!("USB ID: {:04x}:{:04x}", u.vid, u.pid);
            let row = |name: &str, conf: bool, ena: Option<bool>| {
                let state = match (conf, ena) {
                    (false, _) => "not configured",
                    (true, Some(false)) => "configured, reporting off",
                    (true, _) => "configured",
                };
                println!("  {name:<22} {state}");
            };
            row("Keyboard", u.keyboard_configured, Some(u.keyboard_enabled));
            row("Gamepad", u.gamepad_configured, Some(u.gamepad_enabled));
            row("XInput", u.xinput_configured, Some(u.xinput_enabled));
            row("Open Pinball Device", u.pindev_configured, Some(u.pindev_enabled));
            row("Feedback Controller", u.feedback_configured, None);
            row("LedWiz emulation", u.ledwiz_configured, None);
            row("Serial (CDC) port", u.cdc_configured, None);
        }
        Command::Gpio { all } => {
            let (mut cl, _) = open()?;
            println!("{:<5} {:<5} {:<4} Usage", "GPIO", "Func", "Dir");
            for p in cl.query_gpio_config()? {
                if !all && p.usage.is_none() && p.func == 0x1F {
                    continue;
                }
                let dir = if p.adc { "ADC" } else if p.func == 5 { if p.output { "out" } else { "in" } } else { "" };
                println!("GP{:<3} {:<5} {:<4} {}", p.gpio, p.func_name, dir, p.usage.as_deref().unwrap_or(""));
            }
        }
        Command::GetConfig { file, safe_mode } => {
            let (mut cl, _) = open()?;
            match cl.get_config(file_id(safe_mode))? {
                None => return Err("no config file is stored on the device".into()),
                Some(data) => match file {
                    Some(p) => {
                        std::fs::write(&p, &data)?;
                        eprintln!("Saved {} bytes to {}", data.len(), p.display());
                    }
                    None => print!("{}", String::from_utf8_lossy(&data)),
                },
            }
        }
        Command::PutConfig { file, safe_mode, reset: do_reset } => {
            let data = std::fs::read(&file)?;
            let (mut cl, _) = open()?;
            cl.put_config(file_id(safe_mode), &data)?;
            println!("Uploaded {} bytes.", data.len());
            if do_reset {
                cl.reset(reset::NORMAL)?;
                println!("Device reset; the new config is now active.");
            } else {
                println!("The new config takes effect after a reset (pinscape reset).");
            }
        }
        Command::EraseConfig { which } => {
            let fid = match which.as_str() {
                "main" => config::FILE_MAIN,
                "safe-mode" => config::FILE_SAFE_MODE,
                "all" => config::FILE_ALL,
                other => return Err(format!("unknown config '{other}' (main | safe-mode | all)").into()),
            };
            let (mut cl, _) = open()?;
            cl.erase_config(fid)?;
            println!("Erased {which} config. Factory defaults apply after the next reset.");
        }
        Command::Log { follow } => {
            let (mut cl, _) = open()?;
            let mut out = std::io::stdout().lock();
            loop {
                match cl.query_log()? {
                    Some((text, _)) => {
                        out.write_all(&text)?;
                        out.flush()?;
                    }
                    None if follow => std::thread::sleep(Duration::from_millis(250)),
                    None => break,
                }
            }
        }
        Command::Update { file, dry_run, no_verify } => update(cli.id.as_deref(), &file, dry_run, !no_verify)?,
        Command::Validate { file } => {
            let text = std::fs::read_to_string(&file)?;
            let diags = pinscape_config::validate(&text);
            for d in &diags {
                let sev = match d.severity {
                    pinscape_config::Severity::Error => "error",
                    pinscape_config::Severity::Warning => "warning",
                    pinscape_config::Severity::Info => "info",
                };
                println!("{}:{}:{}: {sev}: {}", file.display(), d.range.start_line, d.range.start_col, d.message);
            }
            let errors = diags.iter().filter(|d| d.severity == pinscape_config::Severity::Error).count();
            println!("{} error(s), {} warning(s)", errors, diags.len() - errors);
            if errors > 0 {
                return Err("the device can't use a file with syntax errors".into());
            }
        }
        Command::SetClock => {
            let (mut cl, _) = open()?;
            let t = local_clock();
            cl.set_clock(t)?;
            println!(
                "Device clock set to {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                t.year, t.month, t.day, t.hour, t.minute, t.second
            );
        }
        Command::Reset { mode } => {
            let m = match mode.as_str() {
                "normal" => reset::NORMAL,
                "safe-mode" => reset::SAFE_MODE,
                "bootloader" => reset::BOOTLOADER,
                "factory" => reset::FACTORY,
                other => return Err(format!("unknown reset mode '{other}'").into()),
            };
            let (mut cl, _) = open()?;
            cl.reset(m)?;
            println!("Reset sent ({mode}).");
        }
    }
    Ok(())
}

/// `pinscape update`: flash a UF2 over PICOBOOT.
fn update(id: Option<&str>, file: &std::path::Path, dry_run: bool, verify: bool) -> Res {
    use pinscape_protocol::uf2::{Chip, Uf2Image};
    use pinscape_usb::bootloader::{self, FlashOptions, Phase};
    use std::time::Instant;

    let bytes = std::fs::read(file)?;
    let image = Uf2Image::parse(&bytes)?;

    // Which chip? From the running Pinscape device, else from a Pico already in boot loader mode.
    let running = usb::open_device(id).ok();
    let chip = match &running {
        Some((_, ids)) => Chip::from_cpu_type(ids.cpu_type).ok_or("unknown CPU type")?,
        None => bootloader::find_boot_devices()?
            .first()
            .map(|b| b.chip)
            .ok_or("no Pinscape Pico or boot-loader Pico found")?,
    };
    let plan = image.plan(chip)?;
    let (lo, hi) = plan.range();
    let sectors = plan.erase_ranges(4096).len();
    println!("Image:   {} ({} bytes, {:?})", file.display(), bytes.len(), image.kind());
    println!("Target:  {} — flash {lo:#010x}..{hi:#010x}, {sectors} sector(s) of 4 KB to erase, {} bytes to write",
        chip.name(), plan.total_write_bytes());
    if let Some((_, ids)) = &running {
        println!("Device:  #{} \"{}\" ({})", ids.unit_num, ids.unit_name, ids.hwid);
    }
    if dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }

    let before: Vec<String> = bootloader::find_boot_devices()?.iter().map(|b| b.key()).collect();
    if let Some((mut cl, _)) = running {
        println!("Rebooting into the boot loader...");
        cl.reset(pinscape_protocol::reset::BOOTLOADER)?;
    }
    let t0 = Instant::now();
    let boot = loop {
        let found = bootloader::find_boot_devices()?;
        if let Some(b) = found.iter().find(|b| !before.contains(&b.key())).or(found.first()) {
            break b.clone();
        }
        if t0.elapsed() > Duration::from_secs(20) {
            return Err("the Pico didn't appear in boot loader mode within 20 s".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    println!("Boot loader found ({}).", boot.chip.name());
    // the boot ROM may need a moment before its interface can be claimed
    let mut pb = {
        let t1 = Instant::now();
        loop {
            match boot.open() {
                Ok(pb) => break pb,
                Err(e) if t1.elapsed() < Duration::from_secs(5) && !e.is_permission() => std::thread::sleep(Duration::from_millis(300)),
                Err(e) if e.is_permission() && cfg!(target_os = "linux") => {
                    return Err(format!(
                        "no permission to open the Pico's boot loader (USB 2e8a:{:04x}). The udev rule must cover \
                         the RP2040/RP2350 boot ROM as well as Pinscape itself. Install the project's rule:\n  \
                         sudo cp packaging/udev/99-pinscape.rules /etc/udev/rules.d/\n  \
                         sudo udevadm control --reload && sudo udevadm trigger\n\
                         then run this command again (the Pico is waiting in boot loader mode; nothing was erased).",
                        boot.info.product_id()
                    )
                    .into())
                }
                Err(e) => return Err(format!("can't open the boot loader's PICOBOOT interface: {e}").into()),
            }
        }
    };
    let mut last = (Phase::Done, u64::MAX);
    bootloader::flash(&mut pb, &plan, FlashOptions { verify, reboot: true }, &mut |p| {
        let pct = (p.done * 100).checked_div(p.total).unwrap_or(100);
        if p.phase != last.0 || pct / 10 != last.1 / 10 {
            println!("  {:?}: {pct}%", p.phase);
            last = (p.phase, pct);
        }
    })?;
    drop(pb);
    println!("Flashed; waiting for Pinscape to restart...");
    let t2 = Instant::now();
    loop {
        if let Ok((mut cl, ids)) = usb::open_device(id) {
            println!("Pinscape is back: #{} \"{}\", firmware v{}", ids.unit_num, ids.unit_name, cl.query_version()?);
            return Ok(());
        }
        if t2.elapsed() > Duration::from_secs(30) {
            return Err("flashed, but Pinscape didn't reappear within 30 s".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
