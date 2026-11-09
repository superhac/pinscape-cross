// Typed wrapper for the pinout Tauri command (src-tauri/src/pinout.rs), plus
// the usage-category labels the Pinout tab shows. The board model and the
// category mapping live in Rust (crates/pinscape-protocol/src/pinout.rs).
import { invoke } from "@tauri-apps/api/core";

export type PinCategory =
  | "button"
  | "output"
  | "i2c"
  | "spi"
  | "uart"
  | "ir"
  | "plunger"
  | "accel"
  | "system"
  | "other"
  | "free";

export interface HeaderPin {
  /** Physical pin 1..40: 1-20 down the left side (USB at the top), 21-40 up the right. */
  pin: number;
  label: string;
  kind: "gpio" | "ground" | "power" | "control";
  gpio: number | null;
  adcChannel: number | null;
}

export interface PinInfo {
  gpio: number;
  func: number;
  funcName: string;
  output: boolean;
  adc: boolean;
  usage: string | null;
  category: PinCategory;
  headerPin: number | null;
  adcChannel: number | null;
  /** What the board wires this GPIO to, if it isn't on the header. */
  boardUse: string | null;
  /** Hardware state, e.g. "GPIO input". */
  detail: string;
  /** Full one-line description (tooltip / ARIA label). */
  summary: string;
  warning: string | null;
}

export interface Pinout {
  chip: string;
  targetBoard: string;
  /** Board name when it has the standard Pico header; null = list view. */
  board: string | null;
  /** The 40-pin header, empty when the board isn't a standard Pico layout. */
  header: HeaderPin[];
  pins: PinInfo[];
  free: number[];
  fallbackReason: string | null;
}

export const pinoutApi = {
  get: (key: string) => invoke<Pinout>("gpio_pinout", { key }),
};

/** Legend order and labels. */
export const CATEGORIES: { id: PinCategory; label: string }[] = [
  { id: "button", label: "Button input" },
  { id: "output", label: "Output / PWM" },
  { id: "i2c", label: "I²C" },
  { id: "spi", label: "SPI" },
  { id: "uart", label: "UART" },
  { id: "ir", label: "IR" },
  { id: "plunger", label: "Plunger / ADC" },
  { id: "accel", label: "Accelerometer" },
  { id: "system", label: "Board / internal" },
  { id: "other", label: "Other" },
  { id: "free", label: "Free" },
];

export const categoryLabel = (c: PinCategory) => CATEGORIES.find((x) => x.id === c)?.label ?? c;
