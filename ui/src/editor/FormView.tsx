// Form view of the config editor: the schema-driven alternative to the text
// editor for buttons, outputs and the other top-level sections. The text
// stays the source of truth: the form is rebuilt from it after every change,
// and each change is a small text edit computed in Rust (pinscape_config::edit)
// and applied through the Monaco model, so comments and formatting are kept
// and Undo works the same in both views.

import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { EditorRange } from "../api";
import { EditOp, EditResult, FormField, FormItem, FormView as View, formApi, PropInfo, pathKey } from "../lib/api-configform";
import { isMac } from "../shell/platform";
import "./FormView.css";

interface Props {
  text: string;
  /** Selected top-level section (`null`: the first common one in the file). */
  section: string | null;
  onSection: (name: string) => void;
  readOnly: boolean;
  /** Apply the edits to the editor as one undoable step; false if the text changed meanwhile. */
  apply: (base: string, r: EditResult) => boolean;
  onHelp: (anchor: string | null) => void;
  /** Switch to the text view (optionally selecting a range). */
  onShowText: (range: EditorRange | null) => void;
  onUndo: () => void;
  onRedo: () => void;
  onSave: () => void;
}

const NUMBER = /^[+-]?(0[xX][0-9a-fA-F]+|0[bB][01]+|0[oO][0-7]+|(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?)$/;
const STRUCTURAL = new Set(["remove", "addItem", "duplicate", "create"]);

/** Context shared by all rows. */
interface Ctx {
  info: PropInfo[];
  disabled: boolean;
  run: (op: EditOp) => void;
  fail: (msg: string) => void;
  onHelp: (anchor: string | null) => void;
  open: Set<string>;
  toggle: (key: string) => void;
}

export default function FormView(props: Props) {
  const { section, onSection: setSection } = props;
  const [view, setView] = useState<View | null>(null);
  const [viewFor, setViewFor] = useState<string | null>(null);
  const [err, setErr] = useState("");
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  // Latest text, updated as soon as an edit is applied (before React re-renders).
  const textRef = useRef(props.text);
  const propText = useRef(props.text);
  if (propText.current !== props.text) {
    propText.current = props.text;
    textRef.current = props.text;
  }
  const queue = useRef<Promise<void>>(Promise.resolve());
  const waitForView = useRef(false);
  const cb = useRef(props);
  cb.current = props;

  // Rebuild the form whenever the text or the section changes.
  useEffect(() => {
    let cancelled = false;
    const text = props.text;
    const t = setTimeout(async () => {
      try {
        const v = await formApi.form(text, section);
        if (cancelled) return;
        setView(v);
        setViewFor(text);
        waitForView.current = false;
        if (!section && v.field) setSection(v.field.name);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    }, 60);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.text, section]);

  /** Run edits one at a time, each against the latest text. */
  const run = useCallback((op: EditOp) => {
    queue.current = queue.current.then(async () => {
      // after an add/remove, wait for the rebuilt form before using its paths
      if (waitForView.current) return;
      const base = textRef.current;
      try {
        const r = await formApi.edit(base, op);
        if (!cb.current.apply(base, r)) {
          setErr("The text changed while this edit was being made; please try again.");
          return;
        }
        textRef.current = r.text;
        if (STRUCTURAL.has(op.op)) waitForView.current = true;
        setErr("");
      } catch (e) {
        setErr(String(e));
      }
    });
  }, []);

  const toggle = useCallback((key: string) => {
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });
  }, []);

  const onKeyDown = (e: ReactKeyboardEvent) => {
    // F1: reference for the property with focus (else the app's Help tab)
    if (e.key === "F1" && !e.ctrlKey && !e.metaKey && !e.shiftKey && !e.altKey) {
      const anchor = (e.target as HTMLElement).closest?.("[data-help]")?.getAttribute("data-help");
      if (anchor) {
        e.preventDefault();
        e.stopPropagation();
        cb.current.onHelp(anchor);
      }
      return;
    }
    const mod = isMac() ? e.metaKey : e.ctrlKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === "s") {
      e.preventDefault();
      cb.current.onSave();
      return;
    }
    // Text inputs keep their own undo; elsewhere Ctrl+Z / Ctrl+Y undo form edits.
    const t = e.target as HTMLElement;
    const typing = t instanceof HTMLTextAreaElement || (t instanceof HTMLInputElement && t.type !== "checkbox");
    if (typing) return;
    if (k === "z" && !e.shiftKey) {
      e.preventDefault();
      cb.current.onUndo();
    } else if (k === "y" || (k === "z" && e.shiftKey)) {
      e.preventDefault();
      cb.current.onRedo();
    }
  };

  if (view?.problem) {
    const p = view.problem;
    return (
      <div className="ff" onKeyDown={onKeyDown}>
        <div className="ff-problem">
          <h3>{p.empty ? "Nothing to show yet" : "The form view can't show this file"}</h3>
          <p>{p.message}</p>
          {p.empty ? (
            <p className="muted">Use “New from template…” to start from an example config, or type one in the text view.</p>
          ) : (
            <p className="muted">The form only edits files that parse cleanly, so it never has to guess what you meant.</p>
          )}
          <div className="ff-problem-actions">
            <button className="primary" onClick={() => props.onShowText(p.range)}>
              {p.range ? "Show the error in the text view" : "Open the text view"}
            </button>
          </div>
        </div>
      </div>
    );
  }

  const ctx: Ctx = {
    info: view?.info ?? [],
    disabled: props.readOnly,
    run,
    fail: setErr,
    onHelp: props.onHelp,
    open,
    toggle,
  };
  const field = view?.field ?? null;
  const sections = view?.sections ?? [];
  const info = field?.info != null ? ctx.info[field.info] : null;
  const firstOther = sections.findIndex((s) => !s.common);

  return (
    <div className="ff" onKeyDown={onKeyDown}>
      <nav className="ff-nav" aria-label="Config sections">
        {sections.map((s, i) => (
          <div key={s.name}>
            {i === firstOther && <div className="ff-nav-head muted small">Other sections</div>}
            <button
              className={"ff-nav-item" + (s.name === section ? " active" : "") + (s.present ? "" : " absent")}
              onClick={() => setSection(s.name)}
              title={s.title ?? s.name}
            >
              {s.name}
            </button>
          </div>
        ))}
      </nav>
      <div className="ff-main">
        <div className="ff-head">
          <div className="ff-title">
            <strong>{field?.name ?? section ?? "…"}</strong>
            {info?.title && <span className="muted">{info.title}</span>}
            {info?.link && (
              <button className="link small" onClick={() => props.onHelp(info.link)}>
                help
              </button>
            )}
          </div>
          <span className="spacer" />
          <button onClick={props.onUndo} disabled={props.readOnly} title={isMac() ? "⌘Z" : "Ctrl+Z"}>
            Undo
          </button>
          <button onClick={props.onRedo} disabled={props.readOnly} title={isMac() ? "⇧⌘Z" : "Ctrl+Y"}>
            Redo
          </button>
        </div>
        {err && (
          <div className="ff-error" role="alert">
            {err}
            <button className="link small" onClick={() => setErr("")}>
              dismiss
            </button>
          </div>
        )}
        <div className={"ff-body" + (viewFor !== props.text ? " stale" : "")}>
          {field && <SectionBody f={field} ctx={ctx} info={info} />}
        </div>
      </div>
    </div>
  );
}

function SectionBody({ f, ctx, info }: { f: FormField; ctx: Ctx; info: PropInfo | null }) {
  const summary = info?.summary && <p className="ff-summary muted">{info.summary}</p>;
  if (!f.present) {
    return (
      <>
        {summary}
        <div className="ff-absent">
          <p>
            There's no <code>{f.name}</code> section in this config.
          </p>
          {f.canList && (
            <button disabled={ctx.disabled} onClick={() => ctx.run({ op: "addItem", path: f.path })}>
              Add {f.name} with one entry
            </button>
          )}
          {f.canObject && (
            <button disabled={ctx.disabled} onClick={() => ctx.run({ op: "create", path: f.path, object: true })}>
              Add the {f.name} section
            </button>
          )}
          {!f.canList && !f.canObject && <p className="muted">Add it in the text view.</p>}
        </div>
      </>
    );
  }
  if (f.control === "list") return (
    <>
      {summary}
      <ItemList f={f} ctx={ctx} />
    </>
  );
  if (f.control === "object") return (
    <>
      {summary}
      <Fields fields={f.fields ?? []} ctx={ctx} />
    </>
  );
  return (
    <>
      {summary}
      <Fields fields={[f]} ctx={ctx} />
    </>
  );
}

/** Present properties as rows; the schema's other properties folded away below. */
function Fields({ fields, ctx }: { fields: FormField[]; ctx: Ctx }) {
  const up = (f: FormField) => f.present || f.selector || f.featured;
  const shown = fields.filter(up);
  const more = fields.filter((f) => !up(f));
  return (
    <div className="ff-fields">
      {shown.map((f) => (
        <Row key={pathKey(f.path)} f={f} ctx={ctx} />
      ))}
      {more.length > 0 && (
        <details className="ff-more">
          <summary className="muted small">
            {more.length} more propert{more.length === 1 ? "y" : "ies"} (not set)
          </summary>
          {more.map((f) => (
            <Row key={pathKey(f.path)} f={f} ctx={ctx} />
          ))}
        </details>
      )}
    </div>
  );
}

function Row({ f, ctx }: { f: FormField; ctx: Ctx }) {
  const info = f.info != null ? ctx.info[f.info] : null;
  const nested = f.control === "object" || f.control === "list";
  const label = (
    <div className="ff-label">
      <code className={f.unknown ? "warn" : ""} title={info?.title ?? undefined}>
        {f.name}
      </code>
      {info?.required && (
        <span className="ff-req" title="required">
          *
        </span>
      )}
      <span className="muted small ff-type">{info ? info.typeDesc : ""}</span>
    </div>
  );
  const remove = f.present && (
    <button
      className="icon-btn ff-remove"
      title={`Remove ${f.name}`}
      aria-label={`Remove ${f.name}`}
      disabled={ctx.disabled}
      onClick={() => ctx.run({ op: "remove", path: f.path })}
    >
      ✕
    </button>
  );
  const help = (
    <div className="ff-help small">
      {f.unknown ? (
        <span className="warn">Not a valid property here{f.raw !== undefined ? " (check the type above, or remove it)" : ""}.</span>
      ) : (
        info?.summary && <span className="muted">{info.summary}</span>
      )}
      {info?.link && (
        <button className="link small" onClick={() => ctx.onHelp(info.link)} title="Open the reference at this property (F1 while a field has focus)">
          help
        </button>
      )}
    </div>
  );
  if (nested) {
    return (
      <div className={"ff-row nested" + (f.present ? "" : " absent")} data-help={info?.link ?? undefined}>
        <div className="ff-row-top">
          {label}
          <div className="ff-control">
            {!f.present && f.canObject && (
              <button className="small" disabled={ctx.disabled} onClick={() => ctx.run({ op: "create", path: f.path, object: true })}>
                Add
              </button>
            )}
            {!f.present && f.canList && (
              <button className="small" disabled={ctx.disabled} onClick={() => ctx.run({ op: "addItem", path: f.path })}>
                Add list
              </button>
            )}
          </div>
          {remove}
        </div>
        {help}
        {f.present && (
          <div className="ff-nest">{f.control === "list" ? <ItemList f={f} ctx={ctx} /> : <Fields fields={f.fields ?? []} ctx={ctx} />}</div>
        )}
      </div>
    );
  }
  return (
    <div className={"ff-row" + (f.present ? "" : " absent")} data-help={info?.link ?? undefined}>
      <div className="ff-row-top">
        {label}
        <div className="ff-control">
          <Control f={f} info={info} ctx={ctx} />
        </div>
        {remove}
      </div>
      {help}
    </div>
  );
}

/** Literal for an enum choice: numbers stay numbers unless the property is string-only. */
function enumOp(f: FormField, info: PropInfo | null, v: string): EditOp {
  const stringOnly = info ? info.kinds.length > 0 && info.kinds.every((k) => k === "string") : true;
  if (!stringOnly && NUMBER.test(v)) return { op: "setRaw", path: f.path, text: v };
  if (v === "true" || v === "false") {
    if (info?.kinds.includes("boolean")) return { op: "set", path: f.path, value: v === "true" };
  }
  return { op: "set", path: f.path, value: v };
}

function Control({ f, info, ctx }: { f: FormField; info: PropInfo | null; ctx: Ctx }) {
  const d = ctx.disabled;
  const placeholder = f.present ? "" : "not set";
  switch (f.control) {
    case "enum": {
      const cur = f.present ? (f.value === undefined || f.value === null ? (f.raw ?? "") : String(f.value)) : "";
      const options = f.options ?? [];
      return (
        <select
          value={cur}
          disabled={d}
          onChange={(e) => {
            const v = e.target.value;
            ctx.run(v === "" ? { op: "remove", path: f.path } : enumOp(f, info, v));
          }}
        >
          <option value="">{f.present ? "(remove)" : "(not set)"}</option>
          {cur && !options.includes(cur) && <option value={cur}>{cur} (not valid)</option>}
          {options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      );
    }
    case "bool":
      return (
        <label className="inline">
          <input
            type="checkbox"
            disabled={d}
            checked={f.value === true}
            onChange={(e) => ctx.run({ op: "set", path: f.path, value: e.target.checked })}
          />
          <span className="muted small">{f.present ? String(f.value) : "not set"}</span>
        </label>
      );
    case "string":
      return (
        <TextInput
          display={f.present ? String(f.value ?? "") : ""}
          stamp={f.raw}
          placeholder={placeholder}
          disabled={d}
          onCommit={(v) => {
            if (!f.present && v === "") return;
            ctx.run({ op: "set", path: f.path, value: v });
          }}
        />
      );
    case "number":
    case "numberOrString":
    case "raw": {
      const display = f.present ? (f.control !== "raw" && typeof f.value === "string" ? f.value : (f.raw ?? "")) : "";
      return (
        <TextInput
          display={display}
          stamp={f.raw}
          placeholder={f.control === "raw" ? (info?.typeDesc ?? "value") : placeholder}
          mono={f.control !== "numberOrString"}
          disabled={d}
          onCommit={(v) => {
            const t = v.trim();
            if (t === "") {
              if (f.present) ctx.run({ op: "remove", path: f.path });
              return;
            }
            if (f.control === "number") {
              if (!NUMBER.test(t)) return ctx.fail(`${f.name}: "${t}" isn't a number.`);
              ctx.run({ op: "setRaw", path: f.path, text: t });
            } else if (f.control === "numberOrString") {
              ctx.run(NUMBER.test(t) ? { op: "setRaw", path: f.path, text: t } : { op: "set", path: f.path, value: v });
            } else {
              ctx.run({ op: "setRaw", path: f.path, text: t });
            }
          }}
        />
      );
    }
    default:
      return null;
  }
}

/**
 * A text box that commits on Enter or when it loses focus (Esc reverts).
 * `stamp` (the value's source text) resets it when the document changes.
 */
function TextInput(p: {
  display: string;
  stamp?: string;
  placeholder?: string;
  mono?: boolean;
  disabled: boolean;
  onCommit: (v: string) => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const commit = () => {
    const v = ref.current?.value ?? "";
    if (v !== p.display) p.onCommit(v);
  };
  return (
    <input
      key={p.stamp ?? "\u0000absent"}
      ref={ref}
      type="text"
      className={"ff-input" + (p.mono ? " mono" : "")}
      defaultValue={p.display}
      placeholder={p.placeholder}
      disabled={p.disabled}
      spellCheck={false}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        } else if (e.key === "Escape" && ref.current) {
          ref.current.value = p.display;
        }
      }}
    />
  );
}

function ItemList({ f, ctx }: { f: FormField; ctx: Ctx }) {
  const items = f.items ?? [];
  // Output ports are numbered from 1 (as in DOF); other lists from 0.
  const base = f.path.length === 1 && f.path[0] === "outputs" ? 1 : 0;
  const allOpen = items.length > 0 && items.every((it) => ctx.open.has(pathKey(it.path)));
  return (
    <div className="ff-list">
      {items.length > 1 && (
        <div className="ff-list-head">
          <span className="muted small">
            {items.length} entries
          </span>
          <button
            className="link small"
            onClick={() => items.forEach((it) => (ctx.open.has(pathKey(it.path)) === allOpen ? ctx.toggle(pathKey(it.path)) : null))}
          >
            {allOpen ? "collapse all" : "expand all"}
          </button>
        </div>
      )}
      {items.length === 0 && <p className="muted small">No entries yet.</p>}
      {items.map((it) => (
        <ItemCard key={pathKey(it.path)} it={it} n={it.index + base} ctx={ctx} />
      ))}
      <div>
        <button disabled={ctx.disabled} onClick={() => ctx.run({ op: "addItem", path: f.path })}>
          + Add entry
        </button>
      </div>
    </div>
  );
}

function ItemCard({ it, n, ctx }: { it: FormItem; n: number; ctx: Ctx }) {
  const key = pathKey(it.path);
  const isOpen = ctx.open.has(key);
  let body: ReactNode = null;
  if (isOpen) {
    body = it.fields ? (
      <Fields fields={it.fields} ctx={ctx} />
    ) : (
      <div className="ff-row">
        <div className="ff-row-top">
          <div className="ff-label muted small">value</div>
          <div className="ff-control">
            <TextInput
              display={it.raw}
              stamp={it.raw}
              mono
              disabled={ctx.disabled}
              onCommit={(v) => v.trim() && ctx.run({ op: "setRaw", path: it.path, text: v })}
            />
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className={"ff-item" + (isOpen ? " open" : "")}>
      <div className="ff-item-head">
        <button className="ff-item-toggle" onClick={() => ctx.toggle(key)} aria-expanded={isOpen}>
          <span className="ff-caret">{isOpen ? "▾" : "▸"}</span>
          <span className="ff-item-num">#{n}</span>
          <span className="ff-item-label">{it.label || "(empty)"}</span>
        </button>
        <button className="small" disabled={ctx.disabled} onClick={() => ctx.run({ op: "duplicate", path: it.path })} title="Insert a copy after this entry">
          Duplicate
        </button>
        <button className="small danger" disabled={ctx.disabled} onClick={() => ctx.run({ op: "remove", path: it.path })} title="Remove this entry (Undo brings it back)">
          Remove
        </button>
      </div>
      {body}
    </div>
  );
}
