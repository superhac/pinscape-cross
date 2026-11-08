import { useState } from "react";
import { Recording } from "../lib/api-export";
import "./record.css";

const DURATIONS = [5, 10, 30, 60, 120];

/** "Record N s to CSV" controls for a {@link Recording} (used by the Nudge and Plunger tabs). */
export default function RecordControls<T>({ rec, disabled, what }: { rec: Recording<T>; disabled?: boolean; what: string }) {
  const [secs, setSecs] = useState(10);
  return (
    <span className="record-controls">
      {rec.active ? (
        <>
          <span className="rec-dot" aria-hidden />
          <span className="small">
            Recording {Math.floor(rec.elapsed)} / {rec.seconds} s · {rec.count.toLocaleString()} samples
          </span>
          <button onClick={rec.stop} title="Stop now and save what was recorded">
            Stop
          </button>
        </>
      ) : (
        <>
          <button onClick={() => rec.start(secs)} disabled={disabled} title={`Record ${what} for the chosen time, then save them as a CSV file`}>
            Record CSV
          </button>
          <select value={secs} onChange={(e) => setSecs(Number(e.target.value))} aria-label="Recording length">
            {DURATIONS.map((d) => (
              <option key={d} value={d}>
                {d} s
              </option>
            ))}
          </select>
        </>
      )}
    </span>
  );
}
