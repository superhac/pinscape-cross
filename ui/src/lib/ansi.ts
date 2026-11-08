// Minimal ANSI SGR → styled spans converter for the device log, which uses
// basic 16-color escapes (e.g. "\x1b[36m"). Unknown sequences are dropped.

export interface Span {
  text: string;
  fg?: string;
  bold?: boolean;
}

const COLORS: Record<number, string> = {
  30: "ansi-black", 31: "ansi-red", 32: "ansi-green", 33: "ansi-yellow",
  34: "ansi-blue", 35: "ansi-magenta", 36: "ansi-cyan", 37: "ansi-white",
  90: "ansi-bright-black", 91: "ansi-bright-red", 92: "ansi-bright-green", 93: "ansi-bright-yellow",
  94: "ansi-bright-blue", 95: "ansi-bright-magenta", 96: "ansi-bright-cyan", 97: "ansi-bright-white",
};

export interface AnsiState {
  fg?: string;
  bold?: boolean;
}

/** Parse `text`, continuing from `state` (mutated) so colors carry across chunks. */
export function parseAnsi(text: string, state: AnsiState): Span[] {
  const spans: Span[] = [];
  const re = /\x1b\[([0-9;]*)m/g;
  let last = 0;
  let m: RegExpExecArray | null;
  const push = (t: string) => {
    if (t) spans.push({ text: t, fg: state.fg, bold: state.bold });
  };
  while ((m = re.exec(text))) {
    push(text.slice(last, m.index));
    last = re.lastIndex;
    const codes = m[1] === "" ? [0] : m[1].split(";").map(Number);
    for (const c of codes) {
      if (c === 0) {
        state.fg = undefined;
        state.bold = false;
      } else if (c === 1) state.bold = true;
      else if (c === 22) state.bold = false;
      else if (c === 39) state.fg = undefined;
      else if (COLORS[c]) state.fg = COLORS[c];
    }
  }
  // strip any other control sequences that slipped through
  push(text.slice(last).replace(/\x1b\[[0-9;?]*[A-Za-z]/g, ""));
  return spans;
}
