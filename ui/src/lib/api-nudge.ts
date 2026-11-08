// Typed wrappers for the nudge Tauri commands (src-tauri/src/nudge.rs).
import { invoke } from "@tauri-apps/api/core";

/** NudgeStatus. Axis values are INT16 units; 32768 = gRange g. */
export interface NudgeStatus {
  cb: number;
  flags: number;
  /** In-memory settings differ from the last flash save/load. */
  modified: boolean;
  calibrating: boolean;
  gRange: number;
  xRaw: number;
  yRaw: number;
  zRaw: number;
  xFiltered: number;
  yFiltered: number;
  zFiltered: number;
  /** Microseconds since device startup; 0 = the device has never taken a reading. */
  timestamp: number;
  lastCenteringAgeUs: number;
  xCenter: number;
  yCenter: number;
  zCenter: number;
  xThreshold: number;
  yThreshold: number;
  zThreshold: number;
  xAvg: number;
  yAvg: number;
  zAvg: number;
  vx: number;
  vy: number;
  vz: number;
  /** Derived from timestamp != 0 (no accelerometer = firmware's null device). */
  hasAccelerometer: boolean;
}

/** NudgeParams. */
export interface NudgeParams {
  autoCenter: boolean;
  otherFlags: number;
  autoCenterIntervalMs: number;
  xThreshold: number;
  yThreshold: number;
  zThreshold: number;
  dcTimeMs: number;
  xJitterWindow: number;
  yJitterWindow: number;
  zJitterWindow: number;
  velocityDecayMs: number;
  velocityScalingFactor: number;
  lpFreqHz: number;
  /** The firmware's struct includes lpFreq (older firmware doesn't). */
  hasLpFreq: boolean;
}

/** Firmware noise calibration length (CAL_MODE_INTERVAL). */
export const CALIBRATION_MS = 15000;

export const nudgeApi = {
  status: (key: string) => invoke<NudgeStatus>("nudge_status", { key }),
  params: (key: string) => invoke<NudgeParams>("nudge_params", { key }),
  putParams: (key: string, params: NudgeParams) => invoke<NudgeParams>("nudge_put_params", { key, params }),
  commit: (key: string) => invoke<void>("nudge_commit", { key }),
  revert: (key: string) => invoke<NudgeParams>("nudge_revert", { key }),
  center: (key: string) => invoke<void>("nudge_center", { key }),
  calibrate: (key: string, autoSave: boolean) => invoke<void>("nudge_calibrate", { key, autoSave }),
};

export function paramsEqual(a: NudgeParams, b: NudgeParams): boolean {
  return (Object.keys(a) as (keyof NudgeParams)[]).every((k) => a[k] === b[k]);
}
