//! The only `unsafe` in this crate: loading a known pattern into the whole
//! floating-point register file, yielding, and reading the file back in one
//! `asm!` block.
//!
//! One block is the point: between the load and the read-back the only code that
//! runs is the `Yield` trap and whatever the kernel schedules in the meantime,
//! so a mismatch can only come from the kernel handing this task another task's
//! FP registers. Confined to this file, which is named in
//! `scripts/unsafe-allowlist.toml` (class `test-harness`).

use alloc::format;
use alloc::string::String;

/// Words in one register-file snapshot.
///
/// RV64: `f0`-`f31` raw bit patterns, then `fcsr`.
/// AArch64: `q0`-`q31` as (low, high) word pairs, then `FPCR`, then `FPSR`.
#[cfg(target_arch = "riscv64")]
pub const WORDS: usize = 33;
#[cfg(target_arch = "aarch64")]
pub const WORDS: usize = 66;
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
pub const WORDS: usize = 1;

/// One register-file snapshot, 16-byte aligned so the AArch64 block can move
/// Q registers with `stp`/`ldp`.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
pub struct FpFile(pub [u64; WORDS]);

impl FpFile {
    pub const fn zeroed() -> Self {
        Self([0; WORDS])
    }
}

/// Whether this architecture has a per-task FP register file to test.
///
/// RV64 Cells are rv64gc/lp64d; AArch64 Cells run with FP/SIMD untrapped and
/// std Cells are built with `+neon`. x86_64 saves its vector state elsewhere.
pub const SUPPORTED: bool = cfg!(any(target_arch = "riscv64", target_arch = "aarch64"));

/// A value word no other (thread, round) produces: the thread id and round sit
/// in the high bits, the word index in the low byte.
fn value_word(thread: usize, round: u64, index: usize) -> u64 {
    0x7FF0_0000_0000_0000
        | ((thread as u64 + 1) << 44)
        | ((round & 0xFFFF_FFFF) << 8)
        | index as u64
}

/// The pattern `thread` loads in `round`. Control words use only bits the
/// architecture stores for writes: RV64 `fcsr` `frm` 5-7 (a valid mode, 0-4)
/// and `fflags` 0-4; AArch64 `FPCR` RMode/FZ/DN/AHP (22-26) and `FPSR`
/// cumulative flags 0-4 and QC (27). Thread 0 and 1 differ in every word.
pub fn pattern(thread: usize, round: u64) -> FpFile {
    let mut file = FpFile::zeroed();
    #[cfg(target_arch = "riscv64")]
    {
        for (index, word) in file.0.iter_mut().take(32).enumerate() {
            *word = value_word(thread, round, index);
        }
        let frm = ((thread as u64 + round) % 5) << 5;
        let fflags = (round + thread as u64 * 7) & 0x1F;
        file.0[32] = frm | fflags;
    }
    #[cfg(target_arch = "aarch64")]
    {
        for (index, word) in file.0.iter_mut().take(64).enumerate() {
            *word = value_word(thread, round, index);
        }
        let t = thread as u64;
        let rmode = ((t + round) % 4) << 22;
        let fz = (round & 1) << 24;
        let dn = ((round >> 1) & 1) << 25;
        let ahp = (t & 1) << 26;
        file.0[64] = rmode | fz | dn | ahp;
        let flags = (round + t * 7) & 0x1F;
        let qc = ((round + t) & 1) << 27;
        file.0[65] = flags | qc;
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        file.0[0] = value_word(thread, round, 0);
    }
    file
}

/// The register a snapshot word belongs to, for the FAIL marker.
pub fn reg_name(word: usize) -> String {
    #[cfg(target_arch = "riscv64")]
    {
        if word == 32 {
            "fcsr".into()
        } else {
            format!("f{}", word)
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        match word {
            64 => "fpcr".into(),
            65 => "fpsr".into(),
            _ => format!("q{}.{}", word / 2, if word % 2 == 0 { "lo" } else { "hi" }),
        }
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
    {
        format!("w{}", word)
    }
}

/// RV64: load `f0`-`f31`/`fcsr` from `$exp`, issue one `Yield`, then store the
/// registers to `$obs`. Every FP register the block writes is declared
/// clobbered; `$save` is not needed.
#[cfg(target_arch = "riscv64")]
macro_rules! fp_yield_asm {
    ($exp:expr, $obs:expr, $save:expr, $id:expr) => {{
        let _ = $save;
        core::arch::asm!(
            "fld f0, 0(t1)",
            "fld f1, 8(t1)",
            "fld f2, 16(t1)",
            "fld f3, 24(t1)",
            "fld f4, 32(t1)",
            "fld f5, 40(t1)",
            "fld f6, 48(t1)",
            "fld f7, 56(t1)",
            "fld f8, 64(t1)",
            "fld f9, 72(t1)",
            "fld f10, 80(t1)",
            "fld f11, 88(t1)",
            "fld f12, 96(t1)",
            "fld f13, 104(t1)",
            "fld f14, 112(t1)",
            "fld f15, 120(t1)",
            "fld f16, 128(t1)",
            "fld f17, 136(t1)",
            "fld f18, 144(t1)",
            "fld f19, 152(t1)",
            "fld f20, 160(t1)",
            "fld f21, 168(t1)",
            "fld f22, 176(t1)",
            "fld f23, 184(t1)",
            "fld f24, 192(t1)",
            "fld f25, 200(t1)",
            "fld f26, 208(t1)",
            "fld f27, 216(t1)",
            "fld f28, 224(t1)",
            "fld f29, 232(t1)",
            "fld f30, 240(t1)",
            "fld f31, 248(t1)",
            "ld t0, 256(t1)",
            "fscsr t0",
            "ecall",
            "fsd f0, 0(t2)",
            "fsd f1, 8(t2)",
            "fsd f2, 16(t2)",
            "fsd f3, 24(t2)",
            "fsd f4, 32(t2)",
            "fsd f5, 40(t2)",
            "fsd f6, 48(t2)",
            "fsd f7, 56(t2)",
            "fsd f8, 64(t2)",
            "fsd f9, 72(t2)",
            "fsd f10, 80(t2)",
            "fsd f11, 88(t2)",
            "fsd f12, 96(t2)",
            "fsd f13, 104(t2)",
            "fsd f14, 112(t2)",
            "fsd f15, 120(t2)",
            "fsd f16, 128(t2)",
            "fsd f17, 136(t2)",
            "fsd f18, 144(t2)",
            "fsd f19, 152(t2)",
            "fsd f20, 160(t2)",
            "fsd f21, 168(t2)",
            "fsd f22, 176(t2)",
            "fsd f23, 184(t2)",
            "fsd f24, 192(t2)",
            "fsd f25, 200(t2)",
            "fsd f26, 208(t2)",
            "fsd f27, 216(t2)",
            "fsd f28, 224(t2)",
            "fsd f29, 232(t2)",
            "fsd f30, 240(t2)",
            "fsd f31, 248(t2)",
            "frcsr t0",
            "sd t0, 256(t2)",
            in("t1") $exp,
            in("t2") $obs,
            out("t0") _,
            inlateout("a0") 0usize => _,
            in("a1") 0usize,
            in("a2") 0usize,
            in("a3") 0usize,
            in("a7") $id,
            out("f0") _,
            out("f1") _,
            out("f2") _,
            out("f3") _,
            out("f4") _,
            out("f5") _,
            out("f6") _,
            out("f7") _,
            out("f8") _,
            out("f9") _,
            out("f10") _,
            out("f11") _,
            out("f12") _,
            out("f13") _,
            out("f14") _,
            out("f15") _,
            out("f16") _,
            out("f17") _,
            out("f18") _,
            out("f19") _,
            out("f20") _,
            out("f21") _,
            out("f22") _,
            out("f23") _,
            out("f24") _,
            out("f25") _,
            out("f26") _,
            out("f27") _,
            out("f28") _,
            out("f29") _,
            out("f30") _,
            out("f31") _,
            options(nostack),
        )
    }};
}

/// AArch64: load `q0`-`q31`/`FPCR`/`FPSR` from `$exp`, issue one `Yield`, then
/// store the registers to `$obs`.
///
/// The Cell target is softfloat, so the vector registers cannot be named as
/// `asm!` clobbers; the block instead saves the file it found to `$save` and
/// puts it back before it ends, leaving every register it touched as it was.
#[cfg(target_arch = "aarch64")]
macro_rules! fp_yield_asm {
    ($exp:expr, $obs:expr, $save:expr, $id:expr) => {
        core::arch::asm!(
            ".arch_extension fp",
            "stp q0, q1, [x11, #0]",
            "stp q2, q3, [x11, #32]",
            "stp q4, q5, [x11, #64]",
            "stp q6, q7, [x11, #96]",
            "stp q8, q9, [x11, #128]",
            "stp q10, q11, [x11, #160]",
            "stp q12, q13, [x11, #192]",
            "stp q14, q15, [x11, #224]",
            "stp q16, q17, [x11, #256]",
            "stp q18, q19, [x11, #288]",
            "stp q20, q21, [x11, #320]",
            "stp q22, q23, [x11, #352]",
            "stp q24, q25, [x11, #384]",
            "stp q26, q27, [x11, #416]",
            "stp q28, q29, [x11, #448]",
            "stp q30, q31, [x11, #480]",
            "mrs x12, fpcr",
            "str x12, [x11, #512]",
            "mrs x12, fpsr",
            "str x12, [x11, #520]",
            "ldp q0, q1, [x9, #0]",
            "ldp q2, q3, [x9, #32]",
            "ldp q4, q5, [x9, #64]",
            "ldp q6, q7, [x9, #96]",
            "ldp q8, q9, [x9, #128]",
            "ldp q10, q11, [x9, #160]",
            "ldp q12, q13, [x9, #192]",
            "ldp q14, q15, [x9, #224]",
            "ldp q16, q17, [x9, #256]",
            "ldp q18, q19, [x9, #288]",
            "ldp q20, q21, [x9, #320]",
            "ldp q22, q23, [x9, #352]",
            "ldp q24, q25, [x9, #384]",
            "ldp q26, q27, [x9, #416]",
            "ldp q28, q29, [x9, #448]",
            "ldp q30, q31, [x9, #480]",
            "ldr x12, [x9, #512]",
            "msr fpcr, x12",
            "ldr x12, [x9, #520]",
            "msr fpsr, x12",
            "svc #0",
            "stp q0, q1, [x10, #0]",
            "stp q2, q3, [x10, #32]",
            "stp q4, q5, [x10, #64]",
            "stp q6, q7, [x10, #96]",
            "stp q8, q9, [x10, #128]",
            "stp q10, q11, [x10, #160]",
            "stp q12, q13, [x10, #192]",
            "stp q14, q15, [x10, #224]",
            "stp q16, q17, [x10, #256]",
            "stp q18, q19, [x10, #288]",
            "stp q20, q21, [x10, #320]",
            "stp q22, q23, [x10, #352]",
            "stp q24, q25, [x10, #384]",
            "stp q26, q27, [x10, #416]",
            "stp q28, q29, [x10, #448]",
            "stp q30, q31, [x10, #480]",
            "mrs x12, fpcr",
            "str x12, [x10, #512]",
            "mrs x12, fpsr",
            "str x12, [x10, #520]",
            "ldp q0, q1, [x11, #0]",
            "ldp q2, q3, [x11, #32]",
            "ldp q4, q5, [x11, #64]",
            "ldp q6, q7, [x11, #96]",
            "ldp q8, q9, [x11, #128]",
            "ldp q10, q11, [x11, #160]",
            "ldp q12, q13, [x11, #192]",
            "ldp q14, q15, [x11, #224]",
            "ldp q16, q17, [x11, #256]",
            "ldp q18, q19, [x11, #288]",
            "ldp q20, q21, [x11, #320]",
            "ldp q22, q23, [x11, #352]",
            "ldp q24, q25, [x11, #384]",
            "ldp q26, q27, [x11, #416]",
            "ldp q28, q29, [x11, #448]",
            "ldp q30, q31, [x11, #480]",
            "ldr x12, [x11, #512]",
            "msr fpcr, x12",
            "ldr x12, [x11, #520]",
            "msr fpsr, x12",
            in("x9") $exp,
            in("x10") $obs,
            in("x11") $save,
            out("x12") _,
            inlateout("x0") $id => _,
            in("x1") 0usize,
            in("x2") 0usize,
            in("x3") 0usize,
            in("x4") 0usize,
            options(nostack),
        )
    };
}

/// Load `expected` into the FP register file, issue one `Yield`, then store the
/// registers into `observed`.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn yield_with_pattern(expected: &FpFile, observed: &mut FpFile) {
    let yield_id = abi::syscall::ViSyscall::Yield as usize;
    let mut saved = FpFile::zeroed();
    // SAFETY: the three pointers name 16-byte-aligned snapshots of `WORDS`
    // words owned by the caller or this frame (`expected` read-only; `observed`
    // and `saved` exclusively borrowed). Every FP register the block writes is
    // declared clobbered (RV64) or restored from `saved` before it ends
    // (AArch64). The trap follows ostd's syscall ABI: the number in `a7`/`x0`,
    // arguments in `a0`-`a3`/`x1`-`x4`, and only `a0`/`x0` comes back changed.
    unsafe {
        fp_yield_asm!(
            expected.0.as_ptr(),
            observed.0.as_mut_ptr(),
            saved.0.as_mut_ptr(),
            yield_id
        )
    }
}

#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
pub fn yield_with_pattern(expected: &FpFile, observed: &mut FpFile) {
    *observed = *expected;
}
