import { ReactNode, useEffect, useId, useState } from "react";
import { api } from "../api";
import Modal from "../shell/Modal";
import { emacsNote } from "../shell/shortcuts";
import { useIsMac } from "../shell/platform";
import { resetPrefs, updatePrefs, usePrefs } from "./store";

function Row({ label, children, hint }: { label: string; children: (id: string) => ReactNode; hint?: string }) {
  const id = useId();
  return (
    <div className="pref-row">
      <label htmlFor={id}>{label}</label>
      <div>
        {children(id)}
        {hint && <div className="muted small">{hint}</div>}
      </div>
    </div>
  );
}

function Check({ label, checked, onChange, hint }: { label: string; checked: boolean; onChange: (v: boolean) => void; hint?: string }) {
  return (
    <div className="pref-check">
      <label className="inline">
        <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} /> {label}
      </label>
      {hint && <div className="muted small">{hint}</div>}
    </div>
  );
}

/** Number input that commits valid values in [min, max]. */
function NumberInput({ id, value, min, max, onChange }: { id: string; value: number; min: number; max: number; onChange: (v: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return (
    <input
      id={id}
      type="number"
      min={min}
      max={max}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        const n = Number(e.target.value);
        if (e.target.value !== "" && Number.isInteger(n) && n >= min && n <= max) onChange(n);
      }}
      onBlur={() => setText(String(value))}
    />
  );
}

/** Preferences (gear button / Ctrl+, ). Changes apply immediately and are saved automatically. */
export default function PreferencesDialog({ onClose }: { onClose: () => void }) {
  const p = usePrefs();
  const mac = useIsMac();
  const [backupDir, setBackupDir] = useState("");
  useEffect(() => {
    api
      .backupFolder()
      .then(setBackupDir)
      .catch(() => {});
  }, []);
  const ed = (patch: Partial<typeof p.editor>) => updatePrefs({ editor: patch });

  return (
    <Modal
      title="Preferences"
      onClose={onClose}
      className="wide prefs"
      actions={
        <>
          <button onClick={resetPrefs}>Restore defaults</button>
          <span className="spacer" />
          <button className="primary" onClick={onClose}>
            Done
          </button>
        </>
      }
    >
      <p className="muted small">Changes take effect immediately and are saved automatically.</p>
      <div className="prefs-body">
        <fieldset>
          <legend>Appearance</legend>
          <Row label="Theme">
            {(id) => (
              <select id={id} value={p.theme} onChange={(e) => updatePrefs({ theme: e.target.value as typeof p.theme })}>
                <option value="system">Follow system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            )}
          </Row>
          <Row label="Text size">
            {(id) => (
              <select id={id} value={p.uiFontSize} onChange={(e) => updatePrefs({ uiFontSize: e.target.value as typeof p.uiFontSize })}>
                <option value="small">Small</option>
                <option value="normal">Normal</option>
                <option value="large">Large</option>
              </select>
            )}
          </Row>
        </fieldset>

        <fieldset>
          <legend>Config editor</legend>
          <Row label="Font size">{(id) => <NumberInput id={id} value={p.editor.fontSize} min={8} max={32} onChange={(v) => ed({ fontSize: v })} />}</Row>
          <Row label="Tab size">{(id) => <NumberInput id={id} value={p.editor.tabSize} min={1} max={8} onChange={(v) => ed({ tabSize: v })} />}</Row>
          <Check label="Insert spaces when pressing Tab" checked={p.editor.insertSpaces} onChange={(v) => ed({ insertSpaces: v })} />
          <Check label="Show minimap" checked={p.editor.minimap} onChange={(v) => ed({ minimap: v })} />
          <Check label="Wrap long lines" checked={p.editor.wordWrap} onChange={(v) => ed({ wordWrap: v })} />
          <Check
            label="Emacs-style keys (Ctrl+A/E/F/B/N/P/D/K/Y)"
            checked={p.editor.emacsKeys}
            onChange={(v) => ed({ emacsKeys: v })}
            hint={emacsNote(mac) + ` ${mac ? "Cmd" : "Ctrl"}+S still saves. The full list is in Help › Keyboard shortcuts.`}
          />
          <Check
            label="Confirm before saving to the device"
            checked={p.editor.confirmSave}
            onChange={(v) => ed({ confirmSave: v })}
            hint="Saving with rules-check warnings always asks first."
          />
          <Check
            label={`"Reboot after saving" checked by default`}
            checked={p.editor.rebootAfterSave}
            onChange={(v) => ed({ rebootAfterSave: v })}
          />
        </fieldset>

        <fieldset>
          <legend>Backups</legend>
          <Check
            label="Keep a backup copy of every config saved to a device"
            checked={p.backups.enabled}
            onChange={(v) => updatePrefs({ backups: { enabled: v } })}
          />
          <Row label="Keep backups for" hint="Days; older copies are deleted on the next save. 0 keeps them forever.">
            {(id) => (
              <NumberInput
                id={id}
                value={p.backups.retentionDays}
                min={0}
                max={3650}
                onChange={(v) => updatePrefs({ backups: { retentionDays: v } })}
              />
            )}
          </Row>
          {backupDir && (
            <p className="muted small">
              Folder: <code>{backupDir}</code>
            </p>
          )}
        </fieldset>

        <fieldset>
          <legend>Startup</legend>
          <Check label="Reopen the last selected tab" checked={p.startup.rememberTab} onChange={(v) => updatePrefs({ startup: { rememberTab: v } })} />
          <Check
            label="Reselect the last device (when connected)"
            checked={p.startup.rememberDevice}
            onChange={(v) => updatePrefs({ startup: { rememberDevice: v } })}
          />
        </fieldset>
      </div>
    </Modal>
  );
}
