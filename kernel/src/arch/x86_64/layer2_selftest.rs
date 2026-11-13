// SPDX-License-Identifier: MPL-2.0
//! Layer-2 hardware security self-test for x86_64 (PKU); see the aarch64 twin
//! for the shared contract. Runs at boot under `test-hooks`.

// ─── x86_64 PKU self-test ──────────────────────────────────────────────────

/// x86_64 PKU PKRU value and register self-test.
///
/// Verifies:
/// 1. `pkru_for_key(0)` returns 0x00000000 (trusted-core all-access).
/// 2. `pkru_for_key(1)` returns 0x55555550 (allow keys 0+1, deny rest).
/// 3. RDPKRU in kernel mode returns 0 (kernel PKRU = all-access).
///
/// # SKIP conditions
/// - `PKU_ACTIVE == 0` (PKU not enabled — either unavailable or IBT absent).
///
/// # FAIL conditions
/// - Any computed PKRU value deviates from the expected constant.
/// - RDPKRU returns a non-zero value in kernel context.
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
pub fn run_pku_selftest() {
    use crate::hal::pku;

    // SAFETY: PKU_ACTIVE is written once at boot (single-threaded init), then
    // treated as read-only.  No concurrent Rust reference exists after boot.
    if unsafe { pku::PKU_ACTIVE } == 0 {
        puts_x86("[SELFTEST] PKU-SELFTEST: SKIP: PKU not active\n");
        return;
    }

    // Key 0 = trusted-core → PKRU = 0 (all-access).
    let k0 = pku::pkru_for_key(0);
    if k0 != 0x0000_0000 {
        puts_x86("[SELFTEST] PKU-SELFTEST: FAIL: key 0 PKRU wrong\n");
        return;
    }

    // Key 1 = standard Tier-1 Rust cell.
    // All-deny = 0x5555_5555; clear bits [1:0] (key 0 AD), clear bits [3:2] (key 1 AD).
    // Expected: 0x5555_5550.
    let k1 = pku::pkru_for_key(1);
    if k1 != 0x5555_5550 {
        puts_x86("[SELFTEST] PKU-SELFTEST: FAIL: key 1 PKRU wrong\n");
        return;
    }

    // RDPKRU in Ring-0: the kernel always runs with PKRU = 0 (all-access).
    // ECX must be 0; EAX receives PKRU, EDX receives 0.
    let pkru_val: u32;
    // SAFETY: RDPKRU from Ring-0 with ECX=0; PKU_ACTIVE=1 means CR4.PKE is set
    // so this instruction is supported. RDPKRU reads ECX (implicit, must be 0)
    // and writes PKRU to EAX; EDX is cleared.
    unsafe {
        core::arch::asm!(
            "xor ecx, ecx",  // ECX must be 0 for RDPKRU (privileged guard)
            "rdpkru",
            out("eax") pkru_val,
            out("edx") _,
            options(nomem, nostack)
        );
    }
    if pkru_val != 0 {
        puts_x86("[SELFTEST] PKU-SELFTEST: FAIL: kernel PKRU != 0\n");
        return;
    }

    puts_x86("[SELFTEST] PKU-SELFTEST: PASS\n");
}

/// Write `s` to the 16550 UART (x86_64 boot path).
fn puts_x86(s: &str) {
    for c in s.bytes() {
        crate::hal::uart_16550::putchar(c);
    }
}
