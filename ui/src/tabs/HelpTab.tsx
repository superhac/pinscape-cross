import { useState } from "react";

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
];

export default function HelpTab({ visible }: { visible: boolean }) {
  const [page, setPage] = useState(TOPICS[0].page);
  return (
    <div className="help-tab" style={{ display: visible ? "grid" : "none" }}>
      <nav>
        <ul>
          {TOPICS.map((t) => (
            <li key={t.page}>
              <button className={t.page === page ? "active" : ""} onClick={() => setPage(t.page)}>
                {t.title}
              </button>
            </li>
          ))}
        </ul>
        <p className="muted small">
          These pages come from the Pinscape Pico project and describe the Windows Config Tool, so some buttons and menus they
          mention are laid out differently here. The device concepts and settings are the same.
        </p>
      </nav>
      <iframe title="Help" src={`/help/${page}`} />
    </div>
  );
}
