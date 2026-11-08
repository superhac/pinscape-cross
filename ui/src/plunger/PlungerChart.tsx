// Rolling chart of the plunger position (Z0, Z) and speed, using uPlot.

import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

interface Props {
  /** [x seconds (<= 0, relative to now), z0, z, speed]. */
  data: uPlot.AlignedData;
  /** Visible history, seconds. */
  seconds: number;
}

const HEIGHT = 220;

function cssVar(name: string, fallback: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

export default function PlungerChart({ data, seconds }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const muted = cssVar("--muted", "#6b7280");
    const grid = cssVar("--border", "#dde1e6");
    const axis = (scale: string, side: 1 | 3, label: string, showGrid: boolean): uPlot.Axis => ({
      scale,
      side,
      label,
      stroke: muted,
      size: 56,
      labelSize: 18,
      grid: { show: showGrid, stroke: grid, width: 1 },
      ticks: { stroke: grid, width: 1 },
    });
    const opts: uPlot.Options = {
      width: Math.max(200, el.clientWidth),
      height: HEIGHT,
      scales: {
        x: { time: false, range: () => [-seconds, 0] },
        z: { range: (_u, min, max) => [Math.min(-8000, min), Math.max(33000, max)] },
        s: {
          range: (_u, min, max) => {
            const m = Math.max(5000, Math.abs(min), Math.abs(max)) * 1.05;
            return [-m, m];
          },
        },
      },
      axes: [
        { stroke: muted, grid: { stroke: grid, width: 1 }, ticks: { stroke: grid, width: 1 }, values: (_u, v) => v.map((s) => `${s}s`) },
        axis("z", 3, "Z axis", true),
        axis("s", 1, "Speed (Z/cs)", false),
      ],
      series: [
        {},
        { label: "Z0 (instantaneous)", scale: "z", stroke: cssVar("--accent", "#2563eb"), width: 1.5 },
        { label: "Z (reported)", scale: "z", stroke: cssVar("--ok", "#16a34a"), width: 1.5 },
        { label: "Speed", scale: "s", stroke: cssVar("--warn", "#d97706"), width: 1 },
      ],
      cursor: { drag: { x: false, y: false }, points: { show: false } },
      legend: { live: false },
    };
    const u = new uPlot(opts, data, el);
    plot.current = u;
    const ro = new ResizeObserver(() => u.setSize({ width: Math.max(200, el.clientWidth), height: HEIGHT }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
    // the plot is created once; data updates go through setData below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seconds]);

  useEffect(() => {
    plot.current?.setData(data);
  }, [data]);

  return <div ref={host} className="pl-chart" />;
}
