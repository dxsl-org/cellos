#!/usr/bin/env python3
"""Phase 10 S10-3.2 L0: boot, drive and judge the exact candidate image (no fixture kernel).

Called only by scripts/qemu-phase10-canonical-test.sh, after the runner verified every input
against candidate.json. The driver owns one QEMU process at a time (never touches another
VM), talks to it through private unix serial and QMP sockets in the work directory, and
writes only under the work directory.

Subcommands:
  fixtures OUT_DIR        write the FAT data-volume tree (cat/edit/VT fixtures) the runner
                          copies onto P1 (/mnt/sd) of the main fixture disk.
  run ...                 boot the candidate ISO (main boot: restricted SLIRP; ssh boot: the
                          D79-1 fixture network) and write verdict.json.

Headless (`--profile headless`): the serial `cellbox sh` on the physical UART is driven by
typed bytes; every judged value is read from the UART log (the console's `USER: ` records).
Desktop (`--profile desktop`): only the GUI is driven (QMP input + screendump; the serial
shell is never scripted, decision (c)). The pixel oracle is imported from
scripts/phase04_gui_drive.py (`Proof`: the `phase04-gui-glyphs` renderer, exact window
match, cursor detector, negative controls, the S5-2 VT, S5-4 hostile and S6-4 `top`
scenarios), not copied. Screens whose text is not known in advance (`date`, ssh output) are
read cell by cell against glyphs rendered by the same helper and then accepted only if the
whole window re-rendered from the read text matches with zero differing pixels.
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops

sys.path.insert(0, str(Path(__file__).resolve().parent))
import phase04_gui_drive as gui  # noqa: E402
import phase05_vt_hostile  # noqa: E402
from phase04_gui_drive import (  # noqa: E402
    BANNER, BG, CELL_H, CELL_W, COLS, MIN_NEGATIVE_DIFF, PROMPT, ROWS, WIN_H, WIN_W,
    Proof, Qmp, Serial, StepFailed,
)

MNT = "/mnt/sd"
RG = MNT + "/rg"

# ---- FAT data volume (P1 /mnt/sd) ---------------------------------------------------------
# The cat/VT bytes are those of qemu-phase04-gui-test.sh's VT fixture block (same literals, same order); the
# hostile fixture comes from scripts/phase05_vt_hostile.py.
VT_SAMPLE_BYTES = b"".join([
    b"\033[0m\033[?25l\033[2J",
    b"\033[1;1HJUNK-ROW-0\033[2;1HJUNK00ed1-kept\033[2;6H\033[1J",
    b"\033[7;1H\033[7minverse\033[0m \033[7;31minv-red\033[0m \033[1;7;33mbold-inv\033[0m",
    b"\033[3;1H\033[31mred\033[0m \033[1;33mbold-yellow\033[0m \033[97;41mwhite-on-red\033[0m",
    b"\033[6;1H\033[38;2;250;200;100mrgb-fg\033[0m \033[48;2;60;30;90mrgb-bg\033[0m",
    b"\033[5;1H\033[38;5;208midx208\033[39m \033[48;5;53mon53\033[0m",
    b"\033[4;1H\033[30;103m black-on-yellow \033[0m \033[95mmagenta95\033[0m",
    b"\033[9;1HXXXXXXXXXXkeep-right\033[9;10H\033[43m\033[1K\033[0m",
    b"\033[10;1Herase-tail:YYYYYYYY\033[10;12H\033[K",
    b"\033[11;1HZZZZZZZZZZZZ\033[2K\033[11;3H2K",
    b"\033[13;1Hed0-junk-will-vanish\033[14;1Hmore-junk\033[13;5H\033[J",
    b"\033[?1049h\033[H\033[45mALT-SHOULD-VANISH\033[3;1HALT-SHOULD-VANISH\033[0m\033[?1049l",
    b"\033[0m\033[1;1H\033[1mcellos-vt1 sample\033[0m",
    b"\033[?25h\033[15;1Hend-of-sample\n",
])
VT_ALT_ENTER_BYTES = (b"\033[?1049h\033[1;1H\033[1;97;45m ALT SCREEN \033[0m\033[3;5H\033[31malt-red\033[0m"
                      b" \033[7malt-inverse\033[0m\033[5;1H")
VT_ALT_EXIT_BYTES = b"\033[?1049l\033[0m"
DIR_ENTRY = b"private known directory fixture\n"
NOTE = b"gui-cat-line-one\ngui-cat-line-two\n"
# Headless fixtures (Phase 10 only).
BYTES_TXT = b"p10-bytes-line-1\np10-bytes-line-2 tail\n"
CRLF_TXT = b"one\r\ntwo\nlast\n"
STATE_SH = (b"p10s=from-script\n"
            b"echo src-start\n"
            b"for w in x y; do echo src-$w; done\n"
            b"ls /p10-src-missing\n"
            b"echo src-status=$?\n")
# S10-3b: invalid UTF-8 (a C1 CSI byte, 0xFF 0xFE, a truncated 4-byte sequence) around a valid multibyte
# character; builtin `cat` prints one U+FFFD per invalid sequence, as it does from the boot image.
LOSSY_TXT = b"p10-lossy \x9b31m \xff\xfe \xe2\x82\xac \xf0\x9f end\n"


def raw_payload(n):
    """The P04 lane's non-UTF-8 payload pattern (NUL, 0xFF, a multibyte character)."""
    data = bytearray((i * 73 + 19) % 256 for i in range(n))
    data[31:33] = b"\x00\xff"
    data[511:514] = b"\xe2\x82\xac"
    return bytes(data)


RAW_BIN = raw_payload(1600)


def data_tree():
    return {
        "gui-list/alpha-entry": DIR_ENTRY,
        "gui-list/beta-entry": DIR_ENTRY,
        "gui-text/note.txt": NOTE,
        "p04-output/vt-sample.bin": VT_SAMPLE_BYTES,
        "p04-output/vt-alt-enter.bin": VT_ALT_ENTER_BYTES,
        "p04-output/vt-alt-exit.bin": VT_ALT_EXIT_BYTES,
        "p04-output/vt-hostile.bin": phase05_vt_hostile.fixture(),
        "p10/bytes.txt": BYTES_TXT,
        "p10/raw.bin": RAW_BIN,
        "p10/crlf.txt": CRLF_TXT,
        "p10/state.sh": STATE_SH,
        "p10/lossy.txt": LOSSY_TXT,
    }


def write_fixtures(out):
    out = Path(out)
    manifest = {}
    for rel, data in data_tree().items():
        path = out / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        manifest[MNT + "/" + rel] = {"length": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    return manifest


# ---- secrets and log hygiene -------------------------------------------------------------
ANSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b[=>78NOM]")
KERNEL_LINE = re.compile(r"^\[\s?(?:INFO|WARN|ERROR|DEBUG|TRACE)\]")
PANIC = re.compile(r"panic|PANIC|page fault|PAGE FAULT|cell fault|CELL FAULT|OOM: cell heap")


def strip_ansi(b):
    return ANSI.sub(b"", b)


class Marker:
    def __init__(self, name):
        self.name, self.checks, self.notes, self.error = name, {}, [], None

    def chk(self, name, ok, detail=""):
        self.checks[name] = {"pass": bool(ok), "detail": detail if isinstance(detail, (str, int, float, bool)) else repr(detail)[:2000]}
        print(("  PASS " if ok else "  FAIL ") + self.name + ":" + name + ("" if ok else "  <- " + repr(detail)[:400]), flush=True)
        return bool(ok)

    def note(self, text):
        self.notes.append(text)

    @property
    def passed(self):
        return self.error is None and bool(self.checks) and all(c["pass"] for c in self.checks.values())

    def record(self):
        return {"pass": self.passed, "checks": self.checks, "notes": self.notes, "error": self.error}


class Verdict:
    def __init__(self, expected):
        self.expected = list(expected)
        self.markers = {}

    def marker(self, name):
        assert name in self.expected, name
        m = self.markers.setdefault(name, Marker(name))
        return m


class MarkerScope:
    """`with scope(verdict, NAME) as m:` - an exception inside fails that marker only."""

    def __init__(self, verdict, name):
        self.m = verdict.marker(name)

    def __enter__(self):
        print("==> " + self.m.name, flush=True)
        return self.m

    def __exit__(self, kind, exc, tb):
        if exc is not None:
            self.m.error = f"{kind.__name__}: {exc}"
            print(f"  FAIL {self.m.name}: aborted: {self.m.error}", flush=True)
            return isinstance(exc, (StepFailed, AssertionError, ValueError, KeyError, IndexError, TimeoutError))
        return False


# ---- QEMU ---------------------------------------------------------------------------------
class Machine:
    """One owned QEMU: private serial + QMP sockets, the candidate ISO read-only."""

    def __init__(self, a, label, disk, ssh):
        self.a, self.label, self.ssh = a, label, ssh
        self.sock = Path(a.work) / "sock"
        self.sock.mkdir(mode=0o700, exist_ok=True)
        self.serial_path, self.qmp_path = self.sock / f"{label}-serial.sock", self.sock / f"{label}-qmp.sock"
        cpu = "qemu64,+pdpe1gb,+rdrand,+rdseed" if ssh else "qemu64,+pdpe1gb"
        netdev = "user,id=net0" if ssh else "user,id=net0,restrict=on"
        self.argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", a.accel,
                     "-cpu", cpu if a.accel == "tcg" else "host", "-m", "256M", "-display", "none",
                     "-chardev", f"socket,id=ser0,path={self.serial_path},server=on,wait=off", "-serial", "chardev:ser0",
                     "-qmp", f"unix:{self.qmp_path},server=on,wait=off", "-monitor", "none",
                     "-drive", f"file={a.iso},media=cdrom,readonly=on,format=raw", "-boot", "d", "-no-reboot",
                     "-netdev", netdev, "-device", "e1000,netdev=net0",
                     "-drive", f"file={disk},if=none,format=raw,id=p10fx", "-device", "nvme,drive=p10fx,serial=cellosp10"]
        if a.profile == "desktop":
            self.argv += ["-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"]
        netdevs = [self.argv[i + 1] for i, x in enumerate(self.argv) if x == "-netdev"]
        if (netdevs != [netdev] or any(x in ("-nic", "-net") for x in self.argv)
                or any("fwd" in x for x in self.argv)):
            raise SystemExit("FAIL: QEMU networking must be one user netdev without forwards")
        (Path(a.evidence) / f"qemu-argv-{label}.txt").write_text(" ".join(self.argv).replace(a.work, "$WORK") + "\n")

    def start(self):
        for p in (self.serial_path, self.qmp_path):
            p.unlink(missing_ok=True)
        self.t0 = time.time()
        self.proc = subprocess.Popen(self.argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=open(Path(self.a.evidence) / f"qemu-stderr-{self.label}.txt", "wb"))
        # The wrapper distinguishes every pre-launch failure from a counted candidate run.
        (Path(self.a.evidence) / "qemu-started").write_text(self.label + "\n")
        self.serial = Serial(str(self.serial_path))
        self.serial.connect()
        self.serial.start()
        self.qmp = Qmp(str(self.qmp_path))
        return self

    def alive(self):
        return self.proc.poll() is None

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.serial.stop = True
        raw = self.serial.raw()
        ev = Path(self.a.evidence)
        (ev / f"serial-{self.label}.raw.log").write_bytes(raw)
        (ev / f"serial-{self.label}.log").write_text(raw.replace(b"\x00", b"").decode("utf-8", "replace"))
        for p in (self.serial_path, self.qmp_path):
            p.unlink(missing_ok=True)
        return raw

    def text(self):
        return self.serial.text()

    def wait_serial(self, needle, timeout):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.serial.text():
                return True
            if not self.alive():
                raise StepFailed(f"QEMU exited while waiting for {needle!r}")
            time.sleep(0.3)
        return False


# ---- the serial shell (headless) ----------------------------------------------------------
def logical_lines(raw):
    """UART bytes -> shell lines. `USER: ` opens a Cell line, `USER+ ` continues the one a
    kernel line interrupted (kernel ConsoleLine contract); kernel lines are dropped; bytes a
    TUI wrote without framing are appended to the current line."""
    out = []
    text = raw.replace(b"\x00", b"").decode("utf-8", "replace")
    for n, phys in enumerate(text.split("\n")):
        phys = phys.rstrip("\r")
        if phys.startswith("USER: "):
            out.append(phys[6:])
        elif phys.startswith("USER+ "):
            if out:
                out[-1] += phys[6:]
            else:
                out.append(phys[6:])
        elif KERNEL_LINE.match(phys):
            continue
        elif n == 0 or not out:
            out.append(phys)
        else:
            out[-1] += "\n" + phys
    return out


# An asynchronous Cell line printed by a service after the prompt (`[net-bridge] NIC driver reply
# timeout…`, `[serial] serial cell ready…`, `[nvme] slow completion…`).
SERVICE_LINE = re.compile(r"^\[[a-z0-9-]+\] ")

# A Ratatui exit restores the alternate screen and terminal modes, then the shell writes
# its prompt without a newline or another USER prefix. Preserve that explicit boundary
# before ANSI removal; an arbitrary output line merely ending in "Cellos > " is not one.
TUI_PROMPT_BOUNDARY = re.compile(
    rb"(\x1b\[\?1049l(?:\x1b\[[0-?]*[ -/]*[@-~])*)(?=Cellos > )")


def shell_lines(raw):
    framed = TUI_PROMPT_BOUNDARY.sub(rb"\1\nUSER: ", raw)
    return logical_lines(strip_ansi(framed))


def ends_on_prompt(lines):
    """Only an empty prompt line can precede ignored asynchronous background records."""
    for line in reversed(lines):
        for tail in reversed(line.split("\n")):
            if not tail or SERVICE_LINE.match(tail) or KERNEL_LINE.match(tail):
                continue
            return tail == "Cellos > "
    return False


class Shell:
    """`cellbox sh` on the physical UART: type a line, wait for the next prompt, return the
    lines between the echoed command and that prompt (stdout and stderr, in UART order)."""

    def __init__(self, m):
        self.m = m

    def raw(self):
        return self.m.serial.raw()

    def prompts(self, raw=None):
        lines = shell_lines(raw if raw is not None else self.raw())
        return sum(line.startswith("Cellos > ") for line in lines)

    def idle(self, raw=None, count=0):
        """An empty prompt (possibly followed by background logs) and the required prompt count."""
        raw = raw if raw is not None else self.raw()
        return self.prompts(raw) >= count and ends_on_prompt(shell_lines(raw))

    def wait_prompt(self, count, timeout):
        end = time.time() + timeout
        while time.time() < end:
            raw = self.raw()
            if self.idle(raw, count=count):
                return True
            if not self.m.alive():
                raise StepFailed("QEMU exited")
            time.sleep(0.25)
        return False

    def send(self, data):
        self.m.serial.conn.sendall(data)

    def submit(self, cmd, enter=b"\r"):
        raw = self.raw()
        before = self.prompts(raw)
        self.send(cmd.encode() + enter)
        return before, len(raw)

    def run(self, cmd, timeout=120, enter=b"\r"):
        """(output lines, raw UART bytes of the whole exchange)."""
        before, at = self.submit(cmd, enter)
        if not self.wait_prompt(before + 1, timeout):
            raise StepFailed(f"no prompt after {cmd!r} within {timeout}s")
        seg = self.raw()[at:]
        lines = logical_lines(strip_ansi(seg))
        # The echo continues the old prompt; remove only the new prompt, retaining background logs.
        echo = lines[0] if lines else ""
        if cmd not in echo.split("\n")[0]:
            raise StepFailed(f"echo of {cmd!r} not seen: {echo[:200]!r}")
        output = lines[1:]
        for i in range(len(output) - 1, -1, -1):
            if output[i].split("\n")[0] == "Cellos > ":
                output[i] = output[i][len("Cellos > "):].removeprefix("\n")
                if not output[i]:
                    del output[i]
                break
        return output, seg

    def status(self, var="st"):
        lines, _ = self.run(f"echo {var}=$?", 60)
        vals = [l for l in lines if l.startswith(var + "=")]
        if len(vals) != 1:
            raise StepFailed(f"status line not found: {lines}")
        v = vals[0][len(var) + 1:]
        return int(v) if v.lstrip("-").isdigit() else v

    def run_st(self, cmd, timeout=120):
        lines, seg = self.run(cmd, timeout)
        return lines, self.status(), seg

    def interrupt(self, cmd, after, settle_pred=None, timeout=60):
        """Type `cmd`, wait `after` s (or until settle_pred(seg)), send 0x03, wait the prompt."""
        before, at = self.submit(cmd)
        end = time.time() + max(after, 1)
        while time.time() < end and not (settle_pred and settle_pred(self.raw()[at:])):
            time.sleep(0.25)
        t_cc = time.time()
        self.send(b"\x03")
        ok = self.wait_prompt(before + 1, timeout)
        return ok, time.time() - t_cc, self.raw()[at:]


def json_rows(lines):
    out = []
    for l in lines:
        l = l.strip()
        if l.startswith("{"):
            out.append(json.loads(l))
    return out


class Headless:
    MARKERS = ["P10C-boot", "P10C-lf-submit", "P10C-image-list", "P10C-explicit-forms", "P10C-state-script",
               "P10C-typed", "P10C-bytes", "P10C-rg", "P10C-top", "P10C-edit", "P10C-selective-tui",
               "P10C-denials", "P10C-ssh-serial", "P10C-no-inbound", "P10C-ctrl-c", "P10C-not-on-uart"]

    def __init__(self, a, verdict, cand):
        self.a, self.v, self.cand = a, verdict, cand
        self.manifest = cand["manifest"]
        self.rg = json.loads(Path(a.rg_expected).read_text())

    def m(self, name):
        return MarkerScope(self.v, name)

    # -- boot plan from the candidate's own boot configuration ----------------------------
    def plan(self):
        import tomllib
        cfg = Path(self.cand["dir"]) / "boot-config"
        services = tomllib.loads((cfg / "services.toml").read_text()).get("cells", [])
        autoload = tomllib.loads((cfg / "autoload.toml").read_text()).get("cells", [])
        return services, autoload

    def main_boot(self):
        a = self.a
        mach = Machine(a, "main", a.disk_main, ssh=False).start()
        self.mach, sh = mach, None
        try:
            with self.m("P10C-boot") as m:
                ok = mach.wait_serial("Cellos > ", a.window)
                t_prompt = time.time() - mach.t0
                m.chk("prompt-reached", ok, f"T={t_prompt:.1f}s")
                if not ok:
                    raise StepFailed("no shell prompt")
                m.note(f"first `Cellos >` at T={t_prompt:.1f}s after QEMU start (TCG)")
                time.sleep(5)  # let the post-prompt probe cells finish their boot lines
                sh = self.sh = Shell(mach)
                self.boot_checks(m, mach.text())
            # P10C-ctrl-c runs last: an uninterruptible foreground (if any) cannot block the rest.
            steps = [self.lf_submit, self.image_list, self.explicit_forms, self.typed, self.bytes_, self.rg_,
                     self.top, self.edit, self.selective_tui, self.denials, self.state_script,
                     self.no_inbound_main, self.ctrl_c]
            for step in steps:
                if not mach.alive():
                    break
                step()
        finally:
            self.main_raw = mach.stop()

    def boot_checks(self, m, log):
        boot = log[:log.find("Cellos > ")]
        m.chk("policy-loaded-verified", re.search(r"\[policy\] loaded \+ verified \(\d+ entries", log) is not None,
              [l for l in log.splitlines() if "[policy]" in l and "narrowed" not in l][:3])
        m.chk("tier2-disabled-fleet", "Tier 2 admission: DISABLED (fleet profile)" in log,
              [l for l in log.splitlines() if "Tier 2 admission" in l])
        m.chk("no-panic", not PANIC.search(log), [l for l in log.splitlines() if PANIC.search(l)][:3])
        denies = [l for l in boot.splitlines() if "DENY" in l]
        m.chk("no-deny-at-boot", not denies, denies[:3])
        sig = [l for l in log.splitlines() if re.search(r"(?i)signature.*(fail|invalid|reject|refus)|unsigned.*refus", l)]
        m.chk("signatures-admitted", not sig, sig[:3])
        files = self.manifest["files"]
        refused = re.findall(r"SpawnFromPath refused: caller=\d+ path=(\S+) error=(\S+)", log)
        bad = [(p, e) for p, e in refused if p in files or e != "NotFound"]
        m.chk("no-spawn-failure", not bad, bad)
        m.note("optional absent probe paths (NotFound, not in the manifest): " + ", ".join(sorted({p for p, _ in refused})))
        narrowed = sorted(set(re.findall(r'\[policy\] narrowed "([^"]+)"', log)))
        m.note("fleet profile narrowed paths (expected, D3a-2 A): " + ", ".join(narrowed))
        m.note("known non-blocking (D3-5): " + "; ".join(l.strip() for l in log.splitlines() if "COMPLETION-QUEUE" in l)[:300])
        services, autoload = self.plan()
        launched = set(re.findall(r"Init: (\S+) launched\.", log))
        want = [c["name"] for c in services + autoload if c.get("enabled", True)]
        m.chk("every-planned-cell-launched", set(want) <= launched, sorted(set(want) - launched))
        rows = json_rows(self.sh.run("ps --records | select tid name | to jsonl", 60)[0])
        names = [r["name"] for r in rows]
        self.boot_ps = names
        always = [c["name"] for c in services if c.get("restart") == "always"] + [c["name"] for c in autoload]
        early = {"init", "vfs", "nvme", "ahci", "platform"}  # kernel/init block-table cells (not in the toml plan)
        allowed = set(want) | early
        m.chk("ps-required-running", set(always) | {"init", "vfs"} <= set(names), (always, names))
        m.chk("ps-only-planned-cells", set(names) <= allowed, sorted(set(names) - allowed))
        m.chk("ps-no-sshd-no-shell", not ({"sshd", "shell"} & set(names)), names)
        m.chk("manifest-no-sshd-no-shell", "/bin/sshd" not in files and "/bin/shell" not in files, sorted(files))
        exited = [c["name"] for c in services if c["name"] not in names]
        clean = log.count("Init: service exited cleanly")
        m.chk("absent-hardware-drivers-exited-cleanly", clean >= len([n for n in exited if n in ("igb", "xhci")]),
              f"not running: {exited}; clean exits logged: {clean}")
        m.note(f"ps --records names: {names}")

    def lf_submit(self):
        with self.m("P10C-lf-submit") as m:
            sh = self.sh
            got = {}
            for tag, enter in (("n", b"\n"), ("r", b"\r"), ("rn", b"\r\n")):
                token = f"p10lf-{tag}"
                before, at = sh.submit(f"echo {token}", enter)
                ok = sh.wait_prompt(before + 1, 60)
                time.sleep(3)  # a CRLF client may submit a second (empty) line
                sh.wait_prompt(before + 1, 30)
                seg = sh.raw()[at:]
                lines = logical_lines(strip_ansi(seg))
                runs = sum(1 for l in lines[1:] if l == token)
                prompts = strip_ansi(seg).count(b"Cellos > ")
                got[tag] = {"ran": runs, "prompts": prompts}
                m.chk(f"{tag}-runs-once", ok and runs == 1, got[tag])
            m.note(f"CRLF: {got['rn']['prompts']} prompt(s) after one `cmd\\r\\n` line (an empty second submission is recorded, not a failure)")

    def image_list(self):
        with self.m("P10C-image-list") as m:
            lines, st, _ = self.sh.run_st("ls /bin")
            want = sorted(p[len("/bin/"):] for p in self.manifest["files"] if p.startswith("/bin/"))
            m.chk("ls-bin-equals-manifest", sorted(lines) == want and st == 0, {"ls": lines, "manifest": want, "st": st})
            m.chk("cellbox-and-cellbox-ssh-present", "cellbox" in lines and "cellbox-ssh" in lines, lines)
            files = self.cand["files"]
            m.chk("cellbox-ssh-same-signed-file",
                  files["/bin/cellbox"]["sha256"] == files["/bin/cellbox-ssh"]["sha256"],
                  (files["/bin/cellbox"]["sha256"], files["/bin/cellbox-ssh"]["sha256"]))

    def explicit_forms(self):
        with self.m("P10C-explicit-forms") as m:
            outs = {}
            for cmd in ("ls /", "cellbox ls /", "/bin/cellbox ls /"):
                lines, st, _ = self.sh.run_st(cmd)
                outs[cmd] = (lines, st)
            first = outs["ls /"]
            m.chk("equal-bytes-and-status", all(v == first for v in outs.values()) and first[1] == 0 and first[0], outs)
            lines, st, _ = self.sh.run_st("cellbox cd /")
            m.chk("cellbox-cd-refused-2", st == 2 and any("cellbox sh" in l for l in lines), (lines, st))

    def state_script(self):
        with self.m("P10C-state-script") as m:
            sh = self.sh
            m.chk("variable", sh.run("p10v=alpha; echo v=$p10v")[0] == ["v=alpha"])
            sh.run("p10f() { echo in-f $1; }")
            m.chk("function", sh.run("p10f beta")[0] == ["in-f beta"])
            m.chk("if-else", sh.run("if [ $p10v = alpha ]; then echo if-yes; else echo if-no; fi")[0] == ["if-yes"])
            m.chk("for-loop", sh.run("for i in a b c; do echo it-$i; done")[0] == ["it-a", "it-b", "it-c"])
            lines, st, _ = sh.run_st(f"source {MNT}/p10/state.sh")
            outs = [l for l in lines if l.startswith("src-")]
            m.chk("source-disk-script", outs == ["src-start", "src-x", "src-y", "src-status=1"]
                  and sum("p10-src-missing" in l for l in lines) == 1, (lines, st))
            m.note(f"`source` status after the script: {st}")
            m.chk("source-sets-state", sh.run("echo s=$p10s")[0] == ["s=from-script"])
            lines, st, _ = sh.run_st("ls /p10-no-such-dir")
            m.chk("status-of-failure", st == 1, (lines, st))
            m.chk("status-of-success", sh.run_st("echo ok")[1] == 0)
            # Unsupported `&` forms: refused before any effect.
            lines, st, _ = sh.run_st("touch /tmp/p10bgp | cat &")
            m.chk("pipeline-background-refused", st == 2 and any("not started" in l for l in lines), (lines, st))
            lines, st, _ = sh.run_st("touch /tmp/p10amp & echo p10amp-ran")
            m.chk("misplaced-ampersand-refused", st == 2 and "p10amp-ran" not in lines, (lines, st))
            lines, _ = sh.run("ls /tmp")
            m.chk("refused-forms-no-effect", "p10bgp" not in lines and "p10amp" not in lines, lines)
            lines, st, _ = sh.run_st("sleep 1 &")
            m.chk("plain-background-runs", st == 0 and any(re.fullmatch(r"\[\d+\] running", l) for l in lines), (lines, st))
            time.sleep(2)
            # `exit 4` ends this shell with code 4; init restarts it (autoload on-failure).
            mach = self.mach
            pos = len(mach.serial.raw())
            sh.send(b"exit 4\r")
            end = time.time() + 120
            while time.time() < end and b"Cellos shell ready" not in mach.serial.raw()[pos:]:
                time.sleep(0.5)
            seg = mach.serial.raw()[pos:].decode("utf-8", "replace")
            m.chk("exit-code-4", re.search(r"task \d+ exited with code 4", seg) is not None,
                  [l for l in seg.splitlines() if "exited" in l][:3])
            m.chk("shell-restarted", "Init: service restarted." in seg and "Cellos shell ready" in seg)
            sh.wait_prompt(sh.prompts(), 60)
            time.sleep(2)
            m.chk("fresh-shell-has-no-state", sh.run("echo v=$p10v")[0] == ["v="])

    def typed(self):
        with self.m("P10C-typed") as m:
            sh = self.sh
            lines, st, seg1 = sh.run_st("ps --records | where name '==' '\"cellbox\"' | select tid name | sort-by tid | take 1 | to jsonl")
            m.chk("where-select-sort-take", st == 0 and len(lines) == 1 and re.fullmatch(r'\{"tid":\d+,"name":"cellbox"\}', lines[0]) is not None, (lines, st))
            lines, st, seg2 = sh.run_st("ps --records | select tid name | sort-by tid | to jsonl > /tmp/p10.jsonl")
            m.chk("to-jsonl-redirect", st == 0 and lines == [], (lines, st))
            cat, _ = sh.run("cat /tmp/p10.jsonl")
            back, st, seg3 = sh.run_st("cat /tmp/p10.jsonl | from jsonl | to jsonl")
            rows = json_rows(cat)
            m.chk("jsonl-round-trip", st == 0 and cat == back and len(rows) >= 5 and all(set(r) == {"tid", "name"} for r in rows),
                  {"cat": cat, "back": back})
            lines, st, seg4 = sh.run_st("ps --records | where tid '==' 'abc' | to jsonl > /tmp/p10bad.jsonl")
            m.chk("bad-type-refused", st == 2 and any("not a JSON scalar" in l for l in lines), (lines, st))
            lines, st, _ = sh.run_st("ls /tmp/p10bad.jsonl")
            m.chk("bad-type-before-effect", st != 0, (lines, st))
            m.chk("no-ansi-in-jsonl", all(b"\x1b" not in s for s in (seg1, seg2, seg3, seg4)))

    def bytes_(self):
        with self.m("P10C-bytes") as m:
            sh = self.sh
            want = BYTES_TXT.decode().splitlines()
            lines, st, seg = sh.run_st(f"cat {MNT}/p10/bytes.txt")
            m.chk("disk-fixture-cat-exact", lines == want and st == 0, (lines, st))
            exact = b"".join(b"USER: " + l.encode() + b"\n" for l in want)
            m.chk("disk-fixture-uart-bytes", exact in seg, seg[:400])
            # Raw external stages: the stock /bin/echo worker as producer, alone, into a pipe and
            # into a file; the stock /bin/cat worker on its own route (the boot image).
            lines, st, _ = sh.run_st("/bin/echo p10-echo two  words")
            m.chk("external-echo-exact", lines == ["p10-echo two words"] and st == 0, (lines, st))
            lines, st, _ = sh.run_st("/bin/echo p10-ext-a | /bin/echo p10-ext-b")
            m.chk("external-stage-pipeline-exact", lines == ["p10-ext-b"] and st == 0, (lines, st))
            lines, st, _ = sh.run_st("/bin/echo p10-ext-stage | cat")
            m.chk("builtin-after-external-refused-2", st == 2 and lines == [
                "shell: `cat` cannot consume the output of an external command; only external commands may follow one"], (lines, st))
            sh.run("/bin/echo p10-ext-file > /tmp/p10e.txt")
            sh.run(f"cp {MNT}/p10/raw.bin /tmp/p10raw.bin")
            cfg = self.cand["files"]["/etc/cellos/autoload.toml"]
            packaged = Path(cfg["file"]).read_bytes()
            lines, st, seg = sh.run_st("/bin/cat /etc/cellos/autoload.toml")
            m.chk("external-cat-packaged-file-exact", lines == packaged.decode().splitlines() and st == 0, (lines, st))
            sh.run("/bin/cat /etc/cellos/autoload.toml > /tmp/p10auto.toml")
            rows = {r["name"]: r["size"] for r in json_rows(sh.run("ls --records /tmp | to jsonl")[0])}
            m.chk("external-echo-file-length", rows.get("p10e.txt") == len(b"p10-ext-file\n"), rows)
            m.chk("external-cat-file-length", rows.get("p10auto.toml") == len(packaged), rows)
            m.chk("disk-raw-bytes-length", rows.get("p10raw.bin") == len(RAW_BIN), rows)
            lines, st, _ = sh.run_st("cat /tmp/p10e.txt")
            m.chk("external-file-content-exact", lines == ["p10-ext-file"] and st == 0, (lines, st))
            # The stock /bin/cat reads only the boot image (kernel file route) and takes no stdin:
            # a disk path is `file not found`, status 1 (recorded; disk bytes go through the builtin).
            lines, st, _ = sh.run_st(f"/bin/cat {MNT}/p10/bytes.txt")
            m.chk("external-cat-disk-path-status-1", st == 1 and lines == [f"cat: {MNT}/p10/bytes.txt: file not found"], (lines, st))
            m.note("stock /bin/cat reads the boot image only and has no stdin mode; it cannot be a stage over disk bytes")
            # S10-3b: builtin cat of disk files never falls through to a false ENOENT (D3b-2 A).
            lossy = LOSSY_TXT.decode("utf-8", "replace")
            lines, st, seg = sh.run_st(f"cat {MNT}/p10/lossy.txt")
            m.chk("disk-non-utf8-lossy-text", lines == lossy.splitlines() and st == 0, (lines, st))
            m.chk("disk-non-utf8-uart-bytes", b"USER: " + lossy.encode() in seg and b"\x9b" not in seg, seg[:400])
            big = self.rg["big"]
            m.chk("disk-big-fixture-over-1mib", big["size"] > 1024 * 1024, big["size"])
            lines, st, _ = sh.run_st(f"cat {big['path']}")
            m.chk("disk-oversized-file-too-large-1", lines == [f"cat: {big['path']}: File too large"] and st == 1, (lines, st))
            lines, st, _ = sh.run_st(f"cat {MNT}/p10")
            m.chk("disk-directory-is-a-directory-1", lines == [f"cat: {MNT}/p10: Is a directory"] and st == 1, (lines, st))

    def rg_(self):
        with self.m("P10C-rg") as m:
            sh = self.sh
            exp = self.rg
            t = time.time()
            lines, st, _ = sh.run_st(f"rg NEEDLE {exp['big']['path']} > /tmp/big.jsonl", timeout=2400)
            m.note(f"rg over the {exp['big']['size']} B multi-chunk file: {time.time() - t:.0f}s (TCG)")
            m.chk("big-records-written", st == 0 and lines == [], (lines, st))
            nums = json_rows(sh.run("from jsonl --typed < /tmp/big.jsonl | select line_number | to jsonl")[0])
            offs = json_rows(sh.run("from jsonl --typed < /tmp/big.jsonl | select absolute_offset | to jsonl")[0])
            m.chk("big-line-numbers", [r.get("line_number") for r in nums] == exp["big"]["line_numbers"], nums)
            m.chk("big-offsets", [r.get("absolute_offset") for r in offs] == exp["big"]["absolute_offsets"], offs)
            lines, st, _ = sh.run_st(f"rg ENCTOK {RG}/enc")
            m.chk("non-utf8-lines-kept", lines == exp["enc_text"] and st == 0, (lines, st))
            lines, st, _ = sh.run_st(f"rg wide-hit /srv/cellos {RG}/wide", timeout=600)
            rows = [l for l in lines if not l.startswith("rg: ")]
            err = [l for l in lines if l.startswith("rg: ")]
            m.chk("partial-error-status-2", rows == exp["wide"]["rows"] and st == 2 and any("/srv/cellos" in e for e in err),
                  {"rows": len(rows), "err": err, "st": st})
            lines, st, _ = sh.run_st("rg SRVTOK /srv")
            rows = [l for l in lines if not l.startswith("rg: ")]
            m.chk("denied-child-status-2", rows == exp["srv_rows"] and st == 2, (lines, st))
            lines, st, _ = sh.run_st(f"rg wide-hit {RG}/wide | take 1")
            m.chk("take-1-stops-walk", lines == exp["wide"]["rows"][:1] and st == 0, (lines, st))

    def top(self):
        with self.m("P10C-top") as m:
            sh = self.sh
            lines, st, seg = sh.run_st("top -b")
            m.chk("top-batch", st == 0 and any(l.split()[:2] == ["CELL", "NAME"] for l in lines)
                  and any(re.match(r"\s*\d+:\d+\s+cellbox\s", l) for l in lines) and b"\x1b" not in seg, (lines, st))
            # `top --records` alone is one frame; `take --frame 2` bounds a five-frame producer.
            one, st1, _ = sh.run_st("top --records | to jsonl", timeout=180)
            m.chk("records-default-one-frame", st1 == 0 and self.frame_count(json_rows(one)) == 1, st1)
            lines, st, seg = sh.run_st("top --records -d 1 --frames 5 | take --frame 2 | to jsonl", timeout=180)
            rows = json_rows(lines)
            ends = sum(1 for r in rows if r.get("frame") == "end")
            m.note(f"take --frame 2: {len(rows)} JSONL rows, {self.frame_count(rows)} begin / {ends} end envelopes")
            m.chk("records-bounded-two-frames", st == 0 and bool(rows) and b"\x1b" not in seg and self.frame_count(rows) == 2,
                  {"frames": self.frame_count(rows), "ends": ends, "first": rows[:1]})
            lines, st, _ = sh.run_st("top --records | take 2")
            m.chk("plain-take-on-frames-refused", st == 2 and any("take --frame" in l for l in lines), (lines, st))
            # Interactive (S6-4 serial form): the Ratatui view enters the alternate screen and draws
            # a shell's Cell row; `q` leaves it and the prompt returns; status 0.
            before, at = sh.submit("top")
            drawn = self.wait_raw(at, lambda s: self.cell_row_drawn(s), 120)
            m.chk("interactive-draws", drawn, sh.raw()[at:][:300])
            sh.send(b"q")
            ok = sh.wait_prompt(before + 1, 60)
            seg = sh.raw()[at:]
            m.chk("q-restores-then-prompt", ok and b"\x1b[?1049l" in seg and b"Cellos > " in seg[seg.rfind(b"\x1b[?1049l"):])
            m.chk("q-status-0", sh.status() == 0)

    @staticmethod
    def frame_count(rows):
        """Frames of a `top --records` JSONL stream: one `{"frame":"begin",…}` envelope each."""
        return sum(1 for r in rows if r.get("frame") == "begin")

    def wait_raw(self, at, pred, timeout):
        end = time.time() + timeout
        while time.time() < end:
            if pred(self.sh.raw()[at:]):
                return True
            time.sleep(0.3)
        return pred(self.sh.raw()[at:])

    @staticmethod
    def cell_row_drawn(seg):
        seg = seg.replace(b"\x00", b"").decode("utf-8", "replace")
        at = seg.find("\x1b[?1049h")
        if at < 0:
            return False
        body = re.sub(r"\r?\n(?:\[[^\n]*\r?\n)*(?:USER[:+] )?", "", seg[at:])
        toks = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", " ", body).replace("USER: ", " ").split()
        return any(re.fullmatch(r"\d+:\d+", x) is not None and y == "cellbox" for x, y in zip(toks, toks[1:]))

    def tui_text(self, at):
        """The TUI bytes since `at` as text: every control sequence becomes a space (the
        renderer positions words with CUP), runs of whitespace collapse to one space."""
        seg = ANSI.sub(b" ", self.sh.raw()[at:].replace(b"\x00", b"")).decode("utf-8", "replace")
        return re.sub(r"\s+", " ", seg)

    def tui_has(self, at, *needles, timeout=60):
        return self.wait_raw(at, lambda _s: all(n in self.tui_text(at) for n in needles), timeout)

    def edit(self):
        with self.m("P10C-edit") as m:
            sh = self.sh
            sh.run("mkdir /srv/p10ed")
            lines, st, _ = sh.run_st(f"cp {MNT}/p10/crlf.txt /srv/p10ed/m.txt")
            m.chk("fixture-copied", st == 0, (lines, st))
            # Create: a new file, typed text, Ctrl-S, Ctrl-Q.
            before, at = sh.submit("edit /srv/p10ed/new.txt")
            m.chk("create-opens", self.tui_has(at, "new.txt", "new file"), self.tui_text(at)[-300:])
            sh.send(b"p10 created")
            time.sleep(2)
            sh.send(b"\x13")
            m.chk("create-saved", self.tui_has(at, "saved 11 bytes to /srv/p10ed/new.txt"), self.tui_text(at)[-300:])
            sh.send(b"\x11")
            m.chk("create-quit", sh.wait_prompt(before + 1, 60))
            m.chk("create-status-0", sh.status() == 0)
            # Save over CRLF/LF lines: End, "1" on line 1, Ctrl-S, quit; reopen is clean.
            before, at = sh.submit("edit /srv/p10ed/m.txt")
            m.chk("existing-opens", self.tui_has(at, "/srv/p10ed/m.txt", "Ln 1, Col 1"), self.tui_text(at)[-300:])
            sh.send(b"\x1b[F")
            time.sleep(1)
            sh.send(b"1")
            time.sleep(1)
            sh.send(b"\x13")
            m.chk("existing-saved", self.tui_has(at, "saved 15 bytes to /srv/p10ed/m.txt"), self.tui_text(at)[-300:])
            sh.send(b"\x11")
            m.chk("existing-quit", sh.wait_prompt(before + 1, 60))
            before, at = sh.submit("edit /srv/p10ed/m.txt")
            m.chk("reopen", self.tui_has(at, "/srv/p10ed/m.txt", "Ln 1, Col 1"), self.tui_text(at)[-300:])
            time.sleep(2)
            sh.send(b"\x11")
            m.chk("reopen-clean-quit-without-question", sh.wait_prompt(before + 1, 30) and "(d)iscard" not in self.tui_text(at))
            rows = {r["name"]: r["size"] for r in json_rows(sh.run("ls --records /srv/p10ed | to jsonl")[0])}
            m.chk("stored-sizes", rows.get("new.txt") == 11 and rows.get("m.txt") == len(CRLF_TXT) + 1, rows)
            lines, seg = sh.run("rg created /srv/p10ed/new.txt")
            m.chk("created-bytes", lines == ["/srv/p10ed/new.txt:1:p10 created"], lines)
            lines, seg = sh.run("rg e /srv/p10ed/m.txt")
            m.chk("crlf-kept", lines == ["/srv/p10ed/m.txt:1:one1\\r"], lines)
            lines, seg = sh.run("rg t /srv/p10ed/m.txt")
            m.chk("lf-lines-kept", lines == ["/srv/p10ed/m.txt:2:two", "/srv/p10ed/m.txt:3:last"], lines)
            # Dirty exit: Ctrl-Q asks; Enter is no answer; `d` discards and exits 0.
            before, at = sh.submit("edit")
            m.chk("unnamed-opens", self.tui_has(at, "Ln 1, Col 1"), self.tui_text(at)[-300:])
            sh.send(b"q")
            time.sleep(1)
            sh.send(b"\x11")
            m.chk("dirty-exit-asks", self.tui_has(at, "(d)iscard"), self.tui_text(at)[-300:])
            sh.send(b"\r")
            time.sleep(3)
            m.chk("enter-is-no-answer", sh.prompts() == before)
            sh.send(b"d")
            m.chk("discard-exits", sh.wait_prompt(before + 1, 60))
            m.chk("discard-status-0", sh.status() == 0)

    def selective_tui(self):
        with self.m("P10C-selective-tui") as m:
            sh = self.sh
            for cmd in ("ls /", "ps", "cat /mnt/sd/p10/bytes.txt", "cellbox ls /", "top -b -n 1", "ps --records | to jsonl"):
                _, seg = sh.run(cmd)
                m.chk("no-screen-controls:" + cmd, b"\x1b" not in seg, seg[:200])
            lines, st, seg = sh.run_st("x=$(top)")
            m.chk("top-substitution-refused-2", st == 2 and any("top -b" in l for l in lines) and b"\x1b[?1049h" not in seg, (lines, st))

    def task_set(self):
        return [(r["tid"], r["name"]) for r in json_rows(self.sh.run("ps --records | select tid name | sort-by tid | to jsonl")[0])]

    def denials(self):
        with self.m("P10C-denials") as m:
            sh = self.sh
            lines, st, seg = sh.run_st("/bin/shell")
            m.chk("bin-shell-refused", st in (126, 127) and any("Denied" in l or "not found" in l for l in lines), (lines, st))
            m.note("expected DENY line for /bin/shell: " + "; ".join(l for l in seg.decode("utf-8", "replace").splitlines() if "DENY" in l))
            tasks = self.task_set()
            victim = next(t for t, n in tasks if n == "net")
            lines, st, _ = sh.run_st(f"kill {victim}")
            m.chk("kill-refused-1", st == 1 and any("lifecycle owner" in l for l in lines), (lines, st))
            lines, st, _ = sh.run_st("snapshot")
            m.chk("snapshot-refused-1", st == 1 and any("lifecycle owner" in l for l in lines), (lines, st))
            after = self.task_set()
            m.chk("task-set-unchanged", [t for t in after if t[1] != "cellbox" or True] == tasks, (tasks, after))
            lines, st, _ = sh.run_st("export K=V")
            m.chk("export-config-unavailable-1", st == 1 and lines == ["export: K: config unavailable"], (lines, st))
            lines, st, _ = sh.run_st("ssh --user tester --auth publickey --identity k1 -p 22 10.0.2.2 --command true", 120)
            m.chk("ssh-publickey-refused", st != 0 and any("publickey authentication is not available" in l for l in lines), (lines, st))
            lines, st, _ = sh.run_st("cat /srv/cellos/k.txt")
            m.chk("srv-cellos-read-denied-1", st == 1 and any("Permission denied" in l for l in lines) and not any("SRVTOK" in l for l in lines), (lines, st))

    def ctrl_c(self):
        with self.m("P10C-ctrl-c") as m:
            sh = self.sh
            ok, dt, seg = sh.interrupt("sleep 30", 3)
            time.sleep(2)
            sh.wait_prompt(sh.prompts(), 10)  # a ^C that was not consumed is echoed at the next prompt
            m.chk("sleep-ctrl-c-prompt", ok and dt < 10, f"prompt {dt:.1f}s after Ctrl-C; UART tail {strip_ansi(seg)[-80:]!r}")
            st = sh.status()
            m.chk("sleep-ctrl-c-130", st == 130, f"$?={st}")
            ok, dt, seg = sh.interrupt(f"rg -F p10-literal-that-occurs-nowhere-0123456789 {RG}/big.log", 3)
            m.chk("rg-walk-ctrl-c-prompt", ok, f"{dt:.1f}s")
            m.chk("rg-walk-ctrl-c-130", sh.status() == 130)
            flowing = lambda s: strip_ansi(s).count(b"USER: {") >= 3
            ok, dt, seg = sh.interrupt("top --records --frames 100000 | to jsonl", 120, flowing)
            m.chk("endless-sink-flowing", strip_ansi(seg).count(b"USER: {") >= 3)
            m.chk("endless-sink-ctrl-c-prompt", ok, f"{dt:.1f}s")
            size = len(sh.raw())
            time.sleep(4)
            tail = logical_lines(strip_ansi(sh.raw()[size:]))
            m.chk("endless-sink-output-stops", not any(l.startswith("{") for l in tail), tail[:3])
            m.chk("endless-sink-ctrl-c-130", sh.status() == 130)
            before, at = sh.submit("top")
            self.wait_raw(at, self.cell_row_drawn, 120)
            sh.send(b"\x03")
            m.chk("top-tui-ctrl-c-prompt", sh.wait_prompt(before + 1, 60))
            m.chk("top-tui-ctrl-c-130", sh.status() == 130)
            # S10-3b (D3b-1 B): `top -b` waits between frames through the same interruptible wait.
            ok, dt, seg = sh.interrupt("top -b -d 5 -n 10", 3)
            m.chk("top-batch-ctrl-c-prompt", ok and dt < 10, f"{dt:.1f}s")
            m.chk("top-batch-ctrl-c-130", sh.status() == 130)
            # A finite typed stream into builtin `cat` (control), then the same producer made
            # endless (a 7 MiB disk file is not a cat input: builtin cat refuses files over 1 MiB).
            # Last in the boot: an uninterruptible sink would leave the shell busy.
            lines, st, _ = sh.run_st("top --records -d 1 --frames 2 | to jsonl | cat", 120)
            m.chk("cat-sink-finite-control", st == 0 and self.frame_count(json_rows(lines)) == 2, (len(lines), st))
            ok, dt, seg = sh.interrupt("top --records --frames 100000 | to jsonl | cat", 30, flowing)
            m.note(f"endless typed stream into builtin cat: output before Ctrl-C: {strip_ansi(seg).count(b'USER: {')} rows")
            m.chk("cat-sink-ctrl-c-prompt", ok, f"no prompt {dt:.1f}s after Ctrl-C" if not ok else f"{dt:.1f}s")
            if not ok:
                for _ in range(2):
                    sh.send(b"\x03")
                    time.sleep(10)
                m.chk("cat-sink-shell-recovers", sh.idle(), "three Ctrl-C over 80 s: the shell stays in the sink")
                return
            size = len(sh.raw())
            time.sleep(4)
            tail = logical_lines(strip_ansi(sh.raw()[size:]))
            m.chk("cat-sink-output-stops", not any(l.startswith("{") for l in tail), tail[:3])
            m.chk("cat-sink-ctrl-c-130", sh.status() == 130)

    def no_inbound_main(self):
        with self.m("P10C-no-inbound") as m:
            names = [r["name"] for r in json_rows(self.sh.run("ps --records | select name | to jsonl")[0])]
            m.chk("main-no-listener-cell", not any(n in ("sshd", "httpd", "service-httpd") or "listen" in n for n in names), names)
            argv = self.mach.argv
            nets = [argv[i + 1] for i, x in enumerate(argv) if x == "-netdev"]
            m.chk("main-restricted-slirp-no-forward", nets == ["user,id=net0,restrict=on"]
                  and not any("fwd" in x for x in argv), nets)

    # -- ssh boot -----------------------------------------------------------------------
    def ssh_boot(self):
        a = self.a
        mach = Machine(a, "ssh", a.disk_ssh, ssh=True).start()
        sh = Shell(mach)
        try:
            with self.m("P10C-ssh-serial") as m:
                m.chk("prompt", mach.wait_serial("Cellos > ", a.window))
                time.sleep(5)
                end = time.time() + 180
                while time.time() < end and "10.0.2.15" not in mach.text():
                    time.sleep(1)
                m.chk("network-up", "10.0.2.15" in mach.text())
                port = int(Path(a.ssh_dir, "port").read_text())
                cmd = f"ssh --user tester --auth password -p {port} 10.0.2.2 --command 'echo p10-ssh-ran'"
                # Nothing is typed after the command line: a UART that collected a secret would
                # wait at the prompt; fail-closed means the session ends without reading a key.
                t0 = time.time()
                lines, seg = sh.run(cmd, 180)
                waited = time.time() - t0
                st = sh.status()
                m.note(f"UART ssh with password: status {st} after {waited:.1f}s, output {lines}")
                m.chk("fails-closed-status", st != 0 and not any("p10-ssh-ran" in l for l in lines), (lines, st))
                m.chk("ended-without-any-key-typed", waited < 120, f"{waited:.1f}s")
                prompt_line = [l for l in lines if "'s password:" in l]
                m.chk("no-echo-after-password-prompt", len(prompt_line) == 1 and re.fullmatch(
                    r"tester@10\.0\.2\.2's password: (ssh: [^\n]*)?", prompt_line[0]) is not None, prompt_line)
                container = Path(a.ssh_dir, "container").read_text().strip()
                logs = subprocess.run(["docker", "logs", container], capture_output=True, text=True)
                sshd = logs.stdout + logs.stderr
                m.chk("server-saw-no-password", "Connection from " in sshd and "Failed password" not in sshd
                      and "Accepted password" not in sshd, [l for l in sshd.splitlines() if "tester" in l][-3:])
                m.note("sshd: " + "; ".join(l for l in sshd.splitlines() if "tester" in l or "Connection" in l)[-400:])
            with self.m("P10C-no-inbound") as m:
                names = [r["name"] for r in json_rows(sh.run("ps --records | select name | to jsonl")[0])]
                m.chk("ssh-boot-no-listener-cell", not any(n in ("sshd", "httpd") or "listen" in n for n in names), names)
                nets = [mach.argv[i + 1] for i, x in enumerate(mach.argv) if x == "-netdev"]
                m.chk("ssh-boot-no-forward", nets == ["user,id=net0"] and not any("fwd" in x for x in mach.argv), nets)
        finally:
            self.ssh_raw = mach.stop()


# ---- desktop: GUI only ----------------------------------------------------------------------
QCODE = dict(gui.QCODE)
QCODE.update({c: (c.lower(), True) for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ"})
QCODE.update({"'": "apostrophe", '"': ("apostrophe", True), ";": "semicolon", ":": ("semicolon", True),
              "@": ("2", True), "&": ("7", True), "*": ("8", True), "(": ("9", True), ")": ("0", True),
              ",": "comma", "<": ("comma", True), "+": ("equal", True), "!": ("1", True), "#": ("3", True),
              "%": ("5", True), "^": ("6", True), "`": "grave_accent", "~": ("grave_accent", True),
              "[": "bracket_left", "]": "bracket_right", "{": ("bracket_left", True), "}": ("bracket_right", True),
              "\\": "backslash"})
TASKBAR_Y = 800 - 36 + 4 + 14
TASKBAR = {"terminal": 140 + 74, "ocel": 294 + 74, "files": 448 + 74, "chrome": 602 + 74}
DATE_ROW = re.compile(r"[A-Z][a-z]{2} +\d{1,2} \d\d:\d\d:\d\d UTC \d{4}")


class Desk(Proof):
    """The imported Phase04 GUI oracle, pointed at the candidate's desktop. `fixed` pins the
    judged window to a known origin when two Terminal windows are on screen."""

    def __init__(self, a, mach, verdict):
        ns = argparse.Namespace(work=a.work, evidence=a.evidence, glyph_bin=a.glyph_bin, accel=a.accel, window=a.window)
        super().__init__(ns)
        self.tmp = Path(a.work) / "sock"
        (Path(a.work) / "scratch").mkdir(exist_ok=True)
        self.mach, self.proc, self.serial, self.qmp = mach, mach.proc, mach.serial, mach.qmp
        self.sw, self.sh = 1280, 800
        self.fixed = None
        self.v = verdict
        self.model, self.state = [], {}
        self.cur = None

    # -- the oracle's window, optionally pinned ------------------------------------------
    def window(self, img):
        if self.fixed is None:
            return super().window(img)
        x, y = self.fixed
        win = img.crop((x, y, x + WIN_W, y + WIN_H))
        px = win.load()
        if px[0, 0] != BG or px[WIN_W - 1, WIN_H - 1] != BG:
            return None, None
        return self.fixed, win

    def chk(self, name, ok, why="", info=False):
        """The oracle's checks land in the current marker (one record, one printed line)."""
        self.checks[name] = {"pass": bool(ok), "detail": why, "informational": info}
        if self.cur is not None:
            self.cur.chk(name, ok, why)
        else:
            print(("PASS " if ok else "FAIL ") + name + ("" if ok else "  <- " + str(why)), flush=True)

    def type_text(self, text, gap=0.12):
        for ch in text:
            q = QCODE[ch]
            shift = isinstance(q, tuple)
            if shift:
                q = q[0]
            self.qmp.keys(([("shift", True)] if shift else []) + [(q, True), (q, False)] + ([("shift", False)] if shift else []))
            time.sleep(gap)

    def m(self, name):
        scope = MarkerScope(self.v, name)
        self.cur = scope.m
        return scope

    # -- glyph reader: text of a window, accepted only by an exact re-render ---------------
    def atlas(self):
        if not hasattr(self, "_atlas"):
            chars = [chr(c) for c in range(33, 127)]
            lines = [" ".join(chars[i:i + 39]) for i in range(0, len(chars), 39)]
            img = self.render(lines)
            self._atlas, self._glyphs = {}, []
            for r, line in enumerate(lines):
                for c, ch in enumerate(line):
                    if ch != " ":
                        cell = img.crop(self.cell_box(c, r))
                        self._atlas[cell.tobytes()] = ch
                        self._glyphs.append((ch, cell))
            self._blank = Image.new("RGB", (CELL_W, CELL_H), BG).tobytes()
        return self._atlas

    def read_row(self, win, row, cursor):
        atlas = self.atlas()
        out = []
        for col in range(COLS):
            if cursor == (col, row):
                out.append(" ")
                continue
            cell = win.crop(self.cell_box(col, row))
            key = cell.tobytes()
            if key == self._blank:
                out.append(" ")
            elif key in atlas:
                out.append(atlas[key])
            else:
                best = min(self._glyphs, key=lambda g: sum(ImageChops.difference(cell, g[1]).convert("L").histogram()[1:]))
                out.append(best[0])
        return "".join(out).rstrip()

    def read_screen(self, img):
        """(rows, cursor) if the window's text re-renders to exactly the same pixels, else None.
        A row the ASCII glyph reader cannot spell (the banner's em dash) is accepted only as a
        known line (the banner or a model row) that renders it exactly."""
        _, win = self.window(img)
        if win is None:
            return None
        cursor = self.cursor_of(win)
        rows = [self.read_row(win, r, cursor) for r in range(ROWS)]
        known = {BANNER, *self.model}
        for r, text in enumerate(rows):
            if text and self.diff(win, [text], r, r, cursor)[0] != 0:
                rows[r] = next((k for k in known if self.diff(win, [k], r, r, cursor)[0] == 0), text)
        if self.diff(win, rows, 0, ROWS - 1, cursor)[0] != 0:
            return None
        while rows and not rows[-1]:
            rows.pop()
        return rows, cursor

    # -- one typed command, judged exactly (the lane's `command()` contract) --------------
    def command(self, name, text, outputs, timeout=180):
        model, state = self.model, self.state
        before = state["img"]
        typed_rows = model[:-1] + [model[-1] + text]
        self.type_text(text)
        typed = self.expect_screen(typed_rows, f"{name}: typed line", 120)
        self.chk(f"{name}-typed-line-exact", True, "diff=0")
        self.tap("ret")
        cands = [typed_rows + lines + [PROMPT] for lines in outputs]
        seen = {}

        def pred(i):
            for n, cand in enumerate(cands):
                mm = self.match_full(i, self.pad(cand), self.mcursor(cand))
                if mm and mm.get("diff") == 0:
                    return n + 1
                seen[n] = mm
            return 0
        try:
            after, which = self.wait_for(pred, f"{name}: output", timeout)
        except StepFailed:
            self.notes.append(f"{name}: last observation {seen}")
            self.chk(f"{name}-output-exact", False, f"no acceptable exact screen; last {seen}")
            raise
        which -= 1
        self.shot(f"{name}-after", after)
        final = cands[which]
        self.chk(f"{name}-output-exact", True, f"alternative {which}")
        self.controls(name, before, after, final, len(typed_rows))
        model[:] = final
        state["img"] = after
        return which

    def controls(self, name, before, after, final, first_out):
        dprev = self.match_full(before, self.pad(final), self.mcursor(final))
        self.chk(f"{name}-control-before-frame-differs",
                 dprev is not None and (dprev["diff"] is None or dprev["diff"] >= MIN_NEGATIVE_DIFF), dprev)
        altered = list(final)
        idx = next((i for i in range(first_out, len(final) - 1) if altered[i] and i >= len(final) - ROWS), first_out - 1)
        altered[idx] = altered[idx][:-1] + ("x" if altered[idx][-1] != "x" else "y")
        _, win = self.window(after)
        dalt, _ = self.diff(win, self.pad(altered), 0, ROWS - 1, self.mcursor(final))
        self.chk(f"{name}-control-one-char-change-differs", dalt >= MIN_NEGATIVE_DIFF, f"{dalt} differing pixels")

    def command_read(self, name, text, timeout=180, done=None):
        """Type `text`, Enter, wait until the window reads back exactly with a fresh prompt on
        its last row (or `done(rows)`); the read rows become the model. Returns the rows."""
        model, state = self.model, self.state
        before = state["img"]
        typed_rows = model[:-1] + [model[-1] + text]
        self.type_text(text)
        self.expect_screen(typed_rows, f"{name}: typed line", 120)
        self.chk(f"{name}-typed-line-exact", True, "diff=0")
        self.tap("ret")
        return self.read_until(name, before, done or self.at_fresh_prompt(len(self.pad(typed_rows)), typed_rows[-1]), timeout)

    @staticmethod
    def at_fresh_prompt(min_rows, typed_last):
        def done(rows, cursor):
            return (rows and rows[-1] == PROMPT.rstrip() and cursor == (len(PROMPT), len(rows) - 1)
                    and any(typed_last in r for r in rows[:-1]))
        return done

    def read_until(self, name, before, done, timeout):
        last = {}

        def pred(i):
            got = self.read_screen(i)
            last["read"] = got
            return got if got and done(*got) else None
        try:
            img, (rows, cursor) = self.wait_for(pred, f"{name}: read screen", timeout, interval=1.5)
        except StepFailed:
            self.chk(f"{name}-screen-read-exact", False, f"last read {last.get('read')}")
            raise
        self.shot(f"{name}-read", img)
        self.chk(f"{name}-screen-read-exact", True, "re-render diff=0")
        final = rows if rows[-1] != PROMPT.rstrip() else rows[:-1] + [PROMPT]
        if before is not None:
            self.controls(name, before, img, final, max(1, len(final) - 2))
        self.model[:] = final
        self.state["img"] = img
        return final

    # -- scenarios ------------------------------------------------------------------------
    def launch(self):
        mach = self.mach
        if not mach.wait_serial("[desktop] Ready. Entering event loop.", self.a.window):
            raise StepFailed("desktop never ready")
        time.sleep(3)
        pre = self.shot("desktop-before-terminal")
        self.need("screen-1280x800", pre.size == (1280, 800), pre.size)
        self.need("no-window-before-launch", self.locate(pre) is None, "a bg-coloured 720x480 rectangle exists before launch")
        self.click_px(TASKBAR["terminal"], TASKBAR_Y)
        self.park()
        if not mach.wait_serial("[terminal] Terminal surface ready. Entering event loop.", 300):
            raise StepFailed("terminal never ready")
        rows = ["", BANNER, PROMPT]
        img = self.expect_screen(rows, "initial prompt in the window", 240)
        self.shot("prompt", img)
        org = self.org
        region = pre.crop((org[0], org[1], org[0] + WIN_W, org[1] + WIN_H))
        d, _ = self.diff(region, self.pad(rows), 0, ROWS - 1, self.mcursor(rows))
        self.chk("prompt-matches-exactly", True, f"diff=0 at origin {org}")
        self.chk("control-pre-launch-region-differs", d > 1000, f"{d} differing pixels")
        _, win = self.window(img)
        d2, _ = self.diff(win, self.pad(rows[:-1] + ["Cellos # "]), 0, ROWS - 1, (len(PROMPT), 2))
        self.chk("control-wrong-prompt-differs", d2 >= MIN_NEGATIVE_DIFF, f"{d2} differing pixels")
        self.chk("cursor-cell", self.cursor_of(win) == (len(PROMPT), 2), self.cursor_of(win))
        self.click_px(org[0] + 360, org[1] + 300)
        self.park()
        time.sleep(2)
        self.model[:] = rows
        self.state["img"] = img

    def real_output(self):
        self.command("ls-disk-dir", f"ls {MNT}/gui-list", [["alpha-entry", "beta-entry"]])
        # The stock /bin/ls lists the boot image only (kernel file route): disk listings are builtin.
        self.command("cat-disk-file", f"cat {MNT}/gui-text/note.txt", [["gui-cat-line-one", "gui-cat-line-two"]])
        r1 = self.command_read("date-1", "date")
        d1 = r1[-2]
        time.sleep(2.5)
        r2 = self.command_read("date-2", "date")
        d2 = r2[-2]
        self.chk("date-format", DATE_ROW.fullmatch(d1) is not None and DATE_ROW.fullmatch(d2) is not None, (d1, d2))
        self.chk("date-changes", d1 != d2, (d1, d2))
        self.command("ps-own-terminal-cell", "ps --records | where name '==' '\"terminal\"' | select name | to jsonl",
                     [['{"name":"terminal"}']])
        rows = self.command_read("ps-text", "ps")
        out = rows[rows.index(PROMPT + "ps") + 1:-1]
        self.chk("ps-lists-terminal-and-shell", any(re.search(r"\sterminal\s", " " + l + " ") for l in out)
                 and any(re.search(r"\scellbox\s", " " + l + " ") for l in out), out)
        self.command("mkdir", "mkdir /tmp/p10gdir", [[]])
        self.command("create-file", "echo p10g-made > /tmp/p10gdir/made.txt", [[]])
        self.command("ls-shows-created", "ls /tmp/p10gdir", [["made.txt"]])
        self.command("cat-created", "cat /tmp/p10gdir/made.txt", [["p10g-made"]])

    def tui(self):
        # TOP-2's control scans two stacked glyph rows of the prompt screen: have output first.
        self.command("tui-start", "echo p10g-tui-start", [["p10g-tui-start"]])
        self.top(self.command, self.model, self.state)       # imported S6-4 TOP-0..5
        # edit on a CellosFS file: type, Ctrl-S, Ctrl-Q exits at once (clean = saved).
        typed_rows = self.model[:-1] + [self.model[-1] + "edit /srv/p10g.txt"]
        self.type_text("edit /srv/p10g.txt")
        self.expect_screen(typed_rows, "edit typed", 120)
        self.tap("ret")
        status_row = ROWS - 2

        def drawn(i):
            w = self.window(i)[1]
            return w is not None and all(self.cell_share(w, c, status_row, gui.TOP_SURFACE) > 0.5 for c in range(COLS))
        self.wait_for(drawn, "editor drawn", 180)
        self.chk("edit-opens", True)
        self.type_text("p10g-saved")
        self.tap("ret")
        time.sleep(1)
        self.combo("ctrl", "s")
        time.sleep(3)
        self.combo("ctrl", "q")
        restored = typed_rows + [PROMPT]
        img = self.expect_screen(restored, "edit: saved buffer quits without a question", 120)
        self.chk("edit-save-then-clean-quit", True, "exact restored prompt screen")
        self.model[:] = restored
        self.state["img"] = img
        self.command("edit-status", "echo st=$?", [["st=0"]])
        self.command("edit-stored", "cat /srv/p10g.txt", [["p10g-saved"]])
        typed_rows = self.model[:-1] + [self.model[-1] + "edit /srv/p10g.txt"]
        self.type_text("edit /srv/p10g.txt")
        self.expect_screen(typed_rows, "edit reopen typed", 120)
        self.tap("ret")
        img = self.wait_for(drawn, "editor reopened", 180)[0]
        w = self.window(img)[1]

        def glyph(col, row):
            return any(c not in (BG, gui.EDIT_BG) for _, c in w.crop(self.cell_box(col, row)).getcolors(CELL_W * CELL_H))
        self.chk("edit-reopen-shows-text", all(glyph(c, 0) for c in range(1, 10)), "row 0 cells 1..9 inked")
        self.combo("ctrl", "q")
        restored = typed_rows + [PROMPT]
        img = self.expect_screen(restored, "edit: reopened buffer is clean", 120)
        self.chk("edit-reopen-clean-quit", True, "exact restored prompt screen")
        self.model[:] = restored
        self.state["img"] = img

    def vt_all(self):
        # The imported S5-2 and S5-4 scenarios, with the fixture paths on the P1 data volume.
        gui.VT_ALT_ENTER, gui.VT_ALT_EXIT = MNT + "/p04-output/vt-alt-enter.bin", MNT + "/p04-output/vt-alt-exit.bin"
        gui.VT_SAMPLE, gui.VT_HOSTILE = MNT + "/p04-output/vt-sample.bin", MNT + phase05_vt_hostile.PATH
        self.vt(self.model, self.state)
        # The imported hostile scenario starts with its own `clear`: give it a different screen.
        self.command("vt-done", "echo p10g-vt-done", [["p10g-vt-done"]])
        try:
            self.hostile(self.model, self.state)
        except StepFailed:
            # Record what the window shows instead (read exactly), then fail the marker.
            got = self.read_screen(self.grab())
            self.chk("hostile-observed-screen", False, got[0][-4:] if got else "window unreadable")
            raise

    def recover(self):
        """After a failed marker: Ctrl-C, then `clear`, then the exact empty prompt screen."""
        self.ctrl_c()
        time.sleep(2)
        self.type_text("clear")
        self.tap("ret")
        img = self.expect_screen([PROMPT], "recovered prompt after a failed marker", 120)
        self.model[:], self.state["img"] = [PROMPT], img

    def delete_key(self):
        res = {}
        for key in ("delete", "kp_decimal"):
            text = f"echo p10gdel-{key[:2]}abc"
            typed_rows = self.model[:-1] + [self.model[-1] + text]
            self.type_text(text)
            self.expect_screen(typed_rows, f"delete-key {key}: typed", 120)
            self.tap("left")
            self.tap(key)
            time.sleep(1)
            self.tap("ret")
            rows = self.read_until(f"delete-key-{key}", None, lambda r, c: r and r[-1] == PROMPT.rstrip()
                                   and c == (len(PROMPT), len(r) - 1) and len(r) >= 3 and r[-3].startswith(PROMPT + "echo p10gdel"), 120)
            out = rows[-2]
            res[key] = {"line": rows[-3], "output": out, "deleted": out == f"p10gdel-{key[:2]}ab"}
        self.chk("delete-key-recorded", True, json.dumps(res))
        return res

    def two_sessions(self):
        org_a = self.org
        model_a = list(self.model)
        self.click_px(TASKBAR["terminal"], TASKBAR_Y)
        self.park()
        end = time.time() + 300
        while time.time() < end and self.mach.text().count("[terminal] Terminal surface ready. Entering event loop.") < 2:
            time.sleep(0.5)
        self.chk("second-terminal-started", self.mach.text().count("[terminal] Terminal surface ready. Entering event loop.") == 2)
        # The second window opens at the same origin, on top; its own exact prompt screen.
        self.fixed = org_a
        rows_b = ["", BANNER, PROMPT]
        self.expect_screen(rows_b, "second terminal prompt", 240)
        # Drag it by its title tab so both are on screen.
        org_b = (org_a[0] + 400, org_a[1] + 100)
        tab = (org_a[0] + 110, org_a[1] - 14)
        self.qmp.abs_move(int(tab[0] * 32768 / self.sw), int(tab[1] * 32768 / self.sh))
        time.sleep(0.6)
        self.qmp.button(True)
        time.sleep(0.3)
        for k in range(1, 11):
            x, y = tab[0] + 40 * k, tab[1] + 10 * k
            self.qmp.abs_move(int(x * 32768 / self.sw), int(y * 32768 / self.sh))
            time.sleep(0.2)
        self.qmp.button(False)
        self.park()
        self.fixed = org_b
        img = self.expect_screen(rows_b, "second terminal moved", 120)
        self.chk("two-windows-on-screen", True, f"A at {org_a}, B at {org_b}")
        self.click_px(org_b[0] + 500, org_b[1] + 300)
        self.park()
        time.sleep(2)
        self.model[:], self.state["img"] = rows_b, img
        self.command("B-set-state", "p10gst=bee; echo b=$p10gst", [["b=bee"]])
        model_b, img_b = list(self.model), self.state["img"]
        # Raise by a click in each window's content where the other does not cover it (the
        # Terminal raises on a left click and takes the keyboard); judge the raised window.
        click_a, click_b = (org_a[0] + 150, org_a[1] + 200), (org_b[0] + 500, org_b[1] + 400)
        self.click_px(*click_a)
        self.park()
        self.fixed = org_a
        img = self.expect_screen(model_a, "A raised, its transcript untouched by B's input", 120)
        self.chk("A-unchanged-by-B-input", True, "exact screen")
        time.sleep(1)
        self.model[:], self.state["img"] = model_a, img
        self.command("A-state-absent", "echo a=$p10gst", [["a="]])
        self.click_px(*click_b)
        self.park()
        self.fixed = org_b
        img = self.expect_screen(model_b, "B raised, its transcript untouched by A's input", 120)
        self.chk("B-unchanged-by-A-input", True, "exact screen")
        time.sleep(1)
        self.model[:], self.state["img"] = model_b, img
        self.command("B-state-kept", "echo b=$p10gst", [["b=bee"]])
        self.fixed = None

    def apps(self):
        res = {}
        for app, ready in (("files", "[files] Files surface ready. Entering event loop."),
                           ("ocel", "[ocel] Window initialized and painted."),
                           ("chrome", "[chrome] CellOS Chrome Single-App Guest Launcher")):
            before = self.grab()
            pos = len(self.mach.serial.raw())
            self.click_px(TASKBAR[app], TASKBAR_Y)
            self.park()
            ok = self.mach.wait_serial(ready, 300)
            seg = self.mach.serial.raw()[pos:].decode("utf-8", "replace")
            spawned = re.search(r"\[desktop\] Spawned PID \d+", seg) is not None

            def drew(i):
                d = ImageChops.difference(before.crop((0, 0, 1280, 760)), i.crop((0, 0, 1280, 760))).convert("L")
                return sum(d.histogram()[1:]) >= 20000
            try:
                after, _ = self.wait_for(drew, f"{app} first window", 240, interval=2)
                time.sleep(5)
                after = self.shot(f"app-{app}")
                changed = sum(ImageChops.difference(before.crop((0, 0, 1280, 760)), after.crop((0, 0, 1280, 760))).convert("L").histogram()[1:])
                drawn = True
            except StepFailed:
                changed, drawn = 0, False
            denies = [l for l in seg.splitlines() if "DENY" in l]
            res[app] = {"ready_line": ok, "spawned": spawned, "drawn": drawn, "pixels_changed": changed, "deny_lines": denies}
            self.chk(f"{app}-launched", ok and spawned, res[app])
            self.chk(f"{app}-first-window-drawn", drawn, res[app])
        return res

    # -- ssh (fixture boot) -------------------------------------------------------------
    def ssh(self, port, fp, password):
        """P10G-ssh on the D79-1 fixture. Every screen is read back exactly (glyph reader +
        zero-pixel re-render); command lines stay below the 79-column grid via an alias."""
        def out_after(rows, marker):
            """Rows after the last row containing `marker`, up to the fresh prompt."""
            idx = max(i for i, r in enumerate(rows) if marker in r)
            return rows[idx + 1:-1]

        def run_with_password(name, line, pw):
            self.type_text(line)
            self.tap("ret")
            prompt = "tester@10.0.2.2's password:"

            def asked(rows, cursor):
                return bool(rows) and rows[-1] == prompt and cursor == (len(prompt) + 1, len(rows) - 1)
            self.read_until(f"{name}-password-prompt", None, asked, 240)
            self.type_text(pw, gap=0.1)
            self.tap("ret")
            rows = self.read_until(name, None, self.at_fresh_prompt(0, prompt), 300)
            self.chk(f"{name}-password-never-drawn", not any(pw in r for r in rows), "")
            return rows, out_after(rows, prompt)

        self.type_text("clear")
        self.tap("ret")
        self.state["img"] = self.expect_screen([PROMPT], "ssh: cleared", 60)
        self.model[:] = [PROMPT]
        # Shell variables keep each typed line inside the 79-column grid (an alias keeps only
        # its first word on this image).
        ssh = "ssh --user tester --auth password -p $P"
        self.command("ssh-port-var", f"P={port}", [[]])
        # A changed host key (trust disk: `host` PORT holds another key): refused before any
        # credential, no password prompt, the offered fingerprint named.
        rows = self.command_read("ssh-changed-key", f"{ssh} host --command 'echo nv'", 300)
        out = "".join(out_after(rows, "host --command 'echo nv'"))
        want = f"ssh: HOST KEY CHANGED for host:{port}: server offered ssh-ed25519 {fp}; refused"
        self.chk("changed-key-refused-exact", out == want, (out, want))
        self.command("ssh-changed-key-status", "echo st=$?", [["st=255"]])
        # A wrong password is refused by the server.
        rows, out = run_with_password("ssh-wrong-password", f"{ssh} 10.0.2.2 --command 'echo nv'", "p10g-wrong-password")
        self.chk("wrong-password-refused", out == ["ssh: authentication failed"], out)
        self.command("ssh-wrong-status", "echo st=$?", [["st=255"]])
        # Exec with the known host (trust disk 10.0.2.2:PORT exact): remote stdout, stderr, status.
        self.command("ssh-exec-var", "C='echo p10g-out; echo E >&2; exit 7'", [[]])
        rows, out = run_with_password("ssh-exec", f'{ssh} 10.0.2.2 --command "$C"', password)
        self.chk("exec-remote-bytes", sorted(out) == ["E", "p10g-out"], out)
        self.command("ssh-exec-status", "echo st=$?", [["st=7"]])
        # PTY: TERM and the window grid reach the server; `exit` returns to the local shell.
        self.type_text(f"{ssh} 10.0.2.2")
        self.tap("ret")
        prompt = "tester@10.0.2.2's password:"
        self.read_until("ssh-pty-password-prompt", None,
                        lambda r, c: bool(r) and r[-1] == prompt and c == (len(prompt) + 1, len(r) - 1), 240)
        self.type_text(password, gap=0.1)
        self.tap("ret")
        time.sleep(5)
        self.type_text("echo T=$TERM; stty size")
        self.tap("ret")

        def pty_done(rows, cursor):
            return "T=cellos-vt1" in rows and f"{ROWS} {COLS}" in rows
        try:
            rows = self.read_until("ssh-pty-echo", None, pty_done, 240)
            self.chk("pty-term-and-size", True, rows[-4:])
        except StepFailed:
            self.shot("ssh-pty-unread")
            raise
        self.chk("pty-password-never-drawn", not any(password in r for r in rows), "")
        self.type_text("exit")
        self.tap("ret")
        self.read_until("ssh-pty-exit", None, lambda r, c: bool(r) and r[-1] == PROMPT.rstrip()
                        and c == (len(PROMPT), len(r) - 1) and "T=cellos-vt1" in r, 240)
        self.command("ssh-pty-status", "echo st=$?", [["st=0"]])
        return f"{ROWS} {COLS}"


class Desktop:
    MARKERS = ["P10G-launch", "P10G-real-output", "P10G-two-sessions", "P10G-vt", "P10G-tui", "P10G-ssh",
               "P10G-apps", "P10G-delete-key", "P10C-not-on-uart"]
    # Text that only the GUI Terminal sessions print or receive (the desktop's own serial
    # `cellbox sh` banner and prompt are legitimately on the UART and are not listed).
    PAYLOADS = ["alpha-entry", "gui-cat-line-one", "p10g-made", "p10gst=bee", "b=bee", "end-of-sample", "hostile-begin",
                "hostile-ok", "p10gdel", "p10g-saved", "p10g-tui-start", "p10g-vt-done", "p10g-out", "T=cellos-vt1",
                "HOST KEY CHANGED", "authentication failed"]

    def __init__(self, a, verdict, cand):
        self.a, self.v, self.cand = a, verdict, cand

    def main_boot(self):
        mach = Machine(self.a, "main", self.a.disk_main, ssh=False).start()
        d = Desk(self.a, mach, self.v)
        try:
            steps = [("P10G-launch", d.launch), ("P10G-real-output", d.real_output), ("P10G-vt", d.vt_all),
                     ("P10G-tui", d.tui), ("P10G-delete-key", d.delete_key), ("P10G-two-sessions", d.two_sessions),
                     ("P10G-apps", d.apps)]
            for name, fn in steps:
                with d.m(name) as m:
                    m.note_result = fn()
                    if name == "P10G-delete-key":
                        m.note("Delete key (recorded either way, known defect): " + json.dumps(m.note_result))
                    if name == "P10G-apps":
                        m.note(json.dumps(m.note_result))
                d.cur = None
                if not mach.alive() or (name == "P10G-launch" and not self.v.markers[name].passed):
                    break
                if not self.v.markers[name].passed and name not in ("P10G-two-sessions", "P10G-apps"):
                    try:
                        d.recover()
                    except StepFailed as e:
                        print(f"  recovery after {name} failed: {e}", flush=True)
                        break
        finally:
            self.main_raw = mach.stop()

    def ssh_boot(self):
        a = self.a
        mach = Machine(a, "ssh", a.disk_ssh, ssh=True).start()
        d = Desk(a, mach, self.v)
        try:
            with d.m("P10G-ssh") as m:
                d.launch()
                end = time.time() + 180
                while time.time() < end and "10.0.2.15" not in mach.text():
                    time.sleep(1)
                m.chk("network-up", "10.0.2.15" in mach.text())
                port = int(Path(a.ssh_dir, "port").read_text())
                fp = Path(a.ssh_dir, "fingerprint").read_text().strip()
                password = Path(a.ssh_dir, "password").read_text().strip()
                size = d.ssh(port, fp, password)
                m.note(f"PTY size {size} is the fixed window grid; the GUI window has no user resize control here, "
                       "so a PTY resize is not exercised on this image (L5 ssh-pty-resize carries it)")
        finally:
            self.ssh_raw = mach.stop()


def no_secret(evidence, secrets):
    """Every log, frame list and JSON in the evidence: no secret bytes (raw or hex)."""
    hits = []
    for p in sorted(Path(evidence).rglob("*")):
        if not p.is_file() or p.suffix in (".png",):
            continue
        data = p.read_bytes()
        for name, s in secrets.items():
            if s and (s in data or s.hex().encode() in data):
                hits.append(f"{p.name}: {name}")
    return hits


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fixtures")
    f.add_argument("out")
    f.add_argument("--manifest", required=True)
    r = sub.add_parser("run")
    for name in ("profile", "work", "iso", "evidence", "candidate", "disk-main", "disk-ssh", "ssh-dir", "rg-expected"):
        r.add_argument("--" + name, required=True)
    r.add_argument("--glyph-bin")
    r.add_argument("--accel", default="tcg", choices=("tcg", "kvm"))
    r.add_argument("--window", type=int, default=900)
    r.add_argument("--secret-file", action="append", default=[])
    a = p.parse_args()
    if a.cmd == "fixtures":
        Path(a.manifest).write_text(json.dumps(write_fixtures(a.out), indent=1) + "\n")
        return 0
    cand_all = json.loads(Path(a.candidate).read_text())
    cand = cand_all["profiles"][a.profile]
    t0 = time.time()
    if a.profile == "headless":
        v = Verdict(Headless.MARKERS)
        h = Headless(a, v, cand)
        h.main_boot()
        h.ssh_boot()
        raws = [h.main_raw, h.ssh_raw]
        uart_payloads = []
    else:
        v = Verdict(Desktop.MARKERS)
        dsk = Desktop(a, v, cand)
        dsk.main_boot()
        dsk.ssh_boot()
        raws = [dsk.main_raw, dsk.ssh_raw]
        uart_payloads = Desktop.PAYLOADS
    with MarkerScope(v, "P10C-not-on-uart") as m:
        secrets = {}
        for s in a.secret_file:
            data = Path(s).read_bytes()
            if b"PRIVATE KEY" in data:
                secrets.update({f"{Path(s).name}-line-{i}": l for i, l in enumerate(data.splitlines())
                                if len(l) > 20 and b"PRIVATE KEY" not in l})
            else:
                secrets[Path(s).name] = data.strip()
        import importlib.util
        spec = importlib.util.spec_from_file_location("sign_cell", Path(__file__).resolve().parent / "sign-cell.py")
        mod = importlib.util.module_from_spec(spec)
        sys.modules["sign_cell"] = mod
        spec.loader.exec_module(mod)
        secrets["dev-signing-seed"] = mod.DEV_SEED
        for raw in raws:
            for name, s in secrets.items():
                m.chk(f"no-{name}-on-uart", s not in raw and s.hex().encode() not in raw)
        for tok in uart_payloads:
            m.chk("gui-payload-not-on-uart:" + tok, not any(tok.encode() in raw for raw in raws))
        m.chk("no-private-key-on-uart", not any(b"PRIVATE KEY" in raw for raw in raws))
        hits = no_secret(a.evidence, secrets)
        m.chk("no-secret-in-any-evidence-file", not hits and not any(b"PRIVATE KEY" in pp.read_bytes()
              for pp in Path(a.evidence).rglob("*") if pp.is_file() and pp.suffix != ".png"), hits)
    missing = [n for n in v.expected if n not in v.markers]
    for n in missing:
        mk = v.marker(n)
        mk.error = "not reached"
    out = {
        "schema": "cellos-phase10-l0-verdict/1",
        "profile": a.profile,
        "accel": a.accel,
        "qualifying": a.accel == "tcg",
        "candidate_sha256": hashlib.sha256(Path(a.candidate).read_bytes()).hexdigest(),
        "duration_s": round(time.time() - t0, 1),
        "markers": {n: v.markers[n].record() for n in v.expected},
    }
    out["pass"] = all(rec["pass"] for rec in out["markers"].values())
    Path(a.evidence, "verdict.json").write_text(json.dumps(out, indent=2) + "\n")
    for n in v.expected:
        print(("PASS " if out["markers"][n]["pass"] else "FAIL ") + n)
    print(("PASS" if out["pass"] else "FAIL") + f": {a.profile} L0 ({len(v.expected)} markers) in {out['duration_s']}s")
    return 0 if out["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
