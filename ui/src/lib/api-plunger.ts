// Typed wrappers for the plunger Tauri commands (src-tauri/src/plunger.rs).
import { invoke } from "@tauri-apps/api/core";

/** Sensor type codes (FeedbackControllerReport::PLUNGER_*). */
export const SENSOR = {
  NONE: 0,
  POT: 1,
  AEDR8300: 2,
  VCNL4010: 3,
  VL6180X: 4,
  TCD1103: 5,
  TSL1410R: 6,
  TSL1412S: 7,
} as const;

/** PlungerFiringState values. */
export const FIRING = { NONE: 0, MOVING: 1, FIRED: 2, SETTLING: 3 } as const;
export const FIRING_STATES = ["None", "Moving", "Fired", "Settling"] as const;

export interface PlungerConfig {
  sensorType: number;
  sensorTypeName: string;
  /** Maximum raw reading in native units (minimum is 0). */
  nativeScale: number;
  flags: number;
  configured: boolean;
  imageSensor: boolean;
  hasIntegrationTime: boolean;
  /** Scan-mode names (index = mode byte); empty if not applicable. */
  scanModes: string[];
}

export type SensorData =
  | { kind: "quadrature"; sensorType: number; state: number; a: boolean; b: boolean }
  | { kind: "imageSensor"; sensorType: number; timestamp: number; nPix: number; pixels: number[] }
  | { kind: "vcnl4010"; sensorType: number; proxCount: number }
  | { kind: "opaque"; sensorType: number; data: number[] };

export interface PlungerReading {
  flags: number;
  reverse: boolean;
  calibrated: boolean;
  calibrating: boolean;
  modified: boolean;
  zbLaunch: boolean;
  /** Native units, after orientation and jitter filter. */
  rawPos: number;
  /** Microseconds. */
  timestamp: number;
  /** Joystick Z axis (processed); 0 = rest, 32767 = full retraction. */
  z: number;
  /** Instantaneous Z axis. */
  z0: number;
  z0Prv: number;
  z0Nxt: number;
  dt: number;
  /** Z units per centisecond; negative = moving forward. */
  speed: number;
  firingState: number;
  firingStateName: string;
  calMin: number;
  calZero: number;
  calMax: number;
  calSensorData: number[];
  firingTimeMeasured: number;
  firingTimeLimit: number;
  integrationTime: number;
  manualScalingFactor: number;
  jfWindow: number;
  jfLo: number;
  jfHi: number;
  jfLastPre: number;
  jfLastPost: number;
  scanMode: number;
  sensor: SensorData | null;
}

/** PlungerCal: calibration data that can be saved and put back into effect. */
export interface PlungerCal {
  calibrated: boolean;
  calMin: number;
  calZero: number;
  calMax: number;
  firingTimeMeasured: number;
  sensorData: number[];
}

export function calFromReading(r: PlungerReading): PlungerCal {
  return {
    calibrated: r.calibrated,
    calMin: r.calMin,
    calZero: r.calZero,
    calMax: r.calMax,
    firingTimeMeasured: r.firingTimeMeasured,
    sensorData: [...r.calSensorData],
  };
}

export const plungerApi = {
  config: (key: string) => invoke<PlungerConfig>("plunger_config", { key }),
  reading: (key: string, pixels: boolean) => invoke<PlungerReading>("plunger_reading", { key, pixels }),
  calibrate: (key: string, autoSave: boolean) => invoke<void>("plunger_calibrate", { key, autoSave }),
  setJitterFilter: (key: string, window: number) => invoke<void>("plunger_set_jitter_filter", { key, window }),
  setFiringTimeLimit: (key: string, us: number) => invoke<void>("plunger_set_firing_time_limit", { key, us }),
  setIntegrationTime: (key: string, us: number) => invoke<void>("plunger_set_integration_time", { key, us }),
  setOrientation: (key: string, reverse: boolean) => invoke<void>("plunger_set_orientation", { key, reverse }),
  setScalingFactor: (key: string, percent: number) => invoke<void>("plunger_set_scaling_factor", { key, percent }),
  setScanMode: (key: string, mode: number) => invoke<void>("plunger_set_scan_mode", { key, mode }),
  setCalibration: (key: string, cal: PlungerCal) => invoke<void>("plunger_set_calibration", { key, cal }),
  commit: (key: string) => invoke<void>("plunger_commit", { key }),
  revert: (key: string) => invoke<void>("plunger_revert", { key }),
  restoreDefaults: (key: string) => invoke<void>("plunger_restore_defaults", { key }),
};
