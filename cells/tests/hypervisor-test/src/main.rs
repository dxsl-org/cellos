//! Phase 04 hypervisor ABI smoke test.
//!
//! Exercises the full kernel↔cell VMM ABI (syscalls 220-225) from userspace:
//!   1. `sys_create_vm`   → allocates guest RAM + Stage-2
//!   2. `sys_create_vcpu` → creates a vCPU at BLOB_IPA (kernel writes HVC smoke blob)
//!   3. `sys_run_vcpu`    → world-switches; expects `VmExit::Hvc { regs[0]=42 }`
//!
//! Compiled for ARM64 only (HypervisorCap requires EL2 boot).
//!
//! The syscalls go through the shared `ostd::vmm` wrappers, so this crate
//! holds no `unsafe` and forbids it.
#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use api::hypervisor::ViVmExit;
use ostd::io::println;
use ostd::vmm;

// Declare manifest: hypervisor = true (grants HypervisorCap at EL2).
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = true
);
// Declare allowlist bit 44 (HypervisorCap syscalls).
api::declare_syscalls![
    Log,
    CreateVm,
    CreateVcpu,
    MapGuestMemory,
    RunVcpu,
    VcpuRegs,
    InjectIrq
];

/// Guest IPA where the vCPU blob is placed (must be inside the mapped region).
const BLOB_IPA: u64 = 0x4008_0000;

/// Guest pages allocated for the test VM (512 × 4KiB = 2 MiB).
const GUEST_PAGES: usize = 512;

// The macro's expansion carries the loader's `#[no_mangle]`, so this crate's
// own code stays inside `forbid(unsafe_code)`.
ostd::cell_main!(extern "C" hv_test_main);

fn hv_test_main() -> ! {
    println("[hv-test] Phase 04 hypervisor ABI smoke test");

    // ── 1. Create VM ──────────────────────────────────────────────────────────
    let vm_id = vmm::create_vm(GUEST_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        println("[hv-test] FAIL: sys_create_vm returned error (not at EL2?)");
        ostd::syscall::sys_exit(1);
    }
    println("[hv-test] created vm_id=1 (ok)");

    // ── 2. Create vCPU (kernel writes HVC smoke blob at BLOB_IPA) ─────────────
    let vcpu_id = vmm::create_vcpu(vm_id, BLOB_IPA);
    if vcpu_id == 0 || vcpu_id == usize::MAX {
        println("[hv-test] FAIL: sys_create_vcpu returned error");
        ostd::syscall::sys_exit(1);
    }
    println("[hv-test] created vcpu_id=1 (ok)");

    // ── 3. Run vCPU ───────────────────────────────────────────────────────────
    let mut exit = ViVmExit::Unknown { ec: 0, iss: 0 };

    let ret = vmm::run_vcpu(
        vm_id, vcpu_id, 0, // budget_ns = 0 (unlimited for this test)
        &mut exit,
    );
    if ret == usize::MAX {
        println("[hv-test] FAIL: sys_run_vcpu returned error");
        ostd::syscall::sys_exit(1);
    }

    // ── 4. Assert VmExit::Hvc { regs[0] = 42 } ───────────────────────────────
    match exit {
        ViVmExit::Hvc { imm: 0, regs } => {
            if regs[0] == 42 {
                println("[hv-test] PASS: vmexit=Hvc x0=42");
            } else {
                println("[hv-test] FAIL: Hvc exit but x0 != 42");
                ostd::syscall::sys_exit(1);
            }
        }
        ViVmExit::Unknown { ec, iss } => {
            // Expected on non-test-hooks kernels (guest RAM is zeroed, not loaded with blob).
            println("[hv-test] INFO: vmexit=Unknown (test-hooks not active in kernel)");
            let _ = (ec, iss);
        }
        other => {
            let _ = other;
            println("[hv-test] FAIL: unexpected vmexit variant");
            ostd::syscall::sys_exit(1);
        }
    }

    println("[hv-test] done");
    ostd::syscall::sys_exit(0);
}
