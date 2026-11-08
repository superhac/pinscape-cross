# Pinscape-Cross

A cross-platform (Windows · Linux · macOS) configuration tool for
[Pinscape Pico](https://github.com/mjrgh/PinscapePico) virtual-pinball I/O
controllers, built with [Tauri 2](https://tauri.app) (Rust backend, React UI).

Goal: feature parity with the Windows-only Pinscape Pico GUI Config Tool.
See **[docs/PLAN.md](docs/PLAN.md)** for the full plan, architecture and phases.

> Unofficial project; not affiliated with the upstream Pinscape Pico project.

<img width="1553" height="831" alt="image" src="https://github.com/user-attachments/assets/6c248144-53a6-4120-a77e-aadf1c62a49b" />

## Status: Phase 3 (live testers) done; Linux tested, Windows/macOS not yet

| Piece | State |
|---|---|
| `pinscape-protocol` (wire format) | Request/response, version, IDs, stats, USB interfaces, GPIO, clock, log, config paging, CRC-32; unit-tested |
| `pinscape-usb` (nusb transport) | Discovery by interface string, stream-based request/response, config get/put/exists/erase, stats, GPIO, log, clock, reset. **Tested on hardware.** Includes a simulated device (`mock.rs`) used by the tests |
| `pinscape-config` | Port of the firmware's JSON parser (same syntax, error messages and recovery); schema checker matching the Windows tool's rules check; cursor context for completion, hover and breadcrumbs. All five upstream templates validate with zero findings; differential-tested against the firmware's own parser (`tools/json-diff`) |
| `pinscape` CLI | `list`, `info`, `stats`, `usb`, `gpio`, `get-config`, `put-config`, `erase-config`, `log [-f]`, `set-clock`, `reset`, `validate <file>` (offline config check) |
| Tauri app | Hot-plug device manager; Overview; **Config and Safe-Mode Config in a Monaco editor**: Pinscape JSON highlighting, live syntax errors and rules warnings, schema-driven autocomplete (including subclass properties such as `source: { type: "gpio", ... }`), hover docs, breadcrumb bar, F1 opens the upstream reference beside the editor, Problems list, save blocked on syntax errors and confirmed on warnings, Ctrl+S; import/export and templates; edits survive tab switches; prompt before closing with unsaved edits; automatic backup of every save with restore. Help tab with the upstream help pages. Log viewer; help for permission/"in use" errors |
| Live testers | **Buttons** (logical buttons with live state, shift state, GPIO / PCA9555 / 74HC165 input grids), **Outputs** (logical ports with DOF/computed/output levels and sliders, physical device ports, night mode, test mode with device-side timeout and keepalive, all off), **IR & TV-ON** (received codes in the config's text format, raw pulse timeline, send, TV-ON state and relay control) |
| CI | GitHub Actions: build, test and clippy on Ubuntu, Windows and macOS |

The vendor interface is **exclusive**: close the app before using the CLI
(or the upstream config tools), and vice versa.

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
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
```

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

**4. USB access for your user.** Without this rule, the app and CLI can only
open the device as root:

```bash
sudo cp packaging/udev/99-pinscape.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Then unplug the Pico and plug it back in. The `.deb` and `.rpm` installers
include this rule, so end users don't need this step.

### Windows

- Rust via [rustup](https://rustup.rs), with the MSVC Build Tools (rustup offers to install them)
- [Node.js 20+](https://nodejs.org)
- WebView2 is preinstalled on Windows 10/11. No USB driver is needed: the
  device's WinUSB interface installs itself.

### macOS

- Xcode command-line tools: `xcode-select --install`
- Rust via [rustup](https://rustup.rs)
- Node.js 20+ (`brew install node`, or from nodejs.org)

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
