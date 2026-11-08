//! Dump what pinscape-config's parser makes of a file, in the same format as
//! tools/json-diff/firmware_dump.cpp, for differential testing.
use pinscape_config::parser::{parse, Kind, Value};
use pinscape_config::LineIndex;
use serde_json::{json, Value as J};

fn dump(v: &Value) -> J {
    match &v.kind {
        Kind::Undefined => json!({ "undef": 1 }),
        Kind::Null => J::Null,
        Kind::Bool(b) => J::Bool(*b),
        Kind::Number(n) if !n.is_finite() => J::String("nonfinite".into()),
        Kind::Number(n) => json!(n),
        Kind::String(s) => J::String(s.clone()),
        Kind::Array(a) => json!({ "arr": a.iter().map(dump).collect::<Vec<_>>() }),
        // newest-first, like the firmware's property list
        Kind::Object(p) => json!({ "obj": p.iter().rev().map(|p| json!([p.name, dump(&p.value)])).collect::<Vec<_>>() }),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <file>");
    let text = String::from_utf8_lossy(&std::fs::read(path).expect("read")).into_owned();
    let doc = parse(&text);
    let idx = LineIndex::new(&text);
    let errors: Vec<J> = doc.errors.iter().map(|e| json!([idx.pos(e.span.start).0, e.message])).collect();
    let last_line = idx.pos(text.len()).0;
    println!(
        "{}",
        json!({ "errors": errors, "value": dump(&doc.root), "eofInComment": doc.unterminated_comment.is_some(), "lastLine": last_line })
    );
}
