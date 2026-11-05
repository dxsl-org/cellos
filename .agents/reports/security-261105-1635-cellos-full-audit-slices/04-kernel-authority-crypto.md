## Slice: KernelAuthorityCrypto — files read: 31 — unsafe blocks reviewed: 22

### Findings

#### [S-1] Snapshot payload integrity is CRC32-only; a disk writer can replay arbitrary kernel RAM — Severity: High — STRIDE: T/E — OWASP: A08
- Where: kernel/src/snapshot.rs:2550-2562 (MAC over header only), 2614-2627 (payload checked by `crc32fast`), 2643-2659 (frames written straight to PA from inventory), 781-785 (`SNAPSHOT_TRUST_KEY` = fixed public constant under `dev-signing-key`), 599-613 (`canonical_bytes`/`signed_bytes`)
- What: `restore_image` authenticates only the 512-byte header (`header_mac` over `signed_bytes()`); the inventory and payload are bound solely through the header's `crc32` field. CRC32 is linear and trivially forgeable, so an attacker who can write the P3 partition can substitute any payload (including the kernel's mutable `.data`/`.bss`/stack/GOT run, see `kernel_image()` 720-744) while keeping the MAC'd header valid. With `dev-signing-key` the HMAC key itself is the literal `b"ViCell-snapshot-epoch-mac-key--1"`, so the header is forgeable too. Replay writes attacker frames to physical RAM before any cell runs. Production path only when `snapshot-qualified` is on (`QUALIFICATION_ENABLED`, line 333; `try_restore` 2934-2940 refuses otherwise) — off in every …
- Exploit: Tamper the snapshot partition on disk: keep header, rewrite payload frames, fix up 4 bytes so CRC32 matches; next boot on a `snapshot-qualified` kernel replays attacker-controlled kernel memory → full kernel compromise.
- Fix: Replace CRC32 with SHA-256 (already in-tree, `sha256.rs`) over inventory||payload and bind that digest in the MAC'd header; drop the constant dev MAC key (derive from a provisioned secret or refuse restore without one).

#### [S-2] "Production" kernel postures do not require the release trust anchor: unsigned cells are admitted while signed ones are refused — Severity: High — STRIDE: S/T/E — OWASP: A05
- Where: kernel/Cargo.toml:71 (default includes `dev-signing-key`), 99 (`release-posture` = release key + signing-required, no `policy-required`), 101 (`production-relay-image` = `policy-required`+`signing-required`, no `release-signing-key`); kernel/src/signing.rs:56-61 (no key → all-zero anchor), 74-76 (`unsigned_admitted()` true unless `release-signing-key`), 68-70 (`signing_required()` has no effect on spawn); kernel/src/loader/governed_spawn.rs:90-108 (unsigned → Tier 2, continues to cap derivation 130-158); scripts/check-production-relay-image.py:23-29,86-89 (requires `production-relay-image`, forbids dev keys, never requires `release-signing-key`)
- What: Built as the checker demands (`--no-default-features --features production-relay-image`), `CELL_SIGNER_PUBKEY` is `[0u8;32]`, so every `__ViCell_sig` fails and the signed artifact is denied (line 72-78), while an ELF with the section stripped is admitted (`unsigned_admitted()` → `is_domain = true`) and still receives `manifest ∩ ceiling ∩ policy` caps. `signing-required` is documented and tested (`elf_tests.rs:780-783`) as "the Tier 1 SAS admission gate" but is a pure label. Conversely `release-posture` leaves `policy-required` off, so an absent `/POLICY.BIN` is dev-permissive (policy.rs:1083-1123). Default dev builds trust the public seed `[0x43]*32` (documented).
- Exploit: On a `production-relay-image` kernel, replace a disk-resident cell binary with an unsigned ELF of the same path; it spawns in Tier 2 with whatever the path's ceiling/policy row grants (e.g. `net_*` rights for a networking client) [INFERENCE: which `/bin/*` paths are disk- vs VIFS1-resident].
- Fix: Make `signing-required` actually deny unsigned (`unsigned_admitted() = !cfg!(signing-required)` or fold `release-signing-key` into both named postures); have `check-production-relay-image.py` require `release-signing-key` and `release-posture` include `policy-required`; `compile_error!` when `signing-required` is set with a zero anchor.

#### [S-3] `SpawnCap` doubles as global service-namespace authority; the USB host driver holds it — Severity: High — STRIDE: S/E — OWASP: A01
- Where: kernel/src/task/syscall.rs:6999-7021 (any `caller_has_spawn` may `RegisterService` any `service_id` → any live tid), 6927-6941 (only `BLOCK_DRIVER`/`MACHINE_PIN` are excluded); kernel/src/cell/service_registry.rs:95-131 (`register` replaces unconditionally), 23-25 (comment assumes "the trusted supervisor owns the namespace"); kernel/src/loader/boot_ceiling.rs:190-194 (`/bin/dwc2-usb` ceiling: `spawn: true`), 136-140
- What: The registry's only guard is `SpawnCap`. `/bin/dwc2-usb` needs `spawn` to launch its LAN9514 child but thereby gains the right to rebind `VFS`, `NET`, `GPU_DRIVER`, etc. to any live tid (including itself). Clients resolve by `service_id` right before sending, so a rebind redirects all subsequent requests (file I/O, sockets) to the attacker. `lookup_bound` reports whatever identity was registered, so even bound lookups follow the rebind.
- Exploit: A hostile USB device compromises the dwc2 driver cell (it parses untrusted descriptors/packets); the cell calls `RegisterService(VFS, own_tid)`; every cell doing `LookupService(VFS)` now talks to the attacker, which can proxy/alter file contents (e.g. the cell-store ELFs other cells spawn).
- Fix: Split "may spawn children" from "may publish service ids": gate `RegisterService` on `supervisor` (or a dedicated registrar cap) and restrict non-supervisor callers to self-registration of ids their ceiling names, as already done for `GPU_DRIVER`/`SILO`.

#### [S-4] StateStash is ungated: 8 MiB per cell of a 32 MiB kernel heap, plus a global key namespace that allows squatting — Severity: Medium — STRIDE: D — OWASP: A04
- Where: kernel/src/cell/state_stash.rs:23-30 (1 MiB × 8 per owner, 64 total), 142-179 (`stash`: refuses a key owned by another incarnation, 158), 60 (single global `u64` key space); kernel/src/task/syscall.rs:8541-8579 (no capability check beyond the `cellbox` argv special-case); kernel/src/main.rs:757-758 (heap = 32 MiB)
- What: Any cell may hold 8 MiB of kernel heap; four cells hold the entire heap, after which every `try_reserve`-based kernel path (signing scratch, spawn, IPC copies) fails with OOM for everyone. Independently, keys are first-come: a cell that writes key K first owns it, and a later legitimate provider's `stash(K)` returns 0, breaking its hot-swap state transfer. Entries survive until retirement.
- Exploit: Four cooperating or compromised unprivileged cells each `StateStash` 8 × 1 MiB; or one cell pre-stashes the well-known keys a service uses before the supervisor's hot-swap.
- Fix: Charge stash bytes to the owner's cell quota (`cell_quota`) instead of the shared heap, and namespace keys by owner incarnation (or require `supervisor` to allocate keys).

#### [S-5] Owner admission gate is a permanent no-op in every production image — Severity: Low — STRIDE: - — OWASP: A05
- Where: kernel/src/admission/gate.rs:58-62 (`!production_enforced` → `Ok(())`), 78-81 (`set_production_enforced` is `#[allow(dead_code)]`), only caller kernel/src/admission/slot_selftest.rs:193,219 (test-hooks); kernel/src/loader/governed_spawn.rs:113-119 calls it on every spawn
- What: `evaluate_owner_admission` always passes outside the self-test; the signed A/B slot parser (`slot_format.rs:57-144`, sound) and owner key (`owner_key.rs`, zero placeholder without dev key) are never wired to a floor backend. The module header admits this (mod.rs:3-5). A reader of `governed_spawn.rs` could believe a third trust anchor is enforced.
- Exploit: None (hygiene); the audit-denied path `encode_u32x2(0, 2)` is unreachable.
- Fix: Either wire the registry to a real backend behind an explicit feature or remove the call from the spawn path until then.

#### [S-6] Crypto/policy boot self-tests fail open — Severity: Low — STRIDE: T — OWASP: A05
- Where: kernel/src/main.rs:1420-1437 (ed25519, policy, signing self-test failures only `log_info`), 1453-1457
- What: If `ed25519::self_test()` or `signing::self_test()` fails (miscompile, backend regression), boot proceeds and the signature gate still decides admissions with a verifier known to be broken (a false-accepting verifier passes the tamper-negative only by chance).
- Exploit: Not attacker-triggerable; latent.
- Fix: `panic!`/refuse boot on self-test failure under `signing-required`/`release-signing-key`.

#### [S-7] Limine memory map handled as a contiguous entry array and silently truncated at 64 — Severity: Low — STRIDE: - — OWASP: -
- Where: kernel/src/boot.rs:204-227 (`from_raw_parts(first_entry_ptr, count)` on `**entries`), 96, 116-121 (`MAX_MEMORY_MAP_ENTRIES = 64`, extra entries dropped with a `log::warn!` before the logger is registered on some arches); contrast kernel/src/boot/limine.rs:604-611 which correctly indexes `entries.add(i)`
- What: The Limine protocol provides an array of pointers; contiguity of the pointed-to entries is an implementation detail. Real UEFI maps commonly exceed 64 entries; usable RAM past the cut is lost silently. Boot input is trusted (no secure boot), so no privilege impact.
- Exploit: n/a.
- Fix: Iterate `*entries.add(i)`; raise the cap or derive the buffer from `entry_count`.

#### [S-8] Full-ELF Ed25519 verify runs with interrupts masked — Severity: Low — STRIDE: D — OWASP: -
- Where: kernel/src/signing.rs:168-191 (`PAYLOAD_SCRATCH.lock()` held across copy + `ed25519::verify`); kernel/src/sync.rs:26-33 (`Spinlock::lock` disables interrupts)
- What: The SHA-512 over the whole ELF (hundreds of KiB) executes on the current hart with IRQs off and serialises every concurrent spawn on all harts. On x86 the comment in fs.rs:71-73 notes TLB shootdowns are only answered at lock acquisition.
- Exploit: Any cell with spawn rights spawning a large artifact repeatedly delays timer/IPI handling on that hart.
- Fix: Verify incrementally (`verify_incremental`/`absorb`) directly over the two ELF slices without the scratch copy, outside any IRQ-masking lock.

#### [S-9] CSpace revoke never reclaims arena slots; closing a parent strands its descendants — Severity: Low — STRIDE: D — OWASP: -
- Where: kernel/src/cell/cspace.rs:591-617 (`revoke_ticket` sets `admission_cut`, slot stays `in_use`), 560-589 (`close_ticket` clears `holder_open`; `reclaim_closed_lineage` stops while `child_count != 0`), 435-443 (`live_slot_index` requires `holder_open`)
- What: Revoked slots consume one of 256 arena entries forever (`Capacity` after 256 revokes); a parent closed while children exist can no longer be named by its ticket, so its subtree can never be revoked. Self-inflicted only (derive is restricted to the own incarnation, 709-725).
- Exploit: A cell exhausts its own arena; no cross-cell impact.
- Fix: Release cut slots once quiesced; allow revoke on a closed-but-live lineage root.

#### [S-10] Hot-swap barrier drops the source's queued async requests, contrary to its comment — Severity: Low — STRIDE: - — OWASP: -
- Where: kernel/src/cell/hotswap.rs:410-414 (`core::mem::take(&mut source.pending_msgs)` into a local), 431-434 (comment: "The source mailbox remains owned by the old task until retirement"), 439 (`cutover_peer`), 458 (local dropped on return)
- What: Async-op messages are filtered out of the copy loop (357) and then dropped with `source_mailbox` at function exit, not retained by the old task. If `PendingMsg` drop does not release associated grants/wire resources, they leak until retirement [INFERENCE — `PendingMsg` Drop not read].
- Exploit: n/a (supervisor-only path).
- Fix: Either put the mailbox back on the source task (as the comment states) or document that drop is the intended release.

#### [S-11] FAT run map follows an oversized/cyclic chain until heap exhaustion — Severity: Low — STRIDE: D — OWASP: -
- Where: kernel/src/fs/fat/runs.rs:43-62 (loop bounded only by `extent.size == 0`, which fatfs emits only past the directory-entry size); kernel/src/fs.rs:52-68 (allocates the full declared size)
- What: A cyclic cluster chain with a large declared size yields non-zero extents indefinitely; `runs` grows via `try_reserve` until `OutOfMemory`. Only the kernel-embedded VIFS1 (`ramdisk`) is parsed here (fat.rs:14,24), i.e. trusted input; the external disk FAT lives in the VFS cell.
- Exploit: Requires modifying the kernel image.
- Fix: Cap the walk at `file.size()` bytes and at the volume's cluster count.

#### [S-12] Audit ring has no consumer and no timestamps outside RISC-V — Severity: Info — STRIDE: R — OWASP: A09
- Where: kernel/src/audit.rs:251-257 (`drain` "has no caller today"), 187-190 (`mtime = 0` on non-riscv), 160-164 (oldest records evicted)
- What: Security events (`CellSignatureFailed`, `PolicyInvalid`, …) live only in a 256 KiB RAM ring and vanish on reset; the security model lists the audit log as "✅ Mitigated" for repudiation.
- Fix: Add a drain path (serial or VFS flusher) and a real clock source per arch.

### Verified-safe areas
- Ed25519: `ed25519-compact 2.3.1` rejects non-canonical `S` (`sc_reject_noncanonical`, ed25519.rs:213), identity/all-zero keys (214-216), invalid points (217-222, 242-243); cofactored check (250) is RFC 8032-compliant. Kernel wrapper never panics (kernel/src/ed25519.rs:12-21).
- SHA-256: padding/length handling correct incl. 56..63-byte remainders (kernel/src/sha256.rs:96-108), FIPS constants and rounds (117-168).
- Cell signature coverage: exactly one `PROGBITS` 64-byte `__ViCell_sig`, in-bounds (`elf.bytes`), header itself inside the signed stream (signing.rs:123-143, 147-191); `extract_sig` tolerates garbage (loader/elf_tests.rs:710-775). OOM is reported, not treated as mismatch (113-115, 178-186).
- Key posture: `dev` ∧ `release` and `release` ∧ `phase02` are build errors (signing.rs:32-33, build.rs:109-118); release key must be non-zero and ≠ dev key (build.rs:124-142).
- Policy: verify-then-parse over `blob[..len-64]` (policy.rs:193-204); parser bounds-checks every read, rejects unknown version/flag bits/out-of-domain region bits and non-0|1 privileged bytes (260-427); `Invalid` → `DenyAll` regardless of posture (1041-1042); `Permit` only intersects (1075); anchor lives in the kernel-embedded VIFS1 (fs.rs:11-18, build.rs:220-223).
- Spawn order: policy and system config load before trusted init (main.rs:986-996, 1603); malformed `system.toml` halts (system_config.rs:22-29, 63-71).
- CSpace: tickets are a monotonic `u64`, `checked_add`, never reused (cspace.rs:496-503); derive requires live parent, `delegable`, rights ⊆ parent, expiry ≤ parent, depth ≤ 8 (458-489, 854-880); destination must be the caller's own live incarnation (709-725); retire removes files and row under one lock (803-815).
- Service registry: `BLOCK_DRIVER` unpublishable, `MACHINE_PIN` only by trusted init naming itself (syscall.rs:6930-6941); provider liveness and identity captured under one `SCHEDULER` hold (7008-7013); paused entries invisible to lookups and compare-and-swap commit (service_registry.rs:162-218).
- State stash: owner/destination binding, supervisor may only probe/abort (state_stash.rs:52-57, 198-230); fallible copy before lock (146-151).
- Hotswap: ceiling reservation bound to `(generation, swap_id, nonce)` (hotswap.rs:143-179); barrier requires `Frozen` source, ready target with matching `hotswap_source_tid`, capacity check, full rollback on failed copy/registry CAS (312-429).
- Snapshot restore: state machine, version, kernel hash, RAM layout and geometry checks precede any allocation; `run_count ≤ frame_count` bounds the inventory buffer; `COMMITTED→CONSUMING` + epoch advance before the first RAM write (snapshot.rs:2476-2546, 2629-2641); default builds clear stale headers and cold-boot (2934-2940).
- Boot: Limine strings/records bounded to bootloader-reclaimable entries (limine.rs:241-, 599-623); null DTB guarded on all fallback paths (boot.rs:496, 564, 694).
- MMIO registry: allowlist + class match, PCIe BAR window sanity (power-of-two, aligned, ≤1 GiB), overlap check, overflow-safe end (resource_registry.rs:358-367, 380-385, 467-525); `Spinlock` masks IRQs so ISR `lookup_mmio_owner` cannot deadlock (sync.rs:26-33).
- Admission slot parser: signature before variable parsing, exact length, ≤256 entries, duplicate rejection (slot_format.rs:57-129).

### Not covered
- kernel/src/p01_measurement.rs beyond its `cellos_p01_trace` gating (syscall.rs:224-225, 2007) — trace-only observer, not read in depth.
- kernel/src/snapshot.rs capture side, quiesce/frozen-window and scratch logic (≈1100-2450, 2700-2930) and its tests (3070-5769).
- Self-test/test modules: admission/{hostile.rs beyond 1-83, slot_selftest, state_selftest, transaction_selftest}, cell/{provenance_selftest, retirement_verdict_tests}, policy.rs:429-1024 and 1262-1732, cspace.rs:1056-1510, state_stash/service_registry/hotswap `mod tests`.
- kernel/src/boot/dtb_memory_ranges.rs; dtb_memory.rs bodies (34-145).
- kernel/src/main.rs 334-700 (paging/ACPI/x86 gate) and 1000-1960 except the self-test and init-spawn blocks cited.
- kernel/src/resource_registry.rs 533-932 (self-tests, display framebuffer, BDF owners) only skimmed by signature; kernel/src/fs/fat.rs 424-547; kernel/src/cell/metadata.rs; cell/registry.rs bodies 43-108.

[Some lines truncated to 768 chars. Use agent://KernelAuthorityCrypto-2:raw:7-7 to read line 7 whole]