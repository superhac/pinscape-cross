// Saving captured data (CSV / text) through the save dialog plus the
// write_text_file command, and a small time-limited sample recorder.
import { useCallback, useEffect, useRef, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../api";

export const CSV_FILTER = [{ name: "CSV (comma-separated values)", extensions: ["csv"] }];

/** One CSV field: quoted when it contains a comma, quote or line break. */
export function csvField(v: unknown): string {
  const s = v == null ? "" : String(v);
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** Header plus rows as CSV text (LF line ends, trailing newline). */
export function toCsv(header: string[], rows: unknown[][]): string {
  return [header, ...rows].map((r) => r.map(csvField).join(",")).join("\n") + "\n";
}

/** "<base>-YYYYMMDD-HHMMSS.<ext>", with unsafe characters replaced. */
export function stampedName(base: string, ext: string): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
  return `${base.replace(/[^A-Za-z0-9._-]+/g, "-")}-${stamp}.${ext}`;
}

/** Ask where to save, then write `text`. Resolves to the path, or null if cancelled. Throws on write errors. */
export async function saveTextAs(
  text: string,
  opts: { title: string; defaultName: string; filters?: { name: string; extensions: string[] }[] },
): Promise<string | null> {
  const path = await saveDialog({ title: opts.title, defaultPath: opts.defaultName, filters: opts.filters ?? CSV_FILTER });
  if (!path) return null;
  await api.writeTextFile(path, text);
  return path;
}

export interface Recording<T> {
  /** Recording in progress. */
  active: boolean;
  /** Samples recorded so far (in the current or last recording). */
  count: number;
  /** Seconds elapsed in the current recording. */
  elapsed: number;
  /** Requested length in seconds. */
  seconds: number;
  start: (seconds: number) => void;
  /** Stop early; the samples so far are handed to onDone. */
  stop: () => void;
  /** Feed a sample (ignored unless recording). */
  push: (s: T) => void;
}

/**
 * Collects samples for a fixed time; when the time is up (or stop() is
 * called) hands them to `onDone` (e.g. to save as CSV). Samples are kept in
 * a ref, so pushing doesn't re-render on every sample.
 */
export function useRecorder<T>(onDone: (samples: T[]) => void): Recording<T> {
  const buf = useRef<T[]>([]);
  const t0 = useRef(0);
  const limit = useRef(0);
  const activeRef = useRef(false);
  const [state, setState] = useState({ active: false, count: 0, elapsed: 0, seconds: 0 });
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  const lastUi = useRef(0);

  const finish = useCallback(() => {
    if (!activeRef.current) return;
    activeRef.current = false;
    const samples = buf.current;
    buf.current = [];
    setState((s) => ({ ...s, active: false, count: samples.length }));
    doneRef.current(samples);
  }, []);

  const start = useCallback((seconds: number) => {
    buf.current = [];
    t0.current = performance.now();
    limit.current = seconds * 1000;
    activeRef.current = true;
    setState({ active: true, count: 0, elapsed: 0, seconds });
  }, []);

  const push = useCallback(
    (s: T) => {
      if (!activeRef.current) return;
      buf.current.push(s);
      const now = performance.now();
      if (now - t0.current >= limit.current) {
        finish();
      } else if (now - lastUi.current > 250) {
        lastUi.current = now;
        setState((st) => ({ ...st, count: buf.current.length, elapsed: (now - t0.current) / 1000 }));
      }
    },
    [finish],
  );

  // End on time even if samples stop arriving (tab hidden, device gone).
  useEffect(() => {
    if (!state.active) return;
    const t = setInterval(() => {
      const el = performance.now() - t0.current;
      if (el >= limit.current) finish();
      else setState((st) => ({ ...st, count: buf.current.length, elapsed: el / 1000 }));
    }, 500);
    return () => clearInterval(t);
  }, [state.active, finish]);

  return { ...state, start, stop: finish, push };
}
