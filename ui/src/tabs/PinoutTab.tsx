import { CSSProperties, KeyboardEvent as ReactKeyboardEvent, useRef, useState } from "react";
import { DeviceSnapshot } from "../api";
import { useInterval } from "../lib/useDevices";
import { CATEGORIES, categoryLabel, HeaderPin, PinCategory, PinInfo, Pinout, pinoutApi } from "../lib/api-pinout";
import "./PinoutTab.css";

interface Props {
  dev: DeviceSnapshot;
  onStatus: (s: string) => void;
}

/** Re-read the assignments now and then: they change after a config save + reboot. */
const POLL_MS = 5000;

// Visual Pico pinout / GPIO-assignment view, built from QUERY_GPIO_CONFIG
// (like the pinout diagram on the Windows tool's Device Overview window).
export default function PinoutTab({ dev, onStatus }: Props) {
  const [po, setPo] = useState<Pinout | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // Pin under the mouse / keyboard focus, and the one clicked to keep it shown.
  const [hover, setHover] = useState<number | null>(null);
  const [pinned, setPinned] = useState<number | null>(null);
  // Legend filter: highlight one category, dim the rest.
  const [only, setOnly] = useState<PinCategory | null>(null);

  const load = (manual = false) =>
    pinoutApi
      .get(dev.key)
      .then((p) => {
        setPo(p);
        setErr(null);
        if (manual) onStatus("Pinout refreshed.");
      })
      .catch((e) => setErr(String(e)));
  useInterval(() => load(), POLL_MS);

  if (!po) {
    return <div className="pinout">{err ? <p className="error">{err}</p> : <p className="muted">Loading…</p>}</div>;
  }

  const byGpio = new Map(po.pins.map((p) => [p.gpio, p]));
  const activeGpio = hover ?? pinned;
  const active = activeGpio != null ? (byGpio.get(activeGpio) ?? null) : null;
  const counts = new Map<PinCategory, number>();
  for (const p of po.pins) counts.set(p.category, (counts.get(p.category) ?? 0) + 1);
  const warnings = po.pins.filter((p) => p.warning);
  const internal = po.header.length ? po.pins.filter((p) => p.headerPin == null) : [];

  const pinProps: PinHandlers = {
    active: activeGpio,
    pinned,
    only,
    onHover: setHover,
    onToggle: (g) => setPinned((x) => (x === g ? null : g)),
  };

  return (
    <div className="pinout">
      <div className="toolbar">
        <strong>{po.board ?? `Board "${po.targetBoard}"`}</strong>
        <span className="muted small">
          {po.chip} · firmware build target "{po.targetBoard}"
        </span>
        <span className="spacer" />
        <button onClick={() => load(true)}>Refresh</button>
      </div>
      {err && <p className="error">{err}</p>}
      {po.fallbackReason && <p className="pinout-note">{po.fallbackReason} All GPIOs are listed by number instead.</p>}

      <div className="pinout-main">
        {po.header.length ? (
          <BoardDiagram po={po} byGpio={byGpio} h={pinProps} />
        ) : (
          <PinGrid pins={po.pins} h={pinProps} />
        )}

        <aside className="pinout-side">
          <section className="card pinout-details">
            <h3>{active ? `GP${active.gpio}` : "Pin details"}</h3>
            {active ? <Details p={active} /> : <p className="muted">Hover over, focus or click a pin to see its assignment.</p>}
          </section>

          <section className="card">
            <h3>Legend</h3>
            <ul className="pinout-legend" aria-label="Usage categories (select one to highlight its pins)">
              {CATEGORIES.filter((c) => counts.get(c.id) || c.id === "free").map((c) => (
                <li key={c.id}>
                  <button
                    className={`legend-item cat-${c.id}`}
                    aria-pressed={only === c.id}
                    onClick={() => setOnly((x) => (x === c.id ? null : c.id))}
                    title="Highlight these pins"
                  >
                    <span className="swatch" aria-hidden />
                    {c.label}
                    <span className="muted small">{counts.get(c.id) ?? 0}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>

          <section className="card">
            <h3>Free GPIOs</h3>
            {po.free.length ? (
              <p className="pinout-free">
                {po.free.map((g) => (
                  <span key={g} className="pinout-tag">
                    GP{g}
                    {byGpio.get(g)?.adcChannel != null && <span className="muted"> (ADC{byGpio.get(g)!.adcChannel})</span>}
                  </span>
                ))}
              </p>
            ) : (
              <p className="muted">None: every GPIO on the header is assigned.</p>
            )}
          </section>

          {warnings.length > 0 && (
            <section className="card pinout-warnings">
              <h3>Check these</h3>
              <ul>
                {warnings.map((p) => (
                  <li key={p.gpio}>
                    <strong>GP{p.gpio}</strong> ({p.usage}): {p.warning}
                  </li>
                ))}
              </ul>
            </section>
          )}

          {internal.length > 0 && (
            <section className="card">
              <h3>Not on the header</h3>
              <div className="pinout-internal">
                {internal.map((p) => (
                  <PinChip key={p.gpio} p={p} h={pinProps} showGpio />
                ))}
              </div>
            </section>
          )}
        </aside>
      </div>
    </div>
  );
}

interface PinHandlers {
  active: number | null;
  pinned: number | null;
  only: PinCategory | null;
  onHover: (g: number | null) => void;
  onToggle: (g: number) => void;
}

/** Classes shared by every element that represents a GPIO. */
function pinClass(p: PinInfo, h: PinHandlers) {
  return (
    `cat-${p.category}` +
    (h.active === p.gpio ? " active" : "") +
    (h.only && h.only !== p.category ? " dim" : "") +
    (p.warning ? " warn" : "")
  );
}

function Details({ p }: { p: PinInfo }) {
  const rows: [string, string][] = [
    ["Header pin", p.headerPin != null ? String(p.headerPin) : "—"],
    ["Category", categoryLabel(p.category)],
    ["Assigned to", p.usage ?? (p.category === "free" ? "nothing (free)" : "no Pinscape label")],
    ["Function", `${p.funcName === "NULL" ? "none" : p.funcName}${p.func === 5 || p.adc ? ` (${p.detail})` : ""}`],
  ];
  if (p.adcChannel != null) rows.push(["ADC", `ADC${p.adcChannel}-capable${p.adc ? ", in use as ADC input" : ""}`]);
  if (p.boardUse) rows.push(["On the board", p.boardUse]);
  return (
    <>
      <table className="kv">
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k}>
              <th>{k}</th>
              <td>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {p.warning && <p className="warn small">{p.warning}</p>}
    </>
  );
}

/** The assignment label beside a header pin (focusable; the keyboard/AT handle for the pin). */
function PinChip({
  p,
  h,
  showGpio,
  style,
  side,
  btnRef,
  onKeyDown,
}: {
  p: PinInfo;
  h: PinHandlers;
  showGpio?: boolean;
  style?: CSSProperties;
  side?: "l" | "r";
  btnRef?: (el: HTMLButtonElement | null) => void;
  onKeyDown?: (e: ReactKeyboardEvent) => void;
}) {
  const text = p.usage ?? (p.category === "free" ? "free" : (p.boardUse ?? p.funcName));
  return (
    <button
      ref={btnRef}
      className={`pin-chip ${side ?? ""} ${pinClass(p, h)}`}
      style={style}
      aria-label={p.summary}
      aria-pressed={h.pinned === p.gpio}
      title={p.summary}
      onMouseEnter={() => h.onHover(p.gpio)}
      onMouseLeave={() => h.onHover(null)}
      onFocus={() => h.onHover(p.gpio)}
      onBlur={() => h.onHover(null)}
      onClick={() => h.onToggle(p.gpio)}
      onKeyDown={onKeyDown}
    >
      {p.warning && (
        <span className="pin-warn" aria-hidden>
          !
        </span>
      )}
      {showGpio && <span className="pin-gp">GP{p.gpio}</span>}
      <span className="pin-text">{text}</span>
    </button>
  );
}

/** Grid row (2-21) and side for a physical pin: 1-20 down the left, 21-40 up the right. */
const rowOf = (pin: number) => (pin <= 20 ? pin + 1 : 42 - pin);
const leftSide = (pin: number) => pin <= 20;

// Grid columns: chip | name | pad | board middle | pad | name | chip
const COL = { chipL: 1, nameL: 2, padL: 3, mid: 4, padR: 5, nameR: 6, chipR: 7 };

function BoardDiagram({ po, byGpio, h }: { po: Pinout; byGpio: Map<number, PinInfo>; h: PinHandlers }) {
  const refs = useRef(new Map<number, HTMLButtonElement>());
  const isGpio = (pin: number) => po.header[pin - 1]?.gpio != null;

  // Arrow keys move between pins as laid out: up/down along a side, left/right across.
  const onKey = (pin: number) => (e: ReactKeyboardEvent) => {
    const k = e.key;
    let next: number | null = null;
    if (k === "ArrowLeft" || k === "ArrowRight") {
      const across = 41 - pin;
      if ((k === "ArrowRight") === leftSide(pin) && isGpio(across)) next = across;
    } else if (k === "ArrowUp" || k === "ArrowDown") {
      // pin numbers grow downwards on the left, upwards on the right
      const step = (k === "ArrowDown") === leftSide(pin) ? 1 : -1;
      for (let q = pin + step; leftSide(q) === leftSide(pin) && q >= 1 && q <= 40; q += step) {
        if (isGpio(q)) {
          next = q;
          break;
        }
      }
    }
    if (next != null) {
      e.preventDefault();
      refs.current.get(next)?.focus();
    }
  };

  const cell = (col: number, pin: number): CSSProperties => ({ gridColumn: col, gridRow: rowOf(pin) });

  return (
    <figure className="pico-figure">
      <div className="pico-board" role="group" aria-label={`${po.board} header, 40 pins, top view with USB at the top`}>
        <div className="pcb" style={{ gridColumn: `${COL.padL} / ${COL.padR + 1}`, gridRow: "1 / 23" }} aria-hidden>
          <span className="usb" />
          <span className="cpu">{po.chip}</span>
        </div>
        {po.header.map((hp) => {
          const left = leftSide(hp.pin);
          const p = hp.gpio != null ? byGpio.get(hp.gpio) : undefined;
          return (
            <HeaderRow
              key={hp.pin}
              hp={hp}
              p={p}
              h={h}
              left={left}
              padStyle={cell(left ? COL.padL : COL.padR, hp.pin)}
              nameStyle={cell(left ? COL.nameL : COL.nameR, hp.pin)}
              chipStyle={cell(left ? COL.chipL : COL.chipR, hp.pin)}
              btnRef={(el) => {
                if (el) refs.current.set(hp.pin, el);
                else refs.current.delete(hp.pin);
              }}
              onKeyDown={onKey(hp.pin)}
            />
          );
        })}
      </div>
      <figcaption className="muted small">
        Top view, USB connector at the top. Physical pin numbers are on the pads; ADC-capable pins are marked ADC0–ADC2.
        Arrow keys move between pins.
      </figcaption>
    </figure>
  );
}

function HeaderRow({
  hp,
  p,
  h,
  left,
  padStyle,
  nameStyle,
  chipStyle,
  btnRef,
  onKeyDown,
}: {
  hp: HeaderPin;
  p: PinInfo | undefined;
  h: PinHandlers;
  left: boolean;
  padStyle: CSSProperties;
  nameStyle: CSSProperties;
  chipStyle: CSSProperties;
  btnRef: (el: HTMLButtonElement | null) => void;
  onKeyDown: (e: ReactKeyboardEvent) => void;
}) {
  // Pads and names follow the mouse too, but only the chip takes focus.
  const mouse = p
    ? {
        onMouseEnter: () => h.onHover(p.gpio),
        onMouseLeave: () => h.onHover(null),
        onClick: () => h.onToggle(p.gpio),
      }
    : {};
  const cls = p ? pinClass(p, h) : `kind-${hp.kind}`;
  return (
    <>
      <span className={`pin-pad ${cls}${hp.kind === "ground" ? " square" : ""}`} style={padStyle} aria-hidden {...mouse}>
        {hp.pin}
      </span>
      <span className={`pin-name ${left ? "l" : "r"} ${cls}`} style={nameStyle} aria-hidden {...mouse}>
        {hp.label}
        {hp.adcChannel != null && <span className="adc-tag">ADC{hp.adcChannel}</span>}
      </span>
      {p && <PinChip p={p} h={h} style={chipStyle} side={left ? "l" : "r"} btnRef={btnRef} onKeyDown={onKeyDown} />}
    </>
  );
}

/** Fallback for boards without the standard Pico header: every GPIO by number. */
function PinGrid({ pins, h }: { pins: PinInfo[]; h: PinHandlers }) {
  return (
    <div className="pinout-grid" role="group" aria-label="GPIO assignments by GPIO number">
      {pins.map((p) => (
        <PinChip key={p.gpio} p={p} h={h} showGpio />
      ))}
    </div>
  );
}
