// Typed wrappers for the outputs Tauri commands (src-tauri/src/outputs.rs).
import { invoke } from "@tauri-apps/api/core";

/** Device type codes (OutputPortDesc::DEV_*). */
export const DEV = {
  VIRTUAL: 1,
  GPIO: 2,
  TLC59116: 3,
  TLC5940: 4,
  PCA9685: 5,
  PCA9555: 6,
  HC595: 7,
  ZBLAUNCH: 8,
  PWMWORKER: 9,
  TLC5947: 10,
  SHAREGROUP: 11,
} as const;

/** Physical port usage (OutputDevPortDesc::TYPE_*). */
export const PORT_TYPE = { UNUSED: 0, DIGITAL: 1, PWM: 2 } as const;

export interface LogicalPort {
  /** 1-based port number. */
  port: number;
  flags: number;
  devId: number;
  devType: number;
  devTypeName: string;
  devPort: number;
  noisy: boolean;
  gamma: boolean;
  inverted: boolean;
  flipperLogic: boolean;
  computed: boolean;
  named: boolean;
  name: string | null;
}

export interface OutputDevice {
  addr: number;
  numPorts: number;
  numPortsPerChip: number;
  pwmRes: number;
  configIndex: number;
  devType: number;
  devTypeName: string;
  i2cBus: number | null;
  i2cAddr: number | null;
  portTypes: number[];
  /** Index of this device's first port in `OutputsLevels.device`. */
  firstPort: number;
}

export interface OutputsConfig {
  ports: LogicalPort[];
  devices: OutputDevice[];
}

export interface LogicalLevel {
  dofLevel: number;
  calcLevel: number;
  outLevel: number;
  lwState: number;
  lwProfile: number;
  lwMode: boolean;
  lwOn: boolean;
  lwPeriod: number;
}

export interface OutputsLevels {
  testMode: boolean;
  nightMode: boolean;
  logical: LogicalLevel[];
  device: number[];
}

export const outputsApi = {
  config: (key: string) => invoke<OutputsConfig>("outputs_config", { key }),
  levels: (key: string) => invoke<OutputsLevels>("outputs_levels", { key }),
  setPort: (key: string, port: number, level: number) => invoke<void>("outputs_set_port", { key, port, level }),
  setTestMode: (key: string, enable: boolean, timeoutMs: number) =>
    invoke<void>("outputs_set_test_mode", { key, enable, timeoutMs }),
  setDevicePort: (key: string, devType: number, configIndex: number, port: number, level: number) =>
    invoke<void>("outputs_set_device_port", { key, devType, configIndex, port, level }),
  setNightMode: (key: string, on: boolean) => invoke<void>("outputs_set_night_mode", { key, on }),
  allOff: (key: string) => invoke<void>("outputs_all_off", { key }),
};

/** Highest native level for a device port (digital ports are 0/1). */
export function portMax(d: OutputDevice, port: number): number {
  if (d.portTypes[port] === PORT_TYPE.DIGITAL || d.pwmRes <= 2) return 1;
  return Math.max(1, d.pwmRes - 1);
}

/** Native port label, e.g. "GP15", "OUT7", "LED3". */
export function pinLabel(devType: number, port: number): string {
  switch (devType) {
    case DEV.GPIO:
      return `GP${port}`;
    case DEV.PCA9685:
    case DEV.TLC59116:
      return `LED${port}`;
    case DEV.HC595:
      return `Q${port}`;
    case DEV.PCA9555:
      return `IO${port >> 3}_${port & 7}`;
    default:
      return `OUT${port}`;
  }
}

/** Device label, e.g. "TLC5940[0]", "PCA9685[1] (I2C0 0x40)". */
export function deviceLabel(d: OutputDevice): string {
  if (d.devType === DEV.GPIO) return "Pico GPIO";
  const base = `${d.devTypeName}[${d.configIndex}]`;
  return d.i2cAddr != null ? `${base} (I2C${d.i2cBus} 0x${d.i2cAddr.toString(16).padStart(2, "0")})` : base;
}

/** Logical port's mapping, e.g. "GPIO GP15", "TLC5940[0] OUT7", "Virtual". */
export function mappingLabel(p: LogicalPort): string {
  switch (p.devType) {
    case DEV.VIRTUAL:
      return "Virtual";
    case DEV.ZBLAUNCH:
      return "ZB Launch";
    case DEV.SHAREGROUP:
      return `Share group #${p.devId}`;
    case DEV.GPIO:
      return `GPIO GP${p.devPort}`;
    default:
      return `${p.devTypeName}[${p.devId}] ${pinLabel(p.devType, p.devPort)}`;
  }
}
