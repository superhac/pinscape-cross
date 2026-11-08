//! Config schema, generated from upstream `Firmware/JSONConfigRef.txt` by
//! `tools/schema-gen` (see scripts/sync-upstream-assets.sh), and the rules
//! check ported from the Windows Config Tool (`ConfigEditorWin::CheckSchema*`).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::parser::{Kind, Span, Value};

#[derive(Debug, Deserialize)]
pub struct Schema {
    pub root: usize,
    pub objects: Vec<SchemaObj>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SchemaObj {
    #[serde(default)]
    pub class_type: Option<String>,
    #[serde(default)]
    pub subclass_selector: Option<String>,
    #[serde(default)]
    pub subclasses: BTreeMap<String, usize>,
    #[serde(default)]
    pub props: BTreeMap<String, SchemaProp>,
}

#[derive(Debug, Deserialize)]
pub struct SchemaProp {
    pub types: Vec<SchemaType>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default, rename = "enum")]
    pub enum_values: Vec<String>,
    #[serde(default)]
    pub validate: Option<String>,
    /// Sub-object schema (for object properties and arrays of objects).
    #[serde(default)]
    pub obj: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SchemaType {
    pub kind: String,
    /// Cross-referenced object schema.
    #[serde(default)]
    pub obj: Option<usize>,
    #[serde(default)]
    pub items: Vec<SchemaType>,
}

impl SchemaType {
    pub fn describe(&self) -> String {
        if self.kind == "array" {
            let inner: Vec<String> = self.items.iter().map(|t| t.describe()).collect();
            format!("[{}]", inner.join("|"))
        } else {
            self.kind.clone()
        }
    }
}

impl SchemaProp {
    /// `number|[number]` style type description.
    pub fn type_description(&self) -> String {
        self.types.iter().map(|t| t.describe()).collect::<Vec<_>>().join("|")
    }

    /// The object schema for this property's object value (or array elements):
    /// the direct sub-object, else an object type's cross-reference.
    pub fn sub_obj(&self) -> Option<usize> {
        self.obj.or_else(|| self.types.iter().find(|t| t.kind == "object").and_then(|t| t.obj))
    }

    pub fn accepts(&self, kind: &str) -> bool {
        self.types.iter().any(|t| t.kind == kind)
    }

    pub fn accepts_array_of_objects(&self) -> bool {
        self.types.iter().any(|t| t.kind == "array" && t.items.iter().any(|i| i.kind == "object"))
    }
}

static SCHEMA: OnceLock<Schema> = OnceLock::new();

/// The bundled schema (assets/schema/schema.json).
pub fn schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/schema/schema.json")).expect("bundled schema.json is valid")
    })
}

impl Schema {
    pub fn obj(&self, i: usize) -> &SchemaObj {
        &self.objects[i]
    }

    /// The subclass object selected by the document object's selector property
    /// (e.g. `type: "gpio"`), if any.
    pub fn subclass_of(&self, obj: usize, doc: Option<&Value>) -> Option<usize> {
        let o = self.obj(obj);
        let sel = o.subclass_selector.as_ref()?;
        let v = doc?.get(sel)?;
        if v.is_undefined() {
            return None;
        }
        o.subclasses.get(&v.to_display_string()).copied()
    }

    /// `JSONSchemaObj::FindProp`: look in the object, then in the selected subclass.
    pub fn find_prop(&self, obj: usize, name: &str, doc: Option<&Value>) -> Option<&SchemaProp> {
        let name = name.trim_start_matches('"');
        if let Some(p) = self.obj(obj).props.get(name) {
            return Some(p);
        }
        let sc = self.subclass_of(obj, doc)?;
        self.find_prop(sc, name, doc)
    }

    /// `JSONSchemaObj::ForEachProp`: all properties valid here (base + selected subclass).
    pub fn props_for(&self, obj: usize, doc: Option<&Value>) -> Vec<(&str, &SchemaProp)> {
        let mut out: Vec<(&str, &SchemaProp)> = self.obj(obj).props.iter().map(|(k, v)| (k.as_str(), v)).collect();
        if let Some(sc) = self.subclass_of(obj, doc) {
            out.extend(self.props_for(sc, doc));
        }
        out
    }
}

/// One rules-check finding.
#[derive(Debug, Clone)]
pub struct SchemaIssue {
    pub span: Span,
    pub message: String,
    pub help_anchor: Option<String>,
}

/// Check a parsed document against the schema (the Windows tool's "rules check").
pub fn check(root: &Value) -> Vec<SchemaIssue> {
    let s = schema();
    let mut out = Vec::new();
    check_obj(s, root, Some(s.root), "", &mut out);
    out
}

fn first_line(span: Span, v: &Value) -> Span {
    // Objects/arrays: just underline the opening delimiter.
    match v.kind {
        Kind::Object(_) | Kind::Array(_) => Span::new(span.start, span.start + 1),
        _ => span,
    }
}

fn check_obj(s: &Schema, doc: &Value, schema_obj: Option<usize>, path: &str, out: &mut Vec<SchemaIssue>) {
    let Some(so) = schema_obj else { return };
    let Some(props) = doc.as_object() else {
        out.push(SchemaIssue { span: first_line(doc.span, doc), message: "Expected object".into(), help_anchor: None });
        return;
    };
    for p in props {
        match s.find_prop(so, &p.name, Some(doc)) {
            Some(sp) => {
                let sub = if path.is_empty() { p.name.clone() } else { format!("{path}.{}", p.name) };
                check_prop(s, &p.value, sp, &sub, out);
            }
            None => out.push(SchemaIssue {
                span: p.name_span,
                message: format!("Property \"{}\" is not valid for this object", p.name),
                help_anchor: None,
            }),
        }
    }
    let missing: Vec<String> = s
        .props_for(so, Some(doc))
        .into_iter()
        .filter(|(name, sp)| sp.required && !props.iter().any(|p| p.name == *name))
        .map(|(name, _)| format!("\"{name}\""))
        .collect();
    if !missing.is_empty() {
        let what = if path.is_empty() { "the top level".to_string() } else { path.to_string() };
        out.push(SchemaIssue {
            span: first_line(doc.span, doc),
            message: format!(
                "Required propert{} {} missing for {what}",
                if missing.len() > 1 { "ies" } else { "y" },
                missing.join(", ")
            ),
            help_anchor: None,
        });
    }
}

fn check_prop(s: &Schema, val: &Value, sp: &SchemaProp, path: &str, out: &mut Vec<SchemaIssue>) {
    let anchor = sp.link.clone();
    let issue = |msg: String| SchemaIssue { span: first_line(val.span, val), message: msg, help_anchor: anchor.clone() };

    if !sp.enum_values.is_empty() {
        let matched = if sp.types.first().map(|t| t.kind.as_str()) == Some("number") {
            let n = val.to_number();
            sp.enum_values.iter().any(|e| e.trim().parse::<f64>().ok() == Some(n))
        } else {
            let v = val.to_display_string();
            sp.enum_values.contains(&v)
        };
        if !matched {
            out.push(issue(format!("Value must be one of: {}", sp.enum_values.join(", "))));
        }
    }

    if let Some(re) = &sp.validate {
        if let Ok(re) = regex::Regex::new(&format!("^(?:{re})$")) {
            if !re.is_match(&val.to_display_string()) {
                out.push(issue("Value doesn't match expected pattern".into()));
            }
        }
    }

    let sub = sp.sub_obj();
    let mut matched = false;
    for t in &sp.types {
        match t.kind.as_str() {
            "array" => {
                if let Kind::Array(items) = &val.kind {
                    matched = true;
                    if t.items.iter().any(|i| i.kind == "object") {
                        for (i, item) in items.iter().enumerate() {
                            check_obj(s, item, sub, &format!("{path}[{i}]"), out);
                        }
                    }
                }
            }
            "object" => {
                if matches!(val.kind, Kind::Object(_)) {
                    matched = true;
                    check_obj(s, val, sub, path, out);
                }
            }
            // Any scalar converts implicitly to any other scalar (firmware behavior).
            _ => matched = true,
        }
    }
    if !matched {
        out.push(issue(format!("Invalid type for {path} (expected {})", sp.type_description())));
    }
}
