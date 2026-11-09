import { KeyboardEvent as ReactKeyboardEvent, useEffect, useRef, useState } from "react";
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
import DofTab from "./tabs/DofTab";
import NudgeTab from "./tabs/NudgeTab";
import PlungerTab from "./tabs/PlungerTab";
import FirmwareTab from "./tabs/FirmwareTab";
import PinoutTab from "./tabs/PinoutTab";
import BootLoaderList from "./firmware/BootLoaderList";
import Dashboard, { DASHBOARD, DashboardEntry } from "./dashboard/Dashboard";
import UpdateNotice from "./updater/UpdateNotice";
import { firmwareApi } from "./lib/api-firmware";
import { getPrefs, loadPrefs, onPrefsSaveError, subscribePrefs, updatePrefs } from "./prefs/store";
import { applyAppearance } from "./prefs/appearance";
import PreferencesDialog from "./prefs/PreferencesDialog";
import AboutDialog from "./shell/AboutDialog";
import { onLinkError } from "./shell/links";
import { useGlobalShortcuts } from "./shell/shortcuts";
import { modName, useIsMac } from "./shell/platform";
import { GearIcon, InfoIcon } from "./shell/icons";

// Tabs planned per docs/PLAN.md §3.
const TABS = ["Overview", "Config", "Safe-Mode Config", "Log", "Buttons", "Outputs", "Nudge", "Plunger", "IR", "Pinout", "DOF", "Firmware", "Help"] as const;
type Tab = (typeof TABS)[number];
const READY: Tab[] = ["Overview", "Config", "Safe-Mode Config", "Log", "Buttons", "Outputs", "Nudge", "Plunger", "IR", "Pinout", "DOF", "Firmware", "Help"];

export default function App() {
  const devices = useDevices();
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab | typeof DASHBOARD>("Overview");
  const [status, setStatus] = useState("");
  // Config tabs are mounted on first visit and then kept (hidden), so unsaved
  // edits survive switching tabs.
  const [visited, setVisited] = useState<Set<string>>(new Set());
  const [dialog, setDialog] = useState<"prefs" | "about" | null>(null);
  // Startup choices (env options, remembered tab/device) have been applied.
  const [started, setStarted] = useState(false);
  // Picos in boot loader mode (listed by BootLoaderList), so "No devices" isn't shown beside them.
  const [bootCount, setBootCount] = useState(0);
  const mac = useIsMac();
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // Appearance preferences → <html data-theme / data-font-size>.
  useEffect(() => {
    applyAppearance(getPrefs());
    const un = subscribePrefs(applyAppearance);
    onLinkError(setStatus);
    onPrefsSaveError((e) => setStatus(`Couldn't save preferences: ${e}`));
    return () => {
      un();
      onLinkError(null);
      onPrefsSaveError(null);
    };
  }, []);

  useEffect(() => {
    let alive = true;
    const un = firmwareApi.onBootLoadersChanged((d) => alive && setBootCount(d.length));
    firmwareApi
      .bootList()
      .then((l) => alive && setBootCount(l.devices.length))
      .catch(() => {});
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);

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

  // Startup tab/device: environment options (dev_options) win over the
  // remembered ones (Preferences › Startup).
  useEffect(() => {
    Promise.all([
      api.devOptions().catch(() => ({ startTab: null, startDevice: null })),
      loadPrefs(),
    ]).then(([o, p]) => {
      const wantTab = o.startTab ?? (p.startup.rememberTab ? p.startup.lastTab : null);
      const t = [...TABS, DASHBOARD].find((x) => x.toLowerCase() === wantTab?.toLowerCase());
      if (t) setTab(t);
      const wantDev = o.startDevice ?? (p.startup.rememberDevice ? p.startup.lastDevice : null);
      if (wantDev) setSelected(wantDev);
      setStarted(true);
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

  // Remember the tab and (connected) device for next time.
  useEffect(() => {
    const s = getPrefs().startup;
    if (started && s.rememberTab && s.lastTab !== tab) updatePrefs({ startup: { lastTab: tab } });
  }, [started, tab]);
  const devKey = dev?.key ?? null;
  useEffect(() => {
    const s = getPrefs().startup;
    if (started && devKey && s.rememberDevice && s.lastDevice !== devKey) updatePrefs({ startup: { lastDevice: devKey } });
  }, [started, devKey]);

  const tabIdx = TABS.indexOf(tab as Tab);
  // The dashboard replaces the tab bar and panel; the panel stays mounted (unsaved edits) but no tab is "visible".
  const hideTabs = tab === DASHBOARD ? { display: "none" } : undefined;
  const goTab = (i: number, focus = false) => {
    const n = TABS.length;
    const j = ((i % n) + n) % n;
    setTab(TABS[j]);
    if (focus) tabRefs.current[j]?.focus();
  };
  const onTabKey = (e: ReactKeyboardEvent) => {
    const k = e.key;
    if (k === "ArrowRight" || k === "ArrowLeft" || k === "Home" || k === "End") {
      e.preventDefault();
      goTab(k === "Home" ? 0 : k === "End" ? TABS.length - 1 : tabIdx + (k === "ArrowRight" ? 1 : -1), true);
    }
  };

  const reboot = async () => {
    if (!dev?.ids) {
      setStatus("Reboot: no device selected.");
      return;
    }
    const unsaved = dirtyEditors();
    const ok = await confirm({
      title: "Reboot device?",
      body:
        `Reboot #${dev.ids.unit_num} ${dev.ids.unit_name}? It disconnects briefly and reconnects automatically.` +
        (unsaved.length ? ` Unsaved edits (${unsaved.join(", ")}) stay in the editor.` : ""),
      ok: "Reboot",
    });
    if (!ok) return;
    try {
      await api.reset(dev.key, "normal");
      setStatus("Device rebooting…");
    } catch (e) {
      setStatus(`Reboot failed: ${e}`);
    }
  };

  useGlobalShortcuts(mac, {
    tabIndex: (i) => i < TABS.length && setTab(TABS[i]),
    tabStep: (d) => goTab(tabIdx + d),
    preferences: () => setDialog("prefs"),
    help: () => setTab("Help"),
    reboot,
  });
  const m = modName(mac);

  return (
    <div className="app">
      <aside className="sidebar">
        <header>
          <h1>Pinscape-Cross</h1>
          <button className="icon-btn" onClick={() => setDialog("about")} aria-label="About Pinscape-Cross" title="About">
            <InfoIcon />
          </button>
          <button className="icon-btn" onClick={() => setDialog("prefs")} aria-label="Preferences" title={`Preferences (${m}+,)`}>
            <GearIcon />
          </button>
        </header>
        {devices.length === 0 && bootCount === 0 && (
          <p className="muted small" role="status">
            No devices. Plug in a Pinscape Pico; it appears here automatically.
          </p>
        )}
        <ul aria-label="Devices">
          <DashboardEntry count={devices.length + bootCount} active={tab === DASHBOARD} onClick={() => setTab(DASHBOARD)} />
          {devices.map((d) => (
            <DeviceItem
              key={d.key}
              d={d}
              active={tab !== DASHBOARD && d.key === selected}
              onClick={() => {
                setSelected(d.key);
                if (tab === DASHBOARD) setTab("Overview");
              }}
            />
          ))}
        </ul>
        {selected && !dev && devices.length > 0 && (
          <p className="muted small">Selected device disconnected — waiting for it to come back…</p>
        )}
        <BootLoaderList onStatus={setStatus} />
        <UpdateNotice />
      </aside>

      <main>
        {tab === DASHBOARD && (
          <section className="content">
            <Dashboard
              devices={devices}
              onOpen={(k) => {
                setSelected(k);
                setTab("Overview");
              }}
              onStatus={setStatus}
            />
          </section>
        )}
        <nav className="tabs" role="tablist" aria-label="Device views" onKeyDown={onTabKey} style={hideTabs}>
          {TABS.map((t, i) => (
            <button
              key={t}
              ref={(el) => {
                tabRefs.current[i] = el;
              }}
              id={`tab-${i}`}
              role="tab"
              aria-selected={t === tab}
              aria-controls="tabpanel"
              tabIndex={t === tab ? 0 : -1}
              title={i < 9 ? `${m}+${i + 1}` : undefined}
              className={t === tab ? "active" : READY.includes(t) ? "" : "pending"}
              onClick={() => setTab(t)}
            >
              {t}
            </button>
          ))}
        </nav>
        <section className="content" role="tabpanel" id="tabpanel" aria-labelledby={`tab-${tabIdx}`} style={hideTabs}>
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
              {tab === "Pinout" && <PinoutTab key={dev.key + ":pinout"} dev={dev} onStatus={setStatus} />}
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
              <DofTab key={dev.key + ":dof"} dev={dev} visible={tab === "DOF"} onStatus={setStatus} />
              <NudgeTab key={dev.key + ":nudge"} dev={dev} visible={tab === "Nudge"} onStatus={setStatus} />
              <PlungerTab key={dev.key + ":plunger"} dev={dev} visible={tab === "Plunger"} onStatus={setStatus} />
              <FirmwareTab key={dev.key + ":firmware"} dev={dev} visible={tab === "Firmware"} onStatus={setStatus} />
              {!READY.includes(tab as Tab) && tab !== DASHBOARD && (
                <div className="empty">
                  <p className="muted">{tab}: coming in a later phase (see docs/PLAN.md §8).</p>
                </div>
              )}
            </>
          )}
        </section>
        <footer className="statusbar">
          <span role="status" aria-live="polite">
            {status}
          </span>
          {dev?.ids && (
            <span className="muted">
              #{dev.ids.unit_num} {dev.ids.unit_name} · {dev.ids.hwid}
            </span>
          )}
        </footer>
      </main>
      {dialog === "prefs" && <PreferencesDialog onClose={() => setDialog(null)} />}
      {dialog === "about" && <AboutDialog onClose={() => setDialog(null)} />}
      <ConfirmHost />
    </div>
  );
}

function DeviceItem({ d, active, onClick }: { d: DeviceSnapshot; active: boolean; onClick: () => void }) {
  return (
    <li
      className={(active ? "active " : "") + (d.ids ? "" : "broken")}
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
      title={d.error ?? undefined}
    >
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
