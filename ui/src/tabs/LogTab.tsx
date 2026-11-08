import { useEffect, useRef, useState } from "react";
import { api, DeviceSnapshot } from "../api";
import { AnsiState, parseAnsi, Span } from "../lib/ansi";
import { useInterval } from "../lib/useDevices";

interface Props {
  dev: DeviceSnapshot;
  visible: boolean;
}

const MAX_LINES = 5000;

// Polls the device's in-memory log. Mounted for the whole session (hidden when
// not visible) so messages keep accumulating while other tabs are open.
export default function LogTab({ dev, visible }: Props) {
  const [lines, setLines] = useState<Span[][]>([]);
  const [filter, setFilter] = useState("");
  const [paused, setPaused] = useState(false);
  const [follow, setFollow] = useState(true);
  const ansi = useRef<AnsiState>({});
  const partial = useRef<Span[]>([]);
  const box = useRef<HTMLDivElement>(null);

  useInterval(
    () => {
      api
        .queryLog(dev.key)
        .then((text) => {
          if (!text) return;
          const newLines: Span[][] = [];
          let cur = partial.current;
          for (const span of parseAnsi(text.replace(/\r/g, ""), ansi.current)) {
            const parts = span.text.split("\n");
            parts.forEach((p, i) => {
              if (i > 0) {
                newLines.push(cur);
                cur = [];
              }
              if (p) cur.push({ ...span, text: p });
            });
          }
          partial.current = cur;
          if (newLines.length) setLines((old) => [...old, ...newLines].slice(-MAX_LINES));
        })
        .catch(() => {});
    },
    750,
    !paused,
  );

  useEffect(() => {
    if (follow && visible && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [lines, follow, visible]);

  const f = filter.toLowerCase();
  const shown = f ? lines.filter((l) => l.map((s) => s.text).join("").toLowerCase().includes(f)) : lines;

  return (
    <div className="log" style={{ display: visible ? "flex" : "none" }}>
      <div className="toolbar">
        <input placeholder="Filter…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <label className="inline">
          <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} /> auto-scroll
        </label>
        <button onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</button>
        <button onClick={() => setLines([])}>Clear</button>
        <button onClick={() => navigator.clipboard?.writeText(lines.map((l) => l.map((s) => s.text).join("")).join("\n"))}>
          Copy all
        </button>
        <span className="spacer" />
        <span className="muted small">{lines.length.toLocaleString()} lines</span>
      </div>
      <div className="logview" ref={box}>
        {shown.length === 0 && <p className="muted">No log messages yet.</p>}
        {shown.map((l, i) => (
          <div key={i} className="logline">
            {l.map((s, j) => (
              <span key={j} className={(s.fg ?? "") + (s.bold ? " b" : "")}>
                {s.text}
              </span>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
