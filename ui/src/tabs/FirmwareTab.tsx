import { DeviceSnapshot } from "../api";
import Installer from "../firmware/Installer";

interface Props {
  dev: DeviceSnapshot;
  visible: boolean;
  onStatus: (s: string) => void;
}

/** Firmware update for a running Pinscape device (docs/PLAN.md §4.4). The
 *  install itself, and its progress dialog, outlive this tab while the
 *  device reboots (see ../firmware/store.ts). */
export default function FirmwareTab({ dev, visible, onStatus }: Props) {
  if (!visible) return null;
  return <Installer target={{ kind: "device", dev }} active={visible} onStatus={onStatus} />;
}
