//! The only `unsafe` in this crate: loading a known pattern into the whole
//! floating-point register file, yielding, and reading the file back in one
//! `asm!` block.
//!
//! One block is the point: between the load and the read-back the only code that
//! runs is the `Yield` trap and whatever the kernel schedules in the meantime,
//! so a mismatch can only come from the kernel handing this task another task's
//! FP registers. Confined to this file, which is named in
//! `scripts/unsafe-allowlist.toml` (class `test-harness`).

/// `f0`-`f31` raw bit patterns followed by `fcsr`.
pub type FpFile = [u64; 33];

/// Whether this architecture has a per-task FP register file to test.
///
/// aarch64 Cells are softfloat and x86_64 saves its vector state elsewhere;
/// the lane that runs this cell is RV64.
pub const SUPPORTED: bool = cfg!(target_arch = "riscv64");

/// Load `expected` into `f0`-`f31`/`fcsr`, issue one `Yield`, then store the
/// registers into `observed`.
#[cfg(target_arch = "riscv64")]
pub fn yield_with_pattern(expected: &FpFile, observed: &mut FpFile) {
    let yield_id = abi::syscall::ViSyscall::Yield as usize;
    // SAFETY: `t1`/`t2` point at 33-word arrays owned by the caller (shared and
    // exclusive borrows respectively). Every FP register the block writes is
    // declared clobbered, and the `ecall` follows ostd's syscall ABI: `a7` holds
    // the number, `a0`-`a3` the arguments, and only `a0` comes back changed.
    unsafe {
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
            in("t1") expected.as_ptr(),
            in("t2") observed.as_mut_ptr(),
            out("t0") _,
            inlateout("a0") 0usize => _,
            in("a1") 0usize,
            in("a2") 0usize,
            in("a3") 0usize,
            in("a7") yield_id,
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
        );
    }
}

#[cfg(not(target_arch = "riscv64"))]
pub fn yield_with_pattern(expected: &FpFile, observed: &mut FpFile) {
    *observed = *expected;
}
