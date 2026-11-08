# Pinscape-Cross — Cross-Platform Config Tool: Project Plan

Status: **Draft v1** (2026-11-08)
Target: feature parity with the Windows *Pinscape Pico GUI Config Tool*, running on
**Windows, Linux and macOS** from one codebase.

---

## 1. Goals and non-goals

### Goals
1. Replace the Win32-only GUI Config Tool (`PinscapePico/GUIConfigTool`, ~24k lines) with a
   cross-platform desktop app built on **Tauri 2** (Rust backend + web UI).
2. Reach **feature parity** with every window of the Windows tool (inventory in §3).
3. Use **one USB code path** for all three OSes (no per-OS protocol code), talking to the
   device's existing USB interfaces. **No firmware changes are required.**
4. Produce a reusable Rust crate for the Pinscape vendor protocol, and a small cross-platform
   CLI on top of it, as a by-product (useful for testing and scripting).
5. Ship installers: `.msi`/`.exe` (Windows), `.deb`/`.rpm`/`.AppImage` (Linux, with udev rule),
   `.dmg` (macOS, signed + notarized when a certificate is available).

### Non-goals (for v1)
- Changing the device firmware or USB protocol.
- Replacing DOF or acting as a feedback controller at game time.
- A graphical (form-based) config editor. Like the original, the config is edited as JSON text,
  with schema-driven help. (A form editor is a possible v2 item, see §11.)
- Mobile or web targets.

---

## 2. Architecture

```
┌──────────────────────────────── Tauri app ────────────────────────────────┐
│  Web UI (React + TypeScript + Vite)                                         │
│   ├─ Monaco editor  (config editor, custom "pinscape-json" language)        │
│   ├─ uPlot          (real-time nudge / plunger / output graphs)             │
│   └─ Help viewer    (existing HTML help, rendered in-app)                   │
│              ▲  invoke() commands          ▲  events (live streams)         │
├──────────────┼────────────────────────────┼───────────────────────────────┤
│  src-tauri  (Rust)                                                          │
│   ├─ commands.rs   thin #[tauri::command] wrappers                         │
│   ├─ session.rs    device manager: hot-plug, one worker thread per device   │
│   └─ streams.rs    polling loops → throttled events (buttons, nudge, ...)   │
├─────────────────────────────────────────────────────────────────────────────┤
│  crates/                                                                    │
│   ├─ pinscape-protocol   pure encode/decode of VendorIfcProtocol.h (no I/O) │
│   ├─ pinscape-usb        nusb transport, discovery, VendorClient, PICOBOOT  │
│   ├─ pinscape-config     firmware-compatible JSON parser + schema checker   │
│   └─ pinscape-cli        cross-platform CLI (replaces CmdLineConfigTool)    │
└─────────────────────────────────────────────────────────────────────────────┘
                  │ USB (bulk endpoints, vendor class 0xFF)
             Pinscape Pico (RP2040 / RP2350)
```

### Key technology choices

| Concern | Choice | Why |
|---|---|---|
| App shell | **Tauri 2** | Small binaries, native webview on each OS, Rust backend, mature bundler |
| USB | **`nusb`** (pure Rust) | One API over usbfs (Linux), WinUSB (Windows), IOKit (macOS); no libusb DLL to ship; hot-plug events |
| HID (later phases) | `hidapi` | Only if needed for the Feedback Controller interface (§3, Plunger) |
| UI framework | **React 18 + TypeScript + Vite** | Largest ecosystem; first-class Monaco bindings |
| Code editor | **Monaco** (`monaco-editor`, bundled locally) | Line numbers, folding, find/replace, autocomplete, hovers, markers, built in |
| Charts | **uPlot** | Very fast canvas time-series; handles 60 fps live data easily |
| State | Zustand | Simple, minimal boilerplate |
| Styling | CSS modules + CSS variables (light/dark) | No heavy UI kit dependency |
| Tests | `cargo test`, Vitest, Playwright (UI smoke) | |

### Why Rust re-implements the protocol (instead of wrapping the C++ API)
The existing APIs are uneven: `WinAPI` (~2.9k lines, full coverage, Win32-only) vs `LinuxAPI`
(~580 lines, ~10% coverage, guesses interface numbers 0/1/2). The wire protocol itself is
small, fully documented in `USBProtocol/VendorIfcProtocol.h`, and fixed-size, so a clean Rust
port is less work and less risk than FFI to C++ on three platforms.

---

## 3. Feature inventory (parity checklist)

Each row is a window/tab in the Windows tool, the vendor-interface functions it uses, and
where it lands in this project.

| # | Windows window | Device functions used | Phase |
|---|---|---|---|
| 1 | **Device list / wrapper** (tabs, menus, status line) | QueryID, QueryStats, ResetPico, EnterSafeMode, EnterFactoryMode, EnterBootLoader, EraseConfig, ClearConfig, PutWallClockTime, QueryFileSysInfo, ReadFlashSector | 1 |
| 2 | **Device Overview** | QueryVersion, QueryID, QueryStats, QueryUSBInterfaceConfig, QueryGPIOConfig, QueryButtonGPIOStates, QueryLogicalOutputPortConfig, QueryOutputPortConfig, QueryConfigFileExists, CDC port lookup | 1 |
| 3 | **Config Editor** | GetConfig, PutConfig, ResetPico, QueryID (+ templates, import/export, schema, help links, auto-backup) | 2 |
| 4 | **Log Viewer** | QueryLog (polled), search, save | 2 |
| 5 | **Button Tester** | QueryButtonConfig, QueryLogicalButtonStates, QueryButtonGPIOStates (shift-reg / expander inputs), QueryDeviceInfo | 3 |
| 6 | **Output Tester** (logical + physical) | QueryLogicalOutputPortConfig/Name/Levels, QueryOutputDeviceConfig, QueryOutputDevicePortConfig, QueryPhysicalOutputDeviceLevels, SetOutputTestMode, SetLogicalOutputPortLevel, SetPhysicalOutputPortLevel, SetLogicalPortMode, Query/SetNightMode | 3 |
| 7 | **IR Tester** | QueryIRCommandsReceived, QueryIRRawPulsesReceived, SendIRCommand, QueryTVONState, SetTVRelayManualState | 3 |
| 8 | **Nudge window** (+ MicroPinSim ball/flipper test sim) | QueryNudgeStatus, QueryNudgeParams, PutNudgeParams, CommitNudgeSettings, RevertNudgeSettings, SetNudgeCenterPoint, StartNudgeCalibration | 4 |
| 9 | **Plunger Calibration** | QueryPlungerConfig, QueryPlungerReading (incl. imaging-sensor pixel snapshots for TCD1103/TSL1410R), StartPlungerCalibration, SetPlungerCalibrationData, Commit/RevertPlungerSettings, jitter filter, firing state | 4 |
| 10 | **Boot Loader / firmware update** (drag-drop UF2, erase config, erase whole flash) | EnterBootLoader + RP2 boot device | 5 |
| 11 | **New Device Setup** (fresh Pico in BOOTSEL) | RP2 boot device | 5 |
| 12 | **Help** (1.9 MB HTML + PNG wiring diagrams) | — | 2 |
| 13 | About, preferences, print config | — | 6 |

Vendor command codes involved (from `VendorIfcProtocol.h`): QUERY_VERSION 0x01, QUERY_IDS 0x02,
QUERY_USBIFCS 0x03, RESET 0x04, SET_CLOCK 0x05, CONFIG 0x06, NUDGE 0x07, TVON 0x0A, STATS 0x0B,
QUERY_LOG 0x0C, SEND_IR 0x0D, FLASH_STORAGE 0x0E, PLUNGER 0x0F, BUTTONS 0x10, OUTPUTS 0x11,
PING 0x12, QUERY_GPIO_CONFIG 0x13, QUERY_IR 0x14, SYNC_CLOCKS 0x15, DEBUG 0x16.

---

## 4. USB layer design (`pinscape-usb`)

### 4.1 Wire format (verified against `VendorIfcProtocol.h`)
All little-endian, packed.

**VendorRequest — 28 bytes, host→device on the bulk OUT endpoint**

| Offset | Field | Type | Notes |
|---|---|---|---|
| 0 | token | u32 | host-chosen; echoed in reply |
| 4 | checksum | u32 | `!(token + cmd + xferBytes)` (wrapping) |
| 8 | cmd | u8 | `CMD_*` |
| 9 | argsSize | u8 | bytes of `args` actually used |
| 10 | xferBytes | u16 | length of extra OUT data packet that follows |
| 12 | args | [u8;16] | command-specific union |

**VendorResponse — 28 bytes, device→host on the bulk IN endpoint**

| Offset | Field | Type |
|---|---|---|
| 0 | token | u32 |
| 4 | cmd | u8 |
| 5 | argsSize | u8 |
| 6 | status | u16 (`OK=0`, `ERR_*` 1..17) |
| 8 | xferBytes | u16 (extra IN data that follows) |
| 10 | reserved | u16 |
| 12 | args | [u8;16] |

Every request gets exactly one response. Extra data is sent/read immediately after the struct.

**Read the IN pipe as a byte stream, not packet-by-packet** (verified on hardware,
2026-11-08). The device sometimes packs a reply header and its data into the same 64-byte
packets, and ends full 4 KB transfers with a zero-length packet. A packet-oriented reader
desyncs and times out on the 3rd config page; the upstream Linux API papers over this with a
200 ms sleep per page. `pinscape-usb` uses `nusb::io::EndpointRead` (stream semantics, like
WinUSB) and downloads a 65 KB config in ~0.5 s with no sleeps.

### 4.2 Discovery (no VID/PID assumptions)
Users can reconfigure the device's VID/PID in JSON, so discovery must not depend on them:
1. Enumerate all USB devices with `nusb::list_devices()`.
2. For each, scan interfaces for **class 0xFF** whose interface string is
   **`PinscapePicoControl`** (confirmed on a live device: `iInterface 4 PinscapePicoControl`).
3. Find the interface's bulk IN/OUT endpoints from its descriptor (don't hard-code numbers).
4. Fast path: `1209:EAEA` / `1209:EAEB` are checked first.

Per-OS notes:
- **Windows** – firmware publishes MS OS 2.0 descriptors, so WinUSB binds automatically
  (GUID `{D3057FB3-8F4C-4AF9-9440-B220C3B2BA23}`). nusb opens WinUSB interfaces directly.
  No driver install.
- **Linux** – needs read/write access to `/dev/bus/usb/...`. Installers ship
  `packaging/udev/99-pinscape.rules`; the app detects `EACCES` and shows a "fix permissions"
  dialog with the exact command.
- **macOS** – vendor-class interfaces are not claimed by any kernel driver; nusb/IOKit can
  claim them without entitlements beyond the standard sandbox exception (app is not sandboxed).

Access is **exclusive** (one program at a time, by protocol design). The app surfaces a clear
"device in use by another program" error.

### 4.3 Concurrency model
- One **worker thread per connected device** owns the `nusb::Interface` and serializes
  request/response transactions (the protocol is strictly request→response).
- Tauri commands send jobs to the worker via a channel and `await` a oneshot reply.
- **Live streams** (button states, nudge, plunger, output levels, log) are polling jobs
  scheduled on the same worker at a per-window rate (e.g. 60 Hz plunger, 30 Hz buttons,
  4 Hz log), emitted to the UI as Tauri events. Only the visible tab's stream runs.
- Hot-plug via `nusb::watch_devices()`; reconnect logic matches by hardware ID (QueryIDs),
  so a device that reboots (reset, config save, firmware update) is re-attached to its tab.
- Timeouts: 2 s default, longer for flash writes; one automatic retry on
  `ERR_RETRY_OK`/timeout for idempotent queries.

### 4.4 Firmware update / RP2 boot loader
Two mechanisms, chosen automatically:

1. **PICOBOOT over USB (primary).** The RP2040/RP2350 boot ROM exposes a vendor
   "PICOBOOT" interface (RP2040 `2E8A:0003`, RP2350 `2E8A:000F`). Writing a UF2 through it
   needs no mounted drive, works on Linux even when nothing auto-mounts RPI-RP2
   (observed on the dev machine), and gives real progress reporting.
   We only erase/program the flash ranges covered by the UF2, which preserves Pinscape's
   config storage in the last flash sector.
2. **Mass-storage copy (fallback).** Find a mounted volume containing `INFO_UF2.TXT` and copy
   the UF2. Required on **Windows with RP2040**, whose boot ROM lacks MS OS descriptors, so
   PICOBOOT would need a Zadig driver install. RP2350 auto-binds WinUSB.

Flow: Enter boot loader via `CMD_RESET/SUBCMD_RESET_BOOTLOADER` → wait for boot device →
validate UF2 family ID vs CPU type (RP2040 vs RP2350 "absolute") → flash → reboot →
wait for Pinscape to reappear → verify version.

"Erase config" / "Erase whole flash" (BootLoaderWin) map to PICOBOOT flash-erase of the
config area / whole flash (with strong confirmation dialogs), or to the existing
`ClearConfig` UF2 approach in fallback mode.

---

## 5. Config editor design (`pinscape-config` + Monaco)

The config editor is the single largest feature (4k lines in Win32). Monaco provides the
editing engine, so the work is in language support.

### 5.1 Language: "pinscape-json"
Pinscape configs are JSON5-like: **unquoted keys, `//` and `/* */` comments, trailing
commas**. Monaco's built-in JSON/JSONC mode rejects unquoted keys, so we register a custom
language:
- **Monarch tokenizer** for highlighting (keys, strings, numbers, booleans, comments, GPIO
  values).
- **Diagnostics**: debounced (300 ms) call to Rust `validate_config(text)` which runs the
  firmware-compatible parser (port of `Firmware/JSON.cpp`) and the schema checker, returning
  `{line, col, severity, message, helpAnchor}` markers. Parsing 65 KB is sub-millisecond.
  Saving to the device is **refused on syntax errors** and **confirmed on schema warnings**,
  matching the Windows behavior.
- **Completion provider**: property names valid at the cursor's JSON path (with type, summary,
  "required" flag), enum values, `true/false`, GPIO numbers.
- **Hover provider**: property summary + "Open docs" link to the exact anchor in
  `JSONConfigRef.htm`.
- **F1 / context help**: open the help pane at the anchor for the property under the cursor.
- **Breadcrumb / "where am I" bar** showing the JSON path (replaces the nav drop-down menu).
- Auto-indent and bracket matching come from Monaco language configuration.
- Optional **Emacs keybindings** (Windows tool has them) via `monaco-emacs`.

### 5.2 Schema pipeline
The Windows build runs `ConfigDocParser` to turn `Firmware/JSONConfigRef.txt` into
`JSONConfigRef.htm` + a C++ header (`ConfigSchema.h`). We add a **JSON output mode** so the
same source produces `schema.json` consumed by both Rust (validation) and TypeScript
(completion/hover):

```
PinscapePico/Firmware/JSONConfigRef.txt ──ConfigDocParser──► JSONConfigRef.htm  (help)
                                          └───(new --json)──► schema.json        (app)
```
Option A: patch ConfigDocParser (portable C++, no Windows deps) and upstream the change.
Option B: re-implement the doc parser in Rust as a `build.rs` step. Decide in Phase 2
(A is cheaper; B removes the C++ build dependency).

### 5.3 Editor features list
Load from device · Save to device (+optional reset) · Reload · Import file · Import template
(the five templates from `GUIConfigTool/ConfigTemplates`) · Export to file · Safe-mode config
(separate file ID `CONFIG_FILE_SAFE_MODE`) · Find/replace · Go to line · Error panel ·
Auto-backup of every device save to the app data dir (timestamped, pruned) · Unsaved-changes
guard · Print (via webview print).

---

## 6. UI design

- **Left sidebar**: device list (unit #, name, HWID, firmware version, connection state,
  boot-loader devices shown separately with "Set up / Update" actions).
- **Main area**: tabs per device: Overview · Config · Safe-Mode Config · Buttons · Outputs ·
  Nudge · Plunger · IR · Log · Firmware.
- **Status bar**: connection, last operation, device uptime.
- Light/dark themes following the OS.
- Drag-and-drop a `.uf2` anywhere → firmware update flow; a `.json` → import into editor.
  (Tauri's file-drop works on Windows, macOS, X11 and Wayland via GTK.)
- **Help pane**: the existing HTML help shipped as app resources and shown in a side panel /
  separate window. Pages are the upstream files (BSD-3), synced by a script.

---

## 7. Repository layout

```
pinscape-cross/
├─ docs/PLAN.md                ← this file
├─ Cargo.toml                  Rust workspace
├─ crates/
│  ├─ pinscape-protocol/       wire structs, constants, encode/decode (+ unit tests)
│  ├─ pinscape-usb/            nusb transport, discovery, VendorClient, picoboot
│  ├─ pinscape-config/         JSON5-ish parser (firmware-compatible), schema check
│  └─ pinscape-cli/            `pinscape` CLI
├─ src-tauri/                  Tauri app (commands, session manager, streams)
├─ ui/                         React + TS + Vite frontend
├─ packaging/udev/             Linux udev rule
├─ scripts/                    sync-upstream-assets (help, templates, schema)
└─ third_party/PinscapePico    (optional git submodule of upstream, for assets/schema)
```

---

## 8. Phased delivery

Estimates assume one experienced developer; ranges reflect unknowns.

### Phase 0 — Scaffold & toolchain (2–3 days) ✅ done (2026-11-08)
- Workspace, crates, Tauri app, React UI skeleton, udev rule, CI matrix (GitHub Actions:
  ubuntu-24.04, windows-latest, macos-latest).
- **Exit**: `cargo tauri dev` opens a window on all three OSes; `cargo test` green.

### Phase 1 — Protocol core + Overview (2–3 weeks) ✅ done on Linux (2026-11-08); Windows/macOS pending a CI run and a hardware test
- `pinscape-protocol`: all request/response structs and every `CMD_*`/`SUBCMD_*`.
- `pinscape-usb`: discovery (§4.2), worker thread, timeouts, hot-plug, reconnect.
- `pinscape-cli`: `list`, `info`, `stats`, `get-config`, `put-config`, `reset`, `safe-mode`,
  `bootloader`, `log --follow`. Parity with (and beyond) `CmdLineConfigTool`.
- UI: device sidebar, Overview tab, reset/safe-mode/factory-mode/erase-config actions,
  wall-clock sync on connect.
- **Exit**: on all 3 OSes, app lists the device, shows overview, resets it, and reattaches
  after reboot. CLI round-trips a config byte-for-byte.

### Phase 2 — Config editor, Log viewer, Help (3–4 weeks) ✅ done on Linux (2026-11-08)

Done: schema pipeline (`tools/schema-gen` links upstream ConfigDocParser.cpp unmodified and
emits schema.json); parser port; **differential test against the firmware's compiled JSON.cpp**
(`tools/json-diff/run.py`: 6 seeds × 1,005 cases identical; it found and fixed the
duplicate-property "last one wins" rule); Monaco language, diagnostics, completion, hover,
breadcrumbs, F1 help pane, Problems list, save checks; import/export/templates; automatic backups
of every device save (30 days) with restore; Help tab for the upstream pages; log viewer.
Deferred: Emacs keybindings, print.

Original scope:

- Schema pipeline (§5.2), `pinscape-config` parser with golden tests against the firmware's
  parser output on all templates + fuzzed inputs.
- Monaco language, diagnostics, completion, hover, help links, breadcrumbs.
- Load/save/import/export/templates/safe-mode config/auto-backup.
- Log viewer (polled `QUERY_LOG`, filter, search, save).
- Help pane with upstream HTML.
- **Exit**: a user can do everything in the Windows Config Editor, on Linux and macOS.

### Phase 3 — Live testers: Buttons, Outputs, IR (2–3 weeks) ✅ done on Linux (2026-11-08)

Built in parallel by three agents (one per feature, each in its own repo copy and own files),
then merged. On real hardware: Buttons tab verified live (BOOTSEL press and GP2 pull-up read
correctly); Outputs and IR tabs verified reading config/levels/state. Not yet verified on
hardware: setting output levels, test mode, sending/receiving IR (needs `irRx`/`irTx` wiring),
TV-ON relay. Deferred: button event log (latency measurement), IR pulse export, PWMWorker reset UI.

Original scope:

- Streams infrastructure (start/stop per visible tab, throttled events).
- Button tester (logical + GPIO/shift-register/expander inputs).
- Output tester (logical/physical, test mode, sliders, night mode). **Always exit test
  mode** on tab close, app quit, disconnect (safety: solenoids).
- IR tester (received commands, raw pulses, send, TV-ON state, relay control).

### Phase 4 — Nudge & Plunger (2–3 weeks) ✅ done on Linux (2026-11-08), simulator-tested

Built in parallel (Nudge and Plunger agents). Nudge: live raw/filtered/velocity views and
uPlot charts, centering and 15 s calibration, full parameter editor (apply/save/revert), ball
physics tester. Plunger: live gauge with calibration markers, firing state, Z/speed chart,
image-sensor pixel strip, calibration with undo, all settings with apply/save/revert/defaults.
Not hardware-tested (no accelerometer or plunger sensor on the test Pico yet). Findings to
check on hardware / raise upstream: nudge velocity-decay setting appears inverted vs. its
documentation; plunger calibration values are native units (not 0–65535 as the header says);
firing time limit 0 over USB is stored literally (not "default").

Original scope:

- uPlot real-time graphs, calibration flows, commit/revert semantics.
- Plunger imaging-sensor pixel view (canvas) for TCD1103/TSL1410R.
- MicroPinSim (ball + flipper toy simulator for testing nudge) ported to canvas — may
  slip to v1.1.

### Phase 5 — Firmware & boot loader (1–2 weeks) ✅ RP2040 PICOBOOT flash verified on hardware (2026-11-08)

Built in parallel with Phase 4. UF2 parsing/validation and flash planning (only covered
sectors erased; images reaching the top 4 KB config sector of 2/4/8/16 MB flash refused;
RP2350-E10 workaround block skipped), PICOBOOT client with simulated boot ROM tests,
mass-storage fallback, boot-loader devices in the sidebar, Firmware tab with progress, and
`pinscape update <file.uf2> [--dry-run]` in the CLI.

Hardware result (RP2040, Linux): reboot into boot loader → PICOBOOT erase of 149 sectors →
write → read-back verify → reboot → Pinscape v1.0.2 back with the config byte-identical.
The ack-after-IN-command handshake and 4 KB chunks work as implemented. Requires the udev
rule to cover `2e8a:0003`/`000f` (the CLI now prints the fix when it doesn't). Still to verify:
RP2350 (REBOOT2 flags), the app's Firmware tab on hardware, mass-storage fallback, Windows and
macOS. Not yet: erase config / whole flash via PICOBOOT.

Original scope:

- PICOBOOT implementation (§4.4) + mass-storage fallback, progress UI, UF2 validation,
  new-device setup wizard, erase config / whole flash.

### Phase 6 — Polish & release (1–2 weeks)
- Preferences, About, keyboard shortcuts, print, accessibility pass.
- Installers + signing/notarization; auto-update via Tauri updater (optional).
- User docs.

**Total: ~11–17 weeks** to full parity.
Usable milestones: **Phase 2 end (~6 weeks)** already covers the most-used workflow
(edit config, logs, help) on all OSes.

---

## 9. Testing strategy

| Layer | How |
|---|---|
| Protocol encode/decode | Unit tests with byte vectors derived from `VendorIfcProtocol.h` layouts; checksum tests |
| USB transport | `Transport` trait with a **mock device** (scripted responses) for CI; real-device integration tests behind `--features hw-tests` |
| Config parser | Golden tests on all upstream templates; differential tests vs firmware `JSON.cpp` compiled natively; `cargo fuzz` |
| UI | Vitest for stores/logic; Playwright smoke tests with mocked Tauri `invoke` |
| End-to-end | Manual test matrix per release: {Win 11, Ubuntu 24.04 (X11+Wayland), Debian 13, macOS 14/15} × {RP2040, RP2350} |

Safety tests: output test mode always cleared on exit/crash path; firmware update refuses
mismatched family IDs; config save refuses syntax errors.

---

## 10. Risks & mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Windows RP2040 boot ROM has no WinUSB descriptor | PICOBOOT unavailable on Win/RP2040 | Mass-storage copy fallback (what the Windows tool does today) |
| Linux permissions | "No device found" confusion | udev rule in packages; EACCES detection with fix-it dialog |
| Linux distros without auto-mount | Mass-storage path fails | PICOBOOT is primary on Linux |
| Parser divergence from firmware | Configs that validate in app fail on device | Port JSON.cpp closely; differential tests against compiled C++ |
| Protocol coverage gaps in docs | Wrong struct decoding | Cross-check with `WinAPI/PinscapeVendorInterface.cpp` (reference implementation) |
| WebKitGTK quirks on Linux (rendering perf, Wayland) | Janky graphs | uPlot canvas; throttle events to display rate; test on X11 + Wayland |
| macOS signing/notarization | Gatekeeper warnings | Apple Developer ID needed for release builds; unsigned dev builds documented |
| Upstream protocol changes | Breakage on new firmware | Version-gate features on `QUERY_VERSION`; track upstream `USBProtocol/` changes |
| Exclusive vendor interface | Conflicts with Windows tool / CLI running | Clear "in use" error; CLI and app share the same crate |

---

## 11. Possible v2 items
- Form-based config editing for common sections (buttons, outputs) generated from the schema.
- Visual Pico pinout / GPIO-assignment view built from `QUERY_GPIO_CONFIG`.
- DOF config helper (port mapping export).
- Multi-device dashboard.
- Upstream contribution of the Rust protocol crate / CLI to the PinscapePico project.

---

## 12. Prerequisites (dev machine)

| | Linux (Debian/Ubuntu) | Windows | macOS |
|---|---|---|---|
| Rust | rustup (stable ≥ 1.77) | rustup + MSVC Build Tools | rustup + Xcode CLT |
| Node | `sudo apt install nodejs npm` (Debian 13: Node 20; Ubuntu 24.04: Node 18; Ubuntu 22.04 needs NodeSource/nvm) | Node 20+ | Node 20+ |
| System | `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config`, plus the udev rule from `packaging/udev/` | WebView2 (preinstalled on Win 10/11) | — |
| Tauri CLI | `cargo install tauri-cli --version "^2"` (or `npm run tauri`) | same | same |

---

## 13. Open questions
1. ~~App name~~ Decided: **Pinscape-Cross** (2026-11-08). Must not imply it's the official tool
   unless upstream adopts it.
2. Upstream relationship: contribute back (schema JSON output, Rust crates) or stay separate?
3. Keep the help as upstream HTML, or convert to Markdown for easier theming?
4. Minimum OS versions (proposal: Win 10 1809+, Ubuntu 22.04+/Debian 12+, macOS 12+).
