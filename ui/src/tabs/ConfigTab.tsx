import { useEffect, useRef, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, Breadcrumb, DeviceSnapshot, Diagnostic, EditorRange, Template } from "../api";
import { confirm } from "../lib/confirm";
import TemplatePicker from "../lib/TemplatePicker";
import { setDirty } from "../lib/dirty";
import ConfigEditor, { ConfigEditorHandle } from "../editor/ConfigEditor";
import HelpPane from "../editor/HelpPane";
import FormView from "../editor/FormView";
import BackupPicker from "../lib/BackupPicker";
import { getPrefs, usePrefs } from "../prefs/store";
import { printListing } from "../shell/print";

const JSON_FILTER = [{ name: "Pinscape config", extensions: ["json", "json5", "txt"] }];

/** Text or Form view, remembered across sessions (per-user convenience only). */
type EditView = "text" | "form";
const VIEW_KEY = "pinscape.configView";
const savedView = (): EditView => {
  try {
    return localStorage.getItem(VIEW_KEY) === "form" ? "form" : "text";
  } catch {
    return "text";
  }
};

interface Props {
  dev: DeviceSnapshot;
  safeMode: boolean;
  /** Hidden tabs stay mounted so edits aren't lost when switching tabs. */
  visible: boolean;
  onStatus: (s: string) => void;
}

export default function ConfigTab({ dev, safeMode, visible, onStatus }: Props) {
  const [text, setText] = useState("");
  const [original, setOriginal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const prefs = usePrefs();
  // Default from Preferences › Editor; toggling the box only affects this tab.
  const [resetAfter, setResetAfter] = useState(() => getPrefs().editor.rebootAfterSave);
  const [picking, setPicking] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [diags, setDiags] = useState<Diagnostic[]>([]);
  const [crumb, setCrumb] = useState<Breadcrumb | null>(null);
  const [help, setHelp] = useState<{ page: string; anchor: string | null } | null>(null);
  const [showProblems, setShowProblems] = useState(true);
  const editor = useRef<ConfigEditorHandle>(null);
  const [view, setViewState] = useState<EditView>(savedView);
  const [formSection, setFormSection] = useState<string | null>(null);
  const setView = (v: EditView) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* not remembered */
    }
  };
  /** Select a range in the text view (switching to it from the form view). */
  const reveal = (d: { range: EditorRange }) => {
    setView("text");
    setTimeout(() => editor.current?.reveal(d), 0);
  };

  const dirty = original !== null && text !== original;
  const what = safeMode ? "Safe Mode config" : "config";
  const errors = diags.filter((d) => d.severity === "error");
  const dirtyId = `${dev.ids?.unit_name ?? dev.key} ${what}`;
  useEffect(() => {
    setDirty(dirtyId, dirty);
  }, [dirtyId, dirty]);
  useEffect(() => () => setDirty(dirtyId, false), [dirtyId]);
  useEffect(() => {
    if (visible && view === "text") setTimeout(() => editor.current?.focus(), 0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible]);
  const warnings = diags.filter((d) => d.severity === "warning");

  /** Replace the editor contents (keeps `original`, so it shows as unsaved). */
  const replaceText = async (t: string, source: string) => {
    if (dirty && !(await confirm({ title: "Replace editor contents?", body: `Discard your unsaved edits and load ${source}?`, ok: "Replace" })))
      return;
    setText(t);
    if (original === null) setOriginal("");
    setMsg(`Loaded ${source} (${t.length.toLocaleString()} bytes). Not saved to the device yet.`);
  };

  const importFile = async () => {
    const path = await openDialog({ multiple: false, directory: false, filters: JSON_FILTER, title: `Import ${what}` });
    if (typeof path !== "string") return;
    try {
      await replaceText(await api.readTextFile(path), path.split(/[\\/]/).pop() ?? path);
    } catch (e) {
      setMsg(`Import failed: ${e}`);
    }
  };

  const exportFile = async () => {
    const base = (dev.ids?.unit_name || "pinscape").replace(/[^A-Za-z0-9._-]+/g, "-");
    const stamp = new Date().toISOString().slice(0, 10);
    const path = await saveDialog({
      title: `Export ${what}`,
      filters: JSON_FILTER,
      defaultPath: `${base}${safeMode ? "-safemode" : ""}-${stamp}.json`,
    });
    if (!path) return;
    try {
      await api.writeTextFile(path, text);
      setMsg(`Exported to ${path}`);
    } catch (e) {
      setMsg(`Export failed: ${e}`);
    }
  };

  const useTemplate = async (t: Template) => {
    setPicking(false);
    try {
      await replaceText(await api.readTemplate(t.id), `the "${t.name}" template`);
    } catch (e) {
      setMsg(`Template failed: ${e}`);
    }
  };

  const load = async () => {
    if (dirty && !(await confirm({ title: "Discard changes?", body: `Reload the ${what} from the device and discard your edits?`, ok: "Discard" })))
      return;
    setBusy(true);
    try {
      const t = await api.getConfig(dev.key, safeMode);
      setText(t ?? "");
      setOriginal(t ?? "");
      setMsg(t == null ? `No ${what} stored on the device yet. Start from a template or type one in.` : `Loaded ${t.length.toLocaleString()} bytes from the device.`);
      setTimeout(() => editor.current?.focus(), 0);
    } catch (e) {
      setMsg(`Load failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async () => {
    if (busy || !dirty) return;
    if (!text.trim()) {
      setMsg("Nothing to save: the editor is empty. (Use Erase Config on the Overview tab to delete it.)");
      return;
    }
    // Same checks as the Windows tool: the device can't use a file with
    // syntax errors; rules-check warnings need confirmation.
    const fresh = await api.configValidate(text);
    const errs = fresh.filter((d) => d.severity === "error");
    const warns = fresh.filter((d) => d.severity === "warning");
    if (errs.length) {
      setDiags(fresh);
      setShowProblems(true);
      reveal(errs[0]);
      setMsg(`Not saved: ${errs.length} syntax error${errs.length > 1 ? "s" : ""}. Only a valid file can be saved to the device.`);
      return;
    }
    const body =
      (warns.length
        ? `The rules check found ${warns.length} problem${warns.length > 1 ? "s" : ""} (see the Problems list). The device ignores settings it doesn't recognize, so these may not work as intended.\n\n`
        : "") +
      (resetAfter
        ? `Write ${text.length.toLocaleString()} bytes to the device and reboot it so the new settings take effect?`
        : `Write ${text.length.toLocaleString()} bytes to the device? The new settings take effect after the next reboot.`);
    // Warnings always need confirmation; a clean save only if Preferences say so.
    if (warns.length || prefs.editor.confirmSave) {
      const ok = await confirm({ title: warns.length ? "Save with warnings?" : `Save ${what} to device`, body, ok: "Save" });
      if (!ok) return;
    }
    setBusy(true);
    try {
      const r = await api.putConfig(dev.key, text, safeMode, resetAfter);
      setOriginal(text);
      const m =
        (resetAfter ? `Saved ${what}; the device is rebooting.` : `Saved ${what}. Reboot to apply.`) +
        (r.backup
          ? " Backup copy kept (Backups…)."
          : prefs.backups.enabled
            ? ` Warning: no backup copy (${r.backupError}).`
            : " No backup copy (turned off in Preferences).");
      setMsg(m);
      onStatus(m);
    } catch (e) {
      setMsg(`Save failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const print = () => {
    const name = dev.ids ? `#${dev.ids.unit_num} ${dev.ids.unit_name}` : dev.key;
    printListing({
      title: `${safeMode ? "Safe Mode config" : "Config"} — ${name}`,
      subtitle: [dev.ids?.hwid, dirty ? "includes unsaved edits" : null].filter(Boolean).join(" · "),
      text,
    }).catch((e) => setMsg(`Print failed: ${e}`));
  };

  const openHelp = (anchor: string | null) => setHelp({ page: "JSONConfigRef.htm", anchor });

  return (
    <div className="config" style={{ display: visible ? "flex" : "none" }}>
      <div className="toolbar">
        <button onClick={load} disabled={busy}>
          Reload from device
        </button>
        <button className="primary" onClick={save} disabled={busy || !dirty} title="Ctrl+S">
          Save to device
        </button>
        <label className="inline">
          <input type="checkbox" checked={resetAfter} onChange={(e) => setResetAfter(e.target.checked)} /> reboot after saving
        </label>
        <span className="sep" />
        <button onClick={() => setPicking(true)} disabled={busy}>
          New from template…
        </button>
        <button onClick={importFile} disabled={busy}>
          Import file…
        </button>
        <button onClick={exportFile} disabled={busy || !text}>
          Export…
        </button>
        <button onClick={() => setRestoring(true)} disabled={busy} title="Copies of every config saved to this device">
          Backups…
        </button>
        <button onClick={print} disabled={!text} title="Print the config with line numbers">
          Print…
        </button>
        <span className="sep" />
        <span className="ff-seg" role="group" aria-label="Editor view">
          <button className={view === "text" ? "active" : ""} aria-pressed={view === "text"} onClick={() => setView("text")} title="Edit the config as text">
            Text
          </button>
          <button className={view === "form" ? "active" : ""} aria-pressed={view === "form"} onClick={() => setView("form")} title="Edit common sections with a form (changes go into the text)">
            Form
          </button>
        </span>
        <button onClick={() => (help ? setHelp(null) : openHelp(crumb?.helpAnchor ?? null))} title="F1 in the editor opens help for the property under the cursor">
          {help ? "Hide reference" : "Reference"}
        </button>
        <span className="spacer" />
        {dirty && <span className="badge">unsaved changes</span>}
      </div>

      <div className="crumbbar">
        <span className="crumbs">
          {crumb && (crumb.path || crumb.property) ? (
            <>
              {[crumb.path, crumb.property].filter(Boolean).join(crumb.path ? " › " : "")}
              {crumb.helpAnchor && (
                <button className="link small" onClick={() => openHelp(crumb.helpAnchor)}>
                  help (F1)
                </button>
              )}
            </>
          ) : (
            <span className="muted">top level</span>
          )}
        </span>
        <span className="muted small msg">{msg}</span>
      </div>

      <div className={"editor-split" + (help ? " with-help" : "")}>
        <div className="editor-col">
          {view === "form" && (
            <FormView
              text={text}
              section={formSection}
              onSection={setFormSection}
              readOnly={busy}
              apply={(base, r) => editor.current?.applyEdits(base, r.edits) ?? false}
              onHelp={openHelp}
              onShowText={(range) => (range ? reveal({ range }) : setView("text"))}
              onUndo={() => editor.current?.undo()}
              onRedo={() => editor.current?.redo()}
              onSave={save}
            />
          )}
          {/* The text editor stays mounted in the form view: form edits go through its model (undo history). */}
          <div style={{ display: view === "text" ? "contents" : "none" }}>
            <ConfigEditor
              ref={editor}
              value={text}
              onChange={setText}
              onDiagnostics={setDiags}
              onBreadcrumb={setCrumb}
              onHelp={openHelp}
              onSave={save}
              readOnly={busy}
            />
          </div>
          <div className="problems">
            <button className="problems-head" onClick={() => setShowProblems(!showProblems)} aria-expanded={showProblems}>
              <span>{showProblems ? "▾" : "▸"} Problems</span>
              <span className={errors.length ? "error" : "muted"}>
                {errors.length} error{errors.length === 1 ? "" : "s"}
              </span>
              <span className={warnings.length ? "warn" : "muted"}>
                {warnings.length} warning{warnings.length === 1 ? "" : "s"}
              </span>
            </button>
            {showProblems && diags.length > 0 && (
              <ul>
                {diags.slice(0, 200).map((d, i) => (
                  <li
                    key={i}
                    tabIndex={0}
                    onClick={() => reveal(d)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        reveal(d);
                      }
                    }}
                  >
                    <span className={d.severity === "error" ? "error" : "warn"} role="img" aria-label={d.severity}>
                      {d.severity === "error" ? "✖" : "▲"}
                    </span>
                    <span className="muted small">
                      {d.range.startLine}:{d.range.startCol}
                    </span>
                    <span>{d.message}</span>
                    {d.helpAnchor && (
                      <button
                        className="link small"
                        onClick={(e) => {
                          e.stopPropagation();
                          openHelp(d.helpAnchor);
                        }}
                      >
                        help
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
        {help && <HelpPane page={help.page} anchor={help.anchor} onClose={() => setHelp(null)} />}
      </div>

      {picking && <TemplatePicker safeMode={safeMode} onPick={useTemplate} onClose={() => setPicking(false)} />}
      {restoring && (
        <BackupPicker
          hwid={dev.key}
          safeMode={safeMode}
          onClose={() => setRestoring(false)}
          onPick={async (b) => {
            setRestoring(false);
            try {
              await replaceText(await api.readBackup(b.file), `the backup from ${b.savedAt}`);
            } catch (e) {
              setMsg(`Restore failed: ${e}`);
            }
          }}
        />
      )}
    </div>
  );
}
