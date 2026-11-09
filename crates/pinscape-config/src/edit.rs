//! Structural edits to config text that keep everything else intact.
//!
//! The form view in the app never re-serializes the config: each change
//! (set a property, remove it, add or duplicate an array entry) becomes a
//! small list of text replacements at parser spans, so comments, blank
//! lines, quoting and the rest of the document stay exactly as they were.
//! New text follows the surrounding style: indentation, line endings,
//! trailing commas and inline vs. multi-line objects.
//!
//! Every result is re-parsed before it's returned; an edit that would
//! produce a syntax error is refused instead.

use serde::{Deserialize, Serialize};

use crate::parser::{self, Kind, Span, Value};
use crate::schema::{self, Schema};
use crate::{LineIndex, Range};

/// One step of a path into the document: a property name or an array index.
/// Serialized as a JSON string or number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Seg {
    Index(usize),
    Key(String),
}

impl From<&str> for Seg {
    fn from(s: &str) -> Seg {
        Seg::Key(s.to_string())
    }
}
impl From<usize> for Seg {
    fn from(i: usize) -> Seg {
        Seg::Index(i)
    }
}

/// Display form of a path, e.g. `buttons[2].source`.
pub fn path_string(path: &[Seg]) -> String {
    let mut s = String::new();
    for seg in path {
        match seg {
            Seg::Key(k) => {
                if !s.is_empty() {
                    s.push('.');
                }
                s.push_str(k);
            }
            Seg::Index(i) => s.push_str(&format!("[{i}]")),
        }
    }
    s
}

/// A form edit.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum EditOp {
    /// Set a property (adding it if missing) or replace an array item, to a
    /// scalar: a string is written as a quoted string literal, a number or
    /// boolean as itself.
    Set { path: Vec<Seg>, value: serde_json::Value },
    /// Like `Set`, with the value given as pinscape-json source text
    /// (e.g. `0x20`, `[1, 2]`, `"GP5"`). Must parse as a single value.
    #[serde(rename_all = "camelCase")]
    SetRaw { path: Vec<Seg>, text: String },
    /// Add an empty object (`object: true`) or array at `path`.
    Create { path: Vec<Seg>, object: bool },
    /// Remove a property or array item.
    Remove { path: Vec<Seg> },
    /// Append a new entry to the array at `path` (created if missing),
    /// pre-filled from the schema (e.g. a new button gets a type, a source
    /// and an action).
    AddItem { path: Vec<Seg> },
    /// Insert a copy of the array item at `path` right after it.
    Duplicate { path: Vec<Seg> },
}

/// A replacement in editor coordinates (relative to the text before the edit).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditResult {
    /// Non-overlapping replacements, in document order.
    pub edits: Vec<TextEdit>,
    /// The whole text after the edits.
    pub text: String,
}

/// Compute the text edits for `op`. Fails (without changes) if the document
/// has syntax errors, the path doesn't exist, or the result wouldn't parse.
pub fn apply(text: &str, op: &EditOp) -> Result<EditResult, String> {
    let doc = parser::parse(text);
    if let Some(e) = doc.errors.first() {
        return Err(format!("The config has syntax errors ({}); fix them in the text view first.", e.message));
    }
    if doc.unterminated_comment.is_some() {
        return Err("The config has an unterminated /* comment; fix it in the text view first.".into());
    }
    let cx = Cx::new(text, &doc.root);
    let mut raw = match op {
        EditOp::Set { path, value } => {
            let lit = match value {
                // keep single quotes if the old value used them
                serde_json::Value::String(s) => match cx.resolve(path) {
                    Ok(old) if matches!(old.kind, Kind::String(_)) && text.as_bytes().get(old.span.start) == Some(&b'\'') => quote(s, '\''),
                    _ => quote(s, '"'),
                },
                v => scalar_literal(v)?,
            };
            cx.set(path, Lit::Raw(lit))?
        }
        EditOp::SetRaw { path, text: t } => {
            let t = t.trim();
            check_value_text(t)?;
            cx.set(path, Lit::Raw(t.to_string()))?
        }
        EditOp::Create { path, object } => {
            let multiline = path.len() == 1;
            let lit = if *object { Lit::Obj(vec![], multiline) } else { Lit::Arr(vec![], multiline) };
            cx.set(path, lit)?
        }
        EditOp::Remove { path } => cx.remove(path)?,
        EditOp::AddItem { path } => cx.add_item(path)?,
        EditOp::Duplicate { path } => cx.duplicate(path)?,
    };
    raw.sort_by_key(|(s, _)| (s.start, s.end));
    // merge inserts at the same spot (e.g. a comma and the new element after it)
    raw.dedup_by(|b, a| {
        if a.0.start == a.0.end && b.0 == a.0 {
            a.1.push_str(&b.1);
            true
        } else {
            false
        }
    });
    for w in raw.windows(2) {
        if w[1].0.start < w[0].0.end {
            return Err("internal error: overlapping edits".into());
        }
    }
    let mut out = String::with_capacity(text.len() + 64);
    let mut at = 0;
    for (s, t) in &raw {
        out.push_str(&text[at..s.start]);
        out.push_str(t);
        at = s.end;
    }
    out.push_str(&text[at..]);
    let after = parser::parse(&out);
    if let Some(e) = after.errors.first() {
        return Err(format!("That change would make the config invalid ({}).", e.message));
    }
    let idx = LineIndex::new(text);
    let edits = raw.into_iter().map(|(s, t)| TextEdit { range: idx.range(s), text: t }).collect();
    Ok(EditResult { edits, text: out })
}

/// A pinscape-json literal for a JSON scalar.
fn scalar_literal(v: &serde_json::Value) -> Result<String, String> {
    Ok(match v {
        serde_json::Value::String(s) => quote(s, '"'),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".into(),
        _ => return Err("only strings, numbers and booleans can be set this way".into()),
    })
}

/// Quote a string for pinscape-json, escaping what the parser needs escaped.
pub fn quote(s: &str, q: char) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c == q => {
                o.push('\\');
                o.push(c);
            }
            c if (c as u32) < 0x20 => o.push_str(&format!("\\x{:02X}", c as u32)),
            c => o.push(c),
        }
    }
    o.push(q);
    o
}

/// Check that `t` is exactly one value (no comments needed or allowed around it).
fn check_value_text(t: &str) -> Result<(), String> {
    if t.is_empty() {
        return Err("Enter a value.".into());
    }
    let d = parser::parse(t);
    if let Some(e) = d.errors.first() {
        return Err(format!("Not a valid value: {}", e.message));
    }
    if has_comment(t) {
        return Err("Comments can't be added here; use the text view.".into());
    }
    Ok(())
}

/// `//` or `/*` outside string literals.
fn has_comment(t: &str) -> bool {
    let b = t.as_bytes();
    let mut q: Option<u8> = None;
    let mut i = 0;
    while i < b.len() {
        match (q, b[i]) {
            (Some(_), b'\\') => i += 1,
            (Some(c), x) if x == c => q = None,
            (None, b'"' | b'\'') => q = Some(b[i]),
            (None, b'/') if matches!(b.get(i + 1), Some(b'/') | Some(b'*')) => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Property names that can be written without quotes.
fn key_text(name: &str) -> String {
    let mut chars = name.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !matches!(name, "true" | "false" | "null" | "undefined");
    if ok {
        name.to_string()
    } else {
        quote(name, '"')
    }
}

/// A value to be written as new text.
#[derive(Debug, Clone)]
enum Lit {
    /// Source text of a scalar (or any value, written verbatim).
    Raw(String),
    /// An object; `true` = always multi-line (else inline when short and flat).
    Obj(Vec<(String, Lit)>, bool),
    /// An array; `true` = always multi-line.
    Arr(Vec<Lit>, bool),
}

/// Formatting conventions detected from the document.
struct Style {
    eol: &'static str,
    unit: String,
    /// Multi-line containers end with a trailing comma.
    trailing: bool,
}

struct Cx<'a> {
    text: &'a str,
    root: &'a Value,
    style: Style,
}

type Edits = Vec<(Span, String)>;

/// Where a container element lives.
struct Elem {
    /// Start of the element (property name, or item value).
    start: usize,
    /// End of the value.
    end: usize,
}

impl<'a> Cx<'a> {
    fn new(text: &'a str, root: &'a Value) -> Self {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut cx = Cx { text, root, style: Style { eol, unit: "    ".into(), trailing: true } };
        // Indent unit: from the root object's first property, if it's on its own line.
        if let Some(p) = root.as_object().and_then(|p| p.first()) {
            let ind = cx.indent_at(p.name_span.start);
            if cx.only_space_before(p.name_span.start) && !ind.is_empty() {
                cx.style.unit = ind;
            }
        }
        let (mut with, mut without) = (0, 0);
        cx.count_trailing(root, &mut with, &mut without);
        cx.style.trailing = with >= without;
        cx
    }

    // ---- text helpers ----

    fn line_start(&self, off: usize) -> usize {
        // `off` on the `\n` of a `\r\n` belongs to the line that break ends
        // (callers step back from a line start with `line_start(s - 1)`).
        let b = self.text.as_bytes();
        let off = if off > 0 && b.get(off) == Some(&b'\n') && b[off - 1] == b'\r' { off - 1 } else { off };
        self.text[..off].rfind(['\n', '\r']).map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, off: usize) -> usize {
        self.text[off..].find(['\n', '\r']).map(|i| off + i).unwrap_or(self.text.len())
    }

    /// Start of the next line (just past the terminator), or end of text.
    fn next_line_start(&self, off: usize) -> usize {
        let e = self.line_end(off);
        let b = self.text.as_bytes();
        if e >= b.len() {
            return e;
        }
        if b[e] == b'\r' && b.get(e + 1) == Some(&b'\n') {
            e + 2
        } else {
            e + 1
        }
    }

    /// Leading whitespace of the line containing `off`.
    fn indent_at(&self, off: usize) -> String {
        let ls = self.line_start(off);
        self.text[ls..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    fn only_space_before(&self, off: usize) -> bool {
        self.text[self.line_start(off)..off].chars().all(|c| c == ' ' || c == '\t')
    }

    /// Rest of the line from `off` is blank or a `//` comment.
    fn rest_is_trivia(&self, off: usize) -> bool {
        let rest = self.text[off..self.line_end(off)].trim_start_matches([' ', '\t']);
        rest.is_empty() || rest.starts_with("//")
    }

    fn line_is_blank(&self, line_start: usize) -> bool {
        self.text[line_start..self.line_end(line_start)].trim().is_empty()
    }

    /// Skip whitespace and comments from `off`.
    fn skip_trivia(&self, mut off: usize) -> usize {
        let b = self.text.as_bytes();
        loop {
            while off < b.len() && parser_space(b[off]) {
                off += 1;
            }
            if b.get(off) == Some(&b'/') && b.get(off + 1) == Some(&b'/') {
                off = self.line_end(off);
                continue;
            }
            if b.get(off) == Some(&b'/') && b.get(off + 1) == Some(&b'*') {
                off = self.text[off + 2..].find("*/").map(|i| off + 2 + i + 2).unwrap_or(b.len());
                continue;
            }
            return off;
        }
    }

    /// Position of the comma following a value ending at `end`, if any.
    fn comma_after(&self, end: usize) -> Option<usize> {
        let p = self.skip_trivia(end);
        (self.text.as_bytes().get(p) == Some(&b',')).then_some(p)
    }

    fn count_trailing(&self, v: &Value, with: &mut usize, without: &mut usize) {
        let ends: Vec<(usize, usize)> = match &v.kind {
            Kind::Object(props) => {
                props.iter().for_each(|p| self.count_trailing(&p.value, with, without));
                props.first().map(|f| (f.name_span.start, props.last().unwrap().value.span.end)).into_iter().collect()
            }
            Kind::Array(items) => {
                items.iter().for_each(|i| self.count_trailing(i, with, without));
                items.first().map(|f| (f.span.start, items.last().unwrap().span.end)).into_iter().collect()
            }
            _ => vec![],
        };
        for (first, last_end) in ends {
            if self.text[v.span.start..first].contains(['\n', '\r']) {
                if self.comma_after(last_end).is_some() {
                    *with += 1;
                } else {
                    *without += 1;
                }
            }
        }
    }

    // ---- rendering ----

    fn render(&self, lit: &Lit, indent: &str) -> String {
        let st = &self.style;
        let inner = format!("{indent}{}", st.unit);
        match lit {
            Lit::Raw(s) => s.clone(),
            Lit::Obj(props, multiline) => {
                if props.is_empty() {
                    return if *multiline { format!("{{{}{indent}}}", st.eol) } else { "{}".into() };
                }
                let flat = props.iter().all(|(_, v)| matches!(v, Lit::Raw(_)));
                if flat && !multiline {
                    let body: Vec<String> = props.iter().map(|(k, v)| format!("{}: {}", key_text(k), self.render(v, ""))).collect();
                    let one = format!("{{ {} }}", body.join(", "));
                    if one.len() <= 72 {
                        return one;
                    }
                }
                let mut s = String::from("{");
                for (i, (k, v)) in props.iter().enumerate() {
                    let comma = if i + 1 < props.len() || st.trailing { "," } else { "" };
                    s.push_str(&format!("{}{inner}{}: {}{comma}", st.eol, key_text(k), self.render(v, &inner)));
                }
                s.push_str(&format!("{}{indent}}}", st.eol));
                s
            }
            Lit::Arr(items, multiline) => {
                if items.is_empty() {
                    return if *multiline { format!("[{}{indent}]", st.eol) } else { "[]".into() };
                }
                if !multiline && items.iter().all(|v| matches!(v, Lit::Raw(_))) {
                    let body: Vec<String> = items.iter().map(|v| self.render(v, "")).collect();
                    return format!("[{}]", body.join(", "));
                }
                let mut s = String::from("[");
                for (i, v) in items.iter().enumerate() {
                    let comma = if i + 1 < items.len() || st.trailing { "," } else { "" };
                    s.push_str(&format!("{}{inner}{}{comma}", st.eol, self.render(v, &inner)));
                }
                s.push_str(&format!("{}{indent}]", st.eol));
                s
            }
        }
    }

    // ---- navigation ----

    fn resolve(&self, path: &[Seg]) -> Result<&'a Value, String> {
        let mut v = self.root;
        for (i, seg) in path.iter().enumerate() {
            let next = match (seg, &v.kind) {
                (Seg::Key(k), Kind::Object(_)) => v.get(k),
                (Seg::Index(n), Kind::Array(items)) => items.get(*n),
                _ => None,
            };
            v = next.ok_or_else(|| format!("{} doesn't exist in the config", path_string(&path[..=i])))?;
        }
        Ok(v)
    }

    /// Elements of a container, with their spans.
    fn elems(v: &Value) -> Vec<Elem> {
        match &v.kind {
            Kind::Object(props) => props.iter().map(|p| Elem { start: p.name_span.start, end: p.value.span.end }).collect(),
            Kind::Array(items) => items.iter().map(|i| Elem { start: i.span.start, end: i.span.end }).collect(),
            _ => vec![],
        }
    }

    /// Edits that insert `elem_text` (a property `key: value` or an item,
    /// rendered by `make` for a given indent) after element `after` of
    /// `container` (`None` = at the end).
    fn insert(&self, container: &Value, after: Option<usize>, make: &dyn Fn(&str) -> String) -> Result<Edits, String> {
        let text = self.text;
        let elems = Self::elems(container);
        let open = container.span.start;
        let close = container.span.end.saturating_sub(1);
        if !matches!(text.as_bytes().get(close), Some(b'}') | Some(b']')) {
            return Err("internal error: container isn't closed".into());
        }
        let eol = self.style.eol;
        if elems.is_empty() {
            let inner = &text[open + 1..close];
            let base = self.indent_at(open);
            let child = format!("{base}{}", self.style.unit);
            let item = make(&child);
            let tc = if self.style.trailing { "," } else { "" };
            if inner.trim().is_empty() {
                if inner.contains(['\n', '\r']) || item.contains('\n') || matches!(container.kind, Kind::Array(_)) && item.starts_with('{') {
                    return Ok(vec![(Span::new(open + 1, close), format!("{eol}{child}{item}{tc}{eol}{base}"))]);
                }
                let item = make("");
                return Ok(vec![(Span::new(open + 1, close), format!(" {item} "))]);
            }
            // only comments inside: put the new element first, on its own line
            return Ok(vec![(Span::new(open + 1, open + 1), format!("{eol}{child}{item}{tc}"))]);
        }
        let i = after.unwrap_or(elems.len() - 1).min(elems.len() - 1);
        let last = i == elems.len() - 1;
        let el = &elems[i];
        let comma = self.comma_after(el.end);
        let multiline = text[open..elems[0].start].contains(['\n', '\r']);
        let mut edits: Edits = vec![];
        if comma.is_none() {
            edits.push((Span::new(el.end, el.end), ",".into()));
        }
        let after_comma = comma.map(|c| c + 1).unwrap_or(el.end);
        // keep this container's own trailing-comma habit; mid-list needs one
        let tc = if !last || comma.is_some() { "," } else { "" };
        if multiline {
            let indent = self.indent_at(el.start);
            let item = make(&indent);
            // blank line between entries? keep doing that
            let gap = if i >= 1 {
                let between = &text[elems[i - 1].end..el.start];
                has_blank_line(between)
            } else if !last {
                has_blank_line(&text[el.end..elems[i + 1].start])
            } else {
                false
            };
            let blank = if gap { eol } else { "" };
            if self.rest_is_trivia(after_comma) {
                let at = self.line_end(after_comma);
                edits.push((Span::new(at, at), format!("{eol}{blank}{indent}{item}{tc}")));
            } else {
                edits.push((Span::new(after_comma, after_comma), format!("{eol}{blank}{indent}{item}{tc}")));
            }
        } else {
            // inline: `{ a: 1 }` -> `{ a: 1, b: 2 }`; `{ a: 1, }` -> `{ a: 1, b: 2, }`
            let item = make("");
            let tc = if comma.is_some() { "," } else { "" };
            edits.push((Span::new(after_comma, after_comma), format!(" {item}{tc}")));
        }
        Ok(edits)
    }

    // ---- operations ----

    fn set(&self, path: &[Seg], lit: Lit) -> Result<Edits, String> {
        let (last, parent_path) = path.split_last().ok_or("empty path")?;
        let parent = self.resolve(parent_path)?;
        match (last, &parent.kind) {
            (Seg::Key(k), Kind::Object(props)) => {
                if let Some(p) = props.iter().rev().find(|p| &p.name == k) {
                    let indent = self.indent_at(p.name_span.start);
                    return Ok(vec![(p.value.span, self.render(&lit, &indent))]);
                }
                let key = key_text(k);
                self.insert(parent, None, &|ind: &str| format!("{key}: {}", self.render(&lit, ind)))
            }
            (Seg::Index(n), Kind::Array(items)) => {
                let item = items.get(*n).ok_or_else(|| format!("{} doesn't exist in the config", path_string(path)))?;
                let indent = self.indent_at(item.span.start);
                Ok(vec![(item.span, self.render(&lit, &indent))])
            }
            _ => Err(format!("{} isn't an object or array", if parent_path.is_empty() { "The top level".into() } else { path_string(parent_path) })),
        }
    }

    fn remove(&self, path: &[Seg]) -> Result<Edits, String> {
        let (last, parent_path) = path.split_last().ok_or("empty path")?;
        let parent = self.resolve(parent_path)?;
        let elems = Self::elems(parent);
        let idx = match (last, &parent.kind) {
            (Seg::Key(k), Kind::Object(props)) => props.iter().rposition(|p| &p.name == k),
            (Seg::Index(n), Kind::Array(items)) => (*n < items.len()).then_some(*n),
            _ => None,
        }
        .ok_or_else(|| format!("{} doesn't exist in the config", path_string(path)))?;
        let el = &elems[idx];
        let end = self.comma_after(el.end).map(|c| c + 1).unwrap_or(el.end);
        let text = self.text;
        if self.only_space_before(el.start) && self.rest_is_trivia(end) {
            // whole lines
            let mut s = self.line_start(el.start);
            let mut e = self.next_line_start(end);
            if matches!(parent.kind, Kind::Array(_)) {
                // a list entry's label: `//` lines directly above it go with it
                while s > 0 {
                    let ps = self.line_start(s - 1);
                    if !self.text[ps..self.line_end(ps)].trim_start().starts_with("//") {
                        break;
                    }
                    s = ps;
                }
            }
            let prev_line = (s > 0).then(|| self.line_start(s - 1));
            let prev_blank = prev_line.is_some_and(|p| self.line_is_blank(p));
            let after_open =
                prev_line.is_some_and(|p| self.text[p..self.line_end(p)].trim_end().ends_with(['[', '{']));
            if after_open && e < text.len() && self.line_is_blank(e) {
                e = self.next_line_start(e);
            } else if prev_blank {
                if e < text.len() && self.line_is_blank(e) {
                    e = self.next_line_start(e);
                } else if matches!(text.as_bytes().get(self.skip_trivia(e)), Some(b'}') | Some(b']'))
                    && text[e..self.skip_trivia(e)].trim().is_empty()
                {
                    s = self.line_start(s - 1);
                }
            }
            return Ok(vec![(Span::new(s, e), String::new())]);
        }
        // inline: drop the element, its comma and the spaces after it
        let mut e = end;
        while matches!(text.as_bytes().get(e), Some(b' ') | Some(b'\t')) {
            e += 1;
        }
        let mut s = el.start;
        if end == el.end && idx > 0 {
            // last element: `{ a: 1, b: 2 }` -> `{ a: 1 }` (if no comment in between)
            let prev = elems[idx - 1].end;
            if text[prev..el.start].trim() == "," {
                return Ok(vec![(Span::new(prev, el.end), String::new())]);
            }
        }
        if e < text.len() && matches!(text.as_bytes()[e], b'}' | b']') {
            // last one before the closing delimiter: keep one space before it
            while s > 0 && matches!(text.as_bytes()[s - 1], b' ' | b'\t') {
                s -= 1;
            }
            if s < el.start {
                return Ok(vec![(Span::new(s, e), " ".into())]);
            }
        }
        Ok(vec![(Span::new(s, e), String::new())])
    }

    fn add_item(&self, path: &[Seg]) -> Result<Edits, String> {
        let s = schema::schema();
        let sub = schema_for_path(s, self.root, path).and_then(|sp| sp.sub_obj());
        let item = skeleton(s, path, sub);
        match self.resolve(path) {
            Ok(arr) if matches!(arr.kind, Kind::Array(_)) => self.insert(arr, None, &|ind: &str| self.render(&item, ind)),
            Ok(_) => Err(format!("{} isn't a list", path_string(path))),
            Err(_) => self.set(path, Lit::Arr(vec![item], true)),
        }
    }

    fn duplicate(&self, path: &[Seg]) -> Result<Edits, String> {
        let (Seg::Index(n), parent_path) = path.split_last().ok_or("empty path")? else {
            return Err("only list entries can be duplicated".into());
        };
        let arr = self.resolve(parent_path)?;
        let Kind::Array(items) = &arr.kind else { return Err(format!("{} isn't a list", path_string(parent_path))) };
        let item = items.get(*n).ok_or_else(|| format!("{} doesn't exist in the config", path_string(path)))?;
        let copy = self.text[item.span.start..item.span.end].to_string();
        self.insert(arr, Some(*n), &|_| copy.clone())
    }
}

fn parser_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// Text between two elements contains an empty (whitespace-only) line.
fn has_blank_line(between: &str) -> bool {
    let lines: Vec<&str> = between.split('\n').collect();
    lines.len() > 2 && lines[1..lines.len() - 1].iter().any(|l| l.trim().is_empty())
}

/// The schema property describing the value at `path` (an array or object).
fn schema_for_path<'s>(s: &'s Schema, root: &Value, path: &[Seg]) -> Option<&'s schema::SchemaProp> {
    let mut obj = s.root;
    let mut doc = Some(root);
    let mut prop: Option<&schema::SchemaProp> = None;
    for seg in path {
        match seg {
            Seg::Key(k) => {
                let sp = s.find_prop(obj, k, doc)?;
                prop = Some(sp);
                doc = doc.and_then(|d| d.get(k));
                if let Some(o) = sp.sub_obj() {
                    obj = o;
                }
            }
            Seg::Index(i) => {
                doc = doc.and_then(|d| match &d.kind {
                    Kind::Array(items) => items.get(*i),
                    _ => None,
                });
            }
        }
    }
    prop
}

/// A new array entry: well-known starting points for buttons and outputs,
/// else the schema's required properties.
fn skeleton(s: &Schema, path: &[Seg], sub: Option<usize>) -> Lit {
    let raw = |t: &str| Lit::Raw(t.to_string());
    let q = |t: &str| Lit::Raw(quote(t, '"'));
    if path.len() == 1 {
        match &path[0] {
            Seg::Key(k) if k == "buttons" => {
                return Lit::Obj(
                    vec![
                        ("type".into(), q("push")),
                        ("source".into(), Lit::Obj(vec![("type".into(), q("gpio"))], false)),
                        ("action".into(), Lit::Obj(vec![("type".into(), q("key"))], false)),
                    ],
                    true,
                )
            }
            Seg::Key(k) if k == "outputs" => {
                return Lit::Obj(vec![("device".into(), Lit::Obj(vec![("type".into(), q("gpio"))], false))], true)
            }
            _ => {}
        }
    }
    let Some(o) = sub else { return Lit::Obj(vec![], false) };
    let so = s.obj(o);
    let mut props: Vec<(String, Lit)> = vec![];
    if let Some(sel) = &so.subclass_selector {
        if let Some(first) = so.subclasses.keys().next() {
            props.push((sel.clone(), q(first)));
        }
    }
    for (name, sp) in &so.props {
        if !sp.required || props.iter().any(|(n, _)| n == name) {
            continue;
        }
        let v = if let Some(e) = sp.enum_values.first() {
            if sp.accepts("string") { q(e) } else { raw(e) }
        } else if sp.accepts("number") {
            raw("0")
        } else if sp.accepts("boolean") {
            raw("false")
        } else if sp.accepts("string") {
            q("")
        } else if sp.accepts("object") {
            Lit::Obj(vec![], false)
        } else {
            raw("0")
        };
        props.push((name.clone(), v));
    }
    let multi = props.len() > 2;
    Lit::Obj(props, multi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(segs: &[Seg]) -> Vec<Seg> {
        segs.to_vec()
    }
    fn k(s: &str) -> Seg {
        Seg::Key(s.into())
    }
    fn run(text: &str, op: EditOp) -> String {
        let r = apply(text, &op).unwrap_or_else(|e| panic!("{e}"));
        // applying the returned ranges gives the returned text
        let idx = LineIndex::new(text);
        let mut out = text.to_string();
        for e in r.edits.iter().rev() {
            let s = idx.offset(e.range.start_line, e.range.start_col);
            let en = idx.offset(e.range.end_line, e.range.end_col);
            out.replace_range(s..en, &e.text);
        }
        assert_eq!(out, r.text);
        assert!(parser::parse(&r.text).errors.is_empty());
        r.text
    }
    fn set(path: &[Seg], v: serde_json::Value) -> EditOp {
        EditOp::Set { path: p(path), value: v }
    }

    const DOC: &str = "// My config\n{\n    // unit\n    id: { unitNum: 1, unitName: 'Pin' }, // trailing\n\n    /* buttons */\n    buttons: [\n        // start\n        {\n            source: { type: \"gpio\", gp: 7 },\n            action: { type: \"key\", key: \"enter\" },\n        },\n\n        // exit\n        {\n            source: { type: \"gpio\", gp: 8 },\n            action: { type: \"key\", key: \"esc\" },\n        },\n    ],\n}\n";

    #[test]
    fn replace_scalar_keeps_everything_else() {
        let t = run(DOC, set(&[k("id"), k("unitNum")], serde_json::json!(3)));
        assert_eq!(t, DOC.replace("unitNum: 1", "unitNum: 3"));
    }

    #[test]
    fn single_quotes_are_kept() {
        let t = run(DOC, set(&[k("id"), k("unitName"), ], serde_json::json!("It's")));
        assert!(t.contains("unitName: 'It\\'s' }"), "{t}");
    }

    #[test]
    fn nested_set_in_array_item() {
        let t = run(DOC, set(&[k("buttons"), Seg::Index(1), k("source"), k("gp")], serde_json::json!(12)));
        assert_eq!(t, DOC.replace("gp: 8", "gp: 12"));
    }

    #[test]
    fn add_prop_to_inline_object() {
        let t = run(DOC, set(&[k("buttons"), Seg::Index(0), k("source"), k("pull")], serde_json::json!(true)));
        assert!(t.contains("source: { type: \"gpio\", gp: 7, pull: true },"), "{t}");
        let t = run("{ a: { x: 1, } }", set(&[k("a"), k("y")], serde_json::json!(2)));
        assert_eq!(t, "{ a: { x: 1, y: 2, } }");
    }

    #[test]
    fn add_prop_to_multiline_object() {
        let t = run(DOC, set(&[k("buttons"), Seg::Index(0), k("name")], serde_json::json!("Start")));
        assert!(
            t.contains("            action: { type: \"key\", key: \"enter\" },\n            name: \"Start\",\n        },"),
            "{t}"
        );
        // top level, after a trailing comment, keeps the comment on its line
        let t = run("{\n  a: 1 // one\n}", set(&[k("b")], serde_json::json!(2)));
        assert_eq!(t, "{\n  a: 1, // one\n  b: 2\n}");
    }

    #[test]
    fn add_to_empty_objects() {
        assert_eq!(run("{ a: {} }", set(&[k("a"), k("x")], serde_json::json!(1))), "{ a: { x: 1 } }");
        assert_eq!(run("{\n    a: {\n    },\n}", set(&[k("a"), k("x")], serde_json::json!(1))), "{\n    a: {\n        x: 1,\n    },\n}");
        assert_eq!(run("{}", set(&[k("id")], serde_json::json!(1))), "{ id: 1 }");
    }

    #[test]
    fn quoted_keys_when_needed() {
        let t = run("{\n    a: 1,\n}", EditOp::Create { path: p(&[k("74hc165")]), object: true });
        assert_eq!(t, "{\n    a: 1,\n    \"74hc165\": {\n    },\n}");
    }

    #[test]
    fn remove_whole_lines_and_inline() {
        let t = run(DOC, EditOp::Remove { path: p(&[k("buttons"), Seg::Index(0), k("action")]) });
        assert!(!t.contains("enter"));
        assert!(t.contains("            source: { type: \"gpio\", gp: 7 },\n        },"), "{t}");
        let t = run(DOC, EditOp::Remove { path: p(&[k("id"), k("unitName")]) });
        assert!(t.contains("id: { unitNum: 1 }, // trailing"), "{t}");
        let t = run(DOC, EditOp::Remove { path: p(&[k("id"), k("unitNum")]) });
        assert!(t.contains("id: { unitName: 'Pin' }, // trailing"), "{t}");
        // comments elsewhere survive
        for c in ["// My config", "// unit", "/* buttons */", "// start", "// exit"] {
            assert!(t.contains(c));
        }
    }

    #[test]
    fn remove_array_item_and_blank_line() {
        let t = run(DOC, EditOp::Remove { path: p(&[k("buttons"), Seg::Index(1)]) });
        assert!(!t.contains("esc"));
        // the entry's label comment goes with it; other comments stay
        assert!(t.contains("// start") && !t.contains("// exit"), "{t}");
        assert!(t.ends_with("            action: { type: \"key\", key: \"enter\" },\n        },\n    ],\n}\n"), "{t}");
        let t = run(DOC, EditOp::Remove { path: p(&[k("buttons"), Seg::Index(0)]) });
        assert!(!t.contains("enter") && t.contains("esc"));
        assert!(!t.contains("// start") && t.contains("// exit") && t.contains("/* buttons */"), "{t}");
        assert!(t.contains("    buttons: [\n        // exit\n        {\n"), "{t}");
    }

    #[test]
    fn add_button_matches_style() {
        let t = run(DOC, EditOp::AddItem { path: p(&[k("buttons")]) });
        let tail = "            action: { type: \"key\", key: \"esc\" },\n        },\n\n        {\n            type: \"push\",\n            source: { type: \"gpio\" },\n            action: { type: \"key\" },\n        },\n    ],\n}\n";
        assert!(t.ends_with(tail), "{t}");
    }

    #[test]
    fn add_first_button_creates_list() {
        let t = run("{\n    id: { unitNum: 1 },\n}\n", EditOp::AddItem { path: p(&[k("buttons")]) });
        assert_eq!(
            t,
            "{\n    id: { unitNum: 1 },\n    buttons: [\n        {\n            type: \"push\",\n            source: { type: \"gpio\" },\n            action: { type: \"key\" },\n        },\n    ],\n}\n"
        );
        // and into an empty list
        let t = run("{\n    outputs: [],\n}", EditOp::AddItem { path: p(&[k("outputs")]) });
        assert_eq!(t, "{\n    outputs: [\n        {\n            device: { type: \"gpio\" },\n        },\n    ],\n}");
    }

    #[test]
    fn duplicate_item() {
        let t = run(DOC, EditOp::Duplicate { path: p(&[k("buttons"), Seg::Index(0)]) });
        assert_eq!(t.matches("key: \"enter\"").count(), 2);
        let i1 = t.find("key: \"enter\"").unwrap();
        let i2 = t.rfind("key: \"enter\"").unwrap();
        let ie = t.find("esc").unwrap();
        assert!(i1 < i2 && i2 < ie, "copy goes right after the original");
    }

    #[test]
    fn crlf_and_tabs() {
        let d = "{\r\n\tbuttons: [\r\n\t\t{ type: \"push\" }\r\n\t]\r\n}\r\n";
        let t = run(d, EditOp::Duplicate { path: p(&[k("buttons"), Seg::Index(0)]) });
        assert_eq!(t, "{\r\n\tbuttons: [\r\n\t\t{ type: \"push\" },\r\n\t\t{ type: \"push\" }\r\n\t]\r\n}\r\n");
        let t = run(d, set(&[k("id")], serde_json::json!("x")));
        assert_eq!(t, "{\r\n\tbuttons: [\r\n\t\t{ type: \"push\" }\r\n\t],\r\n\tid: \"x\"\r\n}\r\n");
    }

    #[test]
    fn raw_values() {
        let t = run(DOC, EditOp::SetRaw { path: p(&[k("id"), k("unitNum")]), text: " 0x10 ".into() });
        assert!(t.contains("unitNum: 0x10,"));
        assert!(apply(DOC, &EditOp::SetRaw { path: p(&[k("id"), k("unitNum")]), text: "1 2".into() }).is_err());
        assert!(apply(DOC, &EditOp::SetRaw { path: p(&[k("id"), k("unitNum")]), text: "".into() }).is_err());
    }

    #[test]
    fn refuses_bad_input() {
        assert!(apply("{ a: 1 b: 2 }", &set(&[k("a")], serde_json::json!(2))).is_err());
        assert!(apply(DOC, &set(&[k("nope"), k("x")], serde_json::json!(2))).is_err());
        assert!(apply(DOC, &EditOp::Remove { path: p(&[k("buttons"), Seg::Index(9)]) }).is_err());
    }

    #[test]
    fn templates_round_trip() {
        // every edit on the real templates keeps the rest of the text verbatim
        // Both line-ending styles on every platform: git gives Windows CI CRLF checkouts.
        let templates = [include_str!("../../../assets/templates/DIYExpansionBoard.json"), include_str!("../../../assets/templates/ProExpansionBoard.json")];
        for lf in templates.map(|t| t.replace("\r\n", "\n")) {
            for t in [lf.clone(), lf.replace('\n', "\r\n")] {
            let t = t.as_str();
            let a = run(t, EditOp::AddItem { path: p(&[k("outputs")]) });
            let b = run(&a, EditOp::Remove { path: p(&[k("outputs"), Seg::Index(count(&a, "outputs") - 1)]) });
            assert_eq!(b, t, "add + remove of an output restores the original text");
            let c = run(t, set(&[k("outputs"), Seg::Index(0), k("name")], serde_json::json!("Flipper")));
            assert!(crate::validate(&c).is_empty(), "{:?}", crate::validate(&c));
            }
        }
    }

    #[test]
    fn ops_from_ui_json() {
        // the shapes ui/src/lib/api-configform.ts sends
        let op: EditOp = serde_json::from_str(r#"{"op":"setRaw","path":["buttons",0,"source","gp"],"text":"9"}"#).unwrap();
        assert!(matches!(&op, EditOp::SetRaw { path, .. } if path == &vec![k("buttons"), Seg::Index(0), k("source"), k("gp")]));
        assert!(run(DOC, op).contains("gp: 9 }"));
        for j in [
            r#"{"op":"set","path":["id","unitName"],"value":"x"}"#,
            r#"{"op":"set","path":["id","unitNum"],"value":2}"#,
            r#"{"op":"create","path":["plunger"],"object":true}"#,
            r#"{"op":"remove","path":["id"]}"#,
            r#"{"op":"addItem","path":["outputs"]}"#,
            r#"{"op":"duplicate","path":["buttons",1]}"#,
        ] {
            let op: EditOp = serde_json::from_str(j).unwrap_or_else(|e| panic!("{j}: {e}"));
            run(DOC, op);
        }
        let r = serde_json::to_value(apply(DOC, &EditOp::Remove { path: p(&[k("id")]) }).unwrap()).unwrap();
        assert!(r["edits"][0]["range"]["startLine"].is_number());
        let v = serde_json::to_value(crate::form::form_view(DOC, Some("plunger"))).unwrap();
        assert_eq!(v["field"]["canObject"], serde_json::json!(true));
    }

    fn count(t: &str, arr: &str) -> usize {
        match &parser::parse(t).root.get(arr).unwrap().kind {
            Kind::Array(a) => a.len(),
            _ => 0,
        }
    }
}
