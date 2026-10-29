#!/usr/bin/env bash
# S10-2a real refusals (scratch only; canonical paths are only named, never written).
set -u
W=/tmp/cellbox-s10-2a-u8em
cd /home/dmin/cellos
source "$W/toolchain.env"
unset CELLBOX_PRIVATE_FIXTURE_SIGNING CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI
EV="$W/evidence/refusals"
mkdir -p "$EV" "$W/cli"
: > "$EV/summary.txt"
REL="$W/target/x86_64-unknown-none/release"
relsnap() { (cd "$REL" && sha256sum cellbox terminal app-init platform); }

note() { echo "$1: exit $2; $(grep -E 'REFUSED|FAIL|ROLLOUT' "$EV/$1.log" | head -2 | tr '\n' ' ')" | tee -a "$EV/summary.txt"; }

# 1. Canonical out-dir with the dev key: refused before invalidation or build.
relsnap > "$EV/canonical-outdir.rel-before"
python3 scripts/assemble-x86-image.py --profile headless --out-dir kernel/src/embedded-x86_64 > "$EV/canonical-outdir.log" 2>&1
rc=$?; relsnap > "$EV/canonical-outdir.rel-after"
note canonical-outdir $rc
grep -q "+ cargo build" "$EV/canonical-outdir.log" && echo "canonical-outdir: BUILD STARTED (unexpected)" | tee -a "$EV/summary.txt" \
    || echo "canonical-outdir: no cargo build started" | tee -a "$EV/summary.txt"
cmp -s "$EV/canonical-outdir.rel-before" "$EV/canonical-outdir.rel-after" \
    && echo "canonical-outdir: scratch release binaries not invalidated (unchanged)" | tee -a "$EV/summary.txt"

# 2. cellos-sign CLI with the dev key on a scratch cellbox copy: exit 3.
cp "$REL/cellbox" "$W/cli/cellbox"
sha256sum "$W/cli/cellbox" > "$EV/cli.before"
python3 scripts/cellos-sign --sign "$W/cli/cellbox" > "$EV/cli-dev-cellbox.log" 2>&1
note cli-dev-cellbox $?

# 3. The same with every variable set (fixture, prod override, CI markers): exit 3.
env CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 GITHUB_ACTIONS=true CELLOS_SIGN_CI=1 \
    python3 scripts/cellos-sign --sign "$W/cli/cellbox" > "$EV/cli-dev-cellbox-all-env.log" 2>&1
note cli-dev-cellbox-all-env $?

# 4. The CLI has no route option: argparse exit 2.
python3 scripts/cellos-sign --route x86-candidate-headless --sign "$W/cli/cellbox" > "$EV/cli-route-flag.log" 2>&1
rc=$?; echo "cli-route-flag: exit $rc; $(tail -1 "$EV/cli-route-flag.log")" | tee -a "$EV/summary.txt"
sha256sum -c "$EV/cli.before" > /dev/null && echo "cli: scratch cellbox copy unchanged (unsigned)" | tee -a "$EV/summary.txt"

# 5. The assembler imported by another program (__main__ is not the recipe): the route refuses.
python3 - "$W/via-import" > "$EV/import-caller.log" 2>&1 <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("asm", "scripts/assemble-x86-image.py")
asm = importlib.util.module_from_spec(spec); spec.loader.exec_module(asm)
sys.exit(asm.main(["--profile", "headless", "--out-dir", sys.argv[1]]))
PY
note import-caller $?
[[ -e "$W/via-import" ]] && echo "import-caller: out-dir EXISTS (unexpected)" | tee -a "$EV/summary.txt" \
    || echo "import-caller: out-dir not created" | tee -a "$EV/summary.txt"

# 6. G1 unchanged: the desktop packager still exits first.
python3 scripts/package-desktop-x86.py --out-dir "$W/packager" > "$EV/packager-g1.log" 2>&1
note packager-g1 $?
