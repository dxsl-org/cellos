# S2 phase 07b — file-set extension: batched MMIO mapping flush

Recorded 2026-11-05. The owner (the user) answered **"Cho phép"**.

**Defect (pre-existing, made visible by N-CPU bring-up):** `paging::map_mmio_x86_flags` maps an MMIO window page by page through `map_page_x86`, and each page triggers a global TLB shootdown while `FRAME_ALLOCATOR` is held. The platform Cell's 256 MiB ECAM window is 65,536 pages, so boot stalls: 5.5 s on TCG at 2 CPUs, about 90 s on KVM at 4, more than 180 s at 8.

**Added path:** `kernel/src/memory/paging.rs`, limited to `map_mmio_x86_flags` and a non-flushing variant of `map_page_x86` that it uses. Contract: all PTEs are written under the lock, then one `flush_range(start, size)` covers the window before the syscall returns; fail-stop on an unconfirmed tag is unchanged; the Cell cannot touch the window before the syscall returns.

**Review:** the S2 independent race review covers this change. All 38 lock relations keep their whole-plan scope.
