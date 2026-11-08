import { useState } from "react";
import { fmtVersion } from "../api";
import { confirm } from "../lib/confirm";
import { chipName, EraseResult, EraseTarget, firmwareApi, hex8, InstallResult } from "../lib/api-firmware";
import type { InstallTarget } from "./Installer";
import { startJob, useInstallState } from "./store";

const SIZES: [string, number | null][] = [
  ["Automatic", null],
  ["2 MB", 2 << 20],
  ["4 MB", 4 << 20],
  ["8 MB", 8 << 20],
  ["16 MB", 16 << 20],
];

const kb = (n: number) => `${(n / 1024).toLocaleString()} KB`;

/** Erase result → the shared progress dialog's result. */
function toResult(r: EraseResult): InstallResult {
  const p = r.plan;
  const ranges = p.ranges.map(([a, l]) => `${hex8(a)}–${hex8(a + l)}`).join(", ");
  const notes = [`Erased ${p.sectors.toLocaleString()} sector${p.sectors === 1 ? "" : "s"} (${kb(p.bytes)}): ${ranges}.`];
  if (p.target === "config" && p.files.length)
    notes.push(`Files removed: ${p.files.map((f) => (f.replaced ? "(old copy)" : f.name)).join(", ")}.`);
  notes.push(...r.notes);
  const headline =
    p.target === "config"
      ? r.version
        ? `The Pinscape config was erased. The device restarted with factory defaults (v${fmtVersion(r.version)}).`
        : "The Pinscape config was erased."
      : "The entire flash was erased.";
  return { method: "picoboot", deviceKey: r.deviceKey, version: r.version, notes, headline };
}

/** "Erase flash" section of the installer: config area or everything, over PICOBOOT. */
export default function EraseCard({ target, onStatus }: { target: InstallTarget; onStatus?: (s: string) => void }) {
  const inst = useInstallState();
  const [verify, setVerify] = useState(true);
  const [size, setSize] = useState<number | null>(null);
  const [typed, setTyped] = useState<null | ((ok: boolean) => void)>(null);
  const [word, setWord] = useState("");

  const isBoot = target.kind === "boot";
  const usable = !isBoot || target.boot.picoboot;
  const who =
    target.kind === "device"
      ? `#${target.dev.ids?.unit_num} ${target.dev.ids?.unit_name}`
      : `the ${chipName(target.boot.chip)} Pico in boot loader mode`;
  const reboot = target.kind === "device" ? " The device first restarts into its boot loader." : "";

  /** Second, typed confirmation for erasing everything. */
  const typeToConfirm = () =>
    new Promise<boolean>((resolve) => {
      setWord("");
      setTyped(() => resolve);
    });

  const run = async (what: EraseTarget) => {
    if (what === "config") {
      const ok = await confirm({
        title: "Erase the Pinscape config?",
        body:
          `This erases ALL configuration files and saved settings (main and Safe Mode config, plunger calibration, nudge ` +
          `settings) from ${who}, using the boot loader.${reboot} The firmware is kept, and Pinscape restarts with ` +
          `factory defaults. Erased data can't be recovered; export your config first if you may need it.`,
        ok: "Erase config…",
        danger: true,
      });
      if (!ok) return;
      const ok2 = await confirm({
        title: "Are you sure?",
        body: "Last chance: erase every Pinscape config file and saved setting on this Pico now?",
        ok: "Erase config",
        danger: true,
      });
      if (!ok2) return;
    } else {
      const ok = await confirm({
        title: "Erase the ENTIRE flash?",
        body:
          `This erases EVERYTHING in ${who}'s flash memory: the Pinscape firmware, all configuration files and all saved ` +
          `settings.${reboot} Afterwards the Pico has no program and stays in boot loader mode until you install firmware ` +
          `again. Nothing can be recovered.`,
        ok: "Continue…",
        danger: true,
      });
      if (!ok || !(await typeToConfirm())) return;
    }
    onStatus?.(what === "config" ? "Erasing the Pinscape config…" : "Erasing the entire flash…");
    const r = await startJob(what === "config" ? "Erasing the Pinscape config" : "Erasing the entire flash", async () =>
      toResult(
        await firmwareApi.erase({
          deviceKey: target.kind === "device" ? target.dev.key : null,
          bootKey: target.kind === "boot" ? target.boot.key : null,
          target: what,
          flashSize: size,
          verify,
        }),
      ),
    );
    onStatus?.(r ? r.headline ?? "Erased." : "Erasing failed.");
  };

  return (
    <section className="card erase-card">
      <h3>Erase flash</h3>
      <p className="muted small">
        Uses the Pico's boot loader, so it works even when the firmware or its config is broken. <strong>Erase config</strong>{" "}
        removes Pinscape's flash file system (the directory sector at the top of flash and every sector it lists) and keeps the
        firmware: a factory reset. <strong>Erase entire flash</strong> leaves a blank Pico.
      </p>
      {!usable && (
        <p className="warn small">This boot loader doesn't offer the USB (PICOBOOT) interface needed for erasing.</p>
      )}
      <div className="actions">
        <button className="danger" disabled={!usable || inst.running} onClick={() => run("config")}>
          Erase Pinscape config…
        </button>
        <button className="danger" disabled={!usable || inst.running} onClick={() => run("all")}>
          Erase entire flash…
        </button>
        <span className="spacer" />
        <label className="inline small">
          <input type="checkbox" checked={verify} onChange={(e) => setVerify(e.target.checked)} /> Verify
        </label>
        <label className="inline small" title="Normally detected from the chip; the config's location comes from the firmware build (2 MB Pico, 4 MB Pico 2)">
          Flash size
          <select value={size ?? ""} onChange={(e) => setSize(e.target.value ? Number(e.target.value) : null)}>
            {SIZES.map(([l, v]) => (
              <option key={l} value={v ?? ""}>
                {l}
              </option>
            ))}
          </select>
        </label>
      </div>

      {typed && (
        <div className="modal-backdrop">
          <div className="modal" role="dialog" aria-modal="true">
            <h2>Type ERASE to confirm</h2>
            <p>
              The firmware and all data on {who} will be erased. Type <strong>ERASE</strong> to continue.
            </p>
            <input
              autoFocus
              className="erase-word"
              value={word}
              onChange={(e) => setWord(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  typed(false);
                  setTyped(null);
                } else if (e.key === "Enter" && word === "ERASE") {
                  typed(true);
                  setTyped(null);
                }
              }}
              aria-label="Type ERASE"
            />
            <div className="modal-actions">
              <button
                onClick={() => {
                  typed(false);
                  setTyped(null);
                }}
              >
                Cancel
              </button>
              <button
                className="danger"
                disabled={word !== "ERASE"}
                onClick={() => {
                  typed(true);
                  setTyped(null);
                }}
              >
                Erase everything
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
