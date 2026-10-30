#!/usr/bin/env python3
"""KSSH K8 lane evaluator (k8-cp1.md sections 8-11, k8-contract.md sections 3, 5, 6).

    kssh_judge.py judge   --evidence DIR --keys DIR --mode t0|f|t1 --set b0|rt0|rt0f|rt1 --fixture-fp SHA256:..
                          --password-file F --seed-hex-file F [--fixture-host-key F] [--trust-disk IMG]
                          [--sshd-log F] [--policy-shape loaded|absent]
    kssh_judge.py census  --baseline BASE.json [--out OUT.json] [--compare-baseline CMP.json]
    kssh_judge.py archive --src EVIDENCE_DIR --dst REPO_SET_DIR --keys KEYS_DIR [--extra-secret-file F ...] [--conf F]
    kssh_judge.py selftest

Mode t1 (RT1, k8-rt1-amendment.md sections 8-10, 12.5; only with --set b0|rt1) judges the five boot directories
`EVIDENCE/boot1..boot5` plus boots.json, disk-baseline.json, controls.json and checker.txt (kssh_boots.py / the runner).

Stdlib only (plus kssh_keys.py next to this file). The judge never prints or writes secret bytes:
every string it emits is scanned for the canary set before it is written.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kssh_keys  # noqa: E402

# =========================================================================== serial markers

MARKERS = {
    "t0": ["shell-prompt", "kssh-conf", "kssh-s00", "kssh-s01a", "kssh-s01b", "kssh-s01", "kssh-s09",
           "kssh-s10", "kssh-s04a", "kssh-s04b", "kssh-s06a", "kssh-s06b", "kssh-s06c", "kssh-s06d",
           "kssh-s02", "kssh-s02p", "kssh-s03", "kssh-s03b", "kssh-s02x", "kssh-s08", "kssh-s15",
           "kssh-transcripts", "session-a-close"],
    "f": ["shell-prompt", "kssh-conf", "kssh-s00", "kssh-s01", "kssh-s05a", "kssh-s05b", "kssh-s05c",
          "kssh-s02", "kssh-s11", "kssh-transcripts", "session-a-close"],
}
CORE_MARKERS = ("kssh-s00", "kssh-s01", "kssh-s02")
TIMEOUT_WORDS = ("timeout", "timed out")

# Expected sshd connections per attempt id (k8-contract.md section 3; Main may re-pin after B0).
# `none` = no connection; every other entry is one connection class per list item.
EXPECTED_CONNECTIONS = {
    "t0": {
        "s00": ["accepted-pw"],
        "s10-changed": ["closed"],
        "s10-untrusted": ["closed"],
        "s04a": [],
        "s04b": ["probe-rejected"],
        "s06d": [],
        "s06d-typed": ["probe-ok"],
        "s02": ["accepted-pk:ka16"],
        "s02p": ["accepted-pk:ka16"],
        "s03": ["probe-ok"],
        "s03b": ["probe-ok"],
        "s02x": ["accepted-pk:ka16"],
        **{f"s08-w{i}": ["probe-ok"] for i in range(1, 6)},
        "s08-locked": ["probe-ok"],
        "s08-ok": ["accepted-pk:kl4"],
        "s15-i": [],
        "s15-iii": ["closed"],
        "s15-iv": ["accepted-pk:kp0"],
    },
    "f": {
        "s00": ["accepted-pw"],
        "s05a": ["probe-ok"],
        "s05b": ["probe-ok"],
        "s05c": ["probe-ok"],
        "s02": ["accepted-pk:ka16"],
        "s11": ["accepted-pk:ka16"],
    },
    # Mode t1 is looked up per boot (`EXPECTED_CONNECTIONS["t1"][boot]`): an attempt id keeps its class in every boot
    # it appears in (s00, s02r). k8-rt1-amendment.md section 8.2. The none-class `s15-i` is registered for boot 3
    # because the T0 s15 helper may print it (contract section 4); it must not open a connection.
    "t1": {
        1: {"s00": ["accepted-pw"], "s02": ["accepted-pk:ka16"]},
        2: {"s00": ["accepted-pw"], "s02r": ["accepted-pk:ka16"],
            **{f"s08-w{i}": ["probe-ok"] for i in range(1, 6)}, "s08-locked": ["probe-ok"]},
        3: {"s00": ["accepted-pw"], "s02x": ["accepted-pk:ka16"], "s15-i": [], "s15-iv": ["accepted-pk:kp0"]},
        4: {"s00": ["accepted-pw"], "s04a-ka16": [], "s04a-kp0": []},
        5: {"s00": ["accepted-pw"], "s02r": ["accepted-pk:ka16"]},
    },
}
# Admission attempts of S06(d): the flagless copy launched directly (`s06d`: no connection) and the typed
# form (`s06d-typed`: probe-ok, ends at the passphrase prompt). Any acceptance there is class D.
ADMISSION_ATTEMPTS = ("s06d", "s06d-typed")
NON_ACCEPTING = ("closed", "probe-rejected", "probe-ok")
# Remote ledger (`p09x-*` lines of the fixture's /tmp/p09-ledger), exact counts per mode.
LEDGER_EXPECTED = {"t0": {"p09x-kssh-login": 3},  # s02, s02x, s08-ok run `p9 kssh-login`; s02p, s15-iv do not
                   "f": {"p09x-kssh-login": 1, "p09x-kssh-rekey": 1},
                   # t1 per boot slice (amendment 8.2): s02 / s02r / s02x / none / s02r run `p9 kssh-login`
                   "t1": {1: {"p09x-kssh-login": 1}, 2: {"p09x-kssh-login": 1}, 3: {"p09x-kssh-login": 1},
                          4: {}, 5: {"p09x-kssh-login": 1}}}
# Mode f oracles (k8-cp1.md 8.3): attempts that run the fault cellbox, the rekey sink and its bounds.
F_FAULT_ATTEMPTS = ("s05a", "s05b", "s05c", "s02", "s11")
FAULTS_ON_RE = re.compile(r"INFO ssh-test-config kssh-faults=on\s*$")
REKEYS_RE = re.compile(r"INFO ssh-rekeys (\d+)\s*$")
SINK_RE = re.compile(r"INFO ssh-bytes stdout=(\d+) sha256=([0-9a-f]{64}) peak_pending=(\d+) peak_chunk=(\d+)\s*$")
ZERO_MIB_SHA256 = hashlib.sha256(bytes(1048576)).hexdigest()
CAL_RE = re.compile(r"^cal-R\d+$")  # B0 calibration attempts: one probe-ok each

# Mode t1 (RT1): serial markers per boot (contract section 3), banners, disk images, limits.
T1_BOOTS = (1, 2, 3, 4, 5)
_T1_HEAD = ["shell-prompt", "kssh-conf", "kssh-s00", "kssh-role"]
_T1_TAIL = ["kssh-transcripts", "session-a-close"]
T1_MARKERS = {
    1: _T1_HEAD + ["kssh-s01a", "kssh-s01b", "kssh-s01", "kssh-s09", "kssh-s02"] + _T1_TAIL,
    2: _T1_HEAD + ["kssh-t1-load", "kssh-t1-opaque", "kssh-s02r", "kssh-t1-s08"] + _T1_TAIL,
    3: _T1_HEAD + ["kssh-t1-load", "kssh-s02x", "kssh-t1-del", "kssh-t1-plain", "kssh-s15"] + _T1_TAIL,
    4: _T1_HEAD + ["kssh-t1-absent"] + _T1_TAIL,
    5: _T1_HEAD + ["kssh-t1-load", "kssh-t1-revert"] + _T1_TAIL,
}
T1_PERSIST_BANNER = ("[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to /srv, dev root key co-resident, "
                     "no rollback or crash-durability claim")
T1_CUSTODY_BANNER = "[kms] DEV_POSTURE ssh-dev-custody"
T1_VFS_LINE = "[vfs] CellosFS Native mounted at existing volume"
T1_LOAD_BOOTS = (2, 3, 5)  # boots whose first KMS contact is the lazy load (`kssh-t1-load`)
T1_FRAME_BOUND_MS = 5000   # K6 admin frame bound (SSH_FRAME_TICKS = 500)
# (image label, boot whose disk-after-boot.json holds it | None = disk-baseline.json, expected record set)
T1_IMAGES = [("d0-before", None, "none"), ("after-boot1", 1, "r1"), ("after-boot2", 2, "r1"),
             ("after-boot3", 3, "r1r2"), ("after-boot4", 4, "r1r2"), ("after-boot5", 5, "r1")]
T1_CHECKER_RC = {"persist": 0, "custody": 1, "none": 1}
T1_TOTALS = {"accepted-pw": 5, "accepted-pk-a": 4, "accepted-pk-p": 1, "probe-ok": 6, "ledger-login": 4}

# =========================================================================== sshd log

_H = r"(?P<host>[0-9A-Fa-f:.]+)"
_P = r"(?P<port>\d+)"
_USER = r"(?:(?:authenticating|invalid) )?user \S+ "
# One table for every wording the judge understands. Wording fixed by the C-host control against the
# real OpenSSH_9.9p2 fixture (reports/k8-evidence/c-host/sshd.log, LogLevel VERBOSE) and by the L5
# password forms. Order matters: first match wins.
SSHD_PATTERNS = [
    ("connect", re.compile(rf"Connection from {_H} port {_P}(?: on (?P<laddr>\S+) port (?P<lport>\d+))?")),
    ("accepted-pk", re.compile(
        rf"Accepted publickey for (?P<user>\S+) from {_H} port {_P} ssh2: (?P<alg>\S+) (?P<fp>SHA256:\S+)")),
    ("postponed-pk", re.compile(rf"Postponed publickey for (?P<user>\S+) from {_H} port {_P} ssh2")),
    ("failed-pk", re.compile(
        rf"Failed publickey for (?:invalid user )?(?P<user>\S+) from {_H} port {_P} ssh2"
        r"(?:: (?P<alg>\S+) (?P<fp>SHA256:\S+))?")),
    ("accepted-pw", re.compile(rf"Accepted password for (?P<user>\S+) from {_H} port {_P} ssh2")),
    ("failed-pw", re.compile(rf"Failed password for (?:invalid user )?(?P<user>\S+) from {_H} port {_P} ssh2")),
    ("failed-none", re.compile(rf"Failed none for (?:invalid user )?(?P<user>\S+) from {_H} port {_P} ssh2")),
    ("session", re.compile(rf"Starting session: .*? for (?P<user>\S+) from {_H} port {_P} id \d+")),
    ("closed", re.compile(rf"Connection closed by (?:{_USER})?{_H} port {_P}")),
    ("closed", re.compile(rf"Disconnected from (?:{_USER})?{_H} port {_P}")),
    ("closed", re.compile(rf"Received disconnect from {_H} port {_P}:\d+")),
    ("closed", re.compile(rf"Connection reset by (?:{_USER})?{_H} port {_P}")),
    ("closed", re.compile(rf"Timeout before authentication for {_H} port {_P}")),
    ("info", re.compile(rf"Close session: user \S+ from {_H} port {_P} id \d+")),
    ("info", re.compile(rf"Closing connection to {_H} port {_P}")),
]
_PORT_RE = re.compile(rf"(?:from|by|to|user \S+) {_H} port {_P}")
_AUTH_WORDS = re.compile(r"publickey|password|Failed|Invalid user|Postponed|Accepted|authentication", re.I)
AUTH_EVENTS = ("accepted-pk", "postponed-pk", "failed-pk", "accepted-pw", "failed-pw")


def parse_sshd(lines) -> dict:
    """Group docker-log lines into connections by guest source port (`Connection from H port N`)."""
    conns, open_by_port, orphans, ignored = [], {}, [], 0
    for line in lines:
        line = line.rstrip("\r\n")
        hit = None
        for event, rx in SSHD_PATTERNS:
            m = rx.search(line)
            if m:
                hit = (event, m)
                break
        if hit is None:
            pm = _PORT_RE.search(line)
            if pm and pm.group("port") in open_by_port and _AUTH_WORDS.search(line):
                conns[open_by_port[pm.group("port")]]["events"].append(("unknown", {"line": line[:200]}))
            else:
                ignored += 1
            continue
        event, m = hit
        g = m.groupdict()
        if event == "connect":
            open_by_port[g["port"]] = len(conns)
            conns.append({"idx": len(conns), "port": int(g["port"]), "lport": int(g["lport"]) if g.get("lport") else None,
                          "events": []})
            continue
        idx = open_by_port.get(g["port"])
        if idx is None:
            if event in AUTH_EVENTS:
                orphans.append(line[:200])
            else:
                ignored += 1
            continue
        conns[idx]["events"].append((event, {k: v for k, v in g.items() if k in ("user", "fp", "alg") and v}))
    for c in conns:
        c["cls"] = classify_conn(c["events"])
    return {"conns": conns, "orphans": orphans, "ignored": ignored}


def classify_conn(events) -> str:
    names = [e for e, _ in events]
    if "unknown" in names:
        return "unclassified"
    pk = [d for e, d in events if e == "accepted-pk"]
    pw_any = "accepted-pw" in names or "failed-pw" in names
    if "accepted-pw" in names:
        return "accepted-pw"
    if pk:
        # exactly one Accepted publickey and no password line at all on this connection
        return f"accepted-pk:{pk[0].get('fp', '?')}" if len(pk) == 1 and not pw_any else "accepted-pk-dirty"
    if "failed-pw" in names:
        return "failed-pw"
    if "postponed-pk" in names:
        return "sig-rejected" if "failed-pk" in names else "probe-ok"
    if "failed-pk" in names:
        return "probe-rejected"
    return "closed"


def password_fallbacks(conns) -> list:
    """Connections where a password event follows a publickey event (never a pass)."""
    out = []
    for c in conns:
        names = [e for e, _ in c["events"]]
        pk_at = [i for i, e in enumerate(names) if e in ("postponed-pk", "failed-pk", "accepted-pk")]
        pw_at = [i for i, e in enumerate(names) if e in ("accepted-pw", "failed-pw")]
        if pk_at and pw_at and max(pw_at) > min(pk_at):
            out.append(c["idx"])
    return out


# --------------------------------------------------------------------------- expected sequence

def _conn_table(mode: str, boot=None) -> dict:
    """Attempt id -> connection classes. Mode t1 is per boot (an unknown boot has no table)."""
    table = EXPECTED_CONNECTIONS[mode]
    return table.get(boot, {}) if mode == "t1" else table


def expected_for(mode: str, attempt: str, boot=None):
    """List of connection-class names for one attempt id, or None when the id is unknown (in that boot, mode t1)."""
    table = _conn_table(mode, boot)
    if attempt in table:
        return list(table[attempt])
    if CAL_RE.match(attempt):
        return ["probe-ok"]
    return None


def _slots(mode, attempts, fps, relaxed=False, boot=None):
    """Expected slots in attempt order. `relaxed`: the admission attempts (s06d, s06d-typed) become
    optional non-accepting slots, used only to tell an acceptance from an ordinary mismatch."""
    out = []
    for a in attempts:
        if relaxed and a in ADMISSION_ATTEMPTS:
            out.append((a, ("opt", NON_ACCEPTING)))
            continue
        for c in expected_for(mode, a, boot) or []:
            if c.startswith("accepted-pk:"):
                c = "accepted-pk:" + fps[c.split(":", 1)[1]]
            out.append((a, c))
    return out


def _slot_ok(slot, got, accepted_ok):
    if isinstance(slot, tuple):
        return got in slot[1] or (accepted_ok and got.startswith(("accepted-pk", "accepted-pw")))
    return slot == got


def _align(slots, actual, accepted_ok=False) -> bool:
    memo = {}

    def go(i, j):
        if i == len(slots):
            return j == len(actual)
        if (i, j) in memo:
            return memo[(i, j)]
        slot = slots[i][1]
        r = False
        if isinstance(slot, tuple):
            r = go(i + 1, j) or (j < len(actual) and _slot_ok(slot, actual[j], accepted_ok) and go(i + 1, j + 1))
        else:
            r = j < len(actual) and slot == actual[j] and go(i + 1, j + 1)
        memo[(i, j)] = r
        return r

    return go(0, 0)


def compare_sequence(mode, attempts, conns, fps, boot=None) -> dict:
    """Ordered connection list vs the sequence built from the attempt ids (in serial order).

    ok / unknown attempt ids / first differing index with the attempt id and the two classes; plus
    `optional_accepted` when the sequence only fits if an s06d/s06d-typed attempt ended in an acceptance.
    """
    unknown = [a for a in attempts if expected_for(mode, a, boot) is None]
    if unknown:
        return {"ok": False, "unknown_attempts": unknown}
    slots = _slots(mode, attempts, fps, boot=boot)
    actual = [c["cls"] for c in conns]
    inv = {v: k for k, v in fps.items()}

    def show(c):
        if isinstance(c, tuple):
            return "optional:" + "|".join(c[1])
        return "accepted-pk:" + inv.get(c.split(":", 1)[1], c.split(":", 1)[1]) if c.startswith("accepted-pk:SHA") else c

    if _align(slots, actual):
        return {"ok": True, "expected": len(slots), "actual": len(actual), "optional_accepted": False}
    relaxed = _slots(mode, attempts, fps, relaxed=True, boot=boot)
    optional_accepted = not _align(relaxed, actual) and _align(relaxed, actual, accepted_ok=True)
    i = j = 0
    while i < len(slots) and j < len(actual):
        if slots[i][1] == actual[j]:
            i, j = i + 1, j + 1
        else:
            break
    exp = slots[i] if i < len(slots) else (attempts[-1] if attempts else "-", "<none>")
    return {"ok": False, "index": j, "attempt": exp[0], "expected_class": show(exp[1]),
            "got_class": show(actual[j]) if j < len(actual) else "<none>",
            "got_port": conns[j]["port"] if j < len(actual) else None,
            "expected": len(slots), "actual": len(actual), "optional_accepted": optional_accepted}


def prefix_ok(mode, attempts, conns, fps, upto: str, boot=None) -> bool:
    """Sequence agrees up to and including the first attempt `upto` (the first sign epoch)."""
    if upto not in attempts:
        return False
    pre = attempts[:attempts.index(upto) + 1]
    if any(expected_for(mode, a, boot) is None for a in pre):
        return False
    slots = _slots(mode, pre, fps, boot=boot)
    n = sum(1 for _, s in slots if not isinstance(s, tuple))
    actual = [c["cls"] for c in conns]
    return any(k <= len(actual) and _align(slots, actual[:k]) for k in (n, n + 1))


# =========================================================================== serial

_P04 = re.compile(r"^(?:USER: )?P04 (?P<rest>.*)$")
_TERM = re.compile(r"^(?:USER: )?P04-TERMINAL: (?P<rest>.*)$")


def parse_serial(text: str) -> dict:
    passes, fails, attempts, obs, order, terminal, started = {}, [], [], [], [], [], False
    segments, test_config, cur = [], [], None
    for n, line in enumerate(text.splitlines()):
        line = line.rstrip()
        t = _TERM.match(line)
        if t:
            terminal.append(t.group("rest"))
            continue
        m = _P04.match(line)
        if not m:
            continue
        rest = m.group("rest")
        if rest.startswith("INFO ssh-test-config"):
            test_config.append(rest)
        if re.match(r"(PASS|FAIL) ", rest):
            cur = None  # a marker ends the attempt segment
        elif re.match(r"INFO kssh-attempt \S+\s*$", rest):
            cur = {"id": rest.split()[2], "lines": []}
            segments.append(cur)
        elif cur is not None:
            cur["lines"].append(rest)
        if rest == "START":
            started = True
        mm = re.match(r"(PASS|FAIL) (\S+)(.*)$", rest)
        if mm:
            if mm.group(1) == "PASS":
                passes.setdefault(mm.group(2), []).append(n)
                order.append(mm.group(2))
            else:
                fails.append((mm.group(2), mm.group(3).strip()))
            continue
        mm = re.match(r"INFO kssh-attempt (\S+)\s*$", rest)
        if mm:
            attempts.append(mm.group(1))
            continue
        mm = re.match(r"INFO kssh-obs (\S+)(.*)$", rest)
        if mm:
            obs.append({"id": mm.group(1), "kv": dict(re.findall(r"(\S+?)=(\S+)", mm.group(1) + mm.group(2)))})
    return {"passes": passes, "fails": fails, "attempts": attempts, "obs": obs, "order": order,
            "terminal": terminal, "started": started, "segments": segments, "test_config": test_config}


# =========================================================================== scanner

JOIN_MAX = 128 << 20
_JOIN_RE = re.compile(rb"\r?\n(?:USER: )?")
CHUNK = 8 << 20
ATOMIC_PREFIXES = ("pass:", "wrong:", "seed:", "expanded:", "extra:", "extra-bin:")


class Scanner:
    """Every form of every canary as a needle; reports (label, form) names, never needle bytes."""

    def __init__(self, canaries):
        self.canaries = list(canaries)
        self.needles: dict[bytes, list] = {}
        for label, data in self.canaries:
            for form, needle in kssh_keys.forms(label, data):
                self.needles.setdefault(needle, []).append((label, form))
        self.maxlen = max((len(n) for n in self.needles), default=1)

    def all_forms(self) -> set:
        return {lf for v in self.needles.values() for lf in v}

    def scan_bytes(self, data: bytes) -> set:
        out = set()
        for needle, lfs in self.needles.items():
            if needle in data:
                out.update(lfs)
        return out

    def scan_file(self, path, joined: bool = True, chunk: int = CHUNK) -> list:
        """[{"label","form","offset"}] hits in one file (all-zero chunks skipped, overlap kept)."""
        found: dict = {}
        keep = max(self.maxlen - 1, 0)
        chunk = max(chunk, keep + 1)
        carry, base = b"", 0
        with open(path, "rb") as f:
            while True:
                buf = f.read(chunk)
                if not buf:
                    break
                if buf.count(0) == len(buf) and not carry.strip(b"\0"):
                    carry, base = b"", base + len(buf)
                    continue
                data = carry + buf
                for needle, lfs in self.needles.items():
                    at = data.find(needle)
                    if at >= 0:
                        for lf in lfs:
                            found.setdefault(lf, base - len(carry) + at)
                base += len(buf)
                carry = data[-keep:] if keep else b""
        if joined and Path(path).stat().st_size <= JOIN_MAX:
            joined_data = _JOIN_RE.sub(b"", Path(path).read_bytes())
            for lf in self.scan_bytes(joined_data):
                if lf not in found:
                    found[(lf[0], lf[1] + "+joined")] = -1
        return [{"label": l, "form": f, "offset": o} for (l, f), o in sorted(found.items())]


def _wrap_mid(needle: bytes) -> bytes:
    mid = len(needle) // 2
    return needle[:mid] + b"\r\nUSER: " + needle[mid:]


def run_controls(scanner: Scanner, sample: bytes, tmp: Path, chunk_small=None) -> dict:
    """Plant canaries in copies of `sample` and require detection (and no false hits on decoys)."""
    res: dict = {"ok": True}
    needles = list(scanner.needles)
    expected = scanner.all_forms()

    def plant(name, parts, with_sample=True):
        p = tmp / name
        p.write_bytes((sample if with_sample else b"") + b"".join(b"\nPLANT " + x for x in parts) + b"\n")
        return p

    # (1) every form, large and tiny chunks (the overlap logic)
    p = plant("all-forms.bin", needles)
    for tag, chunk in (("all-forms", CHUNK), ("all-forms-small-chunk", chunk_small or max(2 * scanner.maxlen, 4096))):
        got = {(h["label"], h["form"].replace("+joined", "")) for h in scanner.scan_file(p, joined=True, chunk=chunk)}
        missing = sorted(expected - got)
        res[tag] = {"forms": len(expected), "missing": [f"{l}/{f}" for l, f in missing]}
        res["ok"] &= not missing
    # (2) every raw form split by a terminal wrap (CRLF + `USER: `)
    wrapped = [(l, d) for l, d in scanner.canaries if len(d) >= 16 and b"\n" not in d and b"\r" not in d]
    p = plant("wrapped.bin", [_wrap_mid(d) for _, d in wrapped])
    raw_hit = {h["label"] for h in scanner.scan_file(p, joined=False)}  # contained whole in another plant: not a test
    got = {h["label"] for h in scanner.scan_file(p, joined=True) if h["form"].endswith("+joined")}
    missing = sorted({l for l, _ in wrapped} - raw_hit - got)
    res["wrapped"] = {"canaries": len({l for l, _ in wrapped}), "missing": missing}
    res["ok"] &= not missing
    # (3) near-misses: first byte flipped in every typed/atomic canary: must NOT hit
    decoys = []
    for label, data in scanner.canaries:
        if label.startswith(ATOMIC_PREFIXES):
            bad = bytes([data[0] ^ 1]) + data[1:]
            decoys += [n for _, n in kssh_keys.forms(label, bad)]
    p = plant("decoys.bin", decoys, with_sample=False)  # the sample may legitimately hold a real canary
    false = scanner.scan_file(p, joined=True)
    res["decoys"] = {"planted": len(decoys), "false_hits": [f"{h['label']}/{h['form']}" for h in false]}
    res["ok"] &= not false
    return res


def sample_of(path: Path, limit: int = 4 << 20) -> bytes:
    if not path.exists():
        return b""
    size = path.stat().st_size
    with open(path, "rb") as f:
        if size <= limit:
            return f.read()
        head = f.read(limit // 2)
        f.seek(size - limit // 2)
        return head + f.read()


def redact_obj(obj, scanner: Scanner):
    """Copy of `obj` where any string containing a canary is replaced; returns (obj, hit_count)."""
    hits = 0

    def go(o):
        nonlocal hits
        if isinstance(o, str):
            if scanner.scan_bytes(o.encode()) or scanner.scan_bytes(_JOIN_RE.sub(b"", o.encode())):
                hits += 1
                return "<redacted: canary>"
            return o
        if isinstance(o, dict):
            return {k: go(v) for k, v in o.items()}
        if isinstance(o, (list, tuple)):
            return [go(v) for v in o]
        return o

    return go(obj), hits


# =========================================================================== judge

class Judge:
    def __init__(self):
        self.checks: list[dict] = []
        self.notes: list[str] = []

    def chk(self, name, ok, detail="", cls="D"):
        self.checks.append({"name": name, "pass": bool(ok), "detail": str(detail)[:600], "cls": cls})
        return bool(ok)


def _read_text(p: Path) -> str | None:
    return p.read_text(errors="replace").replace("\r", "") if p.is_file() else None


def decide_class(failed, epoch, controls_ok, canary_hit, core_ok):
    """k8-cp1.md 11.4. `failed`: list of (cls, name). Returns (class, tail_stop)."""
    classes = {c for c, _ in failed}
    if not failed:
        return "A", False
    if not controls_ok:
        return "E", False  # a vacuous scan cannot support A/B
    if not epoch:
        for c in ("E", "C", "D"):
            if c in classes:
                return c, False
        return "C", False
    if canary_hit or "D" in classes:
        return "D", False  # a product contradiction is a real, final finding (k8-cp1.md 11.4), epoch or not
    if core_ok:
        return "B", True
    return next(c for c in ("E", "C", "D") if c in classes), False


def judge(a) -> int:
    ev = Path(a.evidence)
    keys = Path(a.keys)
    mode, setname = a.mode, a.set
    J = Judge()
    notes = J.notes
    no_value = setname == "b0"
    if (setname, mode) in (("rt0", "f"), ("rt0f", "t0"), ("rt0", "t1"), ("rt0f", "t1"), ("rt1", "t0"), ("rt1", "f")):
        print(f"kssh_judge: --set {setname} does not run in --mode {mode}", file=sys.stderr)
        return 2
    if mode == "t1":
        return judge_t1(a)
    exp = kssh_keys.load_expected(keys)
    fps = {n: e["fp"] for n, e in exp["keys"].items()}
    fp_a = fps["ka16"]
    fixture_fp_ok = bool(kssh_keys.HOSTFP_RE.match(a.fixture_fp))
    J.chk("fixture-fp-format", fixture_fp_ok, a.fixture_fp, "E")

    serial_raw_p, serial_p = ev / "serial.raw.log", ev / "serial.log"
    sshd_p = Path(a.sshd_log) if a.sshd_log else ev / "sshd.log"
    argv_p = ev / "qemu-argv.txt"
    for p in (serial_raw_p, serial_p, argv_p):
        J.chk(f"evidence-file:{p.name}", p.is_file(), "missing", "E")
    log = _read_text(serial_p) or ""
    sshd_text = _read_text(sshd_p)
    J.chk("evidence-file:sshd.log", sshd_text is not None, "missing: sshd oracle cannot attribute", "C")

    # ---- (a) serial markers
    S = parse_serial(log)
    expected_markers = MARKERS[mode]
    J.chk("driver-started", S["started"], "no `P04 START`", "C")
    fail_names = {n for n, _ in S["fails"]}
    for name in expected_markers:
        n = len(S["passes"].get(name, []))
        if n == 1 and name not in fail_names:
            J.chk("marker:" + name, True)
        elif name in fail_names:
            why = next(w for m, w in S["fails"] if m == name)
            J.chk("marker:" + name, False, "FAIL " + why,
                  "C" if any(w in why.lower() for w in TIMEOUT_WORDS) else "D")
        else:
            J.chk("marker:" + name, False, "missing" if n == 0 else f"seen {n} times", "C")
    seq = [m for m in S["order"] if m in expected_markers]
    first = [m for m in expected_markers if m in S["passes"]]
    positions = [S["passes"][m][0] for m in first]
    J.chk("markers-in-order", positions == sorted(positions), f"order seen: {seq}", "C")
    unexpected = sorted(set(S["passes"]) - set(expected_markers))
    J.chk("no-unexpected-markers", not unexpected, unexpected, "C")
    unexpected_fail = sorted(fail_names - set(expected_markers))
    other_fails = [(m, w) for m, w in S["fails"] if m in unexpected_fail]
    J.chk("no-FAIL-markers-outside-list", not other_fails, other_fails, "D")
    term = S["terminal"]
    J.chk("p04-terminal-line", len(term) == 1, f"{len(term)} `P04-TERMINAL:` lines", "C")
    if len(term) == 1:
        only_timeouts = bool(S["fails"]) and all(any(w in why.lower() for w in TIMEOUT_WORDS) for _, why in S["fails"])
        J.chk("p04-terminal-pass", term[0].startswith("PASS") and not S["fails"], term[0] + f" fails={len(S['fails'])}",
              "C" if only_timeouts else "D")
    J.chk("banner-dev-posture", "[kms] DEV_POSTURE ssh-dev-custody" in log, "no `[kms] DEV_POSTURE ssh-dev-custody` (wrong build booted?)", "E")
    J.chk("kms-identity-service", "[kms] fail-closed node identity service starting" in log, "missing", "E")
    loaded = "[policy] loaded + verified (59 entries, flags 0x00)" in log
    if a.policy_shape == "loaded":
        J.chk("policy-loaded", loaded, "missing `[policy] loaded + verified (59 entries, flags 0x00)`", "E")
    else:
        notes.append(f"policy-shape absent accepted; loaded line observed={loaded}")
        J.chk("policy-shape-absent-recorded", True)
    boot_ok = "[selftest] boot-ceiling: PASS" in log
    J.chk("boot-ceiling-selftest", boot_ok, "missing", "D" if "boot-ceiling: FAIL" in log else "E")
    J.chk("no-weak-entropy", "serving WEAK xorshift32" not in log, "weak entropy fallback served", "E")
    J.chk("no-panic", not re.search(r"panic|PANIC|page fault|PAGE FAULT", log), "panic/page fault in serial", "D")
    qemu = _read_text(argv_p) or ""
    J.chk("qemu-one-user-netdev-no-forward",
          qemu.count("-netdev ") == 1 and "-netdev user,id=net0 " in qemu and "fwd" not in qemu and "restrict" not in qemu,
          qemu[:300], "E")

    # ---- (b) sshd
    attempts = S["attempts"]
    parsed = parse_sshd((sshd_text or "").splitlines())
    conns = parsed["conns"]
    sound = sshd_text is not None
    unknown = [c["idx"] for c in conns if c["cls"] == "unclassified"]
    J.chk("sshd-wording-recognised", not unknown and not parsed["orphans"],
          f"unclassified connections {unknown}, orphan auth lines {len(parsed['orphans'])}", "C")
    sound &= not unknown and not parsed["orphans"]
    cmp = compare_sequence(mode, attempts, conns, fps) if sshd_text is not None else {"ok": False, "unknown_attempts": []}
    if cmp.get("unknown_attempts"):
        J.chk("attempt-ids-known", False, cmp["unknown_attempts"], "C")
        sound = False
    elif sshd_text is not None:
        J.chk("attempt-ids-known", True)
        J.chk("s06d-no-acceptance", not cmp.get("optional_accepted"), "an acceptance ended an s06d/s06d-typed attempt (flagless copy served)", "D")
        J.chk("sshd-connection-sequence", cmp["ok"],
              "" if cmp["ok"] else
              f"index {cmp.get('index')}: attempt {cmp.get('attempt')} expected {cmp.get('expected_class')} "
              f"got {cmp.get('got_class')} (port {cmp.get('got_port')}); expected {cmp['expected']} actual {cmp['actual']}",
              "D" if sound else "C")
    fb = password_fallbacks(conns)
    J.chk("no-password-fallback", not fb, f"connections {fb}: password event after a publickey event", "D")
    first_pk = next((c["idx"] for c in conns if any(e in ("postponed-pk", "failed-pk", "accepted-pk") for e, _ in c["events"])), None)
    late_pw = sorted({c["idx"] for c in conns if c["cls"] == "accepted-pw" and first_pk is not None and c["idx"] > first_pk}
                     | {i for i in fb if conns[i]["cls"] == "accepted-pw"})
    J.chk("no-accepted-password-after-publickey", not late_pw, f"connections {late_pw}", "D")
    pk_conns = [c for c in conns if c["cls"].startswith("accepted-pk")]
    J.chk("accepted-publickey-clean", all(c["cls"] != "accepted-pk-dirty" for c in conns), "an accepted-pk connection also has password lines or two Accepted lines", "D")
    known = set(fps.values())
    J.chk("accepted-fingerprints-known", all(c["cls"].split(":", 1)[-1] in known for c in pk_conns if c["cls"] != "accepted-pk-dirty"),
          [c["cls"] for c in pk_conns], "D")

    # ---- (c) host oracles
    s01 = [o["kv"] for o in S["obs"] if o["id"] == "s01" and "fp" in o["kv"]]
    ok = len(s01) == 1 and s01[0].get("fp") == fp_a and s01[0].get("prefix") == exp["keys"]["ka16"]["blob_prefix"]
    J.chk("s01-fingerprint-and-blob-prefix", ok,
          f"obs lines {len(s01)}; fp match {bool(s01) and s01[0].get('fp') == fp_a}; prefix match "
          f"{bool(s01) and s01[0].get('prefix') == exp['keys']['ka16']['blob_prefix']}", "D" if s01 else "C")
    # the lane-only fault build must be announced in mode f and absent from the product image in mode t0
    runs: dict = {}
    for seg in S["segments"]:
        runs.setdefault(seg["id"], []).append(seg["lines"])
    if mode == "t0":
        J.chk("t0-no-test-config", not S["test_config"],
              f"faults-build-in-product-image: {len(S['test_config'])} `ssh-test-config` line(s): {S['test_config'][:3]}", "D")
    else:
        missing = [a for a in F_FAULT_ATTEMPTS if len(runs.get(a, [])) != 1]
        wrong = [f"{a}: {sum(1 for l in runs[a][0] if FAULTS_ON_RE.match(l))}x" for a in F_FAULT_ATTEMPTS
                 if a not in missing and sum(1 for l in runs[a][0] if FAULTS_ON_RE.match(l)) != 1]
        J.chk("kssh-faults-announced", not missing and not wrong,
              f"attempts not run exactly once: {missing}; not announced exactly once: {wrong}", "E" if wrong else "C")
        seg = runs.get("s11", [])
        if len(seg) != 1:
            J.chk("s11-rekey-and-sink", False, f"s11 attempt seen {len(seg)} times", "C")
        else:
            rekeys = [int(m.group(1)) for m in (REKEYS_RE.match(l) for l in seg[0]) if m]
            sinks = [l for l in seg[0] if l.startswith("INFO ssh-bytes")]
            m = SINK_RE.match(sinks[0]) if len(sinks) == 1 else None
            ok = (len(rekeys) == 1 and rekeys[0] >= 1 and m is not None and int(m.group(1)) == 1048576
                  and m.group(2) == ZERO_MIB_SHA256 and int(m.group(3)) <= 2048 and int(m.group(4)) <= 16384)
            J.chk("s11-rekey-and-sink", ok, f"ssh-rekeys {rekeys}; ssh-bytes lines {sinks[:2]}", "D")
    ledger_p = ev / "p09-ledger.txt"
    want = LEDGER_EXPECTED[mode]
    if ledger_p.is_file():
        have = dict(collections.Counter(l.strip() for l in ledger_p.read_text().splitlines() if l.strip().startswith("p09x-")))
        J.chk("remote-ledger", have == want, f"ledger {have}, expected exactly {want}", "D")
    elif no_value:
        notes.append("p09-ledger.txt absent (b0)")
    else:
        J.chk("remote-ledger", False, "p09-ledger.txt missing", "C")
    cc_p = ev / "census-compare.json"
    if cc_p.is_file():
        cc = json.loads(cc_p.read_text())
        J.chk("census-drift", cc["missing"] + cc["changed"] == 0 and cc["equal"] == cc["baseline_present"], cc, "E")
    elif no_value:
        notes.append("census-not-run (b0)")
    else:
        J.chk("census-drift", False, "census-not-run: census-compare.json missing", "E")

    # ---- (d) timing observations
    unlock = [int(o["kv"]["unlock-ms"]) for o in S["obs"] if o["kv"].get("unlock-ms", "").isdigit()]
    unlock += [int(o["kv"]["ms"]) for o in S["obs"] if o["id"] == "unlock-ms" and o["kv"].get("ms", "").isdigit()]
    J.chk("driver-no-scan-hit", not any(o["id"] == "scan-hit" for o in S["obs"]),
          [o["kv"] for o in S["obs"] if o["id"] == "scan-hit"], "D")
    kdf = [{"rounds": o["kv"].get("rounds"), "ms": o["kv"].get("ms")} for o in S["obs"] if o["id"] in ("kdf", "kdf-ms") and "ms" in o["kv"]]
    kdf64 = [int(k["ms"]) for k in kdf if k["rounds"] == "64" and str(k["ms"]).isdigit()]
    observations = {
        "attempts": attempts, "unlock_ms": unlock,
        "unlock_ms_bounds": [min(unlock), max(unlock)] if unlock else None,
        "kdf": kdf, "kdf64_le_10s": (max(kdf64) <= 10000) if kdf64 else None,
        "obs": [{"id": o["id"], "kv": o["kv"]} for o in S["obs"] if o["id"] not in ("s01",)],
        "policy_loaded_line": loaded, "fixture_fp": a.fixture_fp,
    }

    # ---- (e) secret scan
    extras = [(a.password_file, "password"), (a.seed_hex_file, "seed")]
    if a.fixture_host_key:
        extras.append((a.fixture_host_key, None))
    canaries = kssh_keys.canaries(keys, extras)
    scanner = Scanner(canaries)
    targets = [serial_raw_p, serial_p, sshd_p, ev / "p09-ledger.txt", argv_p] + ([Path(a.trust_disk)] if a.trust_disk else [])
    targets = [t for t in targets if t.is_file()]
    with tempfile.TemporaryDirectory(prefix="kssh-ctl-") as td:
        tmp = Path(td)
        sample = sample_of(serial_raw_p)
        pre = run_controls(scanner, sample, tmp)
        scans = []
        for t in targets:
            joined = t != Path(a.trust_disk or "\0")
            scans.append({"path": str(t), "size": t.stat().st_size, "hits": scanner.scan_file(t, joined=joined)})
        post = run_controls(scanner, sample, tmp)
    controls_ok = pre["ok"] and post["ok"]
    J.chk("scan-controls", controls_ok, {"pre": pre["ok"], "post": post["ok"]}, "E")
    if a.trust_disk:
        J.chk("trust-disk-scanned", Path(a.trust_disk).is_file(), a.trust_disk, "E")
    hits = [(s["path"], h) for s in scans for h in s["hits"]]
    J.chk("no-canary-in-targets", not hits,
          [f"{Path(p).name}: {h['label']}/{h['form']}@{h['offset']}" for p, h in hits[:12]], "D")
    present = {}
    blob_text = b"".join(p.read_bytes() for p in (serial_raw_p, serial_p, sshd_p) if p.is_file())
    public = [("kms-posture-banner", b"[kms] DEV_POSTURE ssh-dev-custody"), ("fp-a", fp_a.encode())]
    if sshd_text is not None:  # a missing sshd.log is already class C (observability); do not double-count it as E
        public.append(("accepted-publickey", b"Accepted publickey"))
    if a.policy_shape == "loaded":
        public.append(("policy-loaded-line", b"[policy] loaded"))
    for n, needle in public:
        present[n] = needle in blob_text
    J.chk("scan-expected-public-strings", all(present.values()), [n for n, v in present.items() if not v], "E")

    # ---- (f) classification
    failed = [(c["cls"], c["name"]) for c in J.checks if not c["pass"]]
    ck = {c["name"]: c for c in J.checks}
    s02_idx = attempts.index("s02") if "s02" in attempts else None
    epoch = bool(
        all(len(S["passes"].get(m, [])) == 1 and m not in fail_names for m in CORE_MARKERS)
        and sshd_text is not None and not unknown and not parsed["orphans"]
        and prefix_ok(mode, attempts, conns, fps, "s02")
        and any(c["cls"] == "accepted-pk:" + fp_a for c in conns) and s02_idx is not None)
    canary_hit = bool(hits)
    scan_clean_core = not hits and controls_ok
    klass, tail = decide_class(failed, epoch, controls_ok, canary_hit, epoch and scan_clean_core)
    reasons = [f"{c}:{n}: {ck[n]['detail']}" for c, n in failed]
    if tail:
        reasons.insert(0, "tail STOP after the first sign epoch: later scenarios unproven")
    outputs = {
        "verdict.json": {
            "set": setname, "mode": mode, "policy_shape": a.policy_shape, "no_evidence_value": no_value,
            "checks": {c["name"]: {"pass": c["pass"], "detail": c["detail"]} for c in J.checks},
            "observations": observations, "notes": notes,
            "sshd_connections": [{"idx": c["idx"], "port": c["port"], "listener": c["lport"], "class": c["cls"],
                                  "events": [e for e, _ in c["events"]]} for c in conns],
            "sshd_ignored_lines": parsed["ignored"],
        },
        "classification.json": {"class": klass, "reasons": reasons, "first_sign_epoch_passed": epoch,
                                "no_evidence_value": no_value},
        "scan.json": {"canaries": len(canaries), "forms": len(scanner.needles), "targets": scans,
                      "expected_public_strings": present, "judge_outputs_scanned": True,
                      "trust_disk": bool(a.trust_disk), "no_evidence_value": no_value},
        "controls.json": {"pre": pre, "post": post, "ok": controls_ok, "no_evidence_value": no_value},
    }
    leaked = 0
    for name in list(outputs):
        outputs[name], n = redact_obj(outputs[name], scanner)
        leaked += n
    if leaked:
        outputs["classification.json"]["reasons"].append(f"D:output-contained-canary: {leaked} string(s) redacted")
        if klass in ("A", "B"):
            outputs["classification.json"]["class"] = klass = "D"
    for name, obj in outputs.items():
        (ev / name).write_text(json.dumps(obj, indent=2) + "\n")
    post_hits = []
    for name in outputs:
        post_hits += [(name, h) for h in scanner.scan_file(ev / name, joined=True)]
    if post_hits:
        for name, _ in post_hits:
            (ev / name).unlink(missing_ok=True)
        (ev / "classification.json").write_text(json.dumps(
            {"class": "D", "reasons": ["D:judge-output-canary-leak (files removed)"], "first_sign_epoch_passed": epoch,
             "no_evidence_value": no_value}, indent=2) + "\n")
        print("FAIL judge-output-canary-leak  <- a judge output contained a canary; files removed", file=sys.stderr)
        return 1
    for c in J.checks:
        detail = redact_obj(c["detail"], scanner)[0]
        print(("PASS " if c["pass"] else "FAIL ") + c["name"] + ("" if c["pass"] else f"  <- {detail}"))
    for n in notes:
        print("NOTE " + n)
    print(f"CLASS {klass}{' (no evidence value: b0)' if no_value else ''} first_sign_epoch_passed={epoch}"
          f" checks={len(J.checks)} failed={len(failed)}")
    return 0 if klass == "A" else 1

# =========================================================================== judge mode t1 (RT1)

def _read_json(p: Path):
    try:
        return json.loads(p.read_text())
    except (OSError, ValueError):
        return None


def t1_norm_argv(text: str) -> str:
    """qemu-argv.txt with the per-boot disk path and the serial socket path removed (`argv-equal-modulo-disk`)."""
    text = re.sub(r"file=\S+?,if=none", "file=<disk>,if=none", text)
    return re.sub(r"\bpath=[^,\s]+", "path=<sock>", text)


def _role_of(text) -> int | None:
    m = re.search(r"role=(\d+)", text or "") or re.fullmatch(r"\s*(\d+)\s*", text or "")
    return int(m.group(1)) if m else None


def t1_record_problems(doc: dict, expect: str) -> list:
    """Independent reading of `records.valid` of a disk-after-boot.json against the table of amendment 8
    (R1 = slot 0 present, name ka16, revision 1, equal to the host prediction; R2 = slot 1 tombstone chained onto R1)."""
    valid = (doc.get("records") or {}).get("valid")
    if not isinstance(valid, list):
        return ["records.valid missing"]
    r1 = [e for e in valid if e.get("slot") == 0 and e.get("state") == 1]
    r2 = [e for e in valid if e.get("slot") == 1 and e.get("state") == 0]
    out = []
    if len(r1) + len(r2) != len(valid):
        out.append(f"{len(valid) - len(r1) - len(r2)} valid record(s) outside the R1/R2 shapes")
    if expect == "none":
        return out + ([f"{len(valid)} valid record(s) on a disk expected to hold none"] if valid else [])
    if len(r1) != 1:
        out.append(f"{len(r1)} distinct valid slot-0 present records, expected exactly one")
    else:
        e = r1[0]
        if e.get("equals_predicted") is not True:
            out.append("R1 differs from the host's prediction")
        if e.get("name") != "ka16" or e.get("revision") != 1:
            out.append(f"R1 name/revision {e.get('name')!r}/{e.get('revision')}")
    if expect == "r1":
        out += [f"{len(r2)} valid tombstone(s) where none is expected"] if r2 else []
    elif len(r2) != 1:
        out.append(f"{len(r2)} distinct valid slot-1 tombstones, expected exactly one")
    elif len(r1) == 1 and r2[0].get("previous") != r1[0].get("digest"):
        out.append("tombstone `previous` is not the digest of R1")
    return out


def t1_disk_verdicts(label: str, doc, expect: str, load_state, image_name: str) -> list:
    """[(check name, ok, detail, class)] for one scanned image (`t1-disk-records:` / `t1-disk-scan:`).

    `load_state`: True / False / None = boot 2's `kssh-t1-load` passed / failed / boot 2 not run. It attributes a
    miss of the host-derived records or ciphertext (amendment 9, control (2)): load passed -> the bytes are on
    the disk in a form the finder cannot see (C); load failed -> persistence failed (D); no boot 2 -> C.
    """
    rn, sn = f"t1-disk-records:{label}", f"t1-disk-scan:{label}"
    if not isinstance(doc, dict):
        why = "disk findings missing or unreadable (no snapshot is taken from an unclean boot)"
        return [(rn, False, why, "E"), (sn, False, why, "E")]
    miss_cls = "D" if load_state is False else "C"
    out = []
    verdict = doc.get("record_verdict") if isinstance(doc.get("record_verdict"), dict) else {}
    valid = (doc.get("records") or {}).get("valid") or []
    if doc.get("image") != image_name or verdict.get("expect") != expect:
        out.append((rn, False, f"image {doc.get('image')!r} / expected set {verdict.get('expect')!r}, "
                    f"judged for {image_name!r} / {expect!r}", "E"))
    else:
        problems = t1_record_problems(doc, expect)
        reasons = [str(r) for r in (verdict.get("reasons") or [])]
        ok = verdict.get("ok") is True and not problems
        miss = expect != "none" and not valid
        out.append((rn, ok, "" if ok else "; ".join(problems + reasons)[:500] or "record verdict not ok",
                    "D" if not miss else miss_cls))
    scan = doc.get("scan") if isinstance(doc.get("scan"), dict) else {}
    hits = scan.get("hits") or []
    ct = doc.get("ciphertext_found") is True
    if hits:
        out.append((sn, False, "canary hit(s): " + ", ".join(
            f"{h.get('label')}/{h.get('form')}@{h.get('offset')}" for h in hits[:8]), "D"))
    elif scan.get("controls_ok") is not True:
        out.append((sn, False, "disk scan controls failed (the scan cannot support a verdict)", "E"))
    elif expect == "none" and ct:
        out.append((sn, False, "ciphertext positive control found on a blank baseline disk", "E"))
    elif expect != "none" and not ct:
        out.append((sn, False, "host-derived ciphertext of ka16 not found on the image "
                    + ("(boot 2 load failed: persistence failed)" if load_state is False else
                       "(finder cannot see the bytes, or boot 2 did not run: not attributable)"), miss_cls))
    else:
        out.append((sn, True, "", "D"))
    return out


def _t1_boot(J: Judge, n: int, bd: Path, ctx: dict) -> dict:
    """All checks of one run boot (names are `b<n>:...`); returns what the cross-boot checks and classes need."""
    fps, fp_a, notes = ctx["fps"], ctx["fp_a"], J.notes
    st: dict = {"n": n, "names": []}

    def chk(name, ok, detail="", cls="D"):
        st["names"].append(name)
        return J.chk(f"b{n}:{name}", ok, detail, cls)

    raw_p, ser_p, argv_p, sshd_p, led_p = (bd / x for x in ("serial.raw.log", "serial.log", "qemu-argv.txt", "sshd.log", "p09-ledger.txt"))
    for p in (raw_p, ser_p, argv_p):
        chk(f"evidence-file:{p.name}", p.is_file(), "missing", "E")
    log = _read_text(ser_p) or ""
    sshd_text = _read_text(sshd_p)
    chk("evidence-file:sshd.log", sshd_text is not None, "missing: sshd oracle cannot attribute", "C")
    st["log"], st["sshd_text"] = log, sshd_text

    # ---- serial markers
    S = parse_serial(log)
    st["S"] = S
    expected_markers = T1_MARKERS[n]
    chk("driver-started", S["started"], "no `P04 START`", "C")
    fail_names = {m for m, _ in S["fails"]}
    ok_markers = set()
    for name in expected_markers:
        k = len(S["passes"].get(name, []))
        if k == 1 and name not in fail_names:
            ok_markers.add(name)
            chk("marker:" + name, True)
        elif name in fail_names:
            why = next(w for m, w in S["fails"] if m == name)
            chk("marker:" + name, False, "FAIL " + why, "C" if any(w in why.lower() for w in TIMEOUT_WORDS) else "D")
        else:
            chk("marker:" + name, False, "missing" if k == 0 else f"seen {k} times", "C")
    st["ok_markers"] = ok_markers
    first = [m for m in expected_markers if m in S["passes"]]
    positions = [S["passes"][m][0] for m in first]
    chk("markers-in-order", positions == sorted(positions), f"order seen: {[m for m in S['order'] if m in expected_markers]}", "C")
    unexpected = sorted(set(S["passes"]) - set(expected_markers))
    chk("no-unexpected-markers", not unexpected, unexpected, "C")
    other_fails = [(m, w) for m, w in S["fails"] if m not in expected_markers]
    chk("no-FAIL-markers-outside-list", not other_fails, other_fails, "D")
    term = S["terminal"]
    chk("p04-terminal-line", len(term) == 1, f"{len(term)} `P04-TERMINAL:` lines", "C")
    if len(term) == 1:
        only_timeouts = bool(S["fails"]) and all(any(w in why.lower() for w in TIMEOUT_WORDS) for _, why in S["fails"])
        chk("p04-terminal-pass", term[0].startswith("PASS") and not S["fails"], term[0] + f" fails={len(S['fails'])}",
            "C" if only_timeouts else "D")

    # ---- banner, boot environment (T0 checks that still apply)
    chk("banner-dev-posture-persist", T1_PERSIST_BANNER in log, "no `[kms] DEV_POSTURE ssh-dev-persist ...` banner (wrong build booted?)", "E")
    chk("banner-no-custody", T1_CUSTODY_BANNER not in log, "`[kms] DEV_POSTURE ssh-dev-custody` printed: the T0 build booted", "E")
    chk("kms-identity-service", "[kms] fail-closed node identity service starting" in log, "missing", "E")
    loaded = "[policy] loaded + verified (59 entries, flags 0x00)" in log
    st["policy_loaded"] = loaded
    if ctx["policy_shape"] == "loaded":
        chk("policy-loaded", loaded, "missing `[policy] loaded + verified (59 entries, flags 0x00)`", "E")
    else:
        notes.append(f"boot{n}: policy-shape absent accepted; loaded line observed={loaded}")
        chk("policy-shape-absent-recorded", True)
    chk("boot-ceiling-selftest", "[selftest] boot-ceiling: PASS" in log, "missing", "D" if "boot-ceiling: FAIL" in log else "E")
    chk("no-weak-entropy", "serving WEAK xorshift32" not in log, "weak entropy fallback served", "E")
    chk("no-panic", not re.search(r"panic|PANIC|page fault|PAGE FAULT", log), "panic/page fault in serial", "D")
    chk("trust-store-volume-mounted", T1_VFS_LINE in log, f"missing `{T1_VFS_LINE}`", "E")
    qemu = _read_text(argv_p) or ""
    st["argv"] = t1_norm_argv(qemu.strip()) if argv_p.is_file() else None
    chk("qemu-one-user-netdev-no-forward",
        qemu.count("-netdev ") == 1 and "-netdev user,id=net0 " in qemu and "fwd" not in qemu and "restrict" not in qemu,
        qemu[:300], "E")

    # ---- sshd
    attempts = S["attempts"]
    parsed = parse_sshd((sshd_text or "").splitlines())
    conns = parsed["conns"]
    st.update(attempts=attempts, parsed=parsed, conns=conns)
    sound = sshd_text is not None
    unknown = [c["idx"] for c in conns if c["cls"] == "unclassified"]
    chk("sshd-wording-recognised", not unknown and not parsed["orphans"],
        f"unclassified connections {unknown}, orphan auth lines {len(parsed['orphans'])}", "C")
    sound &= not unknown and not parsed["orphans"]
    cmp = compare_sequence("t1", attempts, conns, fps, boot=n) if sshd_text is not None else {"ok": False, "unknown_attempts": []}
    if cmp.get("unknown_attempts"):
        chk("attempt-ids-known", False, cmp["unknown_attempts"], "C")
        sound = False
    elif sshd_text is not None:
        chk("attempt-ids-known", True)
        chk("sshd-connection-sequence", cmp["ok"],
            "" if cmp["ok"] else
            f"index {cmp.get('index')}: attempt {cmp.get('attempt')} expected {cmp.get('expected_class')} "
            f"got {cmp.get('got_class')} (port {cmp.get('got_port')}); expected {cmp['expected']} actual {cmp['actual']}",
            "D" if sound else "C")
    st["sound"] = sound
    fb = password_fallbacks(conns)
    chk("no-password-fallback", not fb, f"connections {fb}: password event after a publickey event", "D")
    first_pk = next((c["idx"] for c in conns if any(e in ("postponed-pk", "failed-pk", "accepted-pk") for e, _ in c["events"])), None)
    late_pw = sorted({c["idx"] for c in conns if c["cls"] == "accepted-pw" and first_pk is not None and c["idx"] > first_pk}
                     | {i for i in fb if conns[i]["cls"] == "accepted-pw"})
    chk("no-accepted-password-after-publickey", not late_pw, f"connections {late_pw}", "D")
    pk_conns = [c for c in conns if c["cls"].startswith("accepted-pk")]
    chk("accepted-publickey-clean", all(c["cls"] != "accepted-pk-dirty" for c in conns),
        "an accepted-pk connection also has password lines or two Accepted lines", "D")
    known = set(fps.values())
    chk("accepted-fingerprints-known", all(c["cls"].split(":", 1)[-1] in known for c in pk_conns if c["cls"] != "accepted-pk-dirty"),
        [c["cls"] for c in pk_conns], "D")

    # ---- host oracles on the observation lines
    obs: dict = collections.defaultdict(list)
    for o in S["obs"]:
        obs[o["id"]].append(o["kv"])
    st["obs"] = obs
    conf = obs.get("conf", [])
    conf_ok = len(conf) == 1 and conf[0].get("mode") == "t1" and conf[0].get("calibrate") == "false" and conf[0].get("keys") == "2"
    chk("conf-observed-t1", conf_ok, f"conf obs {conf[:2]}", "E" if conf else "C")
    st["role_obs"] = [int(k["n"]) for k in obs.get("role", []) if k.get("n", "").isdigit()]
    if n == 1:
        s01 = [k for k in obs.get("s01", []) if "fp" in k]
        pfx = ctx["exp"]["keys"]["ka16"]["blob_prefix"]
        ok = len(s01) == 1 and s01[0].get("fp") == fp_a and s01[0].get("prefix") == pfx
        chk("s01-fingerprint-and-blob-prefix", ok,
            f"obs lines {len(s01)}; fp match {bool(s01) and s01[0].get('fp') == fp_a}; prefix match "
            f"{bool(s01) and s01[0].get('prefix') == pfx}", "D" if s01 else "C")
    load_ok = None
    if n in T1_LOAD_BOOTS:
        ld = obs.get("t1-load", [])
        k = ld[0] if len(ld) == 1 else {}
        facts = (k.get("keys"), k.get("readiness", "").lower(), k.get("posture", "").lower(), k.get("custody", "").lower(), k.get("fp"))
        ok = len(ld) == 1 and facts == ("1", "ready", "persistent", "wrapped", fp_a)
        chk("t1-load-facts", ok, f"{len(ld)} t1-load obs line(s); keys/readiness/posture/custody/fp-match: "
            f"{facts[:4]} {facts[4] == fp_a}", "D" if ld else "C")
        load_ok = bool(ok and "kssh-t1-load" in ok_markers)
    st["load_ok"] = load_ok
    st["t1_load"] = (obs.get("t1-load") or [None])[0]
    if n == 2:
        op = obs.get("t1-opaque", [])
        k = op[0] if len(op) == 1 else {}
        ok = len(op) == 1 and k.get("cat_leak") == "false" and k.get("cat_st", "").isdigit() and int(k["cat_st"]) != 0
        chk("t1-opaque", ok, f"{len(op)} t1-opaque obs line(s): cat_st={k.get('cat_st')} cat_leak={k.get('cat_leak')} "
            f"(a record file served to a non-KMS cell is a finding)", "D" if op else "C")
        st["t1_opaque"] = k or None
    st["imports"] = [k.get("name") for k in obs.get("t1-import", [])]
    ms = [{"op": k.get("op"), "ms": int(k["ms"])} for k in obs.get("t1-ms", []) if k.get("ms", "").isdigit()]
    st["t1_ms"] = ms
    slow = [m for m in ms if m["op"] in ("first-status", "reload") and m["ms"] > T1_FRAME_BOUND_MS]
    chk("t1-ms-frame-bound", not slow, f"K6 frame bound ({T1_FRAME_BOUND_MS} ms) exceeded by single status calls: {slow}", "D")
    tr = obs.get("transcripts", [])
    t = tr[0] if len(tr) == 1 else {}
    hits_n, scans_n = t.get("hits", ""), t.get("scans", "")
    ok = len(tr) == 1 and hits_n.isdigit() and scans_n.isdigit() and int(hits_n) == 0 and int(scans_n) > 0
    chk("transcripts-clean", ok, f"{len(tr)} transcripts obs line(s): scans={scans_n} hits={hits_n}",
        "D" if hits_n.isdigit() and int(hits_n) > 0 else "C")
    chk("t0-no-test-config", not S["test_config"],
        f"faults-build-in-product-image: {len(S['test_config'])} `ssh-test-config` line(s): {S['test_config'][:3]}", "D")
    chk("driver-no-scan-hit", not obs.get("scan-hit"), obs.get("scan-hit"), "D")

    # ---- remote ledger slice
    want = LEDGER_EXPECTED["t1"][n]
    have = None
    if led_p.is_file():
        have = dict(collections.Counter(l.strip() for l in led_p.read_text().splitlines() if l.strip().startswith("p09x-")))
        chk("remote-ledger", have == want, f"ledger {have}, expected exactly {want}", "D")
    elif ctx["no_value"]:
        notes.append(f"boot{n}: p09-ledger.txt absent (b0)")
    else:
        chk("remote-ledger", False, "p09-ledger.txt missing", "C")
    st["ledger"] = have

    # ---- orchestrator's boot.json
    bj = _read_json(bd / "boot.json")
    st["boot_json"] = bj if isinstance(bj, dict) else None
    if st["boot_json"] is None:
        chk("boot-json", False, "boot.json missing or unreadable", "E")
    else:
        q = bj.get("qemu") if isinstance(bj.get("qemu"), dict) else {}
        chk("qemu-clean-exit", q.get("clean") is True and not q.get("escalated"),
            f"qemu {q}: a QEMU that needed SIGKILL (or is still alive) leaves a non-quiescent disk", "E")
        chk("boot-json-identity", bj.get("n") == n and bj.get("disk_image") == ("S1r" if n == 5 else "D0"),
            f"n={bj.get('n')} disk_image={bj.get('disk_image')}", "E")
        chk("driver-verdict", bj.get("verdict") == "driver-verdict", f"verdict {bj.get('verdict')}", "C")
        chk("sshd-slice-settled", bj.get("settled") is True, "docker logs did not settle: slice boundaries unproven", "C")
    st["role_file"] = _role_of(_read_text(bd / "role.txt"))
    st["load_pass"] = load_ok
    return st


def judge_t1(a) -> int:
    ev = Path(a.evidence)
    keys = Path(a.keys)
    setname = a.set
    mode = "t1"
    J = Judge()
    notes = J.notes
    no_value = setname == "b0"
    exp = kssh_keys.load_expected(keys)
    fps = {n: e["fp"] for n, e in exp["keys"].items()}
    if "ka16" not in fps or "kp0" not in fps:
        print("kssh_judge: the key set is not a t1 set (ka16 and kp0 expected)", file=sys.stderr)
        return 2
    fp_a, fp_p = fps["ka16"], fps["kp0"]
    J.chk("keyset-mode-t1", exp.get("mode") == "t1" and isinstance(exp.get("record"), dict), "expected.json has no t1 record expectation", "E")
    J.chk("fixture-fp-format", bool(kssh_keys.HOSTFP_RE.match(a.fixture_fp)), a.fixture_fp, "E")
    if a.trust_disk or a.sshd_log:
        notes.append("--trust-disk / --sshd-log are ignored in mode t1 (per-boot sshd logs, disk findings from the orchestrator)")
    ctx = {"fps": fps, "fp_a": fp_a, "exp": exp, "no_value": no_value, "policy_shape": a.policy_shape}

    # ---- orchestrator files (the controls.json read here is overwritten with the merged set-level one below)
    bjs = _read_json(ev / "boots.json")
    J.chk("boots-json", isinstance(bjs, dict) and bjs.get("set") == setname,
          f"boots.json missing/unreadable or for another set (judged --set {setname})", "E")
    bjs = bjs if isinstance(bjs, dict) else {}
    stopped = bjs.get("stopped") if isinstance(bjs.get("stopped"), dict) else None
    orch = _read_json(ev / "controls.json")
    disk_ctl = orch.get("disk") if isinstance(orch, dict) and isinstance(orch.get("disk"), list) else []
    disk_ctl_ok = bool(disk_ctl) and all(isinstance(c, dict) and c.get("ok") is True for c in disk_ctl)
    J.chk("t1-disk-controls", disk_ctl_ok, f"controls.json disk part: {len(disk_ctl)} entr(y/ies), ok={disk_ctl_ok}", "E")

    # ---- the five boots
    B: dict = {}
    unrun = []
    for n in T1_BOOTS:
        bd = ev / f"boot{n}"
        if bd.is_dir():
            B[n] = _t1_boot(J, n, bd, ctx)
        else:
            unrun.append(n)
    ran = sorted(B)
    if stopped and stopped.get("class_hint") in ("E", "C"):
        J.chk("sequence-stopped", False, f"stopped after boot {stopped.get('after_boot')}: {stopped.get('reason')}",
              stopped["class_hint"])

    # ---- cross-boot checks
    role_bad, role_ok = [], {}
    for n in ran:
        st, bad = B[n], []
        if st["role_obs"] != [n]:
            bad.append(f"boot{n}: serial `role n=` {st['role_obs']}")
        if st["role_file"] != n:
            bad.append(f"boot{n}: role.txt {st['role_file']}")
        if st["boot_json"] is not None:
            bad += [f"boot{n}: boot.json {k}={st['boot_json'].get(k)}" for k in ("n", "role", "role_written")
                    if st["boot_json"].get(k) != n]
        role_ok[n] = not bad
        role_bad += bad
    J.chk("t1-role-sequence", bool(ran) and not role_bad, "; ".join(role_bad) or "no boot ran", "E")

    if 2 in B and 5 in B:
        l2, l5 = B[2]["t1_load"], B[5]["t1_load"]
        J.chk("t1-reload-equals-replay", l2 is not None and l2 == l5, f"boot 2 {l2}; boot 5 {l5}",
              "D" if l2 is not None and l5 is not None else "C")
    imp_bad, imp_missing = [], []
    for n in ran:
        got = B[n]["imports"]
        if n == 3:
            if not got:
                imp_missing.append("boot3: no import observed, expected exactly ['kp0']")
            elif got != ["kp0"]:
                imp_bad.append(f"boot3 imports {got}, expected exactly ['kp0']")
        elif n != 1 and got:
            imp_bad.append(f"boot{n} imports {got}, expected none")
    J.chk("t1-no-import-after-boot1", not imp_bad and not imp_missing, "; ".join(imp_bad + imp_missing),
          "D" if imp_bad else "C")  # a missing observation is not a product contradiction
    argvs = {n: B[n]["argv"] for n in ran}
    J.chk("t1-argv-equal-modulo-disk", bool(ran) and None not in argvs.values() and len(set(argvs.values())) == 1,
          f"{len(set(v for v in argvs.values() if v))} distinct normalised argv over boots {ran}", "E")
    isos = {n: (B[n]["boot_json"] or {}).get("iso_sha256") for n in ran}
    top_iso = bjs.get("iso_sha256")
    J.chk("t1-iso-same", bool(top_iso) and all(v == top_iso for v in isos.values()),
          f"boots.json iso {str(top_iso)[:12]}; per boot {[str(v)[:12] for v in isos.values()]}", "E")
    s1 = [bjs.get(k) for k in ("s1_sha256", "s1r_sha256_before_boot5", "s1_sha256_after_boot5")]
    s1_present = [v for v in s1 if v]
    if 5 in B or s1_present:
        ok = len(set(s1_present)) == 1 and (len(s1_present) == 3 or 5 not in B)
        J.chk("t1-s1-pristine", ok, f"s1 / s1r before boot 5 / s1 after boot 5: {[str(v)[:12] for v in s1]}", "E")
    chk_txt = _read_text(ev / "checker.txt")
    rcs = {}
    for line in (chk_txt or "").splitlines():
        m = re.match(r"(persist|custody|none)\s+rc=(\d+)", line.strip())
        if m:
            rcs.setdefault(m.group(1), int(m.group(2)))
    J.chk("checker-exits", rcs == T1_CHECKER_RC, f"checker.txt {rcs or 'missing'}, expected {T1_CHECKER_RC}", "E")

    # ---- disk images (findings come from the orchestrator; the judge never opens an image)
    boot2_state = None if 2 not in B else bool(B[2]["load_pass"])
    images, disk_hits = [], []
    disk_ok_names = {}
    for label, bn, expect in T1_IMAGES:
        if bn is not None and bn not in B:
            continue
        doc = _read_json(ev / ("disk-baseline.json" if bn is None else f"boot{bn}/disk-after-boot.json"))
        image_name = "D0-before" if bn is None else ("S1r" if bn == 5 else "D0")
        for name, ok, detail, cls in t1_disk_verdicts(label, doc, expect, boot2_state, image_name):
            J.chk(name, ok, detail, cls)
            disk_ok_names[name] = ok
        if isinstance(doc, dict):
            rec = doc.get("records") or {}
            scan = doc.get("scan") or {}
            disk_hits += [(label, h) for h in scan.get("hits") or []]
            images.append({
                "image": label, "disk": doc.get("image"), "sha256": doc.get("sha256"), "size": doc.get("size"),
                "expect": expect, "record_ok": (doc.get("record_verdict") or {}).get("ok"),
                "valid": [{k: e.get(k) for k in ("slot", "state", "revision", "name", "equals_predicted", "offset")}
                          for e in rec.get("valid") or []],
                "invalid": len(rec.get("invalid") or []), "v1": len(rec.get("v1") or []),
                "ciphertext_found": doc.get("ciphertext_found"), "hits": len(scan.get("hits") or []),
                "controls_ok": scan.get("controls_ok")})
            if rec.get("v1"):
                notes.append(f"{label}: {len(rec['v1'])} v1 (CKMS\\x01) record candidate(s) observed, not judged")

    # ---- secret scan of the serial and log targets (leak needles on)
    extras = [(a.password_file, "password"), (a.seed_hex_file, "seed")]
    if a.fixture_host_key:
        extras.append((a.fixture_host_key, None))
    canaries = kssh_keys.canaries(keys, extras, leak=True)
    scanner = Scanner(canaries)
    targets = [(n, bd_f) for n in ran for bd_f in (ev / f"boot{n}" / x for x in (
        "serial.raw.log", "serial.log", "sshd.log", "p09-ledger.txt", "qemu-argv.txt")) if bd_f.is_file()]
    with tempfile.TemporaryDirectory(prefix="kssh-ctl-") as td:
        tmp = Path(td)
        sample = sample_of(ev / f"boot{ran[0]}" / "serial.raw.log") if ran else b""
        pre = run_controls(scanner, sample, tmp)
        scans = [{"boot": n, "path": f"boot{n}/{p.name}", "size": p.stat().st_size, "hits": scanner.scan_file(p, joined=True)}
                 for n, p in targets]
        post = run_controls(scanner, sample, tmp)
    serial_ctl_ok = pre["ok"] and post["ok"]
    J.chk("scan-controls", serial_ctl_ok, {"pre": pre["ok"], "post": post["ok"]}, "E")
    hits = [(s["path"], h) for s in scans for h in s["hits"]]
    leak_hits = [(p, h) for p, h in hits if h["label"].startswith("leak-")]
    secret_hits = [(p, h) for p, h in hits if not h["label"].startswith("leak-")]
    J.chk("no-canary-in-targets", not secret_hits,
          [f"{p}: {h['label']}/{h['form']}@{h['offset']}" for p, h in secret_hits[:12]], "D")
    J.chk("no-ciphertext-or-header-in-serial-or-logs", not leak_hits,
          [f"{p}: {h['label']}/{h['form']}@{h['offset']}" for p, h in leak_hits[:12]], "D")
    present = {}
    blob_text = b"".join(p.read_bytes() for n, p in targets if p.name in ("serial.raw.log", "serial.log", "sshd.log"))
    public = [("kms-persist-banner", T1_PERSIST_BANNER.encode()), ("fp-a", fp_a.encode())]
    if any(B[n]["sshd_text"] is not None for n in ran):
        public.append(("accepted-publickey", b"Accepted publickey"))
    if a.policy_shape == "loaded":
        public.append(("policy-loaded-line", b"[policy] loaded"))
    for name, needle in public:
        present[name] = needle in blob_text
    J.chk("scan-expected-public-strings", all(present.values()), [k for k, v in present.items() if not v], "E")

    # ---- set-level host checks
    cc_p = ev / "census-compare.json"
    if cc_p.is_file():
        cc = json.loads(cc_p.read_text())
        J.chk("census-drift", cc["missing"] + cc["changed"] == 0 and cc["equal"] == cc["baseline_present"], cc, "E")
    elif no_value:
        notes.append("census-not-run (b0)")
    else:
        J.chk("census-drift", False, "census-not-run: census-compare.json missing", "E")
    if len(ran) == len(T1_BOOTS):
        allc = [c["cls"] for n in ran for c in B[n]["conns"]]
        got = {"accepted-pw": allc.count("accepted-pw"), "accepted-pk-a": allc.count("accepted-pk:" + fp_a),
               "accepted-pk-p": allc.count("accepted-pk:" + fp_p), "probe-ok": allc.count("probe-ok"),
               "ledger-login": sum((B[n]["ledger"] or {}).get("p09x-kssh-login", 0) for n in ran)}
        want = dict(T1_TOTALS)
        if any(B[n]["ledger"] is None for n in ran):  # a missing ledger slice is already judged by `remote-ledger`
            del got["ledger-login"], want["ledger-login"]
        J.chk("t1-five-boot-totals", got == want, f"{got}, expected {want}",
              "D" if all(B[n]["sound"] for n in ran) else "C")

    # ---- epochs and class
    def boot_through(n, markers, upto):
        st = B.get(n)
        return bool(st and all(m in st["ok_markers"] for m in markers) and st["sound"]
                    and prefix_ok("t1", st["attempts"], st["conns"], fps, upto, boot=n)
                    and any(c["cls"] == "accepted-pk:" + fp_a for c in st["conns"]))

    sign_epoch = boot_through(1, ("kssh-s00", "kssh-s01", "kssh-s02"), "s02")
    early_disk = [f"t1-disk-{k}:{lab}" for lab in ("d0-before", "after-boot1", "after-boot2") for k in ("records", "scan")]
    early_e = [n for c, n in ((c["cls"], c["name"]) for c in J.checks if not c["pass"]) if c == "E" and n.startswith(("b1:", "b2:"))]
    persist_epoch = bool(
        sign_epoch and boot_through(2, ("kssh-t1-load", "kssh-s02r"), "s02r") and B[2]["load_ok"]
        and role_ok.get(1) and role_ok.get(2) and not early_e
        and all(disk_ok_names.get(n) is True for n in early_disk))
    disk_hit = bool(disk_hits)
    canary_hit = bool(hits) or disk_hit
    controls_ok = bool(serial_ctl_ok and disk_ctl_ok)
    if unrun and not any(not c["pass"] for c in J.checks):
        J.chk("sequence-complete", False, f"boots {unrun} not run; stop recorded: {stopped}", "C" if stopped else "E")
    failed = [(c["cls"], c["name"]) for c in J.checks if not c["pass"]]
    ck = {c["name"]: c for c in J.checks}
    klass, tail = decide_class(failed, persist_epoch, controls_ok, canary_hit,
                               persist_epoch and not hits and not disk_hit and controls_ok)
    reasons = [f"{c}:{n}: {ck[n]['detail']}" for c, n in failed]
    if tail:
        reasons.insert(0, "tail STOP after the first persistence epoch: later scenarios unproven")
    unproven = [f"boot{n}: not run" for n in unrun]
    if klass == "B":
        unproven += [n for _, n in failed]

    boots_out = {}
    for n in T1_BOOTS:
        if n not in B:
            boots_out[str(n)] = {"ran": False}
            continue
        st = B[n]
        prefix = f"b{n}:"
        boots_out[str(n)] = {
            "ran": True,
            "checks": {nm: {"pass": ck[prefix + nm]["pass"], "detail": ck[prefix + nm]["detail"]} for nm in st["names"]},
            "markers_passed": sorted(st["ok_markers"], key=T1_MARKERS[n].index),
            "attempts": st["attempts"], "t1_ms": st["t1_ms"], "t1_load": st["t1_load"],
            "t1_opaque": st.get("t1_opaque"), "t1_import": st["imports"], "role_obs": st["role_obs"],
            "role_file": st["role_file"], "ledger": st["ledger"], "policy_loaded_line": st["policy_loaded"],
            "sshd_connections": [{"idx": c["idx"], "port": c["port"], "listener": c["lport"], "class": c["cls"],
                                  "events": [e for e, _ in c["events"]]} for c in st["conns"]],
            "sshd_ignored_lines": st["parsed"]["ignored"],
            "obs": [{"id": i, "kv": k} for i, ks in st["obs"].items() for k in ks if i not in ("s01", "t1-load")]}
    outputs = {
        "verdict.json": {
            "set": setname, "mode": mode, "policy_shape": a.policy_shape, "no_evidence_value": no_value,
            "checks": {c["name"]: {"pass": c["pass"], "detail": c["detail"]} for c in J.checks},
            "sequence": {"planned": len(T1_BOOTS), "boots_run": ran, "boots_unrun": unrun, "stopped": stopped},
            "boots": boots_out, "images": images,
            "observations": {"t1_ms": {str(n): B[n]["t1_ms"] for n in ran}, "fixture_fp": a.fixture_fp,
                             "comment_len": exp.get("comment_len"),
                             "comment_via": (exp.get("record") or {}).get("comment_via")},
            "notes": notes},
        "classification.json": {"class": klass, "reasons": reasons, "first_sign_epoch_passed": sign_epoch,
                                "first_persistence_epoch_passed": persist_epoch, "boots_run": ran,
                                "unproven": unproven, "no_evidence_value": no_value},
        "scan.json": {"canaries": len(canaries), "forms": len(scanner.needles), "targets": scans, "images": images,
                      "expected_public_strings": present, "judge_outputs_scanned": True, "no_evidence_value": no_value},
        "controls.json": {"serial": {"pre": pre, "post": post, "ok": serial_ctl_ok}, "disk": disk_ctl,
                          "disk_ok": disk_ctl_ok, "ok": controls_ok, "no_evidence_value": no_value},
    }
    leaked = 0
    for name in list(outputs):
        outputs[name], k = redact_obj(outputs[name], scanner)
        leaked += k
    if leaked:
        outputs["classification.json"]["reasons"].append(f"D:output-contained-canary: {leaked} string(s) redacted")
        if klass in ("A", "B"):
            outputs["classification.json"]["class"] = klass = "D"
    for name, obj in outputs.items():
        (ev / name).write_text(json.dumps(obj, indent=2) + "\n")
    post_hits = []
    for name in outputs:
        post_hits += [(name, h) for h in scanner.scan_file(ev / name, joined=True)]
    if post_hits:
        for name, _ in post_hits:
            (ev / name).unlink(missing_ok=True)
        (ev / "classification.json").write_text(json.dumps(
            {"class": "D", "reasons": ["D:judge-output-canary-leak (files removed)"], "first_sign_epoch_passed": sign_epoch,
             "first_persistence_epoch_passed": persist_epoch, "no_evidence_value": no_value}, indent=2) + "\n")
        print("FAIL judge-output-canary-leak  <- a judge output contained a canary; files removed", file=sys.stderr)
        return 1
    for c in J.checks:
        detail = redact_obj(c["detail"], scanner)[0]
        print(("PASS " if c["pass"] else "FAIL ") + c["name"] + ("" if c["pass"] else f"  <- {detail}"))
    for note in notes:
        print("NOTE " + redact_obj(note, scanner)[0])
    print(f"CLASS {klass}{' (no evidence value: b0)' if no_value else ''} first_sign_epoch_passed={sign_epoch}"
          f" first_persistence_epoch_passed={persist_epoch} boots_run={ran} checks={len(J.checks)} failed={len(failed)}")
    return 0 if klass == "A" else 1



# =========================================================================== census

def _sha256_file(path, bufsize=1 << 20):
    h, size = hashlib.sha256(), 0
    with open(path, "rb") as f:
        while True:
            b = f.read(bufsize)
            if not b:
                break
            h.update(b)
            size += len(b)
    return {"size": size, "sha256": h.hexdigest()}


def census_hash(paths) -> dict:
    present, missing = {}, []
    for p in paths:
        try:
            present[p] = _sha256_file(p)
        except OSError:
            missing.append(p)
    return {"paths": len(paths), "method": "protected census: {path: {size, sha256}} (same as common.protected_paths)",
            "present": present, "missing": missing}


def census_compare(base: dict, now: dict) -> dict:
    equal = changed = missing = 0
    for p, v in base["present"].items():
        cur = now["present"].get(p)
        if cur is None:
            missing += 1
        elif cur == v:
            equal += 1
        else:
            changed += 1
    return {"equal": equal, "missing": missing, "changed": changed, "baseline_present": len(base["present"])}


def census(a) -> int:
    base = json.loads(Path(a.baseline).read_text())
    paths = list(base["present"])
    roots = {os.path.dirname(os.path.realpath(p)) for p in paths}
    for out in (a.out, a.compare_baseline):
        if out:
            d = os.path.dirname(os.path.realpath(out))
            if any(d == r or d.startswith(r + os.sep) for r in roots) or os.path.realpath(out) in {os.path.realpath(p) for p in paths}:
                print(f"census: refusing to write inside a census path: {out}", file=sys.stderr)
                return 2
    now = census_hash(paths)
    if a.out:
        Path(a.out).write_text(json.dumps(now, indent=1) + "\n")
    res = census_compare(base, now)
    if a.compare_baseline:
        Path(a.compare_baseline).write_text(json.dumps(res, indent=1) + "\n")
    print(f"census: equal={res['equal']} missing={res['missing']} changed={res['changed']} baseline_present={res['baseline_present']}")
    return 0 if res["missing"] + res["changed"] == 0 and res["equal"] == res["baseline_present"] else 1


# =========================================================================== archive

ALLOW = ["serial.raw.log", "serial.log", "sshd.log", "verdict.json", "scan.json", "controls.json",
         "classification.json", "qemu-argv.txt", "provenance.sha256", "fat-layout.txt", "fixture.txt",
         "census-compare.json", "expected.json", "p09-*.json", "p09-*.txt", "*.pub"]
FORBIDDEN = ["id_*", "*.pem", "*.key", "*.img", "*.iso", "seed.hex"]
# Mode t1 (contract section 9): extra top-level files and the per-boot allowlist (never an image, never a key).
ALLOW_T1 = ["boots.json", "disk-baseline.json", "checker.txt", "preflight*.txt", "census-*.json",
            "f1-waiver-unsafe-allowlist.diff", "provenance*", "frozen-trees*.txt", "cellbox-ceiling.txt", "sshd-full.log"]
BOOT_ALLOW = ["serial.raw.log", "serial.log", "sshd.log", "p09-ledger.txt", "qemu-argv.txt", "role.txt", "boot.json",
              "disk-after-boot.json"]
BOOT_DIR_RE = re.compile(r"^boot[1-9][0-9]*$")


def _match(name, pats):
    return any(fnmatch.fnmatchcase(name, p) for p in pats)


def archive(src, dst, keys, extra_secret_files=(), conf=None) -> tuple[bool, list]:
    """Allowlist copy + full canary scan with controls. On refusal `dst` is deleted. (ok, messages)

    Mode t1 (detected from the key set's expected.json): also copies `boot*/` with BOOT_ALLOW and ALLOW_T1, writes
    `expected-public.json` (no ciphertext) instead of `expected.json`, and scans with the leak needles."""
    src, dst, keys = Path(src), Path(dst), Path(keys)
    msgs: list = []
    if dst.exists() and (not dst.is_dir() or any(dst.iterdir())):
        return False, [f"destination {dst} exists and is not empty: refused (nothing deleted)"]
    created = not dst.exists()
    dst.mkdir(parents=True, exist_ok=True)

    def refuse(m):
        shutil.rmtree(dst, ignore_errors=True)
        return False, msgs + [m]

    exp = _read_json(keys / "expected.json") if keys.is_dir() else None
    t1 = isinstance(exp, dict) and exp.get("mode") == "t1"
    allow = ALLOW + ALLOW_T1 if t1 else ALLOW
    sources = [(p, p.name) for p in sorted(src.iterdir()) if p.is_file() and not p.is_symlink()]
    if keys.is_dir():
        sources += [(p, p.name) for p in sorted(keys.iterdir()) if p.is_file() and not p.is_symlink()
                    and (p.name.endswith(".pub") or (p.name == "expected.json" and not t1))]
    conf_src = Path(conf) if conf else (src / "kssh.conf")
    copied = set()
    for p, name in sources:
        if t1 and name == "expected.json":
            continue
        if _match(name, FORBIDDEN):
            if _match(name, allow):
                return refuse(f"forbidden name in allowlist match: {name}")
            continue
        if _match(name, allow) and name not in copied:
            shutil.copyfile(p, dst / name)
            copied.add(name)
    if t1:
        for bd in sorted(src.iterdir()):
            if not (bd.is_dir() and not bd.is_symlink() and BOOT_DIR_RE.match(bd.name)):
                continue
            (dst / bd.name).mkdir()
            for p in sorted(bd.iterdir()):
                if not (p.is_file() and not p.is_symlink()):
                    continue
                if _match(p.name, FORBIDDEN):
                    if _match(p.name, BOOT_ALLOW):
                        return refuse(f"forbidden name in allowlist match: {bd.name}/{p.name}")
                    continue
                if _match(p.name, BOOT_ALLOW):
                    shutil.copyfile(p, dst / bd.name / p.name)
        (dst / "expected-public.json").write_text(json.dumps(kssh_keys.public_expected(exp), indent=2) + "\n")
    if conf_src.is_file():
        (dst / "kssh.conf").write_text(kssh_keys.redact_conf(conf_src.read_text()))
    for p in dst.rglob("*"):
        if _match(p.name, FORBIDDEN):
            return refuse(f"forbidden name in destination: {p.name}")
    extras = [(f, "password" if "password" in Path(f).name.lower() else None) for f in extra_secret_files]
    scanner = Scanner(kssh_keys.canaries(keys, extras, leak=t1))
    with tempfile.TemporaryDirectory(prefix="kssh-arch-") as td:
        sample_p = dst / "serial.raw.log"
        if not sample_p.is_file():
            sample_p = next(iter(sorted(dst.glob("boot*/serial.raw.log"))), sample_p)
        sample = sample_of(sample_p)
        ctl = run_controls(scanner, sample, Path(td))
    if not ctl["ok"]:
        return refuse("scanner controls failed on the archive scan")
    hits = [(p.name, h) for p in sorted(dst.rglob("*")) if p.is_file() for h in scanner.scan_file(p)]
    if hits:
        return refuse("canary in archive: " + ", ".join(f"{n}:{h['label']}/{h['form']}" for n, h in hits[:12]))
    return True, msgs + [f"archive ok: {len(list(dst.iterdir()))} files, {len(scanner.needles)} needles, controls ok"]


# =========================================================================== selftest

def selftest() -> list:
    """Scanner controls on synthetic canaries (never real keys). [(name, ok, why)]"""
    import secrets as _s
    out = []
    can = [("pass:synthetic", _s.token_hex(12).encode()), ("wrong:synthetic", _s.token_hex(12).encode()),
           ("seed:synthetic", _s.token_bytes(32)), ("expanded:synthetic", _s.token_bytes(64)),
           ("armor:synthetic", b"-----BEGIN OPENSSH PRIVATE KEY-----\n" + _s.token_hex(80).encode() + b"\n-----END OPENSSH PRIVATE KEY-----"),
           ("body:synthetic", _s.token_hex(40).encode())]
    sc = Scanner(can)
    with tempfile.TemporaryDirectory(prefix="kssh-selftest-") as td:
        tmp = Path(td)
        sample = b"USER: P04 INFO benign serial line\n" * 100
        ctl = run_controls(sc, sample, tmp)
        out.append(("plant-detect-all-forms", not ctl["all-forms"]["missing"], ctl["all-forms"]["missing"][:5]))
        out.append(("plant-detect-small-chunks", not ctl["all-forms-small-chunk"]["missing"], ctl["all-forms-small-chunk"]["missing"][:5]))
        out.append(("plant-detect-wrapped", not ctl["wrapped"]["missing"], ctl["wrapped"]["missing"]))
        out.append(("near-miss-not-detected", not ctl["decoys"]["false_hits"], ctl["decoys"]["false_hits"][:5]))
        clean = tmp / "clean.log"
        clean.write_bytes(sample)
        out.append(("clean-sample-no-hit", not sc.scan_file(clean), "clean sample reported a hit"))
        dirty = tmp / "dirty.log"
        dirty.write_bytes(sample + can[0][1])
        out.append(("single-plant-detected", any(h["label"] == "pass:synthetic" for h in sc.scan_file(dirty)),
                    "planted passphrase missed"))
        # mode t1 leak needles (ciphertext, 8-byte record headers): planted in a serial-like sample, every form found
        leak = Scanner([("leak-ct:synthetic", _s.token_bytes(300)), ("leak-hdr:a", b"CKMS\x02\x00\x01\x01"),
                        ("leak-hdr:b", b"CKMS\x02\x01\x01\x00")])
        lctl = run_controls(leak, sample, tmp)
        out.append(("t1-leak-needles-all-forms", lctl["ok"] and not lctl["all-forms"]["missing"],
                    lctl["all-forms"]["missing"][:5]))
        a1 = "-chardev socket,id=ser0,path=/tmp/a/s1.sock,server=on -drive file=/tmp/a/D0.img,if=none,format=raw"
        a2 = "-chardev socket,id=ser0,path=/tmp/b/s5.sock,server=on -drive file=/tmp/b/S1r.img,if=none,format=raw"
        out.append(("t1-argv-equal-modulo-disk-normalisation", t1_norm_argv(a1) == t1_norm_argv(a2)
                    and t1_norm_argv(a1) != t1_norm_argv(a2.replace("-drive", "-drive id=x -drive")), "normalisation"))
    return out


# =========================================================================== CLI

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    j = sub.add_parser("judge")
    j.add_argument("--evidence", required=True)
    j.add_argument("--keys", required=True)
    j.add_argument("--mode", required=True, choices=("t0", "f", "t1"))
    j.add_argument("--set", required=True, choices=("b0", "rt0", "rt0f", "rt1"))
    j.add_argument("--fixture-fp", required=True)
    j.add_argument("--password-file", required=True)
    j.add_argument("--seed-hex-file", required=True)
    j.add_argument("--fixture-host-key")
    j.add_argument("--trust-disk")
    j.add_argument("--sshd-log")
    j.add_argument("--policy-shape", choices=("loaded", "absent"), default="loaded")
    c = sub.add_parser("census")
    c.add_argument("--baseline", required=True)
    c.add_argument("--out")
    c.add_argument("--compare-baseline", metavar="CMP.json")
    r = sub.add_parser("archive")
    r.add_argument("--src", required=True)
    r.add_argument("--dst", required=True)
    r.add_argument("--keys", required=True)
    r.add_argument("--extra-secret-file", action="append", default=[])
    r.add_argument("--conf")
    sub.add_parser("selftest")
    a = ap.parse_args(argv)
    if a.cmd == "judge":
        return judge(a)
    if a.cmd == "census":
        return census(a)
    if a.cmd == "archive":
        ok, msgs = archive(a.src, a.dst, a.keys, a.extra_secret_file, a.conf)
        for m in msgs:
            print(("PASS " if ok else "FAIL ") + m)
        return 0 if ok else 1
    res = selftest()
    for name, ok, why in res:
        print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"  <- {why}"))
    return 0 if all(ok for _, ok, _ in res) else 1


if __name__ == "__main__":
    sys.exit(main())
