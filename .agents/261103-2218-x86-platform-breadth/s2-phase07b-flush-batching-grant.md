# S2 phase 07b — file-set extension: batched shootdowns for ELF loading and W^X lowering (reviewer R1)

Recorded 2026-11-05. The owner (the user) chose **option (a)**: extend S2 to batch these flushes before S2 closes.

**Defect (reviewer R1, Important):** ELF segment mapping and W^X permission lowering still issue one global TLB shootdown per page (`tlb_shootdown::flush_range` per page), so boot slows with the CPU count. In the 07b stress loop, KVM 4-CPU boot-to-prompt ranged from 3.6 to 75.6 s, beyond the 45 s timeout of the new `-smp 4` CI lane. One `/bin/vfs` load took 5.7 s and its W^X lowering 4.3 s.

**Added paths:** the per-page map/protect loops of the Cell ELF load and the W^X lowering pass (in `kernel/src/memory/paging.rs` and the loader/task file that drives them, identified by symbol), limited to writing every PTE of a segment or lowering pass and then issuing one `flush_range` per contiguous range. Same contract as the MMIO grant ([record](s2-phase07b-paging-grant.md)): no CPU can use a weakened or new mapping in a way the old per-page flush prevented; permission lowering completes (flush acknowledged) before the Cell can run or the frames are reused; fail-stop on an unconfirmed tag unchanged.

**Review:** an independent race review covers the change before S2 closes. All 38 lock relations keep their whole-plan scope.
