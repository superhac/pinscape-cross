import { useEffect, useRef, useState } from "react";
import { DeviceSnapshot } from "../api";
import { useInterval } from "../lib/useDevices";
import {
  irApi,
  IrCommandReceived,
  IrConfigStatus,
  IrRawPulse,
  parseIrCode,
  TvOnState,
  TvRelayAction,
  TVON_STATES,
} from "../lib/api-ir";
import { saveTextAs, stampedName, toCsv } from "../lib/api-export";
import "./IrTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; poll the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

interface HistEntry {
  id: number;
  cmd: IrCommandReceived;
  at: Date;
  /** Number of receptions folded into this entry (auto-repeats of the same code). */
  repeats: number;
}

const POLL_MS = 200;
const MAX_HIST = 500;
/** Start a new raw trace when pulses arrive after this much quiet (as the Windows tester does). */
const TRACE_GAP_MS = 2500;

// IR & TV ON tester, modeled on the upstream Windows config tool's IR tester window.
export default function IrTab({ dev, visible, onStatus }: Props) {
  const [hist, setHist] = useState<HistEntry[]>([]);
  const [pulses, setPulses] = useState<IrRawPulse[]>([]);
  const [tv, setTv] = useState<TvOnState | null>(null);
  const [irErr, setIrErr] = useState<string | null>(null);
  const [tvErr, setTvErr] = useState<string | null>(null);
  const [pollErr, setPollErr] = useState<string | null>(null);
  const [cfg, setCfg] = useState<IrConfigStatus | null>(null);
  const [protoNames, setProtoNames] = useState<Map<number, string>>(new Map());
  const [paused, setPaused] = useState(false);

  const nextId = useRef(1);
  const busy = useRef(false);
  const lastPulseAt = useRef(0);

  // Static info: protocol names (no device access) and IR pin assignments.
  useEffect(() => {
    irApi
      .protocols()
      .then((p) => setProtoNames(new Map(p.map((x) => [x.id, x.name]))))
      .catch(() => {});
  }, []);
  useEffect(() => {
    if (!visible) return;
    irApi
      .configStatus(dev.key)
      .then(setCfg)
      .catch(() => setCfg(null));
  }, [visible, dev.key]);

  useInterval(
    () => {
      if (busy.current) return;
      busy.current = true;
      irApi
        .poll(dev.key)
        .then((p) => {
          setPollErr(null);
          setIrErr(p.irError);
          setTvErr(p.tvonError);
          if (p.tvon) setTv(p.tvon);
          if (p.raw.length) {
            const now = Date.now();
            const fresh = now - lastPulseAt.current > TRACE_GAP_MS;
            lastPulseAt.current = now;
            setPulses((old) => (fresh ? p.raw : [...old, ...p.raw]));
          }
          if (p.commands.length) {
            const now = new Date();
            setHist((old) => {
              const h = [...old];
              for (const cmd of p.commands) {
                const last = h[0];
                // Fold an auto-repeat of the previous code into its entry; keep the
                // newest report since dittos only show up in the repeats.
                if (cmd.autoRepeat && last && last.cmd.text === cmd.text) {
                  h[0] = { ...last, cmd, repeats: last.repeats + 1 };
                } else {
                  h.unshift({ id: nextId.current++, cmd, at: now, repeats: 1 });
                }
              }
              return h.slice(0, MAX_HIST);
            });
          }
        })
        .catch((e) => setPollErr(String(e)))
        .finally(() => (busy.current = false));
    },
    POLL_MS,
    visible && !paused,
  );

  const [sendText, setSendText] = useState("");
  const [count, setCount] = useState(1);
  const [sending, setSending] = useState(false);
  const [sendMsg, setSendMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const parsed = parseIrCode(sendText);

  const send = async (text: string, n: number) => {
    setSending(true);
    try {
      const c = await irApi.send(dev.key, text.trim(), n);
      const msg = `Sent ${text.trim().toUpperCase()}${n > 1 ? ` ×${n}` : ""} (${protoNames.get(c.protocol) ?? "protocol " + c.protocol})`;
      setSendMsg({ ok: true, text: msg });
      onStatus(msg);
    } catch (e) {
      setSendMsg({ ok: false, text: String(e) });
      onStatus(`IR send failed: ${e}`);
    } finally {
      setSending(false);
    }
  };

  const copy = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => onStatus(`Copied ${text}`),
      () => onStatus("Copy failed"),
    );
  };

  const relay = async (a: TvRelayAction, label: string) => {
    try {
      await irApi.setTvRelay(dev.key, a);
      onStatus(`TV relay: ${label}`);
    } catch (e) {
      onStatus(`TV relay ${label} failed: ${e}`);
    }
  };

  if (!visible) return null;

  const rxMissing = cfg && cfg.rxGpio === null;
  const txMissing = cfg && cfg.txGpio === null;

  return (
    <div className="irtab">
      <div className="toolbar">
        <button onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</button>
        <span className="muted small">
          {cfg
            ? `IR receiver: ${cfg.rxGpio !== null ? "GP" + cfg.rxGpio : "not configured"} · IR transmitter: ${
                cfg.txGpio !== null ? "GP" + cfg.txGpio : "not configured"
              }`
            : ""}
        </span>
        <span className="spacer" />
        {paused && <span className="badge">paused</span>}
      </div>
      {pollErr && <p className="error">{pollErr}</p>}

      {(rxMissing || txMissing) && (
        <div className="ir-hint">
          <strong>{rxMissing && txMissing ? "IR isn't configured on this device." : rxMissing ? "No IR receiver configured." : "No IR transmitter configured."}</strong>{" "}
          The tester needs {rxMissing && <><code>irRx</code> (a TSOP384xx-style receiver: <code>irRx: {"{"} gpio: 9 {"}"}</code>)</>}
          {rxMissing && txMissing && " and "}
          {txMissing && <><code>irTx</code> (an IR LED driver: <code>irTx: {"{"} gpio: 10 {"}"}</code>)</>} in the device
          config. TV ON uses the <code>tvon</code> section (power-sense circuit, relay and power-on IR codes). Add them on the
          Config tab, save, and reboot.
        </div>
      )}

      <div className="cards">
        <section className="card wide">
          <h3>
            Commands received
            <span className="spacer" />
            <button className="small-btn" onClick={() => setHist([])} disabled={!hist.length}>
              Clear
            </button>
            <button
              className="small-btn"
              disabled={!hist.length}
              onClick={() => copy(hist.map((h) => h.cmd.text).reverse().join("\n"))}
            >
              Copy all
            </button>
          </h3>
          {irErr && <p className="error small">{irErr}</p>}
          {hist.length === 0 ? (
            <p className="muted">
              {rxMissing
                ? "No IR receiver is configured, so nothing can be received."
                : "Point a remote at the receiver and press a button. Hold a button long enough to auto-repeat to see how the remote encodes repeats (dittos or toggle bits); that helps when using the code with the transmitter."}
            </p>
          ) : (
            <div className="ir-hist">
              <table className="grid">
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Δt</th>
                    <th>Protocol</th>
                    <th>Code</th>
                    <th>Flags</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {hist.map((h) => (
                    <tr key={h.id}>
                      <td className="muted">{h.at.toLocaleTimeString()}</td>
                      <td className="muted">{fmtDt(h.cmd.dtUs)}</td>
                      <td className="sans">{h.cmd.protocolName}</td>
                      <td>
                        <strong>{h.cmd.text}</strong>
                        {h.repeats > 1 && <span className="muted"> ×{h.repeats}</span>}
                      </td>
                      <td className="sans">
                        <Flags c={h.cmd} />
                      </td>
                      <td className="ir-actions">
                        <button className="small-btn" onClick={() => copy(h.cmd.text)} title="Copy the code (universal format)">
                          Copy
                        </button>
                        <button
                          className="small-btn"
                          title="Put this code in the transmitter box"
                          onClick={() => {
                            setSendText(h.cmd.text);
                            setSendMsg(null);
                          }}
                        >
                          Use
                        </button>
                        <button
                          className="small-btn"
                          disabled={sending || txMissing === true}
                          title={txMissing ? "No IR transmitter configured" : "Transmit this code now"}
                          onClick={() => send(h.cmd.text, Math.max(1, Math.min(255, h.repeats)))}
                        >
                          Re-send
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>

        <section className="card wide">
          <h3>
            Raw pulses (latest code)
            <span className="spacer" />
            <button className="small-btn" onClick={() => setPulses([])} disabled={!pulses.length}>
              Clear
            </button>
            <button className="small-btn" onClick={() => copy(pulsesText(pulses))} disabled={!pulses.length}>
              Copy as text
            </button>
            <button
              className="small-btn"
              disabled={!pulses.length}
              title="Save the pulse timeline as a CSV file"
              onClick={() =>
                saveTextAs(pulsesCsv(pulses), { title: "Export IR pulses", defaultName: stampedName("ir-pulses", "csv") })
                  .then((p) => p && onStatus(`Exported ${pulses.length} pulses to ${p}`))
                  .catch((e) => onStatus(`Export failed: ${e}`))
              }
            >
              Export CSV…
            </button>
          </h3>
          <PulseView pulses={pulses} />
        </section>

        <section className="card">
          <h3>Transmitter</h3>
          <form
            className="ir-send"
            onSubmit={(e) => {
              e.preventDefault();
              if (parsed) send(sendText, count);
            }}
          >
            <label>
              IR code
              <input
                value={sendText}
                spellCheck={false}
                placeholder="01.02.20DF10EF"
                className={sendText && !parsed ? "invalid" : ""}
                onChange={(e) => {
                  setSendText(e.target.value);
                  setSendMsg(null);
                }}
              />
            </label>
            <label>
              Repeats
              <input
                type="number"
                min={1}
                max={255}
                value={count}
                onChange={(e) => setCount(Math.max(1, Math.min(255, Number(e.target.value) || 1)))}
              />
            </label>
            <button className="primary" type="submit" disabled={!parsed || sending}>
              Send
            </button>
          </form>
          <p className="small muted">
            {sendText && !parsed ? (
              <span className="error">Format: protocol.flags.code in hex, e.g. 01.02.20DF10EF</span>
            ) : parsed ? (
              <>
                {protoNames.get(parsed.protocol) ?? `Unknown protocol 0x${parsed.protocol.toString(16).toUpperCase()}`}
                {parsed.flags & 0x02 ? " · repeats sent as dittos" : ""}
              </>
            ) : (
              "Paste a code from the received list or from the config (protocol.flags.code)."
            )}
          </p>
          {sendMsg && <p className={sendMsg.ok ? "small ok-text" : "small error"}>{sendMsg.text}</p>}
        </section>

        <section className="card">
          <h3>TV ON</h3>
          {tvErr && <p className="error small">{tvErr}</p>}
          {tv ? <TvOnView tv={tv} onRelay={relay} /> : <p className="muted">{tvErr ? "" : "Loading…"}</p>}
        </section>
      </div>
    </div>
  );
}

function fmtDt(us: number): string {
  if (us >= 60e6) return "—";
  if (us >= 1e6) return (us / 1e6).toFixed(2) + " s";
  return (us / 1000).toFixed(1) + " ms";
}

function Flags({ c }: { c: IrCommandReceived }) {
  const f: [string, string][] = [];
  if (c.autoRepeat) f.push(["repeat", "Auto-repeat (button held down)"]);
  if (c.hasDitto) f.push([c.ditto ? "ditto" : "no ditto", "Protocol has ditto codes; this report " + (c.ditto ? "is" : "isn't") + " a ditto"]);
  if (c.hasToggle) f.push([`toggle ${c.toggle ? 1 : 0}`, "Toggle bit value (flips on each new key press)"]);
  if (c.position) f.push([`pos ${c.position}`, "Position code in a repeat sequence"]);
  if (!f.length) return <span className="muted">—</span>;
  return (
    <span className="ir-flags">
      {f.map(([t, title]) => (
        <span key={t} className="ir-flag" title={title}>
          {t}
        </span>
      ))}
    </span>
  );
}

/** One row per pulse: index, mark/space, duration (µs, empty if over range), start offset (µs). */
function pulsesCsv(p: IrRawPulse[]): string {
  let t = 0;
  let known = true;
  const rows = p.map((x, i) => {
    const row = [i, x.mark ? "mark" : "space", x.tUs ?? "", x.tUs === null ? 1 : 0, known ? t : ""];
    if (x.tUs === null) known = false;
    else t += x.tUs;
    return row;
  });
  return toCsv(["index", "type", "duration_us", "over_range", "start_us"], rows);
}

function pulsesText(p: IrRawPulse[]): string {
  return p.map((x) => `${x.mark ? "mark " : "space"} ${x.tUs === null ? ">131068" : x.tUs} us`).join("\n");
}

const LONG_US = 3000; // display width used for an over-range (>131 ms) pulse

function PulseView({ pulses }: { pulses: IrRawPulse[] }) {
  const [usPerPx, setUsPerPx] = useState(25);
  if (!pulses.length) return <p className="muted">No pulses captured yet.</p>;

  const H = 64, top = 18, bottom = 46, pad = 8;
  let x = pad;
  const segs = pulses.map((p) => {
    const w = Math.max(1, (p.tUs ?? LONG_US) / usPerPx);
    const s = { ...p, x, w };
    x += w;
    return s;
  });
  const width = Math.ceil(x + pad);
  let d = `M0 ${bottom} H${pad}`;
  for (const s of segs) {
    const y = s.mark ? top : bottom;
    d += ` V${y} H${s.x + s.w}`;
  }
  d += ` V${bottom} H${width}`;

  const marks = pulses.filter((p) => p.mark).length;
  const total = pulses.reduce((a, p) => a + (p.tUs ?? 0), 0);

  return (
    <div className="ir-pulses">
      <div className="toolbar small">
        <span className="muted">
          {pulses.length} pulses · {marks} marks · {(total / 1000).toFixed(1)} ms
          {pulses.some((p) => p.tUs === null) ? " (plus over-range gaps)" : ""}
        </span>
        <span className="spacer" />
        <label className="inline muted">
          Zoom
          <select value={usPerPx} onChange={(e) => setUsPerPx(Number(e.target.value))}>
            <option value={5}>5 µs/px</option>
            <option value={10}>10 µs/px</option>
            <option value={25}>25 µs/px</option>
            <option value={50}>50 µs/px</option>
            <option value={100}>100 µs/px</option>
          </select>
        </label>
      </div>
      <div className="ir-scope">
        <svg width={width} height={H} role="img" aria-label="IR pulse timeline">
          {segs.map((s, i) => (
            <g key={i}>
              {s.mark && <rect className="ir-mark" x={s.x} y={top} width={s.w} height={bottom - top} />}
              {s.tUs === null && (
                <text className="ir-long" x={s.x + s.w / 2} y={bottom - 6} textAnchor="middle">
                  ≫
                </text>
              )}
              {s.w >= 30 && (
                <text className="ir-dur" x={s.x + s.w / 2} y={s.mark ? top - 5 : bottom + 13} textAnchor="middle">
                  {s.tUs === null ? ">131ms" : s.tUs}
                </text>
              )}
              <rect x={s.x} y={0} width={s.w} height={H} fill="transparent">
                <title>
                  {s.mark ? "Mark (IR on)" : "Space (IR off)"}: {s.tUs === null ? "> 131.068 ms" : `${s.tUs} µs`}
                </title>
              </rect>
            </g>
          ))}
          <path className="ir-trace" d={d} />
        </svg>
      </div>
      <p className="small muted">Durations in µs; marks (IR on) are high. Hover a pulse for details.</p>
    </div>
  );
}

function TvOnView({ tv, onRelay }: { tv: TvOnState; onRelay: (a: TvRelayAction, label: string) => void }) {
  const known = TVON_STATES.some(([s]) => s === tv.powerState);
  const states = known ? TVON_STATES : [...TVON_STATES, [tv.powerState, tv.powerStateName] as [number, string]];
  const irPhase = tv.powerState >= 5 && tv.powerState <= 7;
  const relayWhy = [
    tv.relayPowerOn && "power-on pulse",
    tv.relayManual && "manual",
    tv.relayManualPulse && "manual pulse",
  ].filter(Boolean);
  return (
    <div className="tvon">
      <div className="tvon-states">
        {states.map(([s, name]) => (
          <span key={s} className={s === tv.powerState ? "tvon-state active" : "tvon-state"}>
            {name}
          </span>
        ))}
      </div>
      <p className="small">
        <strong>{tv.powerStateName}</strong> — {tv.powerStateDescription}
      </p>
      <table className="kv">
        <tbody>
          <tr>
            <th>Power-sense GPIO</th>
            <td>
              <span className={`dot ${tv.gpioHigh ? "on" : ""}`} aria-hidden /> {tv.gpioHigh ? "HIGH" : "low"}
            </td>
          </tr>
          <tr>
            <th>TV relay</th>
            <td>
              <span className={`dot ${tv.relayOn ? "on" : ""}`} aria-hidden /> {tv.relayOn ? "ON" : "off"}
              {relayWhy.length > 0 && <span className="muted"> ({relayWhy.join(", ")})</span>}
            </td>
          </tr>
          <tr>
            <th>IR commands</th>
            <td>
              {tv.irCommandCount === 0
                ? "none configured"
                : irPhase
                  ? `sending ${tv.irCommandIndex + 1} of ${tv.irCommandCount}`
                  : `${tv.irCommandCount} configured`}
            </td>
          </tr>
        </tbody>
      </table>
      <div className="toolbar tvon-buttons">
        <button onClick={() => onRelay("on", "manual on")} disabled={tv.relayManual}>
          Relay On
        </button>
        <button onClick={() => onRelay("off", "manual off")} disabled={!tv.relayManual}>
          Relay Off
        </button>
        <button onClick={() => onRelay("pulse", "pulse")}>Pulse relay</button>
      </div>
      <p className="small muted">
        The relay only does something if <code>tvon.relay</code> is configured. Its pulse length is{" "}
        <code>tvon.relay.pulseTime</code>.
      </p>
    </div>
  );
}
