// Typed wrapper for the DOF config helper command (src-tauri/src/dof.rs).
import { invoke } from "@tauri-apps/api/core";

/** A Pinscape Pico as DOF identifies it (by unit number). */
export interface DofUnit {
  unitNum: number;
  unitName: string;
  hwid: string;
}

export type DofPortKind = "physical" | "virtual" | "shareGroup" | "zbLaunch" | "unknown";

export interface DofPort {
  /** DOF port number: 1-based position in the config's `outputs` array. */
  dofPort: number;
  name: string | null;
  kind: DofPortKind;
  /** e.g. "Pico GPIO", "TLC5940[0]", "PCA9685[0] (I2C0 0x40)", "Share group #0". */
  device: string;
  /** e.g. "GP15", "OUT7"; empty for non-physical ports. */
  pin: string;
  noisy: boolean;
  gamma: boolean;
  inverted: boolean;
  flipperLogic: boolean;
  computed: boolean;
  /** Port number fits the Feedback Controller protocol (1..255). */
  addressable: boolean;
}

export type DofSeverity = "error" | "warning" | "info";

export interface DofProblem {
  severity: DofSeverity;
  /** Ports concerned; empty for device-wide problems. */
  ports: number[];
  message: string;
}

export interface DofReport {
  unit: DofUnit;
  /** Feedback Controller USB interface (the one DOF uses); null if unknown. */
  feedbackEnabled: boolean | null;
  ports: DofPort[];
  /** Most severe first. */
  problems: DofProblem[];
  /** Other Pinscape Picos currently connected. */
  otherUnits: DofUnit[];
  /** Mapping table as CSV. */
  csv: string;
  /** Plain-text port list for working in the DOF Config Tool. */
  portList: string;
}

export const dofApi = {
  report: (key: string) => invoke<DofReport>("dof_report", { key }),
};

export const KIND_LABEL: Record<DofPortKind, string> = {
  physical: "Physical",
  virtual: "Virtual",
  shareGroup: "Share group",
  zbLaunch: "ZB Launch",
  unknown: "Unknown",
};
