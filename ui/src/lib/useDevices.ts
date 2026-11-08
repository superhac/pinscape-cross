import { useEffect, useState } from "react";
import { api, DeviceSnapshot } from "../api";

/** Live device list: initial fetch plus `devices-changed` hot-plug events. */
export function useDevices(): DeviceSnapshot[] {
  const [devices, setDevices] = useState<DeviceSnapshot[]>([]);
  useEffect(() => {
    let alive = true;
    const un = api.onDevicesChanged((d) => alive && setDevices(d));
    api.listDevices().then((d) => alive && setDevices(d));
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);
  return devices;
}

/** Run `fn` now and every `ms` while `enabled`. */
export function useInterval(fn: () => void, ms: number, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    fn();
    const t = setInterval(fn, ms);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms, enabled]);
}
