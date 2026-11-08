import { useState } from "react";
import { externalClick, hookIframe, UPSTREAM_URL } from "../shell/links";
import ShortcutsList from "../shell/ShortcutsList";

/** Built-in page (not an upstream file). */
const SHORTCUTS = "#shortcuts";

// Upstream help pages bundled under /help (assets/public/help).
const TOPICS: { page: string; title: string }[] = [
  { page: "ConfigFileFormat.htm", title: "Config file format (JSON basics)" },
  { page: "JSONConfigRef.htm", title: "Configuration reference" },
  { page: "PicoPinout.htm", title: "Pico pinout" },
  { page: "PlungerCal.htm", title: "Plunger calibration" },
  { page: "NudgeWin.htm", title: "Nudge / accelerometer" },
  { page: "IRRemote.htm", title: "IR remote & TV ON" },
  { page: "ButtonTester.htm", title: "Button tester" },
  { page: "OutputTester.htm", title: "Output tester" },
  { page: "LogViewer.htm", title: "Log viewer" },
  { page: "BootLoaderMode.htm", title: "Boot loader mode" },
  { page: "NewDeviceSetup.htm", title: "Setting up a new Pico" },
  { page: "ConfigTool.htm", title: "Windows Config Tool overview" },
  { page: "ConfigEditor.htm", title: "Windows config editor" },
  { page: "DeviceOverview.htm", title: "Windows device overview" },
  { page: SHORTCUTS, title: "Keyboard shortcuts (this app)" },
];

export default function HelpTab({ visible }: { visible: boolean }) {
  const [page, setPage] = useState(TOPICS[0].page);
  return (
    <div className="help-tab" style={{ display: visible ? "grid" : "none" }}>
      <nav aria-label="Help topics">
        <ul>
          {TOPICS.map((t) => (
            <li key={t.page}>
              <button
                className={t.page === page ? "active" : ""}
                aria-current={t.page === page ? "page" : undefined}
                onClick={() => setPage(t.page)}
              >
                {t.title}
              </button>
            </li>
          ))}
        </ul>
        <p className="muted small">
          These pages come from the{" "}
          <a href={UPSTREAM_URL} onClick={externalClick}>
            Pinscape Pico project
          </a>{" "}
          and describe the Windows Config Tool, so some buttons and menus they mention are laid out differently here. The device
          concepts and settings are the same. Links to other web sites open in your browser.
        </p>
      </nav>
      {page === SHORTCUTS ? (
        <div className="help-page">
          <ShortcutsList />
        </div>
      ) : (
        <iframe title="Help" src={`/help/${page}`} onLoad={hookIframe} />
      )}
    </div>
  );
}
