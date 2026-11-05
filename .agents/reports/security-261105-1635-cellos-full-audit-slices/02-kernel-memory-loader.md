## Slice: KernelMemoryLoader — files read: 41 — unsafe blocks reviewed: 47

### Findings

#### [S-1] `xmas-elf` unchecked `raw_data` slice on SHT_NOBITS sections → kernel panic from a hostile ELF — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/loader/elf.rs:419-424 (`get_section` → `find_section_by_name().raw_data()`); callers governed_spawn.rs:14-22 (`__ViCell_syscalls`), governed_spawn.rs:24-34 (`__ViCell_cluster`), elf_prepare.rs:171-175 (`.rela.dyn`); pre-validation gap at manifest_section.rs:37-39 and elf_section.rs:176-178; external: `~/.cargo/registry/.../xmas-elf-0.10.0/src/sections.rs:173-176`.
- What: `manifest_section::classify` is the only structural gate and deliberately skips the byte-range check for `SHT_NOBITS` sections (line 37). `ElfLoader::get_section` then hands the name lookup to `xmas-elf`, whose `raw_data` does `&input[offset..offset+size]` with no bounds check and no overflow check. A NOBITS section named `.rela.dyn` (PIE) or `__ViCell_syscalls`/`__ViCell_cluster` (every ELF) with `sh_offset = len+1` or `sh_offset = u64::MAX, sh_size = 1` (wraps in release) panics; `panic_handler` (main.rs:1841) halts the machine. The `__ViCell_sig` path has its own NOBITS test (elf_tests.rs:767) but `get_section` does not.
- Exploit: Any caller that reaches `spawn_gated` with attacker-controlled bytes: `SpawnFromElf` (syscall.rs:7503-7565) from a cell on a profile that admits the Elf route (cellbox/desktop/tool-spawn/supervisor), or a tampered disk/ramdisk image. Runs after signature verification (governed_spawn.rs:60-109 precede 168), so in `signing-required` posture only signed images trigger it; in unsigned-admitted posture any such caller.
- Fix: Replace `get_section` with `ElfSections` (elf_section.rs) which returns `None` for out-of-range bytes; or in `get_section` reject `ShType::NoBits` and bounds-check `offset+size <= data.len()` with `checked_add` before calling `raw_data`. Also make `classify_inner` fail on any section whose `(offset,size)` is out of range regardless of type.

#### [S-2] Fixed-VA (ET_EXEC) images may map any unmapped user VA: null page, other cells' stack guard pages, kernel-stack guard pages, and other PIE slots — Severity: High — STRIDE: T/E — OWASP: A01
- Where: kernel/src/loader/elf.rs:46-50 (no slot bound when `load_base == 0`), elf.rs:183-200 (only `USER_VADDR_MAX` upper bound), elf.rs:243-252 (collision check = "VA currently resolves"), elf_prepare.rs:104-108 (`elf_is_pie` false → `load_base = 0`, task.rs:2095-2098); guard pages are identity-mapped RAM holes: task/stack.rs:178, 231-243.
- What: For non-PIE images every `p_vaddr` below 2^38 that does not *currently* translate is accepted and mapped `USER|RW(|X)`. Unmapped user-range VAs include (a) VA 0 — mapping it gives the kernel a non-faulting null page (RISC-V runs with `SUM=1`, paging.rs:212-216; x86 SMAP is documented open), (b) every stack guard page, because stacks are identity-mapped physical runs whose bottom frames are simply unmapped (stack.rs:218-243) — this applies to *kernel* stacks too, so a kernel stack overflow silently lands in an attacker-readable frame instead of faulting, (c) any VA inside another cell's reserved-but-sparsely-mapped PIE slot or a not-yet-allocated slot, which makes the next `alloc_cell_va` spawn fail with `PermissionDenied` (slot is returned by `Prepar…
- Exploit: Supply an ET_EXEC ELF via `SpawnFromElf`/tampered image with `PT_LOAD p_vaddr=0` (null page) or `p_vaddr = <victim stack base>` (identity RAM, guessable from `sp`/first-fit allocation order) or `p_vaddr = 0x1_0000_0000 + k*32MiB`.
- Fix: Reject ET_EXEC from untrusted routes (or require `vaddr >= CELL_VA_START` and inside a `va_alloc` reservation for all images); additionally reserve guard-page VAs in a kernel-owned registry (`domain_supervisor_registry` already tracks kernel stacks) and refuse any user mapping that overlaps a registered supervisor/guard range.

#### [S-3] `ElfLoader::unwind` frees pruned page-table frames after a local-only `tlb_flush_all` — Severity: Medium — STRIDE: T — OWASP: A04
- Where: kernel/src/loader/elf.rs:394-401; `tlb_flush_all` is local on riscv64 (`sfence.vma`) and x86_64 (`flush_all_global`) at memory/paging.rs:53-72, contrary to the doc comment at paging.rs:46.
- What: Leaf PTEs are invalidated with the remote-safe `flush_page` (elf.rs:392), but after `prune_empty_tables` the detached intermediate tables are released to the allocator (line 399) with only a local flush (line 397). A remote hart that has the intermediate entry in its page-walk cache (it touched a VA in the same 2 MiB/1 GiB region — any cell on another hart probing the slot) can keep walking a frame that is immediately re-allocated as cell data, interpreting attacker bytes as PTEs. `CellSegments::drop` (stack.rs:563-572) does this correctly with `flush_page`; the same local-only pattern also frees *stack* frames in task/stack.rs:76-87 (`release_frames`: unmap/remap/dealloc, then `tlb_flush_all`) — outside my slice but the same primitive.
- Exploit: SMP boot; attacker cell with `unsafe` on hart B reads the to-be-spawned slot VA while hart A loads a cell that then fails (OOM/W^X/reloc); the pruned table frame is reused.
- Fix: In `unwind`, call `tlb_shootdown::flush_page(page.va)` (or `flush_range` on the pruned region) after `prune_empty_tables` and before `release`, as `CellSegments::drop` does; fix `tlb_flush_all` docs or make it issue remote invalidation.

#### [S-4] Loader maps cell image pages `USER|WRITE(|EXECUTE)` during the load/relocation window — Severity: Medium — STRIDE: T — OWASP: A04
- Where: kernel/src/loader/elf.rs:227-229 (`flags = final | WRITE`, BASE includes `USER` at wx.rs:52), elf.rs:293-298, 270-275; lowered only at elf_prepare.rs:177 via wx.rs:170-184.
- What: Between `map_page` and `wx::enforce` every page of the new cell — including `.text` of a signed, privileged `/bin/*` cell — is user-writable (and user-executable for X segments) in the shared address space. The ELF bytes were hashed/signed in a kernel-only buffer, but the *mapped copy* is modifiable by any concurrently running cell with `unsafe` (Tier-1 FFI cells exist per docs) on another hart during the window, which spans segment copy, relocation and the `atomic_checkpoint`s. Nothing requires `USER` during this window: relocations write via `patch_va` from S-mode/ring 0 (reloc.rs:119-124), which works on supervisor pages.
- Exploit: Cell X on hart B spins writing shellcode at the predictable next slot VA (`CELL_VA_START + idx*stride`, va_alloc.rs:118-123) while init spawns `/bin/vfs` on hart A.
- Fix: Map with `final_flags & !USER | WRITE` (kernel-only) during load; have `wx::enforce` set the final flags including `USER`. Same for the `already_ours` remap.

#### [S-5] Launch-edge authority is keyed by task *name*, and the name derives from a caller-chosen path on the `SpawnFromElf` route — Severity: Medium — STRIDE: S/E — OWASP: A01
- Where: kernel/src/loader/launch_profile/mod.rs:59-102 (`match caller.name`), profiles.rs:88-94 (`desktop_profile`, Path route, `requires_lifecycle_authority=false`), profiles.rs:63-87 (`cellbox` → `/bin/kms-ssh-admin`, `/bin/cellbox-ssh`), targets.rs:80-82 (`/bin/desktop`, `/bin/terminal` are `LEGACY`), mod.rs:111-116 (Elf filter admits LEGACY-only ceilings), governed_spawn.rs:197 (`name = path.rsplit('/')`), governed_spawn.rs:144 (`Spawner::Ceiling` is not intersected with the caller's own caps), syscall.rs:2054-2065, 7524-7539.
- What: A caller whose profile admits the Elf route (cellbox, desktop, tool-spawn, hypha, supervisor) can `SpawnFromElf(arbitrary bytes, "/bin/desktop")` — admitted because `/bin/desktop`'s ceiling is LEGACY-only. The child is then *named* `desktop` and inherits `desktop_profile`: Path-route launches of every reviewed target without SpawnCap, including `/bin/hypervisor` `{hypervisor, net_l2}`, `/bin/net-broker` `{network}`, `/bin/robot-demo` `{network, GPIO}`, with attacker-supplied argv and inherited dir handles (`inherit_from: tid`, spawn_request.rs:56). The ceiling comes from the profile row, not from the caller's authority. `mem_spawn_gate`'s label invariant (mem_spawn_gate.rs:64-74) protects path-keyed sets only; the name-keyed gates (`authorize`, `RAW_…
- Exploit: Compromised `tool-spawn` (hypha's LLM tool, ceiling `spawn:true`) → SpawnFromElf bytes as `/bin/desktop` → child SpawnFromPath `/bin/hypervisor`.
- Fix: Key profiles on a kernel-assigned identity (path + measurement digest, or the `CellOwner` of a path-resolved spawn), never on `task.name`; mark Elf-/Mem-route children with a distinct origin that disables every name-keyed row; intersect `Spawner::Ceiling` with the caller's own `CapSet`.

#### [S-6] Per-cell kernel-heap quota (16 MiB) exceeds half the kernel heap (32 MiB); exhaustion halts the kernel — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/memory/cell_quota.rs:32 (`DEFAULT_QUOTA_BYTES`), cell_quota.rs:316-320 (unregistered row → `usize::MAX`), main.rs:758 (`HEAP_FRAMES = 8192`), memory/heap.rs:171-181 (quota is the only per-cell bound), heap.rs:407-478 (`alloc_error_handler` → `loop { wfi/hlt }`).
- What: The quota bounds kernel-heap bytes charged while a cell is current, but the heap is a single 32 MiB pool shared with the kernel's own infallible allocations (`Vec::push` in scheduler/IPC paths, acknowledged at heap.rs:46-51, elf.rs:112-116). Two cells at quota, or one cell plus normal kernel residency, exhaust it; the next infallible allocation anywhere halts the machine rather than failing that cell. [INFERENCE] which syscalls let a cell drive ~16 MiB of kernel-heap residency (IPC queues, VFS buffers) — not verified in this slice.
- Exploit: Two cooperating cells each retain ~16 MiB of kernel-heap-backed objects.
- Fix: Size `DEFAULT_QUOTA_BYTES × MAX_CELLS` against a reserved kernel-only headroom (e.g. quota pool = heap − kernel reserve, enforced in `charge_owner`), and make the alloc-error path kill the current cell when `current_cell_id() != 0`.

#### [S-7] Per-cell ASLR is not in effect: `kaslr_cell_base` is dead and `va_alloc` is a deterministic bump/free-list — Severity: Low — STRIDE: I — OWASP: A05
- Where: kernel/src/memory/kaslr.rs:61-70 (no callers; `grep kaslr_cell_base` → only definition/docs), kaslr.rs:45-51 and 74-122 (seed = HHDM ^ timer, timer-only on riscv/aarch64), va_alloc.rs:98-123 (`CELL_VA_START + idx*CELL_VA_STRIDE`), paging.rs doc at memory/kaslr.rs:10 claims the loader uses it.
- What: Cell load bases are `0x1_0000_0000 + slot*32 MiB` in spawn order; a hostile cell predicts every other cell's `.text`/`.data` and the next spawn's slot (prerequisite for S-4). `init_kaslr` is only called on x86 (main.rs:225) and its entropy is a free-running counter.
- Exploit: Read neighbour cell data at known VAs (SAS); target S-4 window.
- Fix: Either wire `kaslr_cell_base` (with a real entropy source — virtio-rng is available, elf_prepare.rs:253) into `alloc_cell_va` slot selection, or delete kaslr.rs and correct the docs.

#### [S-8] Tier-2 domain maps every non-writable image page as R+X, and `remap_range_user` (dead) maps user stacks RWX — Severity: Low — STRIDE: T — OWASP: A05
- Where: kernel/src/memory/address_space.rs:1226-1242 (`!is_writable → READ|EXECUTE`), memory/paging.rs:1145-1179 (`VALID|READ|WRITE|EXECUTE|USER`; `grep remap_range_user\(` → no callers).
- What: `create_cell_domain` loses the per-page `final_flags` recorded by the loader: `.rodata` (R--) becomes executable in the private root. `remap_range_user` is an unused RWX+USER mapping helper that any future caller would turn into an executable stack.
- Exploit: ROP/JIT gadget surface in rodata for Tier-2 cells; none for the dead helper.
- Fix: Carry `LoadedPage.final_flags` through `CellSegments` and use them in `create_cell_domain`; delete `remap_range_user`.

#### [S-9] EPT/Stage-2 `map` size math is unchecked — Severity: Low — STRIDE: - — OWASP: A04
- Where: kernel/src/memory/ept.rs:265 and stage2.rs:253,318 (`n_pages as u64 * PAGE_SIZE` wraps for `n_pages ≥ 2^52`), guarded only because all callers pass the carved count (hypervisor/svm_registry.rs:208-216, registry.rs:148, 188).
- What: A wrapped span passes the `checked_add`/SAS checks while the per-page loop still iterates `n_pages` times beyond the carve. Not reachable today (carve fails first), but `map`/`map_mmio_passthrough` are `pub`.
- Exploit: None currently.
- Fix: `n_pages.checked_mul(PAGE_SIZE)` and assert `n_pages <= guest_ram_pages`.

#### [S-10] `load_segments` runs O(pages²) with the frame allocator spinlock (IRQs off) and no size bound for ET_EXEC — Severity: Low — STRIDE: D — OWASP: A04
- Where: kernel/src/loader/elf.rs:243, 262 (`mapped.iter().any/find` per page), elf_prepare.rs:112-116 (lock held across the whole load), elf.rs:45-50 (no bound when `load_base == 0`).
- What: A fixed-VA image with a multi-hundred-MiB BSS performs tens of millions of comparisons plus zeroing with interrupts disabled on that hart before OOM unwinds it; PIE is bounded by the 32/256 MiB slot.
- Exploit: Same route as S-1/S-2.
- Fix: Track `already_ours` via a sorted range or the previous segment's page range; cap total pages for untrusted routes; reject ET_EXEC (S-2).

#### [S-11] Loader does not validate `e_machine`/`EI_CLASS` or that `e_entry` lies inside a loaded page — Severity: Low — STRIDE: - — OWASP: A04
- Where: kernel/src/loader/elf.rs:405-417 (comment admits the machine check is missing), elf_prepare.rs:191 (`entry = header.entry.wrapping_add(load_base)` unchecked), task.rs:2095-2098 (`elf_is_pie` reads `e_type` assuming ELF64 layout).
- What: A foreign-arch or ELFCLASS32 image is mapped and scheduled; an entry outside the image starts the cell in another cell's code (SAS) or faults. Correctness/robustness only.
- Fix: Check `e_machine`, `EI_CLASS == 64`, and `entry ∈ mapped pages ∧ page.final_flags has EXECUTE`.

#### [S-12] Kernel identity map is RWX and boot MMIO windows (virtio, ECAM) are USER for every cell on RISC-V — Severity: Low — STRIDE: E — OWASP: A05
- Where: kernel/src/memory/paging.rs:103-118 (Usable/Kernel RAM mapped `R|W|X`), paging.rs:217-237, 257-265 (`rv_cell_mmio` includes `USER`).
- What: No kernel W^X: any kernel write primitive patches kernel `.text`. Every Tier-1 cell (not just driver cells) can drive virtio-mmio and PCIe ECAM directly; the design relies on `forbid(unsafe_code)` (comment at 212-216). Likely also covered by HalArmRiscv/DriversInputGpu; recorded here because the mappings live in this file.
- Fix: Split kernel sections (text RX, rodata R, data RW) via linker symbols already registered in `domain_supervisor_registry::register_static_image` (domain-supervisor-registry.rs:65-98); map MMIO kernel-only at boot and grant `USER` per cell via `grant_mmio_user` on `RequestMmio` (paging.rs:815-834 already exists).

#### [S-13] Heap owner tag lives in a trailing word past the payload — Severity: Info — STRIDE: - — OWASP: A04
- Where: kernel/src/memory/heap.rs:145-159, 216-220, 313-314.
- What: A one-word heap overrun in any kernel object silently corrupts the quota owner tag; `refund_owner` then credits whatever `(cell_id, generation)` the overrun wrote (cell_quota.rs:358-368 only checks generation equality). Diagnostic, not a boundary.
- Fix: Put the tag in a header before the payload or validate `owner & BYTES_MASK < MAX_CELLS` and mask the generation before trusting it.

### Verified-safe areas
- ELF segment arithmetic: `file_size ≤ mem_size`, `offset+size ≤ data.len()`, `vaddr+mem_size` overflow, page-rounding overflow, copy intersection, per-page `src_end ≤ data.len()` (elf.rs:156-210, 330-353); frame zeroed before copy (elf.rs:319-326); every early return unwinds (elf.rs:127-310).
- PIE extents are bounded to the reserved slot before any allocation; `LoadedPage` Vec reserved fallibly (elf.rs:45-83, 117-121; va_alloc.rs:71-77, const assert 68).
- Relocation engine: entry size/count bounds, `read_unaligned`, every write confined to one owned page with overflow-checked `base±addend` and no cross-page word (reloc.rs:70-97, reloc_target.rs:9-39); applied strictly before `wx::enforce` (elf_prepare.rs:171-177).
- W^X: per-segment W+X refused before allocation (elf.rs:148-154), merged boundary pages refused before any lowering (wx.rs:109-120, 142-144); `protect_page` re-reads the frame from the live table so flags cannot re-point a page (page_protect.rs:58-71, 90-97) and issues remote-safe `flush_page`.
- Bounded section parser used for manifest classification: header, phdr table, shdr table, string table and all PROGBITS ranges checked with `checked_add/mul` and `get(..)` (elf_section.rs:39-125, 131-198); `__ViCell_manifest` NOBITS/duplicate → Malformed (manifest_section.rs:44-46).
- `mem_label` invariants hold for path-keyed gates: final component only, no `/`, `/mem/` prefix, length cap (mem_spawn_gate.rs:75-96); `spawn_from_mem_gated` still runs sig/manifest/admission (governed_spawn.rs:42-128). All launch profiles currently refuse `LaunchRoute::Mem` (profiles.rs:21, 84, 91, 117, 353).
- Frame allocator: double-free detected and logged, not re-marked (frame.rs:194-204, 263-284); index space is the concatenation of managed ranges so holes are unallocatable (frame.rs:132-138); contiguous carves are checked-and-marked under one lock hold (frame.rs:516-543, 1174-1191).
- `flush_page/flush_range`: RV64 `fence` + local `sfence` + synchronous SBI RFENCE to all online remote harts, panic on failure; x86 all-PCID flush + acknowledged per-CPU epoch; AArch64 broadcast TLBI (tlb_shootdown.rs:380-444, 231-253).
- Deferred release is fail-closed: frames are freed only after per-tag epoch acknowledgement; budget exhaustion quarantines and permanently marks the tag; full tables saturate to "unconfirmed" (deferred_release.rs:117-188, 289-299, 380-456, 465-488). `Spinlock::lock` masks IRQs so the timer-ISR reaper cannot deadlock a release path (sync.rs:26-33).
- Private roots: `validate_user_mapping` enforces `< 2^38`, page alignment and W^X (address_space.rs:1476-1484); copy-reader lease vs. ledger-removal ordering is correct (RMW before lock on the reader side, lock before load on the unmapper, address_space.rs:541-554, 618-640, 989-1047); unmap paths never free table frames before confirmed invalidation; x86 copies only the kernel higher half (address_space.rs:391-398).
- Pin registry: fixed tables, per-owner cap, span overflow-checked, `acknowledge` is only reachable from the kernel teardown funnel after device quiesce + IOMMU revoke (pin.rs:322-329, 483-520, 764-825; task.rs:1929-1974).
- Quota counters: generation-tagged CAS, `checked_add` against `min(limit, BYTES_MASK)`, refunds target the charged incarnation (cell_quota.rs:311-368); DMA reservations roll back on drop (402-449).
- EPT/Stage-2: MMIO holes enforced, SAS carve enforced for `map`, device frames bypass only via explicit kernel-only `map_device_frame`, sub-tables and guest RAM freed on `Drop` (ept.rs:258-311, 395-415).
- `child_context::validate`: every field bounds-checked, slots deduplicated, grant kinds whitelisted, `finish()` rejects trailing bytes (child_context.rs:34-153).
- Early loader: table count capped at 64, exact-size fallible allocation, transmutes only over plain integer fields (early.rs:97-115, 180-205; disk_layout.rs:62-84).
- Trap entry switches `satp` back to the kernel root before kernel code runs, so `virt_to_phys` (satp-based) and the loader's collision check consult the kernel root (hal/arch/riscv/src/asm/trap.S:207-210).
- Test/selftest-only code confirmed gated: stack_overflow_probe.rs (`test-hooks`, riscv64), tlb_shootdown_selftest.rs, pin_tests.rs, address_space_tests.rs, elf_tests.rs (boot selftests, not production input paths), atomic_publication_tests (`test-hooks`).

### Not covered
- kernel/src/loader/domain_admission.rs (native-domains policy), boot_ceiling.rs rows 54-241 and selftest.rs, launch_profile/tests.rs, atomic_publication_tests/* — read only as far as the declarations/filters cited above.
- hal/ `PageTable::map/unmap/prune_empty/translate` implementations (riscv/aarch64/x86 walkers) — assumed correct; S-2/S-3 do not depend on them.
- task/stack.rs beyond lines 70-90, 146-253, 540-590; syscall.rs beyond the SpawnFromElf/SpawnFromMem/launch-edge/pin call sites cited; `signing::extract_sig/verify_cell`; `copy_glue`/`validate_user_buf`; `hal::domain::flush_asid`/`smp::request_tlb_flush` epoch machinery.
- heap/p01_owner.rs and all `cellos_p01_trace` instrumentation.

[Some lines truncated to 768 chars. Use agent://KernelMemoryLoader-2:raw:13-13 to read line 13 whole; cut lines run to line 31, each read the same way]