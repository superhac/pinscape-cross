# Changelog

All notable changes to Pinscape-Cross are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow uses the section for a version as the GitHub release notes, and the
app shows them in its update notice, so write entries for end users.

## [Unreleased]

## [0.6.0] - 2026-11-09

### Added

- **MPU-6050 accelerometer** (GY-521 boards and compatibles): the config editor knows the
  `mpu6050` section (autocomplete, hover help, reference page), and the Pinout tab shows its
  interrupt pin as an accelerometer pin. Needs Pinscape Pico firmware with MPU-6050 support.

### Changed

- The Nudge tab's "No accelerometer configured" help now says that a `nudge` section is
  required to turn nudging on.

## [0.5.0] - 2026-11-09

First public release: a cross-platform (Windows, Linux, macOS) replacement for the
Windows-only Pinscape Pico GUI Config Tool.

### Added

- **Device management**: Pinscape Picos are found automatically and appear in the sidebar
  when plugged in (hot-plug, several devices at once). The Overview tab shows device
  information, live status, USB interfaces and GPIO use, with buttons to reboot into
  normal, Safe, Factory or Boot Loader mode and to erase the configuration. The device
  clock is set on connect.
- **Config editor** for the main and Safe-Mode configurations: Pinscape JSON highlighting,
  live syntax errors (same parser and messages as the firmware) and rules-check warnings,
  schema-driven autocomplete, hover documentation, breadcrumb bar, F1 opens the reference,
  Problems list, Ctrl+S. Saving is blocked on syntax errors and asks for confirmation on
  warnings. Import/export, new configs from the upstream templates, and an automatic
  backup of every save with restore.
- **Log viewer** with ANSI colors, and a **Help** tab with the upstream help pages.
- **Live testers**: Buttons (logical buttons, shift state, GPIO / PCA9555 / 74HC165
  inputs), Outputs (logical and physical ports, level sliders, night mode, test mode,
  all off), IR & TV-ON (received codes, raw pulse timeline, send, TV-ON relay control).
- **Nudge**: live accelerometer views and charts, centering, calibration, all parameters
  with apply/save/revert, and a ball physics tester.
- **Plunger**: live gauge, firing state, chart, image-sensor pixels, calibration with
  undo, and sensor settings.
- **Firmware updates**: install a `.uf2` over USB (PICOBOOT, with a copy-to-drive
  fallback) with progress; the device configuration is kept. Picos in boot loader mode
  are listed in the sidebar for setting up new devices.
- **`pinscape` command-line tool**: `list`, `info`, `stats`, `usb`, `gpio`, `get-config`,
  `put-config`, `erase-config`, `log`, `set-clock`, `reset`, `validate` and `update`.
- Installers for Linux (`.deb` and `.rpm` with the udev rule included, AppImage),
  Windows (`.msi`, setup `.exe`) and macOS (`.dmg`).
- **Pinout** tab: a picture of the Pico / Pico 2 header showing what each GPIO is assigned
  to, coloured by use (buttons, outputs, I²C, SPI, UART, IR, plunger/ADC, accelerometer),
  with pin details on hover or focus, a list of free GPIOs, and warnings for GPIOs the board
  uses internally. Other boards get a plain GPIO list.
- **DOF tab**: the output ports as DOF numbers them (DOF port, name, type, chip and pin,
  flags) with the unit number DOF uses, checks for common DOF setup problems (unit number
  conflicts, Feedback Controller interface off, unaddressable or doubled-up ports, unnamed
  ports), and copy / save as CSV or as a text port list for the DOF Config Tool.
- **All devices** dashboard: every connected Pico at a glance (status, firmware, config,
  output test mode), with warnings for duplicate unit numbers, overlapping LedWiz units,
  mixed firmware versions and more. Open it from the top of the sidebar.
- **Form view** for the Config and Safe-Mode Config tabs (**Text | Form** switch): edit
  buttons, outputs and the other sections with drop-downs, check boxes and text boxes,
  with the reference description for each property. Add, duplicate and remove list
  entries. Changes go into the text as small edits, so comments and formatting are kept
  and Undo works across both views.

### Fixed

- Erasing flash over the boot loader (**Erase Pinscape config…**, `pinscape erase-flash`) and
  its dry run froze an RP2040's boot loader until it was unplugged: flash was read before the
  boot loader had set up the flash interface.
- A Pico seen during a reboot before its USB permissions were applied stayed "No permission to
  open" until it was replugged; devices that fail to open are now retried every 2 seconds.
- After printing, the config editor's minimap stayed blank until the tab was switched.

[Unreleased]: https://github.com/superhac/pinscape-cross/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/superhac/pinscape-cross/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/superhac/pinscape-cross/releases/tag/v0.5.0
