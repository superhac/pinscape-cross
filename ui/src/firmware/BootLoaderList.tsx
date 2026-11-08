import { useEffect, useState } from "react";
import { BootDevice, chipName, firmwareApi } from "../lib/api-firmware";
import Installer from "./Installer";
import InstallProgress from "./InstallProgress";
import { useInstallState } from "./store";

/** Sidebar section: Picos in boot loader mode, with "Install firmware…" (new-device setup). */
export default function BootLoaderList({ onStatus }: { onStatus: (s: string) => void }) {
  const [devices, setDevices] = useState<BootDevice[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
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

  const sel = devices.find((d) => d.key === selected) ?? null;
  // The progress dialog takes over once an install starts; drop a vanished device.
  useEffect(() => {
    if (selected && (inst.running || !sel)) setSelected(null);
  }, [inst.running, sel, selected]);

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
      <InstallProgress />
    </>
  );
}
