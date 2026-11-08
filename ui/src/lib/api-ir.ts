// Typed wrappers for the ir Tauri commands (src-tauri/src/ir.rs).
import { invoke } from "@tauri-apps/api/core";

/** Universal IR code. `code` is hex text (u64 doesn't fit a JS number). */
export interface IrCode {
  protocol: number;
  flags: number;
  code: string;
}

export interface IrCommandReceived {
  dtUs: number;
  code: IrCode;
  protocolName: string;
  /** Universal format, e.g. "01.02.20DF10EF" — paste-able into the config. */
  text: string;
  proFlags: number;
  cmdFlags: number;
  proHasDittos: boolean;
  hasToggle: boolean;
  toggle: boolean;
  hasDitto: boolean;
  ditto: boolean;
  position: "first" | "middle" | "last" | null;
  autoRepeat: boolean;
}

export interface IrRawPulse {
  mark: boolean;
  /** Microseconds; null = longer than 131.068 ms. */
  tUs: number | null;
}

export interface TvOnState {
  powerState: number;
  powerStateName: string;
  powerStateDescription: string;
  gpioHigh: boolean;
  relayState: number;
  relayOn: boolean;
  relayPowerOn: boolean;
  relayManual: boolean;
  relayManualPulse: boolean;
  irCommandIndex: number;
  irCommandCount: number;
}

export interface IrPoll {
  commands: IrCommandReceived[];
  raw: IrRawPulse[];
  irError: string | null;
  tvon: TvOnState | null;
  tvonError: string | null;
}

export interface IrConfigStatus {
  rxGpio: number | null;
  txGpio: number | null;
}

export interface IrProtocolInfo {
  id: number;
  name: string;
}

export type TvRelayAction = "on" | "off" | "pulse";

export const irApi = {
  poll: (key: string) => invoke<IrPoll>("ir_poll", { key }),
  send: (key: string, code: string, count: number) => invoke<IrCode>("ir_send", { key, code, count }),
  setTvRelay: (key: string, action: TvRelayAction) => invoke<void>("ir_set_tv_relay", { key, action }),
  configStatus: (key: string) => invoke<IrConfigStatus>("ir_config_status", { key }),
  protocols: () => invoke<IrProtocolInfo[]>("ir_protocols"),
};

/**
 * Client-side check of the universal format, matching the firmware parser
 * (IRCommandDesc::Parse): two hex digits each for protocol and flags, then
 * 1–16 hex digits for the code. The backend re-parses before sending.
 */
export function parseIrCode(text: string): { protocol: number; flags: number; code: string } | null {
  const m = /^\s*([0-9a-f]{2})\.([0-9a-f]{2})\.([0-9a-f]{1,16})\s*$/i.exec(text);
  if (!m) return null;
  return { protocol: parseInt(m[1], 16), flags: parseInt(m[2], 16), code: m[3].toUpperCase() };
}

/** TV ON power states in sequence order (names as in the upstream tools). */
export const TVON_STATES: [number, string][] = [
  [9, "Pico Boot"],
  [0, "Power Off"],
  [1, "Pulsing Latch"],
  [2, "Testing Latch"],
  [3, "Countdown"],
  [4, "Pulsing Relay"],
  [5, "IR Ready"],
  [7, "IR Sending"],
  [6, "IR Waiting"],
  [8, "Power On"],
];
