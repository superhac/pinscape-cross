// Small sidebar notice shown when a newer release is available.
// Mounted once in App.tsx (sidebar footer). Checks once shortly after startup;
// the About dialog can trigger more checks with checkForUpdates() (updater.ts).
// Errors and offline checks are silent; nothing is installed without a click.
import { CSSProperties, useEffect, useState } from "react";
import {
  availableUpdate,
  checkForUpdates,
  installUpdate,
  InstallProgress,
  onUpdateAvailable,
  openReleasesPage,
  UpdateInfo,
} from "./updater";

const STARTUP_DELAY_MS = 4000;

const box: CSSProperties = {
  margin: "12px",
  padding: "10px 12px",
  border: "1px solid var(--border)",
  borderLeft: "3px solid var(--accent)",
  borderRadius: 6,
  background: "var(--bg)",
  fontSize: 12,
  lineHeight: 1.4,
};
const notesStyle: CSSProperties = {
  maxHeight: 160,
  overflow: "auto",
  whiteSpace: "pre-wrap",
  margin: "6px 0 0",
  padding: 0,
  fontFamily: "inherit",
};
const row: CSSProperties = { display: "flex", gap: 6, marginTop: 8, flexWrap: "wrap" };

function mb(n: number) {
  return (n / 1048576).toFixed(1);
}

export default function UpdateNotice() {
  const [update, setUpdate] = useState<UpdateInfo | null>(availableUpdate());
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [showNotes, setShowNotes] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const un = onUpdateAvailable((u) => {
      setUpdate(u);
      // A manual check (About dialog) re-shows a notice dismissed with "Later".
      setDismissed(null);
    });
    const t = window.setTimeout(() => {
      checkForUpdates().catch(() => {});
    }, STARTUP_DELAY_MS);
    return () => {
      un();
      window.clearTimeout(t);
    };
  }, []);

  if (!update || dismissed === update.version) return null;
  const installing = progress !== null && !error;

  const install = async () => {
    setError(null);
    setProgress({ downloaded: 0 });
    try {
      await installUpdate(setProgress);
    } catch (e) {
      setError(String(e));
      setProgress(null);
    }
  };

  let progressText = "";
  if (progress) {
    progressText =
      progress.total && progress.total > 0
        ? `Downloading… ${Math.min(100, Math.round((progress.downloaded / progress.total) * 100))}%`
        : `Downloading… ${mb(progress.downloaded)} MB`;
    if (progress.total && progress.downloaded >= progress.total) progressText = "Installing…";
  }

  return (
    <div style={box} role="status" aria-live="polite">
      <strong>Update available: {update.version}</strong>
      <div className="muted">You have {update.currentVersion}.</div>
      {update.notes && (
        <>
          <button className="link" onClick={() => setShowNotes(!showNotes)} aria-expanded={showNotes}>
            {showNotes ? "Hide release notes" : "Release notes"}
          </button>
          {showNotes && <pre style={notesStyle}>{update.notes}</pre>}
        </>
      )}
      {progress && !error && (
        <div style={{ marginTop: 8 }}>
          <progress
            style={{ width: "100%" }}
            value={progress.total ? progress.downloaded : undefined}
            max={progress.total || undefined}
          />
          <div className="muted">{progressText}</div>
        </div>
      )}
      {error && (
        <div style={{ color: "var(--danger)", marginTop: 6 }}>
          Update failed: {error}{" "}
          <button className="link" onClick={() => openReleasesPage().catch(() => {})}>
            Download manually
          </button>
        </div>
      )}
      {!installing && (
        <div style={row}>
          {update.canInstall ? (
            <button className="primary" onClick={install}>
              Install and restart
            </button>
          ) : (
            <button className="primary" onClick={() => openReleasesPage().catch(() => {})}>
              Download…
            </button>
          )}
          <button onClick={() => setDismissed(update.version)}>Later</button>
        </div>
      )}
    </div>
  );
}
