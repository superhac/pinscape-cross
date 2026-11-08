import { useEffect, useState } from "react";
import { api, BackupInfo } from "../api";

interface Props {
  hwid: string;
  safeMode: boolean;
  onPick: (b: BackupInfo) => void;
  onClose: () => void;
}

/** Lists the automatic backups made each time a config was saved to this device. */
export default function BackupPicker({ hwid, safeMode, onPick, onClose }: Props) {
  const [items, setItems] = useState<BackupInfo[] | null>(null);
  const [folder, setFolder] = useState("");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    api.listBackups(hwid, safeMode).then(setItems).catch((e) => setErr(String(e)));
    api.backupFolder().then(setFolder).catch(() => {});
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hwid, safeMode]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
        <h2>{safeMode ? "Safe Mode config backups" : "Config backups"}</h2>
        <p className="muted">
          A copy is kept every time you save to this device, for 30 days. Loading one puts it in the editor; it isn't sent to the
          device until you save.
        </p>
        {err && <p className="error">{err}</p>}
        {items && items.length === 0 && <p className="muted">No backups yet for this device.</p>}
        {items && items.length > 0 && (
          <ul className="template-list backup-list">
            {items.map((b) => (
              <li key={b.file}>
                <button onClick={() => onPick(b)}>
                  <strong>{b.savedAt}</strong>
                  <span className="muted small">{b.size.toLocaleString()} bytes</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {folder && <p className="muted small">Folder: {folder}</p>}
        <div className="modal-actions">
          <button onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}
