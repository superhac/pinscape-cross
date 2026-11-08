import { fmtVersion } from "../api";
import { PHASE_LABEL } from "../lib/api-firmware";
import { dismissInstall, useInstallState } from "./store";

/** Modal shown for the whole firmware installation, wherever it was started. */
export default function InstallProgress() {
  const s = useInstallState();
  if (!s.open) return null;
  const p = s.progress;
  const pct = p && p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : null;
  return (
    <div className="modal-backdrop">
      <div className="modal wide firmware-progress" role="dialog" aria-modal="true">
        <h2>{s.title}</h2>
        {s.running && (
          <>
            <p>{p ? (p.message ?? PHASE_LABEL[p.phase]) : "Starting…"}</p>
            <div className={"progress" + (pct == null ? " indeterminate" : "")}>
              <div style={{ width: pct == null ? undefined : `${pct}%` }} />
            </div>
            <p className="muted small">
              {pct != null ? `${pct}% · ${p!.done.toLocaleString()} of ${p!.total.toLocaleString()} bytes` : " "}
              <br />
              Don't unplug the Pico or close the app until this finishes.
            </p>
          </>
        )}
        {s.result && (
          <>
            <p className="ok">
              {s.result.version
                ? `Done. The device is running Pinscape Pico v${fmtVersion(s.result.version)}.`
                : "The firmware was written."}
            </p>
            <p className="muted small">
              Method: {s.result.method === "picoboot" ? "USB (PICOBOOT)" : "copied to the boot drive"}
            </p>
            {s.result.notes.map((n) => (
              <p key={n} className="small">
                {n}
              </p>
            ))}
          </>
        )}
        {s.error && (
          <>
            <p className="error">{s.error}</p>
            <p className="muted small">
              If the Pico is left in boot loader mode it shows up in the sidebar; you can install again from there, or unplug and
              replug it to restart the previous firmware (if it wasn't erased yet).
            </p>
          </>
        )}
        <div className="modal-actions">
          <button className="primary" disabled={s.running} onClick={dismissInstall}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
