#!/usr/bin/env python3
"""Differential test: the firmware's JSON parser vs pinscape-config's port.

Builds tools/json-diff/firmware_dump.cpp against an upstream PinscapePico
checkout's Firmware/JSON.cpp (unmodified), then parses a corpus with both and
compares the value trees and the error lists (line + message).

Corpus: the upstream config templates, plus N randomly mutated variants of
them (deleted/duplicated/inserted punctuation, truncation, swapped quotes...),
which exercise the error-recovery paths.

Usage: tools/json-diff/run.py [--upstream ../PinscapePico] [-n 400] [--seed 1]
"""
import argparse, json, math, os, random, shutil, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

def build(upstream: Path, work: Path) -> Path:
    exe = work / "firmware_dump"
    cxx = os.environ.get("CXX", "c++")
    subprocess.run([cxx, "-std=c++17", "-O1", "-w", "-I", str(upstream / "Firmware"),
                    str(HERE / "firmware_dump.cpp"), str(upstream / "Firmware" / "JSON.cpp"),
                    "-o", str(exe)], check=True)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "pinscape-config", "--example", "dump"],
                   check=True, cwd=ROOT)
    return exe

PUNCT = list('{}[](),:"\'') + ["//", "/*", "*/", "\n", " ", "x", "0x", "-", ".", "\\"]

def mutate(text: str, rng: random.Random) -> str:
    t = text
    for _ in range(rng.randint(1, 6)):
        op = rng.random()
        if not t:
            break
        i = rng.randrange(len(t))
        if op < 0.30:      # delete a span
            t = t[:i] + t[i + rng.randint(1, 8):]
        elif op < 0.60:    # insert punctuation
            t = t[:i] + rng.choice(PUNCT) + t[i:]
        elif op < 0.75:    # duplicate a span
            j = i + rng.randint(1, 40)
            t = t[:j] + t[i:j] + t[j:]
        elif op < 0.85:    # truncate
            t = t[:i]
        else:              # swap a delimiter for another
            k = rng.randrange(len(t))
            if t[k] in "{}[],:\"'":
                t = t[:k] + rng.choice("{}[],:\"'") + t[k + 1:]
    return t

def canon(v):
    """Normalize a value tree: sort object properties (stable), round numbers."""
    if isinstance(v, dict):
        if "obj" in v:
            return {"obj": sorted(([k, canon(x)] for k, x in v["obj"]), key=lambda p: p[0])}
        if "arr" in v:
            return {"arr": [canon(x) for x in v["arr"]]}
        return v
    if isinstance(v, float) or isinstance(v, int) and not isinstance(v, bool):
        f = float(v)
        return float(f"{f:.12g}") if math.isfinite(f) else f
    return v

def known_quirk(port, fw_errors, port_errors):
    """The one accepted difference: when the text ends inside an unterminated
    /* comment, the firmware's comment scanner stops one character short of the
    end and doesn't count a final newline, so errors reported at end of file
    carry a line number one too low. The port reports the true (last) line.
    Values, messages and all other lines must still match exactly."""
    if not port.get("eofInComment") or len(fw_errors) != len(port_errors):
        return False
    last = port.get("lastLine")
    for (fl, fm), (pl, pm) in zip(fw_errors, port_errors):
        if fm != pm or not (fl == pl or (pl == fl + 1 and pl == last)):
            return False
    return True

def dump(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    if out.returncode != 0:
        return {"crash": out.returncode, "stderr": out.stderr[-500:]}
    return json.loads(out.stdout)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--upstream", default=str(ROOT.parent / "PinscapePico"))
    ap.add_argument("-n", type=int, default=400)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--keep", action="store_true", help="keep failing cases in ./json-diff-failures")
    a = ap.parse_args()
    upstream = Path(a.upstream).resolve()
    work = Path(tempfile.mkdtemp(prefix="json-diff-"))
    try:
        fw = build(upstream, work)
        rs = ROOT / "target" / "release" / "examples" / "dump"
        templates = sorted((ROOT / "assets" / "templates").glob("*.json"))
        rng = random.Random(a.seed)
        cases = [(t.name, t.read_text()) for t in templates]
        for i in range(a.n):
            src = rng.choice(templates)
            cases.append((f"{src.stem}-mut{i}", mutate(src.read_text(), rng)))
        failures = 0
        for name, text in cases:
            f = work / "case.json"
            f.write_text(text)
            A, B = dump([str(fw), str(f)]), dump([str(rs), str(f)])
            same_err = A.get("errors") == B.get("errors") or known_quirk(B, A.get("errors", []), B.get("errors", []))
            same_val = canon(A.get("value")) == canon(B.get("value"))
            if not (same_err and same_val):
                failures += 1
                if failures <= 5:
                    print(f"MISMATCH {name}: errors {'ok' if same_err else 'DIFFER'}, value {'ok' if same_val else 'DIFFER'}")
                    if not same_err:
                        print("  firmware:", A.get("errors", A)[:6])
                        print("  port:    ", B.get("errors", B)[:6])
                if a.keep:
                    d = Path("json-diff-failures"); d.mkdir(exist_ok=True)
                    (d / f"{name}.json").write_text(text)
        print(f"{len(cases) - failures}/{len(cases)} cases identical "
              f"({len(templates)} templates + {a.n} mutations, seed {a.seed})")
        sys.exit(1 if failures else 0)
    finally:
        shutil.rmtree(work, ignore_errors=True)

if __name__ == "__main__":
    main()
