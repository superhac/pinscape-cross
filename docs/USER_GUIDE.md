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
| **Pinout** | A picture of the Pico's 40-pin header showing what each GPIO is used for; see [Pinout view](#pinout-view). |
| **DOF** | The output ports as DOF numbers them, with checks for common setup problems and exports for the DOF Config Tool; see [Setting up DOF](#setting-up-dof). |
| **Firmware** | Install a new firmware version; see below. |
| **Help** | The upstream Pinscape Pico help pages. |

Tabs for features your configuration doesn't use (no accelerometer, no plunger, no IR
receiver) say so instead of showing readings.

### Pinout view

The **Pinout** tab draws the board as seen from above, USB connector at the top, with the
physical pin numbers on the pads and the GP numbers, ground, power (VBUS, VSYS, 3V3,
ADC_VREF) and control pins (RUN, 3V3_EN) beside them. ADC-capable pins are marked ADC0–ADC2.
Next to each GPIO is what the running configuration uses it for (the same names the
firmware reports on the Overview tab, e.g. "Button #3" or "I2C0 SDA"), coloured by kind:
button input, output/PWM, I²C, SPI, UART, IR, plunger/ADC, accelerometer, board/internal,
other, or free.

- Hover over, focus or click a pin to see its full assignment in **Pin details**; clicking
  keeps it shown. Tab moves through the pins, and the arrow keys move around the diagram.
- Click a **Legend** entry to highlight only those pins (click again to show all).
- **Free GPIOs** lists the header pins nothing is assigned to, handy when wiring something new.
- **Not on the header** shows GP23, GP24, GP25 and GP29, which the board itself uses
  (power-save control, VBUS sense, LED, VSYS measurement; on a Pico W / Pico 2 W the wireless
  chip). If your configuration assigns one of them to something else, **Check these** warns.

The diagram is shown for the Raspberry Pi Pico, Pico W, Pico 2 and Pico 2 W (the firmware's
build target tells the app which board it is). For other boards, such as RP2350B boards
with more GPIOs or third-party layouts, the tab lists every GPIO by number instead of
drawing a header that might not match. The view reads the assignments from the device, so
after changing the config, save it with "reboot after saving" (or reboot) and the tab updates
within a few seconds.
## All devices (dashboard)

Cabinets often use two or more Picos, for example one for buttons and the plunger and
one for outputs. **All devices** at the top of the sidebar shows them side by side: one
card per Pico with its unit number and name, hardware ID, firmware version, uptime, main
loop time, whether a configuration is stored, the output state (test mode, Night Mode)
and its active USB interfaces. Picos that can't be opened ("in use", no permission) and
Picos in boot loader mode get a card too. Click a card, or its **Overview** button, to
switch to that device; **Reboot…** asks first, as on the Overview tab.

Above the cards, the dashboard lists problems it finds across the devices:

- **Duplicate unit numbers.** DOF and other programs tell Pinscape units apart by
  `id.unitNum`, so each Pico needs its own.
- **Overlapping LedWiz unit numbers** (when LedWiz emulation is on), and two devices
  reporting the same XInput player number.
- **Different firmware versions** on different Picos; the older ones are tagged.
- **Output test mode** left on, a Pico with **no configuration** stored, and a Pico that
  reports the default identity (unit 1, "Pinscape") although a configuration is stored.
  The firmware doesn't tell the app when it runs in Safe Mode or Factory Mode, so that
  last one is a hint to check the device's Log tab.

The dashboard refreshes every 3 seconds while it's shown, and stops when you leave it.

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

### Form view

The **Text | Form** switch in the toolbar shows the configuration as a form instead of
text. It's handy for the common sections: **buttons**, **outputs** and **id** are listed
first, followed by the other sections (dimmed if they aren't in the file yet).

- Each property has a control that fits its type: a drop-down for fixed choices (such as
  a button's `type` or a source's `type`), a check box for true/false, a text box for
  numbers and text. The short description from the reference is shown under it, and
  **help** opens the full reference at that property. Fields marked `*` are required.
- Properties that aren't set yet are folded away under "more properties"; give one a value
  to add it. **✕** removes a property. A property shown in orange isn't valid there (for
  example, left over after changing a source's `type`); fix or remove it.
- Lists (buttons, outputs) show one line per entry; click it to open the entry.
  **+ Add entry** appends a new one (a new button starts as a push button on a GPIO pin
  sending a key, so you only fill in the pin and key), **Duplicate** inserts a copy, and
  **Remove** deletes it, together with the comment lines directly above it (usually its
  label, such as `// Output #3`). Outputs are numbered from 1, like DOF ports.
- The text is still what gets saved. Every form change is a small edit to the text, so
  your comments, blank lines and layout are kept, new entries follow the file's existing
  indentation and style, and you can switch between the views at any time. **Undo** /
  **Redo** (Ctrl+Z / Ctrl+Y outside a text box) step through form and text edits alike.
- The form only works on a file without syntax errors. If there are any, it says so and
  takes you to the first one in the text view.
- **Save to device**, the warning check and backups work exactly as in the text view.
- Anything the form doesn't cover (comments, lists of numbers and unusual values) can
  still be typed in the text view; the form shows such values as text you can edit.

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

## Setting up DOF

DOF (DirectOutput Framework) drives your cabinet's feedback devices during games. It
recognizes Pinscape Picos by itself (unified DOF R3++ releases since late 2024); you assign
what each output does in the online DOF Config Tool (configtool.vpuniverse.com). The
**DOF** tab gives you what you need for that:

- **Unit number**: DOF tells Picos apart by `id.unitNum` in the config (default 1). Number
  your Picos 1, 2, 3…; KL25Z Pinscape units are numbered separately.
- **Port mapping**: DOF port numbers are the order of the entries in the config's `outputs`
  array (the first entry is port #1). The table shows each port's name, type, the chip and
  pin it drives, and its flags (noisy/Night Mode, gamma, inverted, Flipper Logic, computed).
- **Checks**: no outputs configured, Feedback Controller interface disabled (DOF can't see
  the Pico), two connected Picos with the same unit number or numbers not starting at 1,
  ports beyond #255 (DOF can't address them), two ports driving the same pin, duplicate or
  missing names, and notes on computed, share-group and ZB Launch ports.
- **Exports**: **Copy port list** / **Save port list…** (plain text), **Copy CSV** /
  **Save CSV…** (a spreadsheet of the table).

In the DOF Config Tool, open **Cabinet › Devices › Port Assignments**, choose the
Pinscape Pico with your unit number, and assign each port number from the list to its DOF
device. For share groups, assign the share-group ports, not the physical pool ports behind
them. Adding or removing an `outputs` entry renumbers the ports after it, so update your
DOF settings afterwards.

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
