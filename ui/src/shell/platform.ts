// Host platform (from the Rust `platform` command), for Cmd-vs-Ctrl shortcuts.
import { useSyncExternalStore } from "react";
import { api } from "../api";

let mac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);
const listeners = new Set<() => void>();

api
  .platform()
  .then((p) => {
    if (mac !== (p === "macos")) {
      mac = p === "macos";
      listeners.forEach((l) => l());
    }
  })
  .catch(() => {});

export const isMac = () => mac;

export function useIsMac(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => mac,
  );
}

/** Label of the primary modifier: "Cmd" on macOS, "Ctrl" elsewhere. */
export const modName = (isMacOs: boolean) => (isMacOs ? "Cmd" : "Ctrl");
