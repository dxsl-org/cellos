"""Evaluator for the GitHub Actions `${{ }}` expression subset ci.yml uses.

Supported: string/number/bool/null literals, context paths (`a.b-c.d`,
`a['k']`), `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, parentheses
and the status/string functions below. Semantics follow GitHub: `&&`/`||`
return an operand, string comparison ignores case, a missing path is null.
"""

import json
import re

_TOKEN = re.compile(
    r"\s*(?:(?P<str>'(?:[^']|'')*')|(?P<num>-?\d+(?:\.\d+)?)"
    r"|(?P<op>==|!=|<=|>=|&&|\|\||[!<>(),\[\]])"
    r"|(?P<name>[A-Za-z_][A-Za-z0-9_-]*(?:\.(?:[A-Za-z0-9_*-]+))*))"
)
_INTERP = re.compile(r"\$\{\{(.*?)\}\}", re.S)


def truthy(v):
    return v not in (None, False, 0, "")


def to_str(v):
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (dict, list)):
        return json.dumps(v)
    return str(v)


def _eq(a, b):
    if isinstance(a, str) and isinstance(b, str):
        return a.lower() == b.lower()
    return a == b


class Evaluator:
    """`ctx` maps context names (github, env, matrix, ...) to dicts; `status`
    is a zero-argument callable returning 'success', 'failure' or 'cancelled'."""

    def __init__(self, ctx, status=lambda: "success"):
        self.ctx, self.status = ctx, status

    def eval(self, text):
        self.toks = self._lex(text.strip())
        self.i = 0
        value = self._or()
        if self.i != len(self.toks):
            raise ValueError(f"trailing tokens in expression: {text!r}")
        return value

    def interpolate(self, text):
        if not isinstance(text, str):
            return text
        return _INTERP.sub(lambda m: to_str(self.eval(m.group(1))), text)

    def condition(self, text):
        """A step/job `if:`; bare expressions without `${{ }}` are allowed."""
        if text is None:
            return self.status() == "success"
        text = str(text).strip()
        m = _INTERP.fullmatch(text)
        return truthy(self.eval(m.group(1) if m else text))

    def _lex(self, text):
        toks, pos = [], 0
        while pos < len(text):
            m = _TOKEN.match(text, pos)
            if not m or m.end() == pos:
                raise ValueError(f"cannot parse expression at: {text[pos:]!r}")
            toks.append((m.lastgroup, m.group(m.lastgroup)))
            pos = m.end()
            while pos < len(text) and text[pos].isspace():
                pos += 1
        return toks

    def _peek(self):
        return self.toks[self.i] if self.i < len(self.toks) else (None, None)

    def _take(self, op=None):
        tok = self._peek()
        if op is not None and tok != ("op", op):
            raise ValueError(f"expected {op!r}, got {tok[1]!r}")
        self.i += 1
        return tok

    def _or(self):
        left = self._and()
        while self._peek() == ("op", "||"):
            self._take()
            right = self._and()
            left = left if truthy(left) else right
        return left

    def _and(self):
        left = self._unary()
        while self._peek() == ("op", "&&"):
            self._take()
            right = self._unary()
            left = right if truthy(left) else left
        return left

    def _unary(self):
        if self._peek() == ("op", "!"):
            self._take()
            return not truthy(self._unary())
        return self._cmp()

    def _cmp(self):
        left = self._primary()
        kind, op = self._peek()
        if kind == "op" and op in ("==", "!=", "<", "<=", ">", ">="):
            self._take()
            right = self._primary()
            if op in ("==", "!="):
                return _eq(left, right) == (op == "==")
            return {"<": left < right, "<=": left <= right,
                    ">": left > right, ">=": left >= right}[op]
        return left

    def _primary(self):
        kind, val = self._take()
        if kind == "str":
            return val[1:-1].replace("''", "'")
        if kind == "num":
            return float(val) if "." in val else int(val)
        if (kind, val) == ("op", "("):
            value = self._or()
            self._take(")")
            return value
        if kind != "name":
            raise ValueError(f"unexpected token {val!r}")
        if val in ("true", "false", "null"):
            return {"true": True, "false": False, "null": None}[val]
        if self._peek() == ("op", "("):
            return self._call(val)
        value = self._path(val)
        while self._peek() == ("op", "["):
            self._take()
            key = self._or()
            self._take("]")
            value = value.get(to_str(key)) if isinstance(value, dict) else None
        return value

    def _path(self, dotted):
        value = self.ctx
        for part in dotted.split("."):
            value = value.get(part) if isinstance(value, dict) else None
        return value

    def _call(self, name):
        self._take("(")
        args = []
        while self._peek() != ("op", ")"):
            args.append(self._or())
            if self._peek() == ("op", ","):
                self._take()
        self._take(")")
        status = self.status()
        funcs = {
            "always": lambda: True,
            "success": lambda: status == "success",
            "failure": lambda: status == "failure",
            "cancelled": lambda: status == "cancelled",
            "contains": lambda h, n: (to_str(n).lower() in to_str(h).lower())
            if not isinstance(h, list) else any(_eq(x, n) for x in h),
            "startsWith": lambda s, p: to_str(s).lower().startswith(to_str(p).lower()),
            "endsWith": lambda s, p: to_str(s).lower().endswith(to_str(p).lower()),
            "format": lambda f, *a: re.sub(r"\{(\d+)\}", lambda m: to_str(a[int(m.group(1))]), f),
            "fromJSON": lambda s: json.loads(s),
            "toJSON": lambda v: json.dumps(v),
            "hashFiles": lambda *a: "",
        }
        if name not in funcs:
            raise ValueError(f"unsupported function {name}()")
        return funcs[name](*args)
