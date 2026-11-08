# Pinscape-Cross

A cross-platform (Windows · Linux · macOS) configuration tool for
[Pinscape Pico](https://github.com/mjrgh/PinscapePico) virtual-pinball I/O
controllers, built with [Tauri 2](https://tauri.app) (Rust backend, React UI).

Goal: feature parity with the Windows-only Pinscape Pico GUI Config Tool.
See **[docs/PLAN.md](docs/PLAN.md)** for the full plan, architecture and phases.

> Unofficial project; not affiliated with the upstream Pinscape Pico project.

<img width="1553" height="831" alt="image" src="https://github.com/user-attachments/assets/6c248144-53a6-4120-a77e-aadf1c62a49b" />

## Installing

Download the installer for your system from the
**[Releases page](https://github.com/superhac/pinscape-cross/releases/latest)**:
`.msi` or `-setup.exe` (Windows), `.dmg` (macOS, Apple silicon or Intel), `.deb`, `.rpm` or
`.AppImage` (Linux). The `pinscape-cli-*` archives contain the command-line tool.
On Linux the `.deb`/`.rpm` packages set up USB permissions; with the AppImage, install the
udev rule by hand (see [Linux USB permissions](#linux-usb-permissions)).
The app shows a notice when a new version is available.

See the **[user guide](docs/USER_GUIDE.md)** for installation details per OS, first steps,
config editing, firmware updates and troubleshooting. Maintainers: see
[docs/RELEASING.md](docs/RELEASING.md) for making a release, and [CHANGELOG.md](CHANGELOG.md).

## Status: Phases 1–6 built (firmware update verified on an RP2040); Linux tested, Windows/macOS not yet

| Piece | State |
|---|---|
| `pinscape-protocol` (wire format) | Request/response, version, IDs, stats, USB interfaces, GPIO, clock, log, config paging, CRC-32; unit-tested |
| `pinscape-usb` (nusb transport) | Discovery by interface string, stream-based request/response, config get/put/exists/erase, stats, GPIO, log, clock, reset. **Tested on hardware.** Includes a simulated device (`mock.rs`) used by the tests |
| `pinscape-config` | Port of the firmware's JSON parser (same syntax, error messages and recovery); schema checker matching the Windows tool's rules check; cursor context for completion, hover and breadcrumbs. All five upstream templates validate with zero findings; differential-tested against the firmware's own parser (`tools/json-diff`) |
| `pinscape` CLI | `list`, `info`, `stats`, `usb`, `gpio`, `get-config`, `put-config`, `erase-config`, `log [-f]`, `set-clock`, `reset`, `validate <file>` (offline config check), `update <file.uf2> [--dry-run]` (firmware install over USB) |
| Tauri app | Hot-plug device manager; Overview; **Config and Safe-Mode Config in a Monaco editor**: Pinscape JSON highlighting, live syntax errors and rules warnings, schema-driven autocomplete (including subclass properties such as `source: { type: "gpio", ... }`), hover docs, breadcrumb bar, F1 opens the upstream reference beside the editor, Problems list, save blocked on syntax errors and confirmed on warnings, Ctrl+S; import/export and templates; edits survive tab switches; prompt before closing with unsaved edits; automatic backup of every save with restore. Help tab with the upstream help pages. Log viewer; help for permission/"in use" errors |
| Live testers | **Buttons** (logical buttons with live state, shift state, GPIO / PCA9555 / 74HC165 input grids), **Outputs** (logical ports with DOF/computed/output levels and sliders, physical device ports, night mode, test mode with device-side timeout and keepalive, all off), **IR & TV-ON** (received codes in the config's text format, raw pulse timeline, send, TV-ON state and relay control) |
| Nudge & plunger | **Nudge** (live accelerometer views and charts, centering, calibration, all parameters with apply/save/revert, ball physics tester), **Plunger** (live gauge, firing state, chart, image-sensor pixels, calibration with undo, settings). Simulator-tested |
| Firmware | UF2 inspection, install with progress over PICOBOOT (falls back to copying onto the boot drive), boot-loader Picos listed in the sidebar for new-device setup; only the sectors the image covers are erased, so the device config is kept |
| CI | GitHub Actions: build, test and clippy on Ubuntu, Windows and macOS |

The vendor interface is **exclusive**: close the app before using the CLI
(or the upstream config tools), and vice versa.

## Testing checklist

What has been checked on real hardware so far, and what still needs testing. Tick items
off (`- [x]`) as they're verified. Everything listed is covered by automated tests against
the simulated device (`cargo test`), unless marked otherwise.

### Verified on hardware (Linux, Debian 13, RP2040 Pico, firmware v1.0.2)

- [x] Device discovery, hot-plug, reconnect after reboot
- [x] Overview tab (device info, live status, USB interfaces, GPIO), Reboot button
- [x] Config download/upload (CLI and app load), byte-identical round trip of a 65 KB config
- [x] Config editor: highlighting, live errors/warnings, autocomplete, hover, F1 reference,
      unsaved-changes prompt
- [x] Log tab (ANSI colors), device clock set on connect
- [x] "In use by another program" detection
- [x] Buttons tab: BOOTSEL button live state, GPIO input with pull-up
- [x] Outputs tab: logical port and GPIO device listing with live levels
- [x] IR tab: correct "not configured" state, TV-ON state display
- [x] Firmware update over USB (PICOBOOT) with `pinscape update`: erase, write, verify,
      reboot; config preserved byte-identical
- [x] `.deb` / `.rpm` / AppImage build locally; `.deb` contents (udev rule, post-install script)
- [x] Preferences dialog opens with Ctrl+, and closes with Esc; Ctrl+2 switches tabs (simulator)

### Still to test

**Config & device management**
- [ ] Save a config to the device from the app's Config tab (incl. "reboot after saving")
- [ ] Safe-Mode Config tab: save, and booting into Safe Mode
- [ ] Automatic backup made on save; restore one from **Backups…**
- [ ] Import / Export file dialogs; **New from template**
- [ ] Overview buttons: Safe Mode, Factory Mode, Boot Loader, Erase Config
- [ ] Two or more Picos connected at once (selection, `--id` in the CLI)
- [ ] App's permission-help panel (unplug, remove the udev rule, replug)
- [ ] **Print…** on the Config tab: line-numbered listing with header on Linux, Windows and macOS; app looks normal afterwards

**Outputs** (needs an output wired, e.g. an LED + resistor on GP3)
- [ ] Set a logical port level from the slider and 0/255 buttons
- [ ] Night Mode toggle (noisy ports silenced)
- [ ] Test mode: enter (confirmation), physical port sliders, leaving the tab exits test mode,
      device returns to normal within 3 s if the app is closed
- [ ] **All off**
- [ ] Other output chips: TLC5940, TLC59116, TLC5947, PCA9685, PCA9555, 74HC595, PWMWorker
- [ ] PWMWorker **Reset** / **Boot loader** buttons

**Buttons** (needs wiring)
- [ ] Real key / gamepad / XInput actions on the PC
- [ ] Shift buttons and the shift-state display
- [ ] PCA9555 and 74HC165 input grids
- [ ] Button event log with `enableLogging: true` on a GPIO button: timestamps, hold times, Clear, CSV export

**IR & TV-ON** (needs an IR receiver on `irRx` and an IR LED on `irTx`)
- [ ] Receive codes from a remote; raw pulse timeline
- [ ] Send a code, and re-send a received one
- [ ] TV-ON power-sense cycle and relay On / Off / Pulse
- [ ] IR raw pulse **Export CSV…**

**Nudge** (needs an accelerometer: LIS3DH, MMA8451Q, MXC6655XA, MC3416 or LIS3DSH)
- [ ] Live views and charts; "no accelerometer" detection
- [ ] Center now; 15 s calibration (with and without "save when done")
- [ ] Parameters: Apply, Save to device, Revert
- [ ] Check the suspected firmware issue: velocity decay seems inverted vs. its documentation
- [ ] **Record CSV** file opens in a spreadsheet

**Plunger** (needs a sensor; a potentiometer on a Pico ADC pin is the simplest)
- [ ] Live gauge, Z axis, firing state, chart
- [ ] Calibration, Undo calibration, Save / Revert / Restore defaults
- [ ] Settings: jitter filter, firing time limit, orientation, scaling factor
- [ ] Image sensors (TCD1103 / TSL1410R / TSL1412S) pixel view and integration time
- [ ] Quadrature (AEDR8300) and proximity (VCNL4010 / VL6180X) sensors
- [ ] Confirm calibration values are native sensor units (the header says 0–65535)
- [ ] **Record CSV**; after a reset that loses settings, **Restore saved calibration** puts the calibration back

**Firmware**
- [ ] Install from the app's Firmware tab (file dialog and drag-and-drop, progress window,
      reconnect, new version shown)
- [ ] New-device setup: a Pico in BOOTSEL mode without Pinscape, from the sidebar
- [ ] Upgrade / downgrade to a different firmware version
- [ ] Pico 2 (RP2350): PICOBOOT reboot, family check
- [ ] Mass-storage fallback (copy to the RPI-RP2 drive)
- [ ] Wrong-chip image is refused (e.g. a `-pico2.uf2` on an RP2040)
- [ ] Dropping a `.uf2` on any tab (not just Firmware) opens the install dialog; with two Picos the target picker works
- [ ] **Erase Pinscape config** (boot loader): Pico restarts with factory defaults, firmware kept (RP2040 and RP2350)
- [ ] **Erase entire flash**, from a running device and from a boot-loader Pico: Pico comes back in boot loader mode, detected flash size is right; reinstall firmware afterwards
- [ ] `pinscape erase-flash --config --dry-run` with a Pico in boot loader mode lists the directory and files
- [ ] Erase on a clone with 4 / 8 / 16 MB flash

**Preferences, shortcuts & shell**
- [ ] Preferences persist across restarts (`preferences.json` in the app config folder)
- [ ] Theme Light / Dark / System: app, editor, log colors and window title bar follow; switching the OS theme while on System
- [ ] Text size Small / Large; editor font size, tab size, insert spaces, minimap, word wrap apply live
- [ ] Emacs keys (Linux/Windows, and real Control on macOS), incl. repeated Ctrl+K then Ctrl+Y; normal Ctrl+A / Ctrl+F when off
- [ ] "Confirm before saving" off: a clean save goes straight through; a save with warnings still asks
- [ ] Backups off: no backup written and the message says so; retention removes old backups on the next save
- [ ] Last tab and device restored at startup
- [ ] About: version, upstream revision, links open in the browser; **Check for updates** message
- [ ] Help tab / reference pane: https and mjrnet.org links open in the browser; links between help pages stay in the frame
- [ ] All shortcuts (list in Help › Keyboard shortcuts), incl. F1 with and without editor focus, Ctrl+Shift+R confirmation, Cmd on macOS
- [ ] Tab bar arrow / Home / End keys; keyboard focus outlines; Esc closes the topmost dialog first
- [ ] Boot-loader Pico with no Pinscape device: the sidebar doesn't say "No devices"

**Releases & auto-update** (need a tagged release; see [docs/RELEASING.md](docs/RELEASING.md))
- [ ] Tag push creates one draft release; all four build jobs (Linux, Windows, macOS arm64 and x64) upload into it
- [ ] Draft contains the `.deb`, `.rpm`, AppImage, `.msi`, NSIS `.exe`, both `.dmg`s and the CLI archives
- [ ] macOS ad-hoc signing works for both architectures
- [ ] With the updater key and pubkey set: `.sig` files and `latest.json` (all platforms, `/download/vX.Y.Z/` URLs); with only one of them set the build fails
- [ ] End to end: install 0.x.0, publish 0.x.1, the notice appears, Install and restart works (AppImage, NSIS, MSI, macOS); `.deb`/`.rpm` show **Download…**
- [ ] A manual workflow run without a tag only uploads workflow artifacts
- [ ] Optional code signing with real certificates (Apple Developer ID + notarization, Windows `.pfx`)
- [ ] Linux CI and release builds work with the trimmed build packages (no `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`)

**Platforms & packaging**
- [ ] First green CI run on Ubuntu, Windows and macOS
- [ ] Windows: device access via WinUSB, NSIS `.exe` and `.msi` installers, RP2040 firmware
      update via the mass-storage fallback
- [ ] macOS: device access, `.dmg`, Gatekeeper warning for the unsigned app
- [ ] Installing the `.deb` (udev rule active right after install), `.rpm` on Fedora, AppImage
- [ ] Other distros: Ubuntu 22.04 / 24.04, Fedora; X11 as well as Wayland (file drag-and-drop)

## Layout

```
crates/pinscape-protocol   wire structs & constants (no I/O)
crates/pinscape-usb        cross-platform USB access (nusb)
crates/pinscape-config     firmware-compatible parser, schema check, editor services
crates/pinscape-cli        `pinscape` command-line tool
src-tauri/                 Tauri app (Rust)
ui/                        React + TypeScript frontend (Vite)
assets/                    synced from upstream PinscapePico (see below)
tools/schema-gen/          JSONConfigRef.txt -> schema.json + HTML reference
tools/json-diff/           differential test: our parser vs the firmware's JSON.cpp
packaging/udev/            Linux udev rule
docs/PLAN.md               project plan
```

## Prerequisites

### Debian / Ubuntu

Tested on Debian 13 (trixie). Run these from the repository root.

**1. Tauri build dependencies** (WebKitGTK, compilers, and so on):

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  librsvg2-dev pkg-config
```

`libwebkit2gtk-4.1-dev` (WebKitGTK, which also pulls in GTK) is the only library the app
links against; `curl`, `wget`, `file` and `librsvg2-dev` are used when packaging (the AppImage
step). Tauri's generic Linux list also names `libxdo-dev`, `libssl-dev` and
`libayatana-appindicator3-dev`: this app needs none of them (no native menus, no OpenSSL, no
tray icon). Installed packages only need the runtime libraries `libwebkit2gtk-4.1-0` and
`libgtk-3-0`, which the `.deb` declares as dependencies.

**2. Node.js and npm** (for the UI). Debian packages `npm` separately from `nodejs`:

```bash
sudo apt install nodejs npm
```

Debian 13 ships Node 20 and Ubuntu 24.04 ships Node 18; both work.
Ubuntu 22.04's Node 12 is too old. On 22.04, install Node 20 from
[NodeSource](https://github.com/nodesource/distributions) or with
[nvm](https://github.com/nvm-sh/nvm) instead.

**3. Rust** via [rustup](https://rustup.rs). The distro `rustc` package may be
too old for current dependencies:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. ~/.cargo/env
```

**4. USB access for your user.** Install the udev rule, then unplug and replug the Pico.
See [Linux USB permissions](#linux-usb-permissions) below for what it covers and how to
check it:

```bash
sudo cp packaging/udev/99-pinscape.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

### Windows

- Rust via [rustup](https://rustup.rs), with the MSVC Build Tools (rustup offers to install them)
- [Node.js 20+](https://nodejs.org)
- WebView2 is preinstalled on Windows 10/11. No USB driver is needed: the
  device's WinUSB interface installs itself.

### macOS

- Xcode command-line tools: `xcode-select --install`
- Rust via [rustup](https://rustup.rs)
- Node.js 20+ (`brew install node`, or from nodejs.org)

## Linux USB permissions

On Linux, USB devices are owned by root until a **udev rule** grants access. Pinscape-Cross
and the `pinscape` CLI never need to run as root: install the rule once and everything works
for normal users. Windows needs no setup (WinUSB binds automatically), and neither does macOS.

### What needs access

| USB ID | Device | Used for | Without the rule |
|---|---|---|---|
| `1209:eaea`, `1209:eaeb` | Pinscape Pico running its firmware (vendor interface `PinscapePicoControl`) | Everything: overview, config, log, testers, nudge, plunger, firmware update start | The sidebar shows "No permission to open" |
| `2e8a:0003` | RP2040 boot ROM (Pico in boot-loader mode) | Firmware updates over USB (PICOBOOT), setting up a new Pico | `pinscape update` / the Firmware tab stop after the reboot with "permission denied"; the Pico waits in boot-loader mode, with nothing erased |
| `2e8a:000f` | RP2350 boot ROM (Pico 2 in boot-loader mode) | Same, for Pico 2 | Same |

The rule must cover **both** groups. A rule with only the `1209` lines lets the app talk to
Pinscape but makes every firmware update fail as soon as the Pico reboots into its boot loader.

### The rule

[`packaging/udev/99-pinscape.rules`](packaging/udev/99-pinscape.rules):

```
SUBSYSTEM=="usb", ATTR{idVendor}=="1209", ATTR{idProduct}=="eaea", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="1209", ATTR{idProduct}=="eaeb", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="2e8a", ATTR{idProduct}=="0003", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ATTR{idVendor}=="2e8a", ATTR{idProduct}=="000f", MODE="0666", TAG+="uaccess"
```

`MODE="0666"` lets any local user open the device. `TAG+="uaccess"` additionally grants the
logged-in desktop user access through systemd-logind. If you prefer, remove `MODE="0666"` to
rely on `uaccess` alone (works for desktop sessions, not for SSH-only logins).

### Installing

- **`.deb` / `.rpm` packages** install the rule to `/usr/lib/udev/rules.d/99-pinscape.rules`
  and reload udev automatically. Nothing else to do.
- **AppImage, building from source, or the CLI alone:**

  ```bash
  sudo cp packaging/udev/99-pinscape.rules /etc/udev/rules.d/
  sudo udevadm control --reload && sudo udevadm trigger
  ```

  `udevadm trigger` applies the rule to devices that are already plugged in (including a Pico
  waiting in boot-loader mode), so a replug isn't strictly needed.

The app's sidebar also detects a permission problem and shows a **Copy command** button with
this fix, and `pinscape update` prints it when it can't open the boot loader.

### Updating an older rule

A file in `/etc/udev/rules.d/` **overrides** a file with the same name in
`/usr/lib/udev/rules.d/`. If you created `/etc/udev/rules.d/99-pinscape.rules` by hand earlier
(for example a one-line rule covering only `1209:eaea|eaeb`), it hides the packaged rule. Check:

```bash
cat /etc/udev/rules.d/99-pinscape.rules
```

If it lacks the `2e8a` lines, replace it with the current file (the copy command above), or
delete it if the package's copy in `/usr/lib/udev/rules.d/` is present:

```bash
sudo rm /etc/udev/rules.d/99-pinscape.rules
sudo udevadm control --reload && sudo udevadm trigger
```

### Checking

```bash
lsusb | grep -iE '1209|2e8a'          # find the bus/device numbers
ls -l /dev/bus/usb/001/017             # use the numbers from lsusb
```

Good: `crw-rw-rw-` (or a `+` at the end, meaning an ACL from `uaccess`). Bad:
`crw-rw-r-- root root` (only root can open it). `pinscape list` should list the device without
an error.

### Custom USB IDs

If your config changes the device's USB vendor/product ID (`usb: { vid: ..., pid: ... }`), add
a line for it, e.g. for `1234:5678`:

```
SUBSYSTEM=="usb", ATTR{idVendor}=="1234", ATTR{idProduct}=="5678", MODE="0666", TAG+="uaccess"
```

### Not needed

- **Root / sudo** for running the app or CLI.
- **`input` / `plugdev` groups or HID permissions**: the app doesn't use the keyboard, gamepad
  or feedback-controller HID interfaces. Games and DOF use those and aren't affected.
- **Serial console** (`/dev/ttyACM*`): the app reads the log over USB, not the serial port.
  Only if you want a terminal on the console (`picocom /dev/ttyACM0`) do you need the
  `dialout` group: `sudo usermod -aG dialout $USER`, then log out and back in.
- **Mounting the RPI-RP2 drive**: firmware updates use PICOBOOT directly. Copying to the
  boot drive is only a fallback when PICOBOOT can't be opened. In that case the drive must be
  mounted (your desktop usually does this; otherwise `udisksctl mount -b /dev/sdX1`).

## Build & run

CLI only (no GUI dependencies needed):

```bash
cargo build -p pinscape-cli
./target/debug/pinscape list
./target/debug/pinscape info
./target/debug/pinscape get-config my-config.json
```

Desktop app:

```bash
npm install
npm run tauri dev        # development, hot reload
npm run tauri build      # installers in target/release/bundle/
```

Tests:

```bash
cargo test
```

Environment variables:

- `PINSCAPE_TRACE=1` logs USB reads.
- `PINSCAPE_MOCK=1` adds a simulated device to the app, for UI work without a Pico:
  `PINSCAPE_MOCK=1 npm run tauri dev`. The simulator has buttons, outputs and IR activity.
- `PINSCAPE_MOCK_BOOTLOADER=1` adds a simulated Pico in boot loader mode.
- `PINSCAPE_START_TAB=<tab>` and `PINSCAPE_START_DEVICE=<hardware ID>` choose the
  tab and device shown at startup (e.g. `PINSCAPE_START_DEVICE=DEADBEEF00000001` for the simulator).

### Parser differential test

`crates/pinscape-config` re-implements the firmware's JSON parser. To check it
against the real thing (needs an upstream checkout and a C++ compiler):

```bash
tools/json-diff/run.py --upstream ../PinscapePico -n 1000 --seed 1
```

It parses the upstream templates plus randomly damaged copies with both
parsers and compares the values and error lists.

## Upstream assets

`assets/` holds files taken or generated from an upstream
[PinscapePico](https://github.com/mjrgh/PinscapePico) checkout: config templates,
the help pages, and `schema/schema.json`. The schema and `JSONConfigRef.htm` are
generated from `Firmware/JSONConfigRef.txt` by `tools/schema-gen`, which links
upstream's `ConfigDocParser.cpp` unmodified, so the parsing rules match upstream.
To refresh after upstream changes (needs a C++20 compiler):

```bash
scripts/sync-upstream-assets.sh ../PinscapePico
```

## License

BSD-3-Clause, matching upstream Pinscape Pico.
