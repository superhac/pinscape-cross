// Horizontal position gauge: a filled bar from `origin` to `value`, with
// labeled markers (calibration points) and an optional window box (jitter filter).

export interface Marker {
  at: number;
  label: string;
  cls?: string;
  title?: string;
}

interface Props {
  value: number;
  min: number;
  max: number;
  origin?: number;
  markers?: Marker[];
  /** Highlighted [lo, hi] range drawn over the bar. */
  window?: [number, number] | null;
  /** Thin tick for a secondary value (e.g. the pre-filter reading). */
  tick?: number | null;
  kind?: "raw" | "z" | "z0" | "prox";
  title?: string;
}

export default function Gauge({ value, min, max, origin = min, markers = [], window, tick, kind = "raw", title }: Props) {
  const span = max - min || 1;
  const pct = (v: number) => Math.max(0, Math.min(100, ((v - min) / span) * 100));
  const a = pct(origin);
  const b = pct(value);
  return (
    <div className={`pl-gauge ${markers.length ? "with-markers" : ""}`} title={title}>
      <div className="pl-track">
        <div className={`pl-fill ${kind}`} style={{ left: `${Math.min(a, b)}%`, width: `${Math.abs(b - a)}%` }} />
        {origin !== min && <div className="pl-origin" style={{ left: `${a}%` }} />}
        {window && window[1] >= window[0] && (
          <div
            className="pl-window"
            style={{ left: `${pct(window[0])}%`, width: `${Math.max(0.4, pct(window[1]) - pct(window[0]))}%` }}
            title={`Jitter window ${window[0]}–${window[1]}`}
          />
        )}
        {tick != null && <div className="pl-tick" style={{ left: `${pct(tick)}%` }} title={`Unfiltered: ${tick}`} />}
      </div>
      {markers.map((m, i) => (
        <div key={i} className={`pl-marker ${m.cls ?? ""}`} style={{ left: `${pct(m.at)}%` }} title={m.title}>
          <span className="pl-arrow">▲</span>
          <span className="pl-mlabel">{m.label}</span>
        </div>
      ))}
    </div>
  );
}
