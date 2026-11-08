import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { BootDevice, chipName, firmwareApi } from "../lib/api-firmware";
import { useDevices } from "../lib/useDevices";
import Installer, { InstallTarget } from "./Installer";
import InstallProgress from "./InstallProgress";
import { dropsClaimed, installRunning, useInstallState } from "./store";

const isUf2 = (p: string) => p.toLowerCase().endsWith(".uf2");
const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/**
 * Sidebar section: Picos in boot loader mode, with "Install firmware…" (new-device
 * setup). Always mounted, so it also handles a .uf2 dropped on the window from any
 * tab (unless an open installer takes it), and hosts the install progress dialog.
 */
export default function BootLoaderList({ onStatus }: { onStatus: (s: string) => void }) {
  const [devices, setDevices] = useState<BootDevice[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [dropped, setDropped] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const pinscape = useDevices().filter((d) => d.ids);
  const inst = useInstallState();

  useEffect(() => {
    let alive = true;
    const un = firmwareApi.onBootLoadersChanged((d) => alive && setDevices(d));
    firmwareApi
      .bootList()
      .then((l) => alive && setDevices(l.devices))
      .catch(() => {});
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);

  // A .uf2 dropped anywhere opens the install flow (if no open installer claims it).
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      const free = !dropsClaimed() && !installRunning();
      if (p.type === "enter") setDragging(free && p.paths.some(isUf2));
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        const f = p.paths.find(isUf2);
        if (f && free) {
          setSelected(null);
          setDropped(f);
        }
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  const sel = devices.find((d) => d.key === selected) ?? null;
  // The progress dialog takes over once an install starts; drop a vanished device.
  useEffect(() => {
    if (selected && (inst.running || !sel)) setSelected(null);
  }, [inst.running, sel, selected]);
  useEffect(() => {
    if (dropped && inst.running) setDropped(null);
  }, [dropped, inst.running]);

  // Targets for a dropped file: Picos in boot loader mode first (new-device setup), then Pinscape devices.
  const targets: { id: string; label: string; target: InstallTarget }[] = [
    ...devices.map((b) => ({
      id: b.key,
      label: `Pico in boot loader mode (${chipName(b.chip)})${b.serial ? ` · ${b.serial}` : ""}`,
      target: { kind: "boot", boot: b } as InstallTarget,
    })),
    ...pinscape.map((d) => ({
      id: d.key,
      label: `#${d.ids!.unit_num} ${d.ids!.unit_name} (${d.ids!.hwid})`,
      target: { kind: "device", dev: d } as InstallTarget,
    })),
  ];
  const chosen = targets.find((t) => t.id === dropTarget) ?? targets[0] ?? null;

  const restart = async (d: BootDevice) => {
    try {
      await firmwareApi.bootReboot(d.key);
      onStatus("Pico restarted.");
      setSelected(null);
    } catch (e) {
      onStatus(`Restart failed: ${e}`);
    }
  };

  return (
    <>
      {devices.length > 0 && (
        <>
          <p className="muted small sidebar-section">Boot loader mode</p>
          <ul>
            {devices.map((d) => (
              <li key={d.key} className="bootloader" title={d.serial ?? undefined}>
                <strong>Pico in boot loader ({chipName(d.chip)})</strong>
                <span className="muted small">
                  {d.serial ?? "no serial"}
                  {!d.picoboot && " · drive copy only"}
                </span>
                <button className="small-btn" onClick={() => setSelected(d.key)} disabled={inst.running}>
                  Install firmware…
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {sel && (
        <div className="modal-backdrop">
          <div className="modal firmware-modal" role="dialog" aria-modal="true">
            <h2>Install firmware — Pico in boot loader ({chipName(sel.chip)})</h2>
            <Installer target={{ kind: "boot", boot: sel }} active onStatus={onStatus} />
            <div className="modal-actions">
              {sel.picoboot && (
                <button onClick={() => restart(sel)} title="Leave the boot loader and run whatever is in flash">
                  Restart Pico
                </button>
              )}
              <span className="spacer" />
              <button onClick={() => setSelected(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
      {dropped && !sel && (
        <div className="modal-backdrop">
          <div className="modal firmware-modal" role="dialog" aria-modal="true">
            <h2>Install {fileName(dropped)}</h2>
            {targets.length === 0 ? (
              <p className="muted">
                No Pico is connected. Plug in a Pinscape Pico, or hold BOOTSEL while plugging in a new one; it appears here
                automatically.
              </p>
            ) : (
              <>
                {targets.length > 1 && (
                  <div className="actions drop-target">
                    <label className="inline">
                      Install on
                      <select value={chosen!.id} onChange={(e) => setDropTarget(e.target.value)}>
                        {targets.map((t) => (
                          <option key={t.id} value={t.id}>
                            {t.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                )}
                <Installer key={chosen!.id} target={chosen!.target} active initialPath={dropped} onStatus={onStatus} showErase={false} />
              </>
            )}
            <div className="modal-actions">
              <span className="spacer" />
              <button onClick={() => setDropped(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
      {dragging && (
        <div className="drop-overlay" aria-hidden>
          <div>Drop the .uf2 file to install firmware</div>
        </div>
      )}
      <InstallProgress />
    </>
  );
}
