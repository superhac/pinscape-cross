//! Cursor context for editor features: which schema object the cursor is
//! inside, whether it's at a property name or a value, and the JSON path.
//! Works on incomplete text (the parser recovers from errors).

use crate::parser::{Kind, Prop, Span, Value};
use crate::schema::{schema, Schema, SchemaProp};

/// One level of nesting from the root to the cursor.
#[derive(Debug, Clone)]
pub struct Level<'a> {
    /// The document object at this level.
    pub doc: &'a Value,
    /// Its schema object, if known.
    pub schema_obj: Option<usize>,
    /// Path segment that led here (`buttons`, `[2]`, ...); empty at the root.
    pub segment: String,
}

#[derive(Debug, Clone)]
pub enum Position<'a> {
    /// Cursor is on/at a property name (or where one can be typed).
    /// `replace` is the span of the partial name under the cursor.
    Key { replace: Span },
    /// Cursor is in the value of property `prop`.
    /// `replace` is the span of the scalar value under the cursor (empty if none).
    Value { prop: &'a Prop, replace: Span },
    /// Somewhere else (inside an array of scalars, punctuation, comments...).
    Other,
}

#[derive(Debug, Clone)]
pub struct Context<'a> {
    pub levels: Vec<Level<'a>>,
    pub position: Position<'a>,
}

impl Context<'_> {
    pub fn innermost(&self) -> Option<&Level<'_>> {
        self.levels.last()
    }

    /// `buttons[2].source` style path of the innermost object.
    pub fn path(&self) -> String {
        let mut out = String::new();
        for l in &self.levels {
            if l.segment.starts_with('[') || out.is_empty() {
                out.push_str(&l.segment);
            } else {
                out.push('.');
                out.push_str(&l.segment);
            }
        }
        out
    }
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// Is `off` inside a `//` line comment or `/* */` block comment?
fn in_comment(text: &[u8], off: usize) -> bool {
    let line_start = text[..off].iter().rposition(|&c| c == b'\n').map(|i| i + 1).unwrap_or(0);
    // crude but effective: a '//' earlier on the same line, not inside quotes
    let mut q: Option<u8> = None;
    let mut i = line_start;
    while i < off {
        let c = text[i];
        match q {
            Some(qc) if c == qc => q = None,
            Some(_) if c == b'\\' => i += 1,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => q = Some(c),
            None if c == b'/' && text.get(i + 1) == Some(&b'/') => return true,
            None => {}
        }
        i += 1;
    }
    // block comment: last '/*' before off with no '*/' between
    let before = &text[..off];
    match (find_last(before, b"/*"), find_last(before, b"*/")) {
        (Some(o), Some(c)) => o > c,
        (Some(_), None) => true,
        _ => false,
    }
}

fn find_last(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).rposition(|w| w == n)
}

/// Determine the cursor context at byte offset `off`.
pub fn locate<'a>(text: &str, root: &'a Value, off: usize) -> Context<'a> {
    let s = schema();
    let bytes = text.as_bytes();
    let mut levels = vec![Level { doc: root, schema_obj: Some(s.root), segment: String::new() }];
    if !matches!(root.kind, Kind::Object(_)) || !inside(root.span, off) || in_comment(bytes, off) {
        return Context { levels, position: Position::Other };
    }

    loop {
        let cur = levels.last().unwrap().clone();
        let props = cur.doc.as_object().unwrap_or(&[]);

        // On a property name?
        if let Some(p) = props.iter().find(|p| p.name_span.contains(off) && p.name_span.end > p.name_span.start) {
            return Context { levels, position: Position::Key { replace: p.name_span } };
        }

        // Inside a property's value?
        if let Some(p) = props.iter().find(|p| value_contains(&p.value, off)) {
            let sp = cur.schema_obj.and_then(|o| s.find_prop(o, &p.name, Some(cur.doc)));
            match &p.value.kind {
                Kind::Object(_) if inside(p.value.span, off) => {
                    levels.push(Level { doc: &p.value, schema_obj: sp.and_then(|sp| sp.sub_obj()), segment: p.name.clone() });
                    continue;
                }
                Kind::Array(items) if inside(p.value.span, off) => {
                    let elem_schema = sp.and_then(|sp| sp.sub_obj());
                    if let Some((i, item)) = items.iter().enumerate().find(|(_, v)| inside(v.span, off) && matches!(v.kind, Kind::Object(_))) {
                        levels.push(Level { doc: &p.value, schema_obj: None, segment: p.name.clone() });
                        levels.push(Level { doc: item, schema_obj: elem_schema, segment: format!("[{i}]") });
                        continue;
                    }
                    let replace = items.iter().find(|v| v.span.contains(off)).map(|v| v.span).unwrap_or(Span::new(off, off));
                    return Context { levels, position: Position::Value { prop: p, replace } };
                }
                _ => {
                    let replace = if p.value.span.end > p.value.span.start { p.value.span } else { Span::new(off, off) };
                    return Context { levels, position: Position::Value { prop: p, replace } };
                }
            }
        }

        // Between properties: decide key vs value from the preceding token.
        let mut word_start = off;
        while word_start > 0 && is_ident(bytes[word_start - 1]) {
            word_start -= 1;
        }
        let mut word_end = off;
        while word_end < bytes.len() && is_ident(bytes[word_end]) {
            word_end += 1;
        }
        return match crate::parser::last_token_before(text, word_start) {
            Some((_, b'{')) | Some((_, b',')) => Context { levels, position: Position::Key { replace: Span::new(word_start, word_end) } },
            Some((colon, b':')) => {
                // value of the property just before the colon
                match props.iter().rev().find(|p| p.name_span.end <= colon) {
                    Some(p) => Context { levels, position: Position::Value { prop: p, replace: Span::new(word_start, word_end) } },
                    None => Context { levels, position: Position::Other },
                }
            }
            // After a complete value with the comma missing (e.g. a new line):
            // still offer property names; the syntax check flags the comma.
            Some((_, c)) if matches!(c, b'}' | b']' | b'"' | b'\'') || c.is_ascii_alphanumeric() => {
                Context { levels, position: Position::Key { replace: Span::new(word_start, word_end) } }
            }
            _ => Context { levels, position: Position::Other },
        };
    }
}

fn inside(span: Span, off: usize) -> bool {
    off > span.start && off <= span.end
}

fn value_contains(v: &Value, off: usize) -> bool {
    match v.kind {
        Kind::Object(_) | Kind::Array(_) => inside(v.span, off),
        _ => v.span.end > v.span.start && off >= v.span.start && off <= v.span.end,
    }
}

/// Schema property for the property named `name` in the innermost object.
pub fn prop_schema<'s>(s: &'s Schema, ctx: &Context<'_>, name: &str) -> Option<&'s SchemaProp> {
    let l = ctx.innermost()?;
    s.find_prop(l.schema_obj?, name, Some(l.doc))
}
