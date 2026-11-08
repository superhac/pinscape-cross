import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "../api";
import Modal from "./Modal";
import { externalClick, REPO_URL, UPSTREAM_URL } from "./links";
import { checkForUpdates } from "../updater/updater";

const LICENSE_URL = "https://opensource.org/license/bsd-3-clause";

export default function AboutDialog({ onClose }: { onClose: () => void }) {
  const [version, setVersion] = useState<string>("…");
  const [rev, setRev] = useState<string>("");
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion("unknown"));
    api
      .aboutInfo()
      .then((i) => setRev(i.upstreamRevision))
      .catch(() => {});
  }, []);
  const [commit, date] = rev.split(/\s+/);
  const [updateMsg, setUpdateMsg] = useState("");
  const [checking, setChecking] = useState(false);
  const check = async () => {
    setChecking(true);
    setUpdateMsg("Checking…");
    try {
      const u = await checkForUpdates();
      // A found update also appears as the notice in the sidebar.
      setUpdateMsg(u ? `Version ${u.version} is available — see the notice in the sidebar.` : "You have the latest version.");
    } catch (e) {
      setUpdateMsg(`Couldn't check for updates: ${e}`);
    } finally {
      setChecking(false);
    }
  };

  return (
    <Modal title="About Pinscape-Cross" onClose={onClose} className="wide about">
      <p>
        <strong>Pinscape-Cross</strong> {version}
        <br />
        Cross-platform configuration tool for Pinscape Pico virtual-pinball I/O controllers.
      </p>
      <p className="muted">
        Unofficial project; not affiliated with or endorsed by the upstream Pinscape Pico project.
      </p>
      <table className="kv">
        <tbody>
          <tr>
            <th scope="row">Project</th>
            <td>
              <a href={REPO_URL} onClick={externalClick}>
                {REPO_URL}
              </a>
            </td>
          </tr>
          <tr>
            <th scope="row">License</th>
            <td>
              <a href={LICENSE_URL} onClick={externalClick}>
                BSD-3-Clause
              </a>
            </td>
          </tr>
          <tr>
            <th scope="row">Based on</th>
            <td>
              <a href={UPSTREAM_URL} onClick={externalClick}>
                Pinscape Pico
              </a>{" "}
              by Michael J Roberts (BSD-3-Clause). Help pages, config templates and the config schema come from that project.
            </td>
          </tr>
          <tr>
            <th scope="row">Upstream assets</th>
            <td>
              {commit ? (
                <a href={`${UPSTREAM_URL}/commit/${commit}`} onClick={externalClick}>
                  {commit}
                </a>
              ) : (
                "unknown"
              )}
              {date && ` (${date})`}
            </td>
          </tr>
        </tbody>
      </table>
      <p className="about-update">
        <button onClick={check} disabled={checking}>
          Check for updates
        </button>{" "}
        <span className="muted small" role="status">
          {updateMsg}
        </span>
      </p>
    </Modal>
  );
}
