#!/usr/bin/env bash
# Assert the FROZEN Law-1 surface recorded in
# `.agents/260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md` §1.
#
# WHY this exists: once both confirmations are recorded the surface is FROZEN, and
# from then on any removal, rename, layout/discriminant change or addition needs the
# ABI process again. A record whose claims nobody re-reads is not a gate, so this
# check asserts every item that is expressible as code text and fails with a
# per-item message.
#
# The whole-file sha256 values in the record are provenance for *which revision* was
# confirmed, not the gate: an unrelated edit elsewhere in one of those files must not
# require a fresh confirmation, while a change to a confirmed item must. After a
# *confirmed* ABI change, update the record, its digests and this check in the same
# commit as the change.
#
# Usage: bash scripts/check-lookupservicebound-law1-digests.sh
# Bash only; no build required.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

SYSCALL="libs/abi/src/syscall.rs"
RECORD="libs/abi/src/service_binding.rs"
TESTS="libs/abi/src/syscall_tests.rs"
# Re-pinned 2026-11-13 (Phase 13 A1): the ABI moved from `libs/api/src/abi/`
# to the `abi` crate root, unchanged; only the paths below moved.
ABI_MOD="libs/abi/src/lib.rs"
OSTD="libs/ostd/src/syscall.rs"
# The dispatcher and its helpers live in kernel/src/task/syscall/ (split in
# architecture remediation Phase 08); the items are checked across the module.
K_SYSCALL="$(mktemp)"
trap 'rm -f "$K_SYSCALL"' EXIT
cat kernel/src/task/syscall/*.rs > "$K_SYSCALL"
K_REGISTRY="kernel/src/cell/service_registry.rs"

failures=0
# require_row <name> <number> <bit|none> <item description>
# The syscall surface lives in `libs/abi/abi/syscalls.toml` (architecture
# remediation Phase 08, Law-1 CP1): `libs/abi/build.rs` generates the enum
# variant, its `From<usize>` decode arm and its `allowlist_bit` arm from one row,
# so the row is the frozen item (re-pinned from the generated text 2026-11-11).
TABLE="libs/abi/abi/syscalls.toml"
require_row() {
    local name="$1" number="$2" bit="$3" item="$4"
    if ! python3 - "$TABLE" "$name" "$number" "$bit" <<'PY'
import sys, tomllib
table, name, number, bit = sys.argv[1:]
rows = [r for r in tomllib.load(open(table, "rb"))["syscall"] if r["name"] == name]
ok = len(rows) == 1 and rows[0]["number"] == int(number) and (
    ("bit" not in rows[0]) if bit == "none" else rows[0].get("bit") == int(bit))
sys.exit(0 if ok else 1)
PY
    then
        echo "FAIL: $item — $TABLE has no single row name=$name number=$number bit=$bit" >&2
        failures=$((failures + 1))
    fi
}

# require_reserved <number> <former name> <item description>
require_reserved() {
    local number="$1" former="$2" item="$3"
    if ! python3 - "$TABLE" "$number" "$former" <<'PY'
import sys, tomllib
table, number, former = sys.argv[1:]
data = tomllib.load(open(table, "rb"))
rows = [r for r in data.get("reserved", []) if r["number"] == int(number)]
live = [r for r in data["syscall"] if r["number"] == int(number)]
sys.exit(0 if len(rows) == 1 and rows[0]["former"] == former and "bit" not in rows[0] and not live else 1)
PY
    then
        echo "FAIL: $item — $TABLE has no bit-less reserved row number=$number former=$former" >&2
        failures=$((failures + 1))
    fi
}

# require_in <file> <literal> <item description>
require_in() {
    local file="$1" needle="$2" item="$3"
    if [[ ! -f "$file" ]]; then
        echo "FAIL: $item — missing file $file" >&2
        failures=$((failures + 1))
        return 0
    fi
    # -F: the needles are literals, and several contain regex metacharacters.
    if ! grep -Fq -- "$needle" "$file"; then
        echo "FAIL: $item — $file no longer contains:" >&2
        printf '        %s\n' "$needle" >&2
        failures=$((failures + 1))
    fi
}

# forbid_in <file> <literal> <item description>
forbid_in() {
    local file="$1" needle="$2" item="$3"
    if [[ -f "$file" ]] && grep -Fq -- "$needle" "$file"; then
        echo "FAIL: $item — $file must not contain:" >&2
        printf '        %s\n' "$needle" >&2
        failures=$((failures + 1))
    fi
}

# ── libs/abi: the opcode (record §1 items 1-3, 6, 7, 14) ──────────────────────
require_row LookupServiceBound 429 37 "opcode 429 declared, decodes to the variant, allowlist bit 37"
# Re-pinned 2026-11-12 (Phase 11 D5, CP2): LookupService (206) is retired;
# bit 37 stays with LookupServiceBound and 206 is a reserved number.
require_reserved 206 "LookupService" "206 retired as a reserved number; bit 37 stays with 429"
require_row SerialConfigure 428 50 "428 kept as the preceding opcode"
require_row Unknown 9999 none "Unknown sentinel kept"
require_in "$TESTS" "(429, ViSyscall::LookupServiceBound)," "CASES round-trip row for 429"
require_in "$TESTS" "assert_eq!(ViSyscall::LookupServiceBound as usize, 429);" \
    "discriminant pinned in a test"
require_in "$TESTS" "fn lookup_bound_keeps_the_retired_lookup_bit()" \
    "bit-37 test present"

# ── libs/abi: the record (record §1 items 8-12) ───────────────────────────────
require_in "$ABI_MOD" "pub mod service_binding;" "record module declared"
require_in "$RECORD" "pub const SERVICE_BINDING_LEN: usize = 24;" "record length 24"
require_in "$RECORD" "pub struct ServiceBinding {" "record type declared"
require_in "$RECORD" "    pub tid: u64," "field tid"
require_in "$RECORD" "    pub cell_id: u64," "field cell_id"
require_in "$RECORD" "    pub generation: u64," "field generation"
require_in "$RECORD" "pub const fn is_live(&self) -> bool {" "is_live present"
require_in "$RECORD" "pub fn to_bytes(self) -> [u8; SERVICE_BINDING_LEN] {" "to_bytes present"
require_in "$RECORD" "pub fn from_bytes(bytes: &[u8]) -> Option<Self> {" "from_bytes present"
require_in "$RECORD" "self.tid != 0 && self.cell_id != 0 && self.generation != 0" \
    "is_live requires all three nonzero"
require_in "$RECORD" "binding.is_live().then_some(binding)" \
    "from_bytes rejects a record that names no live provider"
# The no-reserved-field decision is deliberate (record item 12): a future need is a
# new opcode. A reserved field appearing here means the confirmed layout changed.
forbid_in "$RECORD" "reserved" "record has no reserved field"

# ── client wrapper and kernel plumbing (record §1 items 2, 13) ────────────────
require_in "$OSTD" "pub fn sys_lookup_service_bound(" "client wrapper present"
require_in "$K_SYSCALL" "    LookupServiceBound {" "kernel Syscall variant present"
# The kernel's variant → opcode map (`opcode_of`) is generated from the table row:
# the same-named kernel variant maps to 429 unless the row names another one.
if ! python3 -c 'import sys, tomllib; r = [x for x in tomllib.load(open(sys.argv[1], "rb"))["syscall"] if x["name"] == "LookupServiceBound"]; sys.exit(0 if len(r) == 1 and "kernel" not in r[0] else 1)' "$TABLE"; then
    echo "FAIL: kernel allowlist mapping present — $TABLE row LookupServiceBound names another kernel variant" >&2
    failures=$((failures + 1))
fi
require_in "$K_SYSCALL" "ViSyscall::LookupServiceBound => Syscall::LookupServiceBound {" \
    "kernel decode arm present"
require_in "$K_SYSCALL" "SyscallError::BufferTooSmall" \
    "short-buffer error path present"
require_in "$K_REGISTRY" "pub fn lookup_bound(service_id: u16) -> Option<(usize, u64, u64)> {" \
    "registry lookup_bound present"

if [[ "$failures" -ne 0 ]]; then
    echo "FAIL: $failures LookupServiceBound Law-1 item(s) drifted." >&2
    echo "      A confirmed item changed; the ABI process (two fresh confirmations)" >&2
    echo "      is required before this check may pass again. See" >&2
    echo "      .agents/260927-1100-c2c-anywhere-tier-aware/law1-lookupservicebound.md" >&2
    exit 1
fi

echo "PASS: LookupServiceBound Law-1 items intact (opcode, record layout, allowlist, wiring)"
