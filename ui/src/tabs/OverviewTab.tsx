import { useState } from "react";
import { api, DeviceSnapshot, fmtBytes, fmtUptime, fmtVersion, hex4, Overview, ResetMode } from "../api";
import { useInterval } from "../lib/useDevices";
import { confirm } from "../lib/confirm";

interface Props {
  dev: DeviceSnapshot;
  onStatus: (s: string) => void;
}

export default function OverviewTab({ dev, onStatus }: Props) {
  const ids = dev.ids!;
  const [ov, setOv] = useState<Overview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [showAllGpio, setShowAllGpio] = useState(false);

  // Refresh every 2 s (uptime, loop timing and memory change continuously).
  useInterval(() => {
    api
      .overview(dev.key)
      .then((o) => {
        setOv(o);
        setErr(null);
      })
      .catch((e) => setErr(String(e)));
  }, 2000);

  const doReset = async (mode: ResetMode, label: string, body: string) => {
    if (!(await confirm({ title: label, body, ok: label }))) return;
    try {
      await api.reset(dev.key, mode);
      onStatus(`${label}: sent. The device will disconnect and reconnect.`);
    } catch (e) {
      onStatus(`${label} failed: ${e}`);
    }
  };

  const doErase = async () => {
    const ok = await confirm({
      title: "Erase configuration",
      body: "Delete the main and Safe Mode config files from the device? It reverts to factory defaults after the next reboot. Export a copy from the Config tab first if you want to keep it.",
      ok: "Erase",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.eraseConfig(dev.key, "all");
      onStatus("Configuration erased. Reboot to apply factory defaults.");
    } catch (e) {
      onStatus(`Erase failed: ${e}`);
    }
  };

  const info: [string, string][] = [
    ["Unit", `#${ids.unit_num} "${ids.unit_name}"`],
    ["Firmware", dev.version ? `v${fmtVersion(dev.version)} (build ${dev.version.build_date})` : "?"],
    ["Hardware ID", ids.hwid],
    ["CPU", `RP${ids.cpu_type} (version ${ids.cpu_version}, ROM ${ids.rom_version})`],
    ["Target board", ids.target_board],
    ["USB ID", `${hex4(dev.vid)}:${hex4(dev.pid)}`],
    ["Pico SDK / TinyUSB", `${ids.pico_sdk_version} / ${ids.tinyusb_version}`],
    ["Compiler", ids.compiler_version],
  ];

  const s = ov?.stats;
  const u = ov?.usb;
  const gpio = (ov?.gpio ?? []).filter((p) => showAllGpio || p.usage || p.func !== 0x1f);

  return (
    <div className="overview">
      <div className="actions">
        <button onClick={() => doReset("normal", "Reboot", "Restart the device with its normal configuration?")}>Reboot</button>
        <button onClick={() => doReset("safe-mode", "Safe Mode", "Restart in Safe Mode (minimal Safe Mode config)?")}>
          Safe Mode
        </button>
        <button
          onClick={() =>
            doReset("factory", "Factory Mode", "Restart without loading any config (factory defaults). Nothing is erased.")
          }
        >
          Factory Mode
        </button>
        <button
          onClick={() =>
            doReset(
              "bootloader",
              "Boot Loader",
              "Restart into the Pico's boot loader for a firmware update? The device leaves Pinscape mode until new firmware is installed or it is power-cycled.",
            )
          }
        >
          Boot Loader
        </button>
        <span className="spacer" />
        <button className="danger" onClick={doErase}>
          Erase Config…
        </button>
      </div>
      {err && <p className="error">{err}</p>}

      <div className="cards">
        <section className="card">
          <h3>Device</h3>
          <KV rows={info} />
        </section>

        <section className="card">
          <h3>Status</h3>
          {s ? (
            <KV
              rows={[
                ["Uptime", fmtUptime(s.upTimeUs)],
                ["Main loop", `avg ${s.avgLoopTimeUs} µs · max ${s.maxLoopTimeUs.toLocaleString()} µs`],
                ...(s.nLoopsEver2 > 0
                  ? ([["Core 1 loop", `avg ${s.avgLoopTime2Us} µs · max ${s.maxLoopTime2Us.toLocaleString()} µs`]] as [string, string][])
                  : []),
                ["Heap free", `${fmtBytes(s.heapUnused)} of ${s.heapSize.toLocaleString()}`],
                ["Arena", `${s.arenaAlloc.toLocaleString()} used · ${s.arenaFree.toLocaleString()} free`],
                ["Main config", ov!.hasMainConfig ? "stored" : "none (factory defaults)"],
                ["Safe-mode config", ov!.hasSafeModeConfig ? "stored" : "none"],
              ]}
            />
          ) : (
            <p className="muted">Loading…</p>
          )}
        </section>

        <section className="card">
          <h3>USB interfaces</h3>
          {u ? (
            <ul className="ifaces">
              <Iface name="Keyboard" conf={u.keyboardConfigured} ena={u.keyboardEnabled} />
              <Iface name="Gamepad" conf={u.gamepadConfigured} ena={u.gamepadEnabled} />
              <Iface name="XInput (Xbox)" conf={u.xinputConfigured} ena={u.xinputEnabled} />
              <Iface name="Open Pinball Device" conf={u.pindevConfigured} ena={u.pindevEnabled} />
              <Iface name="Feedback Controller (DOF)" conf={u.feedbackConfigured} />
              <Iface name="LedWiz emulation" conf={u.ledwizConfigured} />
              <Iface name="Serial console (CDC)" conf={u.cdcConfigured} />
            </ul>
          ) : (
            <p className="muted">Loading…</p>
          )}
        </section>

        <section className="card wide">
          <h3>
            GPIO assignments
            <label className="inline small muted">
              <input type="checkbox" checked={showAllGpio} onChange={(e) => setShowAllGpio(e.target.checked)} /> show unused
            </label>
          </h3>
          {gpio.length === 0 ? (
            <p className="muted">{ov ? "No GPIO pins assigned yet." : "Loading…"}</p>
          ) : (
            <table className="grid">
              <thead>
                <tr>
                  <th>Pin</th>
                  <th>Function</th>
                  <th>Direction</th>
                  <th>Used for</th>
                </tr>
              </thead>
              <tbody>
                {gpio.map((p) => (
                  <tr key={p.gpio}>
                    <td>GP{p.gpio}</td>
                    <td>{p.func === 0x1f ? <span className="muted">—</span> : p.funcName}</td>
                    <td>{p.adc ? "ADC" : p.func === 5 ? (p.output ? "Output" : "Input") : ""}</td>
                    <td>{p.usage ?? (p.gpio === 25 && p.func === 5 ? <span className="muted">Pico LED</span> : "")}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>
    </div>
  );
}

function KV({ rows }: { rows: [string, string][] }) {
  return (
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
  );
}

function Iface({ name, conf, ena }: { name: string; conf: boolean; ena?: boolean }) {
  const state = !conf ? "off" : ena === false ? "paused" : "on";
  const label = !conf ? "not configured" : ena === false ? "configured, reporting off" : "active";
  return (
    <li>
      <span className={`dot ${state}`} aria-hidden /> {name} <span className="muted small">{label}</span>
    </li>
  );
}
