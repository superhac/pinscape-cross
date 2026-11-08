// Typed wrappers around the Rust commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export interface DeviceIds {
  hwid: string;
  cpu_type: number;
  cpu_version: number;
  rom_version: number;
  unit_num: number;
  xinput_player_index: number | null;
  led_wiz_unit_mask: number;
  unit_name: string;
  target_board: string;
  pico_sdk_version: string;
  tinyusb_version: string;
  compiler_version: string;
}

export interface Version {
  major: number;
  minor: number;
  patch: number;
  build_date: string;
}

export interface DeviceSnapshot {
  key: string;
  vid: number;
  pid: number;
  ids: DeviceIds | null;
  version: Version | null;
  error: string | null;
  errorKind: "permission" | "busy" | "other" | null;
}

export interface Statistics {
  upTimeUs: number;
  nLoops: number;
  nLoopsEver: number;
  avgLoopTimeUs: number;
  maxLoopTimeUs: number;
  nLoops2: number;
  nLoopsEver2: number;
  avgLoopTime2Us: number;
  maxLoopTime2Us: number;
  heapSize: number;
  heapUnused: number;
  arenaSize: number;
  arenaAlloc: number;
  arenaFree: number;
}

export interface UsbInterfaces {
  vid: number;
  pid: number;
  flags: number;
  keyboardConfigured: boolean;
  keyboardEnabled: boolean;
  gamepadConfigured: boolean;
  gamepadEnabled: boolean;
  xinputConfigured: boolean;
  xinputEnabled: boolean;
  pindevConfigured: boolean;
  pindevEnabled: boolean;
  feedbackConfigured: boolean;
  ledwizConfigured: boolean;
  cdcConfigured: boolean;
}

export interface GpioPort {
  gpio: number;
  func: number;
  funcName: string;
  output: boolean;
  adc: boolean;
  usage: string | null;
}

export interface Overview {
  stats: Statistics;
  usb: UsbInterfaces;
  gpio: GpioPort[];
  hasMainConfig: boolean;
  hasSafeModeConfig: boolean;
}

export interface Template {
  id: string;
  name: string;
  description: string;
  safeMode: boolean;
}

export interface EditorRange {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

export interface Diagnostic {
  range: EditorRange;
  severity: "error" | "warning" | "info";
  message: string;
  helpAnchor: string | null;
}

export interface Completion {
  label: string;
  kind: "property" | "value";
  detail: string | null;
  documentation: string | null;
  insertText: string;
  isSnippet: boolean;
  range: EditorRange;
  sortText: string;
  required: boolean;
}

export interface Hover {
  markdown: string;
  range: EditorRange;
  helpAnchor: string | null;
}

export interface Breadcrumb {
  path: string;
  property: string | null;
  helpAnchor: string | null;
}

export interface BackupInfo {
  file: string;
  savedAt: string;
  size: number;
}

export interface PutResult {
  backup: string | null;
  backupError: string | null;
}

export type ResetMode = "normal" | "safe-mode" | "factory" | "bootloader";
export type ConfigWhich = "main" | "safe-mode" | "all";

export const api = {
  listDevices: () => invoke<DeviceSnapshot[]>("list_devices"),
  onDevicesChanged: (cb: (d: DeviceSnapshot[]) => void): Promise<UnlistenFn> =>
    listen<DeviceSnapshot[]>("devices-changed", (e) => cb(e.payload)),
  overview: (key: string) => invoke<Overview>("device_overview", { key }),
  stats: (key: string) => invoke<Statistics>("query_stats", { key }),
  getConfig: (key: string, safeMode = false) => invoke<string | null>("get_config", { key, safeMode }),
  putConfig: (key: string, text: string, safeMode: boolean, resetAfter: boolean) =>
    invoke<PutResult>("put_config", { key, text, safeMode, resetAfter }),
  reset: (key: string, mode: ResetMode) => invoke<void>("reset_device", { key, mode }),
  eraseConfig: (key: string, which: ConfigWhich) => invoke<void>("erase_config", { key, which }),
  queryLog: (key: string) => invoke<string>("query_log", { key }),
  setClock: (key: string) => invoke<void>("set_clock", { key }),
  listTemplates: () => invoke<Template[]>("list_templates"),
  readTemplate: (id: string) => invoke<string>("read_template", { id }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  writeTextFile: (path: string, text: string) => invoke<void>("write_text_file", { path, text }),
  platform: () => invoke<string>("platform"),
  listBackups: (hwid: string, safeMode: boolean) => invoke<BackupInfo[]>("list_backups", { hwid, safeMode }),
  readBackup: (file: string) => invoke<string>("read_backup", { file }),
  backupFolder: () => invoke<string>("backup_folder"),
  devOptions: () => invoke<{ startTab: string | null }>("dev_options"),
  configValidate: (text: string) => invoke<Diagnostic[]>("config_validate", { text }),
  configCompletions: (text: string, line: number, col: number) =>
    invoke<Completion[]>("config_completions", { text, line, col }),
  configHover: (text: string, line: number, col: number) => invoke<Hover | null>("config_hover", { text, line, col }),
  configBreadcrumb: (text: string, line: number, col: number) =>
    invoke<Breadcrumb>("config_breadcrumb", { text, line, col }),
};

export const fmtVersion = (v: Version) => `${v.major}.${v.minor}.${v.patch}`;
export const hex4 = (n: number) => n.toString(16).padStart(4, "0");

export function fmtUptime(us: number): string {
  const s = Math.floor(us / 1e6);
  const d = Math.floor(s / 86400);
  const hms = [Math.floor(s / 3600) % 24, Math.floor(s / 60) % 60, s % 60]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
  return d > 0 ? `${d}d ${hms}` : hms;
}

export const fmtBytes = (n: number) => n.toLocaleString() + " bytes";
