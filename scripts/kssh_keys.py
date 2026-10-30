#!/usr/bin/env python3
"""KSSH K8 lane: run-time TEST key material, `kssh.conf`, and canary derivation.

    python3 scripts/kssh_keys.py gen  --dir KEYS --stage STAGE [--calibrate] [--mode t0|f|t1]
    python3 scripts/kssh_keys.py conf --dir KEYS --stage STAGE --mode t0|f|t1 --ports MAIN REKEY B \
            --hostfp SHA256:.. --password-file F [--calibrate]

Stdlib plus the host `ssh-keygen` (OpenSSH 9.x). Everything is TEST material that exists for
one evidence run. `--dir` is refused when it lies inside a git work tree or is not mode 700,
`umask 077` is set, and nothing is ever printed that came from a secret.

Importable (used by kssh_judge.py): `load_secrets`, `canaries`, `forms`, `parse_conf`,
`blob_prefix`, `parse_container`, `decode_armor`, `public_expected`.

Mode t1 (RT1, K4b persistent custody): keys `ka16` (wrapped, rounds 16, 1,900-byte comment) and
`kp0` (plain) plus the two malformed variants, no key B, no kl4, no calibration keys; `expected.json`
gains `mode: t1` and the `record` expectation the host record oracle predicts from.

Known host-side exposure (TEST keys only): ssh-keygen receives a passphrase through `-N`/`-P`
on its argument vector, visible in the process list for a few milliseconds on this host.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import secrets
import string
import subprocess
import sys
from pathlib import Path

MAGIC = b"openssh-key-v1\0"
ALNUM = string.ascii_letters + string.digits
PASS_LEN = 24
RAW_LEN = 4096
MIN_CANARY = 8
HOSTFP_RE = re.compile(r"^SHA256:[A-Za-z0-9+/]{43}$")
WRAPPED_ROUNDS = {"ka16": 16, "kl4": 4, "kr1": 1, "kr64": 64}
MAIN_KEYS = ("ka16", "kl4", "kp0")
CAL_KEYS = ("kr1", "kr64")
BAD_KEYS = {"bad-aes128": "aes128-ctr", "bad-chacha": "chacha20-poly1305@openssh.com"}
KEY_FILE_NAMES = MAIN_KEYS + CAL_KEYS + tuple(BAD_KEYS)
T1_KEYS = ("ka16", "kp0")
T1_COMMENT_LEN = 1900
OSSH_BLOCK_PLAIN = 8
# Serial/log leak needles of mode t1: the 8-byte K4b record headers (magic `CKMS`, version 2, slot, kind, state).
LEAK_HEADERS = (("leak-hdr:a", b"CKMS\x02\x00\x01\x01"), ("leak-hdr:b", b"CKMS\x02\x01\x01\x00"))


class KeyError_(Exception):
    """Operator-facing failure; the message never contains secret material."""


# --------------------------------------------------------------------------- wire parsing

class _Wire:
    def __init__(self, data: bytes):
        self.data, self.at = data, 0

    def u32(self) -> int:
        if self.at + 4 > len(self.data):
            raise KeyError_("openssh-key-v1: truncated")
        v = int.from_bytes(self.data[self.at:self.at + 4], "big")
        self.at += 4
        return v

    def string(self) -> bytes:
        n = self.u32()
        if self.at + n > len(self.data):
            raise KeyError_("openssh-key-v1: truncated string")
        v = self.data[self.at:self.at + n]
        self.at += n
        return v

    def rest(self) -> bytes:
        return self.data[self.at:]


def decode_armor(text: bytes) -> bytes:
    """The binary blob of one armored OPENSSH PRIVATE KEY block (what the admin cell hashes)."""
    lines = [l for l in text.replace(b"\r\n", b"\n").split(b"\n") if l != b""]
    if len(lines) < 3 or lines[0] != b"-----BEGIN OPENSSH PRIVATE KEY-----" \
            or lines[-1] != b"-----END OPENSSH PRIVATE KEY-----":
        raise KeyError_("not one armored OPENSSH PRIVATE KEY block")
    body = b"".join(lines[1:-1])
    try:
        blob = base64.b64decode(body, validate=True)
    except (ValueError, base64.binascii.Error):
        raise KeyError_("armor body is not base64") from None
    if base64.b64encode(blob) != body:
        raise KeyError_("armor body is not canonical base64")
    return blob


def blob_prefix(blob: bytes) -> str:
    """First 8 bytes of SHA-256 over the decoded blob, 16 lowercase hex (armor.rs/import.rs/report.rs)."""
    return hashlib.sha256(blob).digest()[:8].hex()


def parse_container(blob: bytes) -> dict:
    """Parse openssh-key-v1. Unencrypted private sections are parsed too (`seed`, `pub`, `comment_len`);
    encrypted ones return `salt`, `rounds` and `ct` (the encrypted private section, without any AEAD tag)."""
    if not blob.startswith(MAGIC):
        raise KeyError_("openssh-key-v1: bad magic")
    w = _Wire(blob[len(MAGIC):])
    cipher, kdf, kdfopts = w.string().decode(), w.string().decode(), w.string()
    nkeys = w.u32()
    pubblob = w.string()
    priv = w.string()
    tag = 16 if "chacha20-poly1305" in cipher or "gcm" in cipher else 0  # AEAD tag follows the section
    if nkeys != 1 or len(w.rest()) != tag:
        raise KeyError_("openssh-key-v1: expected exactly one key and no trailing bytes")
    out = {"cipher": cipher, "kdf": kdf, "rounds": 0, "pubblob": pubblob}
    if kdf == "bcrypt":
        o = _Wire(kdfopts)
        out["salt"], out["rounds"] = o.string(), o.u32()
    elif kdf != "none":
        raise KeyError_("openssh-key-v1: unknown kdf")
    if cipher != "none":
        out["ct"] = priv
    pw = _Wire(pubblob)
    if pw.string() != b"ssh-ed25519":
        raise KeyError_("not an ed25519 key")
    out["pub"] = pw.string()
    if cipher == "none":
        p = _Wire(priv)
        c1, c2 = p.u32(), p.u32()
        if c1 != c2:
            raise KeyError_("openssh-key-v1: check integers differ")
        if p.string() != b"ssh-ed25519":
            raise KeyError_("openssh-key-v1: key type mismatch")
        pub, expanded, comment = p.string(), p.string(), p.string()
        pad = p.rest()
        if len(expanded) != 64 or expanded[32:] != pub or pub != out["pub"] \
                or pad != bytes(range(1, len(pad) + 1)):
            raise KeyError_("openssh-key-v1: inconsistent private section")
        out["seed"], out["expanded"] = expanded[:32], expanded
        out["comment_len"] = len(comment)
    return out


def _wire_string(b: bytes) -> bytes:
    return len(b).to_bytes(4, "big") + b


def plain_container_with_comment(parsed: dict, comment: bytes) -> bytes:
    """Armored unencrypted openssh-key-v1 of the key in `parsed` (from `parse_container`) with `comment`.

    ssh-keygen -C silently truncates the comment at 1,023 bytes on OpenSSH 9.6; the container is plain
    wire format, so the longer comment is written here and ssh-keygen -p then wraps the result.
    """
    check = secrets.token_bytes(4)
    sec = check + check + _wire_string(b"ssh-ed25519") + _wire_string(parsed["pub"]) \
        + _wire_string(parsed["expanded"]) + _wire_string(comment)
    sec += bytes(range(1, (-len(sec) % OSSH_BLOCK_PLAIN) + 1))
    blob = MAGIC + _wire_string(b"none") + _wire_string(b"none") + _wire_string(b"") + (1).to_bytes(4, "big") \
        + _wire_string(parsed["pubblob"]) + _wire_string(sec)
    body = base64.b64encode(blob)
    lines = [body[i:i + 70] for i in range(0, len(body), 70)]
    return b"-----BEGIN OPENSSH PRIVATE KEY-----\n" + b"\n".join(lines) + b"\n-----END OPENSSH PRIVATE KEY-----\n"


def fp_of_pub_line(line: str) -> str:
    blob = base64.b64decode(line.split()[1])
    return "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")


# --------------------------------------------------------------------------- directories

def _in_git_tree(path: Path) -> bool:
    p = path.resolve()
    for d in (p, *p.parents):
        if (d / ".git").exists():
            return True
    return False


def prepare_dir(path, *, strict_mode: bool, what: str) -> Path:
    p = Path(path)
    if _in_git_tree(p):
        raise KeyError_(f"{what} {p} is inside a git work tree: refused")
    if p.exists():
        st = p.stat()
        if not p.is_dir():
            raise KeyError_(f"{what} {p} is not a directory")
        if strict_mode and ((st.st_mode & 0o777) != 0o700 or st.st_uid != os.getuid()):
            raise KeyError_(f"{what} {p} must be mode 700 and owned by the current user")
    else:
        os.makedirs(p, mode=0o700)
    return p.resolve()


def _write_new(path: Path, data: bytes, mode: int = 0o600, exclusive: bool = True) -> None:
    flags = os.O_WRONLY | os.O_CREAT | (os.O_EXCL if exclusive else os.O_TRUNC)
    fd = os.open(path, flags, mode)
    with os.fdopen(fd, "wb") as f:
        f.write(data)


def _wipe(path: Path) -> None:
    try:
        n = path.stat().st_size
        with open(path, "r+b") as f:
            f.write(b"\0" * n)
            f.flush()
            os.fsync(f.fileno())
    except OSError:
        pass
    path.unlink(missing_ok=True)


# --------------------------------------------------------------------------- ssh-keygen

def _keygen(*args: str, text: bool = True) -> str:
    """Run ssh-keygen; errors never echo the argument vector (it may hold a passphrase)."""
    try:
        r = subprocess.run(["ssh-keygen", *args], stdin=subprocess.DEVNULL, capture_output=True, check=False)
    except FileNotFoundError:
        raise KeyError_("ssh-keygen not found") from None
    if r.returncode != 0:
        first = r.stderr.decode(errors="replace").strip().splitlines()[:1]
        raise KeyError_(f"ssh-keygen {args[0]} failed rc={r.returncode}: {first[0] if first else ''}")
    return r.stdout.decode()


def ssh_fingerprint(pub_path: Path) -> str:
    fields = _keygen("-l", "-f", str(pub_path)).split()
    if len(fields) < 2 or not HOSTFP_RE.match(fields[1]):
        raise KeyError_("unexpected ssh-keygen -l output")
    return fields[1]


def _random_alnum(n: int = PASS_LEN) -> str:
    return "".join(secrets.choice(ALNUM) for _ in range(n))


def raw_bin(nonce: str) -> bytes:
    buf, i = b"", 0
    while len(buf) < RAW_LEN:
        buf += hashlib.sha256(b"kssh-raw/" + nonce.encode() + i.to_bytes(4, "big")).digest()
        i += 1
    raw = bytearray(buf[:RAW_LEN])
    raw[17], raw[RAW_LEN // 2], raw[RAW_LEN - 7] = 0x00, 0xFF, 0xFF
    return bytes(raw)


def _random_comment(n: int) -> str:
    """`n` printable ASCII bytes, fresh per run (letters, digits and a few separators; no spaces or quotes)."""
    return "".join(secrets.choice(ALNUM + "._-+=:") for _ in range(n))


def _gen_plain(work: Path, comment_len: int = 0, info: dict | None = None) -> tuple[bytes, dict, str]:
    """A fresh unencrypted ed25519 key at `work`; returns (armored text, parsed, public key line).

    `comment_len`: wanted private-section comment length (0: empty comment). Tried with `ssh-keygen -C`; when
    that does not give exactly this length (OpenSSH 9.6 truncates at 1,023 bytes) the unencrypted container is
    rewritten with the full comment, and when that does not verify either an ordinary comment is used. `info`
    receives `comment_len` (what the key really carries) and `comment_via`.
    """
    def fresh(comment: str):
        _keygen("-q", "-t", "ed25519", "-N", "", "-C", comment, "-f", str(work))
        Path(str(work) + ".pub").unlink(missing_ok=True)
        text = work.read_bytes()
        return text, parse_container(decode_armor(text))

    via = "empty"
    if comment_len:
        try:
            text, parsed = fresh(_random_comment(comment_len))
            via = "ssh-keygen-C"
            if parsed["comment_len"] != comment_len:
                work.unlink()
                text, parsed = fresh("")
                text = plain_container_with_comment(parsed, _random_comment(comment_len).encode())
                _write_new(work, text, exclusive=False)
                parsed = parse_container(decode_armor(text))
                via = "container-rewrite"
            if parsed["comment_len"] != comment_len:
                raise KeyError_("comment length not reached")
            _keygen("-y", "-f", str(work))  # ssh-keygen reads the result back
        except KeyError_:
            if work.exists():
                work.unlink()
            via = "ordinary-fallback"
            text, parsed = fresh("kssh-comment")
    else:
        text, parsed = fresh("")
    if info is not None:
        info.update({"comment_len": parsed["comment_len"], "comment_via": via})
    if parsed["cipher"] != "none" or parsed["kdf"] != "none":
        raise KeyError_("ssh-keygen -N '' did not write an unencrypted key")
    line = _keygen("-y", "-f", str(work)).split()
    pub_line = f"{line[0]} {line[1]}"
    if base64.b64decode(line[1]) != parsed["pubblob"]:
        raise KeyError_("public key line does not match the private container")
    return text, parsed, pub_line


def _build_key(keys: Path, name: str, rounds: int, cipher: str | None, passphrase: str | None,
               sec: dict, exp: dict, comment_len: int = 0) -> str:
    """Generate one key. `rounds == 0`: plain. Returns the public key line."""
    work = keys / f".work-{name}"
    final = keys / name
    try:
        cinfo: dict = {}
        text, plain, pub_line = _gen_plain(work, comment_len, cinfo)
        seed = plain["seed"].hex()
        if passphrase is None:
            os.replace(work, final)
            wanted_cipher, wanted_kdf = "none", "none"
        else:
            os.replace(work, final)
            args = ["-q", "-p", "-a", str(rounds or 16), "-N", passphrase, "-f", str(final)]
            if cipher:
                args[2:2] = ["-Z", cipher]
            _keygen(*args)
            wanted_cipher, wanted_kdf = cipher or "aes256-ctr", "bcrypt"
        ftext = final.read_bytes()
        blob = decode_armor(ftext)
        got = parse_container(blob)
        if (got["cipher"], got["kdf"]) != (wanted_cipher, wanted_kdf) or got["pubblob"] != plain["pubblob"]:
            raise KeyError_(f"{name}: result does not parse as {wanted_cipher}/{wanted_kdf} of the generated key")
        if passphrase is not None:
            if got["rounds"] != (rounds or 16):
                raise KeyError_(f"{name}: kdf rounds {got['rounds']} != {rounds or 16}")
            back = _keygen("-y", "-P", passphrase, "-f", str(final)).split()
            if f"{back[0]} {back[1]}" != pub_line:
                raise KeyError_(f"{name}: re-encrypted key does not unlock to the generated public key")
        (keys / f"{name}.pub").write_text(f"{pub_line} kssh-{name}\n")
        fp = ssh_fingerprint(keys / f"{name}.pub")
        if fp != fp_of_pub_line(pub_line):
            raise KeyError_(f"{name}: fingerprint cross-check failed")
        sec["keys"][name] = {"passphrase": passphrase, "wrong": None, "seed": seed}
        exp["keys"][name] = {
            "fp": fp, "pub": f"{pub_line} kssh-{name}", "blob_prefix": blob_prefix(blob), "blob_len": len(blob),
            "rounds": got["rounds"], "cipher": got["cipher"], "kdf": got["kdf"],
            "custody": "plain" if passphrase is None else "passphrase-wrapped",
        }
        if comment_len:
            exp["keys"][name].update(cinfo)
        return pub_line
    finally:
        if work.exists():
            _wipe(work)


def gen(keys_dir, stage_dir, calibrate: bool = False, mode: str = "t0") -> dict:
    """Generate one key set. `mode` t1 (RT1): ka16 + kp0 + the two malformed variants, `record` expectation,
    no key B / kl4 / calibration keys; t0 and f are the same set (the T0 behaviour)."""
    if mode not in ("t0", "f", "t1"):
        raise KeyError_("mode must be t0, f or t1")
    t1 = mode == "t1"
    if t1 and calibrate:
        raise KeyError_("--calibrate is refused in mode t1")
    old = os.umask(0o077)
    try:
        keys = prepare_dir(keys_dir, strict_mode=True, what="--dir")
        stage = prepare_dir(stage_dir, strict_mode=False, what="--stage")
        if (keys / "secrets.json").exists():
            raise KeyError_(f"{keys} already holds a key set: refused (fresh keys per run)")
        authdir = stage / "authdir"
        os.makedirs(authdir, mode=0o700, exist_ok=True)
        nonce = secrets.token_hex(8)
        sec = {"version": 1, "nonce": nonce, "keys": {}}
        exp = {"version": 1, "nonce": nonce, "keys": {}, "calibrate": bool(calibrate)}
        if t1:
            exp["mode"] = "t1"
        main_keys = T1_KEYS if t1 else MAIN_KEYS + (CAL_KEYS if calibrate else ())
        created: list[Path] = []
        try:
            pubs = {}
            for name in main_keys:
                if name == "kp0":
                    pubs[name] = _build_key(keys, name, 0, None, None, sec, exp)
                else:
                    pubs[name] = _build_key(keys, name, WRAPPED_ROUNDS[name], None, _random_alnum(), sec, exp,
                                            T1_COMMENT_LEN if t1 and name == "ka16" else 0)
                    sec["keys"][name]["wrong"] = _random_alnum()
                created.append(keys / name)
            for name, cipher in BAD_KEYS.items():
                _build_key(keys, name, 16, cipher, _random_alnum(), sec, exp)
                created.append(keys / name)
            if t1:
                got = parse_container(decode_armor((keys / "ka16").read_bytes()))
                ka = exp["keys"]["ka16"]
                exp["comment_len"] = ka["comment_len"]
                exp["record"] = {"name": "ka16", "pub": got["pub"].hex(), "rounds": got["rounds"],
                                 "salt": got["salt"].hex(), "ct": got["ct"].hex(), "blob_prefix": ka["blob_prefix"],
                                 "comment_len": ka["comment_len"], "comment_via": ka["comment_via"]}
            else:
                # Key B: public half only; the private half never survives this block.
                work = keys / ".work-kb"
                try:
                    _text, _parsed, kb_line = _gen_plain(work)
                finally:
                    _wipe(work)
                (keys / "kb.pub").write_text(f"{kb_line} kssh-kb\n")
                exp["keyb"] = {"fp": ssh_fingerprint(keys / "kb.pub"), "pub": f"{kb_line} kssh-kb"}
            raw = raw_bin(nonce)
            exp["raw"] = {"sha256": hashlib.sha256(raw).hexdigest(), "len": len(raw)}
            _write_new(stage / "raw.bin", raw)
            for name in list(main_keys) + list(BAD_KEYS):
                _write_new(stage / f"{name}.key", (keys / name).read_bytes())
            _write_new(authdir / "authorized_keys_main", "".join(
                exp["keys"][n]["pub"] + "\n" for n in main_keys).encode(), exclusive=False)
            # the unchanged fixture helper wants a non-empty second file: in t1 it repeats ka16 and is unused
            auth_b = exp["keys"]["ka16"]["pub"] if t1 else exp["keyb"]["pub"]
            _write_new(authdir / "authorized_keys_b", (auth_b + "\n").encode(), exclusive=False)
            _write_new(keys / "secrets.json", json.dumps(sec, indent=2).encode() + b"\n")
            _write_new(keys / "expected.json", json.dumps(exp, indent=2).encode() + b"\n")
        except BaseException:
            for p in created + [keys / ".work-kb"]:
                if p.exists():
                    _wipe(p)
            raise
        return exp
    finally:
        os.umask(old)


# --------------------------------------------------------------------------- conf

def load_secrets(keys_dir) -> dict:
    return json.loads((Path(keys_dir) / "secrets.json").read_text())


def load_expected(keys_dir) -> dict:
    return json.loads((Path(keys_dir) / "expected.json").read_text())


def build_conf(keys_dir, mode: str, ports, hostfp: str, password: str, calibrate: bool) -> str:
    if mode not in ("t0", "f", "t1"):
        raise KeyError_("mode must be t0, f or t1")
    ports = [int(p) for p in ports]
    if len(ports) != 3 or not all(0 < p < 65536 for p in ports):
        raise KeyError_("ports must be MAIN REKEY B in 1..65535")
    if not HOSTFP_RE.match(hostfp):
        raise KeyError_("hostfp must look like SHA256:<43 base64 chars>")
    if not password or re.search(r"\s", password):
        raise KeyError_("password must be non-empty and contain no whitespace")
    sec, exp = load_secrets(keys_dir), load_expected(keys_dir)
    t1 = mode == "t1"
    if t1 != (exp.get("mode") == "t1"):
        raise KeyError_("the key set mode (t1 or t0/f) does not match --mode")
    if calibrate and t1:
        raise KeyError_("--calibrate is refused in mode t1")
    if calibrate and not exp.get("calibrate"):
        raise KeyError_("--calibrate: the key set was generated without calibration keys")
    out = ["kssh-conf 1", f"mode {mode}", f"calibrate {int(bool(calibrate))}",
           "ports {} {} {}".format(*ports), f"hostfp {hostfp}", f"password {password}",
           f"nonce {sec['nonce']}", f"raw sha256={exp['raw']['sha256']} len={exp['raw']['len']}"]
    for name in T1_KEYS if t1 else MAIN_KEYS + (CAL_KEYS if calibrate else ()):
        k, e = sec["keys"][name], exp["keys"][name]
        pw, wrong = k["passphrase"], k["wrong"]
        if (name == "kp0") != (pw is None):
            raise KeyError_(f"{name}: passphrase presence does not match its custody")
        out.append(f"key {name} {e['fp']} {pw or '-'} {wrong or '-'} {e['blob_prefix']} {e['rounds']}")
    if not t1:
        out.append(f"keyb {exp['keyb']['fp']}")
    text = "\n".join(out) + "\n"
    if any(len(l.encode()) >= 400 for l in out):
        raise KeyError_("a conf line is 400 bytes or longer")
    return text


def parse_conf(text: str) -> dict:
    """Inverse of build_conf (the Rust driver's reading, in Python)."""
    d: dict = {"keys": {}}
    lines = text.splitlines()
    if not lines or lines[0] != "kssh-conf 1":
        raise KeyError_("conf: bad first line")
    for line in lines[1:]:
        f = line.split(" ")
        if f[0] == "mode" and len(f) == 2:
            d["mode"] = f[1]
        elif f[0] == "calibrate" and len(f) == 2:
            d["calibrate"] = int(f[1])
        elif f[0] == "ports" and len(f) == 4:
            d["ports"] = [int(x) for x in f[1:]]
        elif f[0] in ("hostfp", "password", "nonce", "keyb") and len(f) == 2:
            d[f[0]] = f[1]
        elif f[0] == "raw" and len(f) == 3 and f[1].startswith("sha256=") and f[2].startswith("len="):
            d["raw"] = {"sha256": f[1][7:], "len": int(f[2][4:])}
        elif f[0] == "key" and len(f) == 7:
            d["keys"][f[1]] = {"fp": f[2], "passphrase": f[3], "wrong": f[4], "blob_prefix": f[5], "rounds": int(f[6])}
        else:
            raise KeyError_(f"conf: unrecognised line `{f[0]}`")
    return d


REDACTED = "<redacted>"


def redact_conf(text: str) -> str:
    """kssh.conf with the password, passphrase and WRONG fields replaced."""
    out = []
    for line in text.splitlines():
        f = line.split(" ")
        if f[0] == "password" and len(f) == 2:
            f[1] = REDACTED
        elif f[0] == "key" and len(f) == 7:
            f[3], f[4] = REDACTED, REDACTED
        out.append(" ".join(f))
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------------- canaries

def _b64_forms(data: bytes):
    for a in range(3):
        padded = b"\0" * a + data
        lo, hi = -(-8 * a // 6), (8 * len(padded)) // 6
        for tag, enc in (("b64", base64.b64encode), ("b64u", base64.urlsafe_b64encode)):
            yield f"{tag}-a{a}", enc(padded).rstrip(b"=")[lo:hi]


def forms(label: str, data: bytes):
    """Every (form name, needle) of one canary; needles shorter than 8 bytes are not produced.

    raw, hex lower/upper, base64 std/url-safe at three alignments (only the characters fully
    determined by the canary), `1, 2, 3` (decimal) and `0a, 0b` (hex Debug) byte lists, and for
    typed secrets (`pass:`/`wrong:` labels) the hex of one u32 key event per character.
    """
    if len(data) < MIN_CANARY:
        return
    cand = [("raw", data), ("hex-lower", data.hex().encode()), ("hex-upper", data.hex().upper().encode()),
            *_b64_forms(data),
            ("dec-list", ", ".join(str(b) for b in data).encode()),
            ("hexdbg-list", ", ".join("%02x" % b for b in data).encode())]
    if label.startswith(("pass:", "wrong:")):
        text = data.decode("utf-8", "replace")
        for tag, enc in (("keys-u32le", "utf-32-le"), ("keys-u32be", "utf-32-be")):
            raw = text.encode(enc)
            cand += [(tag + "-hex", raw.hex().encode()), (tag + "-hex-upper", raw.hex().upper().encode())]
        cand.append(("keys-codepoints", ", ".join(str(ord(c)) for c in text).encode()))
    seen = set()
    for name, needle in cand:
        if len(needle) >= MIN_CANARY and needle not in seen:
            seen.add(needle)
            yield name, needle


def armor_canaries(tag: str, text: bytes) -> list:
    text = text.strip()
    out = [(f"armor:{tag}", text)]
    lines = [l.strip() for l in text.splitlines() if l.strip() and not l.startswith(b"-----")]
    body = b"".join(lines)
    out.append((f"body:{tag}", body))
    for i, l in enumerate(dict.fromkeys(lines)):
        out.append((f"bodyline:{tag}:{i}", l))
    try:
        out.append((f"blob:{tag}", base64.b64decode(body, validate=True)))
    except (ValueError, base64.binascii.Error):
        pass
    return out


def extra_canaries(path, hint: str | None = None) -> list:
    """Canaries of one extra secret file; `hint="password"` marks a typed secret."""
    p = Path(path)
    data = p.read_bytes()
    name = p.name.lower()
    if b"-----BEGIN" in data:
        return armor_canaries("extra:" + p.name, data)
    value = data.strip()
    pfx = "pass" if hint == "password" or "password" in name or "passphrase" in name else "extra"
    out = [(f"{pfx}:{p.name}", value)]
    try:
        raw = bytes.fromhex(value.decode())
        if len(raw) >= MIN_CANARY:
            out.append((f"extra-bin:{p.name}", raw))
    except (ValueError, UnicodeDecodeError):
        pass
    return out


def canaries(keys_dir, extra_files=(), leak: bool = False) -> list:
    """[(label, bytes)] of everything secret in one run (see k8-cp1.md section 9).

    `leak=True` (serial/log scans of mode t1) adds what must never reach a log: the wrapped ciphertext of
    ka16 (`leak-ct:ka16`) and the 8-byte K4b record headers. `leak=False` (disk scans) omits them: the
    ciphertext and the record headers are expected on disk.
    """
    keys = Path(keys_dir)
    sec = load_secrets(keys)
    out = [("magic", MAGIC)]
    for name, ent in sec["keys"].items():
        for field, pfx in (("passphrase", "pass"), ("wrong", "wrong")):
            if ent.get(field):
                out.append((f"{pfx}:{name}", ent[field].encode()))
        if ent.get("seed"):
            seed = bytes.fromhex(ent["seed"])
            out.append((f"seed:{name}", seed))
            pub = base64.b64decode((keys / f"{name}.pub").read_text().split()[1])
            out.append((f"expanded:{name}", seed + parse_pub_ed25519(pub)))
    for name in KEY_FILE_NAMES:
        f = keys / name
        if f.is_file():
            out += armor_canaries(name, f.read_bytes())
    for extra in extra_files:
        path, hint = extra if isinstance(extra, tuple) else (extra, None)
        out += extra_canaries(path, hint)
    if leak:
        rec = load_expected(keys).get("record")
        if rec and rec.get("ct"):
            ct, name = bytes.fromhex(rec["ct"]), rec.get("name", "ka16")
            out.append((f"leak-ct:{name}", ct))
            # fragments: a truncated or interleaved dump of the record must not slip through on the whole-ciphertext needle
            mid = len(ct) // 2
            out += [(f"leak-ct:{name}:head", ct[:32]), (f"leak-ct:{name}:mid", ct[mid - 16:mid + 16]),
                    (f"leak-ct:{name}:tail", ct[-32:])]
        out += list(LEAK_HEADERS)
    return [(l, d) for l, d in out if len(d) >= MIN_CANARY]


def parse_pub_ed25519(pubblob: bytes) -> bytes:
    w = _Wire(pubblob)
    if w.string() != b"ssh-ed25519":
        raise KeyError_("not an ed25519 public key")
    return w.string()


def public_expected(exp: dict) -> dict:
    """expected.json for the archive: `record.salt` / `record.ct` replaced by length + SHA-256 (no ciphertext)."""
    out = json.loads(json.dumps(exp))
    rec = out.get("record")
    if rec:
        for field in ("salt", "ct"):
            raw = bytes.fromhex(rec.pop(field))
            rec[f"{field}_len"], rec[f"{field}_sha256"] = len(raw), hashlib.sha256(raw).hexdigest()
    return out


# --------------------------------------------------------------------------- CLI

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen")
    g.add_argument("--dir", required=True)
    g.add_argument("--stage", required=True)
    g.add_argument("--calibrate", action="store_true")
    g.add_argument("--mode", choices=("t0", "f", "t1"), default="t0")
    c = sub.add_parser("conf")
    c.add_argument("--dir", required=True)
    c.add_argument("--stage", required=True)
    c.add_argument("--mode", required=True, choices=("t0", "f", "t1"))
    c.add_argument("--ports", required=True, nargs=3, type=int, metavar=("MAIN", "REKEY", "B"))
    c.add_argument("--hostfp", required=True)
    c.add_argument("--password-file", required=True)
    c.add_argument("--calibrate", action="store_true")
    a = ap.parse_args(argv)
    try:
        if a.cmd == "gen":
            exp = gen(a.dir, a.stage, a.calibrate, a.mode)
            tail = f" keyb={exp['keyb']['fp']}" if "keyb" in exp else f" comment_len={exp['comment_len']}" \
                f" comment_via={exp['record']['comment_via']}"
            print("kssh_keys gen ok: " + " ".join(f"{n}={e['fp']}" for n, e in exp["keys"].items()) + tail)
        else:
            old = os.umask(0o077)
            try:
                keys = prepare_dir(a.dir, strict_mode=True, what="--dir")
                stage = prepare_dir(a.stage, strict_mode=False, what="--stage")
                pw = Path(a.password_file).read_text().strip()
                text = build_conf(keys, a.mode, a.ports, a.hostfp, pw, a.calibrate)
                _write_new(stage / "kssh.conf", text.encode(), exclusive=False)
            finally:
                os.umask(old)
            print(f"kssh_keys conf ok: {stage / 'kssh.conf'} ({len(text.splitlines())} lines)")
    except KeyError_ as e:
        print(f"kssh_keys: error: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
