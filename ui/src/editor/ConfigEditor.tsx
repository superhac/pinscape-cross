import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { api, Breadcrumb, Diagnostic } from "../api";
import { currentTheme, LANG, monaco, registerLanguage, toRange } from "./monaco";

export interface ConfigEditorHandle {
  /** Move the cursor to a range and focus the editor. */
  reveal: (d: Diagnostic) => void;
  focus: () => void;
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
      e.setSelection(r);
      e.revealRangeInCenterIfOutsideViewport(r);
      e.focus();
    },
    focus: () => editor.current?.focus(),
  }));

  useEffect(() => {
    registerLanguage();
    const model = monaco.editor.createModel(props.value, LANG);
    const e = monaco.editor.create(host.current!, {
      model,
      theme: currentTheme(),
      automaticLayout: true,
      fontFamily: 'ui-monospace, "SF Mono", "Cascadia Mono", "DejaVu Sans Mono", monospace',
      fontSize: 13,
      tabSize: 4,
      insertSpaces: true,
      minimap: { enabled: true },
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
        const diags = await api.configValidate(text).catch(() => [] as Diagnostic[]);
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

    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onScheme = () => monaco.editor.setTheme(currentTheme());
    mq?.addEventListener("change", onScheme);

    validate();
    breadcrumb();

    return () => {
      clearTimeout(vTimer);
      clearTimeout(bTimer);
      subs.forEach((s) => s.dispose());
      mq?.removeEventListener("change", onScheme);
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
