import { useEffect, useState } from "react";
import { api, Template } from "../api";

interface Props {
  safeMode: boolean;
  onPick: (t: Template) => void;
  onClose: () => void;
}

/** Modal list of the built-in config templates. */
export default function TemplatePicker({ safeMode, onPick, onClose }: Props) {
  const [templates, setTemplates] = useState<Template[]>([]);
  useEffect(() => {
    api.listTemplates().then((t) =>
      // Show templates for this slot first.
      setTemplates([...t].sort((a, b) => Number(b.safeMode === safeMode) - Number(a.safeMode === safeMode))),
    );
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [safeMode, onClose]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
        <h2>New from template</h2>
        <p className="muted">
          Templates come from the Pinscape Pico project. Most settings are commented-out examples you can enable and edit.
        </p>
        <ul className="template-list">
          {templates.map((t) => (
            <li key={t.id}>
              <button onClick={() => onPick(t)}>
                <strong>
                  {t.name}
                  {t.safeMode !== safeMode && (
                    <span className="badge">{t.safeMode ? "for Safe Mode" : "for main config"}</span>
                  )}
                </strong>
                <span className="muted small">{t.description}</span>
              </button>
            </li>
          ))}
        </ul>
        <div className="modal-actions">
          <button onClick={onClose}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
