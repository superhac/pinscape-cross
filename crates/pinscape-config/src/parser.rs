//! Rust port of the Pinscape Pico firmware's JSON parser
//! (`PinscapePico/Firmware/JSON.cpp`), keeping its syntax, error messages
//! and error-recovery behavior, so text the editor accepts is text the
//! firmware accepts.
//!
//! The syntax is JSON5-like JavaScript object notation: unquoted property
//! names, single or double quoted strings with escapes, `//` and `/* */`
//! comments, trailing commas, `undefined`/`null`, hex/octal/binary numbers,
//! a leading `#!` line, and parenthesized values.
//!
//! All positions are byte offsets into the source text.

/// A half-open byte range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }
    pub fn contains(&self, off: usize) -> bool {
        off >= self.start && off <= self.end
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Object(Vec<Prop>),
    Array(Vec<Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub kind: Kind,
    /// Source span; for objects/arrays, from the opening to the closing
    /// delimiter (or end of text if unterminated).
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub name: String,
    pub name_span: Span,
    pub value: Value,
}

impl Value {
    fn undefined(span: Span) -> Value {
        Value { kind: Kind::Undefined, span }
    }

    pub fn as_object(&self) -> Option<&[Prop]> {
        match &self.kind {
            Kind::Object(p) => Some(p),
            _ => None,
        }
    }

    /// Property lookup with the firmware's semantics: if a property is defined
    /// more than once (a syntax error the firmware tolerates), the **last**
    /// definition wins, because the firmware's property map prepends entries
    /// and its `find()` returns the first match.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.as_object()?.iter().rev().find(|p| p.name == name).map(|p| &p.value)
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self.kind, Kind::Undefined)
    }

    /// The firmware's `Value::String()` conversion (used for enum and regex checks).
    pub fn to_display_string(&self) -> String {
        match &self.kind {
            Kind::String(s) => s.clone(),
            Kind::Number(n) => fmt_number(*n),
            Kind::Bool(true) => "true".into(),
            Kind::Bool(false) => "false".into(),
            Kind::Null => "null".into(),
            Kind::Undefined => String::new(),
            Kind::Object(_) => "[object]".into(),
            Kind::Array(_) => "[array]".into(),
        }
    }

    /// The firmware's `Value::Double()` conversion.
    pub fn to_number(&self) -> f64 {
        match &self.kind {
            Kind::Number(n) => *n,
            Kind::Bool(b) => *b as i32 as f64,
            Kind::String(s) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
}

fn fmt_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum T {
    Invalid,
    Eof,
    Undefined,
    Null,
    True,
    False,
    Identifier,
    Str,
    Number,
    LParen,
    RParen,
    LSquare,
    RSquare,
    LBrace,
    RBrace,
    Dot,
    Comma,
    Colon,
    Plus,
    Minus,
}

#[derive(Debug, Clone)]
struct Token {
    t: T,
    span: Span,
    /// Decoded text: string contents, identifier text.
    text: String,
    num: f64,
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    errors: Vec<ParseError>,
    unterminated_comment: Option<usize>,
}

pub struct Document {
    pub root: Value,
    pub errors: Vec<ParseError>,
    /// Start of a `/*` comment that runs to the end of the text. The firmware
    /// doesn't report this, but silently ignores everything after it.
    pub unterminated_comment: Option<usize>,
}

/// Parse config text, collecting errors (never fails; always returns a tree).
pub fn parse(text: &str) -> Document {
    let src = text.as_bytes();
    let mut pos = 0;
    // hash-bang line
    if src.len() >= 2 && src[0] == b'#' && src[1] == b'!' {
        pos = 2;
        while pos < src.len() && src[pos] != b'\n' && src[pos] != b'\r' {
            pos += 1;
        }
    }
    let mut p = Parser { src, pos, errors: Vec::new(), unterminated_comment: None };
    let root = p.parse_value();
    let tok = p.get_token();
    if tok.t != T::Eof {
        p.err(tok.span, "Extraneous text after top-level value definition");
    }
    Document { root, errors: p.errors, unterminated_comment: p.unterminated_comment }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

impl Parser<'_> {
    fn err(&mut self, span: Span, msg: &str) {
        self.errors.push(ParseError { span, message: msg.to_string() });
    }

    fn peek_token(&mut self) -> Token {
        let save = self.pos;
        let t = self.get_token();
        self.pos = save;
        t
    }

    fn skip_space_and_comments(&mut self) {
        let s = self.src;
        loop {
            if self.pos >= s.len() {
                return;
            }
            let c = s[self.pos];
            if is_space(c) {
                self.pos += 1;
                continue;
            }
            if c == b'/' && self.pos + 1 < s.len() {
                if s[self.pos + 1] == b'/' {
                    self.pos += 2;
                    while self.pos < s.len() && s[self.pos] != b'\n' && s[self.pos] != b'\r' {
                        self.pos += 1;
                    }
                    continue;
                }
                if s[self.pos + 1] == b'*' {
                    let open = self.pos;
                    self.pos += 2;
                    while self.pos + 1 < s.len() && !(s[self.pos] == b'*' && s[self.pos + 1] == b'/') {
                        self.pos += 1;
                    }
                    if self.pos + 1 >= s.len() {
                        self.unterminated_comment = Some(open);
                    }
                    self.pos = (self.pos + 2).min(s.len());
                    continue;
                }
            }
            return;
        }
    }

    fn get_token(&mut self) -> Token {
        self.skip_space_and_comments();
        let s = self.src;
        let start = self.pos;
        if start >= s.len() {
            return Token { t: T::Eof, span: Span::new(start, start), text: String::new(), num: 0.0 };
        }
        let c = s[start];
        self.pos += 1;
        let single = |t: T| (t, String::new(), 0.0);
        let (t, text, num) = match c {
            b'"' | b'\'' => {
                let (text, _) = self.scan_string(c);
                (T::Str, text, 0.0)
            }
            b'(' => single(T::LParen),
            b')' => single(T::RParen),
            b'[' => single(T::LSquare),
            b']' => single(T::RSquare),
            b'{' => single(T::LBrace),
            b'}' => single(T::RBrace),
            b',' => single(T::Comma),
            b':' => single(T::Colon),
            b'.' if self.pos < s.len() && s[self.pos].is_ascii_digit() => {
                self.pos = start;
                (T::Number, String::new(), self.scan_number())
            }
            b'.' => single(T::Dot),
            b'+' | b'-' if self.pos < s.len() && (s[self.pos].is_ascii_digit() || s[self.pos] == b'.') => {
                self.pos = start;
                (T::Number, String::new(), self.scan_number())
            }
            b'+' => single(T::Plus),
            b'-' => single(T::Minus),
            b'0'..=b'9' => {
                self.pos = start;
                (T::Number, String::new(), self.scan_number())
            }
            c if c == b'_' || c == b'$' || c.is_ascii_alphabetic() => {
                while self.pos < s.len() && (s[self.pos].is_ascii_alphanumeric() || s[self.pos] == b'_' || s[self.pos] == b'$') {
                    self.pos += 1;
                }
                let id = String::from_utf8_lossy(&s[start..self.pos]).into_owned();
                let t = match id.as_str() {
                    "true" => T::True,
                    "false" => T::False,
                    "undefined" => T::Undefined,
                    "null" => T::Null,
                    _ => T::Identifier,
                };
                (t, id, 0.0)
            }
            _ => {
                // invalid character: skip the whole UTF-8 sequence so spans stay on char boundaries
                while self.pos < s.len() && (s[self.pos] & 0xC0) == 0x80 {
                    self.pos += 1;
                }
                single(T::Invalid)
            }
        };
        Token { t, span: Span::new(start, self.pos), text, num }
    }

    /// Scan a quoted string; `self.pos` is just past the open quote.
    /// Strings end at the matching quote or at end of line (unterminated).
    fn scan_string(&mut self, qu: u8) -> (String, bool) {
        let s = self.src;
        let mut out: Vec<u8> = Vec::new();
        while self.pos < s.len() && s[self.pos] != qu && s[self.pos] != b'\n' && s[self.pos] != b'\r' {
            let c = s[self.pos];
            if c != b'\\' {
                out.push(c);
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            if self.pos >= s.len() {
                break;
            }
            let e = s[self.pos];
            self.pos += 1;
            match e {
                b'0' => out.push(0),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b'v' => out.push(0x0B),
                b't' => out.push(b'\t'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0C),
                b'\n' | b'\r' => {
                    // line continuation; treat CR-LF / LF-CR as one terminator
                    if self.pos < s.len() && ((e == b'\n' && s[self.pos] == b'\r') || (e == b'\r' && s[self.pos] == b'\n')) {
                        self.pos += 1;
                    }
                }
                b'x' | b'u' => {
                    let n = if e == b'x' { 2 } else { 4 };
                    let mut acc: u32 = 0;
                    for _ in 0..n {
                        match s.get(self.pos).and_then(|c| (*c as char).to_digit(16)) {
                            Some(d) => {
                                acc = (acc << 4) + d;
                                self.pos += 1;
                            }
                            None => break,
                        }
                    }
                    // The firmware stores 8-bit characters only.
                    out.push(if acc > 0xFF { 0 } else { acc as u8 });
                }
                other => out.push(other),
            }
        }
        let closed = self.pos < s.len() && s[self.pos] == qu;
        if closed {
            self.pos += 1;
        }
        (String::from_utf8_lossy(&out).into_owned(), closed)
    }

    fn scan_number(&mut self) -> f64 {
        let s = self.src;
        let at = |i: usize| s.get(i).copied().unwrap_or(0);
        let mut neg = false;
        if at(self.pos) == b'-' {
            neg = true;
            self.pos += 1;
        } else if at(self.pos) == b'+' {
            self.pos += 1;
        }
        if at(self.pos) == b'0' && b"bBoOxX".contains(&at(self.pos + 1)) && self.pos + 1 < s.len() {
            let radix = match at(self.pos + 1) {
                b'b' | b'B' => 2,
                b'o' | b'O' => 8,
                _ => 16,
            };
            self.pos += 2;
            let mut i: u64 = 0;
            while let Some(d) = (at(self.pos) as char).to_digit(radix) {
                i = i.wrapping_mul(radix as u64).wrapping_add(d as u64);
                self.pos += 1;
            }
            return if neg { -(i as i64) as f64 } else { i as f64 };
        }
        let mut i: u64 = 0;
        while at(self.pos).is_ascii_digit() {
            i = i.wrapping_mul(10).wrapping_add((at(self.pos) - b'0') as u64);
            self.pos += 1;
        }
        let mut num = if neg { -(i as i64) as f64 } else { i as f64 };
        if at(self.pos) == b'.' {
            self.pos += 1;
            let (mut numer, mut denom): (u64, u64) = (0, 1);
            while at(self.pos).is_ascii_digit() {
                let dd = denom.saturating_mul(10);
                if dd < 9_007_199_254_740_991 {
                    denom = dd;
                    numer = numer * 10 + (at(self.pos) - b'0') as u64;
                }
                self.pos += 1;
            }
            let frac = numer as f64 / denom as f64;
            num += if neg { -frac } else { frac };
        }
        if at(self.pos) == b'e' || at(self.pos) == b'E' {
            self.pos += 1;
            let mut eneg = false;
            if at(self.pos) == b'-' {
                eneg = true;
                self.pos += 1;
            } else if at(self.pos) == b'+' {
                self.pos += 1;
            }
            let mut e: i32 = 0;
            while at(self.pos).is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add((at(self.pos) - b'0') as i32);
                self.pos += 1;
            }
            num *= 10f64.powi(if eneg { -e } else { e });
        }
        num
    }

    fn parse_value(&mut self) -> Value {
        let tok = self.peek_token();
        match tok.t {
            T::Comma | T::RBrace | T::RParen | T::RSquare => {
                self.err(tok.span, "Empty value");
                return Value::undefined(Span::new(tok.span.start, tok.span.start));
            }
            T::Identifier => {
                self.err(tok.span, "Unexpected identifier; a value (string, number, boolean, etc) was expected");
                self.get_token();
                return Value::undefined(tok.span);
            }
            _ => {}
        }
        let tok = self.get_token();
        let span = tok.span;
        match tok.t {
            T::Eof => {
                self.err(span, "Unexpected end of file; value required");
                Value::undefined(span)
            }
            T::LBrace => self.parse_object(span.start),
            T::LSquare => self.parse_array(span.start),
            T::LParen => {
                let mut v = self.parse_value();
                let close = self.get_token();
                if close.t != T::RParen {
                    self.err(close.span, "Expected ')'");
                }
                v.span = Span::new(span.start, close.span.end.max(v.span.end));
                v
            }
            T::Undefined => Value::undefined(span),
            T::Null => Value { kind: Kind::Null, span },
            T::True => Value { kind: Kind::Bool(true), span },
            T::False => Value { kind: Kind::Bool(false), span },
            T::Number => Value { kind: Kind::Number(tok.num), span },
            T::Str => Value { kind: Kind::String(tok.text), span },
            _ => {
                self.err(span, "Expected a value");
                Value::undefined(span)
            }
        }
    }

    fn parse_object(&mut self, start: usize) -> Value {
        let mut props: Vec<Prop> = Vec::new();
        let done = |props: Vec<Prop>, end: usize| Value { kind: Kind::Object(props), span: Span::new(start, end) };
        loop {
            let tok = self.peek_token();
            if tok.t == T::Eof {
                self.err(tok.span, "Missing '}'");
                return done(props, tok.span.end);
            }
            if tok.t == T::RBrace {
                self.get_token();
                return done(props, tok.span.end);
            }
            let prop_tok = self.get_token();
            if prop_tok.t != T::Identifier && prop_tok.t != T::Str {
                self.err(prop_tok.span, "Expected property name");
            }
            if prop_tok.t == T::Comma {
                continue;
            }
            let name = match prop_tok.t {
                T::Str | T::Identifier => prop_tok.text.clone(),
                _ => String::from_utf8_lossy(&self.src[prop_tok.span.start..prop_tok.span.end]).into_owned(),
            };
            if props.iter().any(|p| p.name == name) {
                self.err(
                    prop_tok.span,
                    "This property is already defined for this object (each property name must be unique within an object)",
                );
            }
            let colon = self.peek_token();
            if colon.t == T::Colon {
                self.get_token();
                let value = self.parse_value();
                props.push(Prop { name, name_span: prop_tok.span, value });
            } else {
                self.err(colon.span, "Expected ':'");
                props.push(Prop {
                    name,
                    name_span: prop_tok.span,
                    value: Value::undefined(Span::new(prop_tok.span.end, prop_tok.span.end)),
                });
                match colon.t {
                    T::RBrace => {
                        self.get_token();
                        return done(props, colon.span.end);
                    }
                    T::Eof => return done(props, colon.span.end),
                    T::Identifier | T::Str => continue,
                    _ => {}
                }
            }
            let sep = self.peek_token();
            match sep.t {
                T::Comma => {
                    self.get_token();
                }
                T::RBrace => {
                    self.get_token();
                    return done(props, sep.span.end);
                }
                T::Eof => {}
                _ => self.err(sep.span, "Expected ',' or '}'"),
            }
        }
    }

    fn parse_array(&mut self, start: usize) -> Value {
        let mut items: Vec<Value> = Vec::new();
        let done = |items: Vec<Value>, end: usize| Value { kind: Kind::Array(items), span: Span::new(start, end) };
        loop {
            let tok = self.peek_token();
            if tok.t == T::Eof {
                self.err(tok.span, "Missing ']'");
                return done(items, tok.span.end);
            }
            if tok.t == T::RSquare {
                self.get_token();
                return done(items, tok.span.end);
            }
            let before = self.pos;
            items.push(self.parse_value());
            let sep = self.peek_token();
            match sep.t {
                T::Comma => {
                    self.get_token();
                }
                T::RSquare => {
                    self.get_token();
                    return done(items, sep.span.end);
                }
                T::Eof => {}
                _ => {
                    self.err(sep.span, "Expected ',' or ']'");
                    if self.pos == before {
                        return done(items, sep.span.start);
                    }
                }
            }
        }
    }
}

/// The first byte and start offset of the last token that ends at or before
/// `off`, found with the real tokenizer (so comments and strings are skipped
/// correctly). Used by the editor to tell "after `{` or `,`" (a property name
/// goes here) from "after `:`" (a value goes here).
pub fn last_token_before(text: &str, off: usize) -> Option<(usize, u8)> {
    let mut p = Parser { src: text.as_bytes(), pos: 0, errors: Vec::new(), unterminated_comment: None };
    let mut last = None;
    loop {
        let t = p.get_token();
        if t.t == T::Eof || t.span.end > off {
            return last;
        }
        last = Some((t.span.start, text.as_bytes()[t.span.start]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relaxed_syntax() {
        let d = parse(
            "// comment\n{ id: { unitNum: 1, unitName: 'Main', }, /* block */ \"quoted\": [1, 0x1F, -2.5e1, .5, true, null, undefined,], }",
        );
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        let id = d.root.get("id").unwrap();
        assert_eq!(id.get("unitNum").unwrap().kind, Kind::Number(1.0));
        assert_eq!(id.get("unitName").unwrap().kind, Kind::String("Main".into()));
        let Kind::Array(a) = &d.root.get("quoted").unwrap().kind else { panic!() };
        assert_eq!(a[1].kind, Kind::Number(31.0));
        assert_eq!(a[2].kind, Kind::Number(-25.0));
        assert_eq!(a[3].kind, Kind::Number(0.5));
        assert_eq!(a.len(), 7);
    }

    #[test]
    fn escapes() {
        let d = parse(r#"{ s: "a\tb\x41B\"c" }"#);
        assert_eq!(d.root.get("s").unwrap().kind, Kind::String("a\tbAB\"c".into()));
    }

    #[test]
    fn errors_match_firmware_messages() {
        let d = parse("{ a: 1 b: 2, a: 3, c: , d }");
        let msgs: Vec<_> = d.errors.iter().map(|e| e.message.as_str()).collect();
        assert!(msgs.contains(&"Expected ',' or '}'"), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.starts_with("This property is already defined")), "{msgs:?}");
        assert!(msgs.contains(&"Empty value"), "{msgs:?}");
        assert!(msgs.contains(&"Expected ':'"), "{msgs:?}");
    }

    #[test]
    fn unterminated() {
        let d = parse("{ a: [1, 2");
        assert!(d.errors.iter().any(|e| e.message == "Missing ']'"));
        assert!(d.errors.iter().any(|e| e.message == "Missing '}'"));
        assert_eq!(d.root.span.end, 10);
    }

    #[test]
    fn unterminated_comment() {
        let d = parse("{ a: 1, /* oops\n b: 2 }");
        assert_eq!(d.unterminated_comment, Some(8));
        assert!(parse("{ a: 1 /* ok */ }").unterminated_comment.is_none());
    }

    #[test]
    fn extraneous_text() {
        let d = parse("{} x");
        assert_eq!(d.errors[0].message, "Extraneous text after top-level value definition");
    }

    #[test]
    fn spans() {
        let t = "{ buttons: [ { type: \"push\" } ] }";
        let d = parse(t);
        let b = d.root.as_object().unwrap();
        assert_eq!(&t[b[0].name_span.start..b[0].name_span.end], "buttons");
        let Kind::Array(items) = &b[0].value.kind else { panic!() };
        assert_eq!(&t[items[0].span.start..items[0].span.end], "{ type: \"push\" }");
    }
}
