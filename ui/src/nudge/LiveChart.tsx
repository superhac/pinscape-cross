import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { Col, History } from "./history";
import { axisColors, cssVar, useDarkMode } from "./theme";

interface Props {
  history: History;
  /** Changes whenever new samples arrive. */
  version: number;
  /** History columns for the X, Y, Z series. */
  cols: [Col, Col, Col];
  /** Series hidden initially (toggle by clicking the legend). */
  hideZ?: boolean;
  height?: number;
}

/** Rolling X/Y/Z time-series chart (uPlot), redrawn when `version` changes. */
export default function LiveChart({ history, version, cols, hideZ, height = 170 }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const dark = useDarkMode();

  const data = (): uPlot.AlignedData => [history.t, ...cols.map((c) => history.cols[c])];

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const c = axisColors(dark);
    const ink = cssVar("--muted") || "#6b7280";
    const grid = cssVar("--border") || "#dde1e6";
    const axis: uPlot.Axis = {
      stroke: ink,
      grid: { stroke: grid, width: 1 },
      ticks: { stroke: grid, width: 1 },
      font: "11px system-ui, sans-serif",
    };
    const series = (label: string, stroke: string, show = true): uPlot.Series => ({
      label,
      stroke,
      width: 2,
      show,
      points: { show: false },
      value: (_u, v) => (v == null ? "–" : String(Math.round(v))),
    });
    const opts: uPlot.Options = {
      width: Math.max(200, el.clientWidth),
      height,
      scales: { x: { time: false } },
      axes: [
        // uPlot may place ticks at half seconds; show a decimal only when needed
        { ...axis, values: (_u, ticks) => ticks.map((t) => `${Number.isInteger(t) ? t : +t.toFixed(1)}s`) },
        { ...axis, size: 56 },
      ],
      series: [
        { label: "t", value: (_u, v) => (v == null ? "–" : `${v.toFixed(2)} s`) },
        series("X", c.x),
        series("Y", c.y),
        series("Z", c.z, !hideZ),
      ],
      cursor: { drag: { x: false, y: false } },
      legend: { live: true },
    };
    const u = new uPlot(opts, data(), el);
    plot.current = u;
    const ro = new ResizeObserver(() => u.setSize({ width: Math.max(200, el.clientWidth), height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dark, height, cols.join(","), hideZ, history]);

  useEffect(() => {
    plot.current?.setData(data());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version]);

  return <div className="nudge-chart" ref={host} />;
}
