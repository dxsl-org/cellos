// SPDX-License-Identifier: MPL-2.0
//! Golden dump of the syscall surface (architecture remediation Phase 08, CP1 §3).
//!
//! For every opcode `0..=OPCODE_SPAN` it records what today's hand-written lists
//! say: the api `ViSyscall` decode and allowlist bit, the kernel raw decode
//! (`map_syscall`), its variant → `ViSyscall` map, its result encoder, and the
//! allowlist bits that actually gate the opcode at `check_allowlist`. The table
//! that replaces those lists must reproduce this file row by row.
//!
//! `CELLOS_SYSCALL_GOLDEN_WRITE=1 cargo test -p cellos-kernel --features
//! compile-only --target x86_64-unknown-linux-gnu syscall_golden` rewrites
//! `libs/api/abi/syscalls.golden.json`; otherwise the test fails on any drift.
//! Run without feature flags that add opcodes (`bench-counters`).

use super::{check_allowlist, map_syscall, supports_typed_oom, syscall_to_vi, SyscallError};
use crate::task::cap::CapSet;
use crate::task::{scheduler::Scheduler, tcb::Task, SCHEDULER};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::syscall::ViSyscall;
use types::CellId;

/// Highest opcode probed. Every assigned opcode today is below 1024.
const OPCODE_SPAN: usize = 1023;
const TID: usize = 1;

fn variant_name<T: core::fmt::Debug>(value: &T) -> String {
    let text = format!("{value:?}");
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    String::from(&text[..end])
}

fn set_allowlist(allowlist: u64) {
    let mut guard = SCHEDULER.lock();
    guard
        .as_mut()
        .and_then(|s| s.tasks.get_mut(&TID))
        .expect("golden task")
        .syscall_allowlist = allowlist;
}

/// Bits whose sole removal makes `check_allowlist` refuse `id`, plus whether an
/// empty (non-permit-all) allowlist refuses it. An opcode refused by every
/// explicit allowlist (an unknown opcode) reports no bits and `true`.
fn gate(id: usize) -> (Vec<u8>, bool) {
    let mut bits = Vec::new();
    for bit in 0..64u8 {
        set_allowlist(!(1u64 << bit));
        if check_allowlist(id, TID) == Err(SyscallError::PermissionDenied) {
            bits.push(bit);
        }
    }
    set_allowlist(0);
    let empty_denies = check_allowlist(id, TID).is_err();
    if bits.len() == 64 {
        bits.clear();
    }
    (bits, empty_denies)
}

fn render() -> String {
    let mut rows = Vec::new();
    for id in 0..=OPCODE_SPAN {
        let api = ViSyscall::from(id);
        let kernel = map_syscall(id, 0, 0, 0, 0);
        let (gate_bits, empty_denies) = gate(id);
        if api == ViSyscall::Unknown && kernel.is_none() && gate_bits.is_empty() && empty_denies {
            continue; // unassigned: refused under any explicit allowlist
        }
        let opt = |s: Option<String>| s.map_or(String::from("null"), |s| format!("\"{s}\""));
        let kernel_name = kernel.as_ref().map(variant_name);
        let vi_of_kernel = kernel
            .as_ref()
            .and_then(syscall_to_vi)
            .map(|v| variant_name(&v));
        let typed_oom = kernel.as_ref().is_some_and(supports_typed_oom);
        let bits: Vec<String> = gate_bits.iter().map(|b| format!("{b}")).collect();
        rows.push(format!(
            "    {{\"id\": {id}, \"api\": \"{}\", \"api_bit\": {}, \"kernel\": {}, \
             \"kernel_vi\": {}, \"typed_oom\": {typed_oom}, \"gate_bits\": [{}], \
             \"empty_allowlist_denies\": {empty_denies}}}",
            variant_name(&api),
            api.allowlist_bit()
                .map_or(String::from("null"), |b| format!("{b}")),
            opt(kernel_name),
            opt(vi_of_kernel),
            bits.join(", "),
        ));
    }
    format!(
        "{{\n  \"source\": \"kernel/src/task/syscall/golden_tests.rs (host, compile-only, no bench-counters)\",\n  \"rows\": [\n{}\n  ]\n}}\n",
        rows.join(",\n")
    )
}

#[test]
fn syscall_golden() {
    extern crate std;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    let mut task = alloc::boxed::Box::new(Task::new(TID, CellId(7), "golden", Vec::new()));
    // Every right, so only the allowlist decides (observe rights included).
    CapSet::ALL.apply_to(&mut task);
    scheduler.tasks.insert(TID, task);
    *SCHEDULER.lock() = Some(scheduler);

    let text = render();
    *SCHEDULER.lock() = saved;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../libs/api/abi/syscalls.golden.json");
    if std::env::var_os("CELLOS_SYSCALL_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir libs/api/abi");
        std::fs::write(&path, &text).expect("write syscalls.golden.json");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == text,
        "libs/api/abi/syscalls.golden.json differs from the code; the syscall surface moved"
    );
}

/// CP1 §3.2: one gate. For every opcode the trap decodes, the in-kernel entry
/// (`handle_syscall`, gated on `opcode_of` of the decoded variant) admits under
/// exactly the allowlists the trap admits — except where a raw alias shares a
/// kernel variant with a gated api row (raw 100 and `Call` both decode to
/// `ServiceLookup`), where the in-kernel entry may only be stricter.
#[test]
fn in_kernel_gate_matches_the_trap_gate() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    let mut task = alloc::boxed::Box::new(Task::new(TID, CellId(7), "gate", Vec::new()));
    CapSet::ALL.apply_to(&mut task);
    scheduler.tasks.insert(TID, task);
    *SCHEDULER.lock() = Some(scheduler);

    let mut allowlists: Vec<u64> = (0..64).map(|bit| !(1u64 << bit)).collect();
    allowlists.extend([0, u64::MAX]);
    let mut stricter = Vec::new();
    for id in 0..=OPCODE_SPAN {
        let Some(decoded) = map_syscall(id, 0, 0, 0, 0) else {
            continue;
        };
        let opcode = super::opcode_of(&decoded);
        for &allowlist in &allowlists {
            set_allowlist(allowlist);
            let trap = check_allowlist(id, TID).is_ok();
            let kernel = check_allowlist(opcode, TID).is_ok();
            if trap != kernel {
                assert!(
                    trap && !kernel,
                    "opcode {id}: the in-kernel entry admits what the trap refuses"
                );
                stricter.push(id);
            }
        }
    }
    *SCHEDULER.lock() = saved;
    stricter.dedup();
    assert_eq!(
        stricter,
        [100],
        "only the raw ServiceLookup alias may differ"
    );
}

/// The five kernel variants with no opcode (`[[internal]]` rows) map to
/// `usize::MAX`, which the gate refuses under any explicit allowlist and admits
/// only under permit-all — never through a trap, which cannot decode them.
#[test]
fn internal_variants_have_no_opcode_and_need_permit_all() {
    use super::Syscall;
    let internal = [
        Syscall::Lend {
            target: 0,
            ptr: 0,
            len: 0,
            flags: 0,
        },
        Syscall::BorrowRead {
            lease_id: 0,
            offset: 0,
            ptr: 0,
            len: 0,
        },
        Syscall::BorrowWrite {
            lease_id: 0,
            offset: 0,
            ptr: 0,
            len: 0,
        },
        Syscall::Grant {
            target: 0,
            ptr: 0,
            len: 0,
            flags: 0,
        },
        Syscall::Map { grant_id: 0 },
    ];
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    let mut task = alloc::boxed::Box::new(Task::new(TID, CellId(7), "internal", Vec::new()));
    CapSet::ALL.apply_to(&mut task);
    scheduler.tasks.insert(TID, task);
    *SCHEDULER.lock() = Some(scheduler);
    for call in &internal {
        assert_eq!(super::opcode_of(call), usize::MAX, "{call:?}");
        assert_eq!(super::syscall_to_vi(call), None, "{call:?}");
        set_allowlist(!0u64 >> 1); // every real bit, but not permit-all
        assert_eq!(
            check_allowlist(super::opcode_of(call), TID),
            Err(SyscallError::PermissionDenied)
        );
        set_allowlist(u64::MAX);
        assert_eq!(check_allowlist(super::opcode_of(call), TID), Ok(()));
    }
    *SCHEDULER.lock() = saved;
}

/// Kernel-origin calls (the GPU scanout path) skip the Cell's allowlist by
/// design; the gate `handle_syscall` applies refuses the same call.
#[test]
fn kernel_origin_skips_the_allowlist_and_handle_syscall_does_not() {
    use super::Syscall;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    let mut task = alloc::boxed::Box::new(Task::new(TID, CellId(7), "driver", Vec::new()));
    CapSet {
        pcie_driver: true,
        ..CapSet::EMPTY
    }
    .apply_to(&mut task);
    task.syscall_allowlist = 0; // declares nothing, in particular not bit 40
    scheduler.tasks.insert(TID, task);
    *SCHEDULER.lock() = Some(scheduler);
    // Malformed reserved words: a dispatched call answers InvalidInput on a host.
    let probe = || Syscall::ProbeVirtioMmio {
        base: 0x1000_1000,
        reserved: (0, 1, 0),
    };
    // `handle_syscall` gates on this check (its audit record needs a kernel).
    assert_eq!(
        check_allowlist(super::opcode_of(&probe()), TID),
        Err(SyscallError::PermissionDenied)
    );
    assert_eq!(
        super::handle_syscall_kernel_origin(TID, probe()),
        Err(SyscallError::InvalidInput)
    );
    *SCHEDULER.lock() = saved;
}
