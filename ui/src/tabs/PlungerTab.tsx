import { useCallback, useEffect, useRef, useState } from "react";
import type uPlot from "uplot";
import { DeviceSnapshot } from "../api";
import {
  calFromReading,
  FIRING_STATES,
  PlungerCal,
  PlungerConfig,
  PlungerReading,
  plungerApi,
  SENSOR,
  SensorData,
} from "../lib/api-plunger";
import { confirm } from "../lib/confirm";
import Gauge from "../plunger/Gauge";
import PixelView from "../plunger/PixelView";
import PlungerChart from "../plunger/PlungerChart";
import "./PlungerTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; poll the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

/** Reading poll interval (~40 Hz); requests never overlap. */
const POLL_MS = 25;
/** Image-sensor snapshots are ~1.5 KB each; fetch them at ~10 Hz. */
const PIXEL_MS = 100;
const RETRY_MS = 600;
const HISTORY_S = 8;
const PEAK_S = 3;
/** The firmware's calibration period. */
const CAL_SECONDS = 15;
/** Brief states (Fired lasts ~50 ms) stay lit at least this long so they're visible. */
const LATCH_MS = 350;
/** Joystick Z display range (values below are clipped in the gauge). */
const Z_MIN = -8192;
const Z_MAX = 32767;

type ImageSnap = Extract<SensorData, { kind: "imageSensor" }>;

interface History {
  t: number[];
  z0: number[];
  z: number[];
  speed: number[];
}

const emptyHistory = (): History => ({ t: [], z0: [], z: [], speed: [] });

export default function PlungerTab({ dev, visible, onStatus }: Props) {
  const key = dev.key;
  const [cfg, setCfg] = useState<PlungerConfig | null>(null);
  const [cfgErr, setCfgErr] = useState<string | null>(null);
  const [rd, setRd] = useState<PlungerReading | null>(null);
  const [image, setImage] = useState<ImageSnap | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [showPixels, setShowPixels] = useState(true);
  const [chart, setChart] = useState<uPlot.AlignedData>([[], [], [], []]);
  const [busy, setBusy] = useState(false);
  const [autoSave, setAutoSave] = useState(false);
  /** Calibration in effect before the last Calibrate click, for undo. */
  const [prevCal, setPrevCal] = useState<PlungerCal | null>(null);
  const [calStart, setCalStart] = useState<number | null>(null);

  const hist = useRef<History>(emptyHistory());
  const latch = useRef<Record<number, number>>({});
  const zbSeen = useRef(0);
  const calStartRef = useRef<number | null>(null);
  const wantPixels = useRef(false);
  wantPixels.current = !!cfg?.imageSensor && showPixels;
  const statusRef = useRef(onStatus);
  statusRef.current = onStatus;

  // ---- configuration (re-read whenever the tab is shown: the config file may have changed) ----
  const loadConfig = useCallback(() => {
    plungerApi
      .config(key)
      .then((c) => {
        setCfg(c);
        setCfgErr(null);
      })
      .catch((e) => setCfgErr(String(e)));
  }, [key]);

  useEffect(() => {
    setCfg(null);
    setRd(null);
    setImage(null);
    setPrevCal(null);
    setCalStart(null);
    calStartRef.current = null;
    hist.current = emptyHistory();
    setChart([[], [], [], []]);
  }, [key]);

  useEffect(() => {
    if (visible) loadConfig();
  }, [visible, loadConfig]);

  // ---- live readings ----
  const onReading = useCallback((r: PlungerReading, withPixels: boolean) => {
    const now = Date.now();
    const h = hist.current;
    if (h.t.length && r.timestamp < h.t[h.t.length - 1]) Object.assign(h, emptyHistory()); // device rebooted
    h.t.push(r.timestamp);
    h.z0.push(r.z0);
    h.z.push(r.z);
    h.speed.push(r.speed);
    const cutoff = r.timestamp - HISTORY_S * 1e6;
    let drop = 0;
    while (drop < h.t.length && h.t[drop] < cutoff) drop++;
    if (drop) for (const a of [h.t, h.z0, h.z, h.speed]) a.splice(0, drop);
    const t0 = r.timestamp;
    setChart([h.t.map((t) => (t - t0) / 1e6), [...h.z0], [...h.z], [...h.speed]]);

    latch.current[r.firingState] = now;
    if (r.zbLaunch) zbSeen.current = now;
    if (withPixels && r.sensor?.kind === "imageSensor") setImage(r.sensor);

    // calibration progress / completion
    if (r.calibrating && calStartRef.current == null) {
      calStartRef.current = now;
      setCalStart(now);
    } else if (!r.calibrating && calStartRef.current != null) {
      calStartRef.current = null;
      setCalStart(null);
      statusRef.current(
        r.calibrated
          ? "Calibration finished. Try it out, then Save to keep it (or Revert)."
          : "Calibration didn't detect any plunger travel; defaults are in effect. Run it again.",
      );
    }
    setRd(r);
  }, []);

  useEffect(() => {
    if (!visible || !cfg?.configured || paused) return;
    let stop = false;
    let timer: number | undefined;
    let lastPix = -Infinity;
    const tick = async () => {
      const started = performance.now();
      const pix = wantPixels.current && started - lastPix >= PIXEL_MS;
      let delay = POLL_MS;
      try {
        const r = await plungerApi.reading(key, pix);
        if (stop) return;
        if (pix) lastPix = started;
        onReading(r, pix);
        setErr(null);
      } catch (e) {
        if (stop) return;
        setErr(String(e));
        delay = RETRY_MS;
      }
      timer = window.setTimeout(tick, Math.max(0, delay - (performance.now() - started)));
    };
    tick();
    return () => {
      stop = true;
      clearTimeout(timer);
    };
  }, [visible, cfg, paused, key, onReading]);

  // ---- actions ----
  const act = async (what: string, fn: () => Promise<void>, done?: string) => {
    setBusy(true);
    try {
      await fn();
      if (done) onStatus(done);
      return true;
    } catch (e) {
      onStatus(`${what} failed: ${e}`);
      return false;
    } finally {
      setBusy(false);
    }
  };

  const calibrate = () => {
    if (rd) setPrevCal(calFromReading(rd));
    const now = Date.now();
    calStartRef.current = now;
    setCalStart(now);
    act(
      "Calibration",
      () => plungerApi.calibrate(key, autoSave),
      autoSave ? "Calibration started; the result is saved automatically." : "Calibration started.",
    ).then((ok) => {
      if (!ok) {
        calStartRef.current = null;
        setCalStart(null);
      }
    });
  };

  const undoCal = () => {
    if (prevCal) act("Restore calibration", () => plungerApi.setCalibration(key, prevCal), "Previous calibration restored (not saved yet).");
  };

  const save = () => act("Save", () => plungerApi.commit(key), "Plunger settings and calibration saved to the Pico's flash.");

  const revert = async () => {
    const ok = await confirm({
      title: "Revert plunger settings",
      body: "Discard the unsaved calibration and settings changes and reload the ones saved on the Pico?",
      ok: "Revert",
    });
    if (ok) act("Revert", () => plungerApi.revert(key), "Plunger settings reverted to the saved ones.");
  };

  const restoreDefaults = async () => {
    const ok = await confirm({
      title: "Restore plunger defaults",
      body:
        "Reset the calibration (to the sensor's full range, uncalibrated) and all plunger settings to the firmware defaults? " +
        "This takes effect now but isn't saved until you click Save.",
      ok: "Restore defaults",
      danger: true,
    });
    if (ok) act("Restore defaults", () => plungerApi.restoreDefaults(key), "Plunger defaults restored (not saved yet).");
  };

  // ---- rendering ----
  if (!visible) return null;

  if (!cfg) {
    return (
      <div className="plunger">
        {cfgErr ? (
          <>
            <p className="error">{cfgErr}</p>
            <div>
              <button onClick={loadConfig}>Retry</button>
            </div>
          </>
        ) : (
          <p className="muted">Loading plunger configuration…</p>
        )}
      </div>
    );
  }

  if (!cfg.configured) return <NotConfigured onReload={loadConfig} />;

  const calibrating = rd?.calibrating ?? false;
  const now = Date.now();
  const lit = (s: number) => rd?.firingState === s || now - (latch.current[s] ?? 0) < LATCH_MS;
  const zbLit = (rd?.zbLaunch ?? false) || now - zbSeen.current < LATCH_MS;
  const peakSpeed = peakForward(hist.current);
  const calElapsed = calStart != null ? Math.min(CAL_SECONDS, (now - calStart) / 1000) : 0;
  const isVcnl = cfg.sensorType === SENSOR.VCNL4010;

  return (
    <div className="plunger">
      <div className="toolbar">
        <strong>{cfg.sensorTypeName}</strong>
        <span className="muted small">
          native scale 0–{cfg.nativeScale}
          {cfg.flags ? `, flags 0x${cfg.flags.toString(16)}` : ""}
        </span>
        {rd?.modified && (
          <span className="badge" title="The live settings or calibration differ from the ones saved on the Pico">
            unsaved changes
          </span>
        )}
        <span className="sep" />
        <button className="primary" onClick={save} disabled={busy || !rd?.modified || calibrating} title="Save the settings and calibration to flash">
          Save
        </button>
        <button onClick={revert} disabled={busy || !rd?.modified} title="Reload the settings saved on the Pico">
          Revert
        </button>
        <button className="danger" onClick={restoreDefaults} disabled={busy || calibrating} title="Firmware defaults for calibration and all settings">
          Restore defaults
        </button>
        <span className="spacer" />
        <label className="inline">
          <input type="checkbox" checked={paused} onChange={(e) => setPaused(e.target.checked)} /> Pause
        </label>
        <button onClick={loadConfig} title="Re-read the plunger configuration">
          Reload
        </button>
      </div>

      {err && <p className="error">{err}</p>}

      {calibrating && (
        <div className="pl-banner" role="status">
          <div>
            <strong>Calibration in progress</strong> — {Math.max(0, Math.ceil(CAL_SECONDS - calElapsed))} s left
          </div>
          <div className="pl-progress">
            <div style={{ width: `${(calElapsed / CAL_SECONDS) * 100}%` }} />
          </div>
          <CalSteps />
        </div>
      )}

      <div className="cards">
        <section className="card wide">
          <h3>
            Live sensor view
            {cfg.imageSensor && (
              <label className="inline small">
                <input type="checkbox" checked={showPixels} onChange={(e) => setShowPixels(e.target.checked)} /> show pixels
              </label>
            )}
          </h3>
          {!rd ? (
            <p className="muted">Waiting for readings…</p>
          ) : (
            <>
              {cfg.imageSensor && showPixels && (
                <div className="pl-row">
                  <span className="pl-label">Image</span>
                  {image ? (
                    <PixelView
                      pixels={image.pixels}
                      sensorType={cfg.sensorType}
                      rawPos={rd.rawPos}
                      nativeScale={cfg.nativeScale}
                      reverse={rd.reverse}
                    />
                  ) : (
                    <span className="muted small">Waiting for an image…</span>
                  )}
                </div>
              )}
              <SensorExtra sensor={rd.sensor} />
              <div className="pl-row">
                <span className="pl-label" title="Raw sensor reading, native units">
                  Sensor
                </span>
                <Gauge
                  value={rd.rawPos}
                  min={0}
                  max={cfg.nativeScale}
                  kind="raw"
                  window={!isVcnl && rd.jfWindow > 0 ? [rd.jfLo, rd.jfHi] : null}
                  tick={!isVcnl && rd.jfWindow > 0 ? rd.jfLastPre : null}
                  markers={[
                    { at: rd.calMin, label: `min ${rd.calMin}`, cls: "cal low", title: "Calibration minimum (fully forward)" },
                    { at: rd.calZero, label: `zero ${rd.calZero}`, cls: "cal", title: "Calibration zero (rest position)" },
                    { at: rd.calMax, label: `max ${rd.calMax}`, cls: "cal low", title: "Calibration maximum (fully retracted)" },
                  ]}
                  title={`Raw ${rd.rawPos} of ${cfg.nativeScale}`}
                />
                <span className="pl-num">{rd.rawPos}</span>
              </div>
              <div className="pl-row">
                <span className="pl-label" title="Instantaneous joystick Z axis (no firing-event processing)">
                  Z0
                </span>
                <Gauge value={rd.z0} min={Z_MIN} max={Z_MAX} origin={0} kind="z0" />
                <span className="pl-num">{rd.z0}</span>
              </div>
              <div className="pl-row">
                <span className="pl-label" title="Joystick Z axis as reported to the PC (with firing-event processing)">
                  Z
                </span>
                <Gauge value={rd.z} min={Z_MIN} max={Z_MAX} origin={0} kind="z" />
                <span className="pl-num">{rd.z}</span>
              </div>
              <p className="muted small pl-hint">
                Bars move right as the plunger is pulled back. If they move the wrong way, turn on <em>Reverse orientation</em> below.
                Z axis: 0 = rest position, 32767 = fully retracted.
              </p>
            </>
          )}
        </section>

        <section className="card">
          <h3>Readings</h3>
          {rd && (
            <table className="kv">
              <tbody>
                <tr>
                  <th>Raw position</th>
                  <td>
                    {rd.rawPos} <span className="muted">/ {cfg.nativeScale}</span>
                  </td>
                </tr>
                <tr>
                  <th>Z axis</th>
                  <td>
                    {rd.z} <span className="muted">(Z0 {rd.z0})</span>
                  </td>
                </tr>
                <tr>
                  <th>Speed</th>
                  <td>
                    {rd.speed} <span className="muted">Z/cs</span>
                  </td>
                </tr>
                <tr>
                  <th>Peak forward</th>
                  <td title={`Fastest forward speed over the last ${PEAK_S} s`}>
                    {peakSpeed} <span className="muted">Z/cs</span>
                  </td>
                </tr>
                <tr>
                  <th>Firing state</th>
                  <td className="pl-states">
                    {FIRING_STATES.map((n, i) => (
                      <span key={n} className={`pl-state ${lit(i) ? "on" : ""} s${i}`}>
                        {n}
                      </span>
                    ))}
                  </td>
                </tr>
                <tr>
                  <th>ZB Launch</th>
                  <td>
                    <span className={`dot ${zbLit ? "on" : ""}`} />
                    {zbLit ? "triggered" : "idle"}
                  </td>
                </tr>
                <tr>
                  <th>Flags</th>
                  <td className="pl-flags">
                    <Flag on={rd.calibrated} label="calibrated" />
                    <Flag on={rd.calibrating} label="calibrating" />
                    <Flag on={rd.reverse} label="reverse" />
                    <Flag on={rd.modified} label="modified" />
                    <Flag on={rd.zbLaunch} label="ZB launch" />
                  </td>
                </tr>
                {rd.jfWindow > 0 && (
                  <tr>
                    <th>Jitter filter</th>
                    <td>
                      {rd.jfLastPre} → {rd.jfLastPost}{" "}
                      <span className="muted">
                        (window {rd.jfLo}–{rd.jfHi})
                      </span>
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          )}
        </section>

        <section className="card">
          <h3>Calibration</h3>
          {rd && (
            <>
              <div className="pl-calstatus">
                {calibrating ? (
                  <span className="pl-calrun">Calibrating…</span>
                ) : rd.calibrated ? (
                  <span className="pl-ok">Calibrated</span>
                ) : (
                  <span className="warn">Not calibrated</span>
                )}
              </div>
              <table className="kv">
                <tbody>
                  <tr>
                    <th>Minimum</th>
                    <td>{rd.calMin}</td>
                  </tr>
                  <tr>
                    <th>Zero (rest)</th>
                    <td>{rd.calZero}</td>
                  </tr>
                  <tr>
                    <th>Maximum</th>
                    <td>{rd.calMax}</td>
                  </tr>
                  <tr>
                    <th>Firing time</th>
                    <td title="Average release time observed during the last calibration">
                      {rd.firingTimeMeasured ? `${(rd.firingTimeMeasured / 1000).toFixed(2)} ms` : "—"}
                    </td>
                  </tr>
                </tbody>
              </table>
              {!calibrating && (
                <p className="muted small">
                  With the plunger at rest, click Calibrate. Then pull it all the way back, hold a moment, release, and let it come
                  to rest; repeat a few times during the {CAL_SECONDS} seconds.
                </p>
              )}
              <div className="actions">
                <button className="primary" onClick={calibrate} disabled={busy || calibrating}>
                  Calibrate
                </button>
                <label className="inline small" title="Save the result to flash automatically when calibration ends">
                  <input type="checkbox" checked={autoSave} onChange={(e) => setAutoSave(e.target.checked)} /> save automatically
                </label>
                {prevCal && !calibrating && (
                  <button onClick={undoCal} disabled={busy} title="Put back the calibration that was in effect before the last run">
                    Undo calibration
                  </button>
                )}
              </div>
            </>
          )}
        </section>

        <section className="card">
          <h3>Settings</h3>
          {rd && (
            <div className="pl-settings">
              <label className="inline pl-setting-check">
                <input
                  type="checkbox"
                  checked={rd.reverse}
                  disabled={busy}
                  onChange={(e) => {
                    const v = e.target.checked;
                    act("Orientation", () => plungerApi.setOrientation(key, v), `Orientation: ${v ? "reversed" : "standard"} (not saved yet).`);
                  }}
                />
                Reverse orientation
              </label>
              <NumberSetting
                label="Jitter filter window"
                unit={isVcnl ? "prox counts" : "native units"}
                value={rd.jfWindow}
                min={0}
                max={65535}
                help="Hysteresis window that hides sensor noise while the plunger is still; 0 disables it. Use the smallest window that steadies the reading."
                apply={(v) => act("Jitter filter", () => plungerApi.setJitterFilter(key, v), `Jitter filter window set to ${v}.`)}
              />
              <NumberSetting
                label="Maximum firing time"
                unit="ms"
                value={rd.firingTimeLimit / 1000}
                min={1}
                max={1000}
                step={0.1}
                help="Release motions that reach the rest position within this time count as firing events (ZB Launch, Z axis processing). Start a little above the measured firing time."
                apply={(v) =>
                  act("Firing time limit", () => plungerApi.setFiringTimeLimit(key, Math.round(v * 1000)), `Maximum firing time set to ${v} ms.`)
                }
              />
              <NumberSetting
                label="Manual scaling factor"
                unit="%"
                value={rd.manualScalingFactor}
                min={1}
                max={1000}
                help="Stretches or compresses the calibrated Z axis range; 100 uses the calibration as-is."
                apply={(v) => act("Scaling factor", () => plungerApi.setScalingFactor(key, v), `Scaling factor set to ${v}%.`)}
              />
              {cfg.hasIntegrationTime && (
                <NumberSetting
                  label="Integration time"
                  unit="µs"
                  value={rd.integrationTime}
                  min={0}
                  max={4000}
                  help="Exposure time per image frame; 0 uses the sensor's default. Longer is brighter but limits the frame rate (keep it under about 4 ms)."
                  apply={(v) => act("Integration time", () => plungerApi.setIntegrationTime(key, v), `Integration time set to ${v} µs.`)}
                />
              )}
              {cfg.scanModes.length > 0 && (
                <div className="pl-setting">
                  <span className="pl-setting-label">Scan mode</span>
                  <select
                    value={rd.scanMode}
                    disabled={busy}
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      act("Scan mode", () => plungerApi.setScanMode(key, v), `Scan mode: ${cfg.scanModes[v] ?? v}.`);
                    }}
                  >
                    {cfg.scanModes.map((n, i) => (
                      <option key={i} value={i}>
                        {n}
                      </option>
                    ))}
                    {rd.scanMode >= cfg.scanModes.length && <option value={rd.scanMode}>Mode {rd.scanMode}</option>}
                  </select>
                  <span className="muted small">edge detection algorithm</span>
                </div>
              )}
              <p className="muted small">
                Changes apply immediately but stay in the Pico's memory until you click <strong>Save</strong>; Revert or a Pico reset
                restores the saved settings.
              </p>
            </div>
          )}
        </section>

        <section className="card wide">
          <h3>History ({HISTORY_S} s)</h3>
          <PlungerChart data={chart} seconds={HISTORY_S} />
        </section>
      </div>
    </div>
  );
}

function peakForward(h: History): number {
  if (!h.t.length) return 0;
  const cutoff = h.t[h.t.length - 1] - PEAK_S * 1e6;
  let peak = 0;
  for (let i = h.t.length - 1; i >= 0 && h.t[i] >= cutoff; i--) peak = Math.min(peak, h.speed[i]);
  return peak;
}

function Flag({ on, label }: { on: boolean; label: string }) {
  return <span className={`pl-flag ${on ? "on" : ""}`}>{label}</span>;
}

function CalSteps() {
  return (
    <ol className="pl-steps">
      <li>Wait a couple of seconds with the plunger at rest.</li>
      <li>Pull the plunger all the way back and hold it for a second.</li>
      <li>Release it and let it come fully to rest.</li>
      <li>Repeat a few times until the timer runs out.</li>
    </ol>
  );
}

function SensorExtra({ sensor }: { sensor: SensorData | null }) {
  if (!sensor) return null;
  switch (sensor.kind) {
    case "quadrature":
      return (
        <div className="pl-row">
          <span className="pl-label">Channels</span>
          <span className="pl-quad">
            <span className={`pl-flag ${sensor.a ? "on" : ""}`}>A</span>
            <span className={`pl-flag ${sensor.b ? "on" : ""}`}>B</span>
          </span>
        </div>
      );
    case "vcnl4010":
      return (
        <div className="pl-row">
          <span className="pl-label" title="Raw IR proximity count (brightness of the reflected light)">
            Proximity
          </span>
          <Gauge value={sensor.proxCount} min={0} max={65535} kind="prox" />
          <span className="pl-num">{sensor.proxCount}</span>
        </div>
      );
    case "opaque":
      return (
        <p className="muted small">
          Sensor data: {sensor.data.length} bytes (type {sensor.sensorType}, not interpreted).
        </p>
      );
    default:
      return null;
  }
}

function NumberSetting({
  label,
  unit,
  value,
  min,
  max,
  step = 1,
  help,
  apply,
}: {
  label: string;
  unit: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  help: string;
  apply: (v: number) => Promise<boolean>;
}) {
  // null = not being edited: show the device's live value
  const [draft, setDraft] = useState<string | null>(null);
  const text = draft ?? String(value);
  const n = Number(text);
  const valid = text.trim() !== "" && Number.isFinite(n) && n >= min && n <= max && (step >= 1 ? Number.isInteger(n) : true);
  const changed = draft != null && valid && n !== value;
  const submit = async () => {
    if (!changed) {
      setDraft(null);
      return;
    }
    if (await apply(n)) setDraft(null);
  };
  return (
    <div className="pl-setting" title={help}>
      <span className="pl-setting-label">{label}</span>
      <input
        type="number"
        className={valid ? "" : "invalid"}
        value={text}
        min={min}
        max={max}
        step={step}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
          if (e.key === "Escape") setDraft(null);
        }}
      />
      <span className="muted small">{unit}</span>
      <button onClick={submit} disabled={!changed}>
        Apply
      </button>
      {draft != null && n !== value && <span className="muted small">current: {value}</span>}
    </div>
  );
}

function NotConfigured({ onReload }: { onReload: () => void }) {
  return (
    <div className="empty">
      <div className="pl-empty">
        <h2>No plunger configured</h2>
        <p className="muted">
          This device doesn't have a plunger sensor set up (the firmware reports its "null" sensor). A plunger needs a{" "}
          <code>plunger: {"{ ... }"}</code> section in the device's JSON configuration, plus the section for the sensor hardware
          it reads:
        </p>
        <ul className="muted">
          <li>
            Potentiometer: <code>pico_adc</code> (Pico GPIO 26–28) or <code>ads1115</code>
          </li>
          <li>
            Imaging sensors: <code>tcd1103</code>, <code>tsl1410r</code> (also TSL1412S)
          </li>
          <li>
            Quadrature encoder: <code>aedr8300</code>
          </li>
          <li>
            Proximity / distance: <code>vcnl4010</code>, <code>vl6180x</code>
          </li>
        </ul>
        <p className="muted">
          If more than one sensor is configured, select one with <code>plunger.source</code> (e.g. <code>"tcd1103"</code>). Open
          the Config tab to edit the configuration, save it to the device, and come back here. If the plunger section is present
          but this message persists, check the Log tab for plunger or sensor errors.
        </p>
        <button onClick={onReload}>Reload</button>
      </div>
    </div>
  );
}
