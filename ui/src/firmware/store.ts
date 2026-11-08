// Firmware installation state, kept outside React: the Firmware tab unmounts
// while its device reboots into the boot loader, but the install (and its
// progress dialog, InstallProgress) carries on.
import { useEffect, useState } from "react";
import { UnlistenFn } from "@tauri-apps/api/event";
import { firmwareApi, FirmwareProgress, InstallRequest, InstallResult } from "../lib/api-firmware";

export interface InstallState {
  running: boolean;
  /** Shown as the dialog title, e.g. "Installing PinscapePico-pico.uf2". */
  title: string;
  progress: FirmwareProgress | null;
  result: InstallResult | null;
  error: string | null;
  /** The dialog is shown until the user dismisses the result. */
  open: boolean;
}

let state: InstallState = { running: false, title: "", progress: null, result: null, error: null, open: false };
const listeners = new Set<(s: InstallState) => void>();

function set(patch: Partial<InstallState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l(state));
}

export function useInstallState(): InstallState {
  const [s, setS] = useState(state);
  useEffect(() => {
    listeners.add(setS);
    setS(state);
    return () => {
      listeners.delete(setS);
    };
  }, []);
  return s;
}

export const installRunning = () => state.running;

/** Start an install; resolves when it finishes (never rejects). */
export function startInstall(req: InstallRequest, title: string): Promise<InstallResult | null> {
  return startJob(title, () => firmwareApi.install(req));
}

/** Run any boot-loader job (install, erase) with the shared progress dialog. Never rejects. */
export async function startJob(title: string, run: () => Promise<InstallResult>): Promise<InstallResult | null> {
  if (state.running) return null;
  set({ running: true, title, progress: null, result: null, error: null, open: true });
  let un: UnlistenFn | null = null;
  try {
    un = await firmwareApi.onProgress((p) => set({ progress: p }));
    const result = await run();
    set({ running: false, result });
    return result;
  } catch (e) {
    set({ running: false, error: String(e) });
    return null;
  } finally {
    un?.();
  }
}

export function dismissInstall() {
  if (!state.running) set({ open: false });
}

// ---- "drop a .uf2 anywhere" coordination ----
// An open installer (Firmware tab, boot-loader dialog) handles dropped files
// itself; the always-mounted BootLoaderList only handles drops nobody claims.
let dropClaims = 0;

/** Called by a visible Installer; returns the release function. */
export function claimDrops(): () => void {
  dropClaims++;
  let released = false;
  return () => {
    if (!released) dropClaims--;
    released = true;
  };
}

export const dropsClaimed = () => dropClaims > 0;
