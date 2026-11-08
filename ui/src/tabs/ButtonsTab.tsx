import { useEffect, useMemo, useRef, useState } from "react";
import { DeviceSnapshot } from "../api";
import { useInterval } from "../lib/useDevices";
import {
  actionLabel,
  bitList,
  BTN,
  ButtonConfig,
  ButtonDesc,
  ButtonsLive,
  buttonsApi,
  buttonTypeName,
  FLAG_EVENTLOG,
  pcaPortName,
  SRC,
  sourceLabel,
} from "../lib/api-buttons";
import { saveTextAs, stampedName, toCsv } from "../lib/api-export";
import "./ButtonsTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; poll the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

const POLL_MS = 75;
/** GPIOs always shown (the Pico exposes GP0..GP29). */
const NUM_GPIO = 30;

/** Buttons that read a given physical port, keyed by "src:unit:port". */
type PortMap = Map<string, number[]>;
const portKey = (src: number, unit: number, port: number) => `${src}:${unit}:${port}`;

export default function ButtonsTab({ dev, visible, onStatus }: Props) {
  const [cfg, setCfg] = useState<ButtonConfig | null>(null);
  const [live, setLive] = useState<ButtonsLive | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const busy = useRef(false);
  const flags = useRef({ pca: false, hc: false });
  const keyRef = useRef(dev.key);
  keyRef.current = dev.key;

  const loadConfig = () => {
    buttonsApi
      .config(dev.key)
      .then((c) => {
        setCfg(c);
        setErr(null);
      })
      .catch((e) => {
        setErr(String(e));
        onStatus(`Couldn't read the button configuration: ${e}`);
      });
  };

  // Descriptors: once per visit (the config only changes across a reboot).
  useEffect(() => {
    if (visible) loadConfig();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, dev.key]);

  const pcaChips = useMemo(() => (cfg?.devices ?? []).filter((d) => d.type === SRC.PCA9555), [cfg]);
  const hcChains = useMemo(() => (cfg?.devices ?? []).filter((d) => d.type === SRC.HC165), [cfg]);
  flags.current = { pca: pcaChips.length > 0, hc: hcChains.length > 0 };

  const hasButtons = (cfg?.buttons.length ?? 0) > 0;
  useInterval(
    () => {
      if (busy.current) return; // don't stack requests if the device is slow
      busy.current = true;
      buttonsApi
        .live(keyRef.current, flags.current.pca, flags.current.hc)
        .then((l) => {
          setLive(l);
          setErr(null);
        })
        .catch((e) => setErr(String(e)))
        .finally(() => (busy.current = false));
    },
    POLL_MS,
    visible && hasButtons && !paused,
  );

  // Which buttons use each physical port.
  const ports: PortMap = useMemo(() => {
    const m: PortMap = new Map();
    (cfg?.buttons ?? []).forEach((b, i) => {
      if (b.sourceType !== SRC.GPIO && b.sourceType !== SRC.PCA9555 && b.sourceType !== SRC.HC165) return;
      const k = portKey(b.sourceType, b.sourceType === SRC.GPIO ? 0 : b.sourceUnit, b.sourcePort);
      m.set(k, [...(m.get(k) ?? []), i]);
    });
    return m;
  }, [cfg]);

  if (!visible) return null;

  if (!cfg) {
    return (
      <div className="empty">
        <p className={err ? "error" : "muted"}>{err ?? "Loading button configuration…"}</p>
      </div>
    );
  }

  if (!hasButtons) {
    return (
      <div className="empty">
        <div className="buttons-empty">
          <h2>No buttons configured</h2>
          <p className="muted">
            Logical buttons are defined in the <code>buttons: [...]</code> section of the device's JSON configuration.
            Each entry names an input source (a GPIO pin, a PCA9555 or 74HC165 port, BOOTSEL, IR, …) and an action
            (keyboard key, gamepad button, XInput, …).
          </p>
          <p className="muted">
            Open the <strong>Config</strong> tab to add some, save to the device, then come back here to test them.
          </p>
          <button onClick={loadConfig}>Reload</button>
        </div>
      </div>
    );
  }

  const buttons = cfg.buttons;
  const shiftButtons = buttons.filter((b) => b.type === BTN.SHIFT);
  const shiftUsed = buttons.reduce((m, b) => m | (b.type === BTN.SHIFT ? b.shiftBits : b.shiftMask), 0) >>> 0;
  const shift = live?.shiftState ?? 0;
  const gpioCount = Math.max(
    NUM_GPIO,
    ...buttons.filter((b) => b.sourceType === SRC.GPIO).map((b) => b.sourcePort + 1),
  );

  return (
    <div className="buttons-tab">
      <div className="toolbar">
        <button onClick={() => setPaused((p) => !p)}>{paused ? "Resume" : "Pause"}</button>
        <button onClick={loadConfig} title="Re-read the button list from the device">
          Reload config
        </button>
        <span className="small muted">
          <span className={`dot ${paused || err ? "off" : "on"}`} aria-hidden />
          {paused ? "paused" : `live · polling every ${POLL_MS} ms`}
        </span>
        <span className="spacer" />
        <span className="small muted">
          {buttons.length} button{buttons.length === 1 ? "" : "s"}
          {pcaChips.length > 0 && ` · ${pcaChips.length} PCA9555`}
          {hcChains.length > 0 && ` · ${hcChains.length} 74HC165 chain${hcChains.length === 1 ? "" : "s"}`}
        </span>
      </div>
      {err && <p className="error">{err}</p>}

      <EventLogPanel dev={dev} buttons={buttons} active={visible && !paused} onStatus={onStatus} />

      {shiftButtons.length > 0 && (
        <section className="card">
          <h3>Shift state</h3>
          <div className="shift-bits">
            {Array.from({ length: 32 }, (_, i) => i)
              .filter((i) => ((shiftUsed >>> i) & 1) === 1)
              .map((i) => {
                const on = ((shift >>> i) & 1) === 1;
                const names = shiftButtons
                  .map((b) => (((b.shiftBits >>> i) & 1) === 1 ? `#${buttons.indexOf(b)}` : null))
                  .filter(Boolean)
                  .join(", ");
                return (
                  <span key={i} className={`shift-bit ${on ? "on" : ""}`} title={names ? `Set by button ${names}` : undefined}>
                    bit {i}
                    {names && <span className="small"> ({names})</span>}
                  </span>
                );
              })}
            <span className="small muted mono">0x{(shift >>> 0).toString(16).toUpperCase().padStart(8, "0")}</span>
          </div>
        </section>
      )}

      <section className="card">
        <h3>Logical buttons</h3>
        <div className="table-scroll">
          <table className="grid buttons-table">
            <thead>
              <tr>
                <th>#</th>
                <th>State</th>
                <th>Type</th>
                <th>Source</th>
                <th>Action</th>
                <th>Shift</th>
              </tr>
            </thead>
            <tbody>
              {buttons.map((b, i) => {
                const on = live?.states[i] ?? false;
                return (
                  <tr key={i} className={on ? "on" : ""}>
                    <td>{i}</td>
                    <td>
                      <span className={`led ${on ? "on" : ""}`}>{on ? "ON" : "OFF"}</span>
                    </td>
                    <td>{buttonTypeName(b.type)}</td>
                    <td>{sourceLabel(b)}</td>
                    <td>{actionLabel(b)}</td>
                    <td className="muted">{shiftLabel(b)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <section className="card">
        <h3>Pico GPIO inputs</h3>
        <p className="small muted legend">
          Raw pin level (H/L) and, for pins assigned to buttons, the pressed state after applying the button's
          active-high/low polarity. Pins not used by any button are dimmed.
        </p>
        <div className="pin-grid">
          {Array.from({ length: gpioCount }, (_, gp) => (
            <PinCell
              key={gp}
              label={`GP${gp}`}
              level={live?.gpio[gp]}
              users={ports.get(portKey(SRC.GPIO, 0, gp))}
              buttons={buttons}
            />
          ))}
        </div>
      </section>

      {pcaChips.map((chip, unit) => {
        const base = unit * 16;
        const bus = chip.addr >> 8;
        const addr = chip.addr & 0xff;
        return (
          <section className="card" key={`pca${unit}`}>
            <h3>
              PCA9555[{unit}]
              <span className="small muted plain">
                I2C bus {bus}, address 0x{addr.toString(16).toUpperCase().padStart(2, "0")}
              </span>
            </h3>
            <div className="pin-grid">
              {Array.from({ length: Math.min(chip.numPorts, 16) }, (_, p) => (
                <PinCell
                  key={p}
                  label={pcaPortName(p)}
                  level={live?.pca9555[base + p]}
                  users={ports.get(portKey(SRC.PCA9555, unit, p))}
                  buttons={buttons}
                />
              ))}
            </div>
          </section>
        );
      })}

      {hcChains.map((chain, unit) => {
        const base = hcChains.slice(0, unit).reduce((n, c) => n + c.numPorts, 0);
        const chips = Math.ceil(chain.numPorts / 8);
        return (
          <section className="card" key={`hc${unit}`}>
            <h3>
              74HC165[{unit}]
              <span className="small muted plain">
                daisy chain of {chips} chip{chips === 1 ? "" : "s"}, {chain.numPorts} ports
              </span>
            </h3>
            {Array.from({ length: chips }, (_, chip) => (
              <div key={chip} className="hc-chip">
                <div className="small muted">Chip {chip}</div>
                <div className="pin-grid">
                  {Array.from({ length: Math.min(8, chain.numPorts - chip * 8) }, (_, j) => {
                    const p = chip * 8 + j;
                    return (
                      <PinCell
                        key={p}
                        label={`port ${p}`}
                        level={live?.hc165[base + p]}
                        users={ports.get(portKey(SRC.HC165, unit, p))}
                        buttons={buttons}
                      />
                    );
                  })}
                </div>
              </div>
            ))}
          </section>
        );
      })}
    </div>
  );
}

function shiftLabel(b: ButtonDesc): string {
  if (b.type === BTN.SHIFT) return b.shiftBits ? `sets bit ${bitList(b.shiftBits)}` : "";
  if (!b.shiftMask) return "";
  const hex = (v: number) => `0x${(v >>> 0).toString(16).toUpperCase()}`;
  return `mask ${hex(b.shiftMask)} = ${hex(b.shiftBits)}`;
}

function PinCell({
  label,
  sub,
  level,
  users,
  buttons,
}: {
  label: string;
  sub?: string;
  level: boolean | undefined;
  users: number[] | undefined;
  buttons: ButtonDesc[];
}) {
  const used = !!users?.length;
  // Polarity comes from the first button on the pin (they normally agree).
  const activeHigh = used ? buttons[users![0]].sourceActiveHigh : false;
  const pressed = used && level !== undefined && level === activeHigh;
  const title = used
    ? users!
        .map((i) => `#${i}: ${actionLabel(buttons[i])} (active ${buttons[i].sourceActiveHigh ? "high" : "low"})`)
        .join("\n")
    : "Not used by any button";
  return (
    <div className={`pin ${used ? "used" : "unused"} ${pressed ? "pressed" : ""}`} title={title}>
      <div className="pin-label">
        {label}
        {sub && <span className="muted"> {sub}</span>}
      </div>
      <div className="pin-level">
        <span className={`lvl ${level ? "hi" : "lo"}`}>{level === undefined ? "–" : level ? "H" : "L"}</span>
        {used && <span className="pin-state">{pressed ? "pressed" : "released"}</span>}
      </div>
      {used && <div className="pin-users">{users!.map((i) => `#${i}`).join(" ")}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Button event log (GPIO edge timestamps, for latency / bounce measurement)
// ---------------------------------------------------------------------------

const EVENT_POLL_MS = 200;
const MAX_EVENTS = 500;

interface LoggedEvent {
  gpio: number;
  /** µs since the Pico's reset. */
  t: number;
  pressed: boolean;
  /** µs since the previous event on the same GPIO (null for the first one seen). */
  dt: number | null;
}

const fmtT = (us: number) => (us / 1e6).toFixed(6);
const fmtMs = (us: number | null) => (us == null ? "—" : (us / 1000).toFixed(us < 10_000 ? 3 : 1));

function EventLogPanel({
  dev,
  buttons,
  active,
  onStatus,
}: {
  dev: DeviceSnapshot;
  buttons: ButtonDesc[];
  active: boolean;
  onStatus: (s: string) => void;
}) {
  const gpios = useMemo(
    () =>
      [...new Set(buttons.filter((b) => b.sourceType === SRC.GPIO && (b.flags & FLAG_EVENTLOG) !== 0).map((b) => b.sourcePort))].sort(
        (a, b) => a - b,
      ),
    [buttons],
  );
  const [events, setEvents] = useState<LoggedEvent[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const last = useRef(new Map<number, number>());
  const busy = useRef(false);
  const key = dev.key;

  useInterval(
    () => {
      if (busy.current) return;
      busy.current = true;
      buttonsApi
        .eventLog(key, gpios)
        .then((res) => {
          setErr(null);
          const fresh: LoggedEvent[] = [];
          for (const g of res) {
            for (const e of g.events ?? []) {
              const prev = last.current.get(g.gpio);
              // a timestamp going backwards means the Pico restarted
              const dt = prev != null && e.t >= prev ? e.t - prev : null;
              last.current.set(g.gpio, e.t);
              fresh.push({ gpio: g.gpio, t: e.t, pressed: e.pressed, dt });
            }
          }
          if (fresh.length) {
            fresh.sort((a, b) => a.t - b.t);
            setEvents((old) => [...old, ...fresh].slice(-MAX_EVENTS));
          }
        })
        .catch((e) => setErr(String(e)))
        .finally(() => (busy.current = false));
    },
    EVENT_POLL_MS,
    active && gpios.length > 0,
  );

  const clear = () => {
    buttonsApi
      .clearEventLog(key, null)
      .then(() => {
        setEvents([]);
        last.current.clear();
        onStatus("Button event log cleared.");
      })
      .catch((e) => onStatus(`Clearing the event log failed: ${e}`));
  };

  const exportCsv = () => {
    const rows = events.map((e) => [e.gpio, e.pressed ? "press" : "release", e.t, e.dt ?? ""]);
    saveTextAs(toCsv(["gpio", "event", "t_us", "since_previous_us"], rows), {
      title: "Export button event log",
      defaultName: stampedName(`${dev.ids?.unit_name ?? "pinscape"}-button-events`, "csv"),
    })
      .then((p) => p && onStatus(`Exported ${events.length} events to ${p}`))
      .catch((e) => onStatus(`Export failed: ${e}`));
  };

  if (gpios.length === 0) {
    return (
      <section className="card">
        <h3>Event log</h3>
        <p className="small muted legend">
          No button has event logging enabled. To record exact press/release times (for measuring latency or switch
          bounce), add <code>enableLogging: true</code> to a GPIO button's <code>source</code> in the config, e.g.{" "}
          <code>source: {"{"} type: "gpio", gp: 4, enableLogging: true {"}"}</code>, save and reboot.
        </p>
      </section>
    );
  }

  // per-GPIO summary
  const stats = gpios.map((g) => {
    const ev = events.filter((e) => e.gpio === g);
    const held = ev.filter((e) => !e.pressed && e.dt != null).map((e) => e.dt!);
    const gaps = ev.filter((e) => e.pressed && e.dt != null).map((e) => e.dt!);
    const users = buttons.map((b, i) => (b.sourceType === SRC.GPIO && b.sourcePort === g ? `#${i}` : null)).filter(Boolean);
    return {
      g,
      users: users.join(" "),
      presses: ev.filter((e) => e.pressed).length,
      lastHeld: held.length ? held[held.length - 1] : null,
      minHeld: held.length ? Math.min(...held) : null,
      minGap: gaps.length ? Math.min(...gaps) : null,
    };
  });
  const recent = events.slice(-60).reverse();

  return (
    <section className="card">
      <h3>
        Event log
        <span className="small muted plain">GPIO edges timestamped by the Pico's interrupt handler (µs since reset)</span>
        <span className="spacer" />
        <button className="small-btn" onClick={exportCsv} disabled={!events.length}>
          Export CSV…
        </button>
        <button className="small-btn" onClick={clear} title="Clear the Pico's buffers and this list">
          Clear
        </button>
      </h3>
      {err && <p className="error small">{err}</p>}
      <div className="table-scroll">
        <table className="grid evlog-summary">
          <thead>
            <tr>
              <th>GPIO</th>
              <th>Buttons</th>
              <th>Presses</th>
              <th>Last held (ms)</th>
              <th>Shortest held (ms)</th>
              <th title="Shortest time from a release to the next press; very short gaps suggest switch bounce">Shortest gap (ms)</th>
            </tr>
          </thead>
          <tbody>
            {stats.map((s) => (
              <tr key={s.g}>
                <td>GP{s.g}</td>
                <td className="muted">{s.users}</td>
                <td>{s.presses}</td>
                <td>{fmtMs(s.lastHeld)}</td>
                <td>{fmtMs(s.minHeld)}</td>
                <td>{fmtMs(s.minGap)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {events.length === 0 ? (
        <p className="small muted legend">Waiting for button presses on {gpios.map((g) => `GP${g}`).join(", ")}…</p>
      ) : (
        <div className="table-scroll evlog-scroll">
          <table className="grid evlog">
            <thead>
              <tr>
                <th>GPIO</th>
                <th>Event</th>
                <th>Pico time (s)</th>
                <th>Since previous (ms)</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((e, i) => (
                <tr key={`${e.gpio}:${e.t}:${i}`} className={e.pressed ? "on" : ""}>
                  <td>GP{e.gpio}</td>
                  <td>{e.pressed ? "press" : "release"}</td>
                  <td className="mono">{fmtT(e.t)}</td>
                  <td className="mono" title={e.dt == null ? undefined : e.pressed ? "released for" : "held for"}>
                    {fmtMs(e.dt)} {e.dt != null && <span className="muted small">{e.pressed ? "released" : "held"}</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="small muted legend">
        The Pico keeps the latest 63 edges per GPIO; this list shows the newest 60 of the last {MAX_EVENTS} collected.
        Hold times and gaps show switch bounce and debouncing; for end-to-end latency, compare these times with input timestamps recorded on the host.
      </p>
    </section>
  );
}
