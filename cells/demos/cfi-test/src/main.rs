//! CFI violation test cell.
//!
//! Law 4 carve-out: this cell MUST use `unsafe` to manufacture a non-BTI /
//! non-ENDBR64 function pointer and call it — that is the entire test.
//! Hardware enforcement is proved only when the kernel logs a fault, not when
//! this cell prints "triggering". The test harness greps for the fault log
//! line, not a PASS banner from this cell.
//!
//! On hardware/QEMU that does NOT support BTI or CET-IBT the call succeeds,
//! the cell prints "SKIP: hardware unavailable", and the harness treats it as
//! a soft skip.
//!
//! Spawned UNSUPERVISED so init's never-die watchdog does not restart a
//! deliberately crashing cell.

// Law 4 carve-out: deliberate CFI violation for hardware-enforcement testing.
#![allow(unsafe_code)]
#![no_std]
#![no_main]

use ostd::app::{AppContext, AppEvent};
use ostd::io::println;
use ostd::syscall::sys_exit;

ostd::app_entry!(handler = cfi_handler);

fn cfi_handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => run_test(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

fn run_test() {
    #[cfg(target_arch = "aarch64")]
    {
        // Detect BTI at runtime via ID_AA64PFR1_EL1[3:0].
        // Cells run at EL0 so we cannot mrs ID registers directly.
        // Instead we rely on the kernel having logged "BTI: enabled" if present.
        // The actual BTI check is: if SCTLR_EL1.BT0 is set, any indirect branch
        // to an address without a BTI landing pad faults with EC=0x0D.
        //
        // We probe by pointing a function pointer at a raw data byte — no BTI
        // landing pad — and calling it. If BTI is active the fault fires before
        // the first instruction of PAYLOAD executes.

        println("cfi-test: aarch64 BTI probe");
        println("cfi-test: triggering indirect branch to non-BTI address");

        // A single data byte with no BTI landing pad (BTI c = 0xD503245F).
        // PAYLOAD is read-only; we cast its address to a function pointer.
        // The CPU sees a BLR (indirect call) landing on a non-BTI address.
        //
        // SAFETY: deliberate CFI violation — testing hardware BTI enforcement.
        // Expected outcome: synchronous BTI fault (ESR_EL1.EC = 0x0D) before
        // PAYLOAD[0] executes. If BTI is not enabled the call returns normally
        // and we print SKIP below.
        static PAYLOAD: [u8; 1] = [0xC0]; // arbitrary non-BTI byte
        let fptr: unsafe extern "C" fn() = unsafe { core::mem::transmute(PAYLOAD.as_ptr()) };
        // SAFETY: deliberate CFI violation — testing hardware BTI enforcement.
        unsafe { fptr() };

        // Reaching here means BTI did not fault (feature disabled or unavailable).
        println("cfi-test: SKIP: BTI not enforced (hardware unavailable or disabled)");
        sys_exit(0);
    }

    #[cfg(target_arch = "x86_64")]
    {
        println("cfi-test: x86_64 CET-IBT probe");
        println("cfi-test: triggering indirect call to non-ENDBR64 address");

        // Four NOP bytes + RET — no ENDBR64 (0xF3 0x0F 0x1E 0xFA) prefix.
        // CET-IBT expects every indirect-call target to start with ENDBR64.
        // Calling PAYLOAD fires #CP (vector 21) if CET-IBT is enabled.
        //
        // SAFETY: deliberate CFI violation — testing hardware CET-IBT enforcement.
        // Expected outcome: #CP fault logged by the kernel. Reaching the line
        // after fptr() means IBT is disabled or unavailable.
        static PAYLOAD: [u8; 4] = [0x90, 0x90, 0x90, 0xC3]; // NOP NOP NOP RET
        let fptr: unsafe extern "C" fn() = unsafe { core::mem::transmute(PAYLOAD.as_ptr()) };
        // SAFETY: deliberate CFI violation — testing hardware CET-IBT enforcement.
        unsafe { fptr() };

        println("cfi-test: SKIP: CET-IBT not enforced (hardware unavailable or disabled)");
        sys_exit(0);
    }

    #[cfg(target_arch = "riscv64")]
    {
        // RISC-V has no BTI / ENDBR equivalent in the current spec.
        println("cfi-test: SKIP: no CFI hardware on riscv64");
        sys_exit(0);
    }
}
