//! W^X for cell pages: derive the final per-page permissions from the ELF's
//! `p_flags` and apply them once relocation has finished.
//!
//! # Why a second pass exists
//! `.rela.dyn` patching writes into `.text` and `.data.rel.ro`, so every cell
//! page must be mapped WRITE while the loader runs. Leaving WRITE set afterwards
//! means any cell holding one `unsafe` block can rewrite the code of every other
//! cell in the single address space. This module closes the window.
//!
//! # Ordering contract (do not reorder)
//! 1. `elf::load_segments` maps all pages WRITE and records the *target* flags,
//!    OR-ing them across PT_LOADs that share a boundary page.
//! 2. `reloc::apply_relocations` patches through those writable mappings.
//! 3. `enforce` lowers each page to its recorded target flags.
//! 4. Only then may the spawn path register the task with the scheduler.
//!
//! Applying step 3 per-segment instead of per-page would break a page shared by
//! an R-X and an R-- segment: whichever segment was processed last would win
//! and strip the other's rights.
//!
//! Step 4 is what turns "the cell never runs with a writable `.text`" from
//! likely into guaranteed. Registration pushes the task onto a ready queue, from
//! where another hart's work-stealing can start it on its next tick — so
//! lowering *after* registration leaves a window in which a second hart both
//! executes the cell and caches the writable PTE in a TLB that this kernel has
//! no way to shoot down.
//!
//! # What is NOT guaranteed
//! Nothing here invalidates a stale, more permissive translation another hart
//! may still hold for these same VAs from a previous cell that occupied them:
//! the invalidate inside `protect_page` reaches the calling hart only (see
//! [`crate::memory::page_protect`]).
//!
//! # What stays writable
//! Cell stacks, heaps, grant pages, and MMIO windows are mapped by other paths
//! and are untouched here. Kernel writes into cell memory (segment load, warm
//! snapshot restore, AArch64 relocation) go through the physical/HHDM alias,
//! which carries kernel RW independently of the USER mapping's W bit.

use crate::memory::paging::Flags;
use types::{VAddr, ViError, ViResult};

/// Base permission bits every cell page carries regardless of `p_flags`.
///
/// `READ` is unconditional: a PT_LOAD without `PF_R` is legal ELF but yields a
/// page the cell cannot even fetch its own constants from, and no toolchain in
/// this workspace emits one. `ACCESSED`/`DIRTY` are pre-set because the RISC-V
/// spec permits an implementation to fault instead of updating them in hardware.
const BASE: usize = Flags::VALID | Flags::USER | Flags::READ | Flags::ACCESSED | Flags::DIRTY;

/// Final page-table flags for a page covered by a PT_LOAD with these `p_flags`.
///
/// `WRITE` and `EXECUTE` are the only bits the ELF gets to influence. OR the
/// results together when several PT_LOADs share one page.
pub fn page_flags(write: bool, execute: bool) -> Flags {
    let mut bits = BASE;
    if write {
        bits |= Flags::WRITE;
    }
    if execute {
        bits |= Flags::EXECUTE;
    }
    Flags::from_bits(bits)
}

/// Reject a PT_LOAD that declares both `PF_W` and `PF_X`.
///
/// A self-declared W+X segment is the obvious way around this whole pass: the
/// cell asks for writable code and the loader obliges. Refusing it means a cell
/// that genuinely needs a JIT has to change its linker script or manifest in a
/// diff a reviewer sees, rather than winning the permission silently at load
/// time. No cell in this workspace declares W+X.
///
/// # Errors
/// `ViError::PermissionDenied` when both bits are set.
pub fn reject_wx_segment(index: usize, vaddr: VAddr, write: bool, execute: bool) -> ViResult<()> {
    if write && execute {
        log::error!(
            "ELF: rejecting spawn — PT_LOAD #{} at 0x{:X} declares W+X; \
             writable code is refused (split the segment or drop PF_X in the linker script)",
            index,
            vaddr
        );
        return Err(ViError::PermissionDenied);
    }
    Ok(())
}

/// Reject a page whose permissions come out both writable and executable after
/// the boundary-page merge.
///
/// [`reject_wx_segment`] cannot catch this: two PT_LOADs, one R-X and one R-W,
/// are each individually legal yet OR together into W+X when they share a 4 KiB
/// page, and that merge does not exist yet when the per-segment check runs. A
/// linker pads to a page boundary across a permission change, so a merged W+X
/// page means a hand-authored or hostile section layout rather than toolchain
/// output — and in a single address space one such page is a writable,
/// executable region reachable from every cell in the system, which is exactly
/// the primitive this pass exists to remove.
///
/// Dropping WRITE instead of refusing would silently break a legitimate `.data`
/// boundary page; refusal is the only choice that cannot corrupt a valid cell.
///
/// # Errors
/// `ViError::PermissionDenied` when both bits survive the merge.
pub fn reject_wx_page(cell: &str, vaddr: VAddr, flags: Flags) -> ViResult<()> {
    if flags.bits() & Flags::WRITE != 0 && flags.bits() & Flags::EXECUTE != 0 {
        log::error!(
            "[wx] refusing to spawn '{}': page 0x{:X} is W+X after the boundary merge — \
             two PT_LOADs with different permissions share it; align the segments to 4 KiB",
            cell,
            vaddr
        );
        return Err(ViError::PermissionDenied);
    }
    Ok(())
}

/// Lower every loaded cell page from the loader's writable mapping to its final
/// ELF-derived flags.
///
/// Call AFTER `.rela.dyn` has been applied — see the ordering contract above.
///
/// Fail-closed: a page that cannot be lowered aborts the whole pass so the
/// caller kills the half-spawned cell, rather than letting it run with a
/// writable `.text`.
///
/// # Errors
/// - `ViError::PermissionDenied` if any page's merged flags are W+X. The ELF's
///   section layout is malformed or hostile; no page has been touched yet.
/// - `ViError::InvalidInput` if a page recorded at load time is no longer mapped,
///   or the page table rejects the new flags. Both indicate a loader bug, not a
///   malformed ELF, so the message names the page.
#[cfg(not(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm")))]
pub fn enforce(pages: &[(VAddr, Flags)], cell: &str) -> ViResult<()> {
    // Validate the whole set before lowering any of it, so a refused ELF leaves
    // the mapping uniformly writable for the caller to tear down, rather than
    // half-lowered in a state no later code is written to expect.
    for &(va, flags) in pages {
        reject_wx_page(cell, va, flags)?;
    }

    let mut lowered = 0usize;
    for &(va, flags) in pages {
        if flags.bits() & Flags::WRITE == 0 {
            lowered += 1;
        }
        crate::memory::paging::protect_page(va, flags).map_err(|e| {
            log::error!(
                "[wx] '{}' failed to protect page 0x{:X}: {:?} — killing cell",
                cell,
                va,
                e
            );
            ViError::InvalidInput
        })?;
    }
    log::info!(
        "[wx] '{}': {}/{} cell pages lowered to read-only/execute-only",
        cell,
        lowered,
        pages.len()
    );
    Ok(())
}

/// Bare-physical arches (riscv32 Nano, x86_32, arm32) run with no page tables,
/// so there is no permission to lower. Report the gap rather than returning a
/// silent success that reads like enforcement happened.
#[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
pub fn enforce(pages: &[(VAddr, Flags)], cell: &str) -> ViResult<()> {
    log::warn!(
        "[wx] '{}': {} pages left writable — no MMU on this target, W^X unavailable",
        cell,
        pages.len()
    );
    Ok(())
}

// ─── Boot-time self-tests ────────────────────────────────────────────────────
//
// Written as plain `pub fn` + `assert!` rather than `#[cfg(test)]` to match
// `loader::elf_tests`: the kernel is built for bare-metal targets where
// `cargo test` never runs, so a `#[cfg(test)]` module would be neither executed
// nor even type-checked. These compile on every build and are invoked from
// `elf_tests::run_all`.

/// Assert the flag-derivation rules that the whole pass rests on.
///
/// # Panics
/// Panics (halting boot) if any invariant is violated — a wrong flag table is
/// worse than a failed boot because it silently disables the isolation.
pub fn run_self_tests() {
    // .text — R-X: must lose WRITE, keep EXECUTE and USER.
    let text = page_flags(false, true);
    assert_eq!(text.bits() & Flags::WRITE, 0, ".text must not be writable");
    assert_ne!(
        text.bits() & Flags::EXECUTE,
        0,
        ".text must stay executable"
    );
    assert_ne!(text.bits() & Flags::USER, 0, "cell pages stay user pages");

    // .rodata — R--: neither writable nor executable.
    let rodata = page_flags(false, false);
    assert_eq!(rodata.bits() & Flags::WRITE, 0, ".rodata must be read-only");
    assert_eq!(
        rodata.bits() & Flags::EXECUTE,
        0,
        ".rodata must not be executable"
    );
    assert_ne!(rodata.bits() & Flags::READ, 0, ".rodata must be readable");

    // .data — RW-: keeps WRITE.
    let data = page_flags(true, false);
    assert_ne!(data.bits() & Flags::WRITE, 0, ".data must stay writable");
    assert_eq!(
        data.bits() & Flags::EXECUTE,
        0,
        ".data must not be executable"
    );

    // Boundary page shared by .rodata (R--) and .data (RW-) ORs to RW-.
    let merged = Flags::from_bits(rodata.bits() | data.bits());
    assert_ne!(
        merged.bits() & Flags::WRITE,
        0,
        "merged boundary page must satisfy the writable segment"
    );
    assert_eq!(merged.bits() & Flags::EXECUTE, 0);
    assert!(
        reject_wx_page("selftest", 0x4000, merged).is_ok(),
        "an R-- / RW- boundary page is legitimate and must still load"
    );

    // Boundary page shared by .text (R-X) and .data (RW-) ORs to RWX. Neither
    // segment declares W+X, so only the post-merge check can refuse it.
    let hostile = Flags::from_bits(text.bits() | data.bits());
    assert_ne!(hostile.bits() & Flags::WRITE, 0);
    assert_ne!(hostile.bits() & Flags::EXECUTE, 0);
    assert!(
        reject_wx_segment(0, 0x5000, false, true).is_ok()
            && reject_wx_segment(1, 0x5000, true, false).is_ok(),
        "the two contributing segments are individually legal"
    );
    assert!(
        reject_wx_page("selftest", 0x5000, hostile).is_err(),
        "a merged W+X page must fail the spawn, not merely warn"
    );

    // R-X, R-- and RW- pages are the common case and must never be refused.
    assert!(reject_wx_page("selftest", 0x6000, text).is_ok());
    assert!(reject_wx_page("selftest", 0x7000, rodata).is_ok());
    assert!(reject_wx_page("selftest", 0x8000, data).is_ok());

    // W+X is refused; every single-permission combination is accepted.
    assert!(
        reject_wx_segment(0, 0x1000, true, true).is_err(),
        "W+X segment must be rejected"
    );
    assert!(reject_wx_segment(0, 0x1000, true, false).is_ok());
    assert!(reject_wx_segment(1, 0x2000, false, true).is_ok());
    assert!(reject_wx_segment(2, 0x3000, false, false).is_ok());

    log::info!("[wx] self-tests PASSED");
}
