//! Forward-edge CFI probe cell: AArch64 BTI and x86_64 CET-IBT.
//!
//! Law 4 carve-out: this cell MUST use `unsafe` to make an indirect call to an
//! instruction that is not a landing pad — that is the entire test.
//!
//! ## What the probe executes
//!
//! `global_asm!` places two adjacent labels in this cell's own `.text`, which
//! the loader maps executable for the cell, 16-byte aligned so both sit on one
//! page:
//!
//! ```text
//! cfi_test_padded_target:    BTI c (aarch64) | ENDBR64 (x86_64)
//! cfi_test_unpadded_target:  mov w0/eax, CFI_PROBE_MAGIC
//!                            ret
//! ```
//!
//! 1. **Control** — an indirect call (`blr` / `call reg`) to
//!    `cfi_test_padded_target`. It must return [`CFI_PROBE_MAGIC`]. This proves
//!    the page is executable for the cell and the indirect-call mechanics work,
//!    so a fault in step 2 cannot be an NX/permission fault.
//! 2. **Violation** — the same indirect call to `cfi_test_unpadded_target`: the
//!    very instruction step 1 just executed, so same page and permissions,
//!    differing only in the missing landing pad.
//!
//! (An earlier version jumped into a byte array in `.rodata`; that page is
//! non-executable, so the fault it produced proved NX, not BTI/IBT.)
//!
//! ## What each outcome proves
//!
//! - **Violation faults** — forward-edge CFI is enforced for cells. The cell
//!   dies and the kernel log is the evidence, matched against the printed
//!   `cfi_test_unpadded_target` address: on aarch64 a
//!   `[fault-probe] a64 ... ec=0xd ... pc=<that address>` line (EC 0x0D =
//!   Branch Target exception); on x86_64 a CPL3 vector-21 (#CP) termination.
//! - **Violation returns** — CFI is not enforced for cells: the cell prints
//!   `cfi-test: SKIP:` with the reason and exits 0. This is the expected result
//!   today on every architecture, because the kernel never arms enforcement for
//!   cell code:
//!   - aarch64: the kernel sets SCTLR BT0/BT1 when FEAT_BTI exists
//!     (hal/arch/arm/src/aarch64/cfi.rs), but BTI only checks branches into
//!     *guarded* pages, and no page is ever mapped with the stage-1 GP bit
//!     (descriptor bit 50; hal/arch/arm/src/aarch64/paging.rs `leaf_word`).
//!   - x86_64: the kernel enables supervisor IBT only (IA32_S_CET.ENDBR_EN,
//!     hal/arch/x86/src/x86_64/cet.rs); IA32_U_CET is never written, so CPL3
//!     indirect branches are not tracked.
//!   - riscv64: the kernel does not enable Zicfilp landing pads (no `LPE` bit
//!     in `senvcfg`/`menvcfg`), so the cell skips without probing.
//! - **Control fails** — the test itself is broken: `cfi-test: FAIL`, exit 1.
//!
//! Spawned UNSUPERVISED so init's never-die watchdog does not restart a
//! deliberately crashing cell.

// Law 4 carve-out: deliberate CFI violation for hardware-enforcement testing.
#![allow(unsafe_code)]
#![no_std]
#![no_main]

use ostd::app::{AppContext, AppEvent};
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
use ostd::io::print_fmt;
use ostd::io::println;
use ostd::syscall::sys_exit;

ostd::app_entry!(handler = cfi_handler);

/// Value both probe targets return: it distinguishes "the target ran and
/// returned normally" from a stray return through some other path.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
const CFI_PROBE_MAGIC: u32 = 0x5A;

// Both labels are global + hidden so the Rust side can name them as functions
// without a GOT entry, wherever rustc places this block's codegen unit.
#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".pushsection .text.cfi_test_targets, \"ax\", %progbits",
    ".balign 16",
    ".globl cfi_test_padded_target",
    ".hidden cfi_test_padded_target",
    ".type cfi_test_padded_target, %function",
    "cfi_test_padded_target:",
    // `hint #34` is `BTI c`, spelled as a hint so the assembler needs no BTI
    // feature flag; it is the landing pad BLR requires on a guarded page.
    "    hint #34",
    ".globl cfi_test_unpadded_target",
    ".hidden cfi_test_unpadded_target",
    ".type cfi_test_unpadded_target, %function",
    "cfi_test_unpadded_target:",
    "    mov w0, #{magic}",
    "    ret",
    ".popsection",
    magic = const CFI_PROBE_MAGIC,
);

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".pushsection .text.cfi_test_targets, \"ax\", @progbits",
    ".balign 16",
    ".globl cfi_test_padded_target",
    ".hidden cfi_test_padded_target",
    ".type cfi_test_padded_target, @function",
    "cfi_test_padded_target:",
    // ENDBR64 (F3 0F 1E FA), as raw bytes so the assembler needs no CET flag.
    "    .byte 0xf3, 0x0f, 0x1e, 0xfa",
    ".globl cfi_test_unpadded_target",
    ".hidden cfi_test_unpadded_target",
    ".type cfi_test_unpadded_target, @function",
    "cfi_test_unpadded_target:",
    "    mov eax, {magic}",
    "    ret",
    ".popsection",
    magic = const CFI_PROBE_MAGIC,
);

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
extern "C" {
    /// Landing pad followed by `cfi_test_unpadded_target`; returns
    /// [`CFI_PROBE_MAGIC`].
    fn cfi_test_padded_target() -> u32;
    /// The instruction after the landing pad; returns [`CFI_PROBE_MAGIC`] when
    /// forward-edge CFI is not enforced.
    fn cfi_test_unpadded_target() -> u32;
}

/// Signature of both probe labels.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
type ProbeFn = unsafe extern "C" fn() -> u32;

fn cfi_handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => run_test(),
        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => sys_exit(0),
        _ => {}
    }
}

/// Indirect call through `BLR` to `target`, returning its `w0`.
///
/// # Safety
/// `target` must be one of the two probe labels above: code in this cell's
/// executable `.text` that follows the C ABI, takes no arguments, and touches
/// nothing but `x0`.
#[cfg(target_arch = "aarch64")]
unsafe fn indirect_call(target: ProbeFn) -> u32 {
    let ret: u64;
    // SAFETY: the caller guarantees `target` is C-ABI code with no arguments;
    // clobber_abi("C") covers every register such a callee may clobber,
    // including x30, which BLR itself overwrites.
    unsafe {
        core::arch::asm!(
            "blr {target}",
            target = in(reg) target,
            lateout("x0") ret,
            clobber_abi("C"),
        );
    }
    ret as u32
}

/// Indirect call through `CALL reg` to `target`, returning its `eax`.
///
/// # Safety
/// `target` must be one of the two probe labels above: code in this cell's
/// executable `.text` that follows the C ABI, takes no arguments, and touches
/// nothing but `rax`.
#[cfg(target_arch = "x86_64")]
unsafe fn indirect_call(target: ProbeFn) -> u32 {
    let ret: u64;
    // SAFETY: the caller guarantees `target` is C-ABI code with no arguments;
    // clobber_abi("C") covers every register such a callee may clobber. The
    // asm block may use the stack (no `nostack`), so rustc keeps RSP aligned
    // for the CALL's return-address push.
    unsafe {
        core::arch::asm!(
            "call {target}",
            target = in(reg) target,
            lateout("rax") ret,
            clobber_abi("C"),
        );
    }
    ret as u32
}

/// Runs the control call, then the violating call; returns only by exiting.
///
/// `name` labels the mechanism; `skip` is the lines printed when the violation
/// returns, i.e. when the mechanism is not enforced for cells.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
fn probe(name: &str, skip: &[&str]) -> ! {
    let padded: ProbeFn = cfi_test_padded_target;
    let unpadded: ProbeFn = cfi_test_unpadded_target;
    print_fmt(format_args!(
        "cfi-test: {name} probe: landing pad {padded:p}, non-landing-pad target {unpadded:p}\n"
    ));

    // SAFETY: `padded` is a probe label (see `indirect_call`).
    if unsafe { indirect_call(padded) } != CFI_PROBE_MAGIC {
        println("cfi-test: FAIL: control indirect call to the landing pad returned a wrong value");
        sys_exit(1);
    }
    println("cfi-test: control OK: indirect call to the landing pad ran from executable .text");

    println("cfi-test: triggering indirect call to the non-landing-pad instruction");
    // SAFETY: deliberate CFI violation — `unpadded` is a probe label (see
    // `indirect_call`) that starts without a landing pad. Enforced CFI faults
    // here and the kernel terminates the cell; otherwise it returns the magic.
    if unsafe { indirect_call(unpadded) } != CFI_PROBE_MAGIC {
        println("cfi-test: FAIL: non-landing-pad call returned a wrong value");
        sys_exit(1);
    }
    for line in skip {
        println(line);
    }
    sys_exit(0)
}

fn run_test() -> ! {
    #[cfg(target_arch = "aarch64")]
    probe(
        "aarch64 BTI",
        &[
            "cfi-test: SKIP: BTI not enforced for EL0: BLR to a non-BTI instruction in executable .text returned",
            "cfi-test: reason: the kernel maps no cell page with the BTI GP bit, so BT0 never applies (FEAT_BTI itself: see the 'CFI: BTI' boot line)",
        ],
    );

    #[cfg(target_arch = "x86_64")]
    probe(
        "x86_64 CET-IBT",
        &[
            "cfi-test: SKIP: CET-IBT not enforced at CPL3: CALL to a non-ENDBR64 instruction in executable .text returned",
            "cfi-test: reason: the kernel enables supervisor IBT only (IA32_S_CET); IA32_U_CET.ENDBR_EN is never set (CPU support: see the 'CET-IBT:' boot line)",
        ],
    );

    #[cfg(target_arch = "riscv64")]
    {
        println("cfi-test: SKIP: riscv64 forward-edge CFI (Zicfilp landing pads) is not enabled by the kernel; nothing to probe");
        sys_exit(0);
    }
}
