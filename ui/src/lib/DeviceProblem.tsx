import { useEffect, useState } from "react";
import { api, DeviceSnapshot, hex4 } from "../api";

const RULES = [
  'SUBSYSTEM=="usb", ATTR{idVendor}=="1209", ATTR{idProduct}=="eaea", MODE="0666", TAG+="uaccess"',
  'SUBSYSTEM=="usb", ATTR{idVendor}=="1209", ATTR{idProduct}=="eaeb", MODE="0666", TAG+="uaccess"',
  'SUBSYSTEM=="usb", ATTR{idVendor}=="2e8a", ATTR{idProduct}=="0003", MODE="0666", TAG+="uaccess"',
  'SUBSYSTEM=="usb", ATTR{idVendor}=="2e8a", ATTR{idProduct}=="000f", MODE="0666", TAG+="uaccess"',
];

const UDEV_CMD =
  `printf '%s\\n' \\\n` +
  RULES.map((r) => `  '${r}'`).join(" \\\n") +
  ` \\\n  | sudo tee /etc/udev/rules.d/99-pinscape.rules >/dev/null\n` +
  `sudo udevadm control --reload && sudo udevadm trigger`;

/** Explains why a detected device can't be opened, with OS-specific fixes. */
export default function DeviceProblem({ dev }: { dev: DeviceSnapshot }) {
  const [os, setOs] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    api.platform().then(setOs);
  }, []);
  const usbId = `${hex4(dev.vid)}:${hex4(dev.pid)}`;
  const nonDefaultIds = !(dev.vid === 0x1209 && (dev.pid === 0xeaea || dev.pid === 0xeaeb));

  return (
    <div className="problem card">
      {dev.errorKind === "permission" ? (
        <>
          <h2>No permission to open this Pinscape Pico</h2>
          {os === "linux" ? (
            <>
              <p>
                Linux only lets root open USB devices until a <em>udev rule</em> grants access. Run this once in a terminal, then
                unplug the Pico and plug it back in:
              </p>
              <pre className="cmd">{UDEV_CMD}</pre>
              <button onClick={() => navigator.clipboard?.writeText(UDEV_CMD).then(() => setCopied(true))}>
                {copied ? "Copied" : "Copy command"}
              </button>
              {nonDefaultIds && (
                <p className="muted">
                  This device uses USB ID {usbId}, not the Pinscape default. Change the vendor/product numbers in the rule to{" "}
                  <code>{hex4(dev.vid)}</code> / <code>{hex4(dev.pid)}</code>.
                </p>
              )}
              <p className="muted small">The .deb and .rpm packages install this rule automatically.</p>
            </>
          ) : (
            <p>The operating system refused access to the device. Try unplugging and reconnecting it.</p>
          )}
        </>
      ) : dev.errorKind === "busy" ? (
        <>
          <h2>This Pinscape Pico is in use by another program</h2>
          <p>
            Only one program at a time can use a Pinscape Pico's configuration connection. Close the other program, such as the
            Pinscape command-line tool, the upstream Pinscape Config Tool, or another copy of this app, and the device appears here
            automatically.
          </p>
          <p className="muted small">Games and DOF use other connections and don't block this one.</p>
        </>
      ) : (
        <>
          <h2>Can't open this Pinscape Pico</h2>
          <p>Try unplugging the device and plugging it back in.</p>
        </>
      )}
      <p className="muted small">
        USB ID {usbId} · Error: {dev.error}
      </p>
    </div>
  );
}
