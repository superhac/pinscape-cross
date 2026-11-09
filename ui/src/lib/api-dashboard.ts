// Typed wrapper for the dashboard command (src-tauri/src/dashboard.rs).
import { invoke } from "@tauri-apps/api/core";
import { DeviceIds, Statistics, UsbInterfaces } from "../api";

export interface DashStatus {
  ids: DeviceIds | null;
  stats: Statistics;
  usb: UsbInterfaces;
  hasMainConfig: boolean;
  hasSafeModeConfig: boolean;
  testMode: boolean | null;
  nightMode: boolean | null;
}

export interface DashDevice {
  key: string;
  status: DashStatus | null;
  error: string | null;
}

export type Severity = "error" | "warning" | "info";

export type ProblemKind =
  | "duplicate-unit"
  | "duplicate-xinput"
  | "ledwiz-overlap"
  | "firmware-mismatch"
  | "test-mode"
  | "maybe-safe-mode"
  | "no-config"
  | "unavailable";

export interface Problem {
  severity: Severity;
  kind: ProblemKind;
  message: string;
  keys: string[];
}

export interface DashReport {
  devices: DashDevice[];
  problems: Problem[];
}

export const dashboardApi = {
  /** One status round trip per connected device, plus cross-device problems. */
  poll: () => invoke<DashReport>("dashboard_poll"),
};
