import { useEffect, useRef } from "react";
import { History, Col } from "./history";
import { axisColors, cssVar, useDarkMode } from "./theme";

interface Props {
  title: string;
  x: number;
  y: number;
  z: number;
  /** Trail source (the last ~0.5 s of samples). */
  history: History;
  cols: [Col, Col, Col];
  /** Magnification: 1 = full ±32768 scale. */
  zoom: number;
  /** Center-point axes to overlay (raw view with auto-centering). */
  center?: { x: number; y: number; z: number } | null;
  /** Z value shown at the middle of the Z bar (e.g. 1g for raw/filtered Z). */
  zMid?: number;
  help?: string;
}

const SIZE = 200;
const ZW = 26;
const GAP = 22;
const TRAIL = 24;

/** A 2-D crosshair view of X/Y plus a vertical Z bar, on one canvas. */
export default function AxisView({ title, x, y, z, history, cols, zoom, center, zMid = 0, help }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const dark = useDarkMode();

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = window.devicePixelRatio || 1;
    const W = SIZE + GAP + ZW;
    const H = SIZE;
    if (cv.width !== W * dpr) {
      cv.width = W * dpr;
      cv.height = H * dpr;
      cv.style.width = `${W}px`;
      cv.style.height = `${H}px`;
    }
    const g = cv.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const c = axisColors(dark);
    const ink = cssVar("--muted");
    const border = cssVar("--border");
    const panel2 = cssVar("--panel-2");
    const accent = cssVar("--accent");
    const danger = cssVar("--danger");
    const scale = (SIZE / 65536) * zoom;
    const px = (v: number) => SIZE / 2 + v * scale;
    const py = (v: number) => SIZE / 2 - v * scale;
    const clampXY = (v: number) => Math.max(0, Math.min(SIZE, v));

    g.clearRect(0, 0, W, H);
    // X/Y box
    g.fillStyle = panel2;
    g.fillRect(0, 0, SIZE, SIZE);
    g.strokeStyle = border;
    g.lineWidth = 1;
    g.strokeRect(0.5, 0.5, SIZE - 1, SIZE - 1);
    // grid rings at 1/4, 1/2 of the visible range
    g.beginPath();
    g.moveTo(SIZE / 2, 0);
    g.lineTo(SIZE / 2, SIZE);
    g.moveTo(0, SIZE / 2);
    g.lineTo(SIZE, SIZE / 2);
    g.stroke();
    g.setLineDash([2, 3]);
    for (const r of [SIZE / 4, SIZE / 2 - 1]) {
      g.beginPath();
      g.arc(SIZE / 2, SIZE / 2, r, 0, Math.PI * 2);
      g.stroke();
    }
    g.setLineDash([]);

    // center-point axes
    if (center) {
      g.strokeStyle = accent;
      g.globalAlpha = 0.6;
      g.setLineDash([4, 3]);
      g.beginPath();
      const cx = clampXY(px(center.x));
      const cy = clampXY(py(center.y));
      g.moveTo(cx, 0);
      g.lineTo(cx, SIZE);
      g.moveTo(0, cy);
      g.lineTo(SIZE, cy);
      g.stroke();
      g.setLineDash([]);
      g.globalAlpha = 1;
    }

    // trail
    const n = history.length;
    const xs = history.cols[cols[0]];
    const ys = history.cols[cols[1]];
    const start = Math.max(0, n - TRAIL);
    for (let i = start; i < n; i++) {
      const a = (i - start + 1) / (n - start + 1);
      g.fillStyle = accent;
      g.globalAlpha = a * 0.35;
      g.beginPath();
      g.arc(clampXY(px(xs[i])), clampXY(py(ys[i])), 2.5, 0, Math.PI * 2);
      g.fill();
    }
    g.globalAlpha = 1;

    // crosshair at the current reading
    const out = Math.abs(x * scale) > SIZE / 2 || Math.abs(y * scale) > SIZE / 2;
    const dx = clampXY(px(x));
    const dy = clampXY(py(y));
    g.strokeStyle = out ? danger : cssVar("--text");
    g.lineWidth = 1.5;
    g.beginPath();
    g.moveTo(dx - 9, dy);
    g.lineTo(dx + 9, dy);
    g.moveTo(dx, dy - 9);
    g.lineTo(dx, dy + 9);
    g.stroke();
    g.fillStyle = out ? danger : accent;
    g.beginPath();
    g.arc(dx, dy, 4, 0, Math.PI * 2);
    g.fill();

    // axis labels
    g.fillStyle = ink;
    g.font = "10px system-ui, sans-serif";
    g.fillText("+X", SIZE - 16, SIZE / 2 - 4);
    g.fillText("+Y", SIZE / 2 + 4, 11);

    // Z bar
    const zx = SIZE + GAP;
    g.fillStyle = panel2;
    g.fillRect(zx, 0, ZW, SIZE);
    g.strokeStyle = border;
    g.lineWidth = 1;
    g.strokeRect(zx + 0.5, 0.5, ZW - 1, SIZE - 1);
    g.beginPath();
    g.moveTo(zx, SIZE / 2);
    g.lineTo(zx + ZW, SIZE / 2);
    g.stroke();
    if (center) {
      g.strokeStyle = accent;
      g.setLineDash([4, 3]);
      g.beginPath();
      const cz = clampXY(py(center.z));
      g.moveTo(zx, cz);
      g.lineTo(zx + ZW, cz);
      g.stroke();
      g.setLineDash([]);
    }
    const zy = py(z - zMid);
    const zc = clampXY(zy);
    g.fillStyle = c.z;
    const top = Math.min(zc, SIZE / 2);
    g.fillRect(zx + 6, top, ZW - 12, Math.max(1, Math.abs(zc - SIZE / 2)));
    g.fillStyle = zy < 0 || zy > SIZE ? danger : cssVar("--text");
    g.fillRect(zx + 2, zc - 1.5, ZW - 4, 3);
    g.fillStyle = ink;
    g.fillText("Z", zx + ZW / 2 - 3, SIZE - 4);
  }, [x, y, z, zoom, center, zMid, dark, history, cols]);

  return (
    <div className="nudge-view">
      <div className="nudge-view-title" title={help}>
        {title}
      </div>
      <canvas ref={ref} />
      <div className="nudge-view-vals">
        <span>
          x <b>{x}</b>
        </span>
        <span>
          y <b>{y}</b>
        </span>
        <span>
          z <b>{z}</b>
        </span>
      </div>
    </div>
  );
}
