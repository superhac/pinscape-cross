// External links open in the system browser (tauri-plugin-opener). The
// allowed URLs are limited in src-tauri/capabilities/default.json: any https
// URL, plus http on mjrnet.org (the upstream help pages link there).
import { openUrl } from "@tauri-apps/plugin-opener";

export const REPO_URL = "https://github.com/superhac/pinscape-cross";
export const UPSTREAM_URL = "https://github.com/mjrgh/PinscapePico";

let reportError: ((msg: string) => void) | null = null;

/** Where failures of links opened from help pages are reported (status bar). */
export function onLinkError(cb: ((msg: string) => void) | null) {
  reportError = cb;
}

export const isWebUrl = (url: string) => /^https?:\/\//i.test(url);

/** Open an http(s) URL in the default browser. */
export async function openExternal(url: string): Promise<void> {
  if (!isWebUrl(url)) throw new Error(`not a web link: ${url}`);
  try {
    await openUrl(url);
  } catch (e) {
    reportError?.(`Couldn't open ${url}: ${e}`);
    throw e;
  }
}

/** onClick for <a href> elements that should open externally. */
export function externalClick(e: React.MouseEvent<HTMLAnchorElement>) {
  e.preventDefault();
  openExternal(e.currentTarget.href).catch(() => {});
}

/**
 * `onLoad` handler for same-origin help iframes: clicks on links that leave
 * the app open in the system browser instead of navigating the frame, and
 * shortcut keys pressed inside the frame reach the app's key handler.
 */
export function hookIframe(e: React.SyntheticEvent<HTMLIFrameElement>) {
  const frame = e.currentTarget;
  let doc: Document | null = null;
  try {
    doc = frame.contentDocument;
  } catch {
    return; // cross-origin: nothing we can do
  }
  if (!doc) return;
  const own = frame.contentWindow?.location.origin;

  const onClick = (ev: MouseEvent) => {
    if (ev.button > 1) return;
    const a = (ev.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    if (!a) return;
    let url: URL;
    try {
      url = new URL(a.href, doc!.baseURI);
    } catch {
      return;
    }
    if ((url.protocol === "http:" || url.protocol === "https:") && url.origin !== own) {
      ev.preventDefault();
      ev.stopPropagation();
      openExternal(url.href).catch(() => {});
    }
  };
  doc.addEventListener("click", onClick, true);
  doc.addEventListener("auxclick", onClick, true);

  // Forward modifier/function-key presses to the main window's shortcut handler.
  doc.addEventListener("keydown", (ev) => {
    if (!(ev.ctrlKey || ev.metaKey || ev.altKey || /^F\d+$/.test(ev.key) || ev.key === "Escape")) return;
    const copy = new KeyboardEvent("keydown", {
      key: ev.key,
      code: ev.code,
      ctrlKey: ev.ctrlKey,
      metaKey: ev.metaKey,
      shiftKey: ev.shiftKey,
      altKey: ev.altKey,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(copy);
    if (copy.defaultPrevented) ev.preventDefault();
  });
}
