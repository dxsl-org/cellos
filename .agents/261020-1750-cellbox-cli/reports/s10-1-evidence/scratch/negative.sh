#!/usr/bin/env bash
# S10-1 negative runs against the real assembler (scratch only).
set -u
W=/tmp/cellbox-s10-1-3jLV
cd /home/dmin/cellos
source "$W/toolchain.env"
OUT="$W/headless"
EV="$W/evidence/negative"
mkdir -p "$EV"
printf '\x7fELF legacy split shell fixture\n' > "$W/legacy-shell"
printf '\x7fELF legacy inbound sshd fixture\n' > "$W/legacy-sshd"
TIER2_SHA=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["files"]["/bin/tier2-smoke"]["sha256"])' "$OUT/manifest.json")

snap() { (cd "$OUT" && find . -type f -print0 | sort -z | xargs -0 sha256sum; find . | sort); }

run() {
    local name="$1"; shift
    snap > "$EV/$name.before"
    "$@" > "$EV/$name.log" 2>&1
    local rc=$?
    snap > "$EV/$name.after"
    if cmp -s "$EV/$name.before" "$EV/$name.after"; then same=unchanged; else same=CHANGED; fi
    echo "$name: exit $rc; out-dir $same; refusal: $(grep -E '^(FAIL|REFUSED)' "$EV/$name.log" | head -2 | tr '\n' ' ')" | tee -a "$EV/summary.txt"
}

: > "$EV/summary.txt"
echo "tier2-smoke sha256 used as the injected blocked hash: $TIER2_SHA" | tee -a "$EV/summary.txt"
run inject-bin-shell env CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 \
    python3 scripts/assemble-x86-image.py --profile headless --out-dir "$OUT" --seed-file "$W/keys/seed.hex" \
    --test-inject "$W/legacy-shell=/bin/shell"
run inject-bin-sshd env CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 \
    python3 scripts/assemble-x86-image.py --profile headless --out-dir "$OUT" --seed-file "$W/keys/seed.hex" \
    --test-inject "$W/legacy-sshd=/bin/sshd"
run blocked-hash-input env CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 \
    python3 scripts/assemble-x86-image.py --profile headless --out-dir "$OUT" --seed-file "$W/keys/seed.hex" \
    --test-block-sha256 "$TIER2_SHA"
run dev-key-no-seed env -u CELLBOX_PRIVATE_FIXTURE_SIGNING -u CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI \
    python3 scripts/assemble-x86-image.py --profile headless --out-dir "$OUT"
# The dev-key refusal happens before any key is loaded: the release cellbox stays unsigned.
python3 - "$W/target/x86_64-unknown-none/release/cellbox" <<'PY' | tee -a "$EV/summary.txt"
import sys
data = open(sys.argv[1], "rb").read()
print("dev-key-no-seed: release cellbox carries __ViCell_sig section name:", b"__ViCell_sig" in data)
PY
