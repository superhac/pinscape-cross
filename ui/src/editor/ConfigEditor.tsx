import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { api, Breadcrumb, Diagnostic, EditorRange, Preferences } from "../api";
import { currentTheme, LANG, monaco, registerLanguage, toRange } from "./monaco";
import { installEmacsKeys } from "./emacs";
import { getPrefs, subscribePrefs } from "../prefs/store";
import { isMac } from "../shell/platform";

/** Editor options from Preferences › Editor. */
function prefOptions(p: Preferences["editor"]): monaco.editor.IEditorOptions {
  return {
    fontSize: p.fontSize,
    minimap: { enabled: p.minimap },
    wordWrap: p.wordWrap ? "on" : "off",
  };
}

/** TextModel's undo/redo (not in Monaco's public typings). */
type UndoModel = monaco.editor.ITextModel & { undo(): void; redo(): void };

export interface ConfigEditorHandle {
  /** Move the cursor to a range and focus the editor. */
  reveal: (d: { range: EditorRange }) => void;
  focus: () => void;
  /** Apply form edits as one undoable step; false if the text isn't `base` any more. */
  applyEdits: (base: string, edits: { range: EditorRange; text: string }[]) => boolean;
  undo: () => void;
  redo: () => void;
}

interface Props {
  value: string;
  onChange: (text: string) => void;
  onDiagnostics: (d: Diagnostic[]) => void;
  onBreadcrumb: (b: Breadcrumb) => void;
  /** F1: context help for the property under the cursor. */
  onHelp: (anchor: string | null) => void;
  /** Ctrl/Cmd+S */
  onSave: () => void;
  readOnly?: boolean;
}

/** Monaco editor bound to the pinscape-json language and the Rust validator. */
const ConfigEditor = forwardRef<ConfigEditorHandle, Props>(function ConfigEditor(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  // Keep the latest callbacks without re-creating the editor.
  const cb = useRef(props);
  cb.current = props;
  const lastBreadcrumb = useRef<Breadcrumb | null>(null);

  useImperativeHandle(ref, () => ({
    reveal(d) {
      const e = editor.current;
      if (!e) return;
      const r = toRange(d.range);
      // Coming from the form view the editor was hidden and still thinks its
      // viewport is zero high, which would put the target at the very top,
      // under the sticky-scroll header. Measure it first.
      e.layout();
      e.setSelection(r);
      e.revealRangeInCenterIfOutsideViewport(r);
      e.focus();
    },
    focus: () => editor.current?.focus(),
    applyEdits(base, edits) {
      const e = editor.current;
      const model = e?.getModel();
      if (!e || !model || model.getValue() !== base) return false;
      e.pushUndoStop();
      model.pushEditOperations([], edits.map((x) => ({ range: toRange(x.range), text: x.text })), () => null);
      e.pushUndoStop();
      return true;
    },
    // Straight to the model: the editor's undo command would act on the
    // focused form input instead while the form view is showing.
    undo: () => (editor.current?.getModel() as UndoModel | null)?.undo(),
    redo: () => (editor.current?.getModel() as UndoModel | null)?.redo(),
  }));

  useEffect(() => {
    registerLanguage();
    const ep = getPrefs().editor;
    const model = monaco.editor.createModel(props.value, LANG);
    model.updateOptions({ tabSize: ep.tabSize, insertSpaces: ep.insertSpaces });
    const e = monaco.editor.create(host.current!, {
      model,
      theme: currentTheme(),
      automaticLayout: true,
      fontFamily: 'ui-monospace, "SF Mono", "Cascadia Mono", "DejaVu Sans Mono", monospace',
      ...prefOptions(ep),
      ariaLabel: "Config editor",
      scrollBeyondLastLine: false,
      renderWhitespace: "selection",
      bracketPairColorization: { enabled: true },
      guides: { bracketPairs: "active", indentation: true },
      quickSuggestions: { other: true, comments: false, strings: true },
      suggestOnTriggerCharacters: true,
      wordBasedSuggestions: "off",
      fixedOverflowWidgets: true,
      readOnly: props.readOnly,
    });
    editor.current = e;

    // Diagnostics: debounced validation through Rust.
    let vTimer: ReturnType<typeof setTimeout> | undefined;
    let vSeq = 0;
    const validate = () => {
      clearTimeout(vTimer);
      vTimer = setTimeout(async () => {
        const seq = ++vSeq;
        const text = model.getValue();
        // An empty editor (e.g. no Safe Mode config on the device yet) isn't an
        // error to report: saving it is refused anyway, with its own message.
        const diags = text.trim() === "" ? [] : await api.configValidate(text).catch(() => [] as Diagnostic[]);
        if (seq !== vSeq || model.isDisposed()) return;
        monaco.editor.setModelMarkers(
          model,
          "pinscape",
          diags.map((d) => ({
            startLineNumber: d.range.startLine,
            startColumn: d.range.startCol,
            endLineNumber: d.range.endLine,
            endColumn: d.range.endCol,
            message: d.message,
            severity:
              d.severity === "error"
                ? monaco.MarkerSeverity.Error
                : d.severity === "warning"
                  ? monaco.MarkerSeverity.Warning
                  : monaco.MarkerSeverity.Info,
          })),
        );
        cb.current.onDiagnostics(diags);
      }, 250);
    };

    // Breadcrumb: debounced on cursor moves.
    let bTimer: ReturnType<typeof setTimeout> | undefined;
    const breadcrumb = () => {
      clearTimeout(bTimer);
      bTimer = setTimeout(async () => {
        const p = e.getPosition();
        if (!p) return;
        const b = await api.configBreadcrumb(model.getValue(), p.lineNumber, p.column).catch(() => null);
        if (b) {
          lastBreadcrumb.current = b;
          cb.current.onBreadcrumb(b);
        }
      }, 120);
    };

    const subs = [
      model.onDidChangeContent(() => {
        cb.current.onChange(model.getValue());
        validate();
      }),
      e.onDidChangeCursorPosition(breadcrumb),
    ];

    e.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => cb.current.onSave());
    e.addCommand(monaco.KeyCode.F1, async () => {
      const p = e.getPosition();
      const b = p ? await api.configBreadcrumb(model.getValue(), p.lineNumber, p.column).catch(() => null) : null;
      cb.current.onHelp(b?.helpAnchor ?? lastBreadcrumb.current?.helpAnchor ?? null);
    });
    // Keep the command palette reachable (Monaco binds it to F1 by default).
    e.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyP, () =>
      e.trigger("keyboard", "editor.action.quickCommand", null),
    );

    // Emacs keys (Preferences › Editor), toggled through a context key.
    const emacs = installEmacsKeys(e, isMac());
    emacs.set(ep.emacsKeys);

    // Live preference changes.
    let lastEditor = ep;
    const unPrefs = subscribePrefs((p) => {
      if (p.editor === lastEditor) return;
      lastEditor = p.editor;
      e.updateOptions(prefOptions(p.editor));
      if (!model.isDisposed()) model.updateOptions({ tabSize: p.editor.tabSize, insertSpaces: p.editor.insertSpaces });
      emacs.set(p.editor.emacsKeys);
    });

    // Theme: the OS setting or Preferences › Theme.
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onScheme = () => monaco.editor.setTheme(currentTheme());
    mq?.addEventListener("change", onScheme);
    window.addEventListener("pinscape-theme-change", onScheme);

    validate();
    breadcrumb();

    return () => {
      clearTimeout(vTimer);
      clearTimeout(bTimer);
      subs.forEach((s) => s.dispose());
      mq?.removeEventListener("change", onScheme);
      window.removeEventListener("pinscape-theme-change", onScheme);
      unPrefs();
      e.dispose();
      model.dispose();
      editor.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // External value changes (load from device, template, import).
  useEffect(() => {
    const e = editor.current;
    const model = e?.getModel();
    if (!e || !model || model.getValue() === props.value) return;
    // Keep the text's own line endings (upstream templates use CRLF), so an
    // unedited file isn't reported as changed and saves back byte-identical.
    const eol = props.value.includes("\r\n")
      ? monaco.editor.EndOfLineSequence.CRLF
      : monaco.editor.EndOfLineSequence.LF;
    // pushEditOperations / pushEOL keep it undoable
    model.pushEditOperations([], [{ range: model.getFullModelRange(), text: props.value }], () => null);
    model.pushEOL(eol);
    e.setPosition({ lineNumber: 1, column: 1 });
    e.revealLine(1);
  }, [props.value]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly: props.readOnly });
  }, [props.readOnly]);

  return <div className="monaco-host" ref={host} />;
});

export default ConfigEditor;
