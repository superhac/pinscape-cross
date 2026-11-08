// Applies the Appearance preferences to the document:
//   <html data-theme="light|dark">   (absent = follow the OS)
//   <html data-font-size="small|normal|large">
// styles.css and the Monaco theme (editor/monaco.ts currentTheme) honor these.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Preferences } from "../api";

let lastTheme: string | null = null;

export function applyAppearance(p: Preferences) {
  const root = document.documentElement;
  if (p.theme === "system") delete root.dataset.theme;
  else root.dataset.theme = p.theme;
  root.dataset.fontSize = p.uiFontSize;
  if (lastTheme !== p.theme) {
    lastTheme = p.theme;
    // Also set the native window theme (title bar, and on most platforms the
    // webview's prefers-color-scheme, so views that follow the OS match too).
    getCurrentWindow()
      .setTheme(p.theme === "system" ? null : p.theme)
      .catch(() => {});
    window.dispatchEvent(new Event("pinscape-theme-change"));
  }
}

/** True if the app is currently shown dark (explicit preference or OS setting). */
export function isDarkTheme(): boolean {
  const t = document.documentElement.dataset.theme;
  if (t === "dark") return true;
  if (t === "light") return false;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}
