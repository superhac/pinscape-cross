// Typed wrappers for the buttons Tauri commands (src-tauri/src/buttons.rs),
// plus the protocol constants and display helpers the Buttons tab uses.
import { invoke } from "@tauri-apps/api/core";

/** One logical button (protocol `ButtonDesc`). */
export interface ButtonDesc {
  type: number;
  sourceType: number;
  sourceUnit: number;
  sourcePort: number;
  sourceActiveHigh: boolean;
  actionType: number;
  actionDetail: number;
  flags: number;
  shiftMask: number;
  shiftBits: number;
}

/** A PCA9555 chip or 74HC165 chain (protocol `ButtonDevice`). */
export interface ButtonDevice {
  configIndex: number;
  type: number;
  numPorts: number;
  /** PCA9555: (bus << 8) | 7-bit I2C address. */
  addr: number;
}

export interface ButtonConfig {
  buttons: ButtonDesc[];
  devices: ButtonDevice[];
}

export interface ButtonsLive {
  /** Logical states, in descriptor order. */
  states: boolean[];
  shiftState: number;
  /** Raw GPIO levels from GP0 (true = HIGH). */
  gpio: boolean[];
  /** Raw PCA9555 levels, 16 per chip in config order. */
  pca9555: boolean[];
  /** Raw 74HC165 levels, 8 per chip, chains in config order. */
  hc165: boolean[];
}

export const buttonsApi = {
  config: (key: string) => invoke<ButtonConfig>("buttons_config", { key }),
  /** `pca9555` / `hc165`: whether to also read those expanders' ports. */
  live: (key: string, pca9555: boolean, hc165: boolean) =>
    invoke<ButtonsLive>("buttons_live", { key, pca9555, hc165 }),
};

// ---- Protocol constants (ButtonDesc::TYPE_*, SRC_*, ACTION_*) ----

export const BTN = { PUSH: 1, HOLD: 2, PULSE: 3, TOGGLE: 4, ONOFF: 5, SHIFT: 6 } as const;
export const SRC = {
  GPIO: 1,
  BOOTSEL: 2,
  PCA9555: 3,
  HC165: 4,
  ACCEL: 5,
  PLUNGER: 6,
  ZBLAUNCH: 7,
  IR: 8,
  CLOCK: 9,
  OUTPORT: 10,
  NULL: 11,
  ADC: 12,
} as const;
export const ACT = {
  NONE: 1,
  KEY: 2,
  MEDIA: 3,
  GAMEPAD: 4,
  XINPUT: 5,
  RESET: 6,
  NIGHTMODE: 7,
  PLUNGERCAL: 8,
  IR: 9,
  MACRO: 10,
  OPENPINDEV: 11,
} as const;

const TYPE_NAMES: Record<number, string> = {
  1: "Push",
  2: "Hold",
  3: "Pulse",
  4: "Toggle",
  5: "On/Off",
  6: "Shift",
};

export const buttonTypeName = (t: number) => TYPE_NAMES[t] ?? `Type ${t}`;

/** True for pin inputs (GPIO, PCA9555, 74HC165), where polarity applies. */
export const isPhysical = (b: ButtonDesc) =>
  b.sourceType === SRC.GPIO || b.sourceType === SRC.PCA9555 || b.sourceType === SRC.HC165;

/** PCA9555 port 0..15 as the datasheet name IO0_0..IO1_7. */
export const pcaPortName = (p: number) => `IO${p >> 3}_${p & 7}`;

/** Short source description, e.g. "GP5", "PCA9555[0] IO1_3", "74HC165[0] port 12". */
export function sourceName(b: ButtonDesc): string {
  switch (b.sourceType) {
    case SRC.GPIO:
      return `GP${b.sourcePort}`;
    case SRC.BOOTSEL:
      return "BOOTSEL";
    case SRC.PCA9555:
      return `PCA9555[${b.sourceUnit}] ${pcaPortName(b.sourcePort)}`;
    case SRC.HC165:
      return `74HC165[${b.sourceUnit}] port ${b.sourcePort}`;
    case SRC.ACCEL:
      return "Accelerometer";
    case SRC.PLUNGER:
      return "Plunger";
    case SRC.ZBLAUNCH:
      return "ZB Launch Ball";
    case SRC.IR:
      return "IR receiver";
    case SRC.CLOCK:
      return "Time of day";
    case SRC.OUTPORT:
      return `Output port #${b.sourcePort}`;
    case SRC.NULL:
      return "None";
    case SRC.ADC:
      return `ADC channel ${b.sourcePort}`;
    default:
      return `Source ${b.sourceType}`;
  }
}

/** Source plus polarity for pin inputs, e.g. "GP5 (active low)". */
export const sourceLabel = (b: ButtonDesc) =>
  isPhysical(b) ? `${sourceName(b)} (active ${b.sourceActiveHigh ? "high" : "low"})` : sourceName(b);

// HID keyboard usage page 0x07
const KEY_NAMES: Record<number, string> = (() => {
  const m: Record<number, string> = {};
  for (let i = 0; i < 26; i++) m[0x04 + i] = String.fromCharCode(65 + i);
  for (let i = 1; i <= 9; i++) m[0x1d + i] = String(i);
  m[0x27] = "0";
  const named: [number, string][] = [
    [0x28, "Enter"], [0x29, "Esc"], [0x2a, "Backspace"], [0x2b, "Tab"], [0x2c, "Space"],
    [0x2d, "-"], [0x2e, "="], [0x2f, "["], [0x30, "]"], [0x31, "\\"], [0x32, "# (non-US)"],
    [0x33, ";"], [0x34, "'"], [0x35, "`"], [0x36, ","], [0x37, "."], [0x38, "/"],
    [0x39, "Caps Lock"], [0x46, "Print Screen"], [0x47, "Scroll Lock"], [0x48, "Pause"],
    [0x49, "Insert"], [0x4a, "Home"], [0x4b, "Page Up"], [0x4c, "Delete"], [0x4d, "End"],
    [0x4e, "Page Down"], [0x4f, "Right"], [0x50, "Left"], [0x51, "Down"], [0x52, "Up"],
    [0x53, "Num Lock"], [0x54, "Keypad /"], [0x55, "Keypad *"], [0x56, "Keypad -"],
    [0x57, "Keypad +"], [0x58, "Keypad Enter"], [0x62, "Keypad 0"], [0x63, "Keypad ."],
    [0x64, "\\ (non-US)"], [0x65, "Menu"], [0x7f, "Mute"], [0x80, "Volume Up"], [0x81, "Volume Down"],
    [0xe0, "Left Ctrl"], [0xe1, "Left Shift"], [0xe2, "Left Alt"], [0xe3, "Left GUI"],
    [0xe4, "Right Ctrl"], [0xe5, "Right Shift"], [0xe6, "Right Alt"], [0xe7, "Right GUI"],
  ];
  for (const [k, v] of named) m[k] = v;
  for (let i = 0; i < 12; i++) m[0x3a + i] = `F${i + 1}`;
  for (let i = 0; i < 12; i++) m[0x68 + i] = `F${i + 13}`;
  for (let i = 1; i <= 9; i++) m[0x58 + i] = `Keypad ${i}`;
  return m;
})();

// HID consumer usage page 0x0C (the subset that fits in the one-byte detail)
const MEDIA_NAMES: Record<number, string> = {
  0x30: "power", 0x32: "sleep", 0xb0: "play", 0xb1: "pause", 0xb2: "record", 0xb3: "fast forward",
  0xb4: "rewind", 0xb5: "next track", 0xb6: "previous track", 0xb7: "stop", 0xb8: "eject",
  0xcd: "play/pause", 0xe2: "mute", 0xe9: "volume up", 0xea: "volume down",
};

const XINPUT_NAMES = ["D-pad up", "D-pad down", "D-pad left", "D-pad right", "Start", "Back", "L3", "R3",
  "LB", "RB", "Xbox", "button 11", "A", "B", "X", "Y"];

const HAT_NAMES: Record<number, string> = { 100: "hat up", 101: "hat down", 102: "hat left", 103: "hat right" };

const OPD_NAMES = ["Start", "Exit", "Extra Ball", "Coin 1", "Coin 2", "Coin 3", "Coin 4", "Launch Ball",
  "Lockbar Fire", "Left Flipper", "Right Flipper", "Left Flipper 2", "Right Flipper 2", "Left MagnaSave",
  "Right MagnaSave", "Tilt Bob", "Slam Tilt", "Coin Door", "Service Cancel", "Service Down", "Service Up",
  "Service Enter", "Left Nudge", "Forward Nudge", "Right Nudge", "Vol Up", "Vol Down"];

const hex2 = (n: number) => `0x${n.toString(16).toUpperCase().padStart(2, "0")}`;

export const keyName = (usage: number) => KEY_NAMES[usage] ?? `usage ${hex2(usage)}`;

/** Human-readable action, e.g. "Key: Left Shift", "Gamepad button 3", "XInput A". */
export function actionLabel(b: ButtonDesc): string {
  const d = b.actionDetail;
  switch (b.actionType) {
    case ACT.NONE:
      return "None";
    case ACT.KEY:
      return `Key: ${keyName(d)}`;
    case ACT.MEDIA:
      return `Media: ${MEDIA_NAMES[d] ?? `usage ${hex2(d)}`}`;
    case ACT.GAMEPAD:
      return HAT_NAMES[d] ? `Gamepad ${HAT_NAMES[d]}` : `Gamepad button ${d}`;
    case ACT.XINPUT:
      return `XInput ${XINPUT_NAMES[d] ?? `button ${d}`}`;
    case ACT.RESET:
      return "Reset Pico";
    case ACT.NIGHTMODE:
      return "Night Mode";
    case ACT.PLUNGERCAL:
      return "Plunger calibration";
    case ACT.IR:
      return "Send IR command";
    case ACT.MACRO:
      return "Macro";
    case ACT.OPENPINDEV:
      if (d >= 1 && d <= 32) return `Pinball Device generic button ${d}`;
      return `Pinball Device ${OPD_NAMES[d - 33] ?? `button ${d}`}`;
    default:
      return `Action ${b.actionType}`;
  }
}

/** Shift bits as a list of set bit numbers, e.g. 0x5 -> "0, 2". */
export function bitList(v: number): string {
  const bits: number[] = [];
  for (let i = 0; i < 32; i++) if ((v >>> i) & 1) bits.push(i);
  return bits.join(", ");
}
