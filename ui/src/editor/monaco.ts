// Monaco setup: the editor core (no bundled languages) plus our
// "pinscape-json" language, whose diagnostics, completion and hover come
// from the Rust pinscape-config crate (same parser rules as the firmware).

import * as monaco from "monaco-editor/esm/vs/editor/edcore.main";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { api, EditorRange } from "../api";

export { monaco };
export const LANG = "pinscape-json";

(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

export const toRange = (r: EditorRange) => new monaco.Range(r.startLine, r.startCol, r.endLine, r.endCol);

let registered = false;

/** Register the language once. */
export function registerLanguage() {
  if (registered) return;
  registered = true;

  monaco.languages.register({ id: LANG, extensions: [".json", ".json5"], aliases: ["Pinscape JSON"] });

  monaco.languages.setLanguageConfiguration(LANG, {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"', notIn: ["string", "comment"] },
      { open: "'", close: "'", notIn: ["string", "comment"] },
      { open: "/*", close: " */", notIn: ["string"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    indentationRules: {
      increaseIndentPattern: /^.*(\{[^}"']*|\[[^\]"']*)$/,
      decreaseIndentPattern: /^\s*[}\]]/,
    },
    onEnterRules: [
      // continue block comments
      { beforeText: /^\s*\/\*\*(?!\/)([^*]|\*(?!\/))*$/, afterText: /^\s*\*\/$/, action: { indentAction: monaco.languages.IndentAction.IndentOutdent, appendText: " * " } },
    ],
  });

  monaco.languages.setMonarchTokensProvider(LANG, {
    defaultToken: "",
    tokenizer: {
      root: [
        [/\/\/.*$/, "comment"],
        [/\/\*/, "comment", "@comment"],
        // property names: identifier or quoted string followed by ':'
        [/[A-Za-z_$][\w$]*(?=\s*:)/, "key"],
        [/"(?:[^"\\]|\\.)*"(?=\s*:)/, "key"],
        [/'(?:[^'\\]|\\.)*'(?=\s*:)/, "key"],
        [/\b(?:true|false|null|undefined)\b/, "keyword"],
        [/[+-]?0[xX][0-9a-fA-F]+/, "number.hex"],
        [/[+-]?0[bB][01]+/, "number"],
        [/[+-]?0[oO][0-7]+/, "number"],
        [/[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/, "number"],
        [/"/, "string", "@dstring"],
        [/'/, "string", "@sstring"],
        [/[{}[\]()]/, "@brackets"],
        [/[,:]/, "delimiter"],
        [/[A-Za-z_$][\w$]*/, "identifier"],
      ],
      comment: [
        [/[^*/]+/, "comment"],
        [/\*\//, "comment", "@pop"],
        [/[*/]/, "comment"],
      ],
      dstring: [
        [/[^\\"]+$/, "string", "@pop"], // unterminated at end of line
        [/[^\\"]+/, "string"],
        [/\\./, "string.escape"],
        [/"/, "string", "@pop"],
      ],
      sstring: [
        [/[^\\']+$/, "string", "@pop"],
        [/[^\\']+/, "string"],
        [/\\./, "string.escape"],
        [/'/, "string", "@pop"],
      ],
    },
  } as monaco.languages.IMonarchLanguage);

  monaco.editor.defineTheme("pinscape-light", {
    base: "vs",
    inherit: true,
    rules: [
      { token: "key", foreground: "0451A5" },
      { token: "string", foreground: "A31515" },
      { token: "number", foreground: "098658" },
      { token: "number.hex", foreground: "098658" },
      { token: "keyword", foreground: "0000FF" },
      { token: "comment", foreground: "6A8759", fontStyle: "italic" },
    ],
    colors: {},
  });
  monaco.editor.defineTheme("pinscape-dark", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "key", foreground: "9CDCFE" },
      { token: "string", foreground: "CE9178" },
      { token: "number", foreground: "B5CEA8" },
      { token: "number.hex", foreground: "B5CEA8" },
      { token: "keyword", foreground: "569CD6" },
      { token: "comment", foreground: "6A9955", fontStyle: "italic" },
    ],
    colors: { "editor.background": "#1d2026" },
  });

  monaco.languages.registerCompletionItemProvider(LANG, {
    triggerCharacters: [":", '"', " "],
    async provideCompletionItems(model, pos) {
      const items = await api.configCompletions(model.getValue(), pos.lineNumber, pos.column);
      return {
        suggestions: items.map((c) => ({
          label: c.label,
          kind: c.kind === "property" ? monaco.languages.CompletionItemKind.Property : monaco.languages.CompletionItemKind.Value,
          detail: c.detail ?? undefined,
          documentation: c.documentation ? { value: c.documentation } : undefined,
          insertText: c.insertText,
          insertTextRules: c.isSnippet ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
          range: toRange(c.range),
          sortText: c.sortText,
        })),
      };
    },
  });

  monaco.languages.registerHoverProvider(LANG, {
    async provideHover(model, pos) {
      const h = await api.configHover(model.getValue(), pos.lineNumber, pos.column);
      if (!h) return null;
      return {
        range: toRange(h.range),
        contents: [{ value: h.markdown }, { value: "_F1: open the full reference for this property_" }],
      };
    },
  });
}

/** Light/dark theme following the OS setting. */
export function currentTheme() {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "pinscape-dark" : "pinscape-light";
}
