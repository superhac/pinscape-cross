# Pinscape-Cross user guide

Pinscape-Cross configures and tests [Pinscape Pico](https://github.com/mjrgh/PinscapePico)
I/O controllers on Windows, Linux and macOS. It does the same job as the Windows-only
Pinscape Pico Config Tool: edit the device configuration, watch the log, test buttons,
outputs, IR, nudge and plunger, and install firmware.

> Pinscape-Cross is an unofficial project, not affiliated with the upstream Pinscape Pico
> project. For what each config setting does, see the upstream documentation (also built
> into the app's **Help** tab and the editor's **Reference** pane).

- [Installing](#installing)
- [First connection](#first-connection)
- [The tabs](#the-tabs)
- [Editing the configuration](#editing-the-configuration)
- [Firmware updates](#firmware-updates)
- [Recovering a Pico (BOOTSEL)](#recovering-a-pico-bootsel)
- [App updates](#app-updates)
- [Command-line tool](#command-line-tool)
- [Troubleshooting](#troubleshooting)

## Installing

Download the file for your system from the
[Releases page](https://github.com/superhac/pinscape-cross/releases/latest).

### Windows

Download `Pinscape-Cross_<version>_x64-setup.exe` (installs for your user) or
`Pinscape-Cross_<version>_x64_en-US.msi` (installs for all users, needs admin rights), and run it.

- No USB driver is needed: Pinscape Pico's configuration interface uses WinUSB, which
  Windows sets up by itself.
- If the installer isn't code-signed, SmartScreen shows "Windows protected your PC". Click
  **More info**, then **Run anyway**.
- The app needs Microsoft Edge WebView2, which is part of Windows 10 and 11. The installer
  fetches it if it's missing.

### macOS

Download the `.dmg` for your Mac: `_aarch64.dmg` for Apple silicon (M1 and later),
`_x64.dmg` for Intel Macs. Open it and drag **Pinscape-Cross** to **Applications**.

If the app isn't notarized, macOS refuses to open it the first time. Open **System
Settings > Privacy & Security**, scroll down to the message about Pinscape-Cross and click
**Open Anyway** (on older macOS: right-click the app, choose **Open**, then **Open**). If
macOS says the app "is damaged", run this once in Terminal:

```bash
xattr -cr /Applications/Pinscape-Cross.app
```

No USB setup is needed on macOS.

### Linux

Pick one:

- **Debian, Ubuntu, Mint and similar:** `Pinscape-Cross_<version>_amd64.deb`:

  ```bash
  sudo apt install ./Pinscape-Cross_*_amd64.deb
  ```

- **Fedora, openSUSE and similar:** `Pinscape-Cross-<version>-1.x86_64.rpm`:

  ```bash
  sudo dnf install ./Pinscape-Cross-*.x86_64.rpm
  ```

- **Any distribution:** `Pinscape-Cross_<version>_amd64.AppImage`. Make it executable
  (`chmod +x Pinscape-Cross_*.AppImage`) and run it. The AppImage can update itself
  from inside the app.

**USB permissions.** On Linux a udev rule must allow normal users to open the Pico. The
`.deb` and `.rpm` packages install it for you. With the AppImage (or the CLI on its own),
install it once by hand. The rule file is in the source tree and in each CLI archive
(`99-pinscape.rules`):

```bash
sudo cp 99-pinscape.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

The rule must cover the Pico in boot loader mode too, or firmware updates fail. Details,
how to check it, and how to fix an older hand-made rule are in the README's
[Linux USB permissions](../README.md#linux-usb-permissions) section. Never run the app
with `sudo`.

## First connection

1. Plug the Pinscape Pico into a USB port and start Pinscape-Cross.
2. The Pico appears in the **sidebar** within a second or two, named by its unit name
   and hardware ID. With several Picos connected, click the one you want.
3. The **Overview** tab opens with the device's firmware version, status and pin usage.
   The app sets the Pico's clock when it connects.

A brand-new Pico without Pinscape firmware shows up under **Boot loader mode** in the
sidebar instead; see [Firmware updates](#firmware-updates).

**Only one program at a time** can use the configuration interface. Close the upstream
Windows Config Tool or the `pinscape` command-line tool before using the app, and the
other way round. If the sidebar says the device is "in use", that's the reason.
Games, DOF and other programs that use the keyboard, gamepad or feedback interfaces
are not affected.

## The tabs

| Tab | What it's for |
|---|---|
| **Overview** | Device and firmware information, live status, USB interfaces, GPIO pin assignments. Buttons to reboot normally, into Safe Mode, Factory Mode or the Boot Loader, and to erase the configuration. |
| **Config** | The main configuration (JSON) in a full editor; see [Editing the configuration](#editing-the-configuration). |
| **Safe-Mode Config** | The configuration the Pico uses in Safe Mode, after a crash or when you choose Safe Mode. Keep it minimal so the Pico can always start. |
| **Log** | The Pico's message log, with colors. Check it after saving a config: the firmware reports problems with devices and settings here. |
| **Buttons** | Live state of each logical button and shift state, and the raw inputs (GPIO, PCA9555, 74HC165 chips). Press a button and watch it light up. |
| **Outputs** | Logical output ports with their levels, sliders to set them, physical chip ports, Night Mode and **All off**. **Test mode** drives the physical ports directly; leaving the tab (or closing the app) ends it, and the Pico returns to normal within a few seconds. |
| **Nudge** | Accelerometer readings and charts, centering, calibration, and the nudge parameters (apply to try, save to keep, revert). Includes a ball-physics tester. |
| **Plunger** | Live plunger position and firing state, chart, image-sensor pixels, calibration (with undo) and sensor settings. |
| **IR** | Received IR remote codes in the config's text format (ready to copy), a raw pulse view, sending codes, and the TV-ON power sense and relay. |
| **Firmware** | Install a new firmware version; see below. |
| **Help** | The upstream Pinscape Pico help pages. |

Tabs for features your configuration doesn't use (no accelerometer, no plunger, no IR
receiver) say so instead of showing readings.

## Editing the configuration

The **Config** and **Safe-Mode Config** tabs load the configuration from the Pico when
you open them.

- **Errors and warnings** appear as you type: red underlines for syntax errors (the same
  checks and messages as the firmware), yellow for settings the firmware would reject or
  ignore. The **Problems** list below the editor lists them all; click one to jump to it.
- **Autocomplete** (Ctrl+Space) suggests property names and values allowed at the cursor.
  Hover over a property for its documentation. **F1** opens the reference for the
  property under the cursor beside the editor. The bar above the editor shows where you
  are in the file.
- **Save to device** (Ctrl+S) uploads the text. Saving is refused while there are syntax
  errors and asks for confirmation if there are warnings. Tick **reboot after saving** to
  restart the Pico so the new settings take effect (most settings only apply after a reboot).
- **Reload from device** discards your edits and loads the Pico's copy again.
- **New from template…** starts from one of the upstream example configurations.
  **Import file…** and **Export…** load and save `.json` files on your computer.
- The app asks before closing, or replacing the text, while you have unsaved changes.
  Edits survive switching tabs.

### Backups

Every time you save a configuration to a Pico, the app first keeps a copy on your
computer. **Backups…** lists the copies for the selected Pico; choose one to load it into
the editor, then **Save to device** to restore it. Copies are kept for 30 days, in:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\io.github.superhac.pinscape-cross\backups` |
| macOS | `~/Library/Application Support/io.github.superhac.pinscape-cross/backups` |
| Linux | `~/.local/share/io.github.superhac.pinscape-cross/backups` |

For a configuration you want to keep longer, use **Export…**.

### If a config stops the Pico from working

If the Pico crashes repeatedly after a change, the firmware starts in **Safe Mode** with
the Safe-Mode configuration. Open the **Config** tab, fix or restore the configuration from
**Backups…**, save it, and reboot normally from the Overview tab.

## Firmware updates

Download the firmware (`.uf2` file) from the
[Pinscape Pico releases](https://github.com/mjrgh/PinscapePico/releases). Pick the file for
your board: Pico (RP2040) or Pico 2 (RP2350). The app refuses an image built for the
other chip.

1. Open the **Firmware** tab, then choose the file (or drag it onto the tab).
2. The app shows the image's details and whether it fits this Pico. Click **Install**.
3. The Pico reboots into its boot loader, the app writes and verifies the new firmware and
   restarts the Pico. A progress window shows each step. Don't unplug the Pico meanwhile.
4. The Pico reconnects and the new version appears on the Overview tab.

The configuration is kept: only the flash area the firmware occupies is rewritten.
Still, an **Export…** of your configuration first doesn't hurt.

When the app can't use the boot loader's USB interface directly (for example an RP2040 on
Windows), it copies the file onto the Pico's `RPI-RP2` drive instead, the way you would by hand.

## Recovering a Pico (BOOTSEL)

You can always get back to a working state, whatever was flashed or configured:

1. Unplug the Pico.
2. Hold down the **BOOTSEL** button on the Pico and plug it back in, then release the button.
3. The Pico is now in boot loader mode. It appears in the app's sidebar under
   **Boot loader mode** (and as a USB drive named `RPI-RP2` or `RP2350`).
4. Click **Install firmware…** next to it and choose a Pinscape `.uf2` file. Or click
   **Restart Pico** to leave the boot loader and run whatever is already in flash.

The same steps set up a brand-new Pico. Without the app you can also copy the `.uf2` file
onto the boot drive.

If a configuration (rather than the firmware) is the problem, the **Factory Mode** reboot
on the Overview tab starts the Pico with default settings, and **Erase Config** removes the
stored configuration.

## App updates

When a new version of Pinscape-Cross is published, a small notice appears at the bottom of
the sidebar a few seconds after startup, with the version and release notes.

- **Install and restart** downloads the update, checks its signature, installs it and
  restarts the app (Windows installer, macOS app and AppImage).
- With a `.deb` or `.rpm` install, the notice offers **Download…** instead: install the new
  package the same way as the first one.
- **Later** hides the notice until the next start.

The app never installs anything without your click, and stays silent when you're offline.
To turn the check off, set the environment variable `PINSCAPE_NO_UPDATE_CHECK=1`.

## Command-line tool

The `pinscape` command-line tool is in the `pinscape-cli-*` archives on the Releases page.
It needs the same USB permissions as the app (on Linux, the udev rule above). Some examples:

```bash
pinscape list                       # connected Picos
pinscape info                       # firmware version, IDs
pinscape get-config my-config.json  # download the configuration
pinscape validate my-config.json    # check a file offline
pinscape put-config my-config.json  # upload it
pinscape update PinscapePico.uf2    # install firmware
pinscape --help                     # everything else
```

Close the app before using the CLI; they can't both use the Pico at the same time.

## Troubleshooting

| Problem | What to do |
|---|---|
| The Pico doesn't appear in the sidebar | Check the cable carries data (some are charge-only), try another USB port, and check the Pico runs Pinscape firmware (its LED blinks). A Pico in boot loader mode is listed separately. |
| "No permission to open" (Linux) | Install the udev rule ([Linux](#linux)) and replug. The sidebar's **Copy command** button copies the fix. |
| "In use by another program" | Close the upstream Config Tool, the `pinscape` CLI, or a second copy of Pinscape-Cross. |
| Firmware update stops after the reboot with "permission denied" (Linux) | The udev rule doesn't cover the boot loader (`2e8a` IDs). Update it as described in [Linux USB permissions](../README.md#updating-an-older-rule). The Pico waits in boot-loader mode with nothing erased; start the install again from the sidebar. |
| Config saved but nothing changed | Most settings apply after a reboot: tick **reboot after saving**, or use **Reboot** on the Overview tab. Then look at the **Log** tab for messages about the new settings. |
| The Pico keeps rebooting or is stuck in Safe Mode | See [If a config stops the Pico from working](#if-a-config-stops-the-pico-from-working), or [recover with BOOTSEL](#recovering-a-pico-bootsel). |
| The window is blank (Linux) | Try `WEBKIT_DISABLE_DMABUF_RENDERER=1 pinscape-cross` (a WebKitGTK graphics-driver issue on some systems). |
| macOS says the app is damaged or can't be opened | See [macOS](#macos). |

Reporting a problem: open an issue at
<https://github.com/superhac/pinscape-cross/issues> with your system, the app version,
the firmware version (Overview tab) and, if relevant, the contents of the Log tab.
