import { NudgeStatus } from "../lib/api-nudge";

/** Status fields kept in the rolling history. */
export const COLS = [
  "xRaw",
  "yRaw",
  "zRaw",
  "xFiltered",
  "yFiltered",
  "zFiltered",
  "vx",
  "vy",
  "vz",
] as const;
export type Col = (typeof COLS)[number];

/** Rolling time series of status samples (device time, seconds). */
export class History {
  t: number[] = [];
  cols: Record<Col, number[]> = Object.fromEntries(COLS.map((c) => [c, [] as number[]])) as Record<Col, number[]>;
  private lastTs = 0;
  private t0 = 0;

  constructor(public windowSec = 10) {}

  clear() {
    this.t.length = 0;
    for (const c of COLS) this.cols[c].length = 0;
    this.lastTs = 0;
  }

  /** Add a sample; returns false if it isn't a new reading. */
  push(s: NudgeStatus): boolean {
    if (!s.timestamp || s.timestamp === this.lastTs) return false;
    if (s.timestamp < this.lastTs || this.t.length === 0) {
      // first sample, or the device restarted
      this.clear();
      this.t0 = s.timestamp;
    }
    this.lastTs = s.timestamp;
    const t = (s.timestamp - this.t0) / 1e6;
    this.t.push(t);
    for (const c of COLS) this.cols[c].push(s[c]);
    let drop = 0;
    while (drop < this.t.length && this.t[drop] < t - this.windowSec) drop++;
    if (drop > 0) {
      this.t.splice(0, drop);
      for (const c of COLS) this.cols[c].splice(0, drop);
    }
    return true;
  }

  get length() {
    return this.t.length;
  }
}
