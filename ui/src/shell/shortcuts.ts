// Global keyboard shortcuts. Keys the focused Monaco editor handles itself
// (it calls preventDefault) are left alone, so the editor keeps its bindings.
import { useEffect, useRef } from "react";

export interface ShortcutHandlers {
  /** Ctrl/Cmd+1…9: tab by position (0-based). */
  tabIndex: (i: number) => void;
  /** Ctrl/Cmd+PgUp / PgDn: -1 previous, +1 next. */
  tabStep: (d: -1 | 1) => void;
  preferences: () => void;
  help: () => void;
  reboot: () => void;
}

const inEditor = () => !!(document.activeElement as HTMLElement | null)?.closest?.(".monaco-editor");
const modalOpen = () => !!document.querySelector(".modal-backdrop");

export function useGlobalShortcuts(mac: boolean, handlers: ShortcutHandlers) {
  const h = useRef(handlers);
  h.current = handlers;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.isComposing) return;
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      const run = (f: () => void) => {
        e.preventDefault();
        f();
      };
      // Preferences works anywhere except over another dialog.
      if (mod && !e.shiftKey && !e.altKey && e.key === ",") {
        if (!modalOpen()) run(h.current.preferences);
        return;
      }
      if (modalOpen()) return;
      if (mod && !e.shiftKey && !e.altKey && /^Digit[1-9]$/.test(e.code)) {
        return run(() => h.current.tabIndex(Number(e.code.slice(5)) - 1));
      }
      if (mod && !e.shiftKey && !e.altKey && (e.key === "PageUp" || e.key === "PageDown")) {
        return run(() => h.current.tabStep(e.key === "PageUp" ? -1 : 1));
      }
      if (mod && e.shiftKey && !e.altKey && e.key.toLowerCase() === "r") {
        return run(h.current.reboot);
      }
      if (e.key === "F1" && !mod && !e.shiftKey && !e.altKey && !inEditor()) {
        return run(h.current.help);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mac]);
}

/** For the shortcut list shown in Help and About. */
export function shortcutList(mac: boolean): { keys: string; what: string }[] {
  const m = mac ? "Cmd" : "Ctrl";
  return [
    { keys: `${m}+1 … ${m}+9`, what: "Switch to tab 1–9 (Overview, Config, Safe-Mode Config, Log, Buttons, Outputs, Nudge, Plunger, IR)" },
    { keys: `${m}+PgUp / ${m}+PgDn`, what: "Previous / next tab" },
    { keys: "← → Home End", what: "Move between tabs when the tab bar has focus" },
    { keys: `${m}+,`, what: "Preferences" },
    { keys: "F1", what: "Help tab (in the config editor: reference for the property under the cursor)" },
    { keys: `${m}+Shift+R`, what: "Reboot the selected device (asks first)" },
    { keys: `${m}+S`, what: "Config editor: save to device" },
    { keys: `${m}+Shift+P`, what: "Config editor: command palette" },
    { keys: "Esc", what: "Close a dialog" },
  ];
}

/** Optional Emacs-style keys in the config editor (Preferences › Editor). */
export function emacsKeyList(): { keys: string; what: string }[] {
  return [
    { keys: "Ctrl+A / Ctrl+E", what: "Start / end of line" },
    { keys: "Ctrl+F / Ctrl+B", what: "Forward / back one character" },
    { keys: "Ctrl+N / Ctrl+P", what: "Next / previous line" },
    { keys: "Ctrl+D", what: "Delete the character after the cursor" },
    { keys: "Ctrl+K", what: "Kill to end of line (repeated kills are collected)" },
    { keys: "Ctrl+Y", what: "Yank (paste) the killed text" },
  ];
}

export function emacsNote(mac: boolean): string {
  return mac
    ? "The Emacs keys use the Control key; Cmd shortcuts are unchanged."
    : "With Emacs keys on, these replace the editor's usual Ctrl+A (select all), Ctrl+F (find), Ctrl+Y (redo), Ctrl+D, Ctrl+P, Ctrl+N, Ctrl+B and Ctrl+K chords. Find, Replace (Ctrl+H) and everything else stay reachable from the command palette (Ctrl+Shift+P).";
}
