# Cellos — Full STRIDE/OWASP Security + Correctness Audit

**Date:** 2026-11-05 · **Scope:** full (kernel, HAL, cells, libs, tools, scripts, CI) · **Mode:** full (no `--deep`, no `--fix`)
**Method:** 13 parallel read-only slice reviews (per-slice reports in `security-261105-1635-cellos-full-audit-slices/`), top Critical/High claims re-verified by the lead against source, plus `cargo deny check advisories`, tracked-secret scan, and key-material inspection with `openssl`.

✓ Scope: ~800 files read across 13 slices (593k LOC Rust in tree; vendored MuPDF/QuickJS/Lua cores identified and excluded except glue).
✓ Audit: 199 unique findings — **4 critical, 35 high, 71 medium, 71 low, 18 info** (204 raw, 5 cross-slice duplicates merged).

Severity: Critical = exploitable now for kernel compromise / RCE / auth bypass on a shipped lane; High = memory-safety or privilege bug needing a precondition (cap, profile, hardware, lane); Medium = DoS / logic / missing validation; Low = defense-in-depth; Info = hygiene.

---

## 1. Critical

| # | Finding | Where | Lead-verified |
|---|---|---|---|
| C-1 | **aarch64 non-VHE EL2 host runs EL0 Cells with stage-1 translation disabled.** `HCR_EL2 = RW\|TGE` without `E2H` (default `HostRegime::El2NonVhe` for QEMU virt). Per ARM ARM, with `{E2H,TGE}={0,1}` `SCTLR_EL1.M` is treated as 0 for EL0, so the TTBR0_EL1 "mirror" and every AP/UXN bit are inert: Cells run VA=PA with no permission checks → write kernel text/page tables/GICD. | `hal/arch/arm/src/aarch64/boot.rs:204-215`, `backend/qemu_virt.rs:23-26`, `el2.rs:203-244` | ✓ (HCR bits + default regime read; architectural claim per ARM ARM HCR_EL2.TGE) |
| C-2 | **`MapGuestMemory` span overflow maps all host RAM into a guest.** Syscall checks only `ipa+size`; `size.div_ceil(4096)` → `n_pages=2^52`; `n_pages*PAGE_SIZE` wraps to 0 in release (no `overflow-checks`), so SAS/MMIO-hole checks pass with `span=0` and the loop maps consecutive HPAs past the guest carve until frames run out, with no rollback. Needs `HypervisorCap`. | `kernel/src/task/syscall.rs:10099-10114`, `hypervisor/svm_registry.rs:387-393`, `memory/ept.rs:265-288`, `memory/stage2.rs:246-287` | ✓ |
| C-3 | **sshd: USERAUTH always succeeds; channel/exec not gated on auth phase.** `SSH_MSG_USERAUTH_REQUEST` discards user/method and replies `USERAUTH_SUCCESS`. Any peer completing KEX gets the shell. Mitigation today: `assemble-x86-image.py:75` forbids `/bin/sshd` in x86 images; risk is live on any lane that packages it. | `cells/services/sshd/src/session.rs:191-201` | ✓ |
| C-4 | **sshd: constant x25519 "ephemeral" scalar + constant Ed25519 host-key seed + constant KEXINIT cookie; no contributory check.** Session keys derivable from a pcap; host key reproducible from the repo. | `cells/services/sshd/src/kex.rs:29-36, 63-66, 95-102` | ✓ |

## 2. High (grouped)

### 2.1 Kernel — user/kernel boundary and memory safety
| # | Finding | Where |
|---|---|---|
| H-1 | `SpawnFromElf` / `CellSessionV1` route 2 verify the Ed25519 signature over caller-mutable, **unpinned grant memory** (verify-then-load TOCTOU). Lock dropped at 7523, raw borrow at 7545; `aligned_elf::bytes` borrows page-aligned input instead of copying. Another thread / `GrantShare` peer rewrites bytes after `verify_cell` → unsigned code admitted with signed tier/ceiling. ✓ | `kernel/src/task/syscall.rs:7512-7565`, `2182-2205`, `loader/aligned_elf.rs:23-26` |
| H-2 | SAS user-copy output paths (`Recv`, `ReadCap`, `NetRx`, `PipeRead`, …) probe then `commit_sas` with no lease; concurrent `GrantFree`/stack drop remaps the frame kernel-RWX and frees it → kernel writes attacker bytes into reallocated frames (page tables, other cells). Only `write_getrandom_output` holds the lock. | `kernel/src/task/user_copy/copy.rs:86-104,164-183`, `syscall.rs:996-1041, 3072-3085, 9515-9565` |
| H-3 | `PlatformCap` path of `RequestMmio` user-maps **any** physical range via `request_mmio_unchecked` (overlap check only; no RAM/kernel-image refusal). ✓ | `kernel/src/task/syscall.rs:9930-9940`, `resource_registry.rs:380-412`, `memory/paging.rs:815-836` |
| H-4 | Hypervisor syscalls (`RunVcpu`, `VcpuRegs`, `Read/WriteGuestMemory`) take raw pointers through `validate_user_buf` (null/len/wrap only — no user-VA check) and `ptr::write`/`copy_nonoverlapping` them with SUM set → `HypervisorCap` cell gets arbitrary kernel R/W. ✓ | `syscall.rs:2624-2635, 10116-10212`, `hypervisor/registry.rs:521-524, 558-561, 591, 671-672, 729-730` |
| H-5 | Domain grant `publish` races `revoke` (state read outside lock) → receiver root keeps PTE to freed frames. | `kernel/src/task/domain_grant.rs:148-212, 277-302` |
| H-6 | ET_EXEC (non-PIE) images may map any unmapped user VA: null page, other cells' stack guard pages, kernel-stack guard pages, foreign PIE slots. No `VADDR_MIN`. | `kernel/src/loader/elf.rs:46-50, 183-200, 243-252`, `task/stack.rs:218-243` |
| H-7 | aarch64 SMP: outgoing task's `Context` is stealable by a peer hart before the raw switch has saved it (`current_task_id` published under lock, switch after lock drop; outgoing-save guard is a no-op on aarch64). Two harts on one kernel stack. | `kernel/src/task/hart_local/ready.rs:350-366, 650-695`, `task.rs:1665-1682, 1791-1793` |
| H-8 | `smp::online_harts()` never includes hart 0 on riscv64/aarch64 → remote TLB shootdown of hart 0 skipped; hart 0 keeps stale ASID/frame translations while frames are recycled (real on RV64, `sfence.vma` is hart-local). | `kernel/src/task/smp.rs:166-168, 850, 1315, 1416`, `memory/deferred_release.rs:297-311`, `tlb_shootdown.rs:233-240` |

### 2.2 Kernel — authority, signing, snapshot
| # | Finding | Where |
|---|---|---|
| H-9 | **Release kernel ships with public dev trust anchors.** `kernel/Cargo.toml` `default` includes `dev-policy-key` (seed `0x42*32`), `dev-signing-key` (seed `0x43*32`) and `dev-weak-rng`; `release.yml` builds with default features and publishes it. Anyone can sign cells/`POLICY.BIN` for released kernels; `GetRandom` falls back to xorshift32. ✓ | `kernel/Cargo.toml:71`, `.github/workflows/release.yml:38-47`, `scripts/sign-policy.py:57`, `scripts/sign-cell.py:63` |
| H-10 | **`signing-required` is a label**: `unsigned_admitted()` is `!cfg!(release-signing-key)`; `production-relay-image` posture has `[0u8;32]` anchor so signed cells are *refused* while unsigned ELFs are *admitted* to Tier 2 with path-derived caps. `release-posture` omits `policy-required`. ✓ | `kernel/src/signing.rs:56-76`, `loader/governed_spawn.rs:90-108`, `kernel/Cargo.toml:99-101`, `scripts/check-production-relay-image.py:23-29` |
| H-11 | `RegisterService` guarded only by `SpawnCap`; `/bin/dwc2-usb` holds `spawn` (for its LAN child) and can rebind VFS/NET/KMS/SUPERVISOR to any tid. ✓ (merged with ServicesSupervisorApi S-11) | `syscall.rs:6999-7021`, `cell/service_registry.rs:95-131`, `loader/boot_ceiling.rs:190-194` |
| H-12 | Snapshot payload integrity is CRC32 only (MAC covers header only); dev MAC key is a literal constant → disk writer replays arbitrary kernel RAM at boot. Gated behind `snapshot-qualified` (off by default). | `kernel/src/snapshot.rs:781-785, 2550-2562, 2614-2659` |

### 2.3 HAL x86 / hypervisor
| # | Finding | Where |
|---|---|---|
| H-13 | VMX: no TPR shadow, no CR8 exiting → guest `mov cr8,15` masks host interrupts on that CPU forever (spins with `VM_REGISTRY` held, IF=0). | `hal/arch/x86/src/x86_64/vmx_vcpu.rs:188-199` |
| H-14 | SVM: CLGI/STGI/XSETBV/SKINIT/VMLOAD/VMSAVE/INVLPGA/MONITOR/MWAIT not intercepted → `clgi; jmp $` owns the CPU; XCR0 leaks. | `hal/arch/x86/src/x86_64/vmcb.rs:109-114`, `svm_vcpu.rs:355` |
| H-15 | Guest PKRU/XCR0 not restored on exit; host sets `CR4.PKE` on PKU+IBT hardware → kernel faults on first U-page access after exit. | `world_switch.rs:69-250`, `pku.rs:98-129`, `hypervisor/registry.rs:559-561` |
| H-16 | VMX host state (`HOST_GS_BASE`, `HOST_TR_BASE`, `HOST_CR3`) captured once at `create_vcpu` on the creating CPU; running the vCPU elsewhere resumes into the wrong hart state. | `vmx_vcpu.rs` (see slice 05 S-5) |
| H-17 | No `INVEPT` ever executed: EPT remap/unmap not invalidated; teardown frees EPT roots/guest RAM while cached translations persist. | slice 05 S-6 |

### 2.4 HAL aarch64
| # | Finding | Where |
|---|---|---|
| H-18 | Entire BCM2837 peripheral aperture (DMA engine, IRQ controller, timer, EMMC, DWC2) USER-mapped in the shared kernel root; MMIO allowlist is bookkeeping only. Any Cell programs DMA to kernel RAM. Pi 4 EMMC2 likewise. | `hal/arch/arm/src/aarch64/backend/bcm2837.rs:199-210`, `bcm2711.rs:154-157`, `kernel/src/memory/paging.rs:273-291` |

### 2.5 Network
| # | Finding | Where |
|---|---|---|
| H-19 | **Default TLS trust anchor's CA private key is committed.** `service-net` default features embed `roots/private.der` (CN=Cellos Dev CA; fingerprint matches `tools/hypha-mock-llm/ca.pem`, whose key `ca-key.pem` is tracked and "intentionally" so). `check-production-relay-image.py` accepts `tls-ca-private`. **Latent today** — fleet TLS is fail-closed because `tls/clock.rs::observe()` returns `None` without `qemu-dev-https`; becomes a live fleet-wide MITM the moment authenticated time ships. ✓ (merged: NetStack S-3, AuthorityKms S-1, SupplyChain S-2, ScriptEngines S-3) | `cells/services/net/Cargo.toml:19`, `src/tls/roots.rs:70-71`, `tools/hypha-mock-llm/ca-key.pem`, `tls/clock.rs:19-28` |
| H-20 | NIC driver cells (virtio-net, e1000, igb) accept TX/RX/GetMac from any IPC sender → any cell injects/sniffs raw frames. | `cells/drivers/virtio-net/src/main.rs:69-93`, `e1000/src/main.rs:103-127`, `igb/src/main.rs:167-191` |
| H-21 | smoltcp `random_seed` never set → predictable TCP ISNs, DNS IDs/source ports, IP IDs (off-path injection/poisoning). | slice 07 S-5 |

### 2.6 Storage / VFS / block drivers
| # | Finding | Where |
|---|---|---|
| H-22 | FAT `BlockStream` has no partition bound: crafted FAT32 on `/mnt/sd` (P1, writable by all cells) reads/writes P2–P6 (cell table, `/data`, `/srv` KMS/trust stores, cell-store ELFs). | `cells/services/vfs/src/block_stream.rs:51-121` vs `disk_cellosfs.rs:27-46` |
| H-23 | CellosFS `mutate` poisons the whole mount on *any* `Err` (NotFound, NoSpace, name too long); inode numbers never recycled (256 cap, 7 entries/dir) → any cell permanently kills `/srv` or `/data`. | `cells/services/vfs/src/backend_cellosfs.rs:177-211`, `libs/cellos-fs/src/engine.rs:287-334, 627-660` |
| H-24 | NVMe doorbell MMIO unbounded; stride from device `CAP.DSTRD` → writes past the 16 KiB BAR window. | `cells/drivers/nvme/src/controller.rs:144-150, 368-392` |
| H-25 | NVMe trusts Identify-Namespace LBA format (unchecked shift, no 512-byte check, single-page PRP). | `nvme/src/controller.rs:311-319, 510-518` |

### 2.7 Userspace services, SDK, drivers
| # | Finding | Where |
|---|---|---|
| H-26 | `GpuFlush`/`GpuCursor` forward caller `data_ptr` unchecked to the GPU cell (which `from_raw_parts`s it) or the kernel fallback does so itself → any cell with bit 26 paints kernel/other-cell memory to screen or crashes the GPU cell. ✓ | `kernel/src/task/syscall.rs:8367-8412`, `cells/drivers/virtio-gpu/src/display.rs:124-155`, `cursor.rs:21-31` |
| H-27 | Compositor `ATTACH_GRANT` checks surface owner, not grant owner; `reg_id` is a guessable physical base with a `\x01/\x00` oracle → any client displays any victim's framebuffer. | `cells/services/compositor/src/message_handler/grant.rs:18-57`, `syscall.rs:3594-3601` |
| H-28 | Compositor caches raw grant pointer; owner `GrantUnregister` frees pages underneath (no pin on non-VFS `GrantSlice`) → read-after-free painted to screen. | `compositor/src/surface_table/pixels.rs:84-93`, `syscall.rs:3606-3611` |
| H-29 | Keyboard focus is self-service (input service and compositor CREATE/RAISE) → any cell keylogs. | `cells/services/input/src/main.rs:628-639`, `dispatcher.rs:76-127`, `compositor/.../surface.rs:26-31,127-133` |
| H-30 | `ostd::ChannelClient::connect/endpoint` turn an attacker-supplied u64 into `&BiRingChannel` from safe code (atomic writes at arbitrary address). Latent — no in-tree caller. | `libs/ostd/src/ring_channel.rs:167-208` |
| H-31 | `ostd` heap allocator is unsynchronised `static mut` while safe `task::spawn` creates preemptible threads → free-list corruption from safe Rust. | `libs/ostd/src/heap.rs:36-99`, `task.rs:21-48` |
| H-32 | POSIX `net.rs` uses `Recv`'s return value (sender tid) as a byte count and accepts replies from any task. | slice 10 S-5 |
| H-33 | Lua runtime cell loads arbitrary bytecode (`load` mode "bt", `string.dump` present) and has **no `tier`** in its manifest → runs Tier 1 SAS; crafted bytecode = whole-system R/W. | `cells/runtimes/lua/src/main.rs:10, 95-119, 296-307`, `scripts/unsafe-allowlist.toml:97-135` |
| H-34 | Hypha demo: plaintext-HTTP LLM output drives `spawn_cell`/`kill_cell`/`write_file` with no confirmation; `tool-spawn`/`tool-fs` accept `Invoke` from any sender (confused deputy with `SpawnCap`). | `cells/apps/hypha/core/src/main.rs:182-201`, `tools/spawn/src/main.rs:24-69`, `llm-gateway/src/main.rs:45-48` |
| H-35 | Durable-charge superblock integrity is unkeyed SHA-256; `Journal::open` picks highest `sequence` without ledger/genesis cross-check and never re-verifies `head_digest` → rollback/splice by disk writer. Library not yet wired to `budget-authority`. | `libs/durable-charge-journal/src/superblock.rs:55-76`, `journal.rs:45-125` |

## 3. Medium — themes (71; full list in slice reports)

- **Kernel DoS from hostile ELF / user input:** `xmas-elf` unchecked `raw_data` on SHT_NOBITS → panic (02 S-1); `SetTlsBase` non-canonical → ring-0 #GP (01 S-8); `ShmAlloc` unzeroed + unquota'd (01 S-6); Grant alloc/register unquota'd (01 S-7); 16 MiB per-cell kernel-heap quota vs 32 MiB heap (02 S-6); StateStash 8 MiB/cell + global key squatting (04 S-4); async IPC storage pinning ~16 MiB (03 S-4).
- **Kernel races / correctness:** `ElfLoader::unwind` frees PT frames after local-only flush (02 S-3); loader maps pages `USER|W|X` during load window (02 S-4); launch-edge authority keyed by task *name* derived from caller path (02 S-5); `Reply` forces caller `Ready` regardless of state (03 S-3); NET_RX lost-wakeup and global reservation seizable (03 S-5/S-6); cell_session holder/staging exhaustion (03 S-7/S-8); `WaitIrq` no IRQ-ownership on riscv/aarch64 (01 S-11); `BlkReadAsync` DMA into unpinned grant (01 S-9); pipe lost-wakeup on close (01 S-10); domain publish/revoke; IOMMU page tables leak on cell death (10 S-10); PCIe BAR windows registered without RAM-overlap check (10 S-9).
- **HAL:** EL1-host aarch64 vectors panic on any EL0 `udf`/`brk`/misaligned PC (06 S-3); fixed source-visible PAC key shared EL0/EL1 (06 S-4); VMX `inject_irq` on wrong VMCS (05 S-7); guest AVX/FPU state not switched (05 S-8); ACPI/HPET/MADT MMIO bases mapped without checks, FADT u16 overflow (05 S-9/S-10).
- **Network:** httpd exposes whole VFS root + unauthenticated control/AI endpoints (07 S-6); guest MAC-learning hijacks host RX (07 S-7); cross-cell listen-port squatting (07 S-8); unbounded TLS request queues (07 S-9); virtio-net MMIO RX trusts used `id`/`len` → index panic (07 S-10); sshd unbounded buffers (07 S-11); broker `boot_epoch` = uptime (07 S-12).
- **Storage:** unbounded `ReadAsync` table / `read_to_vec` (08 S-5/S-6); CellosFS allocator sized from on-disk field, no cross-inode ownership check (08 S-7/S-8); FAT recursive delete unbounded (08 S-9); virtio-blk poison flag never set (08 S-10); NVMe timeout reuses in-flight `io_buf` (08 S-11); path-keyed ACL gives every cell R/W/D over every other cell's files incl. D03 staging (08 S-13).
- **Services:** supervisor parses stale bytes from uncleared recv buffer → power button/shutdown silently rejected (10 S-1, reproducible without attacker: `hotswap` then `shutdown`); supervisor/config/compositor/DWC2 reply with blocking `sys_send` → one non-receiving client parks a Tier-1 service forever (10 S-2, 11 S-6); `vsnprintf` trailing `%` over-read + `fmt_float` 64-byte overflow (10 S-6); RISC-V `setjmp` omits `fs2–fs11` (10 S-7); `SpscRing` trusts peer `slot.len` (10 S-8); `MmioRegion::read<T>` admits invalid types/unaligned (10 S-16).
- **Compositor/input/USB:** unchecked geometry → wrap/OOM abort (11 S-5); view/frame ops lack caller checks (11 S-7); client-chosen `Panel` role = fake lock screen (11 S-8); HID LED report-count unbounded (11 S-9); LAN9514 RX reparses continuation as headers (11 S-10); WASM fuel preemption never progresses (11 S-11).
- **Authority/KMS:** `verify_protected_successor` vs `open_boot` epoch bump → authority seals on second boot (09 S-3, correctness); pre-auth relay slot exhaustion (09 S-4); signed-time clock starvation (09 S-5); journal last-block edge (09 S-6); `AuthorityGate` slot leak, 16 lifetime cap (09 S-7); budget-authority recovery zeroes and nothing persists (09 S-8).
- **AI/script engines:** AI KV cache unbounded post-admission (13 S-4); `InferEmbed` 2048 passes per turn, no reply (13 S-5); QuickJS budget is instruction-count only (13 S-6); ocel-pdf deadline cooperative only (13 S-7).
- **Supply chain:** `release.yml` uploads nonexistent `vios-kernel`, workspace-wide build, unpinned `git-cliff` with `contents: write` (12 S-3); git dep without `rev`, no `--locked`, no `[sources]` policy (12 S-4); actions on mutable refs (`dtolnay/rust-toolchain@master`) (12 S-5).

## 4. Low / Info — notable
Per-cell ASLR dead (`kaslr_cell_base` unused, deterministic `va_alloc`) (02 S-7); Tier-2 R+X on all non-writable pages, dead `remap_range_user` RWX (02 S-8); kernel identity map RWX and virtio/ECAM windows USER on RISC-V (02 S-12); owner-admission gate permanent no-op in production (04 S-5); crypto/policy boot self-tests fail open (04 S-6); Limine memmap truncated at 64 (04 S-7); full-ELF Ed25519 verify with IRQs masked (04 S-8); SYSRET restores R11 unmasked (05 S-13); no KPTI/IBRS/retpoline posture (05 S-14); host NMI/#MC delivered into guest (05 S-15); MSI vectors unremapped → device owner injects timer/IPI vectors (05 S-17); RV64 `PageTable::map` walks through leaf as table (06 S-6); e1000/igb promiscuous + unzeroed RX pages (07 S-15); xHCI logs every keystroke (11 S-13); serial `baud=0` divide-by-zero (11 S-20); signing seed on argv (09 S-9, 12 S-7); `Dockerfile` `ubuntu:latest` + `curl|sh` + root (12 S-8); `download-limine.sh` no checksum (12 S-10); docs claim weekly libFuzzer runs that no workflow executes (12 S-14); mock LLM proxy binds `0.0.0.0` unauthenticated (13 S-13).

## 5. Dependency audit
`cargo deny check advisories` → **advisories ok**. Three accepted ignores in `deny.toml` (RUSTSEC-2026-0192 ttf-parser unmaintained; RUSTSEC-2023-0071 rsa Marvin — *not encountered in current graph*, entry is stale; RUSTSEC-2026-0387 crypto-bigint non-CT, review by 2027-01-31). `cargo-audit` is not installed locally; `.cargo/audit.toml` ignore list has drifted from `deny.toml` (12 S-12). Vendored engines: QuickJS 2026-06-04, MuPDF 1.26.1 (JPX/JBIG2/ICC decoders on; later 1.26.x carried decoder fixes — `[INFERENCE]`), Lua 5.4.7.

## 6. Secrets / key material (values redacted)
| File | What | Status |
|---|---|---|
| `tools/hypha-mock-llm/ca-key.pem` | P-256 PKCS#8 private key of "Cellos Dev CA" — the default embedded TLS root | **Tracked, must rotate + untrack** (H-19) |
| `cells/services/net/roots/private.der` | X.509 CA *certificate* (not a key) | Public; fine once CA rotated |
| `cells/services/sshd/src/kex.rs` | Constant host-key seed + constant DH scalar | **C-4** |
| `scripts/sign-policy.py:57`, `sign-cell.py:63` | Dev seeds `0x42*32`, `0x43*32` | Documented dev-only, but shipped in release kernel (H-9) |
| `kernel/src/snapshot.rs:781-785` | Literal snapshot MAC key under `dev-signing-key` | H-12 |
| `hal/arch/arm` PAC key | Fixed, source-visible, shared EL0/EL1 | 06 S-4 |
| `kernel/src/embedded-x86_64/kernel_fs.img`, `silo-guest.bin`, `doom1.wad`, `limine/*.bin`, `gh_2.55.0_linux_amd64/`, partial `build/` | Tracked binaries / third-party | Hygiene (12 S-13) |
No cloud/API tokens found; remaining `PRIVATE KEY` hits are test fixtures and parser constants.

## 7. Recommended order of remediation
1. **Trust anchors (H-9, H-10, H-19):** drop `dev-*` features from `kernel` `default`; make `signing-required` deny unsigned; build release with `release-posture` + `policy-required`; untrack/rotate dev CA, have `check-production-relay-image.py` reject the dev root fingerprint.
2. **aarch64 EL2 regime (C-1):** require VHE (`El2Vhe`) or fall back to EL1 host; delete the EL1 mirror path. Fix BCM peripheral USER aperture (H-18).
3. **Hypervisor (C-2, H-4, H-13–H-17):** `checked_mul` span + cap at guest pages + rollback; route guest-memory syscalls through `TaskCopyView`; add CR8/CLGI/XSETBV intercepts; save/restore PKRU/XCR0; per-run host state; `INVEPT`.
4. **sshd (C-3, C-4):** real `publickey` auth with phase enforcement; per-KEX ephemeral from kernel entropy; persisted host key via KMS.
5. **Kernel boundary (H-1–H-8, H-11, H-26):** copy ELF into kernel buffer before verify; lease/pin across SAS output copies; RAM-range refusal in `RequestMmio`; lock across `publish`/`revoke`; `VADDR_MIN` + guard-page registry; aarch64 switch-completion hook; include hart 0 in `online_harts()`; separate `ServiceRegistryCap` from `SpawnCap`; validate `GpuFlush` range and restrict to compositor.
6. **VFS/storage (H-22–H-25):** bound `BlockStream` by partition sectors; poison only on I/O errors + recycle inodes; `MmioRegion`-bounded NVMe doorbells and LBA-format validation.
7. **Compositor/input (H-27–H-29):** grant-owner check, re-resolve grants per blit, focus policy owned by compositor.
8. **SDK (H-30–H-32), Lua tier (H-33), Hypha confirmation (H-34), charge journal MAC (H-35).**

## 8. Not covered
Vendored MuPDF/QuickJS/Lua cores (glue only); `cells/apps` beyond hypha/privileged apps; `cells/demos`, `cells/tests` (treated as non-production); `libs/viui` pure rendering; `tests/`, `.agents/`; `third_party/` (mlibc, llvm-runtimes, redoxfs, sunset, libdrm); `ocel-pdf` C renderer internals beyond `renderer.c`/`libc.c` entry points. Dynamic verification (QEMU repro) was not performed for any finding; all are static-analysis findings with lead re-reads marked ✓.

## 9. Remediation status (2026-11-05, uncommitted working tree)
| Finding | Status |
|---|---|
| H-9, H-10, H-19 | Fixed: `release-posture` adds `policy-required`; `production-relay-image` adds `release-signing-key`; `signing-required` without an anchor is a `compile_error!`; `release.yml` builds `release-posture` with the `CELLOS_RELEASE_CELL_PUBLIC_KEY` secret; dev CA rotated, key untracked, `service-net/build.rs` + relay checker refuse the leaked fingerprint. Old key remains in git history. |
| C-3, C-4, 07 S-11 | Fixed: RFC 4252 `publickey` (ssh-ed25519) against `/srv/sshd/authorized_keys`, phase enforcement, 5-failure lockout, per-connection x25519 + cookie from `GetRandom` (fail closed), contributory check, persisted host key, buffer caps. Not exercised with a live client. |
| C-2, H-4, H-17 | Fixed: checked span math + carve cap + tables-before-leaves (no partial map) in EPT/Stage-2; guest-memory/vCPU syscalls go through `TaskCopyView` with staging; INVEPT after EPT changes, on teardown and before every VMX entry. |
| H-13–H-16, 05 S-7, S-16 | Fixed: CR8 exiting + virtual TPR; SVM intercepts (CLGI/STGI/XSETBV/…/SHUTDOWN); host XCR0/PKRU save/restore; VMX host state refreshed per run; guest CPUID filter. |
| Open | 05 S-8 (guest FPU state), S-15 (NMI exiting), Stage-2 TLB invalidation on ARM remap, `/srv/sshd` keys readable/writable by other cells (needs VFS per-cell identity, slice 08), all other findings. |
