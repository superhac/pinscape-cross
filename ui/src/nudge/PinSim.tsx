import { MouseEvent, MutableRefObject, useEffect, useRef } from "react";
import { cssVar } from "./theme";

interface Props {
  /** Latest device velocity reading (INT16 units) and the scaling factor (units per mm/s). */
  input: MutableRefObject<{ vx: number; vy: number; scale: number }>;
  running: boolean;
}

// Playfield in mm (x right, y toward the back of the cabinet).
const W = 520;
const H = 1000;
const R = 13.5; // 27 mm ball
const SLOPE_G = 9806.65 * Math.sin((6.5 * Math.PI) / 180); // playfield incline
const RESTITUTION = 0.55;
const ROLL_DRAG = 0.12; // per second
const PX_H = 300;
const S = PX_H / H;
const PX_W = Math.round(W * S);

interface Ball {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

/**
 * A tiny "physics tester" in the spirit of the Windows tool's MicroPinSim:
 * a ball on an inclined playfield. The playfield moves with the cabinet, so
 * a change in the device's velocity reading pushes the ball the opposite way.
 * Click the playfield to drop the ball there.
 */
export default function PinSim({ input, running }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const ball = useRef<Ball>({ x: W / 2, y: H * 0.7, vx: 0, vy: 0 });
  const lastV = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = window.devicePixelRatio || 1;
    cv.width = PX_W * dpr;
    cv.height = PX_H * dpr;
    cv.style.width = `${PX_W}px`;
    cv.style.height = `${PX_H}px`;
    const g = cv.getContext("2d");
    if (!g) return;
    let raf = 0;
    let last = performance.now();

    const step = (dt: number) => {
      const b = ball.current;
      const { vx, vy, scale } = input.current;
      const k = scale > 0 ? scale : 1;
      const v = { x: vx / k, y: vy / k }; // cabinet velocity, mm/s
      const prev = lastV.current ?? v;
      lastV.current = v;
      // the playfield is the reference frame: cabinet motion moves the ball the other way
      b.vx -= v.x - prev.x;
      b.vy -= v.y - prev.y;
      b.vy -= SLOPE_G * dt;
      const drag = Math.max(0, 1 - ROLL_DRAG * dt);
      b.vx *= drag;
      b.vy *= drag;
      b.x += b.vx * dt;
      b.y += b.vy * dt;
      if (b.x < R) [b.x, b.vx] = [R, Math.abs(b.vx) * RESTITUTION];
      if (b.x > W - R) [b.x, b.vx] = [W - R, -Math.abs(b.vx) * RESTITUTION];
      if (b.y < R) [b.y, b.vy] = [R, Math.abs(b.vy) * RESTITUTION];
      if (b.y > H - R) [b.y, b.vy] = [H - R, -Math.abs(b.vy) * RESTITUTION];
    };

    const draw = () => {
      const b = ball.current;
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.fillStyle = cssVar("--panel-2");
      g.fillRect(0, 0, PX_W, PX_H);
      g.strokeStyle = cssVar("--border");
      g.lineWidth = 2;
      g.strokeRect(1, 1, PX_W - 2, PX_H - 2);
      // a few decorative inserts, so motion is easy to see
      g.strokeStyle = cssVar("--border");
      g.lineWidth = 1;
      for (const [ix, iy] of [
        [0.3, 0.75],
        [0.5, 0.8],
        [0.7, 0.75],
        [0.5, 0.55],
      ]) {
        g.beginPath();
        g.arc(ix * PX_W, (1 - iy) * PX_H, 12, 0, Math.PI * 2);
        g.stroke();
      }
      const bx = b.x * S;
      const by = PX_H - b.y * S;
      g.fillStyle = cssVar("--muted");
      g.beginPath();
      g.arc(bx, by, Math.max(4, R * S), 0, Math.PI * 2);
      g.fill();
      g.fillStyle = "rgba(255,255,255,0.6)";
      g.beginPath();
      g.arc(bx - 1.5, by - 1.5, 1.6, 0, Math.PI * 2);
      g.fill();
    };

    const frame = (now: number) => {
      let dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      if (running) {
        // sub-step for stable bounces
        const n = Math.ceil(dt / 0.004);
        dt /= n;
        for (let i = 0; i < n; i++) step(dt);
      }
      draw();
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [input, running]);

  const drop = (e: MouseEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    ball.current = {
      x: Math.min(W - R, Math.max(R, (e.clientX - r.left) / S)),
      y: Math.min(H - R, Math.max(R, (PX_H - (e.clientY - r.top)) / S)),
      vx: 0,
      vy: 0,
    };
  };

  const launch = () => {
    ball.current = { x: W - 40, y: R + 5, vx: -250, vy: 2600 };
  };

  return (
    <div className="nudge-sim">
      <canvas ref={ref} onClick={drop} title="Click to place the ball" />
      <button onClick={launch}>Launch ball</button>
    </div>
  );
}
