// Print a text document (the config) as a monospace, line-numbered listing
// with a header. The listing is a print-only element; @media print in
// styles.css hides the app while it's present.
import { api } from "../api";

export async function printListing(opts: { title: string; subtitle?: string; text: string }) {
  document.getElementById("print-root")?.remove();
  const root = document.createElement("div");
  root.id = "print-root";

  const head = document.createElement("header");
  const h = document.createElement("h1");
  h.textContent = opts.title;
  head.appendChild(h);
  const sub = document.createElement("p");
  sub.textContent = [opts.subtitle, `Printed ${new Date().toLocaleString()}`].filter(Boolean).join(" · ");
  head.appendChild(sub);
  root.appendChild(head);

  const lines = opts.text.replace(/\r\n?/g, "\n").split("\n");
  if (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  const width = String(lines.length).length;
  const body = document.createElement("div");
  body.className = "print-lines";
  body.style.setProperty("--ln-width", `${width + 1}ch`);
  lines.forEach((l, i) => {
    const row = document.createElement("div");
    const n = document.createElement("span");
    n.className = "ln";
    n.textContent = String(i + 1);
    const t = document.createElement("span");
    t.className = "lt";
    t.textContent = l || " ";
    row.append(n, t);
    body.appendChild(row);
  });
  root.appendChild(body);

  document.body.appendChild(root);
  document.body.classList.add("printing");
  const cleanup = () => {
    document.body.classList.remove("printing");
    root.remove();
    window.removeEventListener("afterprint", cleanup);
  };
  window.addEventListener("afterprint", cleanup);
  // Let the listing lay out before the print dialog snapshots the page.
  await new Promise((r) => requestAnimationFrame(() => r(null)));
  try {
    // The native print (Tauri) works on every platform; window.print() isn't
    // implemented by all webviews (e.g. WKWebView on macOS).
    await api.printWindow();
  } catch {
    window.print();
  }
  // The listing stays in the DOM (display:none on screen) until afterprint or
  // the next print: the native call can return before the page is rendered
  // for printing, and some webviews never fire afterprint.
}
