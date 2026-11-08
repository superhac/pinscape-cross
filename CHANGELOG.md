# Changelog

All notable changes to Pinscape-Cross are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow uses the section for a version as the GitHub release notes, and the
app shows them in its update notice, so write entries for end users.

## [Unreleased]

## [0.1.0] - Unreleased

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
  Windows (`.msi`, setup `.exe`) and macOS (`.dmg`), plus in-app update notices.

[Unreleased]: https://github.com/superhac/pinscape-cross/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/superhac/pinscape-cross/releases/tag/v0.1.0
