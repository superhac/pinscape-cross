import { useEffect, useRef, useState } from "react";
import { api, DeviceSnapshot, fmtUptime, fmtVersion, Version } from "../api";
import { useInterval } from "../lib/useDevices";
import { confirm } from "../lib/confirm";
import { dirtyEditors } from "../lib/dirty";
import { BootDevice, chipName, firmwareApi } from "../lib/api-firmware";
import { DashReport, DashStatus, Problem, ProblemKind, dashboardApi } from "../lib/api-dashboard";
import "./dashboard.css";

interface Props {
  devices: DeviceSnapshot[];
  /** Select a device and show its Overview. */
  onOpen: (key: string) => void;
  onStatus: (s: string) => void;
}

/** Short per-card tags for the problems a device is involved in. */
const TAGS: Partial<Record<ProblemKind, string>> = {
  "duplicate-unit": "Duplicate unit number",
  "duplicate-xinput": "Same XInput player",
  "ledwiz-overlap": "LedWiz unit overlap",
  "test-mode": "Output test mode on",
  "maybe-safe-mode": "Safe/Factory Mode?",
  "no-config": "No config stored",
};

const POLL_MS = 3000;

const verNum = (v: Version) => v.major * 1e6 + v.minor * 1e3 + v.patch;

/**
 * "All devices" view: one card per Pinscape Pico (incl. ones that can't be
 * opened) and per Pico in boot loader mode, with live status and the problems
 * found across devices. Mounted only while shown, so it only polls then; each
 * poll is one job per device on that device's own worker.
 */
export default function Dashboard({ devices, onOpen, onStatus }: Props) {
  const [report, setReport] = useState<DashReport | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [boot, setBoot] = useState<BootDevice[]>([]);
  const busy = useRef(false);

  const poll = () => {
    if (busy.current) return; // don't queue polls behind a slow device
    busy.current = true;
    dashboardApi
      .poll()
      .then((r) => {
        setReport(r);
        setErr(null);
      })
      .catch((e) => setErr(String(e)))
      .finally(() => (busy.current = false));
  };
  useInterval(poll, POLL_MS);
  // Re-poll promptly when a device comes or goes.
  const keys = devices.map((d) => d.key).join(",");
  const first = useRef(true);
  useEffect(() => {
    if (first.current) first.current = false;
    else poll();
  }, [keys]);

  useEffect(() => {
    let alive = true;
    const un = firmwareApi.onBootLoadersChanged((d) => alive && setBoot(d));
    firmwareApi
      .bootList()
      .then((l) => alive && setBoot(l.devices))
      .catch(() => {});
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);

  const present = new Set(devices.map((d) => d.key));
  const problems = (report?.problems ?? []).filter((p) => p.keys.some((k) => present.has(k)));
  const serious = problems.filter((p) => p.severity !== "info");
  const statusOf = (key: string) => report?.devices.find((d) => d.key === key);
  const newest = Math.max(0, ...devices.map((d) => (d.version ? verNum(d.version) : 0)));
  const connected = devices.filter((d) => d.ids).length;

  const reboot = async (d: DeviceSnapshot) => {
    const ids = d.ids!;
    const unsaved = dirtyEditors();
    const ok = await confirm({
      title: "Reboot device?",
      body:
        `Reboot #${ids.unit_num} ${ids.unit_name}? It disconnects briefly and reconnects automatically.` +
        (unsaved.length ? ` Unsaved edits (${unsaved.join(", ")}) stay in the editor.` : ""),
      ok: "Reboot",
    });
    if (!ok) return;
    try {
      await api.reset(d.key, "normal");
      onStatus(`#${ids.unit_num} ${ids.unit_name}: rebooting…`);
    } catch (e) {
      onStatus(`Reboot failed: ${e}`);
    }
  };

  const total = devices.length + boot.length;
  const summary = [
    `${connected} connected`,
    devices.length > connected && `${devices.length - connected} unavailable`,
    boot.length > 0 && `${boot.length} in boot loader mode`,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="dash">
      <div className="dash-head">
        <h2>All devices</h2>
        <span className="muted small">{summary}</span>
      </div>
      {err && <p className="error">{err}</p>}

      {total === 0 ? (
        <div className="empty">
          <p className="muted">No devices. Plug in a Pinscape Pico; it appears here automatically.</p>
        </div>
      ) : (
        <>
          <section className="card dash-problems" aria-label="Problems across devices">
            {serious.length === 0 ? (
              <p className="muted small">
                {connected < 2
                  ? "Connect more Picos to check them against each other (unit numbers, firmware versions, …)."
                  : report
                    ? "No problems found across the connected devices."
                    : "Checking…"}
              </p>
            ) : (
              <ul>
                {serious.map((p, i) => (
                  <ProblemLine key={i} p={p} />
                ))}
              </ul>
            )}
          </section>

          <div className="dash-grid">
            {devices.map((d) => (
              <DeviceCard
                key={d.key}
                d={d}
                status={statusOf(d.key)?.status ?? null}
                error={statusOf(d.key)?.error ?? null}
                problems={problems.filter((p) => p.keys.includes(d.key))}
                older={!!d.version && newest > 0 && verNum(d.version) < newest}
                onOpen={() => onOpen(d.key)}
                onReboot={() => reboot(d)}
              />
            ))}
            {boot.map((b) => (
              <section key={b.key} className="card dash-card">
                <div className="dash-title">
                  <strong>Pico in boot loader ({chipName(b.chip)})</strong>
                  <span className="pill boot">Boot loader</span>
                </div>
                <table className="kv">
                  <tbody>
                    <tr>
                      <th>Serial</th>
                      <td>{b.serial ?? "none"}</td>
                    </tr>
                    <tr>
                      <th>Install method</th>
                      <td>{b.picoboot ? "USB (PICOBOOT)" : "drive copy only"}</td>
                    </tr>
                  </tbody>
                </table>
                <p className="muted small">Use Install firmware… in the sidebar to set it up or update it.</p>
              </section>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function ProblemLine({ p }: { p: Problem }) {
  return (
    <li className={`sev-${p.severity}`}>
      <span className="sev-icon" aria-hidden>
        {p.severity === "error" ? "✖" : p.severity === "warning" ? "▲" : "ℹ"}
      </span>
      <span className="visually-hidden">{p.severity}: </span>
      {p.message}
    </li>
  );
}

interface CardProps {
  d: DeviceSnapshot;
  status: DashStatus | null;
  error: string | null;
  problems: Problem[];
  older: boolean;
  onOpen: () => void;
  onReboot: () => void;
}

function DeviceCard({ d, status, error, problems, older, onOpen, onReboot }: CardProps) {
  const ids = status?.ids ?? d.ids;
  const worst = problems.some((p) => p.severity === "error")
    ? "error"
    : problems.some((p) => p.severity === "warning")
      ? "warning"
      : "";
  const tags = [
    ...new Set(problems.map((p) => TAGS[p.kind]).filter((t): t is string => !!t)),
    ...(older ? ["Older firmware"] : []),
  ];

  if (!ids) {
    const what =
      d.errorKind === "permission" ? "No permission" : d.errorKind === "busy" ? "In use by another program" : "Can't open";
    return (
      <section className="card dash-card clickable sev-warning-card" onClick={onOpen}>
        <div className="dash-title">
          <button className="link-btn" onClick={stop(onOpen)}>
            Pinscape Pico
          </button>
          <span className="pill bad">{what}</span>
        </div>
        <p className="muted small">{d.error}</p>
        <p className="muted small">Open it for help with this problem.</p>
      </section>
    );
  }

  const s = status?.stats;
  const u = status?.usb;
  const ifaces = u
    ? [
        u.keyboardConfigured && "Keyboard",
        u.gamepadConfigured && "Gamepad",
        u.xinputConfigured && `XInput${ids.xinput_player_index != null ? ` (player ${ids.xinput_player_index + 1})` : ""}`,
        u.pindevConfigured && "Pinball Device",
        u.feedbackConfigured && "DOF",
        u.ledwizConfigured && `LedWiz ${ledWizUnits(ids.led_wiz_unit_mask)}`,
      ].filter(Boolean)
    : [];

  const rows: [string, string][] = [
    ["Hardware ID", ids.hwid],
    ["Firmware", d.version ? `v${fmtVersion(d.version)}` : "?"],
  ];
  if (s && status) {
    rows.push(
      ["Uptime", fmtUptime(s.upTimeUs)],
      ["Main loop", `avg ${s.avgLoopTimeUs} µs · max ${s.maxLoopTimeUs.toLocaleString()} µs`],
      ["Config", `${status.hasMainConfig ? "stored" : "none (factory defaults)"}${status.hasSafeModeConfig ? " · safe-mode config stored" : ""}`],
      [
        "Outputs",
        status.testMode == null
          ? "?"
          : [status.testMode ? "TEST MODE" : "normal", status.nightMode ? "night mode" : ""].filter(Boolean).join(" · "),
      ],
      ["USB", ifaces.length ? ifaces.join(", ") : "none"],
    );
  }

  return (
    <section className={`card dash-card clickable${worst ? ` sev-${worst}-card` : ""}`} onClick={onOpen}>
      <div className="dash-title">
        <button className="link-btn" onClick={stop(onOpen)} title="Show this device's Overview">
          #{ids.unit_num} {ids.unit_name}
        </button>
        <span className={`pill ${error ? "bad" : "ok"}`} title={error ?? undefined}>
          {error ? "Not responding" : status ? "Connected" : "Connecting…"}
        </span>
      </div>
      {tags.length > 0 && (
        <div className="dash-tags">
          {tags.map((t) => (
            <span key={t} className={`badge ${worst === "error" && t === TAGS["duplicate-unit"] ? "bad" : ""}`}>
              {t}
            </span>
          ))}
        </div>
      )}
      <table className="kv">
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <th>{k}</th>
              <td className={k === "Outputs" && status?.testMode ? "warn-text" : undefined}>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {!status && !error && <p className="muted small">Loading…</p>}
      <div className="dash-actions" onClick={(e) => e.stopPropagation()}>
        <button onClick={onOpen}>Overview</button>
        <button onClick={onReboot}>Reboot…</button>
      </div>
    </section>
  );
}

/** Click handler that doesn't also trigger the card's own click. */
const stop = (f: () => void) => (e: { stopPropagation: () => void }) => {
  e.stopPropagation();
  f();
};

/** LedWiz unit numbers from the bit mask (bit 0 = unit 1). */
function ledWizUnits(mask: number): string {
  const units: number[] = [];
  for (let b = 0; b < 16; b++) if (mask & (1 << b)) units.push(b + 1);
  return units.length ? `unit ${units.join(", ")}` : "";
}

/** The dashboard's pseudo-tab name (App's view state; not shown in the tab bar). */
export const DASHBOARD = "Dashboard" as const;

/** Sidebar entry that opens the dashboard; shown once any Pico is attached. */
export function DashboardEntry({ count, active, onClick }: { count: number; active: boolean; onClick: () => void }) {
  if (count === 0) return null;
  return (
    <li
      className={"dash-entry" + (active ? " active" : "")}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      role="button"
      tabIndex={0}
      aria-current={active ? "true" : undefined}
      title="Status of every connected device, and problems across them"
    >
      <strong className="spacer">All devices</strong>
      <span className="muted small">{count}</span>
    </li>
  );
}
