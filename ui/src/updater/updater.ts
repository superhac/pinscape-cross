// In-app updates (Tauri updater plugin). Used by UpdateNotice (startup check)
// and by the About dialog ("Check for updates").
//
// Checks only run when the build has an update signing key configured
// (src-tauri/src/updater.rs, `updater_status`). Updates are never installed
// without the user clicking "Install and restart".
import { invoke } from "@tauri-apps/api/core";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdaterStatus {
  /** Update checks may run in this build. */
  enabled: boolean;
  /** Why checks are disabled (null when enabled). */
  reason: string | null;
  /** The new version can be installed in place (AppImage, MSI, NSIS, macOS app). */
  canInstall: boolean;
  /** appimage | deb | rpm | msi | nsis | app, or null for an unbundled build. */
  bundleType: string | null;
  /** Release page for manual downloads. */
  releasesUrl: string;
}

export interface UpdateInfo {
  /** The new version, e.g. "0.2.0". */
  version: string;
  /** The running version. */
  currentVersion: string;
  /** Publish date (RFC 3339), if the release has one. */
  date?: string;
  /** Release notes (Markdown text from the release). */
  notes?: string;
  /** "Install and restart" is possible; otherwise offer the download page. */
  canInstall: boolean;
  releasesUrl: string;
}

export type InstallProgress = { downloaded: number; total?: number };

let statusPromise: Promise<UpdaterStatus> | null = null;
let pending: Update | null = null;
let latest: UpdateInfo | null = null;
const listeners = new Set<(u: UpdateInfo | null) => void>();

/** Whether this build checks for updates, and how it can install them. */
export function updaterStatus(): Promise<UpdaterStatus> {
  statusPromise ??= invoke<UpdaterStatus>("updater_status").catch(() => {
    statusPromise = null;
    return {
      enabled: false,
      reason: "updater unavailable",
      canInstall: false,
      bundleType: null,
      releasesUrl: "https://github.com/superhac/pinscape-cross/releases/latest",
    };
  });
  return statusPromise;
}

/**
 * Ask the release server whether a newer version exists.
 * Resolves to the update, or null if this is the latest version.
 * Throws if update checks are disabled in this build or the check fails
 * (offline, server error); the startup check ignores those errors.
 * A found update is also shown by the sidebar notice.
 */
export async function checkForUpdates(): Promise<UpdateInfo | null> {
  const st = await updaterStatus();
  if (!st.enabled) throw new Error(`Update checks are off: ${st.reason ?? "not configured"}.`);
  const u = await check({ timeout: 15000 });
  if (pending && pending !== u) pending.close().catch(() => {});
  pending = u;
  latest = u
    ? {
        version: u.version,
        currentVersion: u.currentVersion,
        date: u.date,
        notes: u.body?.trim() || undefined,
        canInstall: st.canInstall,
        releasesUrl: st.releasesUrl,
      }
    : null;
  listeners.forEach((f) => f(latest));
  return latest;
}

/** The last update found by checkForUpdates (null if none). */
export function availableUpdate(): UpdateInfo | null {
  return latest;
}

/** Be told whenever a check finds an update (or finds none). Returns the unsubscribe function. */
export function onUpdateAvailable(f: (u: UpdateInfo | null) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

/**
 * Download and install the update found by the last check, then restart.
 * On Windows the installer closes the app itself and starts the new version.
 */
export async function installUpdate(onProgress?: (p: InstallProgress) => void): Promise<void> {
  if (!pending) throw new Error("No update to install.");
  let downloaded = 0;
  let total: number | undefined;
  await pending.downloadAndInstall((ev) => {
    if (ev.event === "Started") total = ev.data.contentLength;
    else if (ev.event === "Progress") downloaded += ev.data.chunkLength;
    onProgress?.({ downloaded, total });
  });
  await relaunch();
}

/** Open the GitHub release page in the browser. */
export function openReleasesPage(): Promise<void> {
  return invoke("updater_open_releases");
}
