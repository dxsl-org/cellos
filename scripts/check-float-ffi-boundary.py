#!/usr/bin/env python3
"""Reject float or vector values crossing the Rust <-> C boundary of a Cell.

Rule H1 (runtime plan 261104-0802, owner decision D-R3): Tier-2 C/C++ on x86_64
is hard-float SSE2 while every Rust Cell target is soft-float, so a `double`
travels in different registers. No function crossing the boundary may take or
return, by value, float/double/long double/_Complex, f32/f64, x86 vector types
(__m64..__m512) or a struct/union holding one. Callbacks count both ways;
pointers are fine. Inputs: Rust files and C headers. Exit 1 with one
`path:line: symbol: reason` line per finding. Lexical only: no macro
expansion (macro templates are reported by metavariable), no `void *` tracking.
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

C_FLOAT = re.compile(r"\b(float|double|_Complex|__m(?:64|128|256|512)\w*)\b")
RUST_FLOAT = re.compile(r"^(?:f32|f64|(?:core::arch::x86_64::|std::arch::x86_64::)?__m(?:64|128|256|512)\w*)$")


def strip_comments(text: str) -> str:
    """Blank out comments, keeping newlines so line numbers stay right."""
    def blank(m: re.Match) -> str:
        return re.sub(r"[^\n]", " ", m.group(0))
    return re.sub(r"//[^\n]*|/\*.*?\*/", blank, text, flags=re.S)


def split_top(text: str, sep: str = ",") -> list[str]:
    parts, depth, cur = [], 0, []
    for ch in text:
        if ch in "<([{":
            depth += 1
        elif ch in ">)]}":
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if "".join(cur).strip():
        parts.append("".join(cur))
    return [p.strip() for p in parts if p.strip()]


def matching(text: str, open_at: int) -> int:
    """Index of the bracket closing the one at `open_at`."""
    pairs = {"(": ")", "{": "}"}
    opener, closer, depth = text[open_at], pairs[text[open_at]], 0
    for i in range(open_at, len(text)):
        depth += text[i] == opener
        depth -= text[i] == closer
        if depth == 0:
            return i
    raise ValueError("unbalanced brackets")


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


# ---------------------------------------------------------------- C side

def c_bad(decl: str, structs: set[str], aliases: set[str]) -> str | None:
    """Reason a C parameter/return declaration carries a float by value."""
    decl = decl.strip()
    if decl in ("", "void", "..."):
        return None
    m = re.search(r"\(\s*\*\s*\w*\s*\)\s*\(", decl)
    if m:  # function pointer: its own parameters and return are the boundary
        inner = decl[m.end() - 1:]
        params = inner[1:matching(inner, 0)]
        ret = decl[: decl.index("(")]
        for p in split_top(params) + [ret]:
            reason = c_bad(p, structs, aliases)
            if reason:
                return f"callback {reason}"
        return None
    if "*" in decl or decl.endswith("]"):
        return None
    if C_FLOAT.search(decl):
        return f"float/vector by value ({decl})"
    words = set(re.findall(r"\w+", decl))
    hit = words & (structs | aliases)
    return f"float-carrying type by value ({decl})" if hit else None


def scan_c(path: Path, findings: list[str]) -> None:
    text = re.sub(r"(?m)^\s*#.*$", "", strip_comments(path.read_text()))
    structs: set[str] = set()
    aliases: set[str] = set()
    for m in re.finditer(r"\b(?:struct|union)\s*(\w*)\s*\{", text):
        body_end = matching(text, m.end() - 1)
        body = text[m.end():body_end]
        tail = re.match(r"\s*(\w*)", text[body_end + 1:]).group(1)
        members = [s for s in body.split(";") if s.strip()]
        if any(c_bad(mem, structs, aliases) for mem in members):
            structs.update(n for n in (m.group(1), tail) if n)
    for m in re.finditer(r"\btypedef\b([^;{}]*);", text):
        decl = m.group(1).strip()
        name = re.findall(r"\w+", decl.split("(")[1] if "(*" in decl.replace(" ", "") else decl)[-1:]
        if name and c_bad(decl, structs, aliases):
            aliases.add(name[0])
    top = text
    while True:  # drop every brace body, innermost first, keeping line numbers
        stripped = re.sub(r"\{[^{}]*\}", lambda m: "\n" * m.group(0).count("\n"), top)
        if stripped == top:
            break
        top = stripped
    for m in re.finditer(r"([\w\s\*]+?)\b(\w+)\s*\(([^;{}]*)\)\s*;", top):
        if "typedef" in m.group(1):
            continue
        sym, ret, params = m.group(2), m.group(1), m.group(3)
        for decl in [ret] + split_top(params):
            reason = c_bad(decl, structs, aliases)
            if reason:
                findings.append(f"{path}:{line_of(top, m.start(2))}: {sym}: {reason}")
                break


# ------------------------------------------------------------- Rust side

def rust_bad(ty: str, structs: set[str]) -> str | None:
    ty = " ".join(ty.split())
    if "fn(" in ty.replace(" (", "("):
        inner = ty[ty.index("fn") + 2:].lstrip()
        params = inner[1:matching(inner, 0)]
        ret = inner[matching(inner, 0) + 1:].split("->", 1)
        for t in [p.split(":", 1)[-1] for p in split_top(params)] + ret[1:]:
            reason = rust_bad(t.rstrip(">").strip(), structs)
            if reason:
                return f"callback {reason}"
        return None
    if ty.startswith(("*", "&")):
        return None
    base = ty.split("<")[0].split("::")[-1]
    if RUST_FLOAT.match(ty) or RUST_FLOAT.match(base):
        return f"float/vector by value ({ty})"
    return f"float-carrying type by value ({ty})" if base in structs else None


def scan_rust(path: Path, findings: list[str]) -> None:
    text = strip_comments(path.read_text())
    structs: set[str] = set()
    for m in re.finditer(r"\b(?:struct|union)\s+(\w+)[^;{(]*\{", text):
        body = text[m.end():matching(text, m.end() - 1)]
        fields = [f.split(":", 1)[-1] for f in split_top(body)]
        if any(rust_bad(f, structs) for f in fields):
            structs.add(m.group(1))
    for m in re.finditer(r"\bstruct\s+(\w+)\s*\(([^)]*)\)\s*;", text):
        if any(rust_bad(f.removeprefix("pub").strip(), structs) for f in split_top(m.group(2))):
            structs.add(m.group(1))
    # `$?` also catches `extern "C" fn $name(...)` inside macro_rules! templates,
    # reported once under the metavariable name, since macros are not expanded.
    for m in re.finditer(r'extern\s+"C"\s+fn\s+(\$?\w+)\s*(?:<[^>]*>)?\s*\(|\bfn\s+(\w+)\s*\(', text):
        name = m.group(1) or m.group(2)
        if m.group(2) and not _inside_extern_block(text, m.start()):
            continue
        open_at = m.end() - 1
        close = matching(text, open_at)
        after = text[close + 1:]
        ret = re.match(r"\s*->\s*([^;{]+?)\s*(?:where\b|[;{])", after)
        types = [p.split(":", 1)[1] for p in split_top(text[open_at + 1:close]) if ":" in p]
        for ty in types + ([ret.group(1)] if ret else []):
            reason = rust_bad(ty, structs)
            if reason:
                findings.append(f"{path}:{line_of(text, m.start())}: {name}: {reason}")
                break


def _inside_extern_block(text: str, pos: int) -> bool:
    for m in re.finditer(r'extern\s+"C"\s*\{', text[:pos]):
        if matching(text, m.end() - 1) > pos:
            return True
    return False


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--rust", nargs="*", default=[], type=Path)
    ap.add_argument("--c-header", nargs="*", default=[], type=Path)
    ap.add_argument("--allow", action="append", default=[], help="symbol allowed to cross (review each one)")
    args = ap.parse_args(argv)
    findings: list[str] = []
    for p in args.rust:
        for f in sorted(p.rglob("*.rs")) if p.is_dir() else [p]:
            scan_rust(f, findings)
    for p in args.c_header:
        for f in sorted(p.rglob("*.h")) if p.is_dir() else [p]:
            scan_c(f, findings)
    findings = [f for f in findings if f.split(": ")[1] not in args.allow]
    for f in findings:
        print(f)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
