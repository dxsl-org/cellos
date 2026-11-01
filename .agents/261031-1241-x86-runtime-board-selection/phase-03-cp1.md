# Phase 03 CP1 — machine fingerprint pinning and re-confirmation (Law-1: syscall 433 `QueryMachineFacts`, `ViMachineFactsV1`, service id 20 `MACHINE_PIN`, `api::services::machine_pin`)

Status: **DRAFT v1, 2026-11-01. Awaiting user decision (Law-1 confirmation 1).** No code has been written. The lock handoff is drafted separately in [phase-03-lock-handoff-draft.md](phase-03-lock-handoff-draft.md) and is not effective.

Plan: [plan.md](plan.md) · [phase 03](phase-03-fingerprint-pinning.md) · ADR-0029 §3. Dependencies: phase 01 (`6eaf663a2`) and phase 02 (`a0e009d64`), both committed. Phase 02 requires this CP1 (`phase-02-runtime-profile-selection.md:128-133`), and so does the phase 01–02 handoff (`lock-handoff-approval.md:29`). Evidence ceiling: x86 q35, QEMU/TCG, SeaBIOS (`plan.md:6`).

## Decisions requested (summary; details in §10)

| # | Decision | Default proposed here |
|---|---|---|
| D1 | Option shape | **A**: full facts record via a new syscall 433 (§3) |
| D2 | Approve the exact Law-1 delta in §4.1–§4.3 | 433, `ViMachineFactsV1` (288 + 8·n bytes, n ≤ 256), service id 20, `MachinePin` frame, ostd wrapper; no shim |
| Q3 | Record location | `/srv/.cellos-machine/pin` instead of `/etc/cellos/machine-pin` (deviation, §1.4) |
| Q4 | Unknown version or unreadable record | **safe mode**, not "no record" (deviation from `plan.md:39`, phase 03 l.22) |
| Q5 | Store unavailable (no P5 volume or no storage) | safe mode under `Required` profiles; warn and continue under the q35 `Optional` profile; success criterion 5 reworded |
| Q6 | Safe-mode service set | `input` + `cellbox sh` (+ `serial`?) |
| Q7 | Confirm authority and flow | honoured only in safe mode, only from the shell init itself spawned, digest-bound; reboot afterwards |
| Q8 | Fingerprint inputs | §4.1 list. Add SMBIOS type 0 BIOS version? Keep the profile source? |
| Q9 | Who may register `MACHINE_PIN` | kernel accepts it only from trusted init (small kernel edit) |
| Q10 | CI | add the QEMU lane to `ci.yml`, or keep it manual |
| Q11 | HCL | add a pin-digest column per machine row |

## 0. Verdict

The phase file says to hand the fingerprint to init "through the existing boot-info path" and to stop and escalate if none fits (`phase-03-fingerprint-pinning.md:16`, `:52`). **No path fits, so this CP1 is the escalation.**
- **Init gets no boot record.** The kernel publishes it with `argv = None` and no launch context (`kernel/src/loader/governed_spawn.rs:282-311`; parameter order at `kernel/src/task/launch.rs:54-72`).
- **The existing kernel→Cell records carry something else.** `system.toml` is a build-time S-class file (`docs/abi-boundary-records.md:42`). Telemetry (430) is per-Cell. Child context is parent-built (`kernel/src/task/child_context.rs:32-43`).
- **The full PCI set is known only after init starts.** The Platform Cell is spawned right before init (`kernel/src/main.rs:1520-1550`). Init waits for it to terminate before it spawns anything (`cells/tools/init/src/boot.rs:192-216`).

A boot-time record therefore cannot carry the PCI set. Success criterion 3 ("diff naming the PCI set", phase 03 l.47) needs a query made after the Platform Cell finishes.

**Design (Option A).**
1. **Kernel.** It keeps the boot facts it already computes and answers a new SpawnCap-gated syscall 433 with one bounded, canonical record, `ViMachineFactsV1`, that ends in a SHA-256.
2. **Init gate.** After the bootstrap VFS and the boot plan are up, and before any configured service, init compares that record with the stored copy on a VFS-protected `/srv` path:
   - absent: first boot, write the record;
   - equal: continue;
   - anything else: field-level diff and **safe mode**.
3. **Re-confirmation.** A cellbox `machine pin --confirm` asks init, over a small typed frame, to re-pin. Init honours it only in safe mode and only from the console shell it spawned. Then the operator reboots.

**Law-1 deltas (all additive):**
- syscall 433 and its record;
- `service::MACHINE_PIN = 20`;
- the `api::services::machine_pin` frame;
- the ostd wrapper.

**Internal changes:** the init gate and safe mode, a VFS rule for one protected namespace, the cellbox builtin, the kernel facts cell and encoder, and an optional kernel restriction on who registers `MACHINE_PIN`.

**What it does not claim.** The pin is change detection, not a security boundary. On today's x86 boot path nothing measures or verifies `limine.conf` or the kernel (§6). Durability is not claimed either: no CellosFS durable-state owner is admitted.

## 1. Facts (observed in source, 2026-11-01)

### 1.1 Fingerprint inputs the kernel already has
| Input | Where computed | Retained after boot? |
|---|---|---|
| Bound profile slug | `bind_x86_profile` stores the descriptor once (`kernel/src/board.rs:276-289`); `selected()` reads it (`board.rs:122-128`) | yes, `BOUND_X86_PROFILE` (`board.rs:114`) |
| Profile source (`cmdline`/`default`) | `choose_x86_profile` returns it (`board.rs:254-261`; enum `:207-210`) | **no**: only logged (`board.rs:290-299`) |
| ACPI root OEM id / table id / revision | `AcpiInfo.root_oem` (`kernel/src/acpi.rs:48`, struct `:59`) → `PlatformIdentity` (`kernel/src/main.rs:449-453`) | **no**: `PlatformIdentity` is a `kmain` local (`board.rs:323-330`) |
| SMBIOS type 1 manufacturer, product, UUID | `smbios::discover` (`main.rs:444-445`); strings ≤ `SMBIOS_STRING_MAX = 64` bytes (`kernel/src/smbios.rs:17,47-62`); all-0/all-1 UUID → none (`phase-01-identity-facts-and-catalog.md:47`) | **no** (same local) |
| Hypervisor bit | `cpu_features::hypervisor_present()` (`main.rs:452`) | **no** (same local) |
| DMAR present | `AcpiInfo.dmar_present` (`acpi.rs:45`) | **no** (`acpi_info` is a `kmain` local) |
| VT-d active | `iommu::is_active()` (`kernel/src/task/drivers/iommu.rs:252-256`), set at `main.rs:855` before init | yes |
| PCI functions | `PCI_DEVICES` (`kernel/src/task/drivers/pcie_ecam.rs:235`); `devices()` (`:1073`); `PciDevice { bdf, vendor_id, device_id, class, subclass, prog_if, … }` (`:214-231`) | yes |
| SHA-256 | `crate::sha256::sha256` (`kernel/src/sha256.rs:87`), already used for init's measurement (`governed_spawn.rs:308`) | n/a |

Not available today: SMBIOS type 0 (BIOS vendor/version/date). The parser walks only to the first type 1 (`phase-01-identity-facts-and-catalog.md:46`). ADR-0029 §3 asks for "where relevant, the firmware version". The ACPI OEM revision is the only proxy (phase 03 l.53). See Q8.

### 1.2 Kernel→init hand-off mechanisms
- **Init launch.** `spawn_init` passes `argv: None` (`governed_spawn.rs:298`) and permit-all syscalls (`:291`). Init is spawned after the Platform Cell (`main.rs:1520-1550`).
- **The only kernel→Cell argv use is the Platform Cell's.** It receives `--ecam-base … --bus-start … --bus-end` (`main.rs:1522-1530`, `kernel/src/loader/spawn_request.rs:32`).
- **Syscall numbers.** The highest is 432 `LookupBlockRoute` (`libs/api/src/abi/syscall.rs:656`, decode `:1393`). A test pins 433 as unmapped (`libs/api/src/abi/syscall_tests.rs:327-335`). **Next free: 433.**
- **Pattern for dispatch-gated reads.** 431 and 432 have no allowlist bit and are listed as always-permitted (`kernel/src/task/syscall.rs:4451-4467`). They use the typed sentinels of `encode_ipc_result` (`:1687-1706`). 432 checks the capability, then `out_len`/`reserved`, then copies (`:6262-6276`).
- **SpawnCap check.** `caller_has_spawn` (`syscall.rs:1853-1855`). Holders include `/bin/init` (`kernel/src/loader/boot_ceiling.rs:64-67`, delegated to cellbox and supervisor) and `/bin/supervisor` (`:136-137`).

### 1.3 PCI set completeness and provenance
- **Who adds devices.** The kernel's early scan seeds the first admitted bus (`main.rs:843-855`). The Platform Cell adds the rest through `RegisterPciDevice` (`syscall.rs:8306-8322`), and duplicate BDFs are skipped (`pcie_ecam.rs:1011-1016`).
- **Which fields are kernel-read.** For Platform-registered devices, vendor and device are re-read by the kernel from ECAM (`pcie_ecam.rs:993-1007`). Class, subclass and prog_if come from the Platform Cell's `cls` argument (`:990-992`). The encoder should re-read the class from config space, so that no fingerprint field is Cell-supplied.
- **Doc drift: `PLATFORM_REGISTERED` is not "scan complete".** Its doc says "completed its ECAM scan" (`pcie_ecam.rs:237-240`), but it is set on the **first** `RegisterPcieBar` or `RegisterPciDevice` (`syscall.rs:8302`, `:8319`). Init's barrier is only lifetime ordering: "absence/termination is not scan-success attestation" (`boot.rs:161`).
- **So completeness is never attested.** A Platform Cell that dies mid-scan yields a short set. That set differs from the pinned one, so the result is a mismatch and safe mode, which fails closed. Nothing here claims more.

### 1.4 Where the record can live
- **`/etc/cellos` is not a writable directory.** It is synthetic. The VFS routes exactly three names: `system.toml` to the kernel boot FS, and `services.toml`/`autoload.toml` to `/mnt/sd/etc/cellos/*` when present (`cells/services/vfs/src/manager.rs:326-343`; listing `:148-149`). Any other `/etc/...` falls to the read-only RamFS root (`cells/services/vfs/src/access/rules.rs:113-121`). And `/mnt/sd/etc/cellos/**` is write-protected for every Cell (`access.rs:116-121,161-174`).
  - So the plan's `/etc/cellos/machine-pin` (phase 03 l.18) does not exist and cannot be written → Q3.
- **Writable CellosFS mounts.** `/data` (P4) and `/srv` (P5), at constant LBAs on the primary disk (`cells/services/vfs/src/manager/disks.rs:30-42`; layout `libs/api/src/abi/disk.rs:34-48`). Both are `allow_write_all` for every Cell (`rules.rs:73-77,108-112`). A record there could be deleted or replaced by any Cell, and on the next boot "absent" would auto-pin a changed machine.
- **Existing protected namespaces on `/srv`.**
  - `/srv/cellos/**`: only the live `service::KMS` provider (`access/kms.rs:4-12,48-56`; `access.rs:131-134`).
  - `/srv/.cellos-trust/**`: no caller at all; only the VFS itself reads it (`rules.rs:44-50,103-107`; `cells/services/vfs/src/trust.rs:1-7`).
  - **Proposal:** `/srv/.cellos-machine/` with the KMS pattern, keyed on the live `MACHINE_PIN` provider (init).
- **Atomic replace.** The VFS has D03 conditional save (`SaveBegin/Chunk/Seal(sha256)/Commit`, `libs/api/src/services/ipc.rs:227-242`). Whether the CellosFS backend supports it is not verified [INFERENCE]. Without it, a torn write gives an unreadable record, and that is safe mode (Q4). This fails closed.
- **Durability.** No durability is claimed. CellosFS opens existing volumes only (`disk.rs:39-45`) and runs `clear_staging` on mount (`cells/services/vfs/src/backend_cellosfs.rs:29-44`; gap 4 in `docs/abi-boundary-records.md:94`).

### 1.5 Assumption A3 (writable cellosfs path on CI x86 disks): **does not hold**
- **x86 storage fixtures have no CellosFS.** They are "a FAT32 volume at LBA 2048 and no Cellos MBR, so the CellosFS, `/data` and `/bin` partitions … are simply absent" (`tests/integration/src/fat_disk.rs:1-4`). The image ends 1 MiB after the FAT volume (`fat_disk.rs:70`), before P5 at LBA 931 072. `scripts/qemu-x86_64-test.sh` attaches a disk only when `X86_SATA_IMAGE` is set (`:16-19,58-65`).
- **So `/srv` is unavailable on every CI x86 lane.** It mounts at its constant LBA (`disks.rs:41-42`) and opens existing volumes only, so it reports `CellosFS open failed — volume unavailable` and `BackendHealth::Unavailable` (`backend_cellosfs.rs:34-41,122-127`).
- **The plan's fallback names the wrong script.** It names `scripts/gen-disk*` (`plan.md:35`). `scripts/gen-disk-ci.sh` builds the RV64 `disk_v3.img` (`.github/workflows/ci.yml:1598-1599`).
- **x86 disks with a provisioned P5 already exist, but only in private lanes:**
  - `scripts/phase09_trust_disk.py`: P1 FAT32 + P5 CellosFS through `tools/cellos-fs-provision --seed` (`:1-31`), used by `scripts/qemu-phase09-ssh-test.sh:444`;
  - `scripts/phase07_rg_fixture.py:27`;
  - `scripts/mksrv-img.sh` (P4/P5 only).
- **Fallback proposed:** a new `scripts/x86/machine_pin_disk.py` on the `phase09_trust_disk.py` pattern, with a blank provisioned `/srv`. That handles the phase 03 lane. What existing lanes do without a store is Q5.

### 1.6 Init: gate point, plan, safe set, message loop
- **Start-up order** (`cells/tools/init/src/main.rs:80-166`):
  1. platform barrier (`main.rs:75`, `boot.rs:216`);
  2. block drivers (`main.rs:84`; `boot.rs:473`);
  3. bootstrap VFS (`main.rs:86-93`);
  4. `configuration::load` + ordering (`:94-117`);
  5. configured services (`:122`);
  6. `OCEL_ACTIVATOR` registration (`:127-146`);
  7. test fixtures, "OS ready" and autoload (`:150-163`);
  8. `supervisor::run` (`:165`).

  The gate fits between steps 4 and 5. At that point the VFS answers RPCs (`configuration.rs:12-33`) and `/srv` is mounted (`manager.rs:73`).
- **The default plan has network and required services.**
  - NIC probes `virtio-net`, `e1000`, `igb` (`config/services.toml:8-21`);
  - `input`, service 3 (`:51-55`);
  - `net` and `silo`, both `required = true` (`:57-62`, `:80-87`);
  - `kms`, `net-broker`, `supervisor`, …;
  - the shell is the autoload entry `cellbox sh` (`config/autoload.toml:41-44`).
- **Cellbox reads keys from the `INPUT` service** (`cells/tools/cellbox/src/async_utils.rs:42`). A console needs `input`.
- **HCL requirement R1.** Every qualified machine has a COM1 16550 (`docs/hardware-compatibility-list.md:71`), so the serial console exists in safe mode.
- **Init's message loop.**
  - It receives attested messages with a 64-byte payload (`cells/tools/init/src/activation.rs:13-46`).
  - `supervisor::run` gives each message to the activator first (`supervisor.rs:95-105`).
  - Messages with an identity trailer are never treated as exits (`:106-113`).
- **Service ids.** The next free id is 20 (`libs/api/src/abi/syscall.rs:1476`).
- **`RegisterService` is open to any SpawnCap holder, for any id and any live tid.** The registry is last-wins (`phase-09c-cp1.md:294-299`, `kernel/src/cell/service_registry.rs:84-114`). Only `BLOCK_DRIVER` is refused (`syscall.rs:6278-6280`). The kernel has no record of which tid is trusted init (grep of `kernel/src` for an init-tid static: none).

### 1.7 Profile, DMA and the "remove intel-iommu" criterion
- **Under `x86_64-pc` (`Required`) without a remapper, every Driver Cell DMA map is refused.** `map_dma_for_cell` returns `Rejected` with `DMA isolation REQUIRED … refusing untranslated DMA` (`iommu.rs:141-161`), as observed in phase 02 run (b) (`phase-02-runtime-profile-selection.md:95`).
- **So NVMe/AHCI cannot read the disk.** The pin store is unreachable, and phase 03 criterion 5 ("both the DMA refusal and the fingerprint mismatch", l.49) cannot be observed as a *comparison*. What can be observed is the DMA refusal plus "pin store unavailable" → safe mode (Q5).
- **The VT-d field diff is observable under the q35 profile.** There a missing remapper gives identity DMA (`iommu.rs:162-171`), so storage works: pin with `-device intel-iommu`, then boot without it.

### 1.8 Boot-chain trust today
- **The profile is selected by the Limine `cmdline` in `limine.conf`.** `scripts/x86/limine.conf` carries `cellos.profile=qemu-q35-x86_64`, and `limine-pc.conf` has no option (`phase-02-runtime-profile-selection.md:73-76`).
- **Nothing measures or verifies that file today.** HCL R3: "There is no signed or measured x86 boot path; … A board that locks Secure Boot on cannot be made compatible" (`docs/hardware-compatibility-list.md:73`). ADR-0022 lists secure/measured boot as outside its scope (`docs/decisions/0022-intel-x86-64-c2c-only-direction.md:145`).
- **What the vendored Limine 8.7.0 supports.**
  - Secure Boot with a signed EFI executable, "in combination with enrolling the BLAKE2B hash of the Limine config file into the Limine EFI executable" (`limine enroll-config`; `limine/limine-8.7.0/USAGE.md:13-18`).
  - Per-file `#blake2b` path hashes (`limine/limine-8.7.0/CONFIG.md:187-189`).
  - Whether Limine performs TPM measurement is not verified [not checked; no claim].
- **The UEFI path itself is not evidenced.** OVMF panics before ACPI (`phase-01-identity-facts-and-catalog.md:63`).

## 2. Problem

Phase 03 must:
- record a fingerprint on first boot;
- re-check it on every boot;
- on mismatch, print a field-level diff and withhold non-boot services until an operator re-confirms (ADR-0029 §3; phase 03 l.14-22, l.42-49).

It must do this without:
- giving the record authority;
- changing a profile or DMA decision;
- adding an unreviewed syscall (phase 03 l.16, l.52).

The facts are split across a boot-time `kmain` local (§1.1) and a PCI table that completes only after init starts (§1.3). Writable storage that only init can write does not exist yet (§1.4).

## 3. Options

### A. Full facts record via new syscall 433 (recommended)
- **Kernel.** It keeps the boot facts in a write-once cell. When asked, it encodes one canonical `ViMachineFactsV1`: identity, profile, flags, a sorted PCI multiset, and a SHA-256 trailer. It returns the record to SpawnCap holders.
- **Init.** It stores the exact bytes as the pin. It compares bytes and diffs field by field, showing old and new values.
- **Cost.** One syscall plus one bounded record (≤ 2336 bytes). Firmware-controlled strings (≤ 64 bytes each) reach init, so init must escape them on output, as `board.rs:332-335` does.
- **Benefit.** The operator sees *what* changed, for example `pci +8086:100e 02:00:00` or `smbios.product "A" → "B"`, before confirming. This meets criterion 3 literally. One format serves both the K boundary (syscall) and the P boundary (file).

### B. Group-digest record via new syscall 433 (exact 96 bytes)
- **Kernel.** It returns version, flags, profile source, PCI count, ACPI OEM revision and per-group 16-byte truncated SHA-256 values (identity, profile, PCI), plus the full 32-byte digest. No string leaves the kernel. Operators read the current values from the `[identity]` line (`board.rs:377-379`) and from a new `[pci-set]` kernel line.
- **Cost.** The same Law-1 count as A, with a smaller exact-length record.
- **Weakness.** The *old* values are gone. The diff can say "pci changed (6 → 7)" but not which device was added, unless the operator kept old boot logs. A re-confirmation made without seeing the change is weaker evidence of operator intent.

### C. No new syscall: boot facts through init argv, PCI deferred to v2
- **Kernel.** `spawn_init` passes a hex argv with the boot-time facts only, on the Platform Cell's `with_argv` pattern (`main.rs:1522-1530`).
- **Cost.** The argv content is still a new K-class record that needs a §1 contract. The service id and the `MachinePin` frame are still Law-1. The diff drops the PCI set.
- **Fails** criterion 3 (`-device e1000` → diff naming the PCI set) and the "PCI ID set" input in ADR-0029 §3. It would need a criteria amendment and a second ABI step later.

**Rejected inside the options:**
- Identifying init to the VFS by its `OCEL_ACTIVATOR` registration. That overloads a demand-activation id, and the registration happens only after the configured services start (`main.rs:127-146`).
- Letting the VFS own the record the way it owns the trust store. That would need a new `VfsRequest` op and still an init identity.
- A caller-identity flag. That needs a new `libs/api` bit, a kernel `CapSet` field and launch ceilings: more Law-1 than a service id.

## 4. Recommended design (Option A): exact deltas

### 4.1 Law-1: syscall 433 `QueryMachineFacts`
- **ABI.** `a0 = out_ptr, a1 = out_len, a2 = 0, a3 = 0` → `total_len` of the record written.
- **Gate.** Admitted past the allowlist (`allowlist_bit() → None`, added to the list at `syscall.rs:4451-4467`). Gated at dispatch on `caller_has_spawn`.
- **Typed errors** (`has_typed_errors`, `syscall.rs:1700-1706`):
  - `usize::MAX`: no SpawnCap;
  - `usize::MAX - 3`: `out_len != MACHINE_FACTS_CAPACITY` or `a2`/`a3` non-zero;
  - `usize::MAX - 2`: no facts. That covers a non-x86 build, no bound profile, or more than 256 PCI functions.
- **Evaluation and copy.** Evaluated at call time. Validation comes before computation, and computation before the single `write_user_slice`. There is no side effect.
- **Constants:** `MACHINE_FACTS_VERSION: u16 = 1`, `MACHINE_FACTS_PCI_MAX = 256`, `MACHINE_FACTS_CAPACITY = 288 + 8·256 = 2336`.

**`ViMachineFactsV1`**, little-endian, field order = wire order:

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 2 | `version` | = 1; other → `UnsupportedVersion` |
| 2 | 2 | `flags` | bit 0 `ACPI_OEM`, 1 `SMBIOS`, 2 `SMBIOS_UUID` (requires 1), 3 `DMAR_PRESENT`, 4 `VTD_ACTIVE`, 5 `HYPERVISOR`, 6 `PCI_ECAM` (a validated MCFG window exists); `KNOWN = 0x7F`, other bits refused |
| 4 | 4 | `total_len` | = 288 + 8·`pci_count` = actual length (`!=` refused) |
| 8 | 1 | `profile_source` | 1 = command line, 2 = default; other refused |
| 9 | 1 | `slug_len` | 1..=64 |
| 10 | 1 | `smbios_manufacturer_len` | 0..=64; 0 unless `SMBIOS` |
| 11 | 1 | `smbios_product_len` | 0..=64; 0 unless `SMBIOS` |
| 12 | 2 | `pci_count` | 0..=256; 0 unless `PCI_ECAM` |
| 14 | 2 | `reserved0` | 0 |
| 16 | 4 | `acpi_oem_revision` | 0 unless `ACPI_OEM` |
| 20 | 4 | `reserved1` | 0 |
| 24 | 8 | `acpi_oem_table_id` | zero unless `ACPI_OEM` |
| 32 | 6 | `acpi_oem_id` | zero unless `ACPI_OEM` |
| 38 | 2 | `reserved2` | 0 |
| 40 | 16 | `smbios_uuid` | RFC 4122 order; zero unless `SMBIOS_UUID` |
| 56 | 64 | `slug` | `[a-z0-9_-]` (the phase 02 slug rule, `phase-02-runtime-profile-selection.md:58`); bytes past `slug_len` zero |
| 120 | 64 | `smbios_manufacturer` | raw firmware bytes; bytes past length zero |
| 184 | 64 | `smbios_product` | as above |
| 248 | 8 | `reserved3` | 0 |
| 256 | 8·n | `pci[n]` | `{vendor u16, device u16, class u8, subclass u8, prog_if u8, reserved u8 = 0}`; sorted ascending by `(vendor, device, class, subclass, prog_if)`; duplicates kept (multiset); BDF excluded |
| 256+8n | 32 | `digest` | SHA-256 of bytes `[0, 256+8n)` |

- **Validation.** The decoder (`ViMachineFactsV1::decode`) refuses:
  - every violation in the table;
  - unsorted PCI entries;
  - trailing bytes.

  It runs the full decode before any use. It does **not** verify `digest`: `libs/api`, `ostd` and init have no SHA-256 dependency (their `Cargo.toml`), and adding one would edit `Cargo.lock`. The digest is a display and identity handle, not integrity protection. The gate compares whole records byte for byte.
- **Producer.** Kernel `machine_facts::encode`. Values come from the facts cell (§4.4), `selected()`, `iommu::is_active()` and `pcie_ecam`, with the class re-read from config space (§1.3).
- **Consumers.** Init only (SpawnCap). Cellbox gets results through init.
- **Determinism.** Two permutations of the same device list encode to identical bytes.

### 4.2 Law-1: service id `service::MACHINE_PIN = 20`
- Init registers its own tid under it right before the gate.
- The VFS rule (§4.5) and the cellbox client (§4.6) resolve init through it.
- Update the doc comment "next free: 20" (`syscall.rs:1476`) to 21.

### 4.3 Law-1: `api::services::machine_pin` frame (C class, cellbox ↔ init)
- **Requests.** `MachinePinRequest::Status` and `MachinePinRequest::Confirm { digest: [u8; 32] }`.
- **Responses.**
  - `MachinePinResponse::Status { state: PinState, current: [u8; 32], pinned: [u8; 32] }`, with `PinState ∈ {Match, FirstBootRecorded, Mismatch, RecordUnreadable, StoreUnavailable, FactsUnavailable}`.
  - `Recorded { digest }`.
  - `Refused(code)`, with codes `NotSafeMode`, `NotConsoleShell`, `DigestStale`, `StoreWriteFailed`.
- **Envelope.** It must fit init's 64-byte payload (`activation.rs:13`). It is encoded with the existing `api::ipc` encoder, like the activator frames (`activation.rs:327-335`). It has a version byte; unknown variants are refused before any effect.

### 4.4 Kernel internals (no further Law-1 bytes)
- **Facts cell.** A write-once `X86_MACHINE_FACTS` next to `BOUND_X86_PROFILE` (`board.rs:114`). `bind_x86_profile` fills it with `PlatformIdentity`, `ProfileSource` and `dmar_present`, at the binding site (`main.rs:448-466`). It goes into `scripts/mutable-globals.toml`.
- **Q9 restriction.** The `RegisterService` arm (`syscall.rs:6278-…`) accepts `MACHINE_PIN` only when the caller is the tid published by `spawn_init`, registering itself. That tid is a write-once static set in `governed_spawn.rs:314`, not matched by name; names are spawner-chosen (`rules.rs:13-19`). Every other caller gets `PermissionDenied`, the same treatment as `BLOCK_DRIVER`.

### 4.5 VFS: protected namespace `/srv/.cellos-machine/`
- **Rules.** An exact row for `/srv/.cellos-machine` and a prefix row for `/srv/.cellos-machine/`, both `allow_*_all = false` (next to `rules.rs:44-50,103-107`).
- **Decision.** `decide` (`access.rs:115-141`) admits a caller only when `lookup(MACHINE_PIN) == caller.sender_tid` and the identity is live. This mirrors `live_kms_matches` (`access/kms.rs:48-56`).
- **Other paths.**
  - `can_read_fast` denies the prefix (as `access.rs:90-95` does for KMS).
  - `can_remove_tree`/`can_remove_dir` refuse it and its ancestors (`access.rs:97-113`).
  - Non-canonical paths are refused (`access/kms.rs:25-46`).
- **Effect.** No shell, SSH session or other Cell can read, write, delete or rename the pin.
- **Residual.** A raw-disk or offline attacker can still do all of these (§6).

### 4.6 Init gate, safe mode and confirm (`cells/tools/init/src/machine_pin.rs`)
**Gate.** It runs once, after `configuration::load`/ordering (`main.rs:94-117`) and before the first `launch_order` (`main.rs:122`):
1. Register `MACHINE_PIN`. On failure → safe mode (`FactsUnavailable`).
2. `sys_query_machine_facts` → decode. On error → safe mode (`FactsUnavailable`).
3. VFS `Stat("/srv/.cellos-machine/pin")`:
   - **Absent and backend healthy** → create the directory, write the record, sync, read back and compare. Then log `Init: machine pin: first boot — recorded <digest[0..6] hex>` and continue. A failed write or readback → safe mode (`StoreUnavailable`).
   - **Present** → read it (bounded to the capacity) and decode.
     - Bytes equal → `Init: machine pin: match <digest>` and continue.
     - Decodes but differs → print the diff, then safe mode (`Mismatch`).
     - Does not decode, or unknown version → safe mode (`RecordUnreadable`) (Q4).
   - **Backend unhealthy, or `/srv` unmounted** → `StoreUnavailable` → policy per Q5.

**Diff lines.** One line per changed field: `Init: machine pin: changed <field>: <old> → <new>`.
- Fields: `profile.slug`, `profile.source`, `acpi.oem_id`, `acpi.oem_table_id`, `acpi.oem_revision`, `smbios.manufacturer`, `smbios.product`, `smbios.uuid`, `dmar.present`, `vtd.active`, `cpu.hypervisor`, `pci.ecam`.
- PCI changes print as `pci +VVVV:DDDD CC:SS:PP` / `pci -…` multiset differences, capped at 16 lines plus `…and K more`.
- Firmware strings are escaped like `board.rs:332-335`.

**Safe mode.**
1. Print `Init: SAFE MODE — <reason>; non-boot services withheld. At the console run 'machine pin --confirm' then reboot.`
2. Launch only the static safe set (Q6): plan entries named `input` and the autoload `cellbox` shell (plus `serial` if chosen). They are resolved by name from the already-loaded plan, so that their `args` are kept.
3. Skip:
   - the rest of the configured plan, including the `required` `net`/`silo`;
   - `OCEL_ACTIVATOR` registration;
   - test fixtures;
   - "OS ready";
   - autoload.

   It logs `Init: SAFE MODE — required services withheld: net, silo`.
4. Run `supervisor::run` over the services already running and the safe set. The block drivers and VFS started before the gate stay supervised (plan.md:29).

**Confirm.** It is honoured only when **all** of these hold:
- init is in safe mode;
- the sender tid equals the safe-mode shell init spawned, and its attested identity is live (`activation.rs:48-52`);
- `digest` equals the current kernel digest.

Then:
1. Init re-queries 433.
2. It writes and syncs the record, reads it back, and compares.
3. It replies `Recorded` and logs `Init: machine pin: re-confirmed <digest>; reboot to start services`.
4. Init stays in safe mode, and the operator reboots (Q7).

Outside safe mode, `Confirm` returns `NotSafeMode`. `Status` is answered in every mode.

### 4.7 Cellbox `machine` builtin (ADR-0028 CLI client, `0028-…md:29,31`)
- `machine pin` sends `Status` and prints the state and both digests.
- `machine pin --confirm` sends `Status`, prints the current digest, then sends `Confirm { current }` and prints the outcome. Init has already printed the field diff on the console.
- Dispatch: the builtin table (`cells/tools/cellbox/src/executor.rs:971-973`, `:2252-2269`). The canonical non-shell form is `cellbox machine pin --confirm`.

## 5. File set

See [phase-03-lock-handoff-draft.md](phase-03-lock-handoff-draft.md). It is recorded in an approval only after D1/D2 and Q3–Q11 are answered.

## 6. Trust note and evidence ceiling

**Trust.**
1. **The record carries no authority.** Policy stays with the bound catalog profile: DMA isolation, the driver list and the q35 hypervisor rule (ADR-0029 §2; `board.rs:254-273`). The pin only withholds services.
2. **`limine.conf` must be covered by measured or secure boot before the pin means anything against a capable attacker.** The profile slug and source live only in that file's `cmdline`. Today, anyone who can write the ESP or ISO can change the profile, and the kernel too (HCL R3, §1.8). The pin then catches a profile change only because slug and source are fingerprint inputs, and only while the `/srv` record is intact.
3. **What a sufficient chain would be.** UEFI Secure Boot with a signed Limine EFI, `limine enroll-config` (config BLAKE2B), and `#blake2b` hashes on the kernel path (§1.8), or a measured boot that seals the pin. That is outside this plan: a Security-track item (`hardware-compatibility-list.md:73`, ADR-0022:145).
4. **Phase 02's security note is conditional on a setup that does not exist yet.** It says a hostile firmware "cannot inject the command line on a measured or secure-boot setup" (`phase-02-runtime-profile-selection.md:125`). The ADR-0029 amendment should say so.
5. **A raw-disk or offline attacker defeats the pin.** Deleting the record makes the next boot "first boot". Corrupting it gives safe mode, which fails closed. The VFS rule stops only in-system Cells.
6. **Firmware strings are untrusted.** They are bounded (64 bytes), escaped on output, and never used as format strings.

**Evidence ceiling.**
- **Claimed:** x86 q35 under QEMU 8.2.2 TCG, SeaBIOS, one vCPU, plus one KVM boot as in phase 02 (A2).
- **Not claimed:**
  - UEFI (OVMF panics, §1.8);
  - VMware (the HCL's third pre-physical environment, `hardware-compatibility-list.md:34-44`);
  - any physical Intel machine (pinning a real HCL machine is qualification, `plan.md:6`);
  - durability or power-cut retention of the record;
  - tamper resistance;
  - completeness of the PCI scan (§1.3).

## 7. Test plan

**Host:**
- `cargo test -p api --target x86_64-unknown-linux-gnu`:
  - offsets, round trip, and every refusal in §4.1 (version, length, flags, reserved bytes, lengths, string bytes past the length, slug charset, `SMBIOS_UUID` without `SMBIOS`, `pci_count` without `PCI_ECAM`, unsorted PCI, entry reserved byte, trailing bytes);
  - `machine_pin` frame round trip and refusals;
  - `syscall_tests`: 433 maps, has no allowlist bit, and 434 is unmapped.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`:
  - 433 decode;
  - gate: no SpawnCap → denied; bad `out_len`/reserved → invalid; before bind → not found; more than 256 functions → not found;
  - PCI permutation invariance of bytes and digest;
  - a source change changes the digest;
  - the class is taken from config space, not from the `cls` argument;
  - `RegisterService(MACHINE_PIN)` from a non-init SpawnCap holder → denied (Q9).
- `cargo test -p app-init` (host build, `main.rs:1-3`):
  - the decision table of §4.6, including absent + unhealthy backend → `StoreUnavailable`, never a write;
  - diff rendering and escaping;
  - safe-set selection from `config/*.toml`;
  - confirm authorization (wrong sender, not in safe mode, stale digest).
- `cargo test -p service-vfs`, `access` tests on the `access/kms.rs:58-185` pattern: live provider only; stale lookup denied; ancestors and recursive remove refused; non-canonical paths refused.
- `cargo test -p app-cellbox`: argument parsing for `machine pin [--confirm]`.

**QEMU** (new `tests/integration/tests/machine-pin-x86.rs`):
- Setup:
  - the PC ISO (`X86_LIMINE_CONF=scripts/x86/limine-pc.conf`) with `-device intel-iommu`;
  - the NVMe fixture from `scripts/x86/machine_pin_disk.py`;
  - a VT-d helper with an optional NIC (today `boot_x86_bios_with_vtd` always adds e1000, `tests/integration/src/lib.rs:1437-1440`).
- Steps, all on one disk image:
  1. **First boot.** `machine pin: first boot — recorded`. `cat /srv/.cellos-machine/pin` and `ls /srv/.cellos-machine` are denied at the shell.
  2. **Second boot.** `machine pin: match`, the same digest, the network services start.
  3. **Added `-device e1000`.** A diff line `pci +8086:100e 02:00:00`, `SAFE MODE`, no `Init: net launched.`, no `[e1000]` driver line, and the shell prompt still present.
  4. **`machine pin --confirm` → `Recorded`; reboot with e1000.** `match`, network starts.
  5. **PC profile without `intel-iommu`.** The `DMA isolation REQUIRED … refusing` line, then `SAFE MODE — machine pin store unavailable` (criterion 5 reworded per Q5).
  6. **q35 ISO.** Pin with `intel-iommu`, then boot without it: a diff naming `dmar.present 1 → 0` and `vtd.active 1 → 0`, then `SAFE MODE`. This witnesses the VT-d field.
  7. **Tamper from inside.** `vwrite /srv/.cellos-machine/pin x` and `rm` are refused; the next boot still matches.
  8. **Confirm refused outside safe mode.** `machine pin --confirm` on a matching boot → `NotSafeMode`.
  9. **Optional.** Explicit `cellos.profile=x86_64-pc` vs no option → diff `profile.source default → cmdline`.
- **Regression:**
  - `nvme-x86` and `iommu-dmar-x86` against the canonical ISOs;
  - `qemu-x86_64-test.sh` (e1000/igb gates, `:109-127`);
  - the x86 domain lane.

  Under the Q5 default (q35 warns), the q35 lanes are unchanged. PC-ISO lanes without P5 enter safe mode and must assert that.
- **Repo checks:** `mutable-globals --check`, `-D warnings` kernel builds (default and `policy-required`) as in phase 02, and the remaining items of `scripts/check-board-configs.sh`. The pre-existing `check-hal-boundaries.sh` failure is noted in `phase-02-runtime-profile-selection.md:87-91`.

## 8. Risks

- **Every x86 lane without P5 hits `StoreUnavailable`** (§1.5). With strict handling on every profile, every q35 network lane breaks until its fixture gains P5 → Q5.
- **Platform dies mid-scan** → short PCI set → mismatch → safe mode. This fails closed and is reported, not hidden (§1.3).
- **Firmware updates** change `acpi.oem_revision`, which forces re-confirmation. That is intended (phase 03 l.53).
- **SpawnCap holders can register any service id** (§1.6). Without the Q9 restriction, a later SpawnCap holder could take `MACHINE_PIN` (last-wins) and gain pin write access. KMS has the same pre-existing exposure.
- **The safe set depends on plan names** (`input`, `cellbox`). A custom plan without them → safe mode with no console. Init then logs `SAFE MODE — no console in plan` and keeps supervising storage only.
- **A torn pin write** (no CellosFS durability owner) → `RecordUnreadable` → safe mode. The operator re-confirms.
- **The record bound (256 PCI functions)** may be too small for large servers. Above it the result is `FactsUnavailable` → safe mode, not silent truncation (Q8).

## 9. Rollback

- Revert the Phase 03 commit set. Init stops gating. The pin file and its directory are inert data on `/srv` that no reader interprets, and the VFS rule disappears with the revert.
- Syscall 433 and service id 20 return to unmapped. No other Cell depends on them.
- No other on-disk format changes. The record is versioned (v1), and per Q4 an unknown version means safe mode, not auto-pin.

## 10. Decisions and open questions for the user

1. **D1, option.** A (recommended), B or C (§3).
2. **D2, Law-1 delta.** Approve 433 `QueryMachineFacts` + `ViMachineFactsV1` (§4.1 layout, capacity 2336), `service::MACHINE_PIN = 20` (§4.2), `api::services::machine_pin` (§4.3) and `ostd::syscall::sys_query_machine_facts`. Clean cutover, no shim.
3. **Q3, record path.** `/srv/.cellos-machine/pin` (recommended), on CellosFS P5 and protected by the live `MACHINE_PIN` rule. The alternative `/data/.cellos-machine/pin` (P4) works the same way.
4. **Q4, unknown version or corrupt record.** Safe mode (recommended), or "no record" as `plan.md:39` and phase 03 l.22 say. "No record" would auto-pin a changed machine whenever the record is damaged, which ADR-0029 §3 forbids ("never accepted automatically").
5. **Q5, store unavailable.**
   - (a) Safe mode on every profile. Every x86 fixture then needs P5, which is a large lane change.
   - (b) **Recommended:** safe mode under `Required` profiles; under the `Optional` q35 profile (explicit operator plus hypervisor), log `machine pin disabled — no store` and continue.
   - (c) Warn everywhere. That means no pin on physical machines without a provisioned `/srv`.

   Under (a) or (b), criterion 5 becomes "DMA refusal + `SAFE MODE — store unavailable`", with the VT-d field diff witnessed on q35 (§7 step 6).
6. **Q6, safe set.** `input` + the `cellbox` shell (recommended). Add `serial` (the multi-port service)? Add `xhci` for USB keyboards on machines whose only console is local? Adding `xhci` lets a DMA-capable driver run before re-confirmation.
7. **Q7, confirm.** Safe mode only, console-shell tid only, digest-bound, then a reboot (recommended). The alternative is to leave safe mode and continue the boot in place after confirm: no reboot, but a more complex init state transition.
8. **Q8, inputs.**
   - Keep the profile *source* (recommended: it exposes `limine.conf` edits).
   - Keep the hypervisor bit.
   - PCI as a multiset without BDF (recommended), or a set, or with BDF.
   - A bound of 256 functions.
   - Add SMBIOS type 0 BIOS version and date (ADR-0029 §3 "firmware version")? That is a kernel-only `smbios.rs` addition and a new record field now, or a v2 later.
9. **Q9.** Restrict `RegisterService(MACHINE_PIN)` to the trusted-init tid in the kernel (recommended), or accept the existing last-wins exposure shared with KMS.
10. **Q10.** Add the QEMU lane to `.github/workflows/ci.yml`, or keep it manual like the phase 09 SSH lane.
11. **Q11.** HCL: add a "profile / pin digest" column per machine row, and a note under R3 that the pin is not a boot-integrity control until Secure Boot covers `limine.conf`.
