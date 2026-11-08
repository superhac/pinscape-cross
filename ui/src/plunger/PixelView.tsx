// Linear image sensor snapshot (TCD1103 / TSL1410R / TSL1412S), drawn on a
// canvas in false color (red -> orange -> yellow -> white with brightness),
// as in the Windows config tool, with the detected edge marked.

import { useEffect, useRef } from "react";
import { SENSOR } from "../lib/api-plunger";

interface Props {
  pixels: number[];
  sensorType: number;
  /** Detected position, native units (= pixel index after orientation). */
  rawPos: number;
  nativeScale: number;
  /** Reverse orientation: the image is drawn mirrored so the edge marker lines up. */
  reverse: boolean;
}

const HEIGHT = 96;

/** Pixel value -> brightness 0..765 (3x the 8-bit range, for the false-color ramp). */
function brightnessFn(pixels: number[], sensorType: number): (p: number) => number {
  if (sensorType === SENSOR.TCD1103 && pixels.length > 28) {
    // Negative image with about 110 units of dynamic range; pixels 16..28 are
    // shielded dark-reference pixels.
    let sum = 0;
    for (let i = 16; i <= 28; i++) sum += pixels[i];
    const dark = Math.max(Math.round(sum / 13), 110);
    const bright = dark - 110;
    const norm = 1 / (dark - bright);
    return (p) => (p < bright ? 765 : p > dark ? 0 : Math.round(Math.pow((dark - p) * norm, 0.6) * 765));
  }
  return (p) => p * 3;
}

function falseColor(g: number): [number, number, number] {
  if (g < 255) return [g, 0, 0];
  if (g < 510) return [255, g - 255, 0];
  return [255, 255, Math.min(255, g - 510)];
}

export default function PixelView({ pixels, sensorType, rawPos, nativeScale, reverse }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const strip = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const cv = canvas.current;
    if (!cv) return;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(cv.clientWidth * dpr));
      const h = Math.round(HEIGHT * dpr);
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
      }
      const ctx = cv.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, w, h);
      const n = pixels.length;
      if (n === 0) return;

      // build a 1-pixel-high image of the sensor and stretch it across the canvas
      if (!strip.current) strip.current = document.createElement("canvas");
      const sc = strip.current;
      if (sc.width !== n) sc.width = n;
      sc.height = 1;
      const sctx = sc.getContext("2d");
      if (!sctx) return;
      const img = sctx.createImageData(n, 1);
      const bright = brightnessFn(pixels, sensorType);
      const levels: number[] = new Array(n);
      for (let i = 0; i < n; i++) {
        const src = reverse ? n - 1 - i : i;
        const g = bright(pixels[src]);
        levels[i] = g;
        const [r, gg, b] = falseColor(g);
        img.data[i * 4] = r;
        img.data[i * 4 + 1] = gg;
        img.data[i * 4 + 2] = b;
        img.data[i * 4 + 3] = 255;
      }
      sctx.putImageData(img, 0, 0);
      const barH = Math.round(h * 0.62);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(sc, 0, 0, n, 1, 0, 0, w, barH);

      // brightness profile below the strip
      const top = barH + 4 * dpr;
      const ph = h - top - 2 * dpr;
      ctx.fillStyle = "rgba(128,128,128,0.12)";
      ctx.fillRect(0, top, w, ph);
      ctx.strokeStyle = "rgba(220,120,40,0.9)";
      ctx.lineWidth = dpr;
      ctx.beginPath();
      const step = Math.max(1, Math.floor(n / w));
      for (let i = 0; i < n; i += step) {
        const x = (i / (n - 1)) * w;
        const y = top + ph - (Math.min(765, levels[i]) / 765) * ph;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();

      // detected edge
      const scale = nativeScale > 0 ? nativeScale : n;
      const x = Math.max(0, Math.min(w, (rawPos / scale) * w));
      ctx.strokeStyle = "#22d3ee";
      ctx.lineWidth = 2 * dpr;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
      ctx.fillStyle = "#22d3ee";
      ctx.font = `${11 * dpr}px system-ui, sans-serif`;
      const label = `edge ${rawPos}`;
      const tw = ctx.measureText(label).width;
      ctx.fillText(label, x + 4 * dpr + tw > w ? x - tw - 4 * dpr : x + 4 * dpr, 12 * dpr);
    };
    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(cv);
    return () => ro.disconnect();
  }, [pixels, sensorType, rawPos, nativeScale, reverse]);

  return <canvas ref={canvas} className="pl-pixels" style={{ height: HEIGHT }} aria-label="Image sensor pixels" />;
}
