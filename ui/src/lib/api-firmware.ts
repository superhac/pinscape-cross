// Typed wrappers for the firmware Tauri commands (src-tauri/src/firmware.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { Version } from "../api";

export type Chip = "rp2040" | "rp2350";
export const chipName = (c: Chip) => (c === "rp2040" ? "RP2040" : "RP2350");
/** DeviceIds.cpu_type (2040 / 2350) → Chip. */
export const chipOfCpu = (cpu: number): Chip | null => (cpu === 2040 ? "rp2040" : cpu === 2350 ? "rp2350" : null);

export const RELEASES_URL = "https://github.com/mjrgh/PinscapePico/releases";

export interface FamilyInfo {
  id: number;
  name: string;
  blocks: number;
  payloadBytes: number;
  minAddr: number;
  endAddr: number;
}

export interface Uf2Summary {
  path: string;
  fileName: string;
  fileBytes: number;
  totalBlocks: number;
  families: FamilyInfo[];
  chips: Chip[];
  kind: "pinscape" | "pwmworker" | "unknown";
  e10Block: boolean;
  warnings: string[];
  flashStart: number | null;
  flashEnd: number | null;
  sectors: number | null;
  planError: string | null;
}

export interface BootDevice {
  key: string;
  chip: Chip;
  vid: number;
  pid: number;
  serial: string | null;
  picoboot: boolean;
}

export interface BootVolume {
  path: string;
  chip: Chip;
  info: { version: string; model: string | null; boardId: string; tags: Record<string, string> };
}

export interface BootList {
  devices: BootDevice[];
  volumes: BootVolume[];
}

export type Phase =
  | "enter-boot-loader"
  | "wait-boot-loader"
  | "connecting"
  | "erasing"
  | "writing"
  | "verifying"
  | "copying"
  | "rebooting"
  | "wait-device"
  | "done";

export interface FirmwareProgress {
  phase: Phase;
  done: number;
  total: number;
  message: string | null;
}

export const PHASE_LABEL: Record<Phase, string> = {
  "enter-boot-loader": "Restarting into the boot loader…",
  "wait-boot-loader": "Waiting for the boot loader…",
  connecting: "Connecting to the boot loader…",
  erasing: "Erasing flash…",
  writing: "Writing firmware…",
  verifying: "Verifying…",
  copying: "Copying to the boot drive…",
  rebooting: "Restarting the Pico…",
  "wait-device": "Waiting for the Pinscape device to come back…",
  done: "Done",
};

export type InstallMethod = "auto" | "picoboot" | "volume";

export interface InstallRequest {
  path: string;
  deviceKey?: string | null;
  bootKey?: string | null;
  volume?: string | null;
  method: InstallMethod;
  verify: boolean;
}

export interface InstallResult {
  method: "picoboot" | "volume";
  deviceKey: string | null;
  version: Version | null;
  notes: string[];
  /** UI only: replaces the default success line (used for erase jobs). */
  headline?: string;
}

export type EraseTarget = "config" | "all";

/** One allocation in Pinscape's flash file system directory. */
export interface FsFile {
  name: string;
  addr: number;
  size: number;
  replaced: boolean;
}

/** Exactly what an erase touched (bootloader.rs ErasePlan). */
export interface ErasePlan {
  target: EraseTarget;
  chip: Chip;
  flashSize: number;
  flashSizeSource: "given" | "detected" | "build" | "maximum";
  detectedFlashSize: number | null;
  /** [address, length] runs. */
  ranges: [number, number][];
  sectors: number;
  bytes: number;
  fsFound: boolean;
  dirAddr: number | null;
  files: FsFile[];
  notes: string[];
}

export interface EraseRequest {
  deviceKey?: string | null;
  bootKey?: string | null;
  target: EraseTarget;
  /** Override the flash size in bytes (normally detected). */
  flashSize?: number | null;
  verify: boolean;
}

export interface EraseResult {
  plan: ErasePlan;
  deviceKey: string | null;
  version: Version | null;
  inBootLoader: boolean;
  notes: string[];
}

export const firmwareApi = {
  inspect: (path: string) => invoke<Uf2Summary>("firmware_inspect", { path }),
  install: (req: InstallRequest) => invoke<InstallResult>("firmware_install", { req }),
  bootList: () => invoke<BootList>("bootloader_list"),
  bootReboot: (key: string) => invoke<void>("bootloader_reboot", { key }),
  erase: (req: EraseRequest) => invoke<EraseResult>("bootloader_erase", { req }),
  onProgress: (cb: (p: FirmwareProgress) => void): Promise<UnlistenFn> =>
    listen<FirmwareProgress>("firmware-progress", (e) => cb(e.payload)),
  onBootLoadersChanged: (cb: (d: BootDevice[]) => void): Promise<UnlistenFn> =>
    listen<BootDevice[]>("bootloaders-changed", (e) => cb(e.payload)),
};

export const hex8 = (n: number) => "0x" + n.toString(16).padStart(8, "0");
