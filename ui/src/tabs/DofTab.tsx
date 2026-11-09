import { useCallback, useEffect, useMemo, useState } from "react";
import { api, DeviceSnapshot } from "../api";
import { dofApi, DofPort, DofReport, DofSeverity, KIND_LABEL } from "../lib/api-dof";
import { saveTextAs, stampedName } from "../lib/api-export";
import "./DofTab.css";

interface Props {
  dev: DeviceSnapshot;
  /** The tab stays mounted while hidden; query the device only while visible. */
  visible: boolean;
  onStatus: (s: string) => void;
}

const TEXT_FILTER = [{ name: "Text", extensions: ["txt"] }];
const SEV_RANK: Record<DofSeverity, number> = { error: 0, warning: 1, info: 2 };
const SEV_LABEL: Record<DofSeverity, string> = { error: "Error", warning: "Warning", info: "Note" };

// DOF config helper: the logical output ports as DOF numbers them, checks for
// common setup problems, and exports for the DOF Config Tool. The mapping,
// checks and export text are built in Rust (src-tauri/src/dof.rs, unit-tested).
export default function DofTab({ dev, visible, onStatus }: Props) {
  const [report, setReport] = useState<DofReport | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    dofApi
      .report(dev.key)
      .then((r) => {
        setReport(r);
        setErr(null);
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setLoading(false));
  }, [dev.key]);

  useEffect(() => {
    if (visible) load();
  }, [visible, load]);

  // Another Pico plugged in or out changes the unit-number checks.
  useEffect(() => {
    if (!visible) return;
    let alive = true;
    const un = api.onDevicesChanged(() => alive && load());
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, [visible, load]);

  // Worst problem per port, for row highlighting.
  const portSeverity = useMemo(() => {
    const m = new Map<number, DofSeverity>();
    for (const p of report?.problems ?? []) {
      for (const n of p.ports) {
        const cur = m.get(n);
        if (!cur || SEV_RANK[p.severity] < SEV_RANK[cur]) m.set(n, p.severity);
      }
    }
    return m;
  }, [report]);

  if (!visible) return null;

  const copy = (text: string, what: string) =>
    navigator.clipboard?.writeText(text).then(
      () => onStatus(`Copied ${what} to the clipboard`),
      () => onStatus("Copy failed"),
    );

  const save = (text: string, kind: "csv" | "txt") => {
    if (!report) return;
    const base = `${report.unit.unitName || "pinscape"}-unit${report.unit.unitNum}-dof-ports`;
    saveTextAs(text, {
      title: kind === "csv" ? "Save DOF port mapping (CSV)" : "Save DOF port list",
      defaultName: stampedName(base, kind),
      ...(kind === "txt" ? { filters: TEXT_FILTER } : {}),
    })
      .then((p) => p && onStatus(`Saved DOF port mapping to ${p}`))
      .catch((e) => onStatus(`Save failed: ${e}`));
  };

  const counts = { error: 0, warning: 0, info: 0 };
  report?.problems.forEach((p) => counts[p.severity]++);

  return (
    <div className="doftab">
      <div className="toolbar">
        <button onClick={load} disabled={loading}>
          {loading ? "Reading…" : "Refresh"}
        </button>
        <span className="spacer" />
        <button disabled={!report} onClick={() => report && copy(report.portList, "the DOF port list")} title="Copy the port list as text">
          Copy port list
        </button>
        <button disabled={!report} onClick={() => report && copy(report.csv, "the DOF port table (CSV)")} title="Copy the table as CSV">
          Copy CSV
        </button>
        <button disabled={!report} onClick={() => report && save(report.csv, "csv")}>
          Save CSV…
        </button>
        <button disabled={!report} onClick={() => report && save(report.portList, "txt")}>
          Save port list…
        </button>
      </div>
      {err && <p className="error">{err}</p>}

      {report && (
        <div className="cards">
          <section className="card">
            <h3>How DOF sees this Pico</h3>
            <dl className="dof-ident">
              <dt>DOF unit number</dt>
              <dd>
                <strong className="dof-unit">{report.unit.unitNum}</strong> <span className="muted">(id.unitNum)</span>
              </dd>
              <dt>Unit name</dt>
              <dd>{report.unit.unitName || <span className="muted">(none)</span>}</dd>
              <dt>Hardware ID</dt>
              <dd className="mono">{report.unit.hwid}</dd>
              <dt>Feedback Controller</dt>
              <dd>
                {report.feedbackEnabled == null ? (
                  <span className="muted">unknown</span>
                ) : report.feedbackEnabled ? (
                  <span className="ok-text">enabled (DOF can reach it)</span>
                ) : (
                  <span className="error">disabled (DOF can't see this Pico)</span>
                )}
              </dd>
              <dt>DOF ports</dt>
              <dd>
                {report.ports.length} (#1–#{Math.max(1, report.ports.length)})
              </dd>
              <dt>Other Picos</dt>
              <dd>
                {report.otherUnits.length === 0 ? (
                  <span className="muted">none connected</span>
                ) : (
                  report.otherUnits.map((u) => (
                    <div key={u.hwid} className={u.unitNum === report.unit.unitNum ? "error" : ""}>
                      unit {u.unitNum} "{u.unitName}" <span className="muted mono">{u.hwid}</span>
                    </div>
                  ))
                )}
              </dd>
            </dl>
          </section>

          <section className="card">
            <h3>Setting up the DOF Config Tool</h3>
            <ol className="dof-guide">
              <li>
                DOF recognizes Pinscape Picos natively (unified DOF R3++ releases since late 2024). It finds the Pico over
                USB by itself; you only tell it the <strong>unit number</strong>.
              </li>
              <li>
                Number your Picos 1, 2, 3… with <code>id.unitNum</code> in the config (default 1). KL25Z Pinscape units are
                numbered separately and don't count.
              </li>
              <li>
                In the DOF Config Tool (configtool.vpuniverse.com) open <strong>Cabinet › Devices › Port Assignments</strong>,
                pick the Pinscape Pico with unit number <strong>{report.unit.unitNum}</strong>, and assign each port number
                in the table below to its DOF device (flipper, shaker, flasher…).
              </li>
              <li>
                Port numbers are the order of the entries in the config's <code>outputs</code> array: the first entry is
                port #1. Inserting or removing an entry renumbers every port after it, so update DOF too.
              </li>
              <li>
                Share groups: assign the share-group ports in DOF, not the physical pool ports behind them.
              </li>
            </ol>
          </section>

          <section className="card wide">
            <h3>
              Checks
              <span className="spacer" />
              <span className="muted small dof-counts">
                {counts.error} errors · {counts.warning} warnings · {counts.info} notes
              </span>
            </h3>
            {report.problems.length === 0 ? (
              <p className="ok-text">No problems found.</p>
            ) : (
              <ul className="dof-problems">
                {report.problems.map((p, i) => (
                  <li key={i} className={`sev-${p.severity}`}>
                    <span className={`dof-sev sev-${p.severity}`}>{SEV_LABEL[p.severity]}</span>
                    <span>{p.message}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="card wide">
            <h3>Port mapping</h3>
            {report.ports.length === 0 ? (
              <p className="muted">No output ports are configured.</p>
            ) : (
              <div className="dof-table">
                <table className="grid">
                  <thead>
                    <tr>
                      <th title="The number to use in the DOF Config Tool">DOF port</th>
                      <th>Name</th>
                      <th>Type</th>
                      <th>Device</th>
                      <th>Pin</th>
                      <th>Flags</th>
                    </tr>
                  </thead>
                  <tbody>
                    {report.ports.map((p) => (
                      <PortRow key={p.dofPort} p={p} sev={portSeverity.get(p.dofPort)} />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

function PortRow({ p, sev }: { p: DofPort; sev: DofSeverity | undefined }) {
  const flags: [boolean, string, string][] = [
    [p.noisy, "noisy", "Noisy: turned off in Night Mode"],
    [p.gamma, "gamma", "Gamma correction"],
    [p.inverted, "inverted", "Inverted (active-low) output"],
    [p.flipperLogic, "flipper logic", "Flipper Logic time limit"],
    [p.computed, "computed", "Computed: a source formula sets the level"],
    [!p.addressable, "not addressable", "DOF can only address ports 1–255"],
  ];
  return (
    <tr className={sev ? `row-${sev}` : ""}>
      <td>
        <strong>{p.dofPort}</strong>
      </td>
      <td className="sans">{p.name ?? <span className="muted">(unnamed)</span>}</td>
      <td className="sans">{KIND_LABEL[p.kind]}</td>
      <td>{p.device}</td>
      <td>{p.pin}</td>
      <td className="sans">
        <span className="dof-flags">
          {flags
            .filter(([on]) => on)
            .map(([, label, title]) => (
              <span key={label} className={`dof-flag${label === "not addressable" ? " bad" : ""}`} title={title}>
                {label}
              </span>
            ))}
        </span>
      </td>
    </tr>
  );
}
