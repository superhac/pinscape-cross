import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DeviceSnapshot } from "../api";
import {
  DEV,
  deviceLabel,
  LogicalPort,
  mappingLabel,
  OutputDevice,
  OutputsConfig,
  OutputsLevels,
  outputsApi,
  pinLabel,
  PORT_TYPE,
  portMax,
} from "../lib/api-outputs";
import { confirm } from "../lib/confirm";
import { useInterval } from "../lib/useDevices";
import "./OutputsTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; poll the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

/** Test mode auto-reverts on the device after this long without a keepalive. */
const TEST_TIMEOUT_MS = 3000;
const KEEPALIVE_MS = 1000;
const POLL_MS = 80;
/** A slider shows the value being dragged (not the polled one) for this long after a change. */
const OVERRIDE_MS = 700;

/**
 * Sends only the latest value per key, one request at a time, so dragging a
 * slider doesn't queue up hundreds of USB requests.
 */
function useLatestSender(onError: (e: unknown) => void) {
  const pending = useRef(new Map<string, () => Promise<unknown>>());
  const busy = useRef(false);
  const errRef = useRef(onError);
  errRef.current = onError;
  return useCallback((key: string, fn: () => Promise<unknown>) => {
    pending.current.set(key, fn);
    if (busy.current) return;
    busy.current = true;
    (async () => {
      try {
        while (pending.current.size) {
          const [k, f] = pending.current.entries().next().value as [string, () => Promise<unknown>];
          pending.current.delete(k);
          await f().catch((e) => errRef.current(e));
        }
      } finally {
        busy.current = false;
      }
    })();
  }, []);
}

export default function OutputsTab({ dev, visible, onStatus }: Props) {
  const key = dev.key;
  const [cfg, setCfg] = useState<OutputsConfig | null>(null);
  const [lv, setLv] = useState<OutputsLevels | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [testOn, setTestOn] = useState(false);
  const [view, setView] = useState<"logical" | "physical">("logical");
  const [showUnused, setShowUnused] = useState(false);
  const [overrides, setOverrides] = useState<Record<string, { v: number; t: number }>>({});
  const polling = useRef(false);
  const testOnRef = useRef(false);
  testOnRef.current = testOn;

  const send = useLatestSender((e) => onStatus(`Outputs: ${e}`));

  // ---- configuration (fetched once per visit) ----
  const loadConfig = useCallback(() => {
    outputsApi
      .config(key)
      .then((c) => {
        setCfg(c);
        setErr(null);
      })
      .catch((e) => setErr(String(e)));
  }, [key]);

  useEffect(() => {
    if (visible && !cfg) loadConfig();
  }, [visible, cfg, loadConfig]);

  // ---- live levels ----
  useInterval(
    () => {
      if (polling.current) return;
      polling.current = true;
      outputsApi
        .levels(key)
        .then((l) => {
          setLv(l);
          setErr(null);
        })
        .catch((e) => setErr(String(e)))
        .finally(() => (polling.current = false));
    },
    POLL_MS,
    visible && cfg != null,
  );

  // ---- test mode safety ----
  // While the tab is visible and the user has test mode on, (re)enable it with a
  // short timeout about once a second. Leaving that state for any reason (toggle
  // off, tab hidden, device change/unmount) explicitly disables it; if the app
  // dies, the device reverts on its own when the timeout lapses.
  useEffect(() => {
    if (!visible || !testOn) return;
    let stopped = false;
    // Chain the final disable after the last keepalive, so a keepalive still in
    // flight can't land after it and re-enable test mode.
    let last: Promise<unknown> = Promise.resolve();
    const keepalive = () => {
      last = outputsApi.setTestMode(key, true, TEST_TIMEOUT_MS).catch((e) => {
        if (stopped) return;
        setTestOn(false);
        onStatus(`Test mode stopped: ${e}`);
      });
    };
    keepalive();
    const t = setInterval(keepalive, KEEPALIVE_MS);
    return () => {
      stopped = true;
      clearInterval(t);
      last.finally(() => outputsApi.setTestMode(key, false, 0).catch(() => {}));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, testOn, key]);

  // Hiding the tab ends test mode for good (it doesn't silently resume on return).
  useEffect(() => {
    if (!visible && testOn) {
      setTestOn(false);
      onStatus("Output test mode ended (tab closed).");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible]);

  // Best effort on window reload/close; the device timeout is the backstop.
  useEffect(() => {
    const onUnload = () => {
      if (testOnRef.current) outputsApi.setTestMode(key, false, 0).catch(() => {});
    };
    window.addEventListener("beforeunload", onUnload);
    window.addEventListener("pagehide", onUnload);
    return () => {
      window.removeEventListener("beforeunload", onUnload);
      window.removeEventListener("pagehide", onUnload);
    };
  }, [key]);

  const deviceTestMode = lv?.testMode ?? false;
  const physEnabled = testOn && deviceTestMode;

  const toggleTest = async () => {
    if (testOn) {
      setTestOn(false);
      onStatus("Output test mode off: normal output control resumed.");
      return;
    }
    const ok = await confirm({
      title: "Enter output test mode",
      body:
        "Test mode disconnects the physical outputs from the logical ports and DISABLES the firmware's protective timers " +
        "(Flipper Logic, Chime Logic). A solenoid, coil or motor left on can overheat. Keep activations brief. " +
        `Test mode ends automatically ${TEST_TIMEOUT_MS / 1000} s after this tab stops renewing it.`,
      ok: "Enter test mode",
      danger: true,
    });
    if (!ok) return;
    setTestOn(true);
    onStatus("Output test mode on.");
  };

  const exitDeviceTestMode = () => {
    setTestOn(false);
    outputsApi
      .setTestMode(key, false, 0)
      .then(() => onStatus("Output test mode off."))
      .catch((e) => onStatus(`Couldn't leave test mode: ${e}`));
  };

  const allOff = () => {
    setOverrides({});
    outputsApi
      .allOff(key)
      .then(() => onStatus(deviceTestMode ? "All logical and physical outputs set to 0." : "All logical ports set to 0."))
      .catch((e) => onStatus(`All off failed: ${e}`));
  };

  const setNight = (on: boolean) => {
    setLv((l) => (l ? { ...l, nightMode: on } : l));
    outputsApi.setNightMode(key, on).catch((e) => onStatus(`Night mode: ${e}`));
  };

  const override = (k: string, v: number) => setOverrides((o) => ({ ...o, [k]: { v, t: Date.now() } }));
  const shown = (k: string, live: number) => {
    const o = overrides[k];
    return o && Date.now() - o.t < OVERRIDE_MS ? o.v : live;
  };

  const setLogical = (port: number, level: number) => {
    override(`L${port}`, level);
    send(`L${port}`, () => outputsApi.setPort(key, port, level));
  };

  const setPhysical = (d: OutputDevice, port: number, level: number) => {
    if (!physEnabled) return;
    const k = `D${d.devType}:${d.configIndex}:${port}`;
    override(k, level);
    send(k, () => outputsApi.setDevicePort(key, d.devType, d.configIndex, port, level));
  };

  const resetWorker = async (d: OutputDevice, bootloader: boolean) => {
    const name = deviceLabel(d);
    const ok = await confirm({
      title: bootloader ? `Boot loader: ${name}` : `Reset ${name}`,
      body: bootloader
        ? `Restart the PWMWorker Pico ${name} in its boot loader (BOOTSEL mode)? Its outputs stop until it's restarted, ` +
          "e.g. by installing PWMWorker firmware onto it (it appears in the sidebar under 'Boot loader mode' if it's " +
          "connected to this computer by USB), or by power-cycling it."
        : `Reboot the PWMWorker Pico ${name}? Its outputs switch off briefly while it restarts; Pinscape reconnects to it afterwards.`,
      ok: bootloader ? "Enter boot loader" : "Reset",
      danger: true,
    });
    if (!ok) return;
    outputsApi
      .pwmWorkerReset(key, d.configIndex, bootloader)
      .then(() => onStatus(bootloader ? `${name} restarted in boot loader mode.` : `${name} reset.`))
      .catch((e) => onStatus(`${name}: ${e}`));
  };

  // physical port -> logical ports mapped to it
  const reverseMap = useMemo(() => {
    const m = new Map<string, LogicalPort[]>();
    for (const p of cfg?.ports ?? []) {
      const k = `${p.devType}:${p.devId}:${p.devPort}`;
      m.set(k, [...(m.get(k) ?? []), p]);
    }
    return m;
  }, [cfg]);

  if (!visible) return null;

  if (!cfg) {
    return (
      <div className="outputs">
        {err ? (
          <>
            <p className="error">{err}</p>
            <div>
              <button onClick={loadConfig}>Retry</button>
            </div>
          </>
        ) : (
          <p className="muted">Loading output configuration…</p>
        )}
      </div>
    );
  }

  const usedDevices = cfg.devices.filter((d) => d.portTypes.some((t) => t !== PORT_TYPE.UNUSED));
  if (cfg.ports.length === 0 && usedDevices.length === 0) {
    return (
      <div className="empty">
        <div className="out-empty">
          <h2>No outputs configured</h2>
          <p className="muted">
            This device has no logical output ports. Outputs (lamps, flashers, solenoids, motors) are defined in the
            <code> outputs: [...]</code> array of the device's JSON configuration, each one mapping a DOF port number to a
            Pico GPIO or a port on an output chip (TLC5940, TLC59116, PCA9685, 74HC595, …) declared in the same file.
          </p>
          <p className="muted">Open the Config tab to add them, save to the device, and come back here to test them.</p>
          <button onClick={loadConfig}>Reload</button>
        </div>
      </div>
    );
  }

  return (
    <div className="outputs">
      <div className="toolbar">
        <div className="out-seg" role="tablist">
          <button className={view === "logical" ? "active" : ""} onClick={() => setView("logical")}>
            Logical ports ({cfg.ports.length})
          </button>
          <button className={view === "physical" ? "active" : ""} onClick={() => setView("physical")}>
            Physical outputs ({cfg.devices.length})
          </button>
        </div>
        <span className="sep" />
        <label className="inline" title="Night Mode silences ports marked 'noisy'">
          <input type="checkbox" checked={lv?.nightMode ?? false} disabled={!lv} onChange={(e) => setNight(e.target.checked)} />
          Night mode
        </label>
        <label className="inline" title="Drive the physical outputs directly, bypassing the logical ports">
          <input type="checkbox" checked={testOn} onChange={toggleTest} />
          Test mode
        </label>
        <button className="danger" onClick={allOff} title="Set every logical port (and, in test mode, every physical output) to 0">
          All off
        </button>
        <span className="spacer" />
        <button onClick={loadConfig} title="Re-read the port and device configuration">
          Reload
        </button>
      </div>

      {(testOn || deviceTestMode) && (
        <div className="out-banner" role="alert">
          <strong>⚠ Output test mode is active.</strong>{" "}
          {testOn ? (
            <>
              Physical outputs are driven directly and the firmware's protective timers (Flipper/Chime Logic) are OFF.
              Don't leave solenoids or motors on. Leaving this tab ends test mode.
            </>
          ) : (
            <>
              The device reports test mode, but this tab isn't holding it (another program, or the timeout hasn't lapsed yet).
              <button className="danger out-banner-btn" onClick={exitDeviceTestMode}>
                Exit test mode
              </button>
            </>
          )}
        </div>
      )}
      {err && <p className="error">{err}</p>}

      {view === "logical" ? (
        <LogicalView cfg={cfg} lv={lv} testMode={deviceTestMode} shown={shown} setLogical={setLogical} />
      ) : (
        <>
          <div className="toolbar small">
            <label className="inline">
              <input type="checkbox" checked={showUnused} onChange={(e) => setShowUnused(e.target.checked)} /> show unused
              ports
            </label>
            {!physEnabled && <span className="muted">Turn on Test mode to set physical outputs directly.</span>}
          </div>
          <div className="cards">
            {cfg.devices.map((d) => (
              <DeviceCard
                key={`${d.devType}:${d.configIndex}`}
                d={d}
                levels={lv?.device}
                enabled={physEnabled}
                showUnused={showUnused}
                reverseMap={reverseMap}
                shown={shown}
                setPhysical={setPhysical}
                onWorkerReset={d.devType === DEV.PWMWORKER ? (bl) => resetWorker(d, bl) : undefined}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function Bar({ value, max, kind, title }: { value: number; max: number; kind: string; title?: string }) {
  const pct = max > 0 ? Math.min(100, (value / max) * 100) : 0;
  return (
    <div className={`out-bar ${kind}`} title={title}>
      <div className="out-fill" style={{ width: `${pct}%` }} />
    </div>
  );
}

function LogicalView({
  cfg,
  lv,
  testMode,
  shown,
  setLogical,
}: {
  cfg: OutputsConfig;
  lv: OutputsLevels | null;
  testMode: boolean;
  shown: (k: string, live: number) => number;
  setLogical: (port: number, level: number) => void;
}) {
  return (
    <>
      {testMode && (
        <p className="muted small">
          Test mode is on: logical port levels are still recorded, but they don't reach the physical outputs until test
          mode ends.
        </p>
      )}
      <div className="out-tablewrap">
        <table className="grid out-ports">
          <thead>
            <tr>
              <th>#</th>
              <th>Name</th>
              <th>Device</th>
              <th>Flags</th>
              <th className="out-levels-h">
                Levels <span className="out-key dof">DOF</span> <span className="out-key calc">calc</span>{" "}
                <span className="out-key out">out</span>
              </th>
              <th>Set level</th>
            </tr>
          </thead>
          <tbody>
            {cfg.ports.map((p) => {
              const l = lv?.logical[p.port - 1];
              const v = shown(`L${p.port}`, l?.dofLevel ?? 0);
              return (
                <tr key={p.port} className={testMode ? "dim" : ""}>
                  <td>{p.port}</td>
                  <td className="out-name">{p.name ?? <span className="muted">—</span>}</td>
                  <td>{mappingLabel(p)}</td>
                  <td className="out-flags">
                    {p.noisy && <span className="out-flag" title="Noisy: off in Night Mode">noisy</span>}
                    {p.flipperLogic && <span className="out-flag warn" title="Flipper/Chime Logic time limit">flipper</span>}
                    {p.inverted && <span className="out-flag" title="Inverted (active-low) output">inverted</span>}
                    {p.gamma && <span className="out-flag" title="Gamma correction">gamma</span>}
                    {p.computed && <span className="out-flag" title="Computed data source (not host-controlled)">computed</span>}
                    {l?.lwMode && (
                      <span className="out-flag" title={`LedWiz mode: ${l.lwOn ? "on" : "off"}, profile ${l.lwProfile}`}>
                        LedWiz
                      </span>
                    )}
                  </td>
                  <td className="out-levels">
                    {l ? (
                      <>
                        <div className="out-lrow">
                          <Bar value={l.dofLevel} max={255} kind="dof" title="DOF level (host-commanded)" />
                          <span>{l.dofLevel}</span>
                        </div>
                        <div className="out-lrow">
                          <Bar value={l.calcLevel} max={255} kind="calc" title="Calculated level (after computed sources)" />
                          <span>{l.calcLevel}</span>
                        </div>
                        <div className="out-lrow">
                          <Bar value={l.outLevel} max={255} kind="out" title="Output level (after flipper logic, inversion, night mode)" />
                          <span>{l.outLevel}</span>
                        </div>
                      </>
                    ) : (
                      <span className="muted">…</span>
                    )}
                  </td>
                  <td className="out-set">
                    <input
                      type="range"
                      min={0}
                      max={255}
                      value={v}
                      onChange={(e) => setLogical(p.port, Number(e.target.value))}
                      aria-label={`Port ${p.port} level`}
                    />
                    <span className="out-num">{v}</span>
                    <button onClick={() => setLogical(p.port, 0)}>0</button>
                    <button onClick={() => setLogical(p.port, 255)}>255</button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

function DeviceCard({
  d,
  levels,
  enabled,
  showUnused,
  reverseMap,
  shown,
  setPhysical,
  onWorkerReset,
}: {
  /** PWMWorker only: reboot it (or into its boot loader). */
  onWorkerReset?: (bootloader: boolean) => void;
  d: OutputDevice;
  levels: number[] | undefined;
  enabled: boolean;
  showUnused: boolean;
  reverseMap: Map<string, LogicalPort[]>;
  shown: (k: string, live: number) => number;
  setPhysical: (d: OutputDevice, port: number, level: number) => void;
}) {
  const chips = d.numPortsPerChip > 0 ? Math.max(1, Math.round(d.numPorts / d.numPortsPerChip)) : 1;
  const ports = Array.from({ length: d.numPorts }, (_, i) => i).filter(
    (i) => showUnused || (d.portTypes[i] ?? PORT_TYPE.UNUSED) !== PORT_TYPE.UNUSED,
  );
  const res = d.pwmRes <= 2 ? "digital (on/off)" : `PWM 0–${d.pwmRes - 1}`;
  return (
    <section className="card out-dev">
      <h3>
        {deviceLabel(d)}
        {onWorkerReset && (
          <span className="out-worker-btns">
            <button className="small-btn" onClick={() => onWorkerReset(false)} title="Reboot this PWMWorker Pico">
              Reset
            </button>
            <button className="small-btn danger" onClick={() => onWorkerReset(true)} title="Restart this PWMWorker Pico in its boot loader, e.g. to update its firmware">
              Boot loader
            </button>
          </span>
        )}
      </h3>
      <div className="out-devinfo small muted">
        <span>{d.devTypeName}</span>
        <span>config #{d.configIndex}</span>
        {d.i2cAddr != null && <span>I2C{d.i2cBus} 0x{d.i2cAddr.toString(16).padStart(2, "0")}</span>}
        <span>{chips > 1 ? `${chips} chips × ${d.numPortsPerChip} ports` : `${d.numPorts} ports`}</span>
        <span>{res}</span>
      </div>
      {ports.length === 0 && <p className="muted small">No ports in use.</p>}
      <div className="out-devports">
        {ports.map((i) => {
          const type = d.portTypes[i] ?? PORT_TYPE.UNUSED;
          const max = portMax(d, i);
          const live = levels?.[d.firstPort + i] ?? 0;
          const v = shown(`D${d.devType}:${d.configIndex}:${i}`, live);
          const mapped = reverseMap.get(`${d.devType}:${d.configIndex}:${i}`) ?? [];
          const chipNote = chips > 1 ? `chip ${Math.floor(i / d.numPortsPerChip)}, pin ${i % d.numPortsPerChip}` : "";
          return (
            <div key={i} className={`out-dport ${type === PORT_TYPE.UNUSED ? "unused" : ""}`}>
              <span className="out-pin" title={chipNote}>
                {pinLabel(d.devType, i)}
              </span>
              <span className="out-ptype small muted">
                {type === PORT_TYPE.PWM ? "pwm" : type === PORT_TYPE.DIGITAL ? "digital" : "unused"}
              </span>
              <span className="out-map small" title={mapped.map((p) => `#${p.port} ${p.name ?? ""}`).join(", ")}>
                {mapped.map((p) => `#${p.port}${p.name ? " " + p.name : ""}`).join(", ")}
              </span>
              <Bar value={live} max={max} kind="out" />
              <span className="out-num">{live}</span>
              {max === 1 ? (
                <button
                  className={`out-toggle ${v ? "on" : ""}`}
                  disabled={!enabled || type === PORT_TYPE.UNUSED}
                  onClick={() => setPhysical(d, i, v ? 0 : 1)}
                >
                  {v ? "On" : "Off"}
                </button>
              ) : (
                <input
                  type="range"
                  min={0}
                  max={max}
                  value={v}
                  disabled={!enabled || type === PORT_TYPE.UNUSED}
                  onChange={(e) => setPhysical(d, i, Number(e.target.value))}
                  aria-label={`${pinLabel(d.devType, i)} level`}
                />
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
