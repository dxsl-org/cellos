# Security audit re-triage (2026-11-21) and fix round (2026-11-22)

Re-triage of [the 2026-11-05 audit](security-261105-1635-cellos-full-audit.md) at `e9dffd987e` (read-only, ten sub-agents). Owner chose batches A–E on 2026-11-22; they landed in `97b86702a9` (A), `583e4303b3` (B), `0ea431240f` (C), `047a06510c` (D), `6248e7f318` (E). Batches F (H-12 snapshot SHA-256), G (x86 NMI/#MC exiting 05 S-15, IR-off DMA 05 S-17, AArch64 06 S-9/S-14), H (Low/Info sweep), dev-only AWS/relay/KMS tooling (09 S-*) and model-dependent AI items stay open. Also open from A–E: 08 S-18b (would break the g2-cow slot-recovery contract), httpd `/api/files` listings by path (empty under a cwd handle), ABI text for NET_RX `PermissionDenied` and the x86 `SetTlsBase` range.

The text below is the re-triage as delivered.

---

I re-triaged all 188 rows at HEAD `e9dffd987e`; 141 are still open. 89 of those are reachable in a shipped image and 52 are latent. Of the rest, 32 are fixed, 3 superseded, 11 not defects, and the Stage-2 remap TLBI is in progress as (d).

The one surprise is that guest FPU state (05 S-8) is already fixed by `add38fa6b2` (2026-11-12): each vCPU now keeps its own FXSAVE image, and I read that code myself. So the open list in `docs/roadmap/current-focus.md` item 6 is out of date. NMI exiting (05 S-15) is still open on both SVM and VMX. H-12 (the snapshot payload is protected only by CRC32) is the one open High, and § 9 never marked it fixed.

**How this was done.** Ten read-only sub-agents each took one or two slice reports and checked the code and git history; nothing was built or run. I spot-checked the main claims myself: 25 of the cited fix commits exist with matching subjects, the release target is RV64 only, and the per-vCPU guest FPU code and the AArch64 FP behaviour below are as described. Anything not read directly is marked [INFERENCE].

**What "shipped image" means here.** `release.yml` builds only the RV64 kernel (`--features release-posture`) and packages no cells. So every Cell-level, x86 or AArch64 finding is reachable only in dev or CI images: x86 headless/desktop from `assemble-x86-image.py`, the RV64 disk from `gen-disk-ci.sh`, the aarch64 disks, and the RPi3 image. `catalog/cells.toml` does not exist at HEAD; cell selection comes from `config/services.toml` and `scripts/generate-boot-config.py`.

**Status codes** used below: OPEN-R = still open, reachable in a shipped image · OPEN-L = still open, latent · NAD = not a defect · SUP = superseded · "(partial)" = part of it was fixed and the rest is open. Short paths: `k/` = `kernel/src/`, `vfs/` = `cells/services/vfs/src/`.

## 1. All items

| ID | Sev | Finding | Status | Evidence at HEAD |
|---|---|---|---|---|
| 01 S-6 | Med | ShmAlloc frames not zeroed, no quota | FIXED | `7d552f0c57`, `4ed0c63bef`; `k/task/syscall/grant.rs:1917-1937` |
| 01 S-7 | Med | Grant alloc/register not charged to the cell | OPEN-R | `grant.rs:417-426` uses `Charge::KERNEL`; Phase 17 left it as an owner-decision residual |
| 01 S-8 | Med | `SetTlsBase` non-canonical value → ring-0 #GP | OPEN-R | `k/task/syscall/spawn.rs:557-578`; `k/arch/x86_64/user_access.rs:93-107` |
| 01 S-9 | Med | `BlkReadAsync` writes into an unpinned grant | OPEN-R | `k/task/syscall/dev.rs:1230-1253` |
| 01 S-10 | Med | Pipe close never wakes peers; lost wakeup | OPEN-R | `k/ipc/pipe.rs:224-249`; `syscall/fs.rs:278-305,352-374` |
| 01 S-11 | Med | `WaitIrq` has no IRQ ownership on riscv/aarch64 | FIXED | `d1092034dc`; `syscall/dev.rs:742-748` |
| 01 S-12 | Low | Unchecked deadline arithmetic | OPEN-R (partial; futex part fixed in `0fff39d2ea`) | `fs.rs:273,342`; `ipc.rs:1244,1255` |
| 01 S-13 | Low | Pipe/serial/Wait/PipeShare hygiene | OPEN-R | `fs.rs:278,426-447`; `dev.rs:703-710`; `ipc.rs:1093-1126` |
| 01 S-14 | Low | Byte-wise guarded copy with IRQs masked | OPEN-R | `user_copy/sas-lease.rs:101-127`; `copy.rs:98` |
| 02 S-1 | Med | xmas-elf `raw_data` on NOBITS → kernel panic | OPEN-R | `k/loader/elf.rs:446-451`; `manifest_section.rs:39-41` |
| 02 S-3 | Med | `unwind` frees page tables after a local-only flush | FIXED | `60e32eac3c`; `elf.rs:412-429` |
| 02 S-4 | Med | Image pages USER\|W(\|X) during the load window | OPEN-R | `elf.rs:230-232,288`; `wx.rs:58` |
| 02 S-5 | Med | Launch authority keyed by task name | FIXED | `7dc57d730b`; `spawn.rs:298-316` |
| 02 S-6 | Med | 64 cells × 16 MiB quota vs a 32 MiB kernel heap | OPEN-R | `k/memory/cell_quota.rs:25,34`; `heap.rs:648-695` (OOM spins the hart forever) |
| 02 S-7 | Low | No per-cell ASLR | OPEN-R (partial) | `ed21726730` randomises slot order, but the only entropy is x86 RDRAND (`k/dev/virtio_rng.rs:52-60`) |
| 02 S-8 | Low | Tier-2 rodata mapped R+X; dead RWX helper | OPEN-R (partial) | `address_space.rs:1265-1281`; `5fabb206fa` removed X, dead fn left at `paging.rs:1251,1437` |
| 02 S-9 | Low | EPT/Stage-2 map size math unchecked | FIXED | `7fea9bfbd5`; `ept.rs:308-315`; `stage2.rs:119-128` |
| 02 S-10 | Low | O(pages²) load with IRQs off | OPEN-R (partial) | `2aa62d0c7c` confines each PT_LOAD to its slot; `elf.rs:256` still scans linearly under the frame lock |
| 02 S-11 | Low | No `e_machine`/`EI_CLASS`/entry check | OPEN-R | `elf.rs:436-438`; `elf_prepare.rs:268` |
| 02 S-12 | Low | Kernel RWX; RISC-V MMIO USER | OPEN-R (partial) | MMIO part fixed in `5fabb206fa`; kernel image still `R\|W\|X` (`paging.rs:166-173`) |
| 02 S-13 | Info | Heap owner tag sits after the payload | OPEN-L | `heap.rs:213-217,312` |
| 03 S-3 | Med | `Reply` forces the caller Ready | SUP | `e88613a26b` retired syscall 3 |
| 03 S-4 | Med | Async IPC storage pins the kernel heap | OPEN-R (partial) | wires charged since `7c9c955871`; `async_ipc.rs:141-144` `Operations` still uncharged |
| 03 S-5 | Med | NET_RX completion lost wakeup | OPEN-R | `completion.rs:547-555`; `completion_wait.rs:481-499` |
| 03 S-6 | Med | Global NET_RX reservation can be seized | OPEN-R | `completion_wait.rs:449-452`; `net_rx_reservation.rs:37-85` |
| 03 S-7 | Med | cell_session members exhaust the holder cap | OPEN-R | `ipc/cell_session.rs:278-298,377-388,424-434` |
| 03 S-8 | Med | Orphaned bootstraps fill the global 256 cap | OPEN-R | `cell_session.rs:956-963,2382-2392` |
| 03 S-9 | Low | `transfer_consumption` target never woken | OPEN-L | no caller (`async_ipc.rs:1000-1039`) |
| 03 S-10 | Med | cpu_accounting seals on Busy → fatal | OPEN-L | built only with `g1-cpu-accounting` |
| 03 S-11 | Info | Hygiene bundle | OPEN-R (partial) | `smp.rs:471`; `decode.rs:1311,1326`; `smp_bringup.rs:124` |
| 04 S-1 (H-12) | **High** | Snapshot payload protected by CRC32 only | OPEN-L | `snapshot_engine.rs:2505-2519,777`; only under `snapshot-qualified`, which no build enables |
| 04 S-4 | Med | StateStash key squatting | OPEN-R (partial) | heap charged since `7c9c955871`; global key space at `cell/state_stash.rs:60,154-162` |
| 04 S-5 | Low | Owner admission gate is a no-op | FIXED | `b78d47f31a`; `admission/gate.rs:24-27` |
| 04 S-6 | Low | Crypto/policy self-tests fail open | OPEN-R | `k/main.rs:480-498` |
| 04 S-7 | Low | Limine memmap truncated at 64 | OPEN-R | `k/boot.rs:103,125-127,215-233` |
| 04 S-8 | Low | Ed25519 verify with IRQs masked | OPEN-R | `k/signing.rs:237-260` |
| 04 S-9 | Low | CSpace revoke strands slots | OPEN-L | no syscall path (`cell/cspace.rs:426-433,557-617`) |
| 04 S-10 | Low | Hot-swap drops queued async requests | NAD | `async_ipc.rs:1169-1199` ends them `PEER_GONE`; only the comment at `hotswap.rs:460` is stale |
| 04 S-11 | Low | FAT run map follows a cyclic chain | OPEN-L | `fs/fat/runs.rs:43-62`; kernel-embedded image only |
| 04 S-12 | Info | Audit ring: no consumer, no timestamps | OPEN-R (partial) | `720b8d1725` added the drop event and drain syscall; nothing calls the drain; `audit.rs:291-294` |
| 05 S-8 | Med | Guest FPU/AVX state not switched | **FIXED** | `add38fa6b2` per-vCPU FXSAVE (`hal/arch/x86/src/x86_64/world_switch/guest_fpu.rs`, `world_switch.rs:69-74`); `7fea9bfbd5` XSETBV/XCR0; guest CPUID hides AVX |
| 05 S-9 | Med | Firmware MMIO bases mapped without checks | OPEN-R | `boot.rs:280-301`; `acpi/hpet.rs:42-44`; hal `x86_64/paging.rs:433` |
| 05 S-10 | Med | FADT u16 port overflow | OPEN-R | `acpi/sci.rs:79-86,167`; `power.rs:495-499` |
| 05 S-11 | Med | virtio-input config reads one byte per word | FIXED | `a17ee22fe0`; `virtio_input.rs:309-319` |
| 05 S-12 | Low | VMX EPT violation has no instruction decode | FIXED | `e6486ea38b`; `vmx_vcpu/mmio_access.rs:31-80` |
| 05 S-13 | Low | SYSRET restores R11 unmasked | OPEN-L | `syscall.rs:421,435`; no user-controlled writer today |
| 05 S-14 | Low | Speculation/entry-hardening posture | NAD | SMEP/SMAP/UMIP on since `8a5a1cf961`; the rest is accepted in `docs/security-model.md:226,589-592` |
| 05 S-15 | Low | **Host NMI/#MC delivered into the guest** | OPEN-R | `vmcb.rs:57-66,158-170`; `vmx_vcpu.rs:171-175,222`; `PIN_NMI_EXITING` unused (`vmcs.rs:328`) |
| 05 S-17 | Low | MSI vectors not remapped | OPEN-R (partial) | interrupt remapping (`c3d22ae14e`) is opportunistic; `pcie_msi/program.rs:46-51` |
| 05 S-18a | Low | Duplicate XSDT entries | OPEN-R | `acpi/sdt.rs:153-190` |
| 05 S-18b | Low | v1 RSDP needs 36 mapped bytes | OPEN-R | `sdt.rs:235`; `acpi/mod.rs:229` |
| 05 S-18c | Info | Two parallel SDT validators | OPEN-R | `sdt.rs:76-110` vs `acpi/mod.rs:187-285` |
| 05 S-18d | Low | Checksum reads any Reserved range | OPEN-R | `boot.rs:181-192`; `sdt.rs:59-87` |
| 05 S-18e | Low | virtio-input eventq ignores `pending_count` | OPEN-R | `virtio_input.rs:161-179,281-284` |
| 05 S-18f | Low | Unthrottled host println on guest paths | OPEN-R | `x86-port-dispatch.rs:48-52`, `virtio_mmio.rs`, `virtio_net.rs` |
| 05 S-18g | Low | 8259 vector u8 overflow | OPEN-R | `pic_8259.rs:38,109` |
| 05 S-18h | Low | Guest TX with any source MAC | FIXED | `45258e920c`; `net/src/bridge-ports.rs:70-131` |
| 05 S-18i | Low | Guest loader unchecked u64 adds | OPEN-R (partial) | `boot_x86.rs:40,138,141`; `loader_image.rs` |
| 05 S-18j | Info | RX/event injection ignores DRIVER_OK | OPEN-R | `virtio_net.rs:82-93`; `virtio_input.rs:145-148` |
| 05 S-18k | Info | `switch_with_root` CR3 doc drift | OPEN-R (doc only) | `context.rs:112-117,247-253` |
| 06 S-3 | Med | EL1 vectors: EL0 udf/brk panics the kernel | FIXED | `3a62423c7f`; `aarch64/trap.rs:418-458` |
| 06 S-4 | Med | Fixed, source-visible PAC key | OPEN-L | `aarch64/cfi.rs:116-140`; PAC-RET is never armed (see § 3) |
| 06 S-5 | Low | Global lower-EL vector marker | FIXED | `3a62423c7f` (marker deleted) |
| 06 S-6 | Low | RV64 `map` walks through a leaf | OPEN-L | `hal/arch/riscv/src/rv64/paging.rs:211-239` |
| 06 S-7 | Low | AArch64 `unmap` missing pre-TLBI barrier | OPEN-R | `hal/arch/arm/src/aarch64/paging.rs:382-388` |
| 06 S-8 | Low | EL2 private-root request silently dropped | FIXED | `bb005d35dc`; `aarch64/context.rs:143-144` |
| 06 S-9 | Low | QEMU-virt world switch skips FP/SIMD | OPEN-R | `aarch64/vcpu.rs:642-689,807-851` (Pi only, `.if {pi}`) |
| 06 S-10 | Low | Stage-2 invariants; VTTBR not reset | FIXED | `3c360497f2` (`k/arch/aarch64/vm.rs:769-771`); `5ab026dbb2` (VMID range) |
| 06 S-11 | Info | Pi monitor restore omits x28/x29 | OPEN-L | `backend/monitor.rs:461-479` |
| 06 S-12 | Info | RV32 trap entry has no stack switch | OPEN-L | `rv32/asm/trap.S:16-24`; RV32 is quarantined (L-8) |
| 06 S-13 | Info | SVC bridge drops `sepc`/a1 | OPEN-L | `aarch64/trap.rs:34-46` |
| 06 S-14 | Low | GICv2 SGI EOI drops the source CPU ID | OPEN-R | `aarch64/gic.rs:115-117,134-136` |
| § 9 ARM | — | Stage-2 TLB invalidation on ARM remap | **IN PROGRESS (d)** | carried residual (d); another agent owns it |
| 07 S-6 | Med | httpd exposes the VFS root and unauthenticated endpoints | OPEN-R (partial) | `06fc502411` denylists only `/srv`, `/etc`, `/.cellos`; `httpd/src/router.rs:48-65` |
| 07 S-7 | Med | Guest MAC learning hijacks host RX | FIXED | `45258e920c`; `bridge-ports.rs:70-131` |
| 07 S-8 | Med | Cross-cell listen-port squatting | OPEN-R | `net/src/handlers/tcp.rs:126-147`; `udp.rs:89-99` |
| 07 S-9 | Med | Unbounded TLS queue / legacy reply ledger | OPEN-R (partial) | `net/src/tls/dispatch.rs:155,193-225` |
| 07 S-10 | Med | virtio-net MMIO RX trusts the used id/len | OPEN-R | `third_party/virtio_drivers/src/device/net/dev.rs:80`; `net_buf.rs:75-77` |
| 07 S-12 | Med | Broker `boot_epoch` is uptime | FIXED | `d9ce91dd37`; `local_runtime.rs:49-77` |
| 07 S-13 | Med | Unwired broker prototypes | OPEN-L | all `#![allow(dead_code)]`; remote path disabled |
| 07 S-14 | Low | Multicast membership not bound to caller | FIXED | `7d564f32dd`; `net/src/handlers.rs:193-231`; `rights.rs:58-61` |
| 07 S-15 | Low | e1000/igb promiscuous RX, stale RX bytes, TX slot reuse | OPEN-R | `e1000/src/controller.rs:281,311-337`; `igb/src/controller.rs:409,468` |
| 07 S-16a | Info | Hardcoded interface MAC | FIXED | `06fc502411`; `net/src/interface.rs:316-330` |
| 07 S-16b | Info | TLS RNG panic | NAD | `61aa32d817` makes it a documented fail-closed boot panic |
| 07 S-16c | Info | httpd JSON built by interpolation | OPEN-R | `httpd/src/handlers.rs:243,245` |
| 07 S-16d | Info | `dev-weak-rng` undetectable by broker | NAD | release refuses it (`k/posture.rs:17-21`) |
| 07 S-16e | Info | Gossip key = K1 XOR constant | OPEN-R | `net-broker/src/beacon.rs:54-61` |
| 07 S-16f | Info | `restart-oracle` accepts any sender | OPEN-L | CI oracle image only |
| 08 S-5 | Med | Unbounded `ReadAsync` table pins `/srv` leases | OPEN-R | `vfs/pending.rs:34-53`; `dispatch.rs:132-151` |
| 08 S-6 | Med | Whole-file `read_to_vec`, no size cap | OPEN-R (partial) | `dispatch.rs:143`; `grant_read.rs:113`; `dispatch_dirs.rs:212`; `grant_write.rs:71-82` |
| 08 S-7 | Med | CellosFS bitmap sized from the on-disk field | OPEN-R | `libs/cellos-fs/src/format.rs:157-185`; `allocator.rs:71-86` |
| 08 S-8 | Med | No cross-inode block ownership check | OPEN-R | `cellos-fs/src/engine.rs:122-127,555-596` |
| 08 S-9 | Med | FAT recursive delete/listing unbounded | OPEN-R | `vfs/backend_fat.rs:319-395` |
| 08 S-10 | Med | virtio-blk poison never set | FIXED | `c7a36575d2`, `f42fc6f694`, lane `5b723d00f4` |
| 08 S-12 | Med | Zero-byte files and dirs bypass quota | OPEN-R | `vfs/paths.rs:88-91`; `dir_ops.rs:29-41`; `backend_ramfs.rs` |
| 08 S-13 | Med | Path-keyed ACL: cross-cell R/W/D incl. save staging | OPEN-R (partial) | `/srv/sshd` narrowed (`1eaed9a010`); `access/rules.rs:86-125` |
| 08 S-14 | Low | One FAT cache per mount, each ¼ of the heap | OPEN-R | `page_cache.rs:28`; `manager/disks.rs:80-90` |
| 08 S-15 | Low | `GetFile` `DataPtr` dangles | FIXED | `b9e22d7f27` |
| 08 S-16 | Low | Guest VM disk image world-readable | OPEN-R | `vfs/access.rs:139-143`; `rules.rs:68-72` |
| 08 S-17 | Low | AHCI success ignores PRDBC | OPEN-R | `ahci/src/controller.rs:504,539-617` |
| 08 S-18a | Low | RamDisk sector math | OPEN-L | `cells/drivers/disk/src/lib.rs:43-58`; never constructed |
| 08 S-18b | Low | One bad superblock slot refuses the mount | OPEN-R | `cellos-fs/src/engine.rs:96-100` |
| 08 S-18c | Low | FAT `Write` replaces an empty directory | OPEN-R | `backend_fat.rs:540-541` |
| 08 S-18d | Low | State deserialize skips schema version | SUP | `8021dbfbe6` deleted it |
| 08 S-18e | Low | Fast-path lock vs blocking main loop | SUP | `03ad765abe` deleted it |
| 08 S-18f | Low | NVMe out-of-range single-sector LBA still issued | OPEN-R (partial) | `nvme/src/dispatch.rs:104-114` |
| 08 S-18g | Low | DirEntry size comment wrong | OPEN-L | `cellos-fs/src/format.rs:360` |
| 08 S-18h | Low | CRC32C is not authentication | NAD | observation; disk tamper is handled by image signing |
| 09 S-3 | Med | Successor check rejects the OpenBoot epoch bump | OPEN-L | `libs/authority-protocol/src/state/persistence/successor.rs:18`; STM32 dev reference only |
| 09 S-4 | Med | Pre-auth relay slot exhaustion | OPEN-L | `tools/relay-server/relay_bootstrap.py:16-67`; relay not deployed (ADR-0009) |
| 09 S-5 | Med | Signed-time clock starvation | OPEN-L | `tools/dev-reference-signed-time/src/allocation.py:108-110` |
| 09 S-7 | Med | `AuthorityGate` slot leak; adoption unchecked | OPEN-L | `durable-charge-journal/src/gates.rs:116-321`; no consumer |
| 09 S-8 | Med | budget-authority recovery zeroes allocations | OPEN-L | crate excluded from the workspace and every image |
| 09 S-9 = 12 S-7 | Low | Signing seed on argv (`--seed-hex`) | OPEN-L (host-only) | `scripts/assemble-x86-image.py:362-365`; `cellos_sign/cli.py:290` |
| 09 S-10 | Low | Signed-time role may PutItem any key | OPEN-L | `template.yaml:193-202` |
| 09 S-11 | Low | Pre-registration DDB reads; no TTL; old cryptography | OPEN-L | `handler.py:125`; `requirements.txt:1` |
| 09 S-12 | Low | Relay slow reader; denylist loaded once | OPEN-L | `relay.py:161-166`; `relay_bootstrap.py:106` |
| 09 S-13 | Low | Authority tooling: no lockfile, no audit, weak seed wipe | OPEN-L | no `authority/*/Cargo.lock`; `bundler.rs:32-43` |
| 09 S-14 | Low | KMS payload-id counter per client instance | OPEN-L | `libs/ostd/src/clients/kms.rs:36,210-216`; `ssh-dev-custody` only |
| 09 S-15 | Low | relay-enroll drops Abort; short chunks accepted | OPEN-L | `relay_enroll.py:117-130` |
| 09 S-16 | Low | Mock LLM `Content-Length` unvalidated | OPEN-L | `tools/hypha-mock-llm/mock_proxy.py:176-221` |
| 09 S-17 | Info | ssh-dev-persist public MAC key | NAD | documented dev posture, refused under `CELLOS_PRODUCTION` |
| 09 S-18 | Info | Dead KMS features not production-refused | OPEN-L | `kms/Cargo.toml:13-16` |
| 09 S-19 | Info | HW journal fault seals; TPM readback; prefix MAC | OPEN-L | `journal-core/src/journal.rs:127-151` |
| 10 S-1 | Med | Supervisor parses a stale recv buffer | FIXED | `92b7504a54`, `f37dffe09b`; `ostd/src/service/serve.rs:219-220` |
| 10 S-2 | Med | Supervisor/Config blocking replies | FIXED | `e793273a70`; `ostd/src/ipc/post.rs:21-94` |
| 10 S-6 | Med | `vsnprintf` trailing `%` / `fmt_float` overflow | OPEN-L | `libs/posix-shim/src/stdio_fmt.rs:258,389-394` |
| 10 S-7 | Med | RV64 setjmp omits fs2–fs11 | OPEN-L | `posix-shim/src/setjmp.rs:12-95` |
| 10 S-9 | Med | PCIe BARs not checked against RAM | OPEN-R (partial) | `k/resource_registry.rs:212-221`; `k/dev/ownership.rs:264-289` |
| 10 S-10 | Med | IOMMU page tables leak on cell death | OPEN-R | `iommu_x86/slpt.rs:14-41`; `iommu_riscv/sv39.rs` (no `Drop`) |
| 10 S-12 | Low | Config store unbounded | OPEN-R | `cells/services/config/src/main.rs:72-97` |
| 10 S-13 | Low | Hotswap restore truncates at 64 KiB | FIXED | `cd98848595` |
| 10 S-14 | Low | Supervisor permit-all syscall allowlist | OPEN-R | `supervisor/src/main.rs:227-234`; `ostd/src/lib.rs:188-201` |
| 10 S-15 | Low | Authorization by task name | OPEN-R (partial) | fixed by `f3cd960727`/`7dc57d730b`; shutdown freeze sweep still stops at 64 (`power.rs:27,47-50`) |
| 10 S-16 | Med | `MmioRegion::read<T: Copy>` admits invalid/unaligned types | OPEN-R | `libs/ostd/src/mmio.rs:62-89` |
| 10 S-17 | Low | Grant byte views race the peer | OPEN-R | `ostd/src/grant.rs:155-207`; `vfs/dispatch_grants.rs:36,124` |
| 10 S-18a | Low | RISC-V PSCID exhaustion panics | OPEN-L | `iommu_riscv.rs:177-187,478`; no RISC-V IOMMU board |
| 10 S-18b | Low | RISC-V stale BDF teardown | NAD | precondition cannot happen (`k/task.rs:1920-1992`) |
| 10 S-18c | Low | AllocPcieIrq always sets Bus Master | OPEN-R | `pcie_msi/program.rs:34-40` |
| 10 S-18d | Low | Unbounded UART drain | OPEN-L | `k/dev/uart.rs:396-452`; `console_drv.rs:102-154` |
| 10 S-18e | Low | memmove compiled into a self-call | NAD [INFERENCE] | `posix-shim/src/strings.rs:29-44` |
| 10 S-18f | Low | ReadDir decodes raw FileType | NAD | the kernel is the only writer (`k/fsapi.rs:175-194`) |
| 10 S-18g | Low | UsbDriverCap takes the NIC role | FIXED | `be7240ee0e` |
| 10 S-19 | Info | Skeleton services, stubbed checks | OPEN-L | in no image |
| 11 S-6 | Med | Compositor/DWC2 blocking replies | FIXED | `e793273a70` |
| 11 S-9 | Med | HID LED report-count unbounded | OPEN-R | `cells/drivers/hid/src/report.rs:323`; `leds.rs:53-71` |
| 11 S-10 | Med | LAN9514 RX limited to one 512 B packet | OPEN-R | `dwc2-usb/src/usb_channel.rs:1401-1421`; `lan9514.rs:638-647` |
| 11 S-11 | Med | WASM fuel preemption never progresses | OPEN-L | `cells/tools/wasm/src/main.rs:77-85`; not packaged |
| 11 S-12 | Low | `vi.recv` allocates before validating | OPEN-L | `cells/drivers/wasm/src/imports.rs:64,81` |
| 11 S-13 | Low | xHCI logs every keystroke | OPEN-R | `xhci/src/main.rs:222-229` |
| 11 S-15 | Low | Hub `bNbrPorts` trusted | OPEN-L | `dwc2-usb/src/hub.rs:210-224` |
| 11 S-16 | Low | Bulk-IN FIFO pops `want/4` words | OPEN-R | `usb_channel.rs:1445` |
| 11 S-17 | Low | Compositor-shared grants never freed | OPEN-R | `grant.rs:748-751`; `surface_table/pixels.rs:56-68` |
| 11 S-18 | Low | Raw-tid keys, fail-open watch, wrapping Rect, global caps | OPEN-R | `compositor/src/main.rs:142-154`; `libs/wire/src/display.rs:40-59` |
| 11 S-19 | Low | Async GpuFlush reads a freed buffer | FIXED | `ebacc8193d` |
| 11 S-20a | Low | PL011 baud divide by zero | OPEN-L | `cells/drivers/serial/src/lib.rs:53-104`; no caller passes a baud |
| 11 S-20b | Low | serial-x86 logs every RX byte | OPEN-R (partial; the "never-ending loop" part cannot happen) | `serial-x86/src/main.rs:154-184` |
| 11 S-21 | Low | `probe_userbit` leaves a frame kernel-only | NAD | `grant.rs:1363-1393,1541-1566` |
| 11 S-22a | Info | Input probes VirtIO before claiming | FIXED | `0fff39d2ea` |
| 11 S-22b | Info | GetFocus discloses the focused tid | OPEN-R | `cells/services/input/src/main.rs:653-662` |
| 11 S-22c | Info | Compositor dead length checks | OPEN-L | unsafe claim fixed (`e267c381ba`); dead checks remain |
| 11 S-22d | Info | Unmapped keys raise a spurious Lost(3) | OPEN-R | `input/src/session.rs:3325-3359` |
| 11 S-22e | Info | virtio-gpu `flush_rect` row shear | FIXED | `62b828e146` |
| 11 S-22f | Info | intel-gpu-core / drm-cellos link-only | NAD | informational |
| H-28 residual | High | Compositor resolve→copy race | FIXED | `3a62423c7f` GrantSlice LEASE + `0e83419e59` |
| 12 S-3 | Med | Release workflow broken | FIXED | `7d995b9916`, `d32492e157` |
| 12 S-4 | Med | Git dep without `rev`, no `--locked` | FIXED | `c3d22ae14e`, `06fc502411`, `d32492e157` |
| 12 S-5 | Med | Mutable action refs; no workflow `permissions` | OPEN-R (partial) | actions SHA-pinned; 5 of 6 workflows have no top-level `permissions:` |
| 12 S-6 | Low | `ref_name` interpolated into `run:` | OPEN-R | `ci.yml:195` |
| 12 S-8 | Low | Root Dockerfile unpinned, runs as root | OPEN-L (partial) | `Dockerfile:7,18-19` |
| 12 S-9 | Low | KubeVirt `:latest` tag | OPEN-L | `deploy/kubevirt/cellos-vm.yaml:36` |
| 12 S-10 | Low | Limine binaries unverified | OPEN-R | `scripts/download-limine.sh`; `x86/make-iso-ci.sh:71-74` |
| 12 S-11 | Low | Unpinned installs in CI/dev setup | OPEN-R (partial) | `ensure-ci-python-deps.py:40` (no hashes); `dev-setup.sh:72` |
| 12 S-12 | Info | `audit.toml`/`deny.toml` drift | FIXED | `c76f426626`, `19dc52ea99` |
| 12 S-13 | Info | Tracked binaries | OPEN-L | `gh_2.55.0_linux_amd64/`, `doom1.wad`, `silo-guest.bin`, `build/` |
| 12 S-14 | Info | Doc claims weekly fuzzing | OPEN-R (doc) | `docs/security-model.md:637`; no workflow runs `fuzz/` |
| 13 S-4 | Med | AI KV cache unbounded | OPEN-L | `libs/ai-engine/src/lib.rs:418-433`; the shipped model is tiny |
| 13 S-5 | Med | `InferEmbed` unbudgeted, may not reply | OPEN-L | `ai-engine lib.rs:885-903`; `ai/src/main.rs:383` |
| 13 S-6 | Med | QuickJS budget is instruction-count only | OPEN-R | `ocel-quickjs/src/engine.rs:37-51` |
| 13 S-7 | Med | ocel-pdf deadline cooperative only | OPEN-R | `ocel-pdf/src/c/renderer.c:176` |
| 13 S-8 | Low | GGUF bool array sized 8 B | OPEN-L | `libs/gguf-rs/src/lib.rs:902` |
| 13 S-9 | Low | Quadratic GGUF metadata dedup | OPEN-L | `gguf-rs lib.rs:346-352` |
| 13 S-10 | Low | ocel-pdf `Open{path}` confused deputy | OPEN-R | `ocel-pdf/src/main.rs:42-46,90-99` |
| 13 S-11 | Low | Token drain loses tokens / no reply | OPEN-L | `ai-engine lib.rs:820-836` |
| 13 S-12 | Low | Special-token scan O(n²); token injection | OPEN-L | `libs/ai-tokenizer/src/lib.rs:462-500` |
| 13 S-13 | Info | Mock LLM binds 0.0.0.0 | OPEN-L | `mock_proxy.py:37` |
| 13 S-14 | Info | Stale QuickJS heap comment | OPEN-L | `ocel-quickjs/src/main.rs:38-39` |

## 2. Details for still-open items, by severity

Abbreviations: **KHT** = `cargo test -p cellos-kernel --features compile-only --target x86_64-unknown-linux-gnu`; **VHT** = `cargo test -p service-vfs --target x86_64-unknown-linux-gnu`. Sizes: S < ~40 LOC, M ~40–120 LOC, L larger.

### High
| ID | Consequence | Reachability | Minimal fix (size) | Verify |
|---|---|---|---|---|
| 04 S-1 (H-12) | Anyone who can write the disk swaps the snapshot payload, fixes up the CRC, and has it replayed into kernel RAM. | Latent: needs `snapshot-qualified`, which no workflow, board or script enables, plus the dev MAC key. | SHA-256 over inventory and payload, bound inside the MAC'd header; provisioned MAC key instead of the constant (M, ~60) | Engine host test: flip a payload byte and fix up the CRC → restore returns ColdBoot |

### Medium — reachable
| ID | Consequence | Reachability | Minimal fix (size) | Verify |
|---|---|---|---|---|
| 02 S-6 | A few cells fill the 32 MiB kernel heap and the next kernel allocation spins the hart forever. | Every image and arch; any 2–4 cells (e.g. StateStash 8 MiB each). | Kernel-only heap reserve in `cell_quota::charge_owner`/`heap.rs`; clamp the default quota (M, ~30) | Heap host test; QEMU fixture: 4×8 MiB stashes, then a spawn still succeeds |
| 02 S-1 | A crafted NOBITS section offset panics the kernel. | Dev images, all arches, via SpawnFromElf with an unsigned ELF; release needs release-signed bytes. | Bounds-checked section lookup in `ElfLoader::get_section`; range-check every section in `manifest_section` (S, ~25) | KHT `loader::` case with NOBITS `sh_offset=u64::MAX` |
| 01 S-8 | Any cell faults the kernel with WRMSR #GP in ring 0. | x86 dev/desktop images; the syscall needs no authority. | Canonical/user-range check in `set_tls_base` (S, ~6) | ABI refusal fixture (`scripts/x86/abi_refusal_fixtures.py`) + unit test |
| 03 S-8 | ~256 create/bootstrap/close loops fill the global cap; every cell's `bootstrap()` then fails. | All images; any cell. | `bootstraps.retain(...)` in `remove_operation`; per-controller cap (S, ~6) | `cell_session` test: 300 cycles, then a fresh bootstrap succeeds |
| 03 S-7 | Any member uses up an operation's 64 holders, so the owner's own calls get QuotaExceeded. | All images; any cell (no allowlist bit). | Selectors 2/10 require owner or DELEGATE, or cap per (operation, actor) (S, ~15) | `cell_session` test |
| 03 S-6 | Any WaitCompletion holder steals NET_RX from the NIC; networking stalls. | All images; bit-42 holders and network apps. | Accept NET_RX only from the live NIC/NET provider; displace only a dead owner (S–M, ~40) | `net_rx_selftest` refusal marker + igb DHCP lane |
| 03 S-5 | An RX completion between drain and park is dropped; added RX latency up to the deadline. | SMP images, all arches; ordinary traffic. | Re-check the queue in `publish_wait_state_locked`, or re-raise the wake for a Running tid (S, ~15) | `completion_selftest` with an injected race, RV64 `--harts 2` |
| 01 S-10 | Pipe peers hang forever (no EOF on close; lost write wakeup). | All images; any pipe user. | Wake waiters in `close_end`; re-check under PIPES before parking (S, ~30) | Pipe state-machine host test; `qemu-phase03-pipeline-test.sh` |
| 01 S-7 | Uncharged 16 MiB grant runs starve other spawns. | All images incl. the RV64 release kernel; GrantAlloc/Register holders. | `Account::Memory` reservation in `grant_alloc`/`grant_register`, refunded on free (S–M, ~40) | KHT quota/refund test; grant negative self-test |
| 01 S-9 | PIO sector copy can land in a reallocated frame. | SDHCI board kernels only; BlockIoCap (VFS). | Pin the span across `read_sector`, or retire syscall 212, which has no cell caller (S, ~15) | Host test: GrantFree refused while pinned; raspi3b gate |
| 02 S-4 | A concurrent Tier-1 cell can plant code in a loading image before W^X. | All arches, SMP; needs a compromised Tier-1 SAS cell. | Map the load window `(final & !USER) \| WRITE` (S, ~6) | wx/elf test asserting no USER in the window; x86 + RV64 2-hart lanes |
| 03 S-4 | Residual: per-task `Operations` Vec grows uncharged (~0.3 MiB per 32-thread cell [INFERENCE]). | All images; IpcSubmit holders. | `Charge::of(task)` at `async_ipc.rs:142` (S, ~5) | `async_ipc` cfg(test) case |
| 04 S-4 | First writer squats a stash key and breaks hot-swap transfer; 8 cells fill the table. | All images; syscall-410 holders. | Key by (owner incarnation, key) until Seal (M, ~40) | `state_stash` host test |
| 10 S-9 | A hostile device or compromised Platform Cell gets a BAR aliasing RAM user-mapped. | All arches, release and dev. | Refuse BARs overlapping managed RAM, the kernel image or ECAM in `register_pci_bar`, reusing the ECAM check (S, ~25) | `resource_registry_tests.rs` + nic/virtio-x86 lanes |
| 10 S-10 | Every DMA-domain teardown leaks table pages; repeated restarts panic on the `alloc_page` assert. | x86 with VT-d (release on DMAR hardware; CI VT-d lanes). | `Drop`/`free_tables` for `VtdSlpt` and `Sv39IommuPt` after an acknowledged teardown; return Rejected instead of asserting (M, ~60) | Host test with flat heap over N cycles; `iommu-dmar-x86` restart loop |
| 10 S-16 | UB reachable from `forbid(unsafe)` code (`read::<bool>`, misaligned access); weakens the signed-safe-cell argument. | Every MMIO driver cell; the cell author picks `T`. | Sealed `MmioValue` trait for u8–u64 plus an alignment check (S, ~25) | Compile-fail doctest + unit test |
| 05 S-9 | Low bits of a firmware base OR U/S into the PTE; a base inside RAM gets remapped uncached. | Every x86 boot; hostile ACPI. | Reject unaligned or above-52-bit bases; refuse Usable overlap; `checked_add`; mask phys in HAL `map_with` (S, ~30) | `k/acpi/tests/tables.rs` cases via KHT |
| 05 S-10 | GPE port wraps, writing to the 8237 DMA/PIC ports; panics with overflow checks. | Non-reduced-ACPI x86 boot; hostile FADT. | `checked_add` in `gpe_blocks` and `sci.rs` (S, ~10) | KHT `gpe_blocks` test |
| 08 S-5 | One cell exhausts the 16 MiB VFS heap and blocks `/srv` unlink/rename for every cell. | All images; any reader. | Per-caller (8) and global caps in `pending.rs`, checked before the read and lease (S, ~40) | VHT: the 9th insert is refused |
| 08 S-6 | Reading a file of ~12 MiB or more aborts the VFS; on hypervisor images any cell can read the 3 GiB guest disk. | All images. | `MAX_WHOLE_READ`; chunked `read_at` for ReadAt/ReadFileGrant; `write_at` in GrantWrite (M, ~120) | VHT 2 MiB cases; `vfs-quota` lane |
| 08 S-7 | A crafted superblock makes the VFS allocate ~64 MiB at mount → abort at every boot. | Images mounting `/data`/`/srv`; offline disk tamper. | Require `free_bitmap_blocks == ceil(total/32768)` in `validate_geometry` (S, ~3) | `cellos-fs` test |
| 08 S-8 | Two inodes sharing a block let a later write silently replace KMS/trust file content. | Same; offline tamper. | Walk inodes at open: blocks marked in the bitmap and claimed once (M, ~60) | `cellos-fs` tests |
| 08 S-9 | A FAT directory cycle overflows the VFS stack or spins listing. | `/mnt/sd` or a secondary FAT disk; crafted volume. | Iterative `remove_tree` (depth 32, 4096 entries); cap `page_dir` (S/M, ~50) | VHT with a cyclic FAT image |
| 08 S-12 | Free empty files and dirs exhaust the VFS heap; the 32 MiB quota exceeds the 16 MiB heap. | All images; any VfsMutate cell. | Minimum per-entry charge; cap ramfs nodes and bytes (M, ~60) | VHT; `vfs-quota` lane |
| 08 S-13 | Any VfsMutate cell overwrites or deletes other cells' files and `.cbx-save` staging. | All images. | Short term: deny path access to `.cbx-save` (S, ~20). Full fix: per-cell subtrees like `srv-owner` (L) | `access/tests.rs`; `save/tests.rs` |
| 07 S-6 | An unauthenticated LAN client reads `/mnt/sd` (incl. the broker K1 key [INFERENCE]) and drives `/api/infer` and `/restart`. | RV64 dev and aarch64 CI disks, once httpd is started from the shell. | Fixed web root; drop or gate the control endpoints (S, ~40) | `cargo test -p service-httpd` refusal cases |
| 07 S-8 | A second Listen holder hijacks httpd's or the broker's port. | Every image with a Listen-right cell. | `(proto,port)→owner` map; `AddrInUse` for another owner (S, ~50) | `cargo test -p service-net` |
| 07 S-9 | Retained replies grow the net heap until networking dies. | Every image; NET_CONNECT holders. | Cap `op.queue` and `replies`; drop a failed Legacy reply (S, ~25) | service-net host test with 10k Sends |
| 07 S-10 | A buggy or hostile virtio backend panics the NIC cell. | RV64 and aarch64 QEMU images (MMIO path). | `get_mut(token)`; clamp `pkt_len` in the vendored `dev.rs` (S, ~8) | `cargo test -p virtio-drivers` with a fake transport |
| 13 S-6 | `for(;;){}` in a page script stalls the engine ~10⁹ ops per eval. | RV64 dev image (ocel-quickjs on by default). | Tick deadline in `interrupt_budget` (S, ~20) | ocel-quickjs test: infinite loop is interrupted |
| 13 S-7 | An allocation-free PDF content stream rasterises for minutes. | RV64 dev image. | `fz_cookie` aborted when `work_expired` (S, ~15) | RV64 smoke with a 100k-op PDF |
| 11 S-9 | A malicious HID descriptor stalls DWC2 (USB + LAN + heartbeat). | RPi3 image; physical USB device. | Cap `report_count`/`report_size`; `checked_mul` in `leds.rs` (S, ~10) | `cargo test -p driver-hid` (no CI step today) |
| 11 S-10 | Frames over 508 B dropped; continuation bytes parsed as fake frames; CRC errors delivered as frames. | RPi3 image; any LAN peer. | Full burst plus a record walk with error-bit drop (M, ~40) | `driver-dwc2-usb` synthetic test; RPi3 1400 B ping |
| 12 S-5 | Jobs without `permissions:` get the repo default token scope; no Dependabot for pinned SHAs. | Every ci/security/perf/docs/dco run. | Top-level `permissions: {contents: read}`; `checks: write` on the audit job; `dependabot.yml` (S, ~20) | `grep -L '^permissions:'` prints nothing; actionlint |

### Medium — latent
| ID | Consequence | Why latent | Fix (size) | Verify |
|---|---|---|---|---|
| 06 S-4 | Public PAC key; and PAC-RET is never armed (no EnIA, no `-Z branch-protection`), although `docs/security-model.md:639` claims it ships. | No CI/board CPU has PAuth. | Seed keys per core, set EnIA, add `branch-protection=pac-ret`, or downgrade the doc (M, ~40) | `objdump \| grep paciasp`; `QEMU_CPU=max` virt-vhe lane |
| 03 S-10 | First `bind` during a switch seals the store → fatal. | `g1-cpu-accounting` only. | Don't seal on Busy (S, ~5) | `cpu_accounting` host test |
| 07 S-13 | Framing desync, cap leaks, permanent dedup Busy, forged claims. | Dead code. | Fix before wiring (M, ~120) | `service-net-broker` lib tests |
| 09 S-3 | STM32 authority seals on its 2nd boot. | Dev-reference board only. | Allow +1 epoch when boot advanced (S, ~30) | Two-boot journal test |
| 09 S-4 / S-5 / S-7 / S-8 | Relay slot DoS; signed-time starvation; gate slot leak and unchecked adoption; budget double-spend after restart. | Not deployed / no consumer / excluded crate. | Per-IP handshake gate (S); per-authority time floor (M); free slot + Fenced check (S); journal replay (M) | Respective unittest/cargo tests |
| 10 S-6 / S-7 | posix-shim `%` over-read / `%.60f` overflow; fs2–fs11 not saved by setjmp. | No shipped caller with untrusted formats; Lua not in CI images. | Stop on NUL and clamp precision (S); save fs2–fs11 (S, ~22) | `posix-shim-test`, `c-math-smoke` on RV64 |
| 11 S-11 | WASM run restarts on every trap; never progresses. | `/bin/wasm` not packaged. | Exit on trap; resumable fuel (S, ~20) | `app-wasm` host test |
| 13 S-4 / S-5 | KV cache OOM aborts AI; embed with no reply. | Need a large operator-supplied model. | Reserve KV at submit; refuse large `n_embd` (S) | `ai-engine` tests |

### Low — reachable
| ID | Consequence → minimal fix (size) | Reachability | Verify |
|---|---|---|---|
| 05 S-15 | Platform NMI/#MC during guest run goes to the guest IDT; the host never logs or halts. → SVM: NMI/SMI/INIT intercepts + #MC exception bit, exit as Preempted; VMX: `PIN_NMI_EXITING` + exception bitmap bit 18, re-raise on host (M, ~50) | Desktop image; x86 HV/tier3 lanes; hardware-triggered | QEMU monitor `nmi` during `qemu-x86-hypervisor-boot` |
| 05 S-17 | Without interrupt remapping, a driver's DMA/MSI injects any vector (timer, IPI, fatal 0-31). → Required boards refuse 435/DMA when IR is inactive (S, ~20) | x86 platforms without active IR; compromised driver | KHT gate test |
| 06 S-7 | Stale TLB entry on another core (guard page or recycled frame). → Call `flush_tlb_page` (S) | AArch64, all images; kernel-internal race | Disassembly shows `dsb ishst`; virt-vhe/el2-novhe lanes |
| 06 S-9 | Guest Q-regs/FPCR leak to Cells and the guest's FP state is corrupted (see also § 3). → Move FP banks into the core vCPU (M, ~60) | aarch64 `qemu-hypervisor-machinery` + virt-vhe Silo | FP-heavy guest checksum; aarch64 FP context test |
| 06 S-14 | SGI EOI mismatch can leave an IPI active → shootdown stall (GIC-400/Pi 4; QEMU masks it [INFERENCE]). → EOI the full IAR (S, ~15) | 2-CPU GICv2 lanes, Pi 4 | `hal-arm` host test |
| 02 S-7 / S-8 / S-10 / S-11 / S-12 | Fixed slot bases on RV64/AArch64 → RNDR/Zkr/virtio-rng entropy (M) · Tier-2 rodata exec → per-page X flag + delete `remap_range_user` (S) · O(n²) load → cap/refuse overlapping PT_LOADs (S) · foreign arch / bad entry → check machine, class, entry (S) · kernel RWX → per-section kernel map (M, ~80) | Dev images / RV64+AArch64 | Boot twice, slot bases differ; loader host tests; text-write probe faults |
| 01 S-12 / S-13 / S-14 | Wrapped deadlines (self-harm) → `saturating_add` (S) · data lost on bad pointer, self-wait, foreign PipeShare → preflight + target checks (S, ~30) · IRQs masked per multi-MiB copy → 64 KiB chunks (S, ~25) | All images | KHT; `sas-lease-tests`; pipeline/getrandom lanes |
| 04 S-6 / S-7 / S-8 | Broken verifier continues boot → panic on self-test failure (S) · RAM lost past 64 memmap entries → per-entry pointers, cap 256 (S) · verify with IRQs masked → incremental verify outside the lock (S) | Every boot / x86 Limine / every spawn | Forced-fail fixture; OVMF large-RAM lane |
| 05 S-18a,b,d,e,f,g,i | Duplicate tables → seen-set (S) · v1 RSDP refused → map 20 B first (S) · volatile reads of Reserved MMIO → admission filter (S) · guest descriptor spin → `pending_count` gate (S) · log flood → rate limiter (S) · ICW2 `& 0xF8` (S) · guest loader `checked_add` (S) | Every x86 boot / x86+ARM VMM; guest-triggered | `acpi/tests`; `cargo test -p service-hypervisor` |
| 07 S-15 | Promiscuous RX, stale bytes, TX overwrite → drop UPE/MPE, zero buffers, check EOP/errors, refuse busy slot (S, ~40) | x86 images; RV64 e1000 | `driver-igb` tests; igb DHCP lane |
| 08 S-14 / S-16 / S-17 / S-18b,c,f | Per-mount caches exceed heap → global budget (S/M) · any cell reads the guest disk → read only for the live hypervisor (S, ~2) · short READ reported Completed → check PRDBC (S) · one bad slot blocks mount → skip it (S) · Write replaces a dir → `IsADirectory` (S) · NVMe out-of-range → `NotIssued` (S, 1) | Various images; crafted media or any cell | VHT, `cellos-fs` tests; Tier 3 lane |
| 10 S-12 / S-14 / S-15 / S-17 / S-18c | Config DoS → caps + paginate (S) · supervisor/compositor full syscall surface → `declare_syscalls!` (S) · shutdown freezes only 64 rows → loop or size from count (S) · grant views race (UB) → copy APIs in VFS (M) · Bus Master set without DMA isolation → refuse on Required boards (S) | All images / x86 | Config/supervisor host tests; `readelf` shows `__ViCell_syscalls`; VFS lanes |
| 11 S-13 / S-16 / S-17 / S-18 / S-20b | Keystrokes logged (readable via ReadLog source 0) → delete the log line (S) · FIFO over-pop → `got.div_ceil(4)` (1 LOC) · shared grants leak → drop the ownership transfer at `grant.rs:748` (S) · fail-open watch, wrapping rects, global caps → refuse, saturate, per-owner cap (M) · per-byte log flood → summary log (S) | x86 desktop/headless, RPi3 | `rg` checks; `service-compositor` tests |
| 12 S-6 / S-10 / S-11 | Branch name injection → pass via `env` (S, ~4) · Limine blobs unaudited → `SHA256SUMS` + `sha256sum -c` (S) · pip with no hashes; `curl\|sh` → `--require-hashes`, pinned rustup-init (S) | CI on dispatch / x86 ISOs / CI signing jobs | grep/actionlint; flip a Limine byte → build fails |
| 13 S-10 | ocel-pdf opens any path for any sender → prefix allowlist or grant input (S) | RV64 dev image | RV64 smoke refusal |

### Low — latent
| ID | Why latent | Fix (size) |
|---|---|---|
| 03 S-9 | Dead `transfer_consumption` | Delete it (S, −50) |
| 04 S-9 / S-11 | No syscall path / kernel-embedded FAT only | Revoke closed roots (S) / bound the walk by file size (S) |
| 05 S-13 | No user-controlled `sstatus` writer | Mask R11 `& 0xFFF \| 0x202` (S, 2) |
| 06 S-6 | Nothing installs megapages | Refuse a leaf at levels 2/1 (S, ~8) |
| 08 S-18a / S-18g | Never constructed / comment only | Delete `driver_disk` (S) / fix comment (1) |
| 09 S-9 = 12 S-7 | Host signing path only; no workflow passes a seed | `--seed-file` with 0600 check (S, ~40–50); test asserts no hex seed in argv |
| 09 S-10 – S-16 | Dev AWS stack, relay, enroll, mock tools; not deployed | LeadingKeys condition, TTL + committed wheel hashes, bounded send queue + denylist reload, commit authority lockfiles + audit + `Zeroizing` seed, shared KMS payload-id atomic, Abort-terminated plan, bind 127.0.0.1 + clamp length (each S/M) |
| 10 S-18a / S-18d | No RISC-V IOMMU board / faulty UART only | Return Rejected on PSCID exhaustion (S) / 64-byte drain bound (S) |
| 11 S-12 / S-15 / S-20a | Not packaged / fixed silicon / no caller passes a baud | Clamp before alloc (S) / clamp ports to 15, addresses to 127 (S) / Err on baud 0 (S) |
| 12 S-8 / S-9 | Developer-only paths | Digest-pinned base image, non-root `USER`, `.dockerignore` (S) / digest image ref (S) |
| 13 S-8 / S-9 / S-11 / S-12 | Need an operator-supplied model | BOOL→1 B (1) / BTreeSet dedup (S) / cap drain, encode first (S) / specials opt-in (S) |

### Info — still open (all S)
- **Reachable:**
  - 03 S-11: Acquire load, saturating deadlines, AP-timeout degrade.
  - 04 S-12: supervisor audit drain plus a monotonic clock; fix `security-model.md:220`.
  - 05 S-18c: route `sdt::walk` through one validator.
  - 05 S-18j: gate injection on DRIVER_OK.
  - 05 S-18k: fix the doc wording.
  - 07 S-16c: `json_escape`.
  - 07 S-16e: HKDF instead of XOR; this breaks wire compatibility.
  - 11 S-22b: answer GetFocus only to the compositor.
  - 11 S-22d: drop `code==0` silently.
  - 12 S-14: add a fuzz job or fix the doc.
- **Latent:**
  - 02 S-13: tag check word.
  - 06 S-11: `ldp x28,x29`.
  - 06 S-12: const-assert SPP.
  - 06 S-13: copy a1/sepc.
  - 07 S-16f: bind the oracle sender.
  - 09 S-18: delete the dead features.
  - 09 S-19: retry before seal, HMAC contract.
  - 10 S-19: stub checks return Err; `sys_set_tls_base` becomes `unsafe fn`.
  - 11 S-22c: drop the dead checks.
  - 12 S-13: `git rm` dead blobs, add the WAD to `NOTICE`.
  - 13 S-13: bind 127.0.0.1.
  - 13 S-14: fix the comment.

## 3. New findings the audit does not have
- **AArch64 task switches save no FP/SIMD state.** I confirmed there is no q/fpsr save outside `vcpu.rs` and `bcm2837/vcpu_ext.rs`, and FP is untrapped (`boot.rs:267,314-317`). Kernel and normal cells are softfloat with `-mgeneral-regs-only`. But `targets/aarch64-unknown-cellos.json:7` enables `+neon`, so std cells (the std-smoke CI lane) share vector registers across cells and with guests. This gives cross-cell data leakage and corruption. The RV64 counterpart was fixed in `95c39baa4b`. Fix: lazy FP trap or save in `__switch_el1/el2` (M, ~80).
- **Release posture may refuse every governed spawn** while no admission backend is wired, so a release kernel would boot init only. [INFERENCE] from `admission/mod.rs:4-7`; not run.
- **Residuals of items marked fixed:**
  - `/srv/net-broker` is world-writable, so a local cell could pin the broker epoch at `u64::MAX` [INFERENCE].
  - Multicast membership is never released when its owner exits.
  - `AppContext` still clears only the recv trailer for the 14 drivers and platform cells built on it.
  - The 7 padding bytes in `types::DirEntry` may copy uninitialised kernel stack to user space [INFERENCE].
- **Out-of-date docs:** `docs/roadmap/current-focus.md` item 6 still lists 05 S-8 as open. `docs/security-model.md:639` claims PAC-RET ships. The `security.yml:30` comment says Cargo.lock is not committed.

Full per-slice reports with every file:line are at `agent://AuditRetriage.Triage{KernelSyscallIpc,KernelMemAuth,X86Hyp,ArmRiscv,NetAi,Storage,AuthorityKms,ServicesApi,DriversGui,SupplyChain}`.