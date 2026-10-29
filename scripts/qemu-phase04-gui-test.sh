#!/usr/bin/env bash
# Private, fail-closed Phase04 slice S3-gui build/sign/assemble/boot/judge runner.
#
# README (what this proves, and what it does not)
# -----------------------------------------------
# Scope (phase-04-s3-gui-proof-proposal.md, GUI-1..GUI-5): builds a *private* x86_64
# desktop image with ITS OWN signed artifact set whose kernel trusts a freshly generated
# throw-away cell signing key, boots it in one owned QEMU (networking disabled, display
# none, owned unix QMP + serial sockets in a private directory), and judges the GUI
# Terminal on real compositor scan-out frames. Nothing canonical is read for writing or
# changed: not target/x86_64-unknown-none/release/{cellbox,terminal}, not
# kernel/src/embedded-x86_64/kernel_fs.img (hashes recorded before and after), not any
# other VM. scripts/package-desktop-x86.py and the guards of scripts/x86/make-iso-ci.sh
# are neither used nor edited (the ISO helper is called only with an isolated kernel and
# ISO root, exactly like the S3 runner). All outputs are invalidated before building, so a
# failed rebuild can never be booted.
#
# Image: compositor, input, vfs, config, platform, fb-console, desktop, terminal, files,
# cellbox, the stock tools ls/cat/echo, the private /bin/p03-filter, plus fixtures (/gui-list/*, /gui-text/note.txt,
# /p04-output/vt-*.bin). Boot
# config comes from the supported generator (features input,ui), then a private copy of
# autoload.toml drops the serial `cellbox sh` (a serial shell would both draw a second UI
# shell on the compositor and put a prompt on the UART that this proof needs to be
# absent). Repository templates are untouched.
#
# Launch of /bin/terminal (GUI_LAUNCH, default click):
#   click    the real path: QMP absolute pointer events click the first pinned Taskbar
#            app (Terminal) on the desktop UI; Desktop -> sys_spawn_from_path(/bin/terminal).
#   autoload private autoload row `terminal` through the existing init -> /bin/terminal edge
#            (no new launch-profile row, no kernel change). Fallback if the click path is
#            unreliable; then the "no window" control is the end-of-run Esc close.
#
# Focus race: keys are injected only after the serial marker "[terminal] Terminal surface
# ready", after the prompt is pixel-matched in the window, and after a real pointer click
# inside the window (the Terminal re-requests focus on a left click).
#
# C5 (phase-04-c5-desktop-parity-proposal.md §3 steps 1-6), between the stderr and Ctrl-C
# scenarios: the GUI Terminal (`cellbox sh`) creates /tmp/c5dir/a.txt; Files, launched by a
# real taskbar click, lists it, creates c5new, copies a.txt into it, deletes the copy and
# then the folder, and shows the VFS refusal for /srv/cellos; after each Files change the
# Terminal's `ls`/`cat` show the same VFS state. Spotlight finds nothing for "shutdown" or
# "reboot" and its System section has no power rows. Windows are switched the way a user
# does: the Terminal title tab raises it, a click in its content gives it the keyboard; a
# click on the Files status bar raises Files. Files and Spotlight screens are judged
# exactly (0 differing pixels) on the Files path label, list pane and status bar and on
# Spotlight's results area, each with a one-character-changed and a before-frame control.
#
# S5-2 (phase-05-s2-vt-screen-proposal.md, D52-3), between C5 and the Ctrl-C scenario: the
# GUI Terminal's `cellos-vt1` screen, judged on styled exact screens (glyph helper `styled`
# mode: colours, bold, inverse, two-pass background/glyph render). `cat` of the host-built
# /p04-output/vt-alt-enter.bin leaves the alternate screen up (exact alt screen with the
# prompt on it), /p04-output/vt-alt-exit.bin returns to the untouched primary transcript
# screen plus the new prompt, /p04-output/vt-sample.bin draws the colour/attribute/cursor
# addressing/erase/alternate-screen sample, and `clear` returns to a plain screen.
#
# Oracle: scripts/phase04_gui_drive.py. Expected pixels are rendered by the host helper
# scripts/phase04-gui-glyphs (links libs/ostd and calls the very TextFonts/GlyphAtlas the
# cells call: the Terminal grid in JetBrains Mono Regular 14 px, 9x18 cells, same
# clip/rounding, 79x26 cells at 720x480 = the whole cells inside the 8/6 px padding as
# `terminal::grid_dims` reports, so every expected screen is authored at 79 columns;
# Files/Spotlight by replaying viui's fill/stroke/label/button primitives at
# the guest's surface coordinates), so the comparison is exact (0 differing pixels outside
# the cursor cell); no tolerance and no separate rasterizer. Every positive match has a
# one-character-changed control and a "before" frame control; the UART negative check
# reuses the S3 method (command payloads must never appear on the physical UART).
#
# Signing: cellbox and terminal need CELLBOX_PRIVATE_FIXTURE_SIGNING=1 with the private
# seed (scripts/cellos_sign guard); it is set only on that one signing call.
#
# S5-3 option C (phase-05-s3c-frontend-annex.md §7, decision A), between S5-2 and Ctrl-C: the
# shell controls its own GUI-kind session and the Terminal is only its frontend. A click on the
# Terminal's close control inside the live Terminal Cell releases the frontend (UART: the
# Terminal's release line and the shell's own `frontend detached` line = it observed Lost(4));
# the Terminal re-creates its window AT ONCE by itself and re-attaches the same shell (exact
# same transcript screen, then `pwd` still /tmp and history Up recalls the last line).
# NOT delivered: a user-facing re-open of a running Terminal (desktop single-instance /
# taskbar to a running Cell) and the Terminal-Cell-kill path (no kill route on this image).
#
# S5-4 hostile output (phase-05-s4-proposal.md §2 S5-4a item 3), after the re-attach scenario: from
# a cleared screen, `history -c`, then `cat /p04-output/vt-hostile.bin` (built by
# scripts/phase05_vt_hostile.py: OSC 52 clipboard set/query, OSC 8 link, OSC 0/2 titles, DCS/
# APC/PM strings over 4 KiB, DA/DSR/DECRQM queries, 8-bit C1 CSI/OSC, a 17-parameter CSI,
# embedded ESC[201~/ESC[200~) shows exactly the script's VISIBLE rows in default colours
# plus the next prompt (controls: one character changed; the 17-parameter row bold red). A
# typed `echo` then shows exactly the typed line and its output, and `history` lists exactly
# the cat, the echo and itself: no reply byte reached the shell's input. The UART never
# carries the clipboard, link or title payloads. NOT proven: a clipboard or title surface
# exists on this image at all (neither does; the proof is that nothing is replied or shown).
#
# NOT proven: release/canonical packaging, physical Intel or VMware display, window
# management beyond the Terminal/Files raise-and-focus sequence, resize/tabs; this is
# private fixture evidence, not acceptance.
#
# Usage: CELLBOX_PHASE04_WORK=/tmp/cellbox-phase04-gui-XXXX bash scripts/qemu-phase04-gui-test.sh [--candidate <candidate.json>]
# Optional: PHASE04_ACCEL=tcg|kvm (default tcg), PHASE04_BOOT_WINDOW (seconds, default 900),
#           GUI_LAUNCH=click|autoload, PHASE04_GUI_EVIDENCE_OUT=<dir> (copy sanitized evidence).
#
# S10-3.3 import mode (D3-1 A, opt-in): `--candidate` takes the shared cells and init from the
# candidate's desktop profile instead of building them (scripts/phase10_candidate_import.py),
# re-signs the copies with this run's key in the signing calls below and requires their
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

W="${CELLBOX_PHASE04_WORK:?set CELLBOX_PHASE04_WORK to the owned /tmp/cellbox-phase04-gui-* directory}"
case "$W" in
    /tmp/cellbox-phase04-gui-*) ;;
    *) echo "FAIL: work directory must be /tmp/cellbox-phase04-gui-*" >&2; exit 1 ;;
esac
[[ -d "$W" ]] || { echo "FAIL: work directory missing: $W" >&2; exit 1; }
ACCEL="${PHASE04_ACCEL:-tcg}"
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: PHASE04_ACCEL must be tcg or kvm" >&2; exit 1 ;; esac
WINDOW="${PHASE04_BOOT_WINDOW:-900}"
LAUNCH="${GUI_LAUNCH:-click}"
case "$LAUNCH" in click|autoload) ;; *) echo "FAIL: GUI_LAUNCH must be click or autoload" >&2; exit 1 ;; esac
# phase02-terminal-profile: the private signing-key path only (kernel/build.rs).
# phase03-filter-profile: the existing private `cellbox -> /bin/p03-filter` row (empty
# ceiling), used only as the endless external consumer of the Ctrl-C scenario, exactly as
# in the Phase03/S3 runners. No new launch-profile row is added by this runner. The S5-3/S5-4
# proof-consumer screens (and the `phase04-driver-profile` row they needed) were removed in
# S5-4e (phase-05-s4-proposal.md D54-4); their runs remain archived evidence only.
FEATURES="phase02-terminal-profile,phase03-filter-profile"
PYTHON_BIN="${PYTHON_BIN:-python3}"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
HOST_TARGET_DIR="$W/host-target"
GLYPH_DIR="$W/glyph-host"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
PRIVATE_CONFIG="$W/private-config"
SCRATCH="$W/scratch"
SOCKDIR="$W/sock"
ISO="$W/phase04-gui.iso"
REL="$CELLS_TARGET_DIR/$TARGET/release"

# Refuse any alias of the quarantined/canonical artifacts.
for guarded in "$ROOT/kernel/src/embedded-x86_64" "$ROOT/target/x86_64-unknown-none/release" "$ROOT/build"; do
    [[ "$(realpath -m "$W")" != "$(realpath -m "$guarded")"* ]] || { echo "FAIL: work dir aliases $guarded" >&2; exit 1; }
done
"$PYTHON_BIN" - "$W" <<'PY'
import os, pathlib, sys
w = pathlib.Path(sys.argv[1])
if w.is_symlink() or w.resolve().parent != pathlib.Path("/tmp") or not w.name.startswith("cellbox-phase04-gui-"):
    raise SystemExit("FAIL: work directory must be a real direct /tmp/cellbox-phase04-gui-* directory")
if w.stat().st_uid != os.getuid():
    raise SystemExit("FAIL: work directory is not owned by this uid")
for name in ("cells-target", "kernel-target", "host-target", "glyph-host", "embedded", "keys", "evidence",
             "private-config", "scratch", "iso-root", "sock"):
    path = w / name
    if path.is_symlink() or not path.resolve().is_relative_to(w.resolve()):
        raise SystemExit("FAIL: private output root aliases another directory: " + name)
    if path.exists() and any(p.is_symlink() for p in path.rglob("*")):
        raise SystemExit("FAIL: private output tree contains a symlink: " + name)
PY

# Canonical/quarantined artifact hashes before anything runs (read-only).
CANON=(
    "$ROOT/target/x86_64-unknown-none/release/cellbox"
    "$ROOT/target/x86_64-unknown-none/release/terminal"
    "$ROOT/kernel/src/embedded-x86_64/kernel_fs.img"
)
canonical_hashes() { for f in "${CANON[@]}"; do [[ -e "$f" ]] && sha256sum "$f" || echo "ABSENT  $f"; done; }

# Invalidate every output first (stale artifacts must never be booted).
rm -rf "$EMBEDDED" "$EVIDENCE" "$ISO" "$W/iso-root" "$PRIVATE_CONFIG" "$SCRATCH" "$SOCKDIR" "$GLYPH_DIR" \
    "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
mkdir -p "$EMBEDDED" "$KEYS" "$EVIDENCE" "$PRIVATE_CONFIG" "$SCRATCH" "$SOCKDIR" "$GLYPH_DIR"
chmod 700 "$KEYS" "$SOCKDIR"
canonical_hashes | tee "$EVIDENCE/canonical-before.txt"

echo "==> Repository checks (no build): fake Terminal matcher, fake Files model and power actions gone, cellbox ceiling unchanged"
"$PYTHON_BIN" - "$ROOT" "$EVIDENCE" <<'PY'
import hashlib, pathlib, sys
root, evidence = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
terminal = root / "cells/apps/terminal"
sources = sorted(p for p in terminal.rglob("*") if p.is_file() and p.suffix in (".rs", ".toml"))
if not sources:
    raise SystemExit("FAIL: no sources found under cells/apps/terminal")
banned = [
    "CellOS GUI Terminal (x86_64)", "Available commands:", "CellOS 0.2.1 x86_64 Intel VT-x C2C",
    "Tue Oct 20 06:00:00 UTC 2026", "bin/  apps/  data/  etc/  welcome.md", "PID  NAME         STATUS",
    "running (active)", "[terminal] Session terminated.",
]
banned_in_session = ["fn execute_command", "split_whitespace", '"uname" =>', '"whoami" =>']
found = []
for path in sources:
    text = path.read_text(encoding="utf-8", errors="replace")
    found += [f"{path.relative_to(root)}: {n!r}" for n in banned if n in text]
    if path.name == "session.rs":
        found += [f"{path.relative_to(root)}: {n!r}" for n in banned_in_session if n in text]
if found:
    raise SystemExit("FAIL: the former fake Terminal matcher/fixed responses are still present:\n  " + "\n  ".join(found))
if not any("FrontendHost" in p.read_text(errors="replace") for p in sources if p.suffix == ".rs"):
    raise SystemExit("FAIL: the GUI Terminal sources do not use the SDK FrontendHost")
# C5: Files reaches the VFS service (no sample listing), Desktop has no fake power action
# and no dead launcher entry.
def texts(sub):
    files = sorted(p for p in (root / sub).rglob("*.rs") if "target" not in p.parts)
    if not files:
        raise SystemExit("FAIL: no sources found under " + sub)
    return {p: p.read_text(encoding="utf-8", errors="replace") for p in files}
c5_banned = {
    "cells/apps/files": ['"/bin/fb-console"', '"/data/sample.pdf"', "via associated application",
                         "This is a preview of the markdown content", "sample_function"],
    "cells/apps/desktop": ["/bin/app-sys-tools", "/bin/service-httpd", "/bin/app-c-math-smoke",
                           "SpotlightAction::Reboot", "SpotlightAction::ShutDown", "Authorized system"],
}
c5_found = [f"{p.relative_to(root)}: {n!r}" for sub, names in c5_banned.items()
            for p, text in texts(sub).items() for n in names if n in text]
if c5_found:
    raise SystemExit("FAIL: fabricated Files/Desktop behaviour is still present:\n  " + "\n  ".join(c5_found))
files_text = "\n".join(texts("cells/apps/files").values())
for need in ("VfsClient", "VfsMutate"):
    if need not in files_text:
        raise SystemExit("FAIL: Files sources do not use " + need)
# Source digests of the product code under test (this proof edits none of it).
digests = {}
for sub in ("cells/apps/terminal", "cells/services/compositor", "cells/services/input", "cells/apps/desktop",
            "cells/apps/files", "cells/tools/cellbox", "libs/ostd/src"):
    h = hashlib.sha256()
    for p in sorted(q for q in (root / sub).rglob("*") if q.is_file() and "target" not in q.parts):
        h.update(str(p.relative_to(root)).encode() + b"\0" + p.read_bytes())
    digests[sub] = h.hexdigest()
(evidence / "product-source-digests.json").write_text(__import__("json").dumps(digests, indent=2) + "\n")
print("fake matcher check: none of", len(banned) + len(banned_in_session), "banned strings in", len(sources), "Terminal files")
PY
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py | tee "$EVIDENCE/cellbox-ceiling.txt"
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py cells/apps/terminal/src/main.rs \
    --baseline-ref HEAD --baseline-path cells/apps/terminal/src/main.rs \
    > "$EVIDENCE/terminal-ceiling-delta.txt" 2>&1 || true
echo "terminal ceiling delta (informational):"; sed 's/^/    /' "$EVIDENCE/terminal-ceiling-delta.txt"
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py cells/apps/files/src/main.rs \
    --baseline-ref HEAD --baseline-path cells/apps/files/src/main.rs \
    > "$EVIDENCE/files-ceiling-delta.txt" 2>&1 || true
echo "files ceiling delta (informational; C5 adds VfsMutate):"; sed 's/^/    /' "$EVIDENCE/files-ceiling-delta.txt"

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
fd = os.open(keys / "seed.hex", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(seed.hex())
(keys / "cell.pub").write_bytes(pub)
print("key identity sha256:", hashlib.sha256(pub).hexdigest())
PY

echo "==> Building the host glyph renderer (libs/ostd TextFonts; private target dir, outside the repo tree)"
sed "s#@ROOT@#$ROOT#g" scripts/phase04-gui-glyphs/Cargo.toml.in > "$GLYPH_DIR/Cargo.toml"
mkdir -p "$GLYPH_DIR/src"
cp scripts/phase04-gui-glyphs/src/main.rs "$GLYPH_DIR/src/main.rs"
cp Cargo.lock "$GLYPH_DIR/Cargo.lock"   # same dependency versions as the cells; cargo prunes unused entries
CARGO_TARGET_DIR="$HOST_TARGET_DIR" cargo build --release --offline --target x86_64-unknown-linux-gnu \
    --manifest-path "$GLYPH_DIR/Cargo.toml"
GLYPH_BIN="$HOST_TARGET_DIR/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs"
[[ -x "$GLYPH_BIN" ]] || { echo "FAIL: glyph renderer not built" >&2; exit 1; }

# x86_64 CELLS use relocation-model=pic (the kernel profile keeps static).
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building cells (private target dir)"
# Binaries signed by an earlier run in this private dir are removed so each run signs fresh builds.
rm -f "$REL"/{app-init,cellbox,terminal,service-vfs,service-config,service-input,platform,service-compositor,fb-console,desktop,files,ls,cat,echo,phase03-filter}
if [[ -z "$CANDIDATE" ]]; then
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
        -p service-compositor -p desktop -p files -p fb-console -p terminal -p app-sys-tools -p app-phase03-filter
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-init --features input,ui
else
    # Import mode: only the lane-only cell is built.
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-phase03-filter
fi

# Plain cells are signed without the fixture variable; only cellbox and terminal need it.
PLAIN_BINARIES=(
    "$REL/service-vfs" "$REL/service-config" "$REL/service-input" "$REL/platform"
    "$REL/service-compositor" "$REL/fb-console" "$REL/desktop" "$REL/files"
    "$REL/ls" "$REL/cat" "$REL/echo" "$REL/phase03-filter"
)
GUARDED_BINARIES=( "$REL/cellbox" "$REL/terminal" )
CELL_IMAGE_PATHS=(
    /bin/vfs /bin/config /bin/input /bin/platform
    /bin/compositor /bin/fb-console /bin/desktop /bin/files
    /bin/ls /bin/cat /bin/echo /bin/p03-filter
    /bin/cellbox /bin/terminal
)
CELL_BINARIES=( "${PLAIN_BINARIES[@]}" "${GUARDED_BINARIES[@]}" )
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Importing the shared cells from the candidate (desktop profile)"
    IMPORT_ARGS=()
    for i in "${!CELL_BINARIES[@]}"; do IMPORT_ARGS+=(--cell "${CELL_IMAGE_PATHS[$i]}=${CELL_BINARIES[$i]}"); done
    "$PYTHON_BIN" scripts/phase10_candidate_import.py import --candidate "$CANDIDATE" --profile desktop \
        --lane phase04-gui --release-dir "$REL" --evidence "$EVIDENCE" --init "$REL/app-init" "${IMPORT_ARGS[@]}"
fi
FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
    # A private build must never alias a canonical/quarantined artifact.
    case "$(realpath -m "${CELL_BINARIES[$i]}")" in
        "$W"/*) ;;
        *) echo "FAIL: cell binary outside the private target dir: ${CELL_BINARIES[$i]}" >&2; exit 1 ;;
    esac
done

# Fixtures live only in this private FAT image. The expected bytes are fixed here and
# independent of the guest; the drive script's screen model uses the same literals.
printf 'private known directory fixture\n' > "$SCRATCH/alpha-entry"
printf 'private known directory fixture\n' > "$SCRATCH/beta-entry"
printf 'gui-cat-line-one\ngui-cat-line-two\n' > "$SCRATCH/note.txt"
# Phase 05 S5-2 `cellos-vt1` byte fixtures (cat'd by the builtin `cat` in the GUI Terminal;
# phase04_gui_drive.py authors the expected styled screens from the emulator contract).
# vt-sample.bin: ED 2, ED 1, ED 0, EL 0/1/2 (EL 1 with a current background), rows drawn out
# of order by CUP, SGR 16-colour fg/bg, bold, inverse, 38;5 / 48;5 indexed, 38;2 / 48;2 RGB,
# SGR 0, ?25 hidden while drawing, an alternate screen whose content must vanish on exit;
# it ends on the primary screen with the cursor visible, SGR reset and a final newline.
# Every colour keeps G-R <= 5 so the cyan cursor detector (G-R > 60) never sees content.
{
    printf '\033[0m\033[?25l\033[2J'
    printf '\033[1;1HJUNK-ROW-0\033[2;1HJUNK00ed1-kept\033[2;6H\033[1J'
    printf '\033[7;1H\033[7minverse\033[0m \033[7;31minv-red\033[0m \033[1;7;33mbold-inv\033[0m'
    printf '\033[3;1H\033[31mred\033[0m \033[1;33mbold-yellow\033[0m \033[97;41mwhite-on-red\033[0m'
    printf '\033[6;1H\033[38;2;250;200;100mrgb-fg\033[0m \033[48;2;60;30;90mrgb-bg\033[0m'
    printf '\033[5;1H\033[38;5;208midx208\033[39m \033[48;5;53mon53\033[0m'
    printf '\033[4;1H\033[30;103m black-on-yellow \033[0m \033[95mmagenta95\033[0m'
    printf '\033[9;1HXXXXXXXXXXkeep-right\033[9;10H\033[43m\033[1K\033[0m'
    printf '\033[10;1Herase-tail:YYYYYYYY\033[10;12H\033[K'
    printf '\033[11;1HZZZZZZZZZZZZ\033[2K\033[11;3H2K'
    printf '\033[13;1Hed0-junk-will-vanish\033[14;1Hmore-junk\033[13;5H\033[J'
    printf '\033[?1049h\033[H\033[45mALT-SHOULD-VANISH\033[3;1HALT-SHOULD-VANISH\033[0m\033[?1049l'
    printf '\033[0m\033[1;1H\033[1mcellos-vt1 sample\033[0m'
    printf '\033[?25h\033[15;1Hend-of-sample\n'
} > "$SCRATCH/vt-sample.bin"
# Alternate-screen proof across two commands: enter (left up for a screenshot; builtin
# output is not relayed child output, so cellbox's soft reset does not apply) and exit.
printf '\033[?1049h\033[1;1H\033[1;97;45m ALT SCREEN \033[0m\033[3;5H\033[31malt-red\033[0m \033[7malt-inverse\033[0m\033[5;1H' \
    > "$SCRATCH/vt-alt-enter.bin"
printf '\033[?1049l\033[0m' > "$SCRATCH/vt-alt-exit.bin"
# Phase 05 S5-4 hostile output (OSC 52/8/0/2, >4 KiB DCS/APC/PM, device queries, 8-bit C1,
# 17-param CSI, embedded paste markers); bytes and expected rows live in one host script
# that the drive script imports too. Its 8-bit C1 bytes are not UTF-8 by design.
"$PYTHON_BIN" scripts/phase05_vt_hostile.py "$SCRATCH/vt-hostile.bin"
"$PYTHON_BIN" - "$SCRATCH" "$EVIDENCE/vt-fixtures.txt" <<'PY'
import hashlib, pathlib, re, sys
scratch, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
lines = []
for name in ("vt-sample.bin", "vt-alt-enter.bin", "vt-alt-exit.bin", "vt-hostile.bin"):
    data = (scratch / name).read_bytes()
    if name != "vt-hostile.bin":
        data.decode("ascii")  # the builtin cat path prints valid UTF-8; these fixtures are ASCII
    # The hostile fixture's >4 KiB string bodies are filler runs; record them by length.
    shown = re.sub(rb"Z{16,}", lambda m: b"<Z*%d>" % len(m.group()), data)
    lines.append(f"{name} {len(data)} bytes sha256={hashlib.sha256(data).hexdigest()}\n  {shown!r}")
out.write_text("\n".join(lines) + "\n")
PY
FAT_ARGS+=("$SCRATCH/alpha-entry" /gui-list/alpha-entry "$SCRATCH/beta-entry" /gui-list/beta-entry
    "$SCRATCH/note.txt" /gui-text/note.txt "$SCRATCH/vt-sample.bin" /p04-output/vt-sample.bin
    "$SCRATCH/vt-alt-enter.bin" /p04-output/vt-alt-enter.bin "$SCRATCH/vt-alt-exit.bin" /p04-output/vt-alt-exit.bin
    "$SCRATCH/vt-hostile.bin" /p04-output/vt-hostile.bin)

echo "==> Signing with the private key (policy check inside cellos-sign)"
SEED_HEX="$(cat "$KEYS/seed.hex")"
CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
    --seed-hex "$SEED_HEX" --sign "${PLAIN_BINARIES[@]}" "$REL/app-init"
CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet \
    --objcopy "${OBJCOPY:-objcopy}" --seed-hex "$SEED_HEX" --sign "${GUARDED_BINARIES[@]}"
unset SEED_HEX

echo "==> Assembling private kernel_fs.img (boot config: terminal launch=$LAUNCH, no serial shell)"
# Pass 1: the supported generator selects the default plan for this artifact set.
"$PYTHON_BIN" tools/mkfat32.py --config-features input,ui --config-output-dir "$EVIDENCE/boot-config-default" \
    "$SCRATCH/default-fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" - "$EVIDENCE/boot-config-default" "$PRIVATE_CONFIG" "$LAUNCH" <<'PY'
import re, sys, tomllib
from pathlib import Path
src, dst, launch = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
services = tomllib.loads((src / "services.toml").read_text())
names = {c["name"] for c in services.get("cells", [])}
for need in ("compositor", "input"):
    if need not in names:
        raise SystemExit("FAIL: generated services plan has no " + need)
text = (src / "autoload.toml").read_text()
head, *blocks = re.split(r"(?m)^\[\[cells\]\]\s*$", text)
out, dropped = [], 0
for block in blocks:
    cell = tomllib.loads("[[cells]]\n" + block)["cells"][0]
    if cell.get("path") == "/bin/cellbox":
        dropped += 1   # the serial shell: a second UI shell and a UART prompt this proof must not have
        continue
    out.append(block.strip("\n") + "\n")
if dropped != 1:
    raise SystemExit("FAIL: expected exactly one cellbox autoload entry to drop, found %d" % dropped)
private = head.rstrip("\n") + "\n" + "".join("\n[[cells]]\n" + b for b in out)
if launch == "autoload":
    private += '\n[[cells]]\nname = "terminal"\npath = "/bin/terminal"\nafter = ["compositor", "input"]\nrestart = "never"\n'
cells = tomllib.loads(private)["cells"]
paths = [c.get("path") for c in cells]
if "/bin/desktop" not in paths:
    raise SystemExit("FAIL: private autoload has no desktop")
if any(c.get("path") in ("/bin/cellbox", "/bin/shell") for c in cells):
    raise SystemExit("FAIL: private autoload still launches a shell")
if (launch == "autoload") != ("/bin/terminal" in paths):
    raise SystemExit("FAIL: terminal autoload row does not match GUI_LAUNCH=" + launch)
(dst / "autoload.toml").write_text(private)
(dst / "services.toml").write_bytes((src / "services.toml").read_bytes())
(dst / "system.toml").write_bytes((src / "system.toml").read_bytes())
print("private services plan:", sorted(names))
print("private autoload plan:", [(c["name"], c["path"]) for c in cells])
PY
rm -f "$SCRATCH/default-fs.img"
"$PYTHON_BIN" tools/mkfat32.py --config-features input,ui --config-dir "$PRIVATE_CONFIG" \
    --config-output-dir "$EVIDENCE/boot-config" "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
cmp -s "$EVIDENCE/boot-config/autoload.toml" "$PRIVATE_CONFIG/autoload.toml" \
    || { echo "FAIL: image autoload differs from the private plan" >&2; exit 1; }
[[ -s "$EMBEDDED/kernel_fs.img" ]] || { echo "FAIL: empty kernel_fs.img" >&2; exit 1; }
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/fat-layout.txt"
for path in "${CELL_IMAGE_PATHS[@]}"; do
    grep -Fq -- "-> LFN '${path#/bin/}'" "$EVIDENCE/fat-layout.txt" || { echo "FAIL: image misses $path" >&2; exit 1; }
done
for fixture in alpha-entry beta-entry note.txt vt-sample.bin vt-alt-enter.bin vt-alt-exit.bin vt-hostile.bin; do
    grep -Fq -- "'$fixture'" "$EVIDENCE/fat-layout.txt" || { echo "FAIL: image misses fixture $fixture" >&2; exit 1; }
done
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
"$PYTHON_BIN" - "$W" "$ROOT" "$LAUNCH" "$ACCEL" "$FEATURES" <<'PY'
import hashlib, json, subprocess, sys
from pathlib import Path
w, root, launch, accel, features = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
rel = w / "cells-target/x86_64-unknown-none/release"
names = ["app-init", "cellbox", "terminal", "service-vfs", "service-config", "service-input", "platform",
         "service-compositor", "fb-console", "desktop", "files", "ls", "cat", "echo", "phase03-filter"]
prov = {
    "status": "private fixture provenance; not acceptance",
    "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
    "rustc": subprocess.check_output(["rustc", "-Vv"], cwd=root, text=True),
    "cargo_lock_sha256": sha(root / "Cargo.lock"),
    "kernel_features": "default+" + features,
    "launch_mode": launch,
    "accel": accel,
    "signed_cells_sha256": {n: sha(rel / n) for n in names},
    "init_sha256": sha(w / "embedded/init"),
    "kernel_fs_img_sha256": sha(w / "embedded/kernel_fs.img"),
    "kernel_sha256": sha(w / "kernel-target/x86_64-unknown-none/release/cellos-kernel"),
    "iso_sha256": sha(w / "phase04-gui.iso"),
    "autoload_toml_sha256": sha(w / "evidence/boot-config/autoload.toml"),
    "services_toml_sha256": sha(w / "evidence/boot-config/services.toml"),
    "trusted_key_identity_sha256": sha(w / "keys/cell.pub"),
    "glyph_renderer_sha256": sha(w / "host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs"),
    "policy": "no /POLICY.BIN in the image; the kernel logs it absent and applies its development-permissive posture",
    "signature_verification": "cellos-sign re-verifies each ELF after signing; kernel verifies at spawn (signing-required)",
    "acceptance": "private GUI Terminal proof on a private desktop image; not release acceptance",
}
(w / "evidence/provenance.json").write_text(json.dumps(prov, indent=2) + "\n")
print(json.dumps({k: v for k, v in prov.items() if k.endswith("sha256")}, indent=1))
PY
if [[ -n "$CANDIDATE" ]]; then
    "$PYTHON_BIN" scripts/phase10_candidate_import.py annotate --evidence "$EVIDENCE" \
        --kernel-features "default+$FEATURES" --into "$EVIDENCE/provenance.json"
fi

echo "==> Booting private ISO and judging the GUI (owned QEMU only, networking disabled)"
set +e
"$PYTHON_BIN" scripts/phase04_gui_drive.py --work "$W" --iso "$ISO" --evidence "$EVIDENCE" \
    --glyph-bin "$GLYPH_BIN" --accel "$ACCEL" --seed-file "$KEYS/seed.hex" --launch "$LAUNCH" --window "$WINDOW"
DRIVE_STATUS=$?
set -e
if [[ -n "$CANDIDATE" && -f "$EVIDENCE/verdict.json" ]]; then
    "$PYTHON_BIN" scripts/phase10_candidate_import.py annotate --evidence "$EVIDENCE" --into "$EVIDENCE/verdict.json"
fi

echo "==> Canonical artifact hashes after the run (must equal the 'before' record)"
canonical_hashes > "$EVIDENCE/canonical-after.txt"
if cmp -s "$EVIDENCE/canonical-before.txt" "$EVIDENCE/canonical-after.txt"; then
    echo "PASS: canonical cellbox/terminal/kernel_fs.img hashes unchanged"
    cat "$EVIDENCE/canonical-after.txt"
else
    echo "FAIL: a canonical artifact changed during the run" >&2
    diff "$EVIDENCE/canonical-before.txt" "$EVIDENCE/canonical-after.txt" >&2 || true
    exit 1
fi

# Sanitized evidence: no keys, sockets or images ever reach the copy.
if [[ -n "${PHASE04_GUI_EVIDENCE_OUT:-}" ]]; then
    rm -rf "$PHASE04_GUI_EVIDENCE_OUT"
    mkdir -p "$PHASE04_GUI_EVIDENCE_OUT"
    cp -r "$EVIDENCE"/. "$PHASE04_GUI_EVIDENCE_OUT"/
    rm -rf "$PHASE04_GUI_EVIDENCE_OUT/boot-config-default"
    find "$PHASE04_GUI_EVIDENCE_OUT" \( -name '*.img' -o -name '*.iso' -o -name '*.sock' -o -name 'seed*' -o -name '*.pub' \) -delete
    if grep -rIl "$(cat "$KEYS/seed.hex")" "$PHASE04_GUI_EVIDENCE_OUT" >/dev/null 2>&1; then
        echo "FAIL: key material found in the saved evidence" >&2
        rm -rf "$PHASE04_GUI_EVIDENCE_OUT"
        exit 1
    fi
    echo "sanitized evidence saved: $PHASE04_GUI_EVIDENCE_OUT"
fi
exit "$DRIVE_STATUS"
