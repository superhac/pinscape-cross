//! Firmware-compatible parser, schema checker and editor services for
//! Pinscape Pico JSON configs.
//!
//! * [`parser`]: port of the firmware's `JSON.cpp` (JSON5-like syntax).
//! * [`schema`]: schema generated from upstream `JSONConfigRef.txt`, and the
//!   Windows Config Tool's rules check.
//! * [`context`]: cursor context for completion, hover and breadcrumbs.
//! * [`form`] and [`edit`]: the form view's model and the minimal text edits
//!   it makes (comments and formatting are kept).
//!
//! Editor-facing functions take and return positions as Monaco does:
//! 1-based line numbers and 1-based columns counted in UTF-16 code units.

pub mod context;
pub mod edit;
pub mod form;
pub mod parser;
pub mod schema;

use context::Position;
use parser::{Kind, Span};
use serde::Serialize;

/// Maps between byte offsets and editor (line, UTF-16 column) positions.
pub struct LineIndex<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Line breaks are `\r\n`, `\n` or a lone `\r`, as in Monaco (and the firmware).
    pub fn new(text: &'a str) -> Self {
        let b = text.as_bytes();
        let mut line_starts = vec![0];
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\r' if b.get(i + 1) == Some(&b'\n') => {
                    line_starts.push(i + 2);
                    i += 2;
                    continue;
                }
                b'\r' | b'\n' => line_starts.push(i + 1),
                _ => {}
            }
            i += 1;
        }
        LineIndex { text, line_starts }
    }

    /// End of a line's content (before its terminator).
    fn line_end(&self, li: usize) -> usize {
        let next = self.line_starts.get(li + 1).copied().unwrap_or(self.text.len());
        let b = self.text.as_bytes();
        let mut e = next;
        if e > self.line_starts[li] && li + 1 < self.line_starts.len() {
            e -= 1;
            if e > self.line_starts[li] && b[e] == b'\n' && b[e - 1] == b'\r' {
                e -= 1;
            }
        }
        e
    }

    /// Byte offset -> (1-based line, 1-based UTF-16 column).
    pub fn pos(&self, byte: usize) -> (u32, u32) {
        let byte = byte.min(self.text.len());
        let line = self.line_starts.partition_point(|&s| s <= byte) - 1;
        let start = self.line_starts[line];
        let col: usize = self.text.get(start..byte).map(|s| s.encode_utf16().count()).unwrap_or(byte - start);
        (line as u32 + 1, col as u32 + 1)
    }

    /// (1-based line, 1-based UTF-16 column) -> byte offset.
    pub fn offset(&self, line: u32, col: u32) -> usize {
        let li = (line.max(1) as usize - 1).min(self.line_starts.len() - 1);
        let start = self.line_starts[li];
        let end = self.line_end(li);
        let mut units = col.max(1) as usize - 1;
        for (i, ch) in self.text[start..end].char_indices() {
            if units == 0 {
                return start + i;
            }
            units = units.saturating_sub(ch.len_utf16());
        }
        end
    }

    pub fn range(&self, span: Span) -> Range {
        let (sl, sc) = self.pos(span.start);
        let (el, ec) = self.pos(span.end.max(span.start));
        Range { start_line: sl, start_col: sc, end_line: el, end_col: ec }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A diagnostic for the editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Severity,
    pub message: String,
    /// Anchor in JSONConfigRef.htm for the related property, if any.
    pub help_anchor: Option<String>,
}

/// Syntax errors (as the firmware would report them) plus rules-check warnings.
pub fn validate(text: &str) -> Vec<Diagnostic> {
    let idx = LineIndex::new(text);
    let doc = parser::parse(text);
    let mut out: Vec<Diagnostic> = doc
        .errors
        .iter()
        .map(|e| Diagnostic {
            range: idx.range(widen(text, e.span)),
            severity: Severity::Error,
            message: e.message.clone(),
            help_anchor: None,
        })
        .collect();
    if let Some(open) = doc.unterminated_comment {
        out.push(Diagnostic {
            range: idx.range(Span::new(open, open + 2)),
            severity: Severity::Warning,
            message: "Unterminated /* comment: everything after this point is ignored".into(),
            help_anchor: None,
        });
    }
    // Like the Windows tool, only run the rules check on syntactically valid text.
    if doc.errors.is_empty() {
        out.extend(schema::check(&doc.root).into_iter().map(|i| Diagnostic {
            range: idx.range(widen(text, i.span)),
            severity: Severity::Warning,
            message: i.message,
            help_anchor: i.help_anchor,
        }));
    }
    out
}

/// Make zero-width spans one character wide so they're visible in the editor.
fn widen(text: &str, s: Span) -> Span {
    if s.end > s.start {
        return s;
    }
    let end = text[s.start.min(text.len())..].chars().next().map(|c| s.start + c.len_utf8()).unwrap_or(s.start);
    Span::new(s.start, end)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub label: String,
    /// `property` or `value`.
    pub kind: &'static str,
    /// Type description, e.g. `number|[number]`.
    pub detail: Option<String>,
    pub documentation: Option<String>,
    /// Monaco snippet syntax.
    pub insert_text: String,
    pub is_snippet: bool,
    pub range: Range,
    pub sort_text: String,
    pub required: bool,
}

fn snippet_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('$', "\\$").replace('}', "\\}")
}

fn key_snippet(name: &str, sp: &schema::SchemaProp, s: &schema::Schema) -> String {
    let n = snippet_escape(name);
    if sp.accepts("object") && !sp.accepts("array") {
        return format!("{n}: {{\n\t$0\n}},");
    }
    if sp.accepts_array_of_objects() {
        return format!("{n}: [\n\t{{\n\t\t$0\n\t}},\n],");
    }
    if sp.accepts("array") && !sp.accepts("number") && !sp.accepts("string") && !sp.accepts("gpio") {
        return format!("{n}: [$0],");
    }
    let mut vals = value_choices(sp, None, s);
    if !vals.is_empty() && vals.len() <= 40 {
        vals.iter_mut().for_each(|v| *v = v.replace(',', "\\,").replace('|', "\\|"));
        return format!("{n}: ${{1|{}|}},", vals.join(","));
    }
    if sp.accepts("string") && !sp.accepts("number") {
        return format!("{n}: \"$1\",");
    }
    format!("{n}: $1,")
}

/// Literal values that can be offered for a property (enum values, booleans,
/// subclass selector values), formatted as they'd be typed.
fn value_choices(sp: &schema::SchemaProp, selector_values: Option<Vec<String>>, _s: &schema::Schema) -> Vec<String> {
    let quote = |v: &str| {
        if sp.accepts("string") && v.parse::<f64>().is_err() {
            format!("\"{v}\"")
        } else {
            v.to_string()
        }
    };
    let mut out: Vec<String> = Vec::new();
    if let Some(sel) = selector_values {
        out.extend(sel.iter().map(|v| format!("\"{v}\"")));
    }
    out.extend(sp.enum_values.iter().map(|v| quote(v)));
    if sp.accepts("boolean") {
        out.push("true".into());
        out.push("false".into());
    }
    out.dedup();
    out
}

fn prop_doc(name: &str, sp: &schema::SchemaProp) -> String {
    let mut md = String::new();
    if let Some(t) = &sp.title {
        md.push_str(&format!("**{t}**\n\n"));
    }
    md.push_str(&format!("`{name}`: `{}`{}\n\n", sp.type_description(), if sp.required { " (required)" } else { "" }));
    if let Some(d) = sp.doc.as_ref().or(sp.summary.as_ref()) {
        md.push_str(d);
    }
    if !sp.enum_values.is_empty() && sp.enum_values.len() <= 60 {
        md.push_str(&format!("\n\nAllowed values: {}", sp.enum_values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", ")));
    }
    md
}

/// Completions at (line, col).
pub fn completions(text: &str, line: u32, col: u32) -> Vec<Completion> {
    let idx = LineIndex::new(text);
    let off = idx.offset(line, col);
    let doc = parser::parse(text);
    let ctx = context::locate(text, &doc.root, off);
    let s = schema::schema();
    let Some(level) = ctx.innermost() else { return vec![] };

    match &ctx.position {
        Position::Key { replace } => {
            let Some(obj) = level.schema_obj else { return vec![] };
            let present: Vec<&str> = level
                .doc
                .as_object()
                .unwrap_or(&[])
                .iter()
                .filter(|p| p.name_span != *replace)
                .map(|p| p.name.as_str())
                .collect();
            // Editing an existing name: insert just the name (keep its value).
            let renaming = level.doc.as_object().unwrap_or(&[]).iter().any(|p| p.name_span == *replace);
            let mut items: Vec<Completion> = s
                .props_for(obj, Some(level.doc))
                .into_iter()
                .filter(|(n, _)| !present.contains(n))
                .map(|(name, sp)| Completion {
                    label: name.to_string(),
                    kind: "property",
                    detail: Some(sp.type_description() + if sp.required { " · required" } else { "" }),
                    documentation: Some(prop_doc(name, sp)),
                    insert_text: if renaming { snippet_escape(name) } else { key_snippet(name, sp, s) },
                    is_snippet: true,
                    range: idx.range(*replace),
                    sort_text: format!("{}{}", if sp.required { "0" } else { "1" }, name.to_lowercase()),
                    required: sp.required,
                })
                .collect();
            items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
            items
        }
        Position::Value { prop, replace } => {
            let Some(obj) = level.schema_obj else { return vec![] };
            let Some(sp) = s.find_prop(obj, &prop.name, Some(level.doc)) else { return vec![] };
            let so = s.obj(obj);
            let selector_values = (so.subclass_selector.as_deref() == Some(prop.name.as_str()))
                .then(|| so.subclasses.keys().cloned().collect::<Vec<_>>());
            value_choices(sp, selector_values, s)
                .into_iter()
                .enumerate()
                .map(|(i, v)| Completion {
                    label: v.clone(),
                    kind: "value",
                    detail: Some(prop.name.clone()),
                    documentation: None,
                    insert_text: v,
                    is_snippet: false,
                    range: idx.range(*replace),
                    sort_text: format!("{i:04}"),
                    required: false,
                })
                .collect()
        }
        Position::Other => vec![],
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hover {
    pub markdown: String,
    pub range: Range,
    pub help_anchor: Option<String>,
}

/// Documentation for the property name or value at (line, col).
pub fn hover(text: &str, line: u32, col: u32) -> Option<Hover> {
    let idx = LineIndex::new(text);
    let off = idx.offset(line, col);
    let doc = parser::parse(text);
    let ctx = context::locate(text, &doc.root, off);
    let s = schema::schema();
    let level = ctx.innermost()?;
    let (name, span) = match &ctx.position {
        Position::Key { replace } if replace.end > replace.start => {
            (String::from_utf8_lossy(&text.as_bytes()[replace.start..replace.end]).trim_matches(|c| c == '"' || c == '\'').to_string(), *replace)
        }
        Position::Value { prop, replace } if replace.end > replace.start => (prop.name.clone(), *replace),
        _ => return None,
    };
    let sp = s.find_prop(level.schema_obj?, &name, Some(level.doc))?;
    let mut md = prop_doc(&name, sp);
    let path = ctx.path();
    if !path.is_empty() {
        md = format!("{md}\n\n_{path}.{name}_");
    }
    Some(Hover { markdown: md, range: idx.range(span), help_anchor: sp.link.clone() })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    /// Path of the innermost object, e.g. `buttons[2].source`.
    pub path: String,
    /// Property under the cursor, if any.
    pub property: Option<String>,
    /// Help anchor for the property (or innermost object) under the cursor.
    pub help_anchor: Option<String>,
}

/// Where the cursor is, for the breadcrumb bar and context help (F1).
pub fn breadcrumb(text: &str, line: u32, col: u32) -> Breadcrumb {
    let idx = LineIndex::new(text);
    let off = idx.offset(line, col);
    let doc = parser::parse(text);
    let ctx = context::locate(text, &doc.root, off);
    let s = schema::schema();
    let property = match &ctx.position {
        Position::Value { prop, .. } => Some(prop.name.clone()),
        Position::Key { replace } if replace.end > replace.start => {
            Some(String::from_utf8_lossy(&text.as_bytes()[replace.start..replace.end]).trim_matches('"').to_string())
        }
        _ => None,
    };
    let anchor = property
        .as_deref()
        .and_then(|p| context::prop_schema(s, &ctx, p))
        .and_then(|sp| sp.link.clone())
        .or_else(|| {
            // fall back to the innermost object's own property link
            let n = ctx.levels.len();
            if n < 2 {
                return None;
            }
            let parent = &ctx.levels[n - 2];
            let seg = &ctx.levels[n - 1].segment;
            let (parent, seg) = if seg.starts_with('[') && n >= 3 { (&ctx.levels[n - 3], &ctx.levels[n - 2].segment) } else { (parent, seg) };
            s.find_prop(parent.schema_obj?, seg, Some(parent.doc)).and_then(|sp| sp.link.clone())
        });
    Breadcrumb { path: ctx.path(), property, help_anchor: anchor }
}

/// Quick syntax-only check (used before saving to the device).
pub fn syntax_errors(text: &str) -> usize {
    parser::parse(text).errors.len()
}

#[allow(dead_code)]
fn _kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Undefined => "undefined",
        Kind::Null => "null",
        Kind::Bool(_) => "boolean",
        Kind::Number(_) => "number",
        Kind::String(_) => "string",
        Kind::Object(_) => "object",
        Kind::Array(_) => "array",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTER: &str = include_str!("../../../assets/templates/Starter.json");
    const DIY: &str = include_str!("../../../assets/templates/DIYExpansionBoard.json");
    const PRO: &str = include_str!("../../../assets/templates/ProExpansionBoard.json");
    const SAFE: &str = include_str!("../../../assets/templates/SafeModeStarter.json");
    const OPD: &str = include_str!("../../../assets/templates/OpenPinDevTester.json");

    #[test]
    fn upstream_templates_are_clean() {
        for (name, t) in [("Starter", STARTER), ("DIY", DIY), ("Pro", PRO), ("SafeMode", SAFE), ("OPD", OPD)] {
            let d = validate(t);
            assert!(d.is_empty(), "{name}: {:#?}", &d[..d.len().min(5)]);
        }
    }

    #[test]
    fn schema_warnings() {
        let t = "{ id: { unitNum: 1, bogus: 2 }, buttons: [ { source: { type: \"gpio\", gp: 5 }, action: { type: \"key\", key: \"a\" }, type: \"nope\" } ] }";
        let d = validate(t);
        let msgs: Vec<_> = d.iter().map(|d| d.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("\"bogus\" is not valid")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.starts_with("Value must be one of")), "{msgs:?}");
        assert!(d.iter().all(|d| d.severity == Severity::Warning));
    }

    #[test]
    fn subclass_props_are_known() {
        // gpio-specific props (gp, pull) are valid only because type = "gpio"
        let ok = validate("{ buttons: [ { source: { type: \"gpio\", gp: 5, pull: true }, action: { type: \"key\", key: \"a\" } } ] }");
        assert!(ok.is_empty(), "{ok:#?}");
        let bad = validate("{ buttons: [ { source: { type: \"pca9555\", gp: 5 }, action: { type: \"key\", key: \"a\" } } ] }");
        assert!(bad.iter().any(|d| d.message.contains("\"gp\" is not valid")), "{bad:#?}");
    }

    #[test]
    fn syntax_errors_block_rules_check() {
        let d = validate("{ id: { unitNum: 1 bogus: 2 } }");
        assert!(d.iter().all(|d| d.severity == Severity::Error));
        assert_eq!(d[0].message, "Expected ',' or '}'");
        assert_eq!((d[0].range.start_line, d[0].range.start_col), (1, 20));
    }

    fn at(text: &str, marker: &str) -> (String, u32, u32) {
        let off = text.find(marker).unwrap();
        let clean = text.replacen(marker, "", 1);
        let (l, c) = LineIndex::new(&clean).pos(off);
        (clean, l, c)
    }

    #[test]
    fn complete_top_level_keys() {
        let (t, l, c) = at("{\n  id: { unitNum: 1 },\n  |\n}", "|");
        let items = completions(&t, l, c);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"buttons") && labels.contains(&"outputs"), "{labels:?}");
        assert!(!labels.contains(&"id"), "already present");
        let b = items.iter().find(|i| i.label == "buttons").unwrap();
        assert!(b.insert_text.starts_with("buttons: [\n\t{"), "{}", b.insert_text);
    }

    #[test]
    fn complete_subclass_keys_and_values() {
        let (t, l, c) = at("{ buttons: [ { source: { type: \"gpio\", | } } ] }", "|");
        let labels: Vec<_> = completions(&t, l, c).into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"gp".to_string()) && labels.contains(&"pull".to_string()), "{labels:?}");

        let (t, l, c) = at("{ buttons: [ { source: { type: | } } ] }", "|");
        let labels: Vec<_> = completions(&t, l, c).into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"\"gpio\"".to_string()) && labels.contains(&"\"74hc165\"".to_string()), "{labels:?}");
    }

    #[test]
    fn complete_after_comment_line() {
        let (t, l, c) = at("{\n  // Unit identification\n  |\n  id: { unitNum: 1 },\n}", "|");
        let labels: Vec<_> = completions(&t, l, c).into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"buttons".to_string()), "{labels:?}");
        // and nothing inside the comment itself
        let (t, l, c) = at("{\n  // Unit ident|ification\n}", "|");
        assert!(completions(&t, l, c).is_empty());
    }

    #[test]
    fn complete_after_missing_comma() {
        let (t, l, c) = at("{ id: { unitNum: 1 }\n  but| }", "|");
        let labels: Vec<_> = completions(&t, l, c).into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"buttons".to_string()), "{labels:?}");
    }

    #[test]
    fn partial_key_replacement() {
        let (t, l, c) = at("{ id: { unitN| } }", "|");
        let items = completions(&t, l, c);
        let u = items.iter().find(|i| i.label == "unitNum").expect("unitNum offered");
        assert_eq!((u.range.start_col, u.range.end_col), (9, 14));
    }

    #[test]
    fn hover_docs() {
        let (t, l, c) = at("{ id: { un|itNum: 1 } }", "|");
        let h = hover(&t, l, c).expect("hover");
        assert!(h.markdown.contains("unitNum"), "{}", h.markdown);
        assert!(h.help_anchor.is_some());
    }

    #[test]
    fn breadcrumb_path() {
        let (t, l, c) = at("{ buttons: [ {}, { source: { type: \"gpio\", gp: |5 } } ] }", "|");
        let b = breadcrumb(&t, l, c);
        assert_eq!(b.path, "buttons[1].source");
        assert_eq!(b.property.as_deref(), Some("gp"));
        assert!(b.help_anchor.is_some());
    }

    #[test]
    fn line_index_line_breaks() {
        let t = "a\r\nb\rc\nd\n\re";
        let i = LineIndex::new(t);
        for (ch, line) in [('a', 1), ('b', 2), ('c', 3), ('d', 4), ('e', 6)] {
            assert_eq!(i.pos(t.find(ch).unwrap()), (line, 1), "{ch}");
            assert_eq!(i.offset(line, 1), t.find(ch).unwrap(), "{ch}");
        }
        // column past the end of a CRLF line clamps before the terminator
        assert_eq!(i.offset(1, 99), 1);
    }

    #[test]
    fn line_index_utf16() {
        let t = "a\n€x\n";
        let i = LineIndex::new(t);
        assert_eq!(i.pos(t.find('x').unwrap()), (2, 2));
        assert_eq!(i.offset(2, 2), t.find('x').unwrap());
    }
}
