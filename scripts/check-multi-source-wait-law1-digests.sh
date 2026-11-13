#!/usr/bin/env bash
# Assert the FROZEN Law-1 surface of C2C Phase 03 slice d (multi-source WaitCompletion),
# confirmed in
#   .agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp1-approval.md
#   .agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp2-approval.md
# and recorded in docs/specs/17-ipc-wire-contract.md §9 (2026-11-02 entry).
# Amended by the `IRQ = 1 << 3` source (x86 plan 261103-2218 phase 04b, CP1 approved
# in .agents/261103-2218-x86-platform-breadth/cp1-04b-waitcompletion-irq.md, CP2
# pending in cp2-04b-waitcompletion-irq.md; Spec 17 §9 2026-11-05 entry).
#
# WHY this exists: once both confirmations are recorded the surface is FROZEN, and
# from then on any removal, rename, layout/value change or addition needs the ABI
# process again. A record whose claims nobody re-reads is not a gate, so this check
# asserts every item that is expressible as code text and fails with a per-item
# message.
#
# The whole-file sha256 values in the CP2 record are provenance for *which revision*
# was confirmed, not the gate: an unrelated edit elsewhere in one of those files must
# not require a fresh confirmation, while a change to a confirmed item must. After a
# *confirmed* ABI change, update the records, their digests and this check in the
# same commit as the change.
#
# Usage: bash scripts/check-multi-source-wait-law1-digests.sh
# Bash only; no build required.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# Re-pinned 2026-11-13 (Phase 13 A1): the ABI moved from `libs/api/src/abi/`
# to the `abi` crate root, unchanged; only the paths moved.
COMPLETION="libs/abi/src/completion.rs"
SYSCALL="libs/abi/src/syscall.rs"
OSTD_IPC="libs/ostd/src/ipc.rs"
OSTD_RUNTIME="libs/ostd/src/runtime.rs"
OSTD_LIB="libs/ostd/src/lib.rs"
K_WAIT="kernel/src/task/completion_wait.rs"
K_ASYNC="kernel/src/task/async_ipc.rs"
K_TASK="kernel/src/task.rs"
K_SCHED="kernel/src/sched/scheduler.rs"
# The dispatcher and its helpers live in kernel/src/task/syscall/ (split in
# architecture remediation Phase 08); the items are checked across the module.
K_SYSCALL="$(mktemp)"
trap 'rm -f "$K_SYSCALL"' EXIT
cat kernel/src/task/syscall/*.rs > "$K_SYSCALL"
K_MSI="kernel/src/arch/x86_64/dev/pcie_msi.rs"

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

# require_count <file> <literal> <count> <item description>
require_count() {
    local file="$1" needle="$2" want="$3" item="$4" have
    have="$(grep -Fc -- "$needle" "$file" 2>/dev/null || true)"
    if [[ "$have" != "$want" ]]; then
        echo "FAIL: $item — $file contains $have (want $want) of:" >&2
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

# ── libs/abi: the record and the source vocabulary (Spec 17 §9) ──────────────
require_in "$COMPLETION" "pub const COMPLETION_LEN: usize = 24;" "record length 24"
require_in "$COMPLETION" "pub const COMPLETION_VERSION: u32 = 1;" "record version 1"
require_in "$COMPLETION" "pub const COMPLETION_MAGIC: u32 = 0x5649_4351;" "record magic VICQ"
require_in "$COMPLETION" "out[8..12].copy_from_slice(&self.slot.to_le_bytes());" "slot at bytes 8..12"
require_in "$COMPLETION" "out[12..16].copy_from_slice(&self.source.to_le_bytes());" \
    "source word at bytes 12..16"
require_in "$COMPLETION" "out[16..24].copy_from_slice(&self.result.to_le_bytes());" \
    "result (token for IPC) at bytes 16..24"
require_in "$COMPLETION" "pub const UNSPECIFIED: u32 = 0;" "UNSPECIFIED = 0 kept"
require_in "$COMPLETION" "pub const NET_RX: u32 = 1 << 0;" "NET_RX = 1 kept"
require_in "$COMPLETION" "pub const TIMER: u32 = 1 << 1;" "TIMER = 2 kept"
require_in "$COMPLETION" "pub const IPC: u32 = 1 << 2;" "IPC = 4 appended"
require_in "$COMPLETION" "pub const IRQ: u32 = 1 << 3;" "IRQ = 8 appended (04b)"
require_in "$COMPLETION" "const ALL: u32 = NET_RX | TIMER | IPC | IRQ;" "known source set"
require_in "$COMPLETION" "matches!(mask, NET_RX | TIMER | IPC | IRQ)" \
    "a record carries exactly one known source"
require_in "$COMPLETION" "pub const fn is_wait_mask_supported(mask: u32) -> bool {" \
    "wait-mask predicate present"
require_in "$COMPLETION" "mask != 0 && mask & !ALL == 0" "wait mask is a non-empty known subset"
require_in "$COMPLETION" "fn ipc_record_bytes_carry_the_token_without_moving_any_field()" \
    "IPC record byte test present"
require_in "$COMPLETION" "fn irq_record_carries_the_line_and_refuses_a_mixed_source_word()" \
    "IRQ record byte test present"

# ── libs/abi: opcode, allowlist and the exported allowlist symbol ─────────────
require_row WaitCompletion 242 42 "opcode 242 kept, decodes to the variant, allowlist bit 42 kept (shared)"
require_in "$SYSCALL" '#[export_name = "__vicell_syscall_allowlist"]' \
    "declare_syscalls! exports the allowlist symbol"
# Re-pinned 2026-11-13 (Phase 12 step 3, CP2): `service_entry!` had no user and
# was deleted; services run `ostd::service::serve` under `declare_syscalls!`.
require_count "$OSTD_RUNTIME" '#[export_name = "__vicell_syscall_allowlist"]' 3 \
    "app_entry! exports the allowlist symbol"

# ── libs/ostd: the SDK path choice (no probing) ───────────────────────────────
require_in "$OSTD_LIB" "#![feature(linkage)]" "weak linkage enabled for the default"
require_in "$OSTD_IPC" "pub static __vicell_syscall_allowlist: u64 = u64::MAX;" \
    "weak permit-all default mirrors the loader"
require_in "$OSTD_IPC" '#[linkage = "weak"]' "the SDK default is weak"
require_in "$OSTD_IPC" "fn allowlist_permits_completion_wait(allowlist: u64) -> bool {" \
    "path chosen from the compiled allowlist"
forbid_in "$OSTD_IPC" "completion_wait_refused" "no denied-syscall probe"
require_in "$OSTD_IPC" "pub fn wait_any(sources: u32, timeout_ticks: u64) -> Result<Wake, IpcError> {" \
    "wait_any API present"

# ── kernel: mask rule, one-shot IPC source and the granted predicate lines ────
# Re-pinned 2026-11-13 (Phase 13 A3): the kernel imports the ABI as `abi::`
# instead of `api::`; every `api::completion::source::` literal below became
# `abi::completion::source::`, the code is otherwise unchanged.
require_in "$K_WAIT" "abi::completion::source::is_wait_mask_supported(mask)" \
    "syscall validates with the public mask rule"
require_in "$K_WAIT" "if mask != abi::completion::source::NET_RX && mask != abi::completion::source::TIMER {" \
    "lone NET_RX/TIMER keep the original path"
require_in "$K_WAIT" "fn wait_multi(" "multi-source wait present"
require_in "$K_WAIT" "pub(super) const SELECTION_ORDER: [u32; 4] = [" "rotation order present"
require_in "$K_WAIT" "pub(super) const INITIAL_ROTOR: u8 = (SELECTION_ORDER.len() - 1) as u8;" \
    "a fresh rotor starts at NET_RX"
require_in "$K_WAIT" '(cfg!(target_arch = "x86_64") || mask & abi::completion::source::IRQ == 0)' \
    "IRQ is refused off x86_64"
require_in "$K_WAIT" "return Err(SyscallError::PermissionDenied);" \
    "an IRQ wait without a held line is refused"
require_in "$K_SYSCALL" "let mask = u32::try_from(a0).unwrap_or(u32::MAX);" \
    "mask bits 32..63 are refused, not dropped"
require_in "$K_MSI" "irq_wait::hint_msi_wake();" "the MSI handler raises the wake hint"
require_in "$K_SCHED" "| crate::dev::irq_wait::take_wake_hint();" \
    "the hart-0 pass swaps the MSI wake hint every time"
# Re-pinned 2026-11-12 (Phase 11 step 5, CP2): the sweep's completion rules
# moved, unchanged, into completion_wait::sweep_wake.
require_in "$K_SCHED" "match sweep_wake(task, *source, *delivery, *deadline, now as u64) {" \
    "the sweep applies the one completion-wait rule"
require_in "$K_WAIT" "Delivery::Queue if irq_wake_due(task, source) => SweepWake::Ready," \
    "the sweep wakes an IRQ wait on a pending snapshot line"
forbid_in "$K_WAIT" "PROTO_IPC" "no kernel-private prototype bit remains"
require_in "$K_ASYNC" "source & abi::completion::source::IPC != 0" \
    "IPC terminals wake an IPC-mask completion waiter"
require_in "$K_ASYNC" ".min_by_key(|op| op.id)?;" "oldest submission is reported first"
require_in "$K_ASYNC" "pub(crate) fn restore_notification(task: &mut Task, token: usize) -> bool {" \
    "a failed copy-out re-arms the notification"
require_in "$K_TASK" "if source & abi::completion::source::NET_RX != 0" \
    "raw mailbox interrupts only masks naming NET_RX"
require_in "$K_WAIT" "timed_out: source & abi::completion::source::TIMER == 0," \
    "deadline sweep keys on the TIMER bit"

if [[ "$failures" -ne 0 ]]; then
    echo "FAIL: $failures multi-source WaitCompletion Law-1 item(s) drifted." >&2
    echo "      A confirmed item changed; the ABI process (two fresh confirmations)" >&2
    echo "      is required before this check may pass again. See" >&2
    echo "      .agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp2-approval.md" >&2
    exit 1
fi

echo "PASS: multi-source WaitCompletion Law-1 items intact (sources, record, mask rule, allowlist symbol, wiring)"
