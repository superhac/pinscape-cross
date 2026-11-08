// Preferences store: loaded once from the Rust side (src-tauri/src/prefs.rs),
// changed in memory immediately and written back shortly after each change.
import { useSyncExternalStore } from "react";
import { api, Preferences } from "../api";

export const DEFAULT_PREFS: Preferences = {
  theme: "system",
  uiFontSize: "normal",
  editor: {
    fontSize: 13,
    tabSize: 4,
    insertSpaces: true,
    minimap: true,
    wordWrap: false,
    emacsKeys: false,
    confirmSave: true,
    rebootAfterSave: true,
  },
  backups: { enabled: true, retentionDays: 30 },
  startup: { rememberTab: true, rememberDevice: true, lastTab: null, lastDevice: null },
};

type Patch = {
  [K in keyof Preferences]?: Preferences[K] extends Record<string, unknown> ? Partial<Preferences[K]> : Preferences[K];
};

let current: Preferences = DEFAULT_PREFS;
let loaded = false;
let loading: Promise<Preferences> | null = null;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();
let saveError: ((e: unknown) => void) | null = null;

const notify = () => listeners.forEach((l) => l());

/** Load preferences from disk (once); resolves with the loaded values. */
export function loadPrefs(): Promise<Preferences> {
  if (!loading) {
    loading = api
      .prefsGet()
      .catch(() => DEFAULT_PREFS)
      .then((p) => {
        current = merge(DEFAULT_PREFS, p);
        loaded = true;
        notify();
        return current;
      });
  }
  return loading;
}

export const getPrefs = () => current;
export const prefsLoaded = () => loaded;

/** Called when writing preferences.json fails. */
export function onPrefsSaveError(cb: ((e: unknown) => void) | null) {
  saveError = cb;
}

/** Apply a (nested, partial) change and persist it. */
export function updatePrefs(patch: Patch) {
  current = merge(current, patch as Partial<Preferences>);
  notify();
  if (!loaded) return; // the loaded file would overwrite it anyway
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    api.prefsSet(current).catch((e) => saveError?.(e));
  }, 300);
}

/** Restore defaults (keeps the remembered tab/device). */
export function resetPrefs() {
  updatePrefs({ ...DEFAULT_PREFS, startup: { ...DEFAULT_PREFS.startup, lastTab: current.startup.lastTab, lastDevice: current.startup.lastDevice } });
}

/** Subscribe a component to the preferences. */
export function usePrefs(): Preferences {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
}

/** Subscribe outside React; returns an unsubscribe function. */
export function subscribePrefs(cb: (p: Preferences) => void): () => void {
  const l = () => cb(current);
  listeners.add(l);
  return () => listeners.delete(l);
}

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** One-level-deep merge (the preferences are at most two levels deep). */
function merge(base: Preferences, patch: Partial<Preferences>): Preferences {
  const out: Record<string, unknown> = { ...base };
  for (const [k, v] of Object.entries(patch)) {
    if (v === undefined) continue;
    const b = out[k];
    out[k] = isObj(b) && isObj(v) ? { ...b, ...v } : v;
  }
  return out as Preferences;
}
