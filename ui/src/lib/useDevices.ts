import { useEffect, useRef, useState } from "react";
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

/** Run `fn` now and every `ms` while `enabled`. Always calls the latest `fn`
 * (so props/state used inside it, e.g. the selected device, are never stale). */
export function useInterval(fn: () => void, ms: number, enabled = true) {
  const latest = useRef(fn);
  latest.current = fn;
  useEffect(() => {
    if (!enabled) return;
    latest.current();
    const t = setInterval(() => latest.current(), ms);
    return () => clearInterval(t);
  }, [ms, enabled]);
}
