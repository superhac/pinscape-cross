//! Model for the form view of the config editor: the document's values
//! for one top-level section, laid out by the schema, with a control type
//! for each property. The form never holds its own copy of the config;
//! changes go back to the text through [`crate::edit`].

use std::collections::HashMap;

use serde::Serialize;

use crate::edit::Seg;
use crate::parser::{self, Kind, Value};
use crate::schema::{self, Schema, SchemaProp};
use crate::{LineIndex, Range};

/// Sections listed first (the ones the form view is mainly for).
const COMMON: [&str; 3] = ["buttons", "outputs", "id"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormView {
    /// Set when the form can't be shown; the UI sends the user to the text view.
    pub problem: Option<Problem>,
    /// Top-level sections, common ones first, then the others in the file, then the rest.
    pub sections: Vec<Section>,
    /// The selected section.
    pub field: Option<Field>,
    /// Schema details, referenced by `Field::info` (shared to keep the payload small).
    pub info: Vec<PropInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub message: String,
    /// The first syntax error, to jump to in the text view.
    pub range: Option<Range>,
    /// The text is empty (offer to start a new config).
    pub empty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub name: String,
    pub title: Option<String>,
    pub present: bool,
    pub common: bool,
    /// `object`, `list` or `value`.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropInfo {
    pub title: Option<String>,
    pub summary: Option<String>,
    /// Help anchor in JSONConfigRef.htm.
    pub link: Option<String>,
    /// e.g. `number|[number]`.
    pub type_desc: String,
    /// Scalar kinds accepted (`number`, `string`, `boolean`).
    pub kinds: Vec<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub name: String,
    pub path: Vec<Seg>,
    /// Index into `FormView::info`; `None` for a property the schema doesn't know.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<usize>,
    /// `enum`, `bool`, `number`, `string`, `numberOrString`, `object`, `list` or `raw`
    /// (edit the value's source text).
    pub control: &'static str,
    /// Choices for `enum`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// The property is in the document.
    #[serde(skip_serializing_if = "is_false")]
    pub present: bool,
    /// Present but not valid here (unknown, or not valid for the selected type).
    #[serde(skip_serializing_if = "is_false")]
    pub unknown: bool,
    /// Selects the object's subtype (e.g. `type` of a button source).
    #[serde(skip_serializing_if = "is_false")]
    pub selector: bool,
    /// Not set, but worth showing up front: required, or specific to the
    /// selected subtype (e.g. `gp` for a `gpio` source).
    #[serde(skip_serializing_if = "is_false")]
    pub featured: bool,
    /// Current scalar value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Current value's source text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// Properties of an object value (present ones first).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    /// Entries of a list value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Item>>,
    /// An absent value can be added as an object / as a list.
    #[serde(skip_serializing_if = "is_false")]
    pub can_object: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub can_list: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub index: usize,
    pub path: Vec<Seg>,
    /// One-line summary of the entry.
    pub label: String,
    /// Properties, for object entries with a schema; else edit `raw`.
    pub fields: Option<Vec<Field>>,
    pub raw: String,
}

/// Build the form for `section` (default: the first common section in the file).
pub fn form_view(text: &str, section: Option<&str>) -> FormView {
    let s = schema::schema();
    let doc = parser::parse(text);
    let mut view = FormView { problem: None, sections: vec![], field: None, info: vec![] };
    let idx = LineIndex::new(text);
    if text.trim().is_empty() {
        view.problem = Some(Problem { message: "The config is empty.".into(), range: None, empty: true });
        return view;
    }
    if let Some(e) = doc.errors.first() {
        let n = doc.errors.len();
        let (line, _) = idx.pos(e.span.start);
        view.problem = Some(Problem {
            message: format!(
                "The config has {n} syntax error{} (line {line}: {}). The form view needs a valid file: fix {} in the text view first.",
                if n == 1 { "" } else { "s" },
                e.message,
                if n == 1 { "it" } else { "them" }
            ),
            range: Some(idx.range(e.span)),
            empty: false,
        });
        return view;
    }
    if let Some(open) = doc.unterminated_comment {
        view.problem = Some(Problem {
            message: "The config has a /* comment with no closing */, which hides the rest of the file. Fix it in the text view first.".into(),
            range: Some(idx.range(parser::Span::new(open, open + 2))),
            empty: false,
        });
        return view;
    }
    let Some(props) = doc.root.as_object() else {
        view.problem = Some(Problem {
            message: "The config's top level isn't an object ({ ... }); fix it in the text view.".into(),
            range: Some(idx.range(doc.root.span)),
            empty: false,
        });
        return view;
    };

    // sections
    let root = s.obj(s.root);
    let present = |n: &str| props.iter().any(|p| p.name == n);
    let section_of = |name: &str, sp: &SchemaProp| Section {
        name: name.to_string(),
        title: sp.title.clone(),
        present: present(name),
        common: COMMON.contains(&name),
        kind: if sp.accepts("object") {
            "object"
        } else if sp.accepts_array_of_objects() {
            "list"
        } else {
            "value"
        },
    };
    for n in COMMON {
        if let Some(sp) = root.props.get(n) {
            view.sections.push(section_of(n, sp));
        }
    }
    for p in props {
        if let Some(sp) = root.props.get(&p.name) {
            if !view.sections.iter().any(|x| x.name == p.name) {
                view.sections.push(section_of(&p.name, sp));
            }
        }
    }
    for (n, sp) in &root.props {
        if !view.sections.iter().any(|x| &x.name == n) {
            view.sections.push(section_of(n, sp));
        }
    }

    let chosen = section
        .filter(|n| root.props.contains_key(*n))
        .map(str::to_string)
        .or_else(|| COMMON.iter().find(|n| present(n)).map(|n| n.to_string()))
        .unwrap_or_else(|| COMMON[0].to_string());
    let mut b = Builder { s, text, info: vec![], map: HashMap::new() };
    let sp = root.props.get(&chosen);
    view.field = Some(b.field(&chosen, sp, doc.root.get(&chosen), vec![Seg::Key(chosen.clone())], None));
    view.info = b.info;
    view
}

struct Builder<'s, 't> {
    s: &'s Schema,
    text: &'t str,
    info: Vec<PropInfo>,
    map: HashMap<*const SchemaProp, usize>,
}

impl Builder<'_, '_> {
    fn info_for(&mut self, sp: &SchemaProp) -> usize {
        let key = sp as *const SchemaProp;
        if let Some(i) = self.map.get(&key) {
            return *i;
        }
        let i = self.info.len();
        self.info.push(PropInfo {
            title: sp.title.clone(),
            summary: sp.summary.clone(),
            link: sp.link.clone(),
            type_desc: sp.type_description(),
            kinds: sp
                .types
                .iter()
                .filter(|t| !matches!(t.kind.as_str(), "object" | "array"))
                .map(|t| t.kind.clone())
                .collect(),
            required: sp.required,
        });
        self.map.insert(key, i);
        i
    }

    /// `selector`: the subclass names, if this property selects the object's subtype.
    fn field(&mut self, name: &str, sp: Option<&SchemaProp>, value: Option<&Value>, path: Vec<Seg>, selector: Option<Vec<String>>) -> Field {
        let mut f = Field {
            name: name.to_string(),
            path,
            info: sp.map(|sp| self.info_for(sp)),
            control: "raw",
            options: vec![],
            present: value.is_some(),
            unknown: value.is_some() && sp.is_none(),
            selector: selector.is_some(),
            featured: false,
            value: value.and_then(scalar_json),
            raw: value.map(|v| self.text[v.span.start..v.span.end].to_string()),
            fields: vec![],
            items: None,
            can_object: false,
            can_list: false,
        };
        let Some(sp) = sp else { return f };
        f.can_object = sp.accepts("object");
        f.can_list = sp.accepts_array_of_objects();
        let only = |k: &str| sp.types.len() == 1 && sp.types[0].kind == k;
        let scalar_kinds_only = sp.types.iter().all(|t| matches!(t.kind.as_str(), "number" | "string"));
        match value.map(|v| &v.kind) {
            Some(Kind::Object(_)) if f.can_object => {
                f.control = "object";
                f.fields = self.obj_fields(sp.sub_obj(), value.unwrap(), &f.path);
            }
            Some(Kind::Array(items)) if f.can_list => {
                f.control = "list";
                let sub = sp.sub_obj();
                f.items = Some(
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, it)| {
                            let mut path = f.path.clone();
                            path.push(Seg::Index(i));
                            let fields = (matches!(it.kind, Kind::Object(_)) && sub.is_some()).then(|| self.obj_fields(sub, it, &path));
                            Item { index: i, label: compact(it, 110), raw: self.text[it.span.start..it.span.end].to_string(), path, fields }
                        })
                        .collect(),
                );
            }
            Some(Kind::Object(_)) | Some(Kind::Array(_)) => {}
            v => {
                let is = |k: fn(&Kind) -> bool| v.is_none_or(k);
                if let Some(sel) = selector {
                    f.control = "enum";
                    f.options = sel;
                    for e in &sp.enum_values {
                        if !f.options.contains(e) {
                            f.options.push(e.clone());
                        }
                    }
                } else if !sp.enum_values.is_empty() {
                    f.control = "enum";
                    f.options = sp.enum_values.clone();
                } else if only("boolean") && is(|k| matches!(k, Kind::Bool(_))) {
                    f.control = "bool";
                } else if only("number") && is(|k| matches!(k, Kind::Number(_))) {
                    f.control = "number";
                } else if only("string") && is(|k| matches!(k, Kind::String(_))) {
                    f.control = "string";
                } else if scalar_kinds_only && is(|k| matches!(k, Kind::Number(_) | Kind::String(_))) {
                    f.control = "numberOrString";
                } else if v.is_none() && f.can_object {
                    f.control = "object";
                } else if v.is_none() && f.can_list {
                    f.control = "list";
                }
            }
        }
        f
    }

    /// The fields of an object value: subtype selector, then the properties
    /// in the file (in file order), then the schema's other properties.
    fn obj_fields(&mut self, so: Option<usize>, doc: &Value, path: &[Seg]) -> Vec<Field> {
        let props = doc.as_object().unwrap_or(&[]);
        let child = |n: &str| {
            let mut p = path.to_vec();
            p.push(Seg::Key(n.to_string()));
            p
        };
        let mut out: Vec<Field> = vec![];
        let Some(so) = so else {
            for p in props {
                out.push(self.field(&p.name, None, Some(&p.value), child(&p.name), None));
            }
            return out;
        };
        let s = self.s;
        let obj = s.obj(so);
        let selector = obj.subclass_selector.clone();
        if let Some(sel) = &selector {
            let names: Vec<String> = obj.subclasses.keys().cloned().collect();
            let sp = s.find_prop(so, sel, Some(doc));
            out.push(self.field(sel, sp, doc.get(sel), child(sel), Some(names)));
        }
        for (i, p) in props.iter().enumerate() {
            if Some(&p.name) == selector.as_ref() || props[i + 1..].iter().any(|q| q.name == p.name) {
                continue;
            }
            let sp = s.find_prop(so, &p.name, Some(doc));
            out.push(self.field(&p.name, sp, Some(&p.value), child(&p.name), None));
        }
        let mut absent: Vec<(&str, &SchemaProp)> =
            s.props_for(so, Some(doc)).into_iter().filter(|(n, _)| !out.iter().any(|f| f.name == *n)).collect();
        absent.sort_by(|a, b| b.1.required.cmp(&a.1.required).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        absent.dedup_by(|a, b| a.0 == b.0);
        for (n, sp) in absent {
            let mut f = self.field(n, Some(sp), None, child(n), None);
            f.featured = sp.required || !obj.props.contains_key(n);
            out.push(f);
        }
        out
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn scalar_json(v: &Value) -> Option<serde_json::Value> {
    Some(match &v.kind {
        Kind::String(s) => serde_json::Value::String(s.clone()),
        Kind::Number(n) => serde_json::Number::from_f64(*n).map(serde_json::Value::Number)?,
        Kind::Bool(b) => serde_json::Value::Bool(*b),
        Kind::Null | Kind::Undefined => serde_json::Value::Null,
        _ => return None,
    })
}

/// One-line summary of a value, e.g. `type: "push", source: { type: "gpio", gp: 7 }`.
pub fn compact(v: &Value, max: usize) -> String {
    fn go(v: &Value, out: &mut String, top: bool) {
        match &v.kind {
            Kind::Object(props) => {
                if !top {
                    out.push_str("{ ");
                }
                for (i, p) in props.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&p.name);
                    out.push_str(": ");
                    go(&p.value, out, false);
                }
                if !top {
                    out.push_str(" }");
                }
            }
            Kind::Array(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    go(it, out, false);
                }
                out.push(']');
            }
            Kind::String(s) => out.push_str(&crate::edit::quote(s, '"')),
            _ => out.push_str(&v.to_display_string()),
        }
        if out.len() > 4096 {
            out.truncate(4096);
        }
    }
    let mut s = String::new();
    go(v, &mut s, true);
    if s.chars().count() > max {
        s = s.chars().take(max.saturating_sub(1)).collect::<String>() + "…";
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(fields: &'a [Field], name: &str) -> &'a Field {
        fields.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no field {name}"))
    }

    const DOC: &str = "{\n  // hi\n  id: { unitNum: 1, unitName: \"Pin\", bogus: 3 },\n  buttons: [\n    { type: \"push\", source: { type: \"gpio\", gp: 7 }, action: { type: \"key\", key: \"A\" } },\n    'oops',\n  ],\n}";

    #[test]
    fn sections_and_default() {
        let v = form_view(DOC, None);
        assert!(v.problem.is_none());
        let names: Vec<_> = v.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(&names[..3], &["buttons", "outputs", "id"]);
        assert!(v.sections.iter().find(|s| s.name == "buttons").unwrap().present);
        assert!(!v.sections.iter().find(|s| s.name == "outputs").unwrap().present);
        assert_eq!(v.sections.iter().find(|s| s.name == "buttons").unwrap().kind, "list");
        assert_eq!(v.field.as_ref().unwrap().name, "buttons");
    }

    #[test]
    fn button_items() {
        let v = form_view(DOC, Some("buttons"));
        let f = v.field.unwrap();
        assert_eq!(f.control, "list");
        let items = f.items.unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].label.starts_with("type: \"push\", source: { type: \"gpio\", gp: 7 }"), "{}", items[0].label);
        assert!(items[1].fields.is_none(), "a string entry is edited as text");
        let fields = items[0].fields.as_ref().unwrap();
        // selector first, with subclass names as choices
        assert_eq!(fields[0].name, "type");
        assert!(fields[0].selector && fields[0].options.contains(&"toggle".to_string()));
        let src = find(fields, "source");
        assert_eq!(src.control, "object");
        assert_eq!(src.path, vec![Seg::Key("buttons".into()), Seg::Index(0), Seg::Key("source".into())]);
        let gp = find(&src.fields, "gp");
        assert_eq!((gp.control, gp.present), ("number", true));
        assert_eq!(gp.value, Some(serde_json::json!(7.0)));
        let pull = find(&src.fields, "pull");
        assert_eq!((pull.control, pull.present), ("bool", false));
        assert!(pull.featured, "gpio-specific");
        assert!(!find(&src.fields, "active").featured, "common to all sources");
        let key = find(&find(fields, "action").fields, "key");
        assert_eq!(key.control, "numberOrString");
        assert!(find(fields, "name").info.is_some() && !find(fields, "name").present);
        assert!(v.info.iter().any(|i| i.link.is_some()));
    }

    #[test]
    fn unknown_props_and_scalars() {
        let v = form_view(DOC, Some("id"));
        let f = v.field.unwrap();
        assert_eq!(f.control, "object");
        let b = find(&f.fields, "bogus");
        assert!(b.unknown && b.control == "raw" && b.raw.as_deref() == Some("3"));
        assert_eq!(find(&f.fields, "unitName").control, "string");
        assert_eq!(find(&f.fields, "ledWizUnitNum").control, "raw");
        // present ones come first, in file order
        assert_eq!(f.fields[0].name, "unitNum");
    }

    #[test]
    fn absent_section() {
        let v = form_view(DOC, Some("plunger"));
        let f = v.field.unwrap();
        assert!(!f.present && f.control == "object" && f.can_object);
    }

    #[test]
    fn problems() {
        let v = form_view("{ a: 1 b: 2 }", None);
        let p = v.problem.unwrap();
        assert!(p.message.contains("syntax error") && p.range.is_some());
        assert!(form_view("  \n", None).problem.unwrap().empty);
        assert!(form_view("[1]", None).problem.is_some());
        assert!(form_view("{ a: 1 /* x ", None).problem.is_some());
    }

    #[test]
    fn templates_build() {
        for t in [
            include_str!("../../../assets/templates/DIYExpansionBoard.json"),
            include_str!("../../../assets/templates/ProExpansionBoard.json"),
            include_str!("../../../assets/templates/Starter.json"),
        ] {
            for s in ["buttons", "outputs", "id", "plunger", "nudge"] {
                let v = form_view(t, Some(s));
                assert!(v.problem.is_none());
                assert_eq!(v.field.unwrap().name, s);
            }
        }
    }
}
