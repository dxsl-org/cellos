#!/usr/bin/env bash
# Private, fail-closed Phase04 slice S3 build/sign/assemble/boot runner.
#
# README (what this proves, and what it does not)
# -----------------------------------------------
# Scope (phase-04-s3-gui-terminal-proposal.md §4, gates G3a-G3d): builds a *private*
# x86_64 image whose kernel trusts a freshly generated throw-away cell signing key, boots
# it in an isolated QEMU (restricted SLIRP only, see S2-N below) and records provenance + serial
# evidence. It never writes the canonical embedded artifacts, the quarantined
# cellbox/terminal/kernel_fs.img, a user disk, or any other VM. Every failure aborts;
# stale outputs are removed before building so a failed rebuild can never be booted.
#
# What runs: the serial shell is NOT started. The generated boot config is rewritten
# (private copy; the repository templates are untouched) so init autoloads
# /bin/phase04-terminal-driver in place of `cellbox sh`. The driver
# (cells/tests/phase04-terminal-driver) links terminal_sdk::FrontendHost, the same host the
# GUI Terminal uses, and drives real `cellbox sh` instances through it. Each shell controls
# its own GUI-kind session (Phase 05 S5-3 annex option C, `CreateForFrontend`); the driver
# is only its frontend: it types commands with submit_text/submit_key on the frontend
# attachment, reads the output rings the shell relays to with drain_stdout/drain_stderr and
# prints `P04 PASS|FAIL <name>` markers. Init starts the driver under its reviewed init edge and the
# driver launches /bin/cellbox on its reviewed `phase04-terminal-driver -> /bin/cellbox`
# edge (path route, empty child ceiling); both rows exist only under the kernel feature
# `phase04-driver-profile` (never default). `phase02-terminal-profile` is enabled only for
# the private signing-key path it provides.
#
# The independent verdict is this script's evaluation step: every expected marker must be
# present exactly once as PASS, no FAIL, no kernel launch DENY line, no shell prompt on the
# physical UART (the shells talk to the driver through session endpoints only) and no key
# material in the serial log. A missing marker fails the run.
#
# NOT proven here: GUI pixels/compositor routing. The approved negative-grant
# amendment adds only private Path driver -> /bin/echo, empty child ceiling.
# Missing output slots must reach actual stock echo and exit nonzero; invalid
# references must be refused before publication (not reported as helper execution).
#
# S2-N (phase-04-s2n-net-annex.md §7, E6): the image also carries /bin/net, /bin/e1000 and
# /bin/curl, and QEMU gets one e1000 on `-netdev user,restrict=on` with no hostfwd/guestfwd:
# DHCP works, nothing reaches the host or the outside. `/bin/p03-filter net-probe` (empty
# edge ceiling = the shell's own net rights) must get `Err(0xFC)` for every gated request,
# and `/bin/curl http://10.0.2.2:9/` on its reviewed edge must be admitted and then fail on
# the network (`curl: empty response` on the UART inside the driver's curl frame), never
# with a refusal. No guest-local data transfer is claimed.
#
# S2-T/S2-W (phase-04-s2tw-annex.md §6): one signed `s2tw-probe` ELF declaring GetProcs,
# GetProcs2, TelemetrySnapshot, MemInfo and VfsMutate is packaged twice in the private
# VIFS1, at the sandbox path /data/bin/s2tw-probe (reviewed-target default arm, EMPTY
# ceiling) and at /bin/s2tw-probe (private `phase04-driver-profile` row,
# LEGACY_OBSERVE_MUTATE). From the sandbox path all five rights must read `denied` and
# /tmp/s2tw-probe-sb must not exist; from /bin all five must be admitted (the Phase 06
# S6-1 telemetry snapshot must also decode) and /tmp/s2tw-probe-ok must exist.
#
# S5-3 option C (phase-05-s3c-frontend-annex.md §7): `frontend-*` markers. The shell is the
# controller of its GUI-kind session: `rm -i` answered and `top` quit with `q` through the
# frontend; ReleaseFrontend leaves the shell running Detached (the shell's own UART log line
# `shell: frontend detached` = it observed Lost(4)); ReattachFrontend with the holder-bound
# resume reaches the same shell (cwd and history preserved, prompt redrawn, UART line
# `shell: frontend re-attached`) and the old attachment is Stale. Holder death is not
# exercised: the holder is the driver itself.
#
# S5-2 (phase-05-s2-vt-screen-proposal.md, D52-2 as decided): `/bin/p03-filter tui-abandon`
# exits with the alternate screen, a hidden cursor and bracketed paste left on. The host
# stdout must carry its bytes followed by exactly one `cellos-vt1` soft reset
# (ESC[?1049l ESC[?25h ESC[?2004l ESC[0m) before the prompt, alone and as a pipeline
# stage; builtin echo, /bin/echo and `p03-filter rows` must be followed by none.
#
# S5-3 selective TUI (phase-05-s3-proposal.md §3 item 4): one-shot ls/ps/cat emit no TUI
# screen bytes, and `top` stays batch or refused in pipelines. The raw-lease part of
# "one-shot commands take no lease" is not observable by a frontend and is not claimed.
# The throwaway S5-3 proof consumer, its private launch row and its frame, key, resize,
# refusal and two-session checks were removed in S5-4e (phase-05-s4-proposal.md D54-4);
# their runs remain archived evidence only.
#
# S6-4 (phase-06-s4-proposal.md §7.2): `top-tui-*` markers read the interactive Ratatui
# `top` through the driver's grid model of the host stdout (screen.rs: CUP, ED/EL, SGR incl.
# 38;2/48;2, 1049/25/2004, UTF-8, last-column pending wrap) by structure only: border
# glyphs, the highlight symbol, `id:gen` tokens, theme colours, sample ids, statuses and
# mode bytes. `frontend-top-quit` waits on the same model. `top-subst-refused`: `x=$(top)`
# has status 2, a diagnostic naming `-b` and no TUI bytes. The idle-cost ceiling
# (500 tenths of CPU%) is an implementation choice, printed as `P04 INFO top-tui-idle-cost`.
#
# S6-5 (phase-06-s5-proposal.md §4, D65-3..D65-11): `top-load-*` markers run `top` against a
# real multi-thread Cell. The private fixture cells/tests/top-load is built, signed and
# packaged here only, at the sandbox path /data/bin/top-load (EMPTY ceiling; no launch row,
# no kernel change, DV-13). Two instances (`top-load 31`, 32 tasks each) run in the
# foreground of their own driver hosts while `top` runs on a third (no `&`); the driver steers
# them with typed lines and keys every check on their `TOPLOAD` acknowledgements and on data
# (`tids=`, records rows, the grid model). `control-telemetry-truncated`: `/bin/s2tw-probe
# trunc` captures into a header + one Cell row buffer and must see the provider report the
# truncation (one Cell of more, no task, COMPLETE clear). Every compared value is printed as
# `P04 INFO top-load <marker> key=value …` (D65-9) and echoed below with the
# `top-tui-idle-cost` value; the fixture's `bench` line records the per-call GetTime(0) and
# Yield cost (D65-10, no budget). Values are TCG with one vCPU: no CPU-speed claim.
#
# S5-4a (phase-05-s4-proposal.md §2 items 1, 3, 4): `secret-read-*` markers: `read -s SECRET`
# through the frontend with a fixed fixture token typed after the command line; the exact
# stdout is the command echo, one newline and the prompt (nothing echoed), `echo ${#SECRET}`
# prints only the length, `history` lists only the typed lines, and the token is in no
# drained byte, no driver INFO/FAIL line and nowhere on the UART (text or hex).
# `hostile-*`: builtin `cat` of the host-built /p04-output/vt-hostile.bin
# (scripts/phase05_vt_hostile.py: OSC 52 set/query, OSC 8, OSC 0/2, >4 KiB DCS/APC/PM,
# DA/DSR/DECRQM, 8-bit C1, a 17-parameter CSI, embedded ESC[201~) relays exactly the expected
# bytes, and nothing reaches the shell as input: the next command's stdout is exact and
# `history` lists only typed lines. Screen effects are judged by the GUI runner, not here.
# `paste-*`: a frontend paste containing ESC[201~ is refused whole by the SDK
# (UnsupportedInput, before any owner request) and by the input owner when sent directly
# (CommitPaste UnsupportedInput, then AbortPaste); nothing of it runs (exact next command,
# no /tmp/p04-paste-ran). A clean paste through the same API is the positive control.
# The physical-UART refusal of `read -s` is checked by scripts/qemu-phase02-session-test.sh.
#
# S7-3 (phase-07-s3-proposal.md §7.2): `rg-*` markers, fixture-free. `rg -h` status 0 with a
# usage; unsupported/combined flags and `-` without `-a` are status 2 with no row; stdin rows
# are `<stdin>:LINE:TEXT` (0 on a match, 1 on none); a /tmp/rgt RamFS tree built with
# mkdir/echo gives absolute `PATH:LINE:TEXT` rows without its hidden and `.ignore`d files
# (`--hidden --no-ignore` adds them); redirected rg is typed JSONL whose `from jsonl --typed |
# select line_number` gives the same line numbers; ESC (from `p03-filter tui-abandon`) and CR
# (`echo -e` through `p03-filter cat`) in a file render as `\x1B`/`\r` and never reach the
# shell's stdout raw. Each asserts structure and `$?`, never wording.
#
# Phase 10 S10-0 (phase-10-s0-proposal.md §2 S10-0.8, CP1 2026-10-26): `oneshot-*` markers.
# The driver launches /bin/cellbox directly on its reviewed fixture edge with a prepared
# context (argv without the program path, no cwd, no environment, Stdout/Stderr and
# optionally Stdin grants): exact bytes and status of catalog commands, typed JSONL by
# default and `--text`, refusals 2, unknown names and paths 127 with no task started, a
# missing Stdout grant writes nothing, a closed reader stops a streamed `rg` with 1, an
# Operation cancel ends `sleep 30`, and a terminal frontend runs only `sh` (2).
# `shell-explicit-prefix`: `cellbox CMD` and `/bin/cellbox CMD` inside the shell are the
# short forms, in process. Every one-shot payload carries `p04os-`; `oneshot-not-on-uart`
# requires that token absent from the raw serial log.
#
# Repo checks run before anything is built: the GUI Terminal source carries none of the old
# fake matcher/fixed responses, and the cellbox syscall ceiling is unchanged
# (scripts/cellbox-ceiling-diff.py). The Terminal's own ceiling delta is recorded.
#
# Signing a cellbox-named ELF needs CELLBOX_PRIVATE_FIXTURE_SIGNING=1, set only on the
# signing step of this private fixture.
#
# Usage: CELLBOX_PHASE04_WORK=/tmp/cellbox-phase04-XXXX bash scripts/qemu-phase04-terminal-test.sh [--candidate <candidate.json>]
# Optional: PHASE04_ACCEL=tcg|kvm (default tcg), PHASE04_BOOT_WINDOW (seconds, default 2700:
# the S7-4 7 MiB `rg` read alone takes about nine minutes on TCG).
#
# S10-3.3 import mode (D3-1 A, opt-in): `--candidate` takes the shared cells and init from the
# candidate's headless profile instead of building them (scripts/phase10_candidate_import.py),
# re-signs the copies with this run's key in the signing call below and requires their
# signed payloads to equal the candidate's (evidence/payload-match.json) before any boot.

set -euo pipefail

CANDIDATE=""
while (($#)); do
    case "$1" in
        --candidate)
            [[ $# -ge 2 && -f "$2" ]] || { echo "FAIL: --candidate needs an existing candidate.json" >&2; exit 1; }
            CANDIDATE="$(realpath -e -- "$2")"; shift 2 ;;
        *) echo "FAIL: unknown argument: $1" >&2; exit 1 ;;
    esac
done

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

W="${CELLBOX_PHASE04_WORK:?set CELLBOX_PHASE04_WORK to the owned /tmp/cellbox-phase04-* directory}"
case "$W" in
    /tmp/cellbox-phase04-*) ;;
    *) echo "FAIL: work directory must be /tmp/cellbox-phase04-*" >&2; exit 1 ;;
esac
[[ -d "$W" ]] || { echo "FAIL: work directory missing: $W" >&2; exit 1; }
ACCEL="${PHASE04_ACCEL:-tcg}"
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: PHASE04_ACCEL must be tcg or kvm" >&2; exit 1 ;; esac
WINDOW="${PHASE04_BOOT_WINDOW:-2700}"
# The private-key signing path (build.rs/signing.rs) is keyed on phase02-terminal-profile;
# phase04-driver-profile adds only the init -> driver and driver -> /bin/cellbox rows and
# the /bin/s2tw-probe fixture target row (S2-T/S2-W positive control);
# phase03-filter-profile is the existing private `cellbox -> /bin/p03-filter` row, kept so
# the Phase03 stream fixture can prove exact bytes through the PipeChild path.
FEATURES="phase02-terminal-profile,phase03-filter-profile,phase04-driver-profile"
export PHASE04_FEATURES="$FEATURES"
PYTHON_BIN="${PYTHON_BIN:-python3}"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
PRIVATE_CONFIG="$W/private-config"
SCRATCH="$W/scratch"
ISO="$W/phase04.iso"
REL="$CELLS_TARGET_DIR/$TARGET/release"

# Refuse any alias of the quarantined/canonical artifacts.
for guarded in "$ROOT/kernel/src/embedded-x86_64" "$ROOT/target/x86_64-unknown-none/release"; do
    [[ "$(realpath -m "$W")" != "$(realpath -m "$guarded")"* ]] || { echo "FAIL: work dir aliases $guarded" >&2; exit 1; }
done
# Resolve before any deletion/build/signing; reject symlinked output roots and
# nested target aliases even when the caller supplied an apparently private name.
"$PYTHON_BIN" - "$W" <<'PY'
import os, pathlib, sys
w = pathlib.Path(sys.argv[1])
if w.is_symlink() or w.resolve().parent != pathlib.Path("/tmp") or not w.name.startswith("cellbox-phase04-"):
    raise SystemExit("FAIL: work directory must be a real direct /tmp/cellbox-phase04-* directory")
if w.stat().st_uid != os.getuid():
    raise SystemExit("FAIL: work directory is not owned by this uid")
for name in ("cells-target", "kernel-target", "embedded", "keys", "evidence", "private-config", "scratch", "iso-root"):
    path = w / name
    if path.is_symlink() or not path.resolve().is_relative_to(w.resolve()):
        raise SystemExit("FAIL: private output root aliases another directory: " + name)
    if path.exists() and any(p.is_symlink() for p in path.rglob("*")):
        raise SystemExit("FAIL: private output tree contains a symlink: " + name)
PY

# Invalidate every output first (stale artifacts must never be booted).
rm -rf "$EMBEDDED" "$EVIDENCE" "$ISO" "$W/iso-root" "$PRIVATE_CONFIG" "$SCRATCH" \
    "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
mkdir -p "$EMBEDDED" "$KEYS" "$EVIDENCE" "$PRIVATE_CONFIG" "$SCRATCH"
chmod 700 "$KEYS"

echo "==> Repository checks (no build): fake Terminal matcher gone, cellbox ceiling unchanged"
"$PYTHON_BIN" - "$ROOT" "$EVIDENCE" <<'PY'
import pathlib, sys
root, evidence = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
terminal = root / "cells/apps/terminal"
sources = sorted(p for p in terminal.rglob("*") if p.is_file() and p.suffix in (".rs", ".toml"))
if not sources:
    raise SystemExit("FAIL: no sources found under cells/apps/terminal")
# Fixed strings of the former private matcher and its canned responses (any file).
banned_everywhere = [
    "CellOS GUI Terminal (x86_64)",
    "Available commands:",
    "CellOS 0.2.1 x86_64 Intel VT-x C2C",
    "Tue Oct 20 06:00:00 UTC 2026",
    "bin/  apps/  data/  etc/  welcome.md",
    "PID  NAME         STATUS",
    "running (active)",
    "[terminal] Session terminated.",
]
# The private split/dispatch of session.rs.
banned_in_session = ["fn execute_command", "split_whitespace", '"uname" =>', '"whoami" =>']
found = []
for path in sources:
    text = path.read_text(encoding="utf-8", errors="replace")
    for needle in banned_everywhere:
        if needle in text:
            found.append(f"{path.relative_to(root)}: {needle!r}")
    if path.name == "session.rs":
        for needle in banned_in_session:
            if needle in text:
                found.append(f"{path.relative_to(root)}: {needle!r}")
(evidence / "terminal-fake-check.txt").write_text(
    "scanned: " + ", ".join(str(p.relative_to(root)) for p in sources) + "\nfound: " + repr(found) + "\n")
if found:
    raise SystemExit("FAIL: the former fake Terminal matcher/fixed responses are still present:\n  " + "\n  ".join(found))
if not any("FrontendHost" in p.read_text(errors="replace") for p in sources if p.suffix == ".rs"):
    raise SystemExit("FAIL: the GUI Terminal sources do not use the SDK FrontendHost")
if "terminal-sdk" not in (terminal / "Cargo.toml").read_text():
    raise SystemExit("FAIL: Terminal Cargo.toml has no terminal-sdk dependency")
print("fake matcher check: none of", len(banned_everywhere) + len(banned_in_session), "banned strings in", len(sources), "Terminal files")
PY
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py | tee "$EVIDENCE/cellbox-ceiling.txt"
# C5a: cellbox builds no UI stack (headless data commands need no viUI/compositor).
CELLBOX_TREE=$(cargo tree --offline --locked -p app-cellbox -e normal --prefix none) \
    || { echo "FAIL: cargo tree for app-cellbox failed" >&2; exit 1; }
UI_DEPS=$(awk '{print $1}' <<<"$CELLBOX_TREE" | grep -Ex '(viui.*|service-compositor|.*compositor.*|vi-surface.*)' | sort -u || true)
if [[ -n "$UI_DEPS" ]]; then echo "FAIL: app-cellbox depends on UI crates: $UI_DEPS" >&2; exit 1; fi
echo "C5a PASS: app-cellbox dependency tree has no viUI/compositor crate" | tee "$EVIDENCE/cellbox-ui-deps.txt"
# Terminal's own declared ceiling versus HEAD is informational (G3d lists any delta).
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py cells/apps/terminal/src/main.rs \
    --baseline-ref HEAD --baseline-path cells/apps/terminal/src/main.rs \
    > "$EVIDENCE/terminal-ceiling-delta.txt" 2>&1 || true
echo "terminal ceiling delta (informational):"; sed 's/^/    /' "$EVIDENCE/terminal-ceiling-delta.txt"

# Preserve preexisting authorized phase work plus the approved private echo edge;
# all tool ceilings remain unchanged except the approved free GetTime addition.
"$PYTHON_BIN" - "$ROOT" "$EVIDENCE" <<'PY'
import hashlib, json, pathlib, re, subprocess, sys
root, evidence = map(pathlib.Path, sys.argv[1:])
def baseline(path):
    return subprocess.check_output(["git", "show", "HEAD:" + str(path)], cwd=root)
rows, failures = {}, []
for path in sorted((root / "cells/tools/sys-tools/src/bin").glob("*.rs")):
    relative = path.relative_to(root)
    before, after = baseline(relative).decode(), path.read_text()
    def declarations(source):
        return re.findall(r"api::declare_(?:manifest|syscalls)!\s*[\[(].*?[\])]\s*;", source, re.S)
    old, new = declarations(before), declarations(after)
    if path.stem == "free":
        old = [re.sub(r",?\s*GetTime\s*,?", "", d) for d in old]
        new = [re.sub(r",?\s*GetTime\s*,?", "", d) for d in new]
    ok = [re.sub(r"\s+", "", d) for d in old] == [re.sub(r"\s+", "", d) for d in new]
    rows[str(relative)] = {"ceiling_unchanged_except_free_GetTime": ok}
    if not ok: failures.append(str(relative))
# Reviewed authorized source digests, embedded so a clean checkout does not need
# ignored .agents evidence. These include prior approved phases, the explicit
# 2026-10-22 private echo-edge amendment, the CP1-approved S2-N net-rights rows
# (phase-04-s2n-net-annex.md §3.4/§5) and the CP1-approved S2-T/S2-W rows
# (phase-04-s2tw-annex.md §3/§5: LEGACY_OBSERVE_MUTATE on every pre-existing
# non-sandbox row plus the private /bin/s2tw-probe fixture row), the CP1-approved
# S5-3 option C frontend ABI (phase-05-s3c-frontend-annex.md §3: selectors 23-28,
# context kind 6 / slot 8), with the S5-3 private proof-consumer row removed again in
# S5-4e (phase-05-s4-proposal.md D54-4), the Phase 09 S9-2 shell-only Path edge to
# /bin/cellbox-ssh (phase-09-ssh-client.md D79-9 as revised, CP1 2026-10-25) and the
# Phase 09 S9-4b trust-confirmation selectors 29-31 plus the S9-4c serde derive on
# ObjectRefV1 (phase-09-s0-proposal.md §10, CP1 2026-10-25), not a clean-HEAD baseline claim.
# Re-pinned 2026-10-25 for the Phase 01 CP1 Q right (phase-01-config-right-cp1.md
# §4.1/§4.2, D-Q5): profiles.rs gives the terminal and private driver shell edges
# SHELL_EDGE (LEGACY + config_write) and fixes the terminal-edge comment wording;
# tests.rs pins those ceilings and the Q grant table.
authority = {
    "kernel/src/loader/launch_profile/mod.rs": "b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c",
    "kernel/src/loader/launch_profile/profiles.rs": "12ff2f739cd7440d2d6b9bce22bef32460b764df6411aff396ebe6a3a93134e2",
    "kernel/src/loader/launch_profile/targets.rs": "45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f",
    "kernel/src/loader/launch_profile/tests.rs": "b3d19ebd2670207afdb607c023729643c6ac56edfa2453d5294b2ae37f718170",
    "libs/api/src/abi/cell_session.rs": "590fe5ccaa74ae25c751dedb111d76ff6ec102edd52c088198daa28666441595",
}
for name, expected in authority.items():
    path = root / name
    ok = path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == expected
    rows[name] = {"unchanged_from_preexisting_authorized_source": ok}
    if not ok: failures.append(name)
(evidence / "output-authority-invariants.json").write_text(json.dumps(rows, indent=2) + "\n")
if failures: raise SystemExit("FAIL: unapproved ceiling/launch/wire changes: " + ", ".join(failures))
PY

echo "==> Generating throw-away cell signing key (private to this run)"
"$PYTHON_BIN" - "$KEYS" <<'PY'
import os, sys, hashlib
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
keys = Path(sys.argv[1])
seed = os.urandom(32)
priv = Ed25519PrivateKey.from_private_bytes(seed)
pub = priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
seed_file = keys / "seed.hex"
fd = os.open(seed_file, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(seed.hex())
(keys / "cell.pub").write_bytes(pub)
print("key identity sha256:", hashlib.sha256(pub).hexdigest())
PY

# x86_64 CELLS use relocation-model=pic (the kernel profile keeps static).
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building cells (private target dir)"
# `terminal` (the GUI Terminal) is built as a compile proof only; the private image has no
# compositor and does not carry it.
if [[ -z "$CANDIDATE" ]]; then
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
        -p driver-nvme -p driver-e1000 -p app-sys-tools -p service-net -p app-net-tools \
        -p app-phase04-terminal-driver -p app-phase03-filter -p app-s2tw-probe -p app-top-load -p terminal
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-init --features input
else
    # Import mode: only the lane-only cells are built (app-sys-tools for free/date/env/uname/
    # hotswap; its shared workers are replaced by the candidate copies below), plus the
    # unpackaged `terminal` compile proof.
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-sys-tools -p app-net-tools \
        -p app-phase04-terminal-driver -p app-phase03-filter -p app-s2tw-probe -p app-top-load -p terminal
fi

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/driver-nvme" "$REL/driver-e1000" "$REL/service-net"
    "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps" "$REL/free"
    "$REL/date" "$REL/env" "$REL/uname" "$REL/kill" "$REL/shutdown" "$REL/hotswap"
    "$REL/curl" "$REL/phase04-terminal-driver" "$REL/phase03-filter" "$REL/s2tw-probe"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/nvme /bin/e1000 /bin/net
    /bin/ls /bin/cat /bin/echo /bin/ps /bin/free
    /bin/date /bin/env /bin/uname /bin/kill /bin/shutdown /bin/hotswap
    /bin/curl /bin/phase04-terminal-driver /bin/p03-filter /bin/s2tw-probe
)
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Importing the shared cells from the candidate (headless profile)"
    IMPORT_ARGS=(--cell "/data/bin/s2tw-probe=$REL/s2tw-probe" --cell "/data/bin/top-load=$REL/top-load")
    for i in "${!CELL_BINARIES[@]}"; do IMPORT_ARGS+=(--cell "${CELL_IMAGE_PATHS[$i]}=${CELL_BINARIES[$i]}"); done
    "$PYTHON_BIN" scripts/phase10_candidate_import.py import --candidate "$CANDIDATE" --profile headless \
        --lane phase04-terminal --release-dir "$REL" --evidence "$EVIDENCE" --init "$REL/app-init" "${IMPORT_ARGS[@]}"
fi
FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
done
# S2-T/S2-W: the same probe file also at the sandbox prefix. It is signed once, with
# CELL_BINARIES below, before mkfat32 reads either copy.
FAT_ARGS+=("$REL/s2tw-probe" /data/bin/s2tw-probe)
# S6-5: the load fixture only at the sandbox prefix (EMPTY ceiling, DV-13); signed below
# with the CELL_BINARIES.
[[ -s "$REL/top-load" ]] || { echo "FAIL: missing cell binary $REL/top-load" >&2; exit 1; }
FAT_ARGS+=("$REL/top-load" /data/bin/top-load)

# Immutable binary fixtures live only in this private FAT image: cat uses the kernel
# file route, not VFS RAM files. Keep the independent expected bytes in evidence.
# S5-4a: the hostile cellos-vt1 output fixture (same generator as the GUI runner).
"$PYTHON_BIN" scripts/phase05_vt_hostile.py "$SCRATCH/vt-hostile.bin"
"$PYTHON_BIN" - "$SCRATCH" "$EVIDENCE" <<'PY'
import hashlib, json, sys
from pathlib import Path
scratch, evidence = map(Path, sys.argv[1:])
def payload(n):
    data = bytearray((i * 73 + 19) % 256 for i in range(n))
    for index, start in enumerate(range(0, n, 512)):
        sentinel = f"P04-CHUNK-{index:04}-NO-UART!".encode()
        data[start + 8:start + 8 + len(sentinel)] = sentinel
    data[31:33] = b"\x00\xff"
    data[511:514] = b"\xe2\x82\xac"
    return bytes(data)
manifest = {}
for name, n in (("raw.bin", 1600), ("large.bin", 96 * 1024 + 64)):
    data = payload(n)
    (scratch / name).write_bytes(data)
    (evidence / ("expected-" + name)).write_bytes(data)
    manifest[name] = {"length": n, "sha256": hashlib.sha256(data).hexdigest()}
for name in ("alpha-entry", "beta-entry"):
    (scratch / name).write_bytes(b"private known directory fixture\n")
(scratch / "pressure-data").write_bytes(b"private pressure fixture\n")
listing = b".\n..\n" + b"".join((f"p04-pressure-{i:03}-" + "x" * 43 + "\n").encode() for i in range(256))
(evidence / "expected-pressure-ls.bin").write_bytes(listing)
manifest["pressure-ls"] = {"length": len(listing), "sha256": hashlib.sha256(listing).hexdigest()}
# The builtin `cat` relays valid UTF-8 unchanged and replaces each invalid byte (the 8-bit C1
# bytes 0x9b/0x9d/0x9c of the hostile fixture) with U+FFFD; everything else is byte-exact.
hostile = (scratch / "vt-hostile.bin").read_bytes()
(evidence / "expected-vt-hostile.bin").write_bytes(hostile)
(evidence / "expected-vt-hostile-relayed.bin").write_bytes(hostile.decode("utf-8", "replace").encode("utf-8"))
manifest["vt-hostile"] = {"length": len(hostile), "sha256": hashlib.sha256(hostile).hexdigest()}
(evidence / "output-fixtures.json").write_text(json.dumps(manifest, indent=2) + "\n")
(evidence / "acceptance-limits.txt").write_text(
    "AC-5 requires actual stock echo nonzero exits with missing stdout, stderr or both; "
    "invalid stdout refs must be rejected by canonical admission before publication, "
    "recorded distinctly from helper execution. All negative cases require empty "
    "owned streams, no unrelated host bytes, and independent no-UART-payload checks.\n"
    "AC-4 fragmented stock ls writes use 60-byte names and separate LF across full "
    "4096-byte rings; exact retained bytes are checked, but internal write return "
    "counts are not externally exposed and are not independently observed.\n"
    "date/env/uname/kill/shutdown/hotswap are signed and packaged but NOT launched: "
    "no broadened target admission, authority, shutdown or service mutation.\n"
    "S2-N: denial of Resolve/TcpConnect/UdpCreate/UdpBind/TcpListen/MulticastJoin/L2Send "
    "is observed as Err(0xFC) by a no-rights Cell; the unchanged net socket count is a "
    "host-test claim (service-net), not observed here. curl admission is observed as a "
    "network failure (no response) on restricted SLIRP; no data transfer is claimed.\n"
    "S2-T/S2-W: a telemetry refusal is observed as the syscall ABI's single generic error "
    "return (PermissionDenied is not distinguishable on the wire), so the same binary on "
    "its /bin fixture row must read rows for each call; the VFS refusal is Err(ERR_DENIED) "
    "to WriteAt through the delegated cwd handle. The kernel CapSet values themselves are "
    "host-test claims, not observed here.\n"
    "S5-3 option C: the shell's Lost(4)/FrontendAttached observations are its own fixed UART "
    "log lines; frontend holder death (resume revoked, Lost(4) then EOF) is not exercised "
    "because the holder is the driver itself; it is an input host-test claim here.\n"
    "S5-4a: the driver is the frontend, so no terminal emulator is in this loop: hostile output "
    "is judged as the exact relayed bytes plus the absence of any injected shell input (the next "
    "command's exact echo, history = typed lines only); screen effects are the GUI runner's claim. "
    "read -s: the token is typed 1.5 s after the command line, after the shell took its echo-off "
    "lease (the lease itself is not externally observable here). The physical-UART refusal of "
    "read -s is judged by the Phase02 serial runner.\n"
    "S6-5: TCG with one vCPU; every CPU threshold (D65-6) and every compared value is recorded "
    "as P04 INFO top-load, no CPU-speed or hardware claim follows. The same-snapshot numeric "
    "TUI = batch = records proof stays the constructed host pair (D64-14); here the views are "
    "compared on membership. top's own truncation rendering is host-tested; the runtime claim "
    "is the provider's explicit truncation (s2tw-probe trunc). Allocation failures, watchdog "
    "kills and faults during the load scenarios are judged by the log-wide no-OOM/no-panic "
    "checks and by both fixtures surviving to `bye` with status 0.\n")
PY
# S2-K: a KMS-namespace file in the boot ramdisk (private image only). VFS reserves
# /srv/cellos/** to the live KMS; the shell must never deliver these bytes.
printf 'p04-kms-namespace-fixture-token\n' > "$SCRATCH/p04-secret"
FAT_ARGS+=("$SCRATCH/raw.bin" /p04-output/raw.bin "$SCRATCH/large.bin" /p04-output/large.bin
    "$SCRATCH/alpha-entry" /p04-list/alpha-entry "$SCRATCH/beta-entry" /p04-list/beta-entry
    "$SCRATCH/p04-secret" /srv/cellos/p04-secret "$SCRATCH/vt-hostile.bin" /p04-output/vt-hostile.bin)
printf -v PRESSURE_PAD '%*s' 43 ''
PRESSURE_PAD="${PRESSURE_PAD// /x}"
for ((i = 0; i < 256; i++)); do
    printf -v pressure_name 'p04-pressure-%03d-%s' "$i" "$PRESSURE_PAD"
    FAT_ARGS+=("$SCRATCH/pressure-data" "/p04-pressure/$pressure_name")
done

echo "==> Signing with the private key after the F1/F5 policy check"
SEED_HEX="$(cat "$KEYS/seed.hex")"
CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
    --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/top-load" "$REL/app-init"
unset SEED_HEX

echo "==> Assembling private kernel_fs.img (boot config: the driver replaces the serial shell)"
# Pass 1: the supported generator selects the default plan for this artifact set.
"$PYTHON_BIN" tools/mkfat32.py --config-features input --config-output-dir "$EVIDENCE/boot-config-default" \
    "$SCRATCH/default-fs.img" "${FAT_ARGS[@]}"
# Private config: identical system/services, autoload with the shell entry replaced by the
# driver. The repository templates are never modified; the supported --config-dir hands the
# files to the generator verbatim.
"$PYTHON_BIN" - "$EVIDENCE/boot-config-default" "$PRIVATE_CONFIG" <<'PY'
import re, sys, tomllib
from pathlib import Path
src, dst = Path(sys.argv[1]), Path(sys.argv[2])
services_text = (src / "services.toml").read_text()
services = tomllib.loads(services_text)
# Phase 01 CP1 (Q right): the template selects /bin/config only for the `ai` profile,
# so the private plan adds its service row (service_id 4, as in config/services.toml)
# for the export/config-probe checks. The repository templates stay unchanged.
if not any(c.get("path") == "/bin/config" for c in services.get("cells", [])):
    services_text = services_text.rstrip("\n") + (
        '\n\n[[cells]]\nname = "config"\npath = "/bin/config"\nservice_id = 4\nrestart = "always"\n')
    services = tomllib.loads(services_text)
service_names = {c["name"] for c in services.get("cells", [])}
text = (src / "autoload.toml").read_text()
head, *blocks = re.split(r"(?m)^\[\[cells\]\]\s*$", text)
out, replaced = [], 0
for block in blocks:
    cell = tomllib.loads("[[cells]]\n" + block)["cells"][0]
    if cell.get("path") == "/bin/cellbox":
        after = [n for n in ("vfs", "input", "config") if n in service_names]
        new = 'name = "phase04-terminal-driver"\npath = "/bin/phase04-terminal-driver"\n'
        if after:
            new += "after = [" + ", ".join('"%s"' % n for n in after) + "]\n"
        new += 'restart = "never"\n'
        out.append(new)
        replaced += 1
    else:
        out.append(block.strip("\n") + "\n")
if replaced != 1:
    raise SystemExit("FAIL: expected exactly one cellbox autoload entry to replace, found %d" % replaced)
private = head.rstrip("\n") + "\n" + "".join("\n[[cells]]\n" + b for b in out)
# An unchanged stock binary launched by init has no committed context. Its two
# legacy UART rows are independently required, while the context-mode /bin/free
# run must stay on the host stream. Reuse init's existing reviewed /bin/free edge.
private += '\n[[cells]]\nname = "p04-legacy-free"\npath = "/bin/free"\nrestart = "never"\n'
cells = tomllib.loads(private)["cells"]
mine = [c for c in cells if c.get("path") == "/bin/phase04-terminal-driver"]
if len(mine) != 1 or mine[0].get("name") != "phase04-terminal-driver" or mine[0].get("restart") != "never":
    raise SystemExit("FAIL: private autoload must carry exactly one never-restarted driver entry: %r" % mine)
if any(c.get("path") in ("/bin/cellbox", "/bin/shell") or c.get("name") in ("cellbox", "shell") for c in cells):
    raise SystemExit("FAIL: private autoload still launches a shell")
(dst / "autoload.toml").write_text(private)
# S2-N: packaging /bin/net and /bin/e1000 must select both service rows from the template.
services_cells = services.get("cells", [])
for path in ("/bin/net", "/bin/e1000", "/bin/config"):
    if not any(c.get("path") == path and c.get("enabled", True) for c in services_cells):
        raise SystemExit("FAIL: private services.toml does not start %s" % path)
(dst / "services.toml").write_text(services_text)
(dst / "system.toml").write_bytes((src / "system.toml").read_bytes())
print("private autoload plan:", [(c["name"], c["path"]) for c in cells])
print("private services plan:", [(c["name"], c["path"]) for c in services_cells])
PY
rm -f "$SCRATCH/default-fs.img"
# Pass 2: the real image from the private config.
"$PYTHON_BIN" tools/mkfat32.py --config-features input --config-dir "$PRIVATE_CONFIG" \
    --config-output-dir "$EVIDENCE/boot-config" "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
cmp -s "$EVIDENCE/boot-config/autoload.toml" "$PRIVATE_CONFIG/autoload.toml" \
    || { echo "FAIL: image autoload differs from the private plan" >&2; exit 1; }
[[ -s "$EMBEDDED/kernel_fs.img" ]] || { echo "FAIL: empty kernel_fs.img" >&2; exit 1; }
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/fat-layout.txt"
for path in "${CELL_IMAGE_PATHS[@]}"; do
    grep -Fq -- "-> LFN '${path#/bin/}'" "$EVIDENCE/fat-layout.txt" || { echo "FAIL: image misses $path" >&2; exit 1; }
done
# Both probe copies share one long name, so check each in its own directory listing; the
# S6-5 load fixture exists only at the sandbox prefix.
"$PYTHON_BIN" - "$EVIDENCE/fat-layout.txt" /bin/s2tw-probe /data/bin/s2tw-probe /data/bin/top-load <<'PY'
import re, sys
layout, *paths = sys.argv[1:]
sections, current = {}, None
for line in open(layout, errors="replace"):
    header = re.match(r"  --- (.+) ---$", line.rstrip("\n"))
    if header:
        current = sections.setdefault(header[1].lower(), [])
    elif current is not None:
        current.append(line)
for path in paths:
    directory, name = path.rsplit("/", 1)
    if not any(f"-> LFN '{name}'" in l for l in sections.get(directory.lower(), [])):
        raise SystemExit(f"FAIL: image misses {path}")
PY
cp "$REL/app-init" "$EMBEDDED/init"
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Payload match of the re-signed candidate cells (before boot)"
    "$PYTHON_BIN" scripts/phase10_candidate_import.py verify --evidence "$EVIDENCE" \
        --image "$EMBEDDED/kernel_fs.img" --init-image "$EMBEDDED/init" --trust-pub "$KEYS/cell.pub"
fi

echo "==> Building private kernel ($FEATURES)"
export CELLOS_PHASE02_CELL_PUBLIC_KEY="$KEYS/cell.pub"
EMBEDDED_OVERRIDE="$EMBEDDED" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release --locked --offline -p cellos-kernel --features "$FEATURES" \
    --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced" >&2; exit 1; }

echo "==> Building private ISO"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$W/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
[[ -s "$ISO" ]] || { echo "FAIL: ISO not produced" >&2; exit 1; }

echo "==> Recording provenance"
"$PYTHON_BIN" - "$W" "$ROOT" <<'PY'
import hashlib, json, os, subprocess, sys
from pathlib import Path
w, root = Path(sys.argv[1]), Path(sys.argv[2])
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
rel = w / "cells-target/x86_64-unknown-none/release"
names = ["app-init","cellbox","service-vfs","service-config","service-input","platform",
         "driver-nvme","driver-e1000","service-net","ls","cat","echo","ps","free","date","env","uname",
         "kill","shutdown","hotswap","curl","phase04-terminal-driver","phase03-filter","s2tw-probe",
         "top-load"]
prov = {
    "status": "private fixture provenance; not acceptance",
    "source_head": subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),
    "rustc": subprocess.check_output(["rustc","-Vv"],text=True),
    "cargo_lock_sha256": sha(root/"Cargo.lock"),
    "kernel_features": "default+" + os.environ["PHASE04_FEATURES"],
    "signed_cells_sha256": {n: sha(rel/n) for n in names},
    "init_sha256": sha(w/"embedded/init"),
    "kernel_fs_img_sha256": sha(w/"embedded/kernel_fs.img"),
    "kernel_sha256": sha(w/"kernel-target/x86_64-unknown-none/release/cellos-kernel"),
    "iso_sha256": sha(w/"phase04.iso"),
    "autoload_toml_sha256": sha(w/"evidence/boot-config/autoload.toml"),
    "trusted_key_identity_sha256": sha(w/"keys/cell.pub"),
    "policy": "no /POLICY.BIN in the image; the kernel logs it absent and applies its development-permissive posture",
    "signature_verification": "cellos-sign re-verifies each ELF after signing; kernel verifies at spawn (signing-required)",
    "gui_compositor_not_exercised": True,
    "acceptance": "private stock-output runtime gates including approved negative-grant echo fixture; not GUI acceptance",
    "runtime_stock_tools": ["echo", "ls", "cat", "free"],
    "legacy_no_context_tool": "free (init existing autoload edge)",
    "network": "e1000 on QEMU -netdev user,restrict=on; no hostfwd/guestfwd (S2-N)",
    "s6_5_fixture": "top-load at /data/bin/top-load only (EMPTY sandbox ceiling, DV-13); TCG, one vCPU, HPET ns",
    "output_fixtures_sha256": sha(w/"evidence/output-fixtures.json"),
    "acceptance_limits_sha256": sha(w/"evidence/acceptance-limits.txt"),
}
(w/"evidence/provenance.json").write_text(json.dumps(prov, indent=2)+"\n")
print(json.dumps({k:v for k,v in prov.items() if k.endswith("sha256")}, indent=1))
PY
if [[ -n "$CANDIDATE" ]]; then
    "$PYTHON_BIN" scripts/phase10_candidate_import.py annotate --evidence "$EVIDENCE" \
        --kernel-features "default+$FEATURES" --into "$EVIDENCE/provenance.json"
fi

# S7-4 (phase-07-s4-proposal.md §2.1/§2.2, D77-1/2): the `rg` fixture is a private FAT32
# volume (P1, /mnt/sd in the guest) on a throw-away NVMe disk in this work directory.
echo "==> Generating the private rg fixture disk"
RG_DISK="$W/rg-fixture.img"
rm -f "$RG_DISK"
"$PYTHON_BIN" scripts/phase07_rg_fixture.py "$RG_DISK" "$EVIDENCE/rg-fixture-expected.json" "$EVIDENCE/boot-config"

echo "==> Booting private ISO (owned QEMU process only, restricted SLIRP: no host forward, no outside access)"
"$PYTHON_BIN" - "$ISO" "$EVIDENCE" "$ACCEL" "$WINDOW" "$RG_DISK" <<'PY'
import os, select, socket, subprocess, sys, time
iso, evidence, accel, window, rg_disk = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
sock_path = os.path.join(os.path.dirname(evidence), "serial.sock")
try: os.unlink(sock_path)
except FileNotFoundError: pass
argv = ["qemu-system-x86_64","-machine","q35","-accel",accel,"-cpu","qemu64,+pdpe1gb" if accel=="tcg" else "host",
        "-m","256M","-display","none","-chardev",f"socket,id=ser0,path={sock_path},server=on,wait=off","-serial","chardev:ser0",
        "-monitor","none","-cdrom",iso,"-boot","d","-no-reboot",
        "-netdev","user,id=net0,restrict=on","-device","e1000,netdev=net0",
        "-drive",f"file={rg_disk},if=none,format=raw,id=rgfx","-device","nvme,drive=rgfx,serial=cellosrg"]
# S2-N: exactly one restricted user netdev, never a forward to the host.
netdevs = [argv[i + 1] for i, a in enumerate(argv) if a == "-netdev"]
if (netdevs != ["user,id=net0,restrict=on"] or any(a in ("-nic", "-net") for a in argv)
        or any("hostfwd" in a or "guestfwd" in a for a in argv)):
    raise SystemExit("FAIL: QEMU networking must be one restricted SLIRP netdev without forwards")
open(f"{evidence}/qemu-argv.txt","w").write(" ".join(argv)+"\n")
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
conn = None
for _ in range(100):
    try:
        conn = socket.socket(socket.AF_UNIX); conn.connect(sock_path); break
    except OSError:
        conn = None; time.sleep(0.1)
if conn is None:
    proc.kill(); raise SystemExit("QEMU serial socket never appeared")
log = bytearray(); deadline = time.time() + window
def pump(timeout=0.5):
    try:
        r,_,_ = select.select([conn],[],[],timeout)
        data = conn.recv(65536) if r else b""
    except OSError:
        return False
    if data: log.extend(data); return True
    return False
def text(): return log.replace(b"\x00", b"").decode("utf-8","replace")
verdict = "no-verdict"
try:
    # The driver is fully autonomous: nothing is typed on the serial line. Wait for its
    # verdict line (or the window), then keep reading briefly for trailing output.
    while time.time() < deadline and proc.poll() is None:
        pump()
        if "P04-TERMINAL:" in text():
            verdict = "driver-verdict"; break
    end = time.time() + 3
    while time.time() < end and proc.poll() is None:
        pump(0.3)
finally:
    if proc.poll() is None:
        proc.terminate()
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    try: pump(0.1)
    except OSError: pass
    open(f"{evidence}/serial.raw.log","wb").write(bytes(log))
    open(f"{evidence}/serial.log","w").write(text())
print("QEMU run:", verdict)
PY

echo "==> Evaluating driver observations"
"$PYTHON_BIN" - "$EVIDENCE/serial.log" "$KEYS/seed.hex" "$EVIDENCE" <<'PY'
import json, pathlib, re, sys
log = open(sys.argv[1], errors="replace").read()
seed = open(sys.argv[2]).read().strip()
evidence = pathlib.Path(sys.argv[3])
EXPECTED = [
    "shell-prompt",
    "builtin-echo", "builtin-pwd", "builtin-free", "builtin-date-changes", "builtin-ps-records",
    "headless-no-ui-tasks",
    "typed-pipeline", "typed-pipeline-empty",
    "file-create-ls", "file-cat-roundtrip",
    "external-echo", "external-ls", "external-cat", "external-stdout-not-stderr",
    "external-ls-stderr", "external-free",
    "stock-full-ring-drain-exact", "stock-downstream-close-recovers", "stock-full-ring-cancel",
    "stock-fragmented-ring-drain-exact", "stock-full-ring-host-close",
    "negative-missing-stdout", "negative-missing-stderr", "negative-missing-both",
    "negative-invalid-stdout", "negative-grants-host-isolation",
    "external-filter-single", "external-filter-pipeline", "external-filter-exact-bytes",
    "soft-reset-after-tui-abandon", "soft-reset-after-tui-pipeline",
    "soft-reset-none-builtin-echo", "soft-reset-none-external-echo", "soft-reset-none-filter",
    "exit-status", "exit-status-external-7", "exit-status-unknown-program",
    # Phase 07 S7-3 §7.2: fixture-free `rg` (RamFS /tmp trees, stdin, escapes), status + structure.
    "rg-help", "rg-unsupported-before-reads", "rg-stdin-requires-a", "rg-stdin-match",
    "rg-tmp-tree", "rg-typed-roundtrip", "rg-escape",
    # Phase 07 S7-4 §2.2: the private FAT32 /mnt/sd/rg fixture (scripts/phase07_rg_fixture.py).
    "rg-fx-big-exact", "rg-fx-wide", "rg-fx-ignore", "rg-fx-hidden", "rg-fx-binary",
    "rg-fx-enc", "rg-fx-denied-root", "rg-fx-denied-child", "rg-fx-take-no-leak",
    # Phase 08 S8-5: `edit` over the fixture's CellosFS /srv and FAT32 /mnt/sd.
    "edit-refusals", "edit-save-reopen-exact", "edit-conflict-keeps-buffer",
    "edit-saveas-overwrite-race", "edit-fat-save-refused", "edit-paste-one-group",
    "edit-dirty-exit-no-default", "edit-resize", "edit-quit-restores", "edit-find-select-redo",
    "ctrl-c-recovers", "ctrl-c-output-stops", "ctrl-c-status-130", "ctrl-c-shell-alive",
    "ctrl-c-external-pipeline", "ctrl-c-idle-prompt",
    "endless-local-sink-streams-live", "endless-local-sink-ctrl-c-130",
    "endless-local-sink-shell-alive", "endless-local-sink-no-oom",
    *[f"secret-denied-{i}" for i in range(9)], "secret-no-copy", "secret-never-shown",
    "ramdisk-read-still-works",
    "secret-read-silent", "secret-read-length", "secret-read-history", "secret-read-never-drained",
    "hostile-cat-relayed", "hostile-next-command-exact", "hostile-history-typed-only",
    "paste-clean-control", "paste-escape-rejected-sdk", "paste-escape-rejected-owner",
    "paste-escape-nothing-executed",
    *[f"net-denied-{op}" for op in ("resolve", "tcp-connect", "udp-create", "udp-bind",
                                    "tcp-listen", "multicast-join", "l2-send")],
    "net-probe-done", "net-curl-admitted",
    # Phase 01 CP1 (Q right): `export` through service-config's attested key gate,
    # and the rights-free /bin/p03-filter config-probe.
    "config-export-granted", "config-export-system-denied", "config-export-namespace-denied",
    "config-denial-on-stderr-only", "config-probe-set-denied", "config-probe-delete-denied",
    *[f"sandbox-denied-{r}" for r in ("getprocs", "getprocs2", "telemetry", "meminfo", "vfs-write")],
    "sandbox-no-file",
    *[f"control-allowed-{r}" for r in ("getprocs", "getprocs2", "telemetry", "meminfo", "vfs-write")],
    "control-file-created",
    "resize-accepted",
    "frontend-controller-session", "frontend-rm-interactive", "frontend-top-quit",
    "frontend-release-detached", "frontend-reattach-redraw", "frontend-reattach-cwd-preserved",
    "frontend-reattach-history-preserved", "frontend-old-attachment-stale",
    "frontend-resize-after-reattach",
    # Phase 10 S10-0 §2 S10-0.8: `/bin/cellbox <command>` and the in-shell explicit form.
    "oneshot-echo-exact", "oneshot-argv-literal", "oneshot-ls-fixture", "oneshot-cat-stdin-bytes",
    "oneshot-grep-status", "oneshot-records-jsonl", "oneshot-records-text", "oneshot-rg-records",
    "oneshot-top-batch", "oneshot-tui-refused", "oneshot-state-refused", "oneshot-launch-refused",
    "oneshot-stage-refused", "oneshot-unknown-127", "oneshot-mutate-effect",
    "oneshot-denials-same-as-shell", "oneshot-stderr-separate", "oneshot-missing-stdout",
    "oneshot-consumer-close", "oneshot-cancel", "oneshot-frontend-argv-refused",
    "shell-explicit-prefix",
    "oneshot-no-tui-bytes", "top-pipe-batch",
    # Phase 06 S6-4 §7.2: interactive Ratatui `top` read through the driver's grid model.
    "top-subst-refused",
    "top-tui-frame", "top-tui-refresh", "top-tui-drill", "top-tui-keys", "top-tui-no-lifecycle",
    "top-tui-resize", "top-tui-quit-restores", "top-tui-two-sessions", "top-tui-idle-cost",
    "top-tui-ctrl-c-130",
    # Phase 06 S6-5 §4: `top` against the /data/bin/top-load fixture (#1-#19) and the
    # provider's explicit truncation through /bin/s2tw-probe trunc (#20).
    "top-load-launch", "top-load-over-64-records", "top-load-over-64-text", "top-load-over-64-tui",
    "top-load-cpu-idle", "top-load-cpu-busy", "top-load-cpu-two-busy", "top-load-cpu-sum-bounded",
    "top-load-drill-live", "top-load-views-agree", "top-load-no-lifecycle",
    "top-load-exit-gone-tasks", "top-load-exit-retains-cpu", "top-load-quota-once",
    "top-load-relaunch", "top-load-id-reused", "top-load-gone-cell-empty",
    "top-load-tui-gone-notice", "top-load-teardown", "control-telemetry-truncated",
    "ps-reflects-new-shell",
    "two-hosts-independent-output", "two-hosts-independent-state",
    "two-hosts-independent-stderr",
    "two-hosts-independent-resize", "two-hosts-independent-exit", "two-hosts-close",
    "child-exit-then-close", "relaunch-after-exit",
    "repeat-open-close", "repeat-no-leak",
    "cancel-child",
    "session-a-close",
]
passes = {}
fails = []
for line in log.splitlines():
    m = re.search(r"P04 (PASS|FAIL) (\S+)(.*)$", line)
    if not m: continue
    if m.group(1) == "PASS": passes[m.group(2)] = passes.get(m.group(2), 0) + 1
    else: fails.append((m.group(2), m.group(3).strip()))
checks = {}
def chk(name, ok, why=""): checks[name] = (bool(ok), why)
chk("driver-started", "P04 START" in log)
for name in EXPECTED:
    chk("marker:" + name, passes.get(name) == 1 and not any(f[0] == name for f in fails),
        "missing" if name not in passes else ("duplicate" if passes[name] != 1 else ""))
# Reconstruct observed host bytes independently of driver PASS markers. Offset,
# length and duplicate checks are fail-closed; preserve the exact captures.
captures, declared, malformed = {}, {}, []
for line in log.splitlines():
    match = re.search(r"P04 BYTES (\S+) len=(\d+)$", line)
    if match:
        name, length = match[1], int(match[2])
        if name in declared: malformed.append("duplicate " + name)
        declared[name] = length
        captures.setdefault(name, bytearray())
    match = re.search(r"P04 HEX (\S+) offset=(\d+) ([0-9a-f]+)$", line)
    if match:
        name, offset = match[1], int(match[2])
        data = captures.setdefault(name, bytearray())
        if name not in declared or offset != len(data):
            malformed.append("bad offset/order " + name)
        try: data.extend(bytes.fromhex(match[3]))
        except ValueError: malformed.append("bad hex " + name)
for name, data in captures.items():
    if declared.get(name) != len(data): malformed.append("bad length " + name)
    if re.fullmatch(r"[a-z-]+\.(stdout|stderr)", name):
        (evidence / ("host-" + name + ".bin")).write_bytes(data)
chk("host-evidence-framing", not malformed, malformed)
def host(name): return bytes(captures[name]) if name in declared else None
TUI_ABANDON = b"\x1b[?1049h\x1b[?25l\x1b[?2004hp03-tui-abandon\n"
SOFT_RESET = b"\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m"
expected = {
    "echo.stdout": b"p04-ext two\n", "echo.stderr": b"",
    "ls-error.stdout": b"", "ls-error.stderr": b"ls: /p04-output-missing: no such directory\n",
    "cat.stdout": (evidence / "expected-raw.bin").read_bytes(), "cat.stderr": b"",
    "large-cat.stdout": (evidence / "expected-large.bin").read_bytes(), "large-cat.stderr": b"",
    "pressure-ls.stdout": (evidence / "expected-pressure-ls.bin").read_bytes(), "pressure-ls.stderr": b"",
    "host-a.stdout": b"p04-stock-from-a\n", "host-a.stderr": b"",
    "host-b.stdout": b"p04-stock-from-b\n", "host-b.stderr": b"",
    "host-a-error.stdout": b"", "host-a-error.stderr": b"ls: /p04-missing-host-a: no such directory\n",
    "host-b-error.stdout": b"", "host-b-error.stderr": b"ls: /p04-missing-host-b: no such directory\n",
    # S5-2: a child that left the alternate screen, hidden cursor and bracketed paste on is
    # followed by exactly one cellos-vt1 soft reset; a plain fixture mode by none.
    "tui-abandon.stdout": TUI_ABANDON + SOFT_RESET, "tui-abandon.stderr": b"",
    "tui-pipeline.stdout": TUI_ABANDON + SOFT_RESET, "tui-pipeline.stderr": b"",
    "plain-filter.stdout": b"{\"a\":1}\n{\"a\":2}\n", "plain-filter.stderr": b"",
    # S5-4a: the hostile fixture as relayed by the builtin cat (8-bit C1 bytes -> U+FFFD).
    "hostile-cat.stdout": (evidence / "expected-vt-hostile-relayed.bin").read_bytes(), "hostile-cat.stderr": b"",
    # S10-0: a one-shot's streams are exactly the command's bytes.
    "oneshot-echo.stdout": b"a b \n", "oneshot-echo.stderr": b"",
    "oneshot-stderr.stdout": b"", "oneshot-stderr.stderr": b"cat: /p04os-missing: No such file or directory\n",
}
for name in ("negative-missing-stdout", "negative-missing-stderr", "negative-missing-both", "negative-invalid-stdout"):
    for stream in ("stdout", "stderr"):
        expected[f"{name}.{stream}"] = b""
for name, data in expected.items():
    chk("host-exact:" + name, host(name) == data)
chk("host-exact:ls.stdout", host("ls.stdout") in (b".\n..\nalpha-entry\nbeta-entry\n", b".\n..\nbeta-entry\nalpha-entry\n"))
chk("host-exact:ls.stderr", host("ls.stderr") == b"")
free = host("free.stdout")
match = re.fullmatch(rb"              total        used        free\nMem \(KiB\):    (\d+)      (\d+)      (\d+)\n", free or b"")
chk("host-free-accounting", match is not None and int(match[1]) > 0 and int(match[1]) == int(match[2]) + int(match[3]))
chk("host-free-stderr-empty", host("free.stderr") == b"")
for name in ("tui-abandon", "tui-pipeline"):
    chk("host-soft-reset-once:" + name, (host(name + ".stdout") or b"").count(SOFT_RESET) == 1)

# Independent NEGATIVE UART assertions replace the old UART success oracle.
# Driver evidence is hex, and INFO diagnostics are prefixed P04, not command data.
# A `USER+ ` line is the rest of the Cell line a kernel log line interrupted (kernel
# `ConsoleLine` contract): join it onto that `USER: ` line so a payload split by a
# kernel line is still one line to the assertions below.
uart, last_cell = [], None
for line in log.splitlines():
    line = line.rstrip("\r")
    if line.startswith("USER+ ") and last_cell is not None:
        uart[last_cell] += line[len("USER+ "):]
        continue
    if line.startswith(("USER: ", "USER+ ")):
        last_cell, line = len(uart), "USER: " + line[len("USER+ "):]
    uart.append(line)
for name, payload in {
    "echo": "p04-ext two", "status-probe": "fine", "ls-alpha": "alpha-entry",
    "ls-beta": "beta-entry",
    "partial-write-ls": "p04-pressure-",
    "host-a": "p04-stock-from-a", "host-b": "p04-stock-from-b",
    "ls-error": "ls: /p04-output-missing: no such directory",
    "host-a-error": "ls: /p04-missing-host-a: no such directory",
    "host-b-error": "ls: /p04-missing-host-b: no such directory",
    "legacy-s3-ls-error": "ls: /p04-no-such-dir: no such directory",
    "after-close": "p04-after-stock-close",
}.items():
    chk("no-command-uart:" + name, not any(line.startswith("USER: " + payload) for line in uart))
# Every 512-byte chunk, including the final short chunk, has a unique canary.
# Check raw UART bytes anywhere, independently of USER framing and driver markers.
raw_serial = (evidence / "serial.raw.log").read_bytes()
for index in range((96 * 1024 + 64 + 511) // 512):
    sentinel = f"P04-CHUNK-{index:04}-NO-UART!".encode()
    chk(f"no-binary-chunk-uart:{index}", sentinel not in raw_serial)
chk("no-negative-payload-uart", b"p04-forbidden-" not in raw_serial)
# S10-0: no one-shot payload reaches the serial line (raw bytes, any framing).
chk("oneshot-not-on-uart", b"p04os-" not in raw_serial)
# The kernel stops a producer whose final consumer closed (stop reason 3; a shell reports 141).
consumer = re.findall(r"P04 INFO oneshot-consumer-close lines=\d+ reported=Some\(141\) exit=Err\(3\) .* ms=(\d+)", log)
chk("oneshot-consumer-close-bounded", len(consumer) == 1 and int(consumer[0]) < 20_000, consumer)
chk("no-tui-bytes-uart", b"p03-tui-abandon" not in raw_serial and SOFT_RESET not in raw_serial
    and b"\x1b[?1049h" not in raw_serial)
chk("invalid-ref-admission-not-helper", len(re.findall(
    r"P04 INFO negative-invalid-ref admission=\S+ helper-executed=false", log)) == 1)
for name in ("negative-missing-stdout", "negative-missing-stderr", "negative-missing-both"):
    exits = re.findall(r"P04 INFO negative-child name=" + name + r" exit=Ok\((-?\d+)\) published=true", log)
    chk("stock-negative-published-nonzero:" + name, len(exits) == 1 and int(exits[0]) != 0, exits)
for name in ("stock-close", "stock-cancel"):
    elapsed = re.findall(r"P04 INFO " + name + r" elapsed_ms=(\d+)", log)
    chk("stock-stop-before-write-deadline:" + name, len(elapsed) == 1 and int(elapsed[0]) < 15_000, elapsed)
sink_ms = re.findall(r"P04 INFO endless-local-sink elapsed_ms=(\d+)", log)
chk("endless-local-sink-ctrl-c-prompt-bounded", len(sink_ms) == 1 and int(sink_ms[0]) < 15_000, sink_ms)
# S7-4: the fixture volume really is the NVMe FAT32 P1 (never a silent fallback), and the
# whole-file search time over the 7 MiB file is recorded (no speed claim).
chk("rg-fixture-fat32-mounted", "[vfs] FAT32 /mnt/sd volume mounted" in log)
# The seeded P5 /srv volume (denied `cellos` child) is a real CellosFS mount, not a gap.
chk("rg-fixture-srv-cellosfs-mounted", "[vfs] CellosFS Native mounted at existing volume" in log)
big_ms = re.findall(r"P04 INFO rg-fx-big elapsed_ms=(\d+) bytes=(\d+)", log)
chk("rg-fx-big-elapsed-recorded", len(big_ms) == 1, big_ms)
# A streamed local sink never exhausts the cell heap: no allocator-failure text on the UART
# (raw bytes and framed log lines alike), independent of the driver's own OOM marker.
# `OOM` is matched case-sensitively so the marker name `endless-local-sink-no-oom` is not a hit.
oom_hits = [l for l in log.splitlines() if re.search(r"(?-i:\bOOM\b)|heap exhausted", l, re.I)]
chk("no-oom-heap-exhausted-uart", not oom_hits and not re.search(rb"(?-i:\bOOM\b)|heap exhausted", raw_serial, re.I), oom_hits[:3])
chk("secret-token-not-on-uart", b"p04-kms-namespace-fixture-token" not in raw_serial)
# S5-4a: the token typed into `read -s` never reaches the UART: not as text, not as the
# driver's hex evidence (it is in no captured stream), not in any INFO/FAIL line.
READ_TOKEN = b"p04-read-s-fixture-token-Q7v2Lx"
chk("read-secret-token-not-on-uart", READ_TOKEN not in raw_serial and READ_TOKEN.hex().encode() not in raw_serial
    and READ_TOKEN.decode() not in log)
# The hostile output goes only to the driver's host stream (hex evidence), never the UART.
hostile_text = [b"hostile-begin", b"osc52-hidden", b"p04-title", b"cDA0LWNsaXA=", b"dcs-tail", b"hostile-end"]
chk("hostile-output-not-on-uart", not any(t in raw_serial for t in hostile_text),
    [t for t in hostile_text if t in raw_serial])
legacy_rows = [line for line in uart if re.fullmatch(r"USER: Mem \(KiB\):    \d+      \d+      \d+", line)]
chk("legacy-no-context-free-uart-once", len(legacy_rows) == 1, legacy_rows)
chk("legacy-no-context-free-header-once", uart.count("USER:               total        used        free") == 1)
chk("unchanged-vfs-log-uart", "USER: VFS Service v0.2: RamFS + mkdir/rmdir/unlink IPC (typed postcard)" in uart)
chk("unchanged-input-log-uart", "USER: [input] Input Service v0.3: US QWERTY + VirtIO + typed focus routing" in uart)
# S2-N. curl reports through the legacy Log path, so its own text on the UART, inside the
# driver's begin/end frame, is the independent admission verdict: past Resolve and
# TcpConnect (`empty response`), never a refusal and never a response.
begins = [i for i, l in enumerate(uart) if "P04 INFO net-curl-begin" in l]
ends = [i for i, l in enumerate(uart) if "P04 INFO net-curl-end" in l]
frame = uart[begins[0] + 1:ends[0]] if len(begins) == 1 and len(ends) == 1 and begins[0] < ends[0] else None
chk("net-curl-frame", frame is not None, (begins, ends))
frame = frame or []
chk("net-curl-network-failure-uart",
    sum("curl: empty response" in l for l in frame) == 1, [l for l in frame if "curl" in l][:5])
refusals = ("cannot resolve host", "connect failed", "TcpConnect syscall failed", "no net service", "invalid URL")
chk("net-curl-not-refused-uart", not any(r in l for l in frame for r in refusals),
    [l for l in frame if any(r in l for r in refusals)][:3])
# Restricted SLIRP never answers: no HTTP status line anywhere on the UART.
chk("net-no-response-uart", not any("HTTP/" in l for l in uart), [l for l in uart if "HTTP/" in l][:3])
# The probe writes only to its delegated stdout (the driver's host stream), never the UART.
chk("net-probe-not-on-uart", b"NETPROBE" not in raw_serial)
# Phase 01 CP1 (Q): the config probe also writes only to its delegated stdout; a
# driver line may quote it only behind the P04 prefix (a FAIL diagnostic).
chk("config-probe-not-on-uart", not any("CFGPROBE" in l and "P04 " not in l for l in uart),
    [l for l in uart if "CFGPROBE" in l and "P04 " not in l][:3])
# S2-T/S2-W: the probe writes only to its delegated stdout; driver lines carry the P04 prefix.
chk("s2tw-probe-not-on-uart", not any("S2TW" in l and "P04 " not in l for l in uart),
    [l for l in uart if "S2TW" in l and "P04 " not in l][:3])
qemu_argv = (evidence / "qemu-argv.txt").read_text().split()
chk("qemu-restricted-slirp-no-forward",
    [qemu_argv[i + 1] for i, a in enumerate(qemu_argv) if a == "-netdev"] == ["user,id=net0,restrict=on"]
    and "e1000,netdev=net0" in qemu_argv and "-nic" not in qemu_argv
    and not any("hostfwd" in a or "guestfwd" in a for a in qemu_argv), " ".join(qemu_argv))
chk("no-unexpected-markers", set(passes) <= set(EXPECTED), sorted(set(passes) - set(EXPECTED)))
chk("no-FAIL-markers", not fails, fails)
chk("driver-final-verdict", "P04-TERMINAL: PASS" in log and "P04-TERMINAL: FAIL" not in log)
# The one intended refusal is the driver's unknown-program probe (not a reviewed target).
denies = [l for l in log.splitlines() if "DENY" in l and "/bin/p04-no-such-program" not in l]
chk("no-unexpected-launch-deny", not denies, denies[:3])
chk("unknown-program-denied", any("DENY launch edge" in l and "target=/bin/p04-no-such-program" in l for l in log.splitlines()))
chk("no-panic", not re.search(r"panic|PANIC|page fault|PAGE FAULT", log), [l for l in log.splitlines() if re.search(r"panic|PANIC|page fault", l)][:3])
# S5-3 option C: the released shell observed Lost(4) (at least once: a cancelled shell may
# also see its frontend lost before it ends) before the one re-attach notice.
detached = [i for i, l in enumerate(uart) if "shell: frontend detached; session waits" in l]
reattached = [i for i, l in enumerate(uart) if "shell: frontend re-attached; redraw" in l]
chk("frontend-detach-observed-uart", detached and reattached and detached[0] < reattached[0], (detached[:3], reattached))
chk("frontend-reattach-once-uart", len(reattached) == 1, reattached)
chk("no-launch-context-refused-uart", not any("shell: launch context refused" in l for l in uart))
# The shells talk to the driver through relayed endpoints: nothing of theirs reaches the UART.
chk("no-shell-prompt-on-uart", "Cellos >" not in log and "Cellos shell ready" not in log)
chk("no-key-material-in-log", seed not in log)
# D65-9: echo the measured values behind `top-tui-idle-cost` and every S6-5 check into this
# runner's own output (joined across `USER+ ` continuations; not checks themselves).
for line in uart:
    if re.search(r"P04 INFO (top-load |top-tui-idle-cost )", line):
        print(line[line.index("P04 INFO"):])
for name, (ok, why) in checks.items():
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else "  <- " + str(why)))
(evidence / "verdict.json").write_text(json.dumps({
    "checks": {name: {"pass": ok, "detail": why} for name, (ok, why) in checks.items()},
    "scope": "private S3-output stock runtime gates; GUI and independent Phase02/03 regressions remain separate",
    "coverage_notes": (evidence / "acceptance-limits.txt").read_text(),
    # S10-3.3 import mode only (the evidence dir is recreated each run).
    **({"candidate_import": json.loads((evidence / "candidate-provenance.json").read_text())}
       if (evidence / "candidate-provenance.json").exists() else {}),
}, indent=2) + "\n")
bad = [n for n, (ok, _) in checks.items() if not ok]
if bad:
    raise SystemExit("FAIL: %d check(s) failed: %s" % (len(bad), ", ".join(bad)))
print("PASS: %d Phase04 S3-output runtime checks (%d driver markers); GUI/regression evidence remains separate" % (len(checks), len(EXPECTED)))
PY
