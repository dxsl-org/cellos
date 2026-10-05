---
phase: 2
sub: b
title: "AHCI part B — data path, block registration, persistence"
status: pending
priority: P1
effort: M (sub-phase of the AHCI family)
dependencies: [2, 1]
tier: thinking
ceiling: qemu
---

# Phase 02b — AHCI part B: data path, block registration, persistence

## Evidence (2026-10-05, `qemu` ceiling)

- Cell extended with READ/WRITE DMA EXT (48-bit LBA, PRDT transfers, capacity
  from IDENTIFY) and a new `src/dispatch.rs`; block registration reuses the NVMe
  path (`sys_register_block_driver`).
- Standard-path rebuild (the one CI uses): `pwsh scripts/build-x86_64-cells.ps1`
  → signed 16-file `kernel/src/embedded-x86_64/kernel_fs.img` → kernel
  (`--features board-x86-pc`) → ISO → SATA boot. Part-A markers byte-identical,
  plus `[driver_cell] block driver registered: tid=4`, then `Cellos >`.
- `ahci-x86` **5/5**, including `ahci_fat32_persistence_reboot_x86` (marker
  written through the shell, reboot on the **same** raw image, marker read back)
  and `ahci_and_nvme_both_register_x86`.
- Regressions on the same standard image: `nvme-x86` 3/3, `pcie-multibus-x86`
  2/2, `x86_64-boot` 9/9, `driver-registration-contract` 3/3; diskless boot
  idles; default q35 `cargo check` clean; `cellos-sign --check` green
  (F1 56 allowlisted files).
- Re-verified after the review fixes through the standard packaging path
  (`pwsh scripts/build-x86_64-cells.ps1` → kernel `board-x86-pc` → ISO → SATA
  boot): part-A markers unchanged, `[driver_cell] block driver registered: tid=4`,
  shell reached; `ahci-x86` **5/5** (the persistence oracle still passes with the
  corrected write-flag bit 6) and `nvme-x86` 3/3.
- Evidence log: `evidence/phase-02b-ahci-persistence.log`.

## Interface facts this phase had to settle (recorded, not incidental)

1. **Block registration is single-slot and last-wins**
   (`kernel/src/task/drivers/driver_cell.rs:84-91`). On q35 with both an NVMe
   controller and a SATA disk attached, both `/bin/nvme` and `/bin/ahci` register
   successfully and the later registrant wins; the observed order varied between
   runs, so *which* cell wins is a spawn race. Each lane therefore attaches
   exactly one storage device; `ahci_and_nvme_both_register_x86` pins the
   observed behaviour instead of assuming it.
2. **`unsafe` allowlist**: the F1 ratchet requires an entry before a Driver Cell
   with `unsafe` can be signed, so `scripts/unsafe-allowlist.toml` gained
   `[[file]]` entries for `cells/drivers/ahci/src/controller.rs` and
   `dispatch.rs` plus the `[[crate]] driver-ahci` entry — mirroring driver-nvme's
   class. **Approver confirmed by the repository owner on 2026-10-05** (the field
   records `dmin`).
3. **Part-A defect fixed here**: the IDENTIFY validation read PRDBC from
   command-header byte offset 12 (CTBA high) instead of offset 4 (AHCI 1.3.1
   DWORD1). The committed part-A *binary* had passed because the image predated
   the validation; rebuilding from the committed source failed bring-up with
   `PRDBC=0`, which is how the mismatch surfaced. Now reads offset 4 (verified
   `hdr1=0x00000200` at 4, `0` at 12 on q35). The phase-02a evidence carries the
   corresponding correction.

## Review fixes (2026-10-05, independent reviewer)

The reviewer confirmed the data path is exercised by a real two-boot oracle and
that part-A behaviour is preserved, but found three wire/robustness defects that
QEMU does not punish and two test-honesty gaps. All fixed:

1. **Command-header write flag.** `CMD_HDR_W` was `1 << 5`; DWORD0 bit 5 is the
   ATAPI flag and bit 6 is W (AHCI 1.3.1 §4.2.1). Every `WRITE DMA EXT` therefore
   carried an ATAPI-marked, host-read header while the FIS asked for a disk
   write — QEMU accepted it, a conforming HBA need not. Now `1 << 6`.
2. **Fatal port conditions.** Completion only checked `PxTFD.ERR`/`PxIS.TFES`, so
   `HBFS` (29), `HBDS` (28), `IFS` (27) and `OFS` (24) could clear `PxCI` and be
   reported as success — a read would return stale DMA contents and a write would
   be acknowledged without reaching the device. Now a shared `IS_FATAL` mask,
   used by the data path *and* the IDENTIFY check.
3. **Timeout left the slot owned.** A poll timeout returned while `PxCI` still
   said the HBA owned tag 0, and the shared slot 0 structures let VFS's retry
   race an in-flight DMA. Now a timeout or fatal condition stops the engine,
   clears the latched bits and restarts it; if that does not come back clean the
   controller is poisoned (`faulted`) and every later request is refused with a
   named error.
4. **Test honesty (routing).** The combined-lane test equated two registration
   lines with a usable route. It now asserts only the *registration* contract and
   states that which cell serves I/O is not proven — `service-vfs` caches its
   provider TID, so the winner on a two-storage-device machine is
   scheduling-dependent. Recorded as a plan risk with owner-side arbitration as
   the fix.
5. **Test honesty (skip).** The persistence oracle returned success when Python
   was absent or `mkfat32_inplace.py` failed, bypassing `ci_guard` — CI could be
   green with the only write-touching oracle never run. The Python gap now goes
   through `ci_guard` (skip locally, hard-fail in CI) and a formatter failure
   panics.


## Open observation (LBA refusals) — resolved by review

After registration the plain SATA boot logs repeated typed refusals such as
`[ahci] port=5 tag=0 READ DMA EXT rejected: LBA 800000 >= capacity 131072
sectors`. The independent reviewer concluded these are **correct and benign**:
they are probes of fixed partition offsets that do not exist on the ad-hoc 64 MiB
image, refused by the capacity bound instead of being clamped — the cell behaving
as designed, not a defect in the request path.


## Target

Roadmap gate **X86-PC-1**, second half: make the SATA device usable as storage,
so `/data` and the hypervisor guest disk can live on it.

## Change

- READ/WRITE DMA EXT through the command list built in 02a, with bounded polling
  (interrupts only if the existing IRQ substrate makes it cheap).
- Register as a block device through the **same path NVMe uses**, so VFS,
  littlefs `/data`, and the hypervisor's guest-disk file work unchanged — no
  second block-registration surface.
- **Registration is single-slot** (`kernel/src/task/drivers/driver_cell.rs:84-91`:
  one active block driver, a later registration overwrites the previous one with
  a warn). Both `/bin/nvme` and `/bin/ahci` are spawned on x86, so the cell
  registers only after it has a usable disk — never while idle — and each lane
  attaches exactly one storage device; the phase evidence records the observed
  ordering rather than assuming it.
- Bounded error handling: device fault, task-file error, and timeout must
  surface as typed errors with the port/tag named, never as a silent stall.
- If 02a's polled path proves too slow for the persistence gate, an interrupt
  path is in scope here; otherwise it stays out.

## QEMU-first gate

- Extend `tests/integration/tests/ahci-x86.rs`: block registration marker
  (`[driver_cell] block driver registered`) → FAT32 mount → marker write →
  **second boot on the same image** → marker read back (the NVMe lane already
  proves this two-boot shape).
- Regression set that must stay green: `nvme-x86` 3/3, `pcie-multibus-x86` 2/2,
  `x86_64-boot` 7/7 — the registration ordering these lanes depend on must not
  change.

## Acceptance

- On QEMU: the marker survives a reboot on the same raw image, and the
  regression set above is green.
- Unsupported/absent device and fault paths are typed and logged with port/tag.
- No change to block-registration ordering for NVMe/virtio-blk.

## Out of scope

- Filesystem choice (this sub-phase only makes the block device behave like the
  existing ones), NCQ, hotplug, TRIM, port multipliers.

## Risk assessment

- **Undo:** remove the data-path code and the registration call; 02a's controller
  init remains harmless on its own.
- **Not undoable:** promoting a physical machine's storage to a working HCL row —
  reserved for phase 07.
