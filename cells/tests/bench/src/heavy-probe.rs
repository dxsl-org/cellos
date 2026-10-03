#![no_std]
#![no_main]
#![forbid(unsafe_code)]

//! Heavy cell for the D5 gate: a large *resident* footprint that stays parked.
//!
//! `docs/roadmap/beam-parity-backend-roadmap.md` §2.3 defines the heavy profile as
//! "heap lớn + grant 16 MiB" and requires the light capacity sweep measured with M
//! of them resident. Both halves are real memory here: the arena below is a
//! declared cell heap that is fully touched, and the grant is kernel-owned frames.
//! The cell never exits — it parks blocked in receive.
//!
//! It is a *separate binary* from `bench-probe` on purpose: the light sweep spawns
//! hundreds of `bench-probe` children, and giving those a 16 MiB arena would make
//! every light cell cost the kernel ~128 KiB of ledger instead of ~6 KB.
//!
//! Note the heap is a *fixed declared arena*, not a growable heap: cell heaps are
//! static regions by design (`ostd::heap`), and the heavy profile needs a large
//! resident region, not growth. Dynamic growth is a separate feature.

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use ostd::syscall::{sys_grant_alloc, sys_recv};

api::declare_syscalls![GrantAlloc, Recv, Log, Exit];
api::declare_manifest!(block_io = false, network = false, spawn = false);

/// Resident heap arena. Every page is touched, so this is frames, not slack.
///
/// The arena is deliberately larger than the region touched: a `try_reserve_exact`
/// of the arena's own size cannot fit (the allocator's block header), and that
/// failure used to take the following log allocation down with it.
const HEAVY_HEAP_BYTES: usize = 20 * 1024 * 1024;
/// Resident heap actually touched, matching the grant.
const HEAVY_TOUCH_BYTES: usize = 16 * 1024 * 1024;
/// Grant size — the roadmap's 16 MiB, the `MAX_GRANT_PAGES` ceiling.
const HEAVY_GRANT_BYTES: usize = 16 * 1024 * 1024;

ostd::declare_custom_heap!(HEAVY_HEAP_BYTES);

ostd::cell_main!(cell_main);

fn cell_main() {
    init_custom_heap();

    let mut heap: Vec<u8> = Vec::new();
    match heap.try_reserve_exact(HEAVY_TOUCH_BYTES) {
        Ok(()) => {
            heap.resize(HEAVY_TOUCH_BYTES, 0);
            for page in (0..heap.len()).step_by(4096) {
                heap[page] = 1;
            }
            ostd::io::println(&format!(
                "[heavy-probe] heap resident: {} MiB touched",
                HEAVY_TOUCH_BYTES >> 20
            ));
            // Park with the arena live: this role never returns.
            core::mem::forget(heap);
        }
        Err(_) => ostd::io::println(&format!(
            "[heavy-probe] heap {} MiB REFUSED",
            HEAVY_TOUCH_BYTES >> 20
        )),
    }

    // The runner's heavy-run guard looks for this exact prefix.
    match sys_grant_alloc(HEAVY_GRANT_BYTES) {
        Some(base) => ostd::io::println(&format!(
            "[heavy-probe] heavy resident: grant={} MiB at {:#x}",
            HEAVY_GRANT_BYTES >> 20,
            base
        )),
        None => ostd::io::println(&format!(
            "[heavy-probe] heavy: grant {} MiB DENIED",
            HEAVY_GRANT_BYTES >> 20
        )),
    }

    let mut rx = [0u8; 64];
    loop {
        rx.fill(0);
        let _ = sys_recv(0, &mut rx);
    }
}
