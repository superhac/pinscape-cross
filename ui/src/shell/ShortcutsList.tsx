import { emacsKeyList, emacsNote, shortcutList } from "./shortcuts";
import { useIsMac } from "./platform";

function KeyTable({ caption, rows }: { caption: string; rows: { keys: string; what: string }[] }) {
  return (
    <table className="grid shortcut-table">
      <caption>{caption}</caption>
      <thead>
        <tr>
          <th scope="col">Keys</th>
          <th scope="col">Action</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.keys}>
            <td>
              <kbd>{r.keys}</kbd>
            </td>
            <td>{r.what}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Keyboard shortcut reference (Help tab). */
export default function ShortcutsList() {
  const mac = useIsMac();
  return (
    <div className="shortcuts">
      <KeyTable caption="Keyboard shortcuts" rows={shortcutList(mac)} />
      <KeyTable caption="Emacs-style editor keys (turn on in Preferences › Editor)" rows={emacsKeyList()} />
      <p className="muted small">{emacsNote(mac)}</p>
    </div>
  );
}
