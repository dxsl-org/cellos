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
//! ## Opt-in
//!
//! The kernel arms forward-edge CFI only for an image whose signed ELF carries
//! the GNU property note (`GNU_PROPERTY_AARCH64_FEATURE_1_AND` BTI /
//! `GNU_PROPERTY_X86_FEATURE_1_AND` IBT; `kernel/src/loader/gnu_property.rs`).
//! The linker emits it only when every object has landing pads, so this cell
//! must be built in its own cargo invocation with
//! `-Z branch-protection=bti` (aarch64) or `-Z cf-protection=branch` (x86_64)
//! in RUSTFLAGS — `scripts/qemu-cfi-test.sh` does that. `build.rs` reports which
//! kind of build this is as `cfg(cfi_landing_pads)`.
//!
//! ## What each outcome proves
//!
//! - **Violation faults** — forward-edge CFI is enforced for this cell. The
//!   cell dies and the kernel log is the evidence, matched against the printed
//!   `cfi_test_unpadded_target` address: on aarch64
//!   `[fault-probe] a64 ... ec=0xd ... pc=<that address>` plus a `[bti] Branch
//!   Target exception` line; on x86_64 `[cet] #CP (vector 21) at CPL3
//!   pc=<that address>` and the `[fault] Cell ... cause=0x15` termination.
//! - **Violation returns** — CFI was not enforced. The verdict depends on
//!   whether it should have been:
//!   - image built without landing pads: `cfi-test: SKIP:` (not opted in; the
//!     kernel runs it exactly like any other cell), exit 0;
//!   - x86_64, CPU does not enumerate IBT (CPUID.(EAX=7,ECX=0):EDX[20] = 0):
//!     `cfi-test: SKIP:` with that reason, exit 0;
//!   - x86_64, CPU enumerates IBT and the image opted in: `cfi-test: FAIL`,
//!     exit 1 — the kernel should have set IA32_U_CET.ENDBR_EN;
//!   - aarch64, image opted in: `cfi-test: SKIP: BTI not enforced` with a
//!     reason, exit 0. EL0 cannot read `ID_AA64PFR1_EL1` (the access traps and
//!     the kernel does not emulate it), so the cell cannot tell a CPU without
//!     FEAT_BTI from a kernel that failed to arm it. The QEMU harness
//!     (`scripts/qemu-cfi-test.sh`) is the authority: it fails the run when the
//!     boot log shows `CFI: BTI enabled` and this line appears.
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

/// What the cell can tell, before probing, about the violation's outcome.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
enum Expect {
    /// The violation must fault; returning is a kernel bug (`FAIL`, exit 1).
    /// The text says why enforcement is expected. Only x86_64 can know this
    /// from inside the cell (CPUID is readable at CPL3).
    #[cfg(target_arch = "x86_64")]
    Fault(&'static str),
    /// The violation cannot fault, or the cell cannot know whether it must:
    /// returning prints these lines (`SKIP`, exit 0).
    Skip(&'static [&'static str]),
}

/// Runs the control call, then the violating call; returns only by exiting.
///
/// `name` labels the mechanism; `expect` decides the verdict when the
/// violation returns instead of faulting.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
fn probe(name: &str, expect: Expect) -> ! {
    let padded: ProbeFn = cfi_test_padded_target;
    let unpadded: ProbeFn = cfi_test_unpadded_target;
    print_fmt(format_args!(
        "cfi-test: {name} probe: landing pad {padded:p}, non-landing-pad target {unpadded:p}\n"
    ));
    match &expect {
        #[cfg(target_arch = "x86_64")]
        Expect::Fault(why) => {
            print_fmt(format_args!(
                "cfi-test: expecting the violation to fault: {why}\n"
            ));
        }
        Expect::Skip(_) => println(
            "cfi-test: the cell cannot assert enforcement here; a returned violation reports SKIP with the reason",
        ),
    }

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
    match expect {
        #[cfg(target_arch = "x86_64")]
        Expect::Fault(why) => {
            print_fmt(format_args!(
                "cfi-test: FAIL: {name} not enforced: the non-landing-pad call returned although {why}\n"
            ));
            sys_exit(1)
        }
        Expect::Skip(lines) => {
            for line in lines {
                println(line);
            }
            sys_exit(0)
        }
    }
}

/// AArch64: the cell cannot read `ID_AA64PFR1_EL1` at EL0, so even an opted-in
/// build can only report a returned violation; the harness decides FAIL from
/// the boot log (see the module docs).
#[cfg(target_arch = "aarch64")]
fn expectation() -> Expect {
    if cfg!(cfi_landing_pads) {
        Expect::Skip(&[
            "cfi-test: SKIP: BTI not enforced for EL0: this image opts in (GNU property BTI), yet BLR to a non-BTI instruction in executable .text returned",
            "cfi-test: reason: the CPU lacks FEAT_BTI or the kernel did not map the image guarded; EL0 cannot read ID_AA64PFR1_EL1 to tell which, so this is a FAIL when the boot log shows 'CFI: BTI enabled' (scripts/qemu-cfi-test.sh)",
        ])
    } else {
        Expect::Skip(&[
            "cfi-test: SKIP: image not opted in: built without -Z branch-protection=bti, so it carries no GNU_PROPERTY_AARCH64_FEATURE_1_AND BTI note",
            "cfi-test: reason: the kernel maps no page of a cell without the note guarded (stage-1 GP); build with scripts/qemu-cfi-test.sh to test enforcement",
        ])
    }
}

/// x86_64: CPUID is readable at CPL3, so the verdict is exact.
#[cfg(target_arch = "x86_64")]
fn expectation() -> Expect {
    // CPUID leaf 7 subleaf 0, EDX bit 20 = CET-IBT (Intel SDM Vol. 2A, CPUID).
    let ibt = core::arch::x86_64::__cpuid(0).eax >= 7
        && (core::arch::x86_64::__cpuid_count(7, 0).edx >> 20) & 1 != 0;
    if !ibt {
        Expect::Skip(&[
            "cfi-test: SKIP: CET-IBT unsupported: the CPU does not enumerate IBT (CPUID.(EAX=7,ECX=0):EDX[20] = 0), so CPL3 indirect branches cannot be tracked",
        ])
    } else if !cfg!(cfi_landing_pads) {
        Expect::Skip(&[
            "cfi-test: SKIP: image not opted in: built without -Z cf-protection=branch, so it carries no GNU_PROPERTY_X86_FEATURE_1_AND IBT note",
            "cfi-test: reason: the kernel leaves IA32_U_CET = 0 for a cell without the note; build with scripts/qemu-cfi-test.sh to test enforcement",
        ])
    } else {
        Expect::Fault(
            "the CPU enumerates CET-IBT and this image carries the GNU_PROPERTY_X86_FEATURE_1_AND IBT note, so the kernel must set IA32_U_CET.ENDBR_EN",
        )
    }
}

fn run_test() -> ! {
    #[cfg(target_arch = "aarch64")]
    probe("aarch64 BTI", expectation());

    #[cfg(target_arch = "x86_64")]
    probe("x86_64 CET-IBT", expectation());

    #[cfg(target_arch = "riscv64")]
    {
        println("cfi-test: SKIP: riscv64 forward-edge CFI (Zicfilp landing pads) is not enabled by the kernel; nothing to probe");
        sys_exit(0);
    }
}
