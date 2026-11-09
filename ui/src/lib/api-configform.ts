// Typed wrappers for the config form-view commands (src-tauri/src/configform.rs,
// pinscape_config::form and ::edit).
import { invoke } from "@tauri-apps/api/core";
import { EditorRange } from "../api";

/** Path into the document: property names and array indexes. */
export type Seg = string | number;
export type Path = Seg[];

export interface FormProblem {
  message: string;
  range: EditorRange | null;
  empty: boolean;
}

export interface FormSection {
  name: string;
  title: string | null;
  present: boolean;
  common: boolean;
  kind: "object" | "list" | "value";
}

export interface PropInfo {
  title: string | null;
  summary: string | null;
  link: string | null;
  typeDesc: string;
  /** Scalar kinds accepted: number, string, boolean. */
  kinds: string[];
  required: boolean;
}

export type Control = "enum" | "bool" | "number" | "string" | "numberOrString" | "object" | "list" | "raw";

export interface FormField {
  name: string;
  path: Path;
  info?: number;
  control: Control;
  options?: string[];
  present?: boolean;
  unknown?: boolean;
  selector?: boolean;
  /** Not set, but shown up front (required, or specific to the selected type). */
  featured?: boolean;
  value?: string | number | boolean | null;
  raw?: string;
  fields?: FormField[];
  items?: FormItem[];
  canObject?: boolean;
  canList?: boolean;
}

export interface FormItem {
  index: number;
  path: Path;
  label: string;
  fields: FormField[] | null;
  raw: string;
}

export interface FormView {
  problem: FormProblem | null;
  sections: FormSection[];
  field: FormField | null;
  info: PropInfo[];
}

export type EditOp =
  | { op: "set"; path: Path; value: string | number | boolean }
  | { op: "setRaw"; path: Path; text: string }
  | { op: "create"; path: Path; object: boolean }
  | { op: "remove"; path: Path }
  | { op: "addItem"; path: Path }
  | { op: "duplicate"; path: Path };

export interface TextEdit {
  range: EditorRange;
  text: string;
}

export interface EditResult {
  edits: TextEdit[];
  /** The whole text after the edits. */
  text: string;
}

export const formApi = {
  form: (text: string, section: string | null) => invoke<FormView>("config_form", { text, section }),
  /** Rejects with a message if the change can't be made. */
  edit: (text: string, op: EditOp) => invoke<EditResult>("config_form_edit", { text, op }),
};

/** `buttons[2].source` */
export const pathKey = (p: Path) =>
  p.map((s, i) => (typeof s === "number" ? `[${s}]` : i ? `.${s}` : s)).join("");
