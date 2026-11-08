import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, DeviceSnapshot, fmtVersion } from "./api";
import { useDevices } from "./lib/useDevices";
import { confirm, ConfirmHost } from "./lib/confirm";
import { dirtyEditors } from "./lib/dirty";
import OverviewTab from "./tabs/OverviewTab";
import ConfigTab from "./tabs/ConfigTab";
import LogTab from "./tabs/LogTab";
import DeviceProblem from "./lib/DeviceProblem";
import HelpTab from "./tabs/HelpTab";
import ButtonsTab from "./tabs/ButtonsTab";
import OutputsTab from "./tabs/OutputsTab";
import IrTab from "./tabs/IrTab";
import NudgeTab from "./tabs/NudgeTab";
import PlungerTab from "./tabs/PlungerTab";
import FirmwareTab from "./tabs/FirmwareTab";
import BootLoaderList from "./firmware/BootLoaderList";

// Tabs planned per docs/PLAN.md §3.
const TABS = ["Overview", "Config", "Safe-Mode Config", "Log", "Buttons", "Outputs", "Nudge", "Plunger", "IR", "Firmware", "Help"] as const;
type Tab = (typeof TABS)[number];
const READY: Tab[] = ["Overview", "Config", "Safe-Mode Config", "Log", "Buttons", "Outputs", "Nudge", "Plunger", "IR", "Firmware", "Help"];

export default function App() {
  const devices = useDevices();
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("Overview");
  const [status, setStatus] = useState("");
  // Config tabs are mounted on first visit and then kept (hidden), so unsaved
  // edits survive switching tabs.
  const [visited, setVisited] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (selected && (tab === "Config" || tab === "Safe-Mode Config")) {
      const k = `${selected}:${tab}`;
      setVisited((v) => (v.has(k) ? v : new Set(v).add(k)));
    }
  }, [selected, tab]);

  // Warn before closing the window with unsaved config edits.
  useEffect(() => {
    const un = getCurrentWindow().onCloseRequested(async (e) => {
      const d = dirtyEditors();
      if (!d.length) return;
      e.preventDefault();
      const ok = await confirm({
        title: "Discard unsaved changes?",
        body: `You have unsaved config edits (${d.join(", ")}). Close anyway and discard them?`,
        ok: "Discard and close",
        danger: true,
      });
      if (ok) getCurrentWindow().destroy();
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    api.devOptions().then((o) => {
      const t = TABS.find((x) => x.toLowerCase() === o.startTab?.toLowerCase());
      if (t) setTab(t);
      if (o.startDevice) setSelected(o.startDevice);
    });
  }, []);

  // Keep a selection: prefer the current one, else the first usable device.
  useEffect(() => {
    if (!devices.some((d) => d.key === selected)) {
      const first = devices.find((d) => d.ids);
      if (first) setSelected(first.key);
    }
  }, [devices, selected]);

  const dev = devices.find((d) => d.key === selected && d.ids) ?? null;
  const broken = devices.find((d) => d.key === selected && !d.ids) ?? null;

  return (
    <div className="app">
      <aside className="sidebar">
        <header>
          <h1>Pinscape-Cross</h1>
        </header>
        {devices.length === 0 && <p className="muted small">No devices. Plug in a Pinscape Pico; it appears here automatically.</p>}
        <ul>
          {devices.map((d) => (
            <DeviceItem key={d.key} d={d} active={d.key === selected} onClick={() => setSelected(d.key)} />
          ))}
        </ul>
        {selected && !dev && devices.length > 0 && (
          <p className="muted small">Selected device disconnected — waiting for it to come back…</p>
        )}
        <BootLoaderList onStatus={setStatus} />
      </aside>

      <main>
        <nav className="tabs">
          {TABS.map((t) => (
            <button
              key={t}
              className={t === tab ? "active" : READY.includes(t) ? "" : "pending"}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </nav>
        <section className="content">
          <HelpTab visible={tab === "Help"} />
          {broken ? (
            tab !== "Help" && <DeviceProblem dev={broken} />
          ) : !dev ? (
            tab !== "Help" && <div className="empty">
              <p className="muted">{selected ? "Device disconnected. It will reconnect automatically." : "Select a device."}</p>
            </div>
          ) : (
            <>
              {tab === "Overview" && <OverviewTab dev={dev} onStatus={setStatus} />}
              {visited.has(`${dev.key}:Config`) && (
                <ConfigTab key={dev.key + ":main"} dev={dev} safeMode={false} visible={tab === "Config"} onStatus={setStatus} />
              )}
              {visited.has(`${dev.key}:Safe-Mode Config`) && (
                <ConfigTab key={dev.key + ":safe"} dev={dev} safeMode visible={tab === "Safe-Mode Config"} onStatus={setStatus} />
              )}
              {/* The log stays mounted so it keeps collecting while other tabs are shown. */}
              <LogTab key={dev.key + ":log"} dev={dev} visible={tab === "Log"} />
              <ButtonsTab key={dev.key + ":buttons"} dev={dev} visible={tab === "Buttons"} onStatus={setStatus} />
              <OutputsTab key={dev.key + ":outputs"} dev={dev} visible={tab === "Outputs"} onStatus={setStatus} />
              <IrTab key={dev.key + ":ir"} dev={dev} visible={tab === "IR"} onStatus={setStatus} />
              <NudgeTab key={dev.key + ":nudge"} dev={dev} visible={tab === "Nudge"} onStatus={setStatus} />
              <PlungerTab key={dev.key + ":plunger"} dev={dev} visible={tab === "Plunger"} onStatus={setStatus} />
              <FirmwareTab key={dev.key + ":firmware"} dev={dev} visible={tab === "Firmware"} onStatus={setStatus} />
              {!READY.includes(tab) && (
                <div className="empty">
                  <p className="muted">{tab}: coming in a later phase (see docs/PLAN.md §8).</p>
                </div>
              )}
            </>
          )}
        </section>
        <footer className="statusbar">
          <span>{status}</span>
          {dev?.ids && (
            <span className="muted">
              #{dev.ids.unit_num} {dev.ids.unit_name} · {dev.ids.hwid}
            </span>
          )}
        </footer>
      </main>
      <ConfirmHost />
    </div>
  );
}

function DeviceItem({ d, active, onClick }: { d: DeviceSnapshot; active: boolean; onClick: () => void }) {
  return (
    <li className={(active ? "active " : "") + (d.ids ? "" : "broken")} onClick={onClick} title={d.error ?? undefined}>
      {d.ids ? (
        <>
          <strong>
            #{d.ids.unit_num} {d.ids.unit_name}
          </strong>
          <span className="muted small">
            {d.ids.hwid} · v{d.version ? fmtVersion(d.version) : "?"}
          </span>
        </>
      ) : (
        <>
          <strong>Pinscape Pico</strong>
          <span className="error small">
            {d.errorKind === "permission"
              ? "No permission to open"
              : d.errorKind === "busy"
                ? "In use by another program"
                : "Can't open device"}
          </span>
        </>
      )}
    </li>
  );
}
