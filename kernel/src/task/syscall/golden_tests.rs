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
