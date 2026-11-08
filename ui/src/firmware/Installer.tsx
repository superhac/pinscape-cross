import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { DeviceSnapshot, fmtVersion } from "../api";
import { confirm } from "../lib/confirm";
import {
  BootDevice,
  Chip,
  chipName,
  chipOfCpu,
  firmwareApi,
  hex8,
  InstallMethod,
  RELEASES_URL,
  Uf2Summary,
} from "../lib/api-firmware";
import { claimDrops, startInstall, useInstallState } from "./store";
import EraseCard from "./EraseCard";
import "./firmware.css";

export type InstallTarget = { kind: "device"; dev: DeviceSnapshot } | { kind: "boot"; boot: BootDevice };

interface Props {
  target: InstallTarget;
  /** Listen for dropped .uf2 files only while shown. */
  active: boolean;
  onStatus?: (s: string) => void;
  /** A .uf2 to load right away (e.g. one dropped on the window from another tab). */
  initialPath?: string | null;
  /** Show the "Erase flash" section (default true). */
  showErase?: boolean;
}

const boardOf = (c: Chip) => (c === "rp2040" ? "Pico" : "Pico 2");

/** Choose a .uf2, check it against the target Pico, and install it. */
export default function Installer({ target, active, onStatus, initialPath, showErase = true }: Props) {
  const [uf2, setUf2] = useState<Uf2Summary | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [verify, setVerify] = useState(true);
  const [method, setMethod] = useState<InstallMethod>("auto");
  const [copied, setCopied] = useState(false);
  const inst = useInstallState();

  const chip: Chip | null = target.kind === "device" ? chipOfCpu(target.dev.ids?.cpu_type ?? 0) : target.boot.chip;
  const picobootOk = target.kind === "device" || target.boot.picoboot;

  const load = async (path: string) => {
    setLoadErr(null);
    try {
      setUf2(await firmwareApi.inspect(path));
    } catch (e) {
      setUf2(null);
      setLoadErr(String(e));
    }
  };

  useEffect(() => {
    if (initialPath) load(initialPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialPath]);

  // Drop a .uf2 file onto the window (while shown, this installer takes drops
  // instead of the global handler in BootLoaderList).
  useEffect(() => {
    if (!active) return;
    const release = claimDrops();
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter") setDragging(p.paths.some((x) => x.toLowerCase().endsWith(".uf2")));
      else if (p.type === "leave") setDragging(false);
      else if (p.type === "drop") {
        setDragging(false);
        const f = p.paths.find((x) => x.toLowerCase().endsWith(".uf2"));
        if (f) load(f);
        else if (p.paths.length) setLoadErr("Drop a .uf2 firmware file.");
      }
    });
    return () => {
      release();
      un.then((f) => f());
    };
  }, [active]);

  const choose = async () => {
    const f = await openDialog({ multiple: false, directory: false, filters: [{ name: "UF2 firmware", extensions: ["uf2", "UF2"] }] });
    if (typeof f === "string") load(f);
  };

  const mismatch = uf2 && chip && !uf2.chips.includes(chip);
  const blocked = !uf2 || !chip || !!mismatch || !!uf2.planError || inst.running;
  const targetLabel =
    target.kind === "device"
      ? `#${target.dev.ids?.unit_num} ${target.dev.ids?.unit_name} (${chip ? chipName(chip) : "?"})`
      : `the ${chipName(target.boot.chip)} Pico in boot loader mode`;

  const install = async () => {
    if (!uf2 || !chip) return;
    const range = uf2.flashStart != null && uf2.flashEnd != null ? ` (${hex8(uf2.flashStart)}–${hex8(uf2.flashEnd)})` : "";
    const steps =
      target.kind === "device"
        ? "The device restarts into its boot loader, the firmware area of flash is erased and rewritten, and then it restarts with the new firmware."
        : "The firmware area of flash is erased and rewritten, and then the Pico restarts with the new firmware.";
    const ok = await confirm({
      title: "Install firmware?",
      body:
        `Install ${uf2.fileName} on ${targetLabel}? ${steps} ` +
        `Only the ${uf2.sectors ?? "?"} flash sectors the file covers${range} are erased; your configuration is stored ` +
        `separately at the top of flash and is kept. Don't unplug the Pico until the update finishes.`,
      ok: "Install",
      danger: true,
    });
    if (!ok) return;
    if (uf2.kind !== "pinscape") {
      const ok2 = await confirm({
        title: "Not Pinscape firmware?",
        body:
          uf2.kind === "pwmworker"
            ? "This file looks like the PWMWorker program, which belongs on a Pico in an expansion board's output chain, not on your main Pinscape Pico. Install it anyway?"
            : "This file doesn't look like Pinscape Pico firmware. After installing it, the device won't appear in this app until you install Pinscape again. Continue?",
        ok: "Install anyway",
        danger: true,
      });
      if (!ok2) return;
    }
    onStatus?.(`Installing ${uf2.fileName}…`);
    const r = await startInstall(
      {
        path: uf2.path,
        deviceKey: target.kind === "device" ? target.dev.key : null,
        bootKey: target.kind === "boot" ? target.boot.key : null,
        method: picobootOk ? method : "volume",
        verify,
      },
      `Installing ${uf2.fileName}`,
    );
    onStatus?.(r ? `Firmware installed${r.version ? `: v${fmtVersion(r.version)}` : ""}.` : "Firmware installation failed.");
  };

  return (
    <div className="firmware">
      <section className="card">
        <h3>Current firmware</h3>
        {target.kind === "device" ? (
          <p>
            <strong>{target.dev.version ? `Pinscape Pico v${fmtVersion(target.dev.version)}` : "unknown version"}</strong>
            {target.dev.version && <span className="muted"> · build {target.dev.version.build_date}</span>}
            <span className="muted">
              {" "}
              · {chip ? `${chipName(chip)} (${boardOf(chip)})` : `RP${target.dev.ids?.cpu_type}`}
            </span>
          </p>
        ) : (
          <p>
            <strong>Pico in boot loader mode ({chipName(target.boot.chip)})</strong>
            <span className="muted"> · no Pinscape firmware running. Install the {boardOf(target.boot.chip)} build.</span>
          </p>
        )}
        <p className="muted small">
          Download firmware from the Pinscape Pico releases page: <code className="selectable">{RELEASES_URL}</code>{" "}
          <button className="small-btn" onClick={() => navigator.clipboard?.writeText(RELEASES_URL).then(() => setCopied(true))}>
            {copied ? "Copied" : "Copy link"}
          </button>
          <br />
          Use <code>PinscapePico-pico.uf2</code> for a Pico (RP2040) and <code>PinscapePico-pico2.uf2</code> for a Pico 2 (RP2350).
        </p>
      </section>

      <section className={"card dropzone" + (dragging ? " dragging" : "")}>
        <h3>Firmware file</h3>
        <div className="actions">
          <button onClick={choose} disabled={inst.running}>
            Choose .uf2 file…
          </button>
          <span className="muted small">or drop a .uf2 file onto this window</span>
        </div>
        {loadErr && <p className="error">{loadErr}</p>}
        {uf2 && <Uf2Details uf2={uf2} chip={chip} />}
      </section>

      {uf2 && (
        <section className="card">
          <h3>Install</h3>
          {mismatch && chip && (
            <p className="error">
              This file is for {uf2.chips.map(chipName).join(" / ") || "another device"}, but this Pico is an {chipName(chip)} (
              {boardOf(chip)}). Installing it is refused.
            </p>
          )}
          {uf2.planError && <p className="error">Can't install this file: {uf2.planError}</p>}
          <div className="actions">
            <label className="inline">
              <input type="checkbox" checked={verify} onChange={(e) => setVerify(e.target.checked)} disabled={inst.running} /> Verify
              after writing
            </label>
            <label className="inline">
              Method
              <select value={picobootOk ? method : "volume"} onChange={(e) => setMethod(e.target.value as InstallMethod)} disabled={inst.running || !picobootOk}>
                <option value="auto">Automatic</option>
                <option value="picoboot">USB (PICOBOOT) only</option>
                <option value="volume">Copy to boot drive</option>
              </select>
            </label>
            <span className="spacer" />
            <button className="danger" disabled={blocked} onClick={install}>
              Install firmware…
            </button>
          </div>
          <p className="muted small">
            Automatic writes the firmware over USB (PICOBOOT) with progress and verification, and falls back to copying the file
            onto the Pico's boot drive (RPI-RP2 / RP2350) if the USB interface can't be opened
            {chip === "rp2040" ? " — which is always the case on Windows for an RP2040 without a WinUSB driver" : ""}. Verify only
            applies to the USB method.
          </p>
        </section>
      )}

      {showErase && <EraseCard target={target} onStatus={onStatus} />}
    </div>
  );
}

function Uf2Details({ uf2, chip }: { uf2: Uf2Summary; chip: Chip | null }) {
  const rows: [string, string][] = [
    ["File", uf2.fileName],
    ["Size", `${uf2.fileBytes.toLocaleString()} bytes · ${uf2.totalBlocks.toLocaleString()} blocks`],
    [
      "Contents",
      uf2.kind === "pinscape" ? "Pinscape Pico firmware" : uf2.kind === "pwmworker" ? "PWMWorker (expansion board program)" : "unknown program",
    ],
    ["For", uf2.chips.length ? uf2.chips.map((c) => `${chipName(c)} (${boardOf(c)})`).join(", ") : "—"],
    ...uf2.families.map(
      (f) =>
        [
          `Family ${f.name}`,
          `${hex8(f.id)} · ${f.blocks.toLocaleString()} blocks · ${f.payloadBytes.toLocaleString()} bytes · ${hex8(f.minAddr)}–${hex8(f.endAddr)}`,
        ] as [string, string],
    ),
  ];
  if (uf2.sectors != null) rows.push(["Flash erased", `${uf2.sectors} × 4 KB sectors (${((uf2.sectors * 4096) / 1024).toLocaleString()} KB)`]);
  if (uf2.e10Block) rows.push(["Note", "includes the RP2350-E10 workaround block (skipped when writing over USB)"]);
  const warnings = uf2.warnings;
  return (
    <>
      <table className="kv">
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <th>{k}</th>
              <td>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {chip && uf2.chips.includes(chip) && uf2.kind === "pinscape" && !uf2.planError && (
        <p className="ok small">Compatible with this Pico.</p>
      )}
      {warnings.map((w) => (
        <p key={w} className="warn small">
          {w}
        </p>
      ))}
    </>
  );
}
