// Dark theme for the bundled upstream help pages. Their Help.css (synced from
// upstream, so not edited here) has fixed light colors; in dark mode we add an
// override stylesheet to each page as it loads, and keep it in step with
// theme changes while the page stays open.
import { isDarkTheme } from "../prefs/appearance";

const STYLE_ID = "pinscape-cross-dark";
const ATTR = "data-pc-dark";

// Mirrors the selectors in Help.css; colors follow the app's dark tokens (styles.css).
const DARK_CSS = `
:root[${ATTR}] { color-scheme: dark; }
:root[${ATTR}], :root[${ATTR}] table { color: #e6e8eb; background-color: #1d2026; }
:root[${ATTR}] a:link { color: #7cb2ff; }
:root[${ATTR}] a:visited { color: #b49cff; }
:root[${ATTR}] #TopNav { background: #23272e; }
:root[${ATTR}] h1, :root[${ATTR}] h2, :root[${ATTR}] h3, :root[${ATTR}] h4 { color: #8ab4ff; }
:root[${ATTR}] h1 { border-bottom-color: #3b4a6b; }
:root[${ATTR}] h2 { border-top-color: #2e333b; }
:root[${ATTR}] h2.mode { background: #23272e; border-top-color: #4b5563; }
:root[${ATTR}] .shift-green, :root[${ATTR}] .shift-gray { background: #23272e; border-color: #3a4049; }
:root[${ATTR}] .shift-green { color: #4ade80; }
:root[${ATTR}] .shift-gray { color: #9aa1ab; }
:root[${ATTR}] table.data td, :root[${ATTR}] table.data th,
:root[${ATTR}] .jsonProp table td, :root[${ATTR}] .jsonProp table th { border-color: #2e333b; }
:root[${ATTR}] .jsonSectTitle { border-top-color: #3a4049; }
:root[${ATTR}] .jsonExampleHdr, :root[${ATTR}] .jsonSectIndexHdr,
:root[${ATTR}] .jsonSectIndex .propName, :root[${ATTR}] .jsonSectIndex .propName a:link,
:root[${ATTR}] .jsonSectIndex .propName a:visited, :root[${ATTR}] .jsonExample { color: #7cb2ff; }
:root[${ATTR}] .jsonSectIndex .typeNames, :root[${ATTR}] .jsonEnumVals { color: #f2777c; }
:root[${ATTR}] .jsonExample { border-color: #2e333b; background: #181b20; }
:root[${ATTR}] .jsonProp { border-color: #2a3240; }
:root[${ATTR}] .jsonPropHdr { background: #1e2a3a; }
:root[${ATTR}] .jsonPropHdr > .jsonPropPath { color: #9aa1ab; }
:root[${ATTR}] .jsonPropHdr > .jsonPropRequired { color: #f2777c; }
:root[${ATTR}] .jsonPropHdr > .jsonPropOptional { color: #4ade80; }
:root[${ATTR}] .jsonSee { color: #d8a4f0; }
:root[${ATTR}] #outputSourceTable .rowGroup { background: #2e2b1c; }
:root[${ATTR}] [style*="color: red"] { color: #f2777c !important; }
/* Wiring diagrams and screenshots are drawn for a white page. */
:root[${ATTR}] img { background: #ffffff; }
`;

const docs = new Set<Document>();

function apply(doc: Document) {
  const root = doc.documentElement;
  if (!root) return;
  if (!doc.getElementById(STYLE_ID)) {
    const style = doc.createElement("style");
    style.id = STYLE_ID;
    style.textContent = DARK_CSS;
    (doc.head ?? root).appendChild(style);
  }
  root.toggleAttribute(ATTR, isDarkTheme());
}

function refreshAll() {
  for (const doc of docs) {
    if (!doc.defaultView) docs.delete(doc); // frame navigated away or closed
    else apply(doc);
  }
}

let listening = false;

/** Style a same-origin help page for the current theme, and follow later changes. */
export function themeHelpPage(doc: Document) {
  docs.add(doc);
  apply(doc);
  if (!listening) {
    listening = true;
    window.addEventListener("pinscape-theme-change", refreshAll);
    window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", refreshAll);
  }
}
