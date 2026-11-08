// A small set of Emacs-style keys for the config editor, active while the
// `pinscapeEmacs` context key is true (Preferences › Editor):
//   Ctrl+A / Ctrl+E   start / end of line
//   Ctrl+F / Ctrl+B   forward / back one character
//   Ctrl+N / Ctrl+P   next / previous line
//   Ctrl+D            delete the character after the cursor
//   Ctrl+K            kill to end of line (joins the next line at line end);
//                     consecutive kills are collected into one kill buffer
//   Ctrl+Y            yank the kill buffer
// On macOS these use the real Control key (Cmd shortcuts are untouched).
import { monaco } from "./monaco";

export const EMACS_CONTEXT = "pinscapeEmacs";

let killBuffer = "";

export function installEmacsKeys(e: monaco.editor.IStandaloneCodeEditor, mac: boolean): monaco.editor.IContextKey<boolean> {
  const key = e.createContextKey<boolean>(EMACS_CONTEXT, false);
  // KeyMod.WinCtrl is the Control key on macOS; elsewhere CtrlCmd is Control.
  const C = mac ? monaco.KeyMod.WinCtrl : monaco.KeyMod.CtrlCmd;
  const K = monaco.KeyCode;

  // Track whether the previous command was a kill at the same spot, so
  // repeated Ctrl+K appends to the buffer like Emacs.
  let lastKillAt: string | null = null;
  const posKey = () => {
    const p = e.getPosition();
    return p ? `${e.getModel()?.getVersionId()}:${p.lineNumber}:${p.column}` : null;
  };

  const move = (cmd: string) => () => {
    lastKillAt = null;
    e.trigger("emacs", cmd, null);
  };
  const bind = (k: number, f: () => void) => e.addCommand(C | k, f, EMACS_CONTEXT);

  bind(K.KeyA, move("cursorLineStart"));
  bind(K.KeyE, move("cursorLineEnd"));
  bind(K.KeyF, move("cursorRight"));
  bind(K.KeyB, move("cursorLeft"));
  bind(K.KeyN, move("cursorDown"));
  bind(K.KeyP, move("cursorUp"));
  bind(K.KeyD, move("deleteRight"));

  bind(K.KeyK, () => {
    const model = e.getModel();
    const p = e.getPosition();
    if (!model || !p || e.getOption(monaco.editor.EditorOption.readOnly)) return;
    const end = model.getLineMaxColumn(p.lineNumber);
    let range: monaco.Range;
    if (p.column < end) range = new monaco.Range(p.lineNumber, p.column, p.lineNumber, end);
    else if (p.lineNumber < model.getLineCount()) range = new monaco.Range(p.lineNumber, p.column, p.lineNumber + 1, 1);
    else return;
    const text = model.getValueInRange(range);
    const append = lastKillAt !== null && lastKillAt === posKey();
    killBuffer = append ? killBuffer + text : text;
    e.pushUndoStop();
    e.executeEdits("emacs", [{ range, text: "" }]);
    e.setPosition(p);
    lastKillAt = posKey();
  });

  bind(K.KeyY, () => {
    lastKillAt = null;
    const sel = e.getSelection();
    if (!sel || !killBuffer || e.getOption(monaco.editor.EditorOption.readOnly)) return;
    e.pushUndoStop();
    e.executeEdits("emacs", [{ range: sel, text: killBuffer, forceMoveMarkers: true }]);
    e.pushUndoStop();
  });

  return key;
}
