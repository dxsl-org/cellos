#!/usr/bin/env python3
"""Independent verdict for the Tier-3 guest endpoint lane (S10-11, 261110-0900-tier3-guest-endpoint).

Subcommands (host-testable, no QEMU needed; see scripts/test_tier3_ssh_guest_judge.py):

  boot     Judge the T3-B/T3-C boot stage from the lane's evidence directory (`--carry-secret-checks-from`
           re-judges the same evidence after the keys were deleted, carrying only the secret scans over):
             B1 VMM        SVM root, hypervisor cell, vCPU ready; no panic/fault/triple-fault/hv error
             B2 guest      guest kernel banner, TIER3_SSHD_* markers, sshd `Server listening` line
             C1 route (R4) host-side ssh-keyscan returned the pre-generated guest host key (probe only)
             C2 route (R1) the cell-side client probe saw exactly that fingerprint and sshd logged
                           connections from 10.0.2.2 beyond the keyscan's
           plus the QEMU argv invariant, pinned hashes, no inbound sshd in the outer image, no secret
           anywhere, protected-path digests unchanged. Writes verdict.json and names the STOP rule
           of cp1-draft.md section 5 that applies when a gate fails.
  full     The T3-D run: everything `boot` judges (B1, B2, C1, C2 are re-proved at the start of every
           full run), plus the driver's markers (each required marker PASS exactly once, no FAIL,
           the three guest-only scenarios included), `P04-TERMINAL: PASS`, the guest sshd's own log
           accounting (accepted/failed passwords before and after the guest scenarios, one session
           per connection) and the no-replay ledger rule. A marker FAIL is reported as the stop
           rule `MARKER-FAIL`: no fix-and-rerun without a new CP1. T3-D3 adds the Net checks
           `net:*` (below).
  diag     The T3-D2 diagnostic stage (cp1-diag-draft.md): everything `full` judges for the reused
           scenarios, plus the positive control `ssh-guest-consumer-close-producer` (must PASS) and the
           cancel scenario as an OBSERVATION (PASS or FAIL, exactly once; recorded, not a claim). The
           host-side data (ss snapshots, the slirp netdev pcap summary, guest snapshots) is CLASSIFIED,
           not judged: the verdict names one of the outcome labels O1..O7 of the draft's section 5
           and of rerun-cp1.md section 4.1 (O7 = "reclaimed": the Net service aborted the dead
           owner's socket with an RST, the guest connection ended and the producer stopped, the only
           passing outcome of the T3-D3 diag boot). `--expect-outcome LABEL` (diag only) adds the check
           `diag:outcome-is-LABEL`, so any other label fails the verdict.

  Stages full and diag also judge the Net service's own serial lines (rerun-cp1.md 5.2 J3):
           `net:no-owner-death-watch-unavailable` and `net:owner-death-reclaim-logged-in-cancel-window`
           (`[net] owner death: cell C gen G: aborted N TCP, released M other` with N >= 1 between the
           cancel scenario's `p09-case` line and its PASS/FAIL line). The line `[net] owner death watch
           unavailable; socket reclaim deferred` is the stop rule STOP-7 (after the secret, digest and
           argv rules, before the gate rules and MARKER-FAIL).
  archive  Copy the text evidence to the repository after a secret scan (STOP-5): refuses to copy
           anything when a seed, password or host private key is found in any candidate file.

Exit codes: 0 pass; 1 a gate failed or a STOP rule applies; 2 usage/host error.
"""
import argparse
import base64
import json
import re
import shutil
import struct
import sys
from pathlib import Path

PINS = {
    "vmlinuz-virt": "26bf81ada3e8fc30fd4d81805fe6c8c60be5c7fb18a43563c707e49117e624ca",
    "initramfs-virt": "e2562e019a506f9bdac24d06953823106a2ab29da50eea01185d005a3ca4acdf",
    "vmlinux": "2d62d53b39ed16f5d24a89a39abebcf946d9a0f9482734baf3da51e867b1ec07",
    "qemu-system-x86_64": "169fb4eaaa79f7ad1f3a2d1a1f3534a0807354eabdedaf6a51b6f4b1412241fe",
}
EXPECTED_CPU = "qemu64,+pdpe1gb,+svm,+rdrand,+rdseed"
FATAL_HOST = [r"KERNEL PANIC", r"\[fault\] Cell", r"create_vm failed", r"\[hv-x86\] .*guest triple-fault"]
HV_BAD = re.compile(r"\[hv-x86\] .*(fail|error|unexpected|unhandled|guest exited)", re.I)
S93 = ["ssh-conf", "ssh-refusals", "ssh-changed-host-key", "ssh-first-trust-no", "ssh-first-trust-paste-refused",
       "ssh-first-trust-yes", "ssh-first-trust-known-after", "ssh-wrong-password", "ssh-exec-bytes", "ssh-exec-stdin-eof",
       "ssh-exec-signal", "ssh-pty-shell", "ssh-pty-resize", "ssh-pty-escape-disconnect", "ssh-shell-alive", "shell-long-line"]
GUEST_MARKERS = ["ssh-guest-cancel-producer", "ssh-guest-fault-may-have-run", "ssh-guest-no-replay-ledger"]
EXPECTED_MARKERS = ["shell-prompt"] + S93 + GUEST_MARKERS + ["session-a-close"]
MAX_WINDOW_S = 4 * 3600  # cp1-draft.md section 5: hard cap per run
FIXTURE_KERNEL_FEATURES = "phase02-terminal-profile,phase04-driver-profile"
DIAG_CONTROL = "ssh-guest-consumer-close-producer"
DIAG_OBSERVATION = "ssh-guest-cancel-producer"
DIAG_MARKERS = ["shell-prompt"] + S93 + [DIAG_CONTROL] + GUEST_MARKERS + ["session-a-close"]
DIAG_MARK_NAMES = ["cancel-done", "snap-a", "snap-b", "snap-c"]
DIAG_SS_NAMES = DIAG_MARK_NAMES + ["final", "after-kill"]
DIAG_WINDOW_CAP_S = 3600  # cp1-diag-draft.md section 8 STOP-D5
OUTCOMES = {
    "O1": "H1 confirmed: the Net cell holds the dead owner's socket; cooperative close works end to end",
    "O2": "H3: the guest saw the control's close but the producer survived (helper or sshd semantics)",
    "O3": "the control's close did not reach the guest (FIN lost or not forwarded)",
    "O4": "delayed reclaim: something closes the cancelled connection, but later than the 20 s `gone` window",
    "O5": "environment/tooling failure before the scenarios; nothing is claimed",
    "O6": "surprise: any other combination; raw data recorded, no further run without a new CP1",
    "O7": "reclaimed: the Net service aborted the dead owner's socket (RST), the guest connection ended, the producer stopped",
}
STATE_NAMES = {"01": "ESTABLISHED", "02": "SYN_SENT", "03": "SYN_RECV", "04": "FIN_WAIT1", "05": "FIN_WAIT2", "06": "TIME_WAIT",
               "07": "CLOSE", "08": "CLOSE_WAIT", "09": "LAST_ACK", "0A": "LISTEN", "0B": "CLOSING"}
SS_STATES = {"ESTAB", "SYN-SENT", "SYN-RECV", "FIN-WAIT-1", "FIN-WAIT-2", "TIME-WAIT", "CLOSE-WAIT", "LAST-ACK", "CLOSING",
             "UNCONN", "LISTEN", "CLOSE"}
ARCHIVE_FILES = [
    "serial.log", "qemu-argv.txt", "timeline.json", "c1-keyscan.json", "verdict.json", "fat-layout.txt",
    "provenance.sha256", "provenance-build.txt", "initramfs-audit.json", "preflight.txt", "preflight.final.txt",
    "protected-before.txt", "protected-after.txt", "guest-host-key.fingerprint", "guest-host-key.pub",
    "hostfwd-port.txt", "prepare-initramfs.txt", "diag-marks.json", "pcap-summary.json", "pcap-scan.json", "guest-snaps.txt",
]
ARCHIVE_DIRS = ["boot-config"]


def secrets_from(keys: Path):
    """Secret byte strings of a run: cell seed, guest password (plain, hex), host private key."""
    out = []
    seed = keys / "seed.hex"
    if seed.exists():
        out.append(("seed", seed.read_text().strip().encode()))
    pw = keys / "password"
    if pw.exists():
        p = pw.read_text().strip().encode()
        if p:
            out.append(("password", p))
            out.append(("password-hex", p.hex().encode()))
    hk = keys / "host_ed25519"
    if hk.exists():
        lines = [l for l in hk.read_text().splitlines() if l and not l.startswith("-----")]
        body = "".join(lines)
        if body:
            out.append(("host-private-key-body", body.encode()))
            try:
                out.append(("host-private-key-raw", base64.b64decode(body)))
            except Exception:
                pass
            for l in lines:
                if len(l) >= 24:
                    out.append(("host-private-key-line", l.encode()))
        out.append(("host-private-key-armor", b"BEGIN OPENSSH PRIVATE KEY"))
    return out


def scan(data: bytes, secrets):
    return sorted({name for name, s in secrets if s and s in data})


def host_lines(text):
    return [l for l in text.splitlines() if "[guest-uart]" not in l]


def guest_lines(text):
    return [l for l in text.splitlines() if "[guest-uart]" in l]


def full_checks(chk, ev: Path, log: str, host, guest, stage: str = "full"):
    """T3-D checks on the driver's markers and the guest sshd's log (serial order is time order).

    Stage `diag` (T3-D2): the expected markers are DIAG_MARKERS (the consumer-close control added);
    the cancel observation may PASS or FAIL but must be recorded exactly once, and a FAIL anywhere
    else is a claim failure. Returns (passes, fails)."""
    diag = stage == "diag"
    expected = DIAG_MARKERS if diag else EXPECTED_MARKERS
    passes, fails = {}, []
    for line in host:
        m = re.search(r"P04 (PASS|FAIL) (\S+)(.*)$", line)
        if m:
            if m.group(1) == "PASS":
                passes[m.group(2)] = passes.get(m.group(2), 0) + 1
            else:
                fails.append((m.group(2), m.group(3).strip()[:160]))
    chk("D:driver-started", any("P04 START" in l for l in host))
    for name in expected:
        n_fail = sum(1 for f in fails if f[0] == name)
        if diag and name == DIAG_OBSERVATION:
            seen = passes.get(name, 0) + n_fail
            chk("D:marker:" + name, seen == 1, "recorded %d times (PASS or FAIL once: observation)" % seen)
        else:
            chk("D:marker:" + name, passes.get(name) == 1 and n_fail == 0,
                "missing" if name not in passes else ("duplicate" if passes[name] != 1 else "FAIL present"))
    chk("D:no-unexpected-markers", set(passes) <= set(expected), sorted(set(passes) - set(expected)))
    allowed_fail = {DIAG_OBSERVATION} if diag else set()
    bad_fails = [f for f in fails if f[0] not in allowed_fail]
    chk("D:no-FAIL-markers", not bad_fails, bad_fails)
    if diag:
        n_obs_fail = sum(1 for f in fails if f[0] == DIAG_OBSERVATION)
        finals = re.findall(r"P04-TERMINAL: (PASS|FAIL)(?: failures=(\d+))? checks=(\d+)", log)
        want = [("FAIL", str(n_obs_fail), str(len(DIAG_MARKERS)))] if n_obs_fail else [("PASS", "", str(len(DIAG_MARKERS)))]
        chk("D:driver-final-verdict", finals == want, finals)
    else:
        finals = re.findall(r"P04-TERMINAL: (PASS|FAIL)\S*(?: failures=\d+)? checks=(\d+)", log)
        chk("D:driver-final-verdict", finals == [("PASS", str(len(EXPECTED_MARKERS)))], finals)
    chk("D:boot-ceiling-selftest-pass", "[selftest] boot-ceiling: PASS" in log)
    chk("D:trust-store-volume-mounted", "[vfs] CellosFS Native mounted at existing volume" in log)
    chk("D:no-weak-entropy", "serving WEAK xorshift32" not in log)
    chk("D:fixture-kernel-features-recorded",
        FIXTURE_KERNEL_FEATURES in (ev / "provenance-build.txt").read_text(errors="replace")
        if (ev / "provenance-build.txt").exists() else False)
    # The guest sshd's log, split where the driver opens the first guest-only scenario.
    lines = log.splitlines()
    split = next((i for i, l in enumerate(lines)
                  if re.search(r"P04 INFO p09-case (ssh-guest-cancel-producer|ssh-guest-consumer-close-producer)\b", l)), None)
    chk("D:guest-scenarios-started", split is not None)
    before, after = ("\n".join(lines[:split]), "\n".join(lines[split:])) if split is not None else (log, "")

    def n(pat, text):
        return len(re.findall(pat, text))

    chk("D:sshd-saw-guest-connections", n(r"Connection from 10\.0\.2\.2 port \d+", log) >= 10)
    chk("D:sshd-accepted-password-six-before-guest-scenarios", n(r"Accepted password for tester", before) == 6,
        n(r"Accepted password for tester", before))
    chk("D:sshd-failed-password-once-before-guest-scenarios", n(r"Failed password for tester", before) == 1,
        n(r"Failed password for tester", before))
    logins = re.findall(r"P04 INFO ssh-guest-logins (\d+)", log)
    acc_after = n(r"Accepted password for tester", after)
    chk("D:guest-scenario-logins-accounted",
        len(logins) == 1 and (9 if diag else 4) <= acc_after <= int(logins[0]) and n(r"Failed password for tester", after) == 0,
        {"driver_logins": logins, "accepted_after": acc_after, "failed_after": n(r"Failed password for tester", after)})
    ledger = re.findall(r"P04 INFO ssh-guest-ledger selfkill=(\d+) lines=(\d+)", log)
    chk("D:no-replay-ledger-exactly-one-selfkill", len(ledger) == 1 and ledger[0][0] == "1" and int(ledger[0][1]) >= 1, ledger)
    chk("D:selfkill-dispatch-reported", n(r"P04 INFO ssh-guest-selfkill dispatch=(accepted|may_have_run)\b", log) == 1)
    # One session request per connection: the sshd log never starts two sessions on one connection.
    by_port, starts = {}, 0
    for l in guest:
        m = re.search(r"Starting session: \S+ .*?from \S+ port (\d+)", l)
        if m:
            by_port[m.group(1)] = by_port.get(m.group(1), 0) + 1
            starts += 1
    chk("D:sshd-one-session-per-connection", starts >= 10 and all(v == 1 for v in by_port.values()), {"sessions": starts, "max": max(by_port.values(), default=0)})
    return passes, fails


# ---- T3-D2 diagnostic data: guest snapshots, host ss, pcap summary, outcome classification ----

def hex_ipv4(h):
    """/proc/net/tcp prints an IPv4 address as the little-endian hex of its 32-bit value."""
    try:
        b = bytes.fromhex(h)
    except ValueError:
        return "?"
    return ".".join(str(x) for x in b[::-1]) if len(b) == 4 else "?"


def parse_snap_lines(lines):
    """One `p9 snap` output (the lines after `ssh-guest-snap NAME |`) into data."""
    s = {"up": None, "ssh_conn": "", "pidfile": "", "procs": [], "tcp": [], "diag": []}
    for l in lines:
        if l.startswith("up="):
            s["up"] = l[3:].strip()
        elif l.startswith("ssh_conn="):
            s["ssh_conn"] = l[9:].strip()
        elif l.startswith("pidfile="):
            s["pidfile"] = l[8:].strip()
        elif l.startswith("proc "):
            p = l.split()
            if len(p) >= 5:
                s["procs"].append({"pid": p[1], "comm": p[2], "state": p[3], "ppid": p[4]})
        elif l.startswith("tcp "):
            m = re.match(r"tcp ([0-9A-Fa-f]{8}):([0-9A-Fa-f]{4}) ([0-9A-Fa-f]{8}):([0-9A-Fa-f]{4}) st=([0-9A-Fa-f]{2}) "
                         r"q=([0-9A-Fa-f]{8}):([0-9A-Fa-f]{8})", l)
            if m:
                s["tcp"].append({"local": "%s:%d" % (hex_ipv4(m.group(1)), int(m.group(2), 16)),
                                 "remote": "%s:%d" % (hex_ipv4(m.group(3)), int(m.group(4), 16)),
                                 "remote_port": int(m.group(4), 16),
                                 "state": STATE_NAMES.get(m.group(5).upper(), m.group(5)),
                                 "tx": int(m.group(6), 16), "rx": int(m.group(7), 16)})
        elif l.startswith("diag "):
            s["diag"].append(l[5:].strip())
    return s


def extract_snaps(host):
    """(parsed snaps that ended, raw lines per snap) from the driver's `ssh-guest-snap NAME ...` lines."""
    raw, ended = {}, set()
    for l in host:
        m = re.search(r"P04 INFO ssh-guest-snap (snap-[abc]) \| (.*)$", l)
        if m:
            raw.setdefault(m.group(1), []).append(m.group(2).strip())
            continue
        m = re.search(r"P04 INFO ssh-guest-snap (snap-[abc]) end\s*$", l)
        if m:
            ended.add(m.group(1))
    return {k: parse_snap_lines(v) for k, v in raw.items() if k in ended}, raw


def yes_alive(snap):
    """The pid recorded in the pid file is a live (not zombie) `yes` in this snapshot."""
    pid = snap.get("pidfile", "")
    return any(p["pid"] == pid and p["comm"] == "(yes)" and p["state"] != "Z" for p in snap.get("procs", [])) if pid else False


def conn_ports(lines):
    """Guest-side (slirp source) port of the control and the cancel connection: the first sshd
    `Connection from 10.0.2.2 port N` after the scenario's `p09-case` line."""
    out = {}
    for key, name in (("control", DIAG_CONTROL), ("cancel", DIAG_OBSERVATION)):
        i = next((k for k, l in enumerate(lines) if "P04 INFO p09-case %s" % name in l), None)
        port = None
        if i is not None:
            for l in lines[i + 1:]:
                m = re.search(r"Connection from 10\.0\.2\.2 port (\d+)", l)
                if m:
                    port = int(m.group(1))
                    break
        out[key] = port
    return out


def closed_logged(lines, port):
    """The guest sshd logged the end of the connection from `port` (it saw FIN/RST or a disconnect)."""
    if port is None:
        return False
    pat = re.compile(r"(?:Connection closed by|Received disconnect from|Disconnected from).*?\bport %d\b" % port)
    return any(pat.search(l) for l in lines)


def parse_ss_file(text):
    """A lane `host-ss-NAME.txt`: header comments (`# rc=N`) and the `ss -tnoi` rows."""
    rc, states, rows = None, {}, []
    for l in text.splitlines():
        if l.startswith("#"):
            m = re.match(r"# rc=(-?\d+)", l)
            if m:
                rc = int(m.group(1))
            continue
        toks = l.split()
        if toks and toks[0] in SS_STATES:
            states[toks[0]] = states.get(toks[0], 0) + 1
            rows.append(l.strip()[:200])
    return {"rc": rc, "states": states, "rows": rows}


def iter_pcap(path):
    """(timestamp, link type, frame bytes) of a classic libpcap file."""
    with open(path, "rb") as fh:
        g = fh.read(24)
        if len(g) < 24:
            raise ValueError("short pcap header")
        magic = g[:4]
        table = {b"\xd4\xc3\xb2\xa1": ("<", False), b"\xa1\xb2\xc3\xd4": (">", False),
                 b"\x4d\x3c\xb2\xa1": ("<", True), b"\xa1\xb2\x3c\x4d": (">", True)}
        if magic not in table:
            raise ValueError("not a classic pcap file")
        end, nano = table[magic]
        link = struct.unpack(end + "I", g[20:24])[0]
        hdr = struct.Struct(end + "IIII")
        while True:
            h = fh.read(16)
            if len(h) < 16:
                break
            sec, frac, caplen, _orig = hdr.unpack(h)
            data = fh.read(caplen)
            if len(data) < caplen:
                break
            yield sec + frac / (1e9 if nano else 1e6), link, data


def parse_tcp(frame):
    """(src, dst, sport, dport, flags, window, payload length) of an Ethernet/IPv4/TCP frame, else None."""
    if len(frame) < 34:
        return None
    et, off = int.from_bytes(frame[12:14], "big"), 14
    if et == 0x8100 and len(frame) >= 38:
        et, off = int.from_bytes(frame[16:18], "big"), 18
    if et != 0x0800 or len(frame) < off + 20 or frame[off] >> 4 != 4 or frame[off + 9] != 6:
        return None
    ihl = (frame[off] & 15) * 4
    if ihl < 20 or int.from_bytes(frame[off + 6:off + 8], "big") & 0x1FFF:
        return None
    t = off + ihl
    if len(frame) < t + 20:
        return None
    total = int.from_bytes(frame[off + 2:off + 4], "big") or (len(frame) - off)
    doff = (frame[t + 12] >> 4) * 4
    src = ".".join(str(x) for x in frame[off + 12:off + 16])
    dst = ".".join(str(x) for x in frame[off + 16:off + 20])
    sport, dport = struct.unpack(">HH", frame[t:t + 4])
    return (src, dst, sport, dport, frame[t + 13], int.from_bytes(frame[t + 14:t + 16], "big"),
            max(0, min(total, len(frame) - off) - ihl - doff))


def flow_of(src, dst, sport, dport, port):
    """Flow A: cell 10.0.2.15 <-> slirp 10.0.2.2:PORT (client = cell). Flow B: slirp 10.0.2.2 <->
    nested guest 10.0.2.16:22 (client = slirp). Returns (flow, client ip, client port, server ip,
    server port, direction c2s/s2c) or None."""
    if src == "10.0.2.15" and dst == "10.0.2.2" and dport == port:
        return ("A", src, sport, dst, dport, "c2s")
    if src == "10.0.2.2" and dst == "10.0.2.15" and sport == port:
        return ("A", dst, dport, src, sport, "s2c")
    if src == "10.0.2.2" and dst == "10.0.2.16" and dport == 22:
        return ("B", src, sport, dst, dport, "c2s")
    if src == "10.0.2.16" and dst == "10.0.2.2" and sport == 22:
        return ("B", dst, dport, src, sport, "s2c")
    return None


def flag_str(f):
    return "".join(c for bit, c in ((0x02, "S"), (0x01, "F"), (0x04, "R"), (0x08, "P"), (0x10, "A"), (0x20, "U")) if f & bit)


def summarize_pcap(path, port, ports, marks, t_start, max_after=400):
    """Per-connection summary of the slirp-netdev capture (no payload). `ports`: guest-side port of
    the control/cancel connection; `marks`: wall-clock time of each diag mark; `t_start`: QEMU start
    wall time (times in the result are seconds after it)."""
    cancel_t = marks.get("cancel-done")
    conns = {"A": [], "B": []}
    live = {}
    stats = {"packets": 0, "tcp_packets": 0, "linktype": None, "first_t": None, "last_t": None, "clock_offset_s": None}
    # T3-D2 boot 1 observed: the filter-dump stamps were the lane's wall clock shifted by exactly the host's UTC
    # offset (-25200 s at +07:00). The first packet follows the QEMU start by seconds, so the offset is
    # taken as that skew rounded to a whole 15 minutes (recorded as pcap.clock_offset_s).
    offset = None
    for t, link, frame in iter_pcap(path):
        stats["packets"] += 1
        stats["linktype"] = link
        if offset is None:
            offset = round((t - t_start) / 900.0) * 900.0
            stats["clock_offset_s"] = offset
        t -= offset
        if link != 1:
            raise ValueError("unsupported link type %s" % link)
        stats["first_t"] = t if stats["first_t"] is None else stats["first_t"]
        stats["last_t"] = t
        p = parse_tcp(frame)
        if p is None:
            continue
        stats["tcp_packets"] += 1
        src, dst, sport, dport, flags, win, plen = p
        info = flow_of(src, dst, sport, dport, port)
        if info is None:
            continue
        flow, cip, cport, sip, sport_, d = info
        key = (flow, cip, cport, sip, sport_)
        c = live.get(key)
        syn_only = bool(flags & 0x02) and not flags & 0x10
        closed = c is not None and (c["fin"]["c2s"] + c["fin"]["s2c"] + c["rst"]["c2s"] + c["rst"]["s2c"]) > 0
        if c is None or (d == "c2s" and syn_only and closed):
            c = {"flow": flow, "client": "%s:%d" % (cip, cport), "server": "%s:%d" % (sip, sport_), "first_t": t, "last_t": t,
                 "pkts": 0, "syn": 0, "fin": {"c2s": 0, "s2c": 0}, "rst": {"c2s": 0, "s2c": 0}, "first_fin": None,
                 "first_rst": None, "zero_win": {"c2s": 0, "s2c": 0}, "zero_win_after_cancel": {"c2s": 0, "s2c": 0},
                 "zero_win_after_first_rst": {"c2s": 0, "s2c": 0},
                 "payload_bytes": {"c2s": 0, "s2c": 0}, "after_cancel": [], "after_cancel_truncated": False}
            live[key] = c
            conns[flow].append(c)
        c["pkts"] += 1
        c["last_t"] = t
        c["payload_bytes"][d] += plen
        if syn_only:
            c["syn"] += 1
        if flags & 0x01:
            c["fin"][d] += 1
            c["first_fin"] = c["first_fin"] or {"t": t, "dir": d}
        if flags & 0x04:
            c["rst"][d] += 1
            c["first_rst"] = c["first_rst"] or {"t": t, "dir": d}
        if win == 0 and flags & 0x10 and not flags & 0x07:
            c["zero_win"][d] += 1
            if cancel_t is not None and t >= cancel_t:
                c["zero_win_after_cancel"][d] += 1
            if c["first_rst"] is not None:
                c["zero_win_after_first_rst"][d] += 1
        if cancel_t is not None and t >= cancel_t - 2.0:
            if len(c["after_cancel"]) < max_after:
                c["after_cancel"].append([round(t - cancel_t, 3), d, flag_str(flags), plen, win])
            else:
                c["after_cancel_truncated"] = True

    def pick_b(n):
        if n is None:
            return None
        cand = [c for c in conns["B"] if c["client"].endswith(":%d" % n) and (cancel_t is None or c["first_t"] <= cancel_t + 5.0)]
        return cand[-1] if cand else None

    def pick_a(b, before=None):
        if b is not None:
            cand = [c for c in conns["A"] if c["first_t"] <= b["first_t"] + 0.5]
            return (cand[-1], "paired-by-syn-time") if cand else (None, "no-A-connection-before-the-B-syn")
        if before is not None:
            cand = [c for c in conns["A"] if c["first_t"] <= before]
            return (cand[-1], "last-A-syn-before-the-cancel-mark") if cand else (None, "no-A-connection")
        return None, "unidentified"

    b_cancel, b_control = pick_b(ports.get("cancel")), pick_b(ports.get("control"))
    a_cancel, m_cancel = pick_a(b_cancel, cancel_t)
    a_control, m_control = pick_a(b_control)

    def rel(t):
        return None if t is None else round(t - t_start, 3)

    def final(c, keep):
        if c is None:
            return None
        out = {k: v for k, v in c.items() if k not in ("after_cancel", "first_fin", "first_rst", "first_t", "last_t")}
        out["first_t"], out["last_t"] = rel(c["first_t"]), rel(c["last_t"])
        for k in ("first_fin", "first_rst"):
            out[k] = None if c[k] is None else {"t": rel(c[k]["t"]), "dir": c[k]["dir"]}
        if keep:
            out["after_cancel"] = c["after_cancel"]
        return out

    def fr(c, d):
        return None if c is None else (c["fin"][d] + c["rst"][d]) > 0

    derived = {
        "cancel_A_fin_rst_from_cell": fr(a_cancel, "c2s"), "cancel_A_fin_rst_from_slirp": fr(a_cancel, "s2c"),
        "cancel_B_fin_rst_from_slirp": fr(b_cancel, "c2s"), "cancel_B_fin_rst_from_guest": fr(b_cancel, "s2c"),
        "control_A_fin_rst_from_cell": fr(a_control, "c2s"), "control_B_fin_rst_from_slirp": fr(b_control, "c2s"),
        "cancel_A_zero_window_from_cell_after_mark": None if a_cancel is None else a_cancel["zero_win_after_cancel"]["c2s"],
        "cancel_A_rst_from_cell": None if a_cancel is None else a_cancel["rst"]["c2s"] >= 1,
        "cancel_A_fin_from_cell": None if a_cancel is None else a_cancel["fin"]["c2s"] >= 1,
        "cancel_A_zero_window_from_cell_after_first_rst": None if a_cancel is None else a_cancel["zero_win_after_first_rst"]["c2s"],
        "control_A_rst_from_cell": None if a_control is None else a_control["rst"]["c2s"] >= 1,
        "control_A_fin_from_cell": None if a_control is None else a_control["fin"]["c2s"] >= 1,
        "control_A_zero_window_from_cell_after_first_rst": None if a_control is None else a_control["zero_win_after_first_rst"]["c2s"],
    }
    return {
        "schema": 1, "pcap": dict(stats, first_t=rel(stats["first_t"]), last_t=rel(stats["last_t"])),
        "t_start_wall": t_start, "hostfwd_port": port, "guest_side_ports": ports,
        "marks_t": {k: rel(v) for k, v in marks.items()},
        "cancel": {"A": final(a_cancel, True), "B": final(b_cancel, True), "A_pairing": m_cancel},
        "control": {"A": final(a_control, False), "B": final(b_control, False), "A_pairing": m_control},
        "derived": derived,
        "connections": {fl: [final(c, False) for c in conns[fl]] for fl in ("A", "B")},
    }


def scan_file(path, secrets, chunk=16 * 1024 * 1024):
    """Secret names found in a large file (chunked, overlapping by the longest secret)."""
    keep = max((len(s) for _, s in secrets if s), default=1) - 1
    hits, tail = set(), b""
    with open(path, "rb") as fh:
        while True:
            buf = fh.read(chunk)
            if not buf:
                break
            data = tail + buf
            hits.update(scan(data, secrets))
            tail = data[-keep:] if keep > 0 else b""
    return sorted(hits)


def prep_diag(ev: Path, pcap: Path, secrets, port: str):
    """Derive pcap-summary.json, pcap-scan.json and guest-snaps.txt into the evidence directory
    before judging (the raw pcap stays outside it)."""
    log = (ev / "serial.log").read_text(errors="replace") if (ev / "serial.log").exists() else ""
    host = host_lines(log)
    mk = json.loads((ev / "diag-marks.json").read_text()) if (ev / "diag-marks.json").exists() else {}
    walls = {n: m.get("wall") for n, m in mk.get("marks", {}).items() if m.get("wall") is not None}
    ports = conn_ports(log.splitlines())
    if pcap.is_file() and pcap.stat().st_size > 0 and mk.get("qemu_start_wall") is not None:
        try:
            summary = summarize_pcap(pcap, int(port), ports, walls, mk["qemu_start_wall"])
        except (OSError, ValueError, struct.error) as e:
            summary = {"error": "pcap parse failed: %s" % e}
    else:
        summary = {"error": "pcap missing or empty, or no qemu start time"}
    (ev / "pcap-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    scanned = {"scanned": False, "hits": []}
    if pcap.is_file():
        scanned = {"scanned": True, "bytes": pcap.stat().st_size, "hits": scan_file(pcap, secrets or [])}
    (ev / "pcap-scan.json").write_text(json.dumps(scanned, indent=2) + "\n")
    _, raw = extract_snaps(host)
    (ev / "guest-snaps.txt").write_text("".join("== %s ==\n%s\n" % (k, "\n".join(raw[k])) for k in sorted(raw)))


NET_RECLAIM_RE = re.compile(r"\[net\] owner death: cell (\d+) gen (\d+): aborted (\d+) TCP, released (\d+) other")
NET_WATCH_UNAVAILABLE = "owner death watch unavailable"


def net_owner_death(lines):
    """The Net service's owner-death lines in the serial log (host lines only; a `[guest-uart]` line
    never counts). `in_cancel_window`: a reclaim line with `aborted N TCP`, N >= 1, lies strictly between
    the cancel scenario's `p09-case` line and its `P04 PASS/FAIL` line."""
    obs = re.escape(DIAG_OBSERVATION)
    start = next((i for i, l in enumerate(lines) if "[guest-uart]" not in l and re.search(r"P04 INFO p09-case %s\b" % obs, l)), None)
    end = None
    if start is not None:
        end = next((i for i in range(start + 1, len(lines))
                    if "[guest-uart]" not in lines[i] and re.search(r"P04 (?:PASS|FAIL) %s\b" % obs, lines[i])), None)
    found, unavailable = [], False
    for i, l in enumerate(lines):
        if "[guest-uart]" in l:
            continue
        if NET_WATCH_UNAVAILABLE in l:
            unavailable = True
        m = NET_RECLAIM_RE.search(l)
        if m:
            in_win = start is not None and end is not None and start < i < end
            found.append({"serial_line": i + 1, "cell": int(m.group(1)), "gen": int(m.group(2)), "aborted": int(m.group(3)),
                          "released": int(m.group(4)), "in_cancel_window": in_win})
    return {"watch_unavailable": unavailable, "owner_death_lines": found,
            "in_cancel_window": any(f["in_cancel_window"] and f["aborted"] >= 1 for f in found)}


def _is_zero(v):
    return isinstance(v, int) and not isinstance(v, bool) and v == 0


def o7_holds(d):
    """Outcome O7 "reclaimed" (rerun-cp1.md section 4.1 rows 2-9). Every key is read with `.get`: data that
    lacks a key (the O1..O6 fixtures, a failed prep) can never be O7, and only exact booleans/zero count."""
    st = [d.get("cancel_conn_state_at_snap_" + x) for x in "abc"]
    return (not d.get("env_failure")
            and d.get("control_ok") is True and d.get("control_closed_logged") is True
            and d.get("control_producer_started") is True and d.get("control_producer_stopped_at_snap_a") is True
            and d.get("cancel_marker_pass") is True
            and d.get("cancel_A_rst_from_cell") is True and d.get("cancel_A_fin_from_cell") is False
            and _is_zero(d.get("cancel_A_zero_window_from_cell_after_first_rst"))
            and d.get("slirp_fin_rst_to_guest_after_cancel") is True
            and all(s is not None and s not in ("ESTABLISHED", "CLOSE_WAIT") for s in st) and st[2] == "absent"
            and all(d.get("yes_alive_at_snap_" + x) is False and d.get("yes_pid_known_at_snap_" + x) is True for x in "abc")
            and all(_is_zero(d.get("host_ss_established_at_" + x)) for x in "abc")
            and d.get("net_owner_death_in_cancel_window") is True and d.get("net_watch_unavailable") is False)


def classify(d):
    """Outcome label (cp1-diag-draft.md section 5) from the observed data `d`; returns (label, why)."""
    if o7_holds(d):
        return "O7", ("control PASS; cancel marker PASS; RST (no FIN) from the cell on flow A and no zero-window ACK after it; "
                      "FIN/RST from slirp towards the guest; guest connection not ESTABLISHED/CLOSE_WAIT at snap-a/b/c and absent at snap-c; "
                      "yes not alive at snap-a/b/c; host ss ESTAB 0 at snap-a/b/c; [net] owner death line with N >= 1 in the cancel window")
    if d["env_failure"]:
        return "O5", d["env_failure"]
    st = [d["cancel_conn_state_at_snap_" + x] for x in "abc"]
    ys = [d["yes_alive_at_snap_" + x] for x in "abc"]
    ss = [d["host_ss_established_at_" + x] for x in "abc"]
    fin_cell, fin_slirp = d["net_fin_rst_after_cancel"], d["slirp_fin_rst_to_guest_after_cancel"]
    if d["control_ok"] and d.get("control_producer_started") is True and d.get("control_producer_stopped_at_snap_a") is True \
            and d["control_closed_logged"] and not d["cancel_marker_pass"] and fin_cell is False and fin_slirp is False \
            and all(s == "ESTABLISHED" for s in st) and all(y is True for y in ys) and all(n >= 2 for n in ss):
        return "O1", "control PASS (producer started, its close logged, producer gone); cancel: guest connection ESTABLISHED and yes alive at snap-a/b/c, host SA+SB ESTAB, no FIN/RST from the cell on flow A, none from slirp on flow B"
    if d["control_ok"] and st[0] == "ESTABLISHED" and ys[0] is True and any(s != "ESTABLISHED" for s in st[1:]):
        return "O4", "control PASS; the cancelled connection was alive at snap-a and gone at a later snap"
    if d["control_timed_out"] and not d["control_gone"] and not d["cancel_marker_pass"]:
        if d["control_closed_logged"]:
            return "O2", "control: client timeout, guest logged the close, yet the producer was alive after 20 s"
        return "O3", "control: client timeout, producer alive and no close logged by the guest sshd"
    return "O6", "no outcome row matches (control_ok=%s control_closed_logged=%s cancel_pass=%s fin_cell=%s fin_slirp=%s states=%s yes=%s ss=%s)" % (
        d["control_ok"], d["control_closed_logged"], d["cancel_marker_pass"], fin_cell, fin_slirp, st, ys, ss)


def diag_checks(chk, ev: Path, log: str, host, guest, tl, passes, fails):
    """T3-D2 stage checks (presence/health of the observation data) and the classification record."""
    def read(name):
        p = ev / name
        return p.read_text(errors="replace") if p.exists() else ""

    lines = log.splitlines()
    mk = json.loads(read("diag-marks.json") or "{}")
    marks = mk.get("marks", {})
    ss = {n: parse_ss_file(read("host-ss-%s.txt" % n)) for n in DIAG_SS_NAMES}
    chk("diag:host-ss-files-present-nonempty",
        all((ev / ("host-ss-%s.txt" % n)).is_file() and (ev / ("host-ss-%s.txt" % n)).stat().st_size > 0 for n in DIAG_SS_NAMES),
        [n for n in DIAG_SS_NAMES if not (ev / ("host-ss-%s.txt" % n)).is_file()])
    chk("diag:host-ss-tool-ok", all(ss[n]["rc"] == 0 for n in DIAG_SS_NAMES), {n: ss[n]["rc"] for n in DIAG_SS_NAMES})
    chk("diag:marks-recorded", all(n in marks for n in DIAG_SS_NAMES) and mk.get("qemu_start_wall") is not None, sorted(marks))
    ps = json.loads(read("pcap-summary.json") or "{}")
    chk("diag:pcap-summary-present", bool(ps) and "error" not in ps and ps.get("pcap", {}).get("tcp_packets", 0) > 0,
        ps.get("error", ps.get("pcap")))
    scan_rec = json.loads(read("pcap-scan.json") or "{}")
    chk("diag:no-secret-in-pcap", scan_rec.get("scanned") is True and not scan_rec.get("hits"), scan_rec)
    snaps, _raw = extract_snaps(host)
    chk("diag:three-snaps-present",
        all(k in snaps and snaps[k]["procs"] and snaps[k]["up"] is not None for k in ("snap-a", "snap-b", "snap-c")), sorted(snaps))
    chk("diag:window-within-1h-cap", 0 < int(tl.get("window_s", 0)) <= DIAG_WINDOW_CAP_S and float(tl.get("elapsed_s", 0)) <= DIAG_WINDOW_CAP_S,
        {"window_s": tl.get("window_s"), "elapsed_s": tl.get("elapsed_s")})

    ports = conn_ports(lines)
    ctl_fail = next((f[1] for f in fails if f[0] == DIAG_CONTROL), None)
    control_ok = passes.get(DIAG_CONTROL) == 1
    derived = ps.get("derived", {}) if isinstance(ps, dict) else {}
    net = net_owner_death(lines)
    d = {
        "control_ok": control_ok,
        "control_timed_out": control_ok or (ctl_fail is not None and "timed_out=true" in ctl_fail),
        "control_gone": control_ok or (ctl_fail is not None and "gone_ok=true" in ctl_fail),
        "control_closed_logged": closed_logged(guest, ports["control"]),
        "control_guest_port": ports["control"], "cancel_guest_port": ports["cancel"],
        "cancel_marker_pass": passes.get(DIAG_OBSERVATION) == 1,
        "cancel_closed_logged": closed_logged(guest, ports["cancel"]),
        "net_fin_rst_after_cancel": derived.get("cancel_A_fin_rst_from_cell"),
        "slirp_fin_rst_to_guest_after_cancel": derived.get("cancel_B_fin_rst_from_slirp"),
        "control_net_fin_rst": derived.get("control_A_fin_rst_from_cell"),
        "control_slirp_fin_rst_to_guest": derived.get("control_B_fin_rst_from_slirp"),
        "cancel_A_zero_window_from_cell_after_mark": derived.get("cancel_A_zero_window_from_cell_after_mark"),
        "cancel_A_rst_from_cell": derived.get("cancel_A_rst_from_cell"),
        "cancel_A_fin_from_cell": derived.get("cancel_A_fin_from_cell"),
        "cancel_A_zero_window_from_cell_after_first_rst": derived.get("cancel_A_zero_window_from_cell_after_first_rst"),
        "control_A_rst_from_cell": derived.get("control_A_rst_from_cell"),
        "control_A_fin_from_cell": derived.get("control_A_fin_from_cell"),
        "control_A_zero_window_from_cell_after_first_rst": derived.get("control_A_zero_window_from_cell_after_first_rst"),
        "net_owner_death_in_cancel_window": net["in_cancel_window"], "net_watch_unavailable": net["watch_unavailable"],
        "host_ss_states": {n: ss[n]["states"] for n in DIAG_SS_NAMES},
    }
    for x in "abc":
        s = snaps.get("snap-" + x)
        row = next((t for t in s["tcp"] if t["remote_port"] == ports["cancel"]), None) if s and ports["cancel"] else None
        d["cancel_conn_state_at_snap_" + x] = None if s is None else (row["state"] if row else "absent")
        d["cancel_conn_queues_at_snap_" + x] = None if row is None else "%d:%d" % (row["tx"], row["rx"])
        d["yes_alive_at_snap_" + x] = None if s is None else yes_alive(s)
        d["yes_pid_known_at_snap_" + x] = None if s is None else bool(s.get("pidfile"))
        d["yes_ppid_at_snap_" + x] = None if s is None else next((p["ppid"] for p in s["procs"] if p["pid"] == s["pidfile"] and p["comm"] == "(yes)"), None)
        d["host_ss_established_at_" + x] = ss["snap-" + x]["states"].get("ESTAB", 0)
        d["guest_uptime_at_snap_" + x] = None if s is None else s["up"]
    sa = snaps.get("snap-a")
    pids = [m.group(1) for l in (sa["diag"] if sa else []) for m in [re.match(r"pid=(\d+) ppid=\d+", l)] if m]
    d["control_producer_started"] = None if sa is None else len(pids) >= 2
    d["control_producer_pid"] = pids[0] if len(pids) >= 2 else None
    d["control_producer_stopped_at_snap_a"] = (None if sa is None or len(pids) < 2 else
                                               not any(p["pid"] == pids[0] and p["comm"] == "(yes)" and p["state"] != "Z" for p in sa["procs"]))
    d["host_ss_established_at_cancel_done"] = ss["cancel-done"]["states"].get("ESTAB", 0)
    d["host_ss_established_final"] = ss["final"]["states"].get("ESTAB", 0)
    d["host_ss_established_after_kill"] = ss["after-kill"]["states"].get("ESTAB", 0)
    env = None
    if "P04 INFO p09-case %s" % DIAG_CONTROL not in log:
        env = "the guest scenarios never started (run ended: %s)" % tl.get("verdict")
    elif not all(ss[n]["rc"] == 0 for n in DIAG_SS_NAMES) or "error" in ps or not ps:
        env = "host tooling failure: ss rc / pcap summary (%s)" % (ps.get("error") if isinstance(ps, dict) else None)
    d["env_failure"] = env
    if env is None and any(d["cancel_conn_state_at_snap_" + x] is None for x in "abc"):
        label, why = "O6", "a guest snapshot is missing"
    else:
        label, why = classify(d)
    d["outcome"], d["outcome_text"], d["outcome_why"] = label, OUTCOMES[label], why
    return d


def judge_boot(ev: Path, secrets, fingerprint: str, port: str, head: str = "", carry=None, stage: str = "boot", expect_outcome: str = ""):
    """Return (verdict dict, stop rule or None). Pure function of the evidence directory.

    `carry` (re-judging after the run's keys were deleted): the earlier verdict's checks, from
    which exactly the two secret-scan results are taken over unchanged and marked as carried."""
    checks = {}

    def chk(name, ok, why=""):
        checks[name] = {"pass": bool(ok), "detail": str(why)}

    def read(name, default=""):
        p = ev / name
        return p.read_text(errors="replace") if p.exists() else default

    log = read("serial.log")
    raw = (ev / "serial.raw.log").read_bytes() if (ev / "serial.raw.log").exists() else log.encode()
    tl = json.loads(read("timeline.json", "{}") or "{}")
    c1 = json.loads(read("c1-keyscan.json", "{}") or "{}")
    host, guest = host_lines(log), guest_lines(log)
    gtext, htext = "\n".join(guest), "\n".join(host)

    def count(pat, lines):
        return sum(1 for l in lines if re.search(pat, l))

    # B1: the VMM
    chk("B1:svm-root-active", count(r"x86 virt: root operation active \(SVM\)", host) == 1)
    chk("B1:hypervisor-cell-starting", count(r"\[hv-x86\] hypervisor cell starting \(SVM PVH\)", host) == 1)
    chk("B1:vcpu-ready", count(r"\[hv-x86\] vCPU ready", host) == 1,
        "exactly one `vCPU ready` (a second one means the hypervisor cell restarted)")
    fatal = [l.strip() for l in host if any(re.search(p, l) for p in FATAL_HOST)]
    chk("B1:no-panic-fault-triplefault", not fatal, fatal[:3])
    hv_bad = [l.strip() for l in host if HV_BAD.search(l)]
    chk("B1:no-hv-error-lines", not hv_bad, hv_bad[:3])
    # B2: the guest
    ver = next((re.search(r"Linux version (\S+)", l).group(1) for l in guest if re.search(r"Linux version \S+", l)), None)
    chk("B2:guest-kernel-banner", ver is not None, ver)
    kline = next((re.search(r"TIER3_SSHD_KERNEL (\S+)", l).group(1) for l in guest if "TIER3_SSHD_KERNEL" in l), None)
    chk("B2:guest-kernel-marker", kline is not None and kline == ver, (kline, ver))
    chk("B2:guest-init-started", count(r"TIER3_SSHD_GUEST_INIT_START", guest) == 1)
    chk("B2:crng-ready", count(r"TIER3_SSHD_CRNG_READY", guest) == 1 and count(r"CRNG_WAIT_TIMEOUT", guest) == 0)
    chk("B2:net-up", count(r"TIER3_SSHD_NET_UP \S+ 10\.0\.2\.16/24 gw 10\.0\.2\.2", guest) == 1)
    chk("B2:apk-installed-offline", count(r"TIER3_SSHD_APK_INSTALLED OpenSSH_\S+", guest) == 1)
    hostkey = [re.search(r"TIER3_SSHD_HOSTKEY \d+ (SHA256:\S+)", l) for l in guest if "TIER3_SSHD_HOSTKEY" in l]
    chk("B2:guest-hostkey-equals-pinned", len(hostkey) == 1 and hostkey[0] and hostkey[0].group(1) == fingerprint,
        [h.group(1) for h in hostkey if h])
    chk("B2:sshd-server-listening-line", count(r"Server listening on 0\.0\.0\.0 port 22\.", guest) == 1)
    chk("B2:listening-marker", count(r"TIER3_SSHD_LISTENING", guest) == 1)
    chk("B2:no-guest-init-failure", count(r"TIER3_SSHD_FAIL", guest) == 0, [l.strip() for l in guest if "TIER3_SSHD_FAIL" in l][:2])
    # C1: host-side probe (R4) only
    attempts = c1.get("attempts", [])
    chk("C1:keyscan-fingerprint-equals-guest-key", bool(c1.get("ok")) and any(a.get("match") for a in attempts),
        [(a.get("rc"), a.get("got_fingerprint")) for a in attempts][-3:])
    # C2: cell-side probe (R1)
    chk("C2:driver-ready-marker-once", count(r"P04 INFO ssh-guest-ready attempts=\d+", host) == 1
        and count(r"P04 INFO ssh-guest-ready never", host) == 0)
    conns = re.findall(r"Connection from 10\.0\.2\.2 port (\d+)", gtext)
    keyscan_conns = sum(1 for a in attempts if a.get("connected_returned_key"))
    chk("C2:sshd-saw-cell-connection-beyond-keyscan", len(conns) >= keyscan_conns + 1,
        {"connections_from_10.0.2.2": len(conns), "keyscan_connections": keyscan_conns})
    order = tl.get("timeline_s", {})
    # sshd's own `Server listening` line is the reference: the init's TIER3_SSHD_LISTENING marker
    # is printed by a polling loop (`sleep 1` between checks) and can trail a first connection.
    chk("C2:ready-after-sshd-server-listening", "sshd-server-listening" in order and "ssh-guest-ready" in order
        and order["sshd-server-listening"] <= order["ssh-guest-ready"], order)
    # QEMU argv invariant (narrower than the Phase 09 "no forwards" rule)
    argv_text = read("qemu-argv.txt").strip()
    argv = argv_text.split(" ") if argv_text else []
    netdevs = [argv[i + 1] for i, a in enumerate(argv[:-1]) if a == "-netdev"]
    want_nd = "user,id=net0,hostfwd=tcp:127.0.0.1:%s-10.0.2.16:22" % port
    pairs = {argv[i]: argv[i + 1] for i in range(len(argv) - 1) if argv[i].startswith("-")}
    objects = [argv[i + 1] for i, a in enumerate(argv[:-1]) if a == "-object"]
    if stage == "diag":
        # The one allowed addition (cp1-diag-draft.md section 3.3): the netdev capture.
        objects_ok = len(objects) == 1 and re.fullmatch(r"filter-dump,id=fd0,netdev=net0,file=/tmp/cellbox-tier3-[A-Za-z0-9._-]+/net0\.pcap", objects[0]) is not None
    else:
        objects_ok = not objects
    argv_ok = (netdevs == [want_nd] and argv_text.count("fwd") == 1 and "restrict" not in argv_text
               and argv_text.count("e1000") == 1 and pairs.get("-m") == "1G" and pairs.get("-accel") == "tcg"
               and pairs.get("-cpu") == EXPECTED_CPU and "-enable-kvm" not in argv and "kvm" not in argv_text.replace("-accel tcg", "")
               and pairs.get("-monitor") == "none" and objects_ok and argv_text.count("-object") == len(objects)
               and bool(argv) and argv[0].endswith("/qemu-10.2.0/bin/qemu-system-x86_64"))
    chk("argv:one-loopback-hostfwd-tcg-qemu-10.2.0", argv_ok, argv_text[:400])
    # Hashes
    prov = {}
    for l in read("provenance.sha256").splitlines():
        if "  " in l:
            h, n = l.split("  ", 1)
            prov[Path(n).name] = h
    pin_bad = {n: (prov.get(n), h) for n, h in PINS.items() if prov.get(n) != h}
    chk("hashes:pins-match-and-iso-kernel-initramfs-recorded",
        not pin_bad and all(n in prov for n in ("tier3.iso", "cellos-kernel", "initramfs-sshd.cpio.gz", "kernel_fs.img")), pin_bad)
    # No inbound Cellos sshd
    layout = read("fat-layout.txt")
    cfg = "".join(p.read_text(errors="replace") for p in sorted((ev / "boot-config").glob("*.toml"))) if (ev / "boot-config").is_dir() else ""
    chk("no-sshd-in-outer-image", "sshd" not in layout.lower() and "sshd" not in cfg.lower() and bool(layout) and bool(cfg))
    # Secrets
    if carry is not None:
        for name in ("no-secret-in-any-evidence-file", "no-secret-in-serial-raw"):
            old_check = carry["checks"][name]
            chk(name, old_check["pass"], "carried from the run's own verdict (keys no longer present): " + old_check["detail"])
    else:
        leaked = {}
        for p in sorted(ev.rglob("*")):
            if p.is_file() and p.name not in ("host_ed25519", "password", "seed.hex") and p.suffix != ".img":
                try:
                    hits = scan(p.read_bytes() if p.stat().st_size < 64 * 1024 * 1024 else b"", secrets)
                except OSError:
                    hits = []
                if hits:
                    leaked[str(p.relative_to(ev))] = hits
        chk("no-secret-in-any-evidence-file", not leaked, leaked)
        chk("no-secret-in-serial-raw", not scan(raw, secrets))
    # Protected paths
    before, after = read("protected-before.txt"), read("protected-after.txt")
    chk("protected-paths-and-cargo-lock-unchanged", bool(before) and before == after)
    diag_out = None
    net_info = None
    if stage == "boot":
        chk("run-ended-by-gates", tl.get("verdict") == "gates-satisfied", tl.get("verdict"))
    else:
        chk("run-ended-by-driver-verdict", tl.get("verdict") == "driver-verdict", tl.get("verdict"))
        if stage == "full":
            chk("window-within-4h-cap", 0 < int(tl.get("window_s", 0)) <= MAX_WINDOW_S, tl.get("window_s"))
        passes, fails = full_checks(chk, ev, log, host, guest, stage)
        net_info = net_owner_death(log.splitlines())
        chk("net:no-owner-death-watch-unavailable", not net_info["watch_unavailable"],
            "a host line says `owner death watch unavailable`" if net_info["watch_unavailable"] else "")
        chk("net:owner-death-reclaim-logged-in-cancel-window", net_info["in_cancel_window"],
            net_info["owner_death_lines"] or "no `[net] owner death:` line")
        if stage == "diag":
            diag_out = diag_checks(chk, ev, log, host, guest, tl, passes, fails)
            if expect_outcome:
                chk("diag:outcome-is-" + expect_outcome, diag_out["outcome"] == expect_outcome,
                    "outcome %s: %s" % (diag_out["outcome"], diag_out["outcome_why"]))

    # STOP classification (cp1-draft.md section 5; the diag stage names them STOP-D1..D5 of cp1-diag-draft.md section 8)
    b1 = all(v["pass"] for k, v in checks.items() if k.startswith("B1:"))
    b2 = all(v["pass"] for k, v in checks.items() if k.startswith("B2:"))
    c1ok = checks["C1:keyscan-fingerprint-equals-guest-key"]["pass"]
    c2ok = all(v["pass"] for k, v in checks.items() if k.startswith("C2:"))
    stop = None
    secrets_bad = (not checks["no-secret-in-any-evidence-file"]["pass"] or not checks["no-secret-in-serial-raw"]["pass"]
                   or (stage == "diag" and not checks["diag:no-secret-in-pcap"]["pass"]))
    if secrets_bad:
        stop = "STOP-D2" if stage == "diag" else "STOP-5"
    elif not checks["protected-paths-and-cargo-lock-unchanged"]["pass"]:
        stop = "STOP-D1" if stage == "diag" else "STOP-6"
    elif stage == "diag" and not checks["argv:one-loopback-hostfwd-tcg-qemu-10.2.0"]["pass"]:
        stop = "STOP-D3"
    elif stage in ("full", "diag") and net_info["watch_unavailable"]:
        stop = "STOP-7"
    elif not (b1 and b2 and c1ok and c2ok):
        stop = "STOP-D4" if stage == "diag" else ("STOP-1" if not b1 else "STOP-2" if not b2 else "STOP-3" if not c1ok else "STOP-4")
    elif stage == "diag" and not checks["diag:window-within-1h-cap"]["pass"]:
        stop = "STOP-D5"
    elif stage in ("full", "diag") and not all(v["pass"] for k, v in checks.items() if k.startswith(("D:", "diag:", "net:"))):
        stop = "MARKER-FAIL"
    gates = {"B1": b1, "B2": b2, "C1": c1ok, "C2": c2ok}
    ok = all(v["pass"] for v in checks.values())
    verdict = {
        "head": head, "pass": ok, "stop_rule": stop, "gates": gates, "guest_kernel": ver,
        "stage": stage, "time_to_listening_s": order.get("listening"), "time_to_ssh_guest_ready_s": order.get("ssh-guest-ready"),
        "timeline_s": order, "run_verdict": tl.get("verdict"), "checks": checks,
    }
    if diag_out is not None:
        verdict["diag"] = diag_out
    if net_info is not None:
        verdict["net"] = {"owner_death_lines": net_info["owner_death_lines"], "watch_unavailable": net_info["watch_unavailable"],
                          "in_cancel_window": net_info["in_cancel_window"]}
    return verdict, stop


def cmd_boot(a):
    ev = Path(a.evidence)
    carry = None
    secrets = None
    if a.carry_secret_checks_from:
        carry = json.loads(Path(a.carry_secret_checks_from).read_text())
    else:
        secrets = secrets_from(Path(a.keys))
        if not secrets:
            print("FAIL: no secrets found under --keys (cannot scan)", file=sys.stderr)
            return 2
    stage = a.cmd if a.cmd in ("full", "diag") else "boot"
    if a.expect_outcome and (stage != "diag" or a.expect_outcome not in OUTCOMES):
        print("FAIL: --expect-outcome takes one of %s and is only valid for the diag stage" % ", ".join(OUTCOMES), file=sys.stderr)
        return 2
    if stage == "diag":
        if carry is not None or not a.pcap:
            print("FAIL: the diag stage needs --keys and --pcap (no carry mode)", file=sys.stderr)
            return 2
        prep_diag(ev, Path(a.pcap), secrets, a.port)
    verdict, stop = judge_boot(ev, secrets, a.fingerprint, a.port, a.head, carry, stage, a.expect_outcome)
    out = Path(a.verdict_out) if a.verdict_out else ev / "verdict.json"
    out.write_text(json.dumps(verdict, indent=2) + "\n")
    for n, v in verdict["checks"].items():
        print(("PASS " if v["pass"] else "FAIL ") + n + ("" if v["pass"] else "  <- " + v["detail"]))
    print("gates:", verdict["gates"], "stop_rule:", stop, "time_to_listening_s:", verdict["time_to_listening_s"])
    if stage == "diag":
        dg = verdict["diag"]
        print("OUTCOME %s: %s (%s)" % (dg["outcome"], dg["outcome_text"], dg["outcome_why"]))
    label = {"full": "full run (T3-D3 class)", "diag": "diagnostic run (T3-D3 class)"}.get(stage, "boot stage B1 B2 C1 C2")
    print(("PASS: Tier-3 " + label) if verdict["pass"] else "FAIL: Tier-3 %s (%s)" % (label, stop or "see checks"))
    return 0 if verdict["pass"] else 1


def cmd_archive(a):
    src, dst, keys = Path(a.src), Path(a.dst), Path(a.keys)
    secrets = secrets_from(keys)
    if not secrets:
        print("FAIL: no secrets found under --keys (cannot scan)", file=sys.stderr)
        return 2
    files = [src / n for n in ARCHIVE_FILES if (src / n).is_file()]
    for d in ARCHIVE_DIRS:
        files += sorted(p for p in (src / d).rglob("*") if p.is_file()) if (src / d).is_dir() else []
    files += sorted(src.glob("host-ss-*.txt"))
    leaked = {str(p.relative_to(src)): scan(p.read_bytes(), secrets) for p in files}
    leaked = {k: v for k, v in leaked.items() if v}
    if leaked:
        print("FAIL: secret material found, nothing archived: %s" % json.dumps(leaked), file=sys.stderr)
        return 1
    for p in files:
        target = dst / p.relative_to(src)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(p, target)
    print("archived %d text files to %s" % (len(files), dst))
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("boot", "full", "diag"):
        b = sub.add_parser(name)
        b.add_argument("--evidence", required=True)
        b.add_argument("--keys", default="")
        b.add_argument("--carry-secret-checks-from", default="", help="earlier verdict.json whose secret-scan checks are carried over (keys already deleted)")
        b.add_argument("--verdict-out", default="", help="write the verdict here instead of EVIDENCE/verdict.json")
        b.add_argument("--fingerprint", required=True)
        b.add_argument("--port", required=True)
        b.add_argument("--head", default="")
        b.add_argument("--pcap", default="", help="diag stage: the raw slirp-netdev pcap (kept outside the evidence directory)")
        b.add_argument("--expect-outcome", default="", help="diag stage only: adds the check diag:outcome-is-LABEL (O1..O7)")
        b.set_defaults(fn=cmd_boot)
    r = sub.add_parser("archive")
    r.add_argument("--src", required=True)
    r.add_argument("--dst", required=True)
    r.add_argument("--keys", required=True)
    r.set_defaults(fn=cmd_archive)
    a = ap.parse_args(argv)
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
