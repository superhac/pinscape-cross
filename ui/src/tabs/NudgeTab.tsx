import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DeviceSnapshot } from "../api";
import { CALIBRATION_MS, NudgeParams, NudgeStatus, nudgeApi, paramsEqual } from "../lib/api-nudge";
import { confirm } from "../lib/confirm";
import AxisView from "../nudge/AxisView";
import { Col, History } from "../nudge/history";
import LiveChart from "../nudge/LiveChart";
import ParamsEditor, { paramsProblem } from "../nudge/ParamsEditor";
import PinSim from "../nudge/PinSim";
import RecordControls from "../nudge/RecordControls";
import { saveTextAs, stampedName, toCsv, useRecorder } from "../lib/api-export";
import "./NudgeTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; poll the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

/** Status poll period (~40 Hz); requests never overlap. */
const POLL_MS = 25;
/** Slower polling while no accelerometer is present, or after an error. */
const IDLE_POLL_MS = 1000;
/** Consecutive zero-timestamp replies before showing the "not configured" page. */
const NO_ACCEL_POLLS = 3;

const RAW: [Col, Col, Col] = ["xRaw", "yRaw", "zRaw"];
const FILTERED: [Col, Col, Col] = ["xFiltered", "yFiltered", "zFiltered"];
const VELOCITY: [Col, Col, Col] = ["vx", "vy", "vz"];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function ago(us: number): string {
  const s = us / 1e6;
  if (s < 120) return `${s.toFixed(1)} s ago`;
  if (s < 7200) return `${(s / 60).toFixed(1)} min ago`;
  return `${(s / 3600).toFixed(1)} h ago`;
}

export default function NudgeTab({ dev, visible, onStatus }: Props) {
  const key = dev.key;
  const history = useMemo(() => new History(10), []);
  const [status, setStatus] = useState<NudgeStatus | null>(null);
  const [version, setVersion] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const [noAccel, setNoAccel] = useState(0);
  const [paused, setPaused] = useState(false);
  const [zoom, setZoom] = useState(4);
  const [devParams, setDevParams] = useState<NudgeParams | null>(null);
  const [draft, setDraft] = useState<NudgeParams | null>(null);
  const [busy, setBusy] = useState(false);
  const [autoSaveCal, setAutoSaveCal] = useState(false);
  const [calStart, setCalStart] = useState<number | null>(null);

  const pausedRef = useRef(paused);
  pausedRef.current = paused;
  const noAccelRef = useRef(noAccel);
  noAccelRef.current = noAccel;
  const calibratingRef = useRef(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const devParamsRef = useRef(devParams);
  devParamsRef.current = devParams;
  const simInput = useRef({ vx: 0, vy: 0, scale: 100 });

  const dirty = draft != null && devParams != null && !paramsEqual(draft, devParams);

  // ---- CSV capture (same columns as the Windows tool's capture, plus velocity) ----
  const lastCapT = useRef(-1);
  const rec = useRecorder<NudgeStatus>((samples) => {
    if (!samples.length) {
      onStatus("Nothing recorded (no new readings).");
      return;
    }
    const header = ["timestamp", "xRaw", "yRaw", "zRaw", "xFiltered", "yFiltered", "zFiltered", "xAvg", "yAvg", "zAvg",
      "xCenter", "yCenter", "zCenter", "vx", "vy", "vz"];
    const rows = samples.map((s) => [s.timestamp, s.xRaw, s.yRaw, s.zRaw, s.xFiltered, s.yFiltered, s.zFiltered,
      s.xAvg, s.yAvg, s.zAvg, s.xCenter, s.yCenter, s.zCenter, s.vx, s.vy, s.vz]);
    saveTextAs(toCsv(header, rows), { title: "Save nudge readings", defaultName: stampedName(`${dev.ids?.unit_name ?? "pinscape"}-nudge`, "csv") })
      .then((p) => onStatus(p ? `Saved ${samples.length} nudge readings to ${p}` : `Recording discarded (${samples.length} readings).`))
      .catch((e) => onStatus(`Saving the readings failed: ${e}`));
  });
  const recPush = rec.push;

  // ---- parameters ----
  const loadParams = useCallback(
    (keepEdits = false) =>
      nudgeApi
        .params(key)
        .then((p) => {
          const d = draftRef.current;
          const dp = devParamsRef.current;
          const edited = keepEdits && d && dp && !paramsEqual(d, dp);
          setDevParams(p);
          // keep the user's edits, but take the (possibly recalibrated) thresholds
          setDraft(edited ? { ...d, xThreshold: p.xThreshold, yThreshold: p.yThreshold, zThreshold: p.zThreshold } : p);
        })
        .catch((e) => onStatus(`Nudge parameters: ${e}`)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  );

  // device changed: start over
  useEffect(() => {
    history.clear();
    setStatus(null);
    setDevParams(null);
    setDraft(null);
    setNoAccel(0);
    setCalStart(null);
    calibratingRef.current = false;
  }, [key, history]);

  useEffect(() => {
    if (visible && !devParams) loadParams();
  }, [visible, devParams, loadParams]);

  const onSample = useCallback(
    (s: NudgeStatus) => {
      setNoAccel((n) => (s.hasAccelerometer ? 0 : n + 1));
      // calibration start/end transitions
      if (s.calibrating && !calibratingRef.current) setCalStart((t) => t ?? Date.now());
      if (!s.calibrating && calibratingRef.current) {
        setCalStart(null);
        onStatus("Noise calibration finished; new quiet thresholds are in effect.");
        loadParams(true);
      }
      calibratingRef.current = s.calibrating;
      // record each device reading once (polls can return the same sample twice)
      if (s.hasAccelerometer && s.timestamp !== lastCapT.current) {
        lastCapT.current = s.timestamp;
        recPush(s);
      }
      simInput.current = { vx: s.vx, vy: s.vy, scale: devParamsRef.current?.velocityScalingFactor ?? 100 };
      if (pausedRef.current) return;
      setStatus(s);
      if (history.push(s)) setVersion((v) => v + 1);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [history, loadParams, recPush],
  );

  // ---- live status polling: one request at a time, while visible ----
  useEffect(() => {
    if (!visible) return;
    let stop = false;
    (async () => {
      while (!stop) {
        const t0 = performance.now();
        let wait = POLL_MS;
        try {
          const s = await nudgeApi.status(key);
          if (stop) break;
          onSample(s);
          setErr(null);
          if (!s.hasAccelerometer && noAccelRef.current + 1 >= NO_ACCEL_POLLS) wait = IDLE_POLL_MS;
        } catch (e) {
          if (stop) break;
          setErr(String(e));
          wait = IDLE_POLL_MS;
        }
        await sleep(Math.max(0, wait - (performance.now() - t0)));
      }
    })();
    return () => {
      stop = true;
    };
  }, [visible, key, onSample]);

  // ---- actions ----
  const act = async (what: string, fn: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await fn();
      if (done) onStatus(done);
    } catch (e) {
      onStatus(`${what} failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const apply = async (): Promise<boolean> => {
    if (!draft) return false;
    const problem = paramsProblem(draft);
    if (problem) {
      onStatus(problem);
      return false;
    }
    const p = await nudgeApi.putParams(key, draft);
    setDevParams(p);
    setDraft(p);
    return true;
  };

  const onApply = () =>
    act("Apply", apply, "Nudge settings applied (in effect now, not yet saved to flash).");

  const onSave = () =>
    act(
      "Save",
      async () => {
        if (dirty && !(await apply())) throw new Error("settings not applied");
        await nudgeApi.commit(key);
      },
      "Nudge settings and calibration saved to the device's flash.",
    );

  const onRevert = async () => {
    const ok = await confirm({
      title: "Revert nudge settings",
      body:
        "Reload the nudge parameters and noise calibration last saved in the device's flash memory? Any applied but " +
        "unsaved changes (and edits not yet applied) are discarded.",
      ok: "Revert",
      danger: true,
    });
    if (!ok) return;
    act(
      "Revert",
      async () => {
        const p = await nudgeApi.revert(key);
        setDevParams(p);
        setDraft(p);
      },
      "Nudge settings reverted to the saved values.",
    );
  };

  const onCalibrate = async () => {
    const ok = await confirm({
      title: "Noise calibration",
      body:
        `Calibration measures the accelerometer's noise at rest for about ${CALIBRATION_MS / 1000} seconds and uses it ` +
        "to set the auto-centering quiet thresholds. Don't touch or bump the cabinet (or walk around it) until it " +
        "finishes." +
        (autoSaveCal ? " The result will be saved to flash automatically." : " The result takes effect immediately; use Save to keep it."),
      ok: "Start calibration",
    });
    if (!ok) return;
    act(
      "Calibration",
      async () => {
        await nudgeApi.calibrate(key, autoSaveCal);
        setCalStart(Date.now());
      },
      "Noise calibration running: keep the cabinet still.",
    );
  };

  const onCenter = () => act("Center", () => nudgeApi.center(key), "Center point set from the recent average.");

  if (!visible) return null;

  // ---- not configured ----
  if (noAccel >= NO_ACCEL_POLLS) {
    return (
      <div className="empty">
        <div className="nudge-empty">
          <h2>No accelerometer configured</h2>
          <p className="muted">
            The device isn't reading an accelerometer, so there's no nudge input. Pinscape Pico reads nudge data from an
            accelerometer chip declared in its JSON configuration: one of <code>lis3dh</code>, <code>lis3dsh</code>,{" "}
            <code>mma8451q</code>, <code>mxc6655xa</code>, <code>mc3416</code> or <code>mpu6050</code> (with its I2C/SPI
            bus and interrupt pin), plus a <code>nudge: {"{ … }"}</code> section, which turns nudging on and can select
            the <code>source</code> device and map its axes to the cabinet (<code>x: "+X"</code>, <code>y: "+Y"</code>, <code>z: "+Z"</code>).
          </p>
          <p className="muted">
            If you've configured one, check the Log tab for accelerometer errors (wrong address or bus, chip not
            responding). Otherwise open the Config tab to add it, save to the device, and come back here once it has
            rebooted.
          </p>
          <button onClick={() => setNoAccel(0)}>Check again</button>
        </div>
      </div>
    );
  }

  const s = status;
  const oneG = s && s.gRange > 0 ? 32768 / s.gRange : 16384;
  const autoCenter = devParams?.autoCenter ?? false;
  const calibrating = s?.calibrating ?? false;
  const calPct = calibrating && calStart ? Math.min(100, ((Date.now() - calStart) / CALIBRATION_MS) * 100) : 0;
  const center = s && autoCenter ? { x: s.xCenter, y: s.yCenter, z: s.zCenter } : null;

  return (
    <div className="nudge">
      <div className="toolbar">
        <button onClick={() => setPaused((p) => !p)} title="Freeze the live views and charts">
          {paused ? "Resume" : "Pause"}
        </button>
        <label className="inline" title="Magnification of the X/Y and Z views (charts auto-scale)">
          Scale
          <select value={zoom} onChange={(e) => setZoom(Number(e.target.value))}>
            <option value={1}>Full (±32768)</option>
            <option value={4}>×4</option>
            <option value={16}>×16</option>
            <option value={64}>×64</option>
          </select>
        </label>
        <span className="sep" />
        <button onClick={onCenter} disabled={busy || !s} title="Set the center point now from the last half second of readings">
          Center now
        </button>
        <button onClick={onCalibrate} disabled={busy || calibrating || !s} title="Measure the at-rest noise (~15 s)">
          {calibrating ? "Calibrating…" : "Calibrate"}
        </button>
        <label className="inline small" title="Commit the calibration result to flash when it finishes">
          <input type="checkbox" checked={autoSaveCal} onChange={(e) => setAutoSaveCal(e.target.checked)} />
          save when done
        </label>
        <span className="sep" />
        <RecordControls rec={rec} disabled={!s} what="the accelerometer readings" />
        <span className="spacer" />
        {s?.modified && (
          <span className="badge" title="The device's in-memory settings differ from those saved in flash">
            Unsaved settings
          </span>
        )}
        {dirty && (
          <span className="badge nudge-badge-edit" title="Edited here but not yet sent to the device">
            Edits not applied
          </span>
        )}
      </div>

      {err && <p className="error">{err}</p>}

      {calibrating && (
        <div className="nudge-cal" role="status">
          <strong>Noise calibration in progress: don't touch the cabinet.</strong>
          <div className="nudge-progress">
            <div style={{ width: `${calPct}%` }} />
          </div>
          <span className="muted small">{Math.max(0, Math.ceil(((100 - calPct) / 100) * (CALIBRATION_MS / 1000)))} s</span>
        </div>
      )}

      <div className="nudge-info small">
        <span>
          <span className={`dot ${s?.hasAccelerometer ? (paused ? "paused" : "on") : ""}`} />
          {s ? (paused ? "Paused" : "Live") : "Waiting for data…"}
        </span>
        {s && <span>Range ±{s.gRange} g (1 g = {Math.round(oneG)} units)</span>}
        {s && (
          <span>
            Last centered {ago(s.lastCenteringAgeUs)}
            {!autoCenter && devParams && <span className="muted"> (auto-centering off)</span>}
          </span>
        )}
        <span className="muted">All axes: −32768 … +32767</span>
      </div>

      <div className="nudge-views">
        <AxisView
          title="Raw accelerometer"
          help="Readings mapped to the cabinet axes, before centering and filtering. Dashed lines: current center point. Z is drawn relative to 1 g."
          x={s?.xRaw ?? 0}
          y={s?.yRaw ?? 0}
          z={s?.zRaw ?? 0}
          zMid={oneG}
          center={center}
          history={history}
          cols={RAW}
          zoom={zoom}
        />
        <AxisView
          title="Filtered (nudge output)"
          help="After centering, DC blocker, low-pass and jitter filters: what the pinball simulator sees as acceleration. Z is drawn relative to 1 g."
          x={s?.xFiltered ?? 0}
          y={s?.yFiltered ?? 0}
          z={s?.zFiltered ?? 0}
          zMid={oneG}
          history={history}
          cols={FILTERED}
          zoom={zoom}
        />
        <AxisView
          title="Velocity"
          help="Integrated cabinet velocity, scaled by the velocity scaling factor."
          x={s?.vx ?? 0}
          y={s?.vy ?? 0}
          z={s?.vz ?? 0}
          history={history}
          cols={VELOCITY}
          zoom={zoom}
        />
        <div className="card nudge-centering">
          <h3>Centering</h3>
          <table className="kv">
            <tbody>
              <tr>
                <th>Center</th>
                <td>{s ? `${s.xCenter}, ${s.yCenter}, ${s.zCenter}` : "–"}</td>
              </tr>
              <tr>
                <th>Average</th>
                <td>{s ? `${s.xAvg}, ${s.yAvg}, ${s.zAvg}` : "–"}</td>
              </tr>
              <tr>
                <th>Quiet thresholds</th>
                <td>{s ? `${s.xThreshold}, ${s.yThreshold}, ${s.zThreshold}` : "–"}</td>
              </tr>
              <tr>
                <th>Auto-centering</th>
                <td>{devParams ? (autoCenter ? `on, ${devParams.autoCenterIntervalMs} ms` : "off") : "–"}</td>
              </tr>
            </tbody>
          </table>
          <p className="muted small">
            Auto-centering takes the trailing average as the new center after a period of stillness. Noise calibration
            sets the quiet thresholds from the sensor's at-rest noise (4 standard deviations).
          </p>
        </div>
      </div>

      <div className="card nudge-charts">
        <h3>
          Last 10 seconds <span className="muted small">(click a legend entry to show/hide an axis)</span>
        </h3>
        <div className="nudge-chart-title">Raw</div>
        <LiveChart history={history} version={version} cols={RAW} hideZ />
        <div className="nudge-chart-title">Filtered</div>
        <LiveChart history={history} version={version} cols={FILTERED} hideZ />
        <div className="nudge-chart-title">Velocity</div>
        <LiveChart history={history} version={version} cols={VELOCITY} />
      </div>

      <div className="nudge-bottom">
        <div className="card">
          <h3>Settings</h3>
          {draft && devParams ? (
            <>
              <ParamsEditor draft={draft} device={devParams} onChange={setDraft} disabled={busy} />
              <div className="toolbar nudge-actions">
                <button
                  className="primary"
                  onClick={onApply}
                  disabled={busy || !dirty}
                  title="Send to the device; takes effect immediately but isn't saved to flash"
                >
                  Apply
                </button>
                <button onClick={() => setDraft(devParams)} disabled={busy || !dirty} title="Discard edits not yet applied">
                  Discard edits
                </button>
                <span className="sep" />
                <button
                  onClick={onSave}
                  disabled={busy || (!dirty && !s?.modified)}
                  title="Save the device's current settings and calibration to flash (applies pending edits first)"
                >
                  Save to device
                </button>
                <button
                  className="danger"
                  onClick={onRevert}
                  disabled={busy}
                  title="Reload the settings and calibration last saved in flash"
                >
                  Revert
                </button>
                <span className="spacer" />
                <button onClick={() => loadParams()} disabled={busy} title="Re-read the parameters from the device">
                  Reload
                </button>
              </div>
              <p className="muted small">
                Apply puts the settings into effect right away (in RAM only); Save to device writes them, with the noise
                calibration, to flash so they survive a reboot; Revert reloads the saved values.
              </p>
            </>
          ) : (
            <p className="muted">Loading settings…</p>
          )}
        </div>
        <div className="card nudge-sim-card">
          <h3>Physics tester</h3>
          <PinSim input={simInput} running={!paused} />
          <p className="muted small">
            A ball on a tilted playfield, pushed by the device's velocity output: nudge the cabinet and watch the ball
            react. Click the playfield to place the ball.
          </p>
        </div>
      </div>
    </div>
  );
}
