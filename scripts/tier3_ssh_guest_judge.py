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
           rule `MARKER-FAIL`: no fix-and-rerun without a new CP1.
  archive  Copy the text evidence to the repository after a secret scan (STOP-5): refuses to copy
           anything when a seed, password or host private key is found in any candidate file.

Exit codes: 0 pass; 1 a gate failed or a STOP rule applies; 2 usage/host error.
"""
import argparse
import base64
import json
import re
import shutil
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
ARCHIVE_FILES = [
    "serial.log", "qemu-argv.txt", "timeline.json", "c1-keyscan.json", "verdict.json", "fat-layout.txt",
    "provenance.sha256", "provenance-build.txt", "initramfs-audit.json", "preflight.txt", "preflight.final.txt",
    "protected-before.txt", "protected-after.txt", "guest-host-key.fingerprint", "guest-host-key.pub",
    "hostfwd-port.txt", "prepare-initramfs.txt",
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


def full_checks(chk, ev: Path, log: str, host, guest):
    """T3-D checks on the driver's markers and the guest sshd's log (serial order is time order)."""
    passes, fails = {}, []
    for line in host:
        m = re.search(r"P04 (PASS|FAIL) (\S+)(.*)$", line)
        if m:
            if m.group(1) == "PASS":
                passes[m.group(2)] = passes.get(m.group(2), 0) + 1
            else:
                fails.append((m.group(2), m.group(3).strip()[:160]))
    chk("D:driver-started", any("P04 START" in l for l in host))
    for name in EXPECTED_MARKERS:
        chk("D:marker:" + name, passes.get(name) == 1 and not any(f[0] == name for f in fails),
            "missing" if name not in passes else ("duplicate" if passes[name] != 1 else "FAIL present"))
    chk("D:no-unexpected-markers", set(passes) <= set(EXPECTED_MARKERS), sorted(set(passes) - set(EXPECTED_MARKERS)))
    chk("D:no-FAIL-markers", not fails, fails)
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
    split = next((i for i, l in enumerate(lines) if "P04 INFO p09-case ssh-guest-cancel-producer" in l), None)
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
    chk("D:guest-scenario-logins-accounted", len(logins) == 1 and 4 <= acc_after <= int(logins[0]) and n(r"Failed password for tester", after) == 0,
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


def judge_boot(ev: Path, secrets, fingerprint: str, port: str, head: str = "", carry=None, stage: str = "boot"):
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
    argv_ok = (netdevs == [want_nd] and argv_text.count("fwd") == 1 and "restrict" not in argv_text
               and argv_text.count("e1000") == 1 and pairs.get("-m") == "1G" and pairs.get("-accel") == "tcg"
               and pairs.get("-cpu") == EXPECTED_CPU and "-enable-kvm" not in argv and "kvm" not in argv_text.replace("-accel tcg", "")
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
    if stage == "boot":
        chk("run-ended-by-gates", tl.get("verdict") == "gates-satisfied", tl.get("verdict"))
    else:
        chk("run-ended-by-driver-verdict", tl.get("verdict") == "driver-verdict", tl.get("verdict"))
        chk("window-within-4h-cap", 0 < int(tl.get("window_s", 0)) <= MAX_WINDOW_S, tl.get("window_s"))
        full_checks(chk, ev, log, host, guest)

    # STOP classification (cp1-draft.md section 5)
    b1 = all(v["pass"] for k, v in checks.items() if k.startswith("B1:"))
    b2 = all(v["pass"] for k, v in checks.items() if k.startswith("B2:"))
    c1ok = checks["C1:keyscan-fingerprint-equals-guest-key"]["pass"]
    c2ok = all(v["pass"] for k, v in checks.items() if k.startswith("C2:"))
    stop = None
    if not checks["no-secret-in-any-evidence-file"]["pass"] or not checks["no-secret-in-serial-raw"]["pass"]:
        stop = "STOP-5"
    elif not checks["protected-paths-and-cargo-lock-unchanged"]["pass"]:
        stop = "STOP-6"
    elif not b1:
        stop = "STOP-1"
    elif not b2:
        stop = "STOP-2"
    elif not c1ok:
        stop = "STOP-3"
    elif not c2ok:
        stop = "STOP-4"
    elif stage == "full" and not all(v["pass"] for k, v in checks.items() if k.startswith("D:")):
        stop = "MARKER-FAIL"
    gates = {"B1": b1, "B2": b2, "C1": c1ok, "C2": c2ok}
    ok = all(v["pass"] for v in checks.values())
    verdict = {
        "head": head, "pass": ok, "stop_rule": stop, "gates": gates, "guest_kernel": ver,
        "stage": stage, "time_to_listening_s": order.get("listening"), "time_to_ssh_guest_ready_s": order.get("ssh-guest-ready"),
        "timeline_s": order, "run_verdict": tl.get("verdict"), "checks": checks,
    }
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
    verdict, stop = judge_boot(ev, secrets, a.fingerprint, a.port, a.head, carry, "full" if a.cmd == "full" else "boot")
    out = Path(a.verdict_out) if a.verdict_out else ev / "verdict.json"
    out.write_text(json.dumps(verdict, indent=2) + "\n")
    for n, v in verdict["checks"].items():
        print(("PASS " if v["pass"] else "FAIL ") + n + ("" if v["pass"] else "  <- " + v["detail"]))
    print("gates:", verdict["gates"], "stop_rule:", stop, "time_to_listening_s:", verdict["time_to_listening_s"])
    label = "full run (T3-D)" if a.cmd == "full" else "boot stage B1 B2 C1 C2"
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
    for name in ("boot", "full"):
        b = sub.add_parser(name)
        b.add_argument("--evidence", required=True)
        b.add_argument("--keys", default="")
        b.add_argument("--carry-secret-checks-from", default="", help="earlier verdict.json whose secret-scan checks are carried over (keys already deleted)")
        b.add_argument("--verdict-out", default="", help="write the verdict here instead of EVIDENCE/verdict.json")
        b.add_argument("--fingerprint", required=True)
        b.add_argument("--port", required=True)
        b.add_argument("--head", default="")
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
