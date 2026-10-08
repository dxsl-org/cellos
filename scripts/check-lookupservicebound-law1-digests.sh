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

SYSCALL="libs/api/src/abi/syscall.rs"
RECORD="libs/api/src/abi/service_binding.rs"
TESTS="libs/api/src/abi/syscall_tests.rs"
ABI_MOD="libs/api/src/abi.rs"
OSTD="libs/ostd/src/syscall.rs"
K_SYSCALL="kernel/src/task/syscall.rs"
K_REGISTRY="kernel/src/cell/service_registry.rs"

failures=0

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

# ── libs/api: the opcode (record §1 items 1-3, 6, 7, 14) ──────────────────────
require_in "$SYSCALL" "LookupServiceBound = 429," "opcode 429 declared"
require_in "$SYSCALL" "429 => ViSyscall::LookupServiceBound," "429 decodes to the variant"
require_in "$SYSCALL" "Self::LookupService | Self::LookupServiceBound => Some(37)," \
    "allowlist bit 37 shared with LookupService"
require_in "$SYSCALL" "LookupService = 206," "LookupService 206 kept"
require_in "$SYSCALL" "SerialConfigure = 428," "428 kept as the preceding opcode"
require_in "$SYSCALL" "Unknown = 9999," "Unknown sentinel kept"
require_in "$TESTS" "(429, ViSyscall::LookupServiceBound)," "CASES round-trip row for 429"
require_in "$TESTS" "assert_eq!(ViSyscall::LookupServiceBound as usize, 429);" \
    "discriminant pinned in a test"
require_in "$TESTS" "fn lookup_bound_shares_open_lookup_authority()" \
    "allowlist-sharing test present"

# ── libs/api: the record (record §1 items 8-12) ───────────────────────────────
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
require_in "$K_SYSCALL" "Syscall::LookupServiceBound { .. } => V::LookupServiceBound," \
    "kernel allowlist mapping present"
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
