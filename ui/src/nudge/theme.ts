import { useEffect, useState } from "react";

/** Axis series colors (categorical slots 1-3: blue, orange, aqua), light and dark steps. */
const LIGHT = { x: "#2a78d6", y: "#eb6834", z: "#1baf7a" };
const DARK = { x: "#3987e5", y: "#d95926", z: "#199e70" };

export type AxisColors = typeof LIGHT;

const query = () => window.matchMedia?.("(prefers-color-scheme: dark)");

export function isDark(): boolean {
  return query()?.matches ?? false;
}

export function axisColors(dark = isDark()): AxisColors {
  return dark ? DARK : LIGHT;
}

/** A CSS custom property from :root (e.g. "--muted"). */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Re-renders when the OS color scheme changes; returns true for dark. */
export function useDarkMode(): boolean {
  const [dark, setDark] = useState(isDark);
  useEffect(() => {
    const q = query();
    if (!q) return;
    const on = () => setDark(q.matches);
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);
  return dark;
}
