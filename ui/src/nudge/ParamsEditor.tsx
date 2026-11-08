import { NudgeParams } from "../lib/api-nudge";

type NumKey = Exclude<keyof NudgeParams, "autoCenter" | "hasLpFreq" | "otherFlags">;

interface Field {
  key: NumKey;
  label: string;
  unit: string;
  min: number;
  help: string;
}

const AUTO_CENTER: Field[] = [
  {
    key: "autoCenterIntervalMs",
    label: "Auto-centering interval",
    unit: "ms",
    min: 1,
    help:
      "How long the accelerometer must stay still (within the quiet thresholds) before the trailing average becomes the new center point. 4000-5000 ms is recommended.",
  },
  {
    key: "xThreshold",
    label: "Quiet threshold X",
    unit: "units",
    min: 0,
    help:
      "Largest deviation from the rolling average that still counts as 'at rest', in raw axis units. Set by noise calibration. Lower it if auto-centering kicks in while you're nudging; raise it if auto-centering never refreshes when the cabinet is still.",
  },
  { key: "yThreshold", label: "Quiet threshold Y", unit: "units", min: 0, help: "Quiet threshold for the Y (front/back) axis." },
  { key: "zThreshold", label: "Quiet threshold Z", unit: "units", min: 0, help: "Quiet threshold for the Z (vertical) axis." },
];

const FILTERS: Field[] = [
  {
    key: "dcTimeMs",
    label: "DC blocker time constant",
    unit: "ms",
    min: 0,
    help:
      "Approximate time the DC blocker takes to cancel a change in the constant offset (fixed tilt). Shorter adapts faster but attenuates slow motion more. ~200 ms suits most setups. 0 disables the DC blocker. It does the same job as auto-centering; you normally need only one.",
  },
  {
    key: "lpFreqHz",
    label: "Low-pass cutoff frequency",
    unit: "Hz",
    min: 0,
    help:
      "Corner (-3 dB) frequency of a low-pass filter that reduces higher-frequency vibration from subwoofers, shakers and solenoids. Try lowering the simulator's nudge gain first. 0 disables the filter (the default).",
  },
  {
    key: "xJitterWindow",
    label: "Jitter window X",
    unit: "units",
    min: 0,
    help:
      "Hysteresis window: readings that stay inside the window report a constant value. Set it to about the axis's at-rest noise level (see noise calibration), the smallest value that keeps the filtered reading steady when the cabinet is still. 0 disables.",
  },
  { key: "yJitterWindow", label: "Jitter window Y", unit: "units", min: 0, help: "Jitter (hysteresis) window for the Y axis; 0 disables." },
  { key: "zJitterWindow", label: "Jitter window Z", unit: "units", min: 0, help: "Jitter (hysteresis) window for the Z axis; 0 disables." },
];

const VELOCITY: Field[] = [
  {
    key: "velocityDecayMs",
    label: "Velocity decay time",
    unit: "ms",
    min: 0,
    help:
      "Half-life applied to the integrated velocity, to cancel accumulated error from any bias in the readings. Shorter is smoother and more stable but makes the ball 'jump back' after a hard nudge. Default 2000 ms.",
  },
  {
    key: "velocityScalingFactor",
    label: "Velocity scaling factor",
    unit: "per mm/s",
    min: 0,
    help:
      "INT16 report units per mm/s. Choose it so your largest real nudges come close to full scale (32767); e.g. 32768 / 300 mm/s ≈ 109.",
  },
];

interface Props {
  draft: NudgeParams;
  device: NudgeParams;
  onChange: (p: NudgeParams) => void;
  disabled?: boolean;
}

/** Form for every NudgeParams field; edited values are highlighted until applied. */
export default function ParamsEditor({ draft, device, onChange, disabled }: Props) {
  const set = (k: NumKey, raw: string) => {
    const n = Math.round(Number(raw));
    if (!Number.isFinite(n)) return;
    onChange({ ...draft, [k]: Math.max(0, Math.min(65535, n)) });
  };

  const row = (f: Field) => {
    const na = f.key === "lpFreqHz" && !draft.hasLpFreq;
    const changed = draft[f.key] !== device[f.key];
    const invalid = draft[f.key] < f.min;
    return (
      <label key={f.key} className={`nudge-field ${changed ? "changed" : ""}`} title={f.help}>
        <span className="nudge-field-label">{f.label}</span>
        <input
          type="number"
          min={f.min}
          max={65535}
          step={1}
          value={draft[f.key]}
          disabled={disabled || na}
          className={invalid ? "invalid" : ""}
          onChange={(e) => set(f.key, e.target.value)}
        />
        <span className="muted small nudge-unit">{na ? "n/a (older firmware)" : f.unit}</span>
      </label>
    );
  };

  return (
    <div className="nudge-params">
      <section>
        <h4>Auto-centering</h4>
        <label className={`inline nudge-check ${draft.autoCenter !== device.autoCenter ? "changed" : ""}`}>
          <input
            type="checkbox"
            checked={draft.autoCenter}
            disabled={disabled}
            onChange={(e) => onChange({ ...draft, autoCenter: e.target.checked })}
          />
          Enable auto-centering
        </label>
        <p className="muted small">
          Re-centers X/Y/Z on the trailing average whenever the cabinet has been still for the interval below. Turning it
          off zeroes the center point.
        </p>
        {AUTO_CENTER.map(row)}
      </section>
      <section>
        <h4>Filters</h4>
        <p className="muted small">Applied to the centered readings: DC blocker, then low-pass, then jitter window.</p>
        {FILTERS.map(row)}
      </section>
      <section>
        <h4>Velocity</h4>
        <p className="muted small">
          The device integrates acceleration into a cabinet velocity. Only simulators that accept velocity input use it.
        </p>
        {VELOCITY.map(row)}
      </section>
    </div>
  );
}

/** First validation problem in a parameter set, or null. */
export function paramsProblem(p: NudgeParams): string | null {
  if (p.autoCenterIntervalMs < 1) return "Auto-centering interval must be at least 1 ms.";
  return null;
}
