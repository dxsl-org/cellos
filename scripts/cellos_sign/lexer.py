"""Reduce Rust source to the text rustc would treat as code.

The F1 scan asks two questions of a source file — does it carry
`#![forbid(unsafe_code)]`, and does it contain the `unsafe` keyword — and both
answers must be about *code*, never about the contents of a comment or a
literal. Answering them on raw text is wrong in both directions: the word
"unsafe" in prose is a false positive, and a `/*` or a fake attribute hidden
inside a string literal is a false *negative*, because it makes the naive
scanner blind to real code that follows. False negatives are the only
unacceptable failure here (Spec 18 §2.1), so the reduction below mirrors Rust's
own lexer for every construct that can swallow a delimiter:

  * `//` line and `/* */` block comments — block comments nest in Rust;
  * `"…"`, `b"…"`, `c"…"` strings, where `\\` escapes the next byte;
  * `r"…"`, `r#"…"#`, `br##"…"##`, … raw strings with any hash count and no
    escapes — the closing delimiter is the quote plus the same hashes;
  * `'x'` and `b'x'` char literals, including `'\\''` and `'\\u{1f600}'`.

A lifetime (`&'a str`) also starts with a quote but is not a literal, so a
quote that does not complete a char literal is emitted as ordinary code.

Every removed span is replaced by one space plus the newlines it contained, so
line numbers and line boundaries survive the reduction: an attribute that began
a line still begins a line afterwards.

Unterminated literals consume to end of file. That is a divergence from rustc,
which rejects the file — a file that cannot compile cannot execute `unsafe`.
"""

from __future__ import annotations

import re

# A string/char prefix only counts at a token boundary: `r#type` is a raw
# identifier and `absorb` merely ends in `b`, neither starts a literal.
_IDENT_CHARS = re.compile(r"[A-Za-z0-9_]")
_RAW_PREFIX = re.compile(r'(?:b|c)?r(#*)"')
_STR_PREFIX = re.compile(r'(?:b|c)?"')
_CHAR_LITERAL = re.compile(
    r"b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^\\'\n])'"
)


def _blank(span: str) -> str:
    """A removed span's replacement: a separator plus its newlines.

    The space matters — without it `un/*x*/safe`, two tokens to rustc, would
    fuse into the keyword and report a violation that does not exist.
    """
    return " " + "\n" * span.count("\n")


def _at_boundary(text: str, index: int) -> bool:
    return index == 0 or _IDENT_CHARS.match(text[index - 1]) is None


def _end_of_block_comment(text: str, start: int) -> int:
    """Index just past the `*/` closing the (nesting) block comment at `start`."""
    depth, i, n = 1, start + 2, len(text)
    while i < n and depth:
        if text.startswith("/*", i):
            depth += 1
            i += 2
        elif text.startswith("*/", i):
            depth -= 1
            i += 2
        else:
            i += 1
    return i


def _end_of_string(text: str, start: int) -> int:
    """Index just past the closing quote of the escaped string opening at `start`."""
    i, n = start, len(text)
    while i < n:
        if text[i] == "\\":
            i += 2
        elif text[i] == '"':
            return i + 1
        else:
            i += 1
    return n


def strip_noncode(text: str) -> str:
    """Return `text` with comments and literals blanked, code and lines intact."""
    out: list[str] = []
    i, n = 0, len(text)
    while i < n:
        char = text[i]

        if char == "/" and text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end == -1 else end
            out.append(_blank(text[i:end]))
            i = end
            continue

        if char == "/" and text.startswith("/*", i):
            end = _end_of_block_comment(text, i)
            out.append(_blank(text[i:end]))
            i = end
            continue

        if (char in "rbc" or char == '"') and _at_boundary(text, i):
            raw = _RAW_PREFIX.match(text, i)
            if raw:
                closing = '"' + raw.group(1)
                found = text.find(closing, raw.end())
                end = n if found == -1 else found + len(closing)
                out.append(_blank(text[i:end]))
                i = end
                continue
            plain = _STR_PREFIX.match(text, i)
            if plain:
                end = _end_of_string(text, plain.end())
                out.append(_blank(text[i:end]))
                i = end
                continue

        if (char == "'" or char == "b") and _at_boundary(text, i):
            literal = _CHAR_LITERAL.match(text, i)
            if literal:
                out.append(_blank(literal.group(0)))
                i = literal.end()
                continue

        out.append(char)
        i += 1
    return "".join(out)
