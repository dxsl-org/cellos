# ABI boundary records

**Status**: convention + census, 2026-11-01 (ginux-lessons Phase 06). Zero ABI
bytes were changed to write it.
**Rule source**: [code standards, Law 1](code-standards.md#boundary-specific-abi-records).
**Related specs**: [Spec 17 IPC wire contract](specs/17-ipc-wire-contract.md)
(Cell↔Cell framing, §4), [Spec 23 §6](specs/23-native-sdk-contract.md#6-compatibility-versions-and-errors)
(frozen ABI, Manifest v2), [Spec 22 §2.7](specs/22-native-domain-cell-implementation-gate.md#27-admission-and-manifest-compatibility)
(no Manifest v3 without its own versioning).

A boundary record is needed where bytes cross a real trust or update boundary:
a serialized record, a guest or native ABI, persisted metadata, or a component
that is built or updated independently of its peer. It is not needed for
same-build typed Rust calls. A Tier-1 SAS method call stays a typed call: do not
add magic, version or size fields, or decode a wire format on every call.
Typed calls still need their bounded holder/rights/live-lineage checks and a
snapshot of mutable shared metadata at admission (Spec 25).

## 1. What a record states

Write these six fields for every boundary record. A field may be satisfied by an
enclosing typed envelope or an exact-length contract; not every record needs its
own header.

| Field | Meaning |
|---|---|
| Identity / type | Magic, tag, section name, or the typed envelope that fixes it (syscall number + fixed size). |
| Version policy | The exact accepted set. A version that policy still supports is not refused for being older (CellManifest v1 is accepted). An unknown version is refused, never ignored. |
| Length | Exact (`!=` refused) or an explicit bound. Trailing bytes are refused unless the record says why they are tolerated. |
| Reserved / unknown bits | Writers emit zero; decoders refuse non-zero reserved fields and undefined flag bits. Feature bits describe real negotiated behavior, never a test stage or a capability grant. |
| Ownership | Producer, consumer and update class (§2). |
| Validation before effect | The full record is decoded and validated before any task, authority, device, resource or persistent state changes. |

Changing a public byte, SDK surface or error meaning still follows Law 1 (CP1
approval before code). Fixing a gap in §4 is such a change when it alters what
a caller observes.

## 2. Update classes

| Class | Boundary | Examples |
|---|---|---|
| S | Same build: the kernel and bytes it embeds | VIFS1 `kernel_fs.img`, embedded init, `system.toml` |
| K | Kernel ↔ independently built Cell | syscalls, ELF sections, `cell_session`, child context, telemetry, completion, caller identity |
| C | Cell ↔ Cell IPC | block wire, KMS frame |
| O | Operator/fleet signed | `__ViCell_sig`, `/POLICY.BIN` |
| P | Persisted across builds | CellosFS, P2 cell table, `/mnt/sd/etc/cellos/*.toml`, KMS records |
| F | Firmware / bootloader | ACPI, Limine responses |
| G | Guest-delivered values | VM exit fields |

## 3. Census

`file:line` references were read on 2026-11-01.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect |
|---|---|---|---|---|---|---|
| Syscall ABI | K | `ViSyscall` numbers (`libs/api/src/abi/syscall.rs`); unknown → `Unknown` | none (no ABI version) | registers; user buffers bounded | unknown number fails closed, handler never runs (`kernel/src/task/syscall.rs:10036-10038`); flags per syscall, e.g. `TelemetrySnapshot` strict (`kernel/src/task/telemetry.rs:200-205`) | per handler |
| **CellManifest** `__ViCell_manifest` | K, inside signed bytes | magic `0x5649_4345` (`libs/api/src/abi/manifest_flags.rs:8`) | v2 exact; v1 upcast accepted; others refused (`libs/api/src/abi/manifest_parse.rs:31-85`) | exactly 8 (v1) or 16 (v2) (`manifest_parse.rs:24`, `kernel/src/loader/manifest_section.rs:48-52`) | undefined flags, v1 padding, v2 `cap_args_off`/`reserved`, class outside 0..=3 and `0xFF` refused (`manifest_parse.rs:34-75`) | classified before signature, owner admission, caps and task preparation (`kernel/src/loader/governed_spawn.rs:45-56` vs :60, :113, :130, :206) — see §5 |
| `__ViCell_sig` | O | one `SHT_PROGBITS` section of 64 bytes (`kernel/src/signing.rs:120-140`) | none (raw Ed25519, compile-time anchor) | exact 64 | n/a | covers every ELF byte except itself (`signing.rs:7-12`); verified before admission (`governed_spawn.rs:60-89`) |
| `__ViCell_syscalls` | K | none (raw u64) | none | ≥ 8 read, trailing ignored | all bits meaningful | **gap 1**: missing/short → `u64::MAX` permit-all (`governed_spawn.rs:14-22`) |
| `__ViCell_cluster` | K | none | none | ≥ 16, trailing ignored | pad and mode byte unchecked | **gap 2**: short/missing → `(0, 0)` (`governed_spawn.rs:24-34`) |
| `/POLICY.BIN` | O | magic `"VPOL"` (`kernel/src/policy.rs:33`) | v1..v8 accepted (`policy.rs:75-87`) | header + signature bounded; **no end-of-body check** (`policy.rs:426`) | header flags refused (`policy.rs:272`) | signature verified before parse (`policy.rs:201-204`); invalid → deny-all (`policy.rs:250`) |
| VIFS1 `kernel_fs.img` | S | FAT image | none | FAT-bounded | n/a | mount failure refuses |
| P2 cell table | P | magic `0x5649_4F53_5F43_454C` (`kernel/src/loader/disk_layout.rs`) | **none** | ≤ 128 entries (`MAX_CELL_ENTRIES`, §14); every blob inside P2 | `_pad` unchecked (`disk_layout.rs`) | magic, count and each entry's blob range checked in `EarlyLoader::probe` (`kernel/src/loader/early.rs`); each ELF later signature-checked |
| Block wire | C | `op: u16` (`libs/types/src/block_wire.rs`) | none | `<` checks, trailing accepted | unknown op → NotIssued; unknown reply byte → Indeterminate | parsed before device I/O; since Phase 09C each block Driver Cell serves only the registered VFS provider and answers any other sender NotIssued |
| Syscall 416 `RegisterBlockDriver` (Phase 09C) | K | register envelope `a0 = kind`, `a1 = id`, `a2 = a3 = 0` (`libs/api/src/abi/syscall.rs`, `ostd::syscall::sys_register_block_device`) | none; the closed `kind` set {1 PCI, 2 MMIO} is the policy | four register arguments, exact | unknown kind, PCI id > 0xFFFF, MMIO id 0, non-zero a2/a3 → `usize::MAX - 3`; caller not owning the device → `usize::MAX - 2` | caps, device ownership (BDF lease or exact MMIO window) and the route slot are checked before the slot is written; `RegisterService(BLOCK_DRIVER)` is refused for every caller |
| `ViBlockRouteV1` (syscall 432 `LookupBlockRoute`; Phase 09C) | K | syscall 432 envelope + exact 40 bytes (`libs/api/src/abi/block_route.rs:15`) | u16 version, set {1}; other → `UnsupportedVersion` (`block_route.rs:132`) | `!= 40` refused (`block_route.rs:117`) | `kind`/`id` outside the device rules refused (`:135`); flag bits outside `FLAG_PROVIDER_LIVE` refused (`:138`); provider fields must be zero unless live (`:146`) | full decode before use (`block_route.rs:115-148`); 432 admits only BlockIoCap or SpawnCap holders |
| `ViMachineFactsV1` (syscall 433 `QueryMachineFacts`; plan 261031-1241 phase 03, ADR-0029 §3) | K | syscall 433 envelope; `out_len` must equal `MACHINE_FACTS_CAPACITY` = 2336 (`libs/api/src/abi/machine_facts.rs`) | u16 version, set {1}; other → `UnsupportedVersion` | `total_len` = 288 + 8·`pci_count` = bytes present, `pci_count` ≤ 256; anything else → `Length` (no trailing bytes) | flag bits outside `0x7F`, reserved bytes, string bytes past their length and PCI entry reserved bytes refused; strings and UUID only with their flags; PCI entries only with `PCI_ECAM` and in ascending order | full decode before use (`ViMachineFactsV1::decode`); the trailing SHA-256 is an identity handle, not verified (consumers compare whole records); 433 admits only SpawnCap holders, validates `out_len`/a2/a3 before computing, and has no side effect |
| Machine pin file `/srv/.cellos-machine/pin` (phase 03) | P | the exact `ViMachineFactsV1` bytes of the pinned boot | as the record; an unknown version is safe mode, never "no record" | at most 2336 bytes; larger is unreadable | as the record | init decodes it before comparing; equal bytes → continue, any other readable or unreadable content → safe mode; written only by the live `MACHINE_PIN` provider (VFS rule), never on a store read failure |
| `MachinePinRequest` / `MachinePinResponse` (`api::services::machine_pin`; phase 03) | C | magic `MACHPINQ` / `MACHPINR` + version byte | exact 1 | request exactly 48 (bytes past it in init's 64-byte payload must be zero), response exactly 80 | unknown op/kind/state/refusal codes, reserved bytes and non-zero unused digests refused | init decodes before any effect; caller identity comes from the kernel trailer, never the frame; `Confirm` needs safe mode, the console shell's tid and the current digest |
| Completion record | K | magic `"VICQ"` | exact v1 | `< 24` refused, longer accepted (`libs/api/src/abi/completion.rs:97`) | source bits checked | kernel validates the wait mask first |
| `cell_session` | K | `VERSION = 1` at offset 0 | exact | header 24; total length exact | header reserved and flags refused; unknown selector refused | header decoded first |
| Child context | K | u32 version 1 (`kernel/src/task/child_context.rs:35`) | exact | 48..=max, declared == actual (`child_context.rs:33-36`) | reserved fields refused | validated before publication |
| Telemetry snapshot | K | `TELEMETRY_VERSION = 1` | exact | exact | flags/reserved refused | arguments refused before sampling |
| `ViRetirementV1` (syscall 431 `QueryRetirement`; Phase 09B, committed 9d9e1e10a) | K | syscall 431 envelope + exact 96 bytes (`libs/api/src/abi/retirement.rs:11`) | u16 version, set {1}; other → `UnsupportedVersion` (`retirement.rs:170-172`) | `!= 96` refused (`retirement.rs:143-145`) | `reserved0`/`reserved1` written zero, non-zero refused; undefined `state` and unknown `reasons` bits refused (`retirement.rs:173-181`) | full decode before use (`retirement.rs:142-183`) |
| Caller identity trailer | K | magic `"VICI"` | **none** | last 32 bytes | **gap 5**: bits 8..31 documented must-be-zero (`libs/api/src/abi/caller_identity.rs:62`) but kept (`caller_identity.rs:167`) | written by the kernel after the payload copy |
| Dir attestation | K | magic `"VIDH"` | exact v1 | `<` refused, trailing accepted (`libs/api/src/abi/dir_attestation.rs:88`) | reserved word never read | — |
| Service binding | K | none | **none** | `<` refused, trailing accepted (`libs/api/src/abi/service_binding.rs:74`) | all-nonzero liveness | — |
| KMS frame v1 | C | `abi_version` byte | exact 1 | exact 128 | flags/reserved refused; canonical zero tail | decoded before dispatch; undecodable → no reply |
| CellosFS superblock / inode | P | magic `b"CELL_OSF"`, CRC32C | `FORMAT_VERSION = 1` exact (`libs/cellos-fs/src/format.rs:15`, :163) | 4096 / 512 | **gap 4**: superblock `flags` (`format.rs:50`) never checked; `reserved` not required zero (`format.rs:134`) | **gap 4**: mount writes at once (`cells/services/vfs/src/backend_cellosfs.rs:33`) |
| Boot config TOML | S / P | `version` key | exact 1 | ≤ 16 KiB | unknown fields refused (`deny_unknown_fields`) | validated before any configured cell launches |
| Limine responses | F | base-revision tag | written-back revision not read | null/bounds checked | unknown memmap type → reserved | at boot |
| ACPI tables | F | signatures, checksums | RSDP revision only; SDT revision unchecked | bounded per table | unknown tables skipped | checksum before use; failure closes hardware gates |
| `ViVmExit` | K (values G) | `repr(C, u8)` enum | `VERSION = 2` never sent or checked | size pinned 80 | append-only discriminants | guest decode in HAL |

Not serialized boundaries: `fast_ipc` (zero-sized type token) and `vm.rs`
(`ViVmRuntime`, no users).

## 4. Known gaps

None is fixed in Phase 06. Each fix that changes what a caller, Cell, operator
or volume observes needs its own Law 1 CP1 before code. Each gap names its
current behavior.

1. **Syscall allowlist defaults to permit-all.** A missing, short or unparsable `__ViCell_syscalls` gives `u64::MAX` (`kernel/src/loader/governed_spawn.rs:14-22`). These sections are read by `ElfLoader::get_section` through xmas-elf `find_section_by_name`, with no duplicate check (`kernel/src/loader/elf.rs:405-411`). The bounded `ElfSections` parser used for the manifest and signature refuses duplicates.
2. **The cluster section is lenient.** Its length check is `>= 16`; pad and mode bytes are not range-checked; short or missing becomes isolated (`governed_spawn.rs:24-34`).
3. **A malformed signature section counts as unsigned.** `extract_sig` returns `None` for a wrong size, duplicate or non-`PROGBITS` section (`kernel/src/signing.rs:94-102`, :133). The dev posture then admits the ELF as Tier 2 (`governed_spawn.rs:90-107`). Release posture refuses it (`signing.rs:71-73`).
4. **CellosFS ignores unknown superblock flags and mutates on mount.** An older build can open and then write (`clear_staging`) a volume whose `flags` it does not understand (`libs/cellos-fs/src/format.rs:50`, `cells/services/vfs/src/backend_cellosfs.rs:33`).
5. **Kernel→Cell records accept reserved bits or trailing bytes.** Caller-identity flags 8..31 are documented must-be-zero and kept (`libs/api/src/abi/caller_identity.rs:62`, :167). Completion, dir attestation and service binding check `<`, not `!=` (`completion.rs:97`, `dir_attestation.rs:88`, `service_binding.rs:74`). Do not copy this pattern into new records.
6. **POLICY.BIN accepts trailing bytes and has no sequence.** It returns after the last entry without an end-of-body check (`kernel/src/policy.rs:426`). An older, validly signed blob is replayable [INFERENCE: the header has no epoch field].
7. **The P2 cell table has no version** and does not check its padding (`kernel/src/loader/disk_layout.rs:62-69`).
8. **There is no syscall ABI version.** Unknown numbers fail closed without an audit record (`kernel/src/task/syscall.rs:10036-10038`).
9. **The audit denial codes collide.** `CellSpawnDenied(0, 1)` means "malformed manifest" in `governed_spawn.rs:49-52` and "domain admission feature disabled" in `kernel/src/loader/domain_admission.rs:49`, :262-265; codes 2 to 4 collide the same way. The ring has no consumer (`kernel/src/audit.rs:250-251`), so this is not a public ABI. The Phase 06 witness separates the two by record sequence (§5). **Update 2026-11-11 (architecture remediation Phase 07):** governed-spawn denials now use the 32-byte `audit::encode_subject` payload (spawner `(cell, generation, tid)`, then code, then manifest flags), while domain-admission denials keep the 8-byte `encode_u32x2(0, code)`. The payload length now tells the two families apart; the codes themselves are unchanged.
10. **The audit docs have drifted.** `AuditEvent::CellSignatureFailed` says "Path is logged separately via `log::warn!`" (`kernel/src/audit.rs:95`), but the signature branch logs nothing (`governed_spawn.rs:72-78`). The module header says new writes are dropped when the ring is full (`audit.rs:16-18`), but the ring evicts the oldest records (`audit.rs:180-185`).

## 5. CellManifest record and refusal evidence

**Record.** 16-byte v2 `{magic u32, version u8 = 2, class u8, flags u16, cap_args_off u32 = 0, reserved u32 = 0}` or 8-byte v1 `{magic, version u8 = 1, flags u8, pad [0; 2]}` (`libs/api/src/abi/manifest.rs:11-34`). One section per ELF; a named `SHT_NOBITS` section or a duplicate is malformed (`kernel/src/loader/manifest_section.rs:37-46`). Producer: `declare_manifest!` or the Zig v1 emitter. Consumer: the kernel loader. The section is inside the signed bytes.

**Order on the real admission path** (`kernel/src/loader/governed_spawn.rs`), reached from `SpawnFromElf` and `SpawnFromPath` (`kernel/src/task/syscall.rs:6753`, :6499) after the launch-edge gate (`syscall.rs:1959-1994`):

1. `manifest_section::classify` (:45). `Malformed` writes `CellSpawnDenied` with code 1 (since 2026-11-11 the subject layout `[spawner cell, generation, tid][1][0]`; before that `(0, 1)`) and returns `PermissionDenied` (:48-55). No signature work happens, and no task, TID, CSpace, measurement, route or capability is created.
2. Signature (:60-89). A mismatch writes `CellSignatureFailed(0, 0)` and returns `PermissionDenied`.
3. Owner admission (:113), privilege check (:121), capability derivation (:130), platform reservation (:176-183), task preparation (:206), then publication (:241). A published task writes `CellMeasure(tid, …)` and `CellSpawn(tid, 0)` (`kernel/src/task/launch.rs:262-267`).

Both refusals return `PermissionDenied` to the caller and print nothing. The
evidence therefore reads the kernel's own audit ring.

**Fixtures** (`scripts/x86/abi_refusal_fixtures.py`). Each fixture is the manifest-free `echo` cell with one `__ViCell_manifest` section added **before** signing. The fixtures are then signed by the normal test-image route: `scripts/lib-sign-cells.sh` `sign_cells` runs `cellos-sign --sign`, which uses the dev key and passes the F1/F5 check. This route is authorized by the test-key rule in [code standards § Cell Signing](code-standards.md#cell-signing-test-key-vs-release-key), and the user admitted Phase 06's private signed refusal fixtures on 2026-11-01. The signer is not modified. Only `abifx-sigbad` is damaged after signing (one signature bit), and it carries a valid record: it is signature evidence only. Every fixture's signature is checked on the host with `scripts/sign-cell.py --verify`.

The launch-edge gate allows init to launch only exact reviewed paths
(`kernel/src/loader/launch_profile/profiles.rs:55-59`,
`kernel/src/loader/launch_profile/targets.rs:29-71`). Each fixture is therefore
installed at a distinct reviewed `LEGACY` slot in a private image. The slot name
says nothing about the bytes.

| Fixture | Slot | Manifest bytes | Host signature | Expected |
|---|---|---|---|---|
| abifx-v2 | /bin/echo | `4543495602ff00000000000000000000` | valid | admitted (current v2) |
| abifx-badver | /bin/cat | `4543495603ff00000000000000000000` | valid | format refusal (version 3) |
| abifx-v1 | /bin/ls | `4543495601000000` | valid | admitted (older v1) |
| abifx-badflag | /bin/ps | `4543495602ff00100000000000000000` | valid | format refusal (flag bit 12) |
| abifx-v1pad | /bin/free | `4543495601000100` | valid | format refusal (v1 padding) |
| abifx-badrsv | /bin/wx-test | `4543495602ff00000000000001000000` | valid | format refusal (reserved) |
| abifx-capargs | /bin/cfi-test | `4543495602ff00000100000000000000` | valid | format refusal (`cap_args_off`) |
| abifx-badtier | /bin/futex-test | `45434956020400000000000000000000` | valid | format refusal (class 4) |
| abifx-v1len | /bin/tls-test | `45434956010000000000000000000000` | valid | format refusal (v1 tag, 16 bytes) |
| abifx-sigbad | /bin/c-pthread | `4543495602ff00000000000000000000` | **invalid** | signature refusal |
| abifx-v2b | /bin/pipe-peer | `4543495602ff00000000000000000000` | valid | admitted after the refusals |

**Image.** `bash scripts/x86/make-abi-refusal-iso.sh` builds a private image in
`target/abi-refusal-06` with the canonical kernel features (default posture
plus `policy-required`). The kernel, init and signer are unchanged. Its
`autoload.toml` lists only the fixtures, in order.

**Runner.** `python3 scripts/x86/run-abi-refusal-witness.py` boots q35 under
TCG and waits until init has reported every fixture. It then pauses the guest,
copies `audit::RING` (address from the kernel symbol table) with QMP `memsave`,
and matches the trailing admission attempts against the table. A refused
fixture yields one attempt per init route (ELF, then path). A control must show
its own `CellExit` record. Host tests:
`python3 scripts/x86/test_abi_refusal_fixtures.py`.

**Observed** on 2026-11-01. The run was repeated; both runs gave identical
results.

```text
[ INFO] [signing] cell trust posture=dev anchor=[22, fc, 29, 77, 92, f0, b6, ff] unsigned=tier2
ABI-FX: audit ring @0xffffffff8061c3b0 records=77 layout={'buf_offset': 0, 'head': 1386, 'tail': 0, 'dropped': 0}
ABI-FX abifx-v2 /bin/echo: PASS expect=admit observed=['admit'] attempts=1 records=[CellSignatureVerified(0, 0)+CapNarrowedByPolicy(7, 57344)+CellMeasure(7, 2016906938)+CellSpawn(7, 0)] exit(tid=7)=0
ABI-FX abifx-badver /bin/cat: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-v1 /bin/ls: PASS expect=admit observed=['admit'] attempts=1 records=[CellSignatureVerified(0, 0)+CapNarrowedByPolicy(8, 57344)+CellMeasure(8, 2209215457)+CellSpawn(8, 0)] exit(tid=8)=0
ABI-FX abifx-badflag /bin/ps: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-v1pad /bin/free: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-badrsv /bin/wx-test: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-capargs /bin/cfi-test: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-badtier /bin/futex-test: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-v1len /bin/tls-test: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]
ABI-FX abifx-sigbad /bin/c-pthread: PASS expect=signature-refused observed=['signature-refused'] attempts=2 records=[CellSignatureFailed(0, 0); CellSignatureFailed(0, 0)]
ABI-FX abifx-v2b /bin/pipe-peer: PASS expect=admit observed=['admit'] attempts=1 records=[CellSignatureVerified(0, 0)+CapNarrowedByPolicy(9, 57344)+CellMeasure(9, 2016906938)+CellSpawn(9, 0)] exit(tid=9)=0
ABI-FX: PASS
```

What this shows:

- **Refused before effect.** None of the 14 format attempts or the 2 signature attempts has a `CellSignatureVerified`, `CellMeasure` or `CellSpawn` record. The controls on either side were published as consecutive TIDs 7, 8 and 9, so the 16 refused attempts allocated no TID. A format refusal stops before the signature check.
- **Signature-invalid and format-invalid are distinct.** A signature refusal is `CellSignatureFailed(0, 0)`. A format refusal is `CellSpawnDenied` with code 1 (`(0, 1)` in the transcript above; the subject layout since 2026-11-11, which `abi_refusal_fixtures.py` reads) with no preceding `CellSignatureVerified`, even though every format fixture's signature is valid on the host. That sequence excludes the colliding domain-admission code (gap 9), which can only follow a verified signature for a signed ELF.
- **Compatible records still run.** v1 and current v2 were admitted, published, ran and exited with status 0. In the same boot the image's real cells with built-in v2 manifests were also admitted: platform, ahci, vfs (flags `0x00c1`) and input. `serial` has no manifest section.

Ceiling: x86 q35 QEMU/TCG, one vCPU, private image. It is not KVM evidence or
physical-Intel evidence. Existing host coverage of the same decoder:
`libs/api/src/abi/manifest_compat_tests.rs` and `manifest_tests.rs`, plus the
RV64 test-hooks continuity guard
`tests/integration/tests/manifest-qemu-continuity.rs`.

## 6. Records from ADR-0031 (x86 platform breadth)

Each needs Law-1 CP1 before implementation and CP2 after it.

### 6.1 Implemented in phase 04a (CP2 approved 2026-11-04)

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | Code |
|---|---|---|---|---|---|---|---|
| `ViPcieFunctionV1` (syscall 434 `QueryPcieFunction`) | K | syscall 434 envelope + exact 584 bytes | u16 version, set {1} | `out_len != 584` → `InvalidInput` | flag bits beyond 0-3 (bit 3 `DMA_REMAPPED`, CP2 amendment 2026-11-04), BAR kinds above 3, BAR flag bits beyond 0-1, `reserved*`, cap entries past `cap_count` and cap bytes past `len` all zero (`ViPcieFunctionV1::decode` refuses otherwise); `a3` zero | caller holds `PcieDriverCap` and owns the BDF; the record is assembled before the copy-out | `libs/api/src/abi/pcie_function.rs`; `kernel/src/task/drivers/pcie_msi/record.rs` |
| `AllocPcieIrq` result (syscall 435) | K | register envelope; return `first_irq \| count<<8 \| kind<<16` | none (register form) | n/a | `want` outside 1..=32, empty mode or mode bits beyond 0-1 refused; `a3` zero | ownership, capability presence, carve rule and FADT MSI flag checked before any config write | `kernel/src/task/drivers/pcie_msi.rs` |
| 418/424 lookup semantics | K | unchanged 48-byte `PcieDeviceInfo` | unchanged | unchanged | unchanged | skip devices owned by another live task (semantic change only) | `kernel/src/task/drivers/pcie_ecam.rs` (`find_class_where`, `find_vendor_device`) |

Error sentinels for 434/435 (`api::pcie_function::ERR_*`): `usize::MAX`
permission denied, `MAX - 2` no function, `MAX - 3` invalid input,
`MAX - 5` not supported, `MAX - 6` vectors exhausted. Cells reach them through
`ostd::syscall::{sys_query_pcie_function, sys_alloc_pcie_irq}`.

Envelope: the cell syscall stub carries four arguments, so "`a3..a5` zero" in
CP1 is enforced as `a3` zero; `a4`/`a5` never reach the kernel.

### 6.2 Implemented in phase 06 (CP2-06 pending owner decision)

Implemented on `feat/x86-s2-phase06-power` ([CP2-06](../.agents/261103-2218-x86-platform-breadth/cp2-06-system-power.md)); not frozen until the owner approves CP2-06. CP1: [package](../.agents/261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md) §5.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | Code |
|---|---|---|---|---|---|---|---|
| `SystemPower` (syscall 436) | K | register envelope; `a0` 1 reboot, 2 poweroff; no return on success | none (register form) | n/a | unknown action or non-zero `a1..a3` → `InvalidInput` (checked after the gate) | `SupervisorCap` (allowlist bit 49) before any argument is acted on; poweroff needs a static `\_S5_` and an I/O PM1 control block, else `NotSupported` before any write | `kernel/src/task/syscall.rs` (`system_power`); `kernel/src/acpi/power.rs` |

Error sentinels for 436 (`api::syscall::system_power::ERR_*`): `usize::MAX`
permission denied, `MAX - 3` invalid input, `MAX - 5` not supported (the
434/435 values). Same four-argument envelope: `a4`/`a5` never reach the kernel.
Cells reach them through `ostd::syscall::sys_system_power`.

## 7. Records from ADR-0033 (Intel iGPU compute)

CP1 confirmation 1 was approved on 2026-11-04 ([package](../.agents/261104-0801-x86-igpu-compute/cp1-gpu-kernel-abi.md), [approval](../.agents/261104-0801-x86-igpu-compute/cp1-approval.md)). The GPU requests themselves are the existing Linux `xe` uAPI carried over IPC; Cellos adds no GPU ABI.

**437/438/439 and `ViDmaExportV1`: approved 2026-11-10 by user decision (implement per CP1 §5); implemented 2026-11-10; CP2 confirmed by the user on 2026-11-10, frozen.** Numbers, record and sentinels are exactly CP1 §5 (D-G13). Constants and codec: `libs/api/src/abi/dma_export.rs` (`ERR_*`: `MAX` denied, `MAX-1` try again, `MAX-2` not found, `MAX-3` invalid, `MAX-4` buffer too small, `MAX-5` not supported, `MAX-6` out of memory, `MAX-7` unknown = 438 published-unconfirmed); kernel: `kernel/src/task/dma_export.rs` (lease table, state machine `Live → Revoking → dropped`, host tests in `dma_export_tests.rs`), the arms and `encode_dma_export_result` in `kernel/src/task/syscall.rs`, the window map/unmap in `kernel/src/task/drivers/iommu_x86.rs` (`map_window_for_cell`, `unmap_window_for_cell`, `bdf_translating`) and `iommu_pt.rs` (`VtdSlpt::{window_free, map_window}`, `R`-only leaves), the owner-range lock and `ReleaseKey::DmaExport` in `kernel/src/memory/pin.rs`; cells: `ostd::syscall::{sys_export_dma_range, sys_map_dma_export, sys_map_dma_export_into, sys_revoke_dma_export}`, `ostd::dma::{DmaBuf::export, map_export, revoke_export}`. Kernel-internal choices (no wire change): an exported range must lie inside one live SAS Grant the caller owns (private-root callers are refused `PermissionDenied` until D-G0 memory exists; a range under a live VFS lease is refused); limits 4096 leases, 1024 per owner, `EXPORT_MAX_BYTES` 2^32; windows are first-fit in `[2^38, 2^39)` per holder task (2 MiB aligned when ≥ 2 MiB) and are reused only after the drain is acknowledged; 438 charges an upper bound of the window's SLPT pages to the holder Cell's DMA quota and refuses a window that would overwrite any present leaf; exports are refused (`NotSupported`) unless VT-d translation is active and RAM ends below 2^38; `DMA_EXPORT_SERVICES = [XE_GPU]`, plus the test-only id `0x7AE0` in `test-hooks` kernels. Death ordering: owner death revokes the owner's leases first in `reap_retired_task_resources` (before its grants are reaped; unacknowledged → frames quarantined under `DmaExport{owner, holder_cell, export_id}` and freed only by the acknowledged per-pass retry); holder death drops its leases only after `teardown_dma_authority` acknowledged the whole-domain teardown. QEMU evidence: `scripts/x86/qemu-edu-export-test.sh` (lanes L1-L5; L6 fault injection not built).

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP1 |
|---|---|---|---|---|---|---|---|
| `ExportDmaRange` (syscall 437) | K | register envelope `a0 va, a1 len, a2 holder_service, a3 flags` → export id `1..=isize::MAX` (boot-wide counter, never reused) | none | n/a | `va`/`len` 4 KiB aligned, `1 ≤ len ≤ 2^32`, `a2 ≤ 0xFFFF`, `flags ⊆ {EXPORT_DEVICE_WRITE}` else `InvalidInput` | service in `DMA_EXPORT_SERVICES` (else `PermissionDenied`); translating remapper (else `NotSupported`); live bound provider (else `FileNotFound`) holding `PcieDriverCap` and not the caller's incarnation (else `PermissionDenied`); lease table room (else `OutOfMemory`); range inside one live caller-owned Grant, proved and locked in one grant-table transaction (else `PermissionDenied`) — every refusal before any lease, lock, window or SLPT change; no allowlist bit | package §5.1; implemented 2026-11-10, CP2 confirmed 2026-11-10 |
| `MapDmaExport` (syscall 438) → `ViDmaExportV1` | K | syscall 438 envelope `a0 export_id, a1 bdf, a2 out_ptr, a3 out_len` + exact 48 bytes | u16 version, set {1} | `out_len != 48` → `InvalidInput`; `out_ptr` probed writable before any effect | `bdf ≤ 0xFFFF`; record `reserved0` zero, flags ⊆ {`EXPORT_DEVICE_WRITE`}; decoder refuses other versions, lengths, reserved bytes and flags | allowlist bit 48 (shared with `GrantDma`); `PcieDriverCap` and BDF ownership (else `PermissionDenied`); caller incarnation is the lease holder (else `PermissionDenied`); lease Live (else `FileNotFound`); the BDF's unit translating (else `NotSupported`); window space, SLPT quota, a free window (else `OutOfMemory`); the `GrantDma` retirement barrier is held across publication; the record is built before the copy-out; an unacknowledged invalidation → `Unknown`, lease stays mapped-unconfirmed until revoke/death, bus mastering not enabled; repeat call → same record (another owned BDF of the same holder task joins the same window) | package §5.2; implemented 2026-11-10, CP2 confirmed 2026-11-10 |
| `RevokeDmaExport` (syscall 439) | K | register envelope `a0 export_id` → 0 | none | n/a | `a1..a3` zero, else `InvalidInput` | caller incarnation is the lease owner or holder; anyone else (and an unknown or dropped id) gets `FileNotFound`; clears the window leaves and drains the domain IOTLB (re-drained on every retry); unacknowledged → `TryAgain`, lease Revoking, range locked, retried each scheduler pass | package §5.3; implemented 2026-11-10, CP2 confirmed 2026-11-10 |
| `service::XE_GPU = 21` | K | registry constant | n/a | n/a | n/a | init-registered; added to `OWNER_WATCH_SERVICES` | package §5.4 |
| `AnonMemory` (syscall 440), op-multiplexed map / unmap / protect | K | register envelope | none | n/a | per package; `prot` ∈ {none, R, RW}, never EXEC | x86_64 private-root callers only (others `NotSupported`); commit charged against the `POLICY.BIN` v9 `anon_limit_mib` budget before any frame is mapped; ranges under a live DMA export return `TryAgain`; no allowlist bit | [package](../.agents/261104-0801-x86-igpu-compute/cp1-tier2-large-memory.md) (approved 2026-11-04, [record](../.agents/261104-0801-x86-igpu-compute/cp1-tier2-large-memory-approval.md)) |
| `POLICY.BIN` v9 `anon_limit_mib` | K, inside signed bytes | policy version 9 | v9 | per package | per package | per package | same package (D-M4) |

## 8. Records from the x86 Tier-2 runtime plan (slice RE)

CP1 AX-1…AX-8 approved 2026-11-04 ([package](../.agents/261104-0802-x86-tier2-gpu-runtime/cp1-cell-initial-stack-auxv.md), [approval](../.agents/261104-0802-x86-tier2-gpu-runtime/cp1-cell-initial-stack-auxv-approval.md)); implemented in slice RE; **CP2 approved 2026-11-04, frozen** ([package](../.agents/261104-0802-x86-tier2-gpu-runtime/cp2-cell-initial-stack-auxv.md), [approval](../.agents/261104-0802-x86-tier2-gpu-runtime/cp2-cell-initial-stack-auxv-approval.md)). The host tests in `libs/cell-initial-stack` pin the layout.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| Cell initial stack v1 (argc, argv, envp, auxv at the entry `sp`) | K | layout pinned by `libs/cell-initial-stack` (64-bit LE words, `sp` 16-byte aligned) | v1 = this layout; no version word — a change is a new CP | bounded: ≤ 16 program headers, ≤ 8 auxv pairs, ≤ `MAX_BYTES` (1072) | `argc` = 0, argv/envp empty; `AT_BASE` carries the load base (Cellos meaning); unknown auxv keys are not emitted | built before the task exists; a header table that is not 56-byte entries or exceeds 16 entries refuses the spawn (`InvalidInput`) | CP2 approved, frozen |

## 9. Records from the `/srv/sshd` image-identity slice (audit §9)

Approved and frozen on 2026-11-05 by two explicit user confirmations. 437-440 stay reserved for §7.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `ResolveCellImage` (syscall 441) → `CellImage` record | K | syscall 441 envelope; request = `CellOwnerRequest` (16 bytes, a0/a1), output a2/a3 ≥ 80 bytes (`libs/api/src/abi/cell-image.rs`) | u16 version, set {1}; other refused by `CellImage::from_bytes` | exact 80 written; decode refuses `!= 80` | reserved bytes 4..8 zero; anchor ∈ {0 unsigned, 1 dev, 2 release, 3 private}; `name_len` ≤ 24, name bytes past it zero; a name only under a signing anchor; zero cell/generation refused | the caller task must belong to the registered VFS Cell, or hold `SupervisorCap` and ask about a Cell whose root was launched from a kernel-resolved `/bin` install (`Task::launch_named`; architecture remediation Phase 06 CP1 option A, 2026-11-11), and `(cell_id, generation)` must be live, else `PermissionDenied`; the record is read from the root task's immutable `Task::image`, bound once before publication (`kernel/src/task/cell-image.rs`); no allowlist bit | approved 2026-11-05, frozen |
| `__ViCell_image` | K, inside signed bytes | one non-`NOBITS` section, emitted by `scripts/sign-cell.py` from the artifact name minus `service-`/`app-`/`driver-` | none (raw name) | 1..=24 bytes `[a-z0-9_-]` | absent, duplicated, `NOBITS` or invalid → no name, the image still loads | read with the duplicate-refusing `ElfSections` parser and believed only when the signature verified (`kernel/src/loader/image-name-section.rs`) | approved 2026-11-05, frozen |

## 10. Records from the compositor Grant-owner slice (audit H-27)

Approved and frozen on 2026-11-06 by two explicit user confirmations. No new syscall number: GrantSlice's previously ignored `a2` (always passed as 0) gains a meaning, and `a2 = 0` keeps the old behavior.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `GrantSlice` (syscall 210) `a2 = expected_owner_tid` | K | register envelope `a0 = grant_id`, `a1 = size_out` pointer, `a2 = expected_owner_tid` (`libs/api/src/abi/syscall.rs`) | none (register form) | n/a | `a2 = 0`: no owner expectation (unchanged). `a2 ≠ 0` and not the tid that allocated/registered the Grant → `usize::MAX`, exactly the unknown-id result: checked at the point an unknown id is refused, before any log, receiver mapping or VFS lease, so `size_out` handling and every other effect match an unknown id | compared under the grant-table lock that resolves the row, on the SAS and the private-root path (`kernel/src/task/syscall.rs`, `grant_owner_expected`); cells reach it through `ostd::syscall::sys_grant_slice_with_len_owned_by`, which refuses `owner_tid = 0`; every other wrapper passes 0 | approved 2026-11-06, frozen |

## 11. Records from the non-x86 boot-MMIO hardening completion (5fabb206f follow-up)

Approved on 2026-11-10 by explicit user choice ("finish the hardening: new read-only ProbeVirtioMmio syscall"). Boot MMIO on riscv64/aarch64 is kernel-only, so a VirtIO Cell can no longer read the transport header before it owns the slot.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `ProbeVirtioMmio` (syscall 442) → packed slot identity | K | register envelope `a0 = slot base`; success word = device id (bits 0..16) \| status byte (bits 16..24) (`libs/api/src/abi/syscall.rs` `virtio_probe`) | none (register form) | n/a | `a1..a3` must be 0, else `usize::MAX - 3`; no VirtIO magic or device id > 16 bits → `usize::MAX - 2`; x86_64 → `NotSupported` | caller must hold `PcieDriverCap` (else `usize::MAX`); `a0` must equal one of `virtio_common::virtio_slots()`; the kernel reads magic/device id/status through its own mapping, no user mapping is created and no ownership is taken; shares allowlist bit 40 with `RequestMmio` | approved 2026-11-10 |

## 12. Records from the Intel iGPU driver service (ADR-0033, `XE_GPU` IPC)

Implemented on 2026-11-10 as the user-approved follow-up "finish the Intel GPU driver IPC"; approved and frozen on 2026-11-10 by explicit user confirmation of this wire (Law-1; CP1 had approved `service::XE_GPU = 21` and the `xe` uAPI as the request format, this records the framing). The frames carry one Linux `xe`/DRM ioctl unchanged; the framing adds only the DRM file lifecycle and the user ranges a struct points at. Codec and host tests: `libs/intel-gpu-core/src/wire.rs`, `server.rs`; driver `cells/drivers/intel-gpu/src/service.rs`; client `cells/runtimes/mesa-anv/src/shim.rs`.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `XE_GPU` request (client → driver, `IpcSubmit` or `Send`) | C | service id 21 (init-registered); 16-byte header `op u8 ∈ {1 OPEN, 2 CLOSE, 3 IOCTL}`, `segment_count u8 ≤ 4`, `arg_len u16`, `file u32`, `request u32` (full Linux ioctl code), `reserved u32`; then `segment_count` × `{addr u64, len u32, flags u32}`, the `arg` bytes, the bytes of every `IN` segment | none: the `xe` request code and struct size identify the payload | bounded by `MAX_REQUEST_LEN` (4064 = `IPC_BUF_SIZE` − caller-identity trailer); the header counts define the frame because a receive reports no length, so bytes after it in the receive buffer are ignored (the driver zeroes the buffer before each receive) | header `reserved` must be 0; segment `flags` ⊆ `{IN, OUT}` and non-zero; unknown `op` → `EINVAL`; `arg_len` ≠ `ioctl_size(request)` → `EINVAL` from the decoder | the whole frame is decoded, the caller identity attested (`sender_tid` must equal the kernel-reported sender) and the file resolved per sender tid before the ioctl runs; reads outside `IN` segments and writes outside `OUT` segments fail `EFAULT` | approved 2026-11-10, frozen |
| `XE_GPU` reply (driver → client) | C | 16-byte header `errno u32` (0 = success), `value u32` (OPEN: file id), `arg_len u16`, `write_count u8 ≤ 8`, `reserved u8`, `reserved u32`; then `write_count` × `{addr u64, len u32, reserved u32}`, the `arg` (copied back as `drm_ioctl` does, even on error), the bytes of each write | none | exact (`IpcTake` reports the length), ≤ 4096 | reserved fields must be 0, else the client refuses the reply (`EIO`) | the client decodes the whole reply and refuses it whole (`EFAULT` → `EIO`) if any write leaves the `OUT` ranges it declared, before copying the `arg` back or applying a write | approved 2026-11-10, frozen |
| `XE_GPU` export attachment (amendment to the request row; CP1 D-G1 option B "export id as a transport attachment") | C | a segment descriptor `{addr = export id, len = 0, flags = SEG_EXPORT (1 << 2)}`; no header change; the `drm_xe_gem_create` struct travels unchanged | none | counts toward `segment_count ≤ 4`; carries no data bytes | id in `1..=i64::MAX`, `len` 0, `SEG_EXPORT` alone; else `EINVAL` from both `encode_request` and `decode_request` | only `GEM_CREATE` may carry one, and exactly one (`EINVAL` otherwise, including on OPEN/CLOSE); the driver maps it with 438 and refuses (`EPERM`) unless the record's `owner_cell_id`/`owner_generation` equal the request's kernel-attested caller identity; `len < size`, a misaligned IOVA or a lease already backing another object → `EINVAL`. Codec `libs/intel-gpu-core/src/wire{,/request}.rs`; driver `cells/drivers/intel-gpu/src/{gpu,service}.rs`; shim `cells/runtimes/mesa-anv/src/{shim,memory}.rs` | implemented 2026-11-10, CP2 confirmed 2026-11-10 |

## 13. Records from the compositor Grant-lease slice (pixels.rs owner-free window)

Approved 2026-11-10 by user decision; CP2 confirmed by the user on 2026-11-10, frozen. No new syscall number: GrantSlice's previously ignored `a3` (every caller passed 0) gains flag bits, and `a3 = 0` keeps the old behavior. Constants: `libs/api/src/abi/syscall.rs` `grant_slice`; kernel: `kernel/src/task/syscall.rs` (`lease_grant_slice_locked`, `end_grant_leases_at_send`, the receive hook in `handle_syscall_inner`) and `kernel/src/memory/pin.rs` (`lease_grant`, `release_grant_holder_leases`, `release_grant_leases_to_owner`); cells reach it through `ostd::syscall::sys_grant_slice_with_len_leased_owned_by`. Receive forms: `Recv`, `RecvTimeout`, `TryRecv`, `RecvScatter` (masked or not). Send forms: `Send`, `TrySend`, `SendGather`, `IpcSubmit`, and `Reply` (addressed to the caller's current IPC caller); `IpcReply` is not a boundary. A leased domain receiver keeps the frames out of the allocator, but owner death still revokes its receiver mapping.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| GrantSlice (syscall 210) a3 = flags, bit 0 LEASE | K | register envelope a0 = grant_id, a1 = size_out, a2 = expected_owner_tid (≠ 0 when LEASE), a3 = flags | none (register form) | n/a | a3 bits other than 0 → usize::MAX; LEASE with a2 = 0 → usize::MAX; holder lease table full → usize::MAX | resolve, owner check and lease publication in one grant-table transaction; the lease ends at the holder's next receive, its next send to the Grant owner, or holder death; meanwhile the owner's GrantFree/GrantUnregister → PermissionDenied and owner death quarantines the frames until release | approved 2026-11-10, CP2 confirmed 2026-11-10 |

## 14. Records from the P2 cell-table bound and P2/P3 resize (2026-11-10)

Directed 2026-11-10 by Main after `scripts/qemu-lua-vnet.py` hit the 64-entry cap and the shipped `disk_v3.img` was found writing cell blobs up to LBA ~572,077, past P2's end (560,000) into the P3 snapshot partition. CP2 (post-implementation confirmation) pending. The cell-table record format is unchanged (header `count`, one sector per entry, explicit `data_lba`); only acceptance bounds and two partition addresses move. Constants: `libs/api/src/abi/disk.rs`; kernel: `kernel/src/loader/disk_layout.rs`, `kernel/src/loader/early.rs`; writers `tools/write-cell-table.py`, `tools/add-cell-to-disk.py`, `tools/write-mbr.py`, `tools/write-rpi3-mbr.py --tier3`; parity and bound tests `tools/test_cell_table.py`.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| P2 cell table entry bound | P | `MAX_CELL_ENTRIES` 64 → 128 in the kernel and both writers (a reader acceptance bound, not a layout offset) | none (unchanged) | `count ≤ 128`; header + 128 entries + P6's 65,536 sectors fit P2 (kernel `const` assert) | an older kernel rejects a > 64-entry table whole and resolves through VIFS1 | writers refuse > 128 entries and any table whose last blob would end past P2 (exit 1 before writing); the probe refuses a table with an entry whose blob starts before the end of the entry array or ends past P2 | CP2 confirmed 2026-11-10 |
| MBR partition map P2/P3 | P | P2 0x7F `526,336 + 33,664` → `526,336 + 73,664`; P3 0x7D `560,000 + 240,000` → `600,000 + 200,000`; P1, P4, P5, P6 and P3's end (800,000) unchanged; contiguity is a `const` assert in `api::disk` | none | P3 still ≥ 64 MiB heap + headers (~97 MiB); snapshot capacity bound derives from `PART_SNAPSHOT_SECTORS` | an image written with the old map gets the `verify_mbr` drift warning; a snapshot at the old P3 base is not found (cold boot) | snapshot restore validates header magic/geometry at the new base before any write; an old image's cell table (blobs < 600,000) still passes the new P2 bound | CP2 confirmed 2026-11-10 (user accepted the layout change; existing images must be regenerated) |

## 15. Records from the per-Cell forward-edge CFI opt-in (item 3, BTI / CET-IBT)

Approved 2026-11-10 by user decision; CP2 confirmed by the user on 2026-11-10, frozen. No syscall, auxv or layout change: the kernel loader starts *reading* a standard ELF note the toolchain already emits, and the behaviour of every image without it is unchanged. Parser and arming: `kernel/src/loader/gnu_property.rs`; GP bit: `hal/arch/arm/src/aarch64/paging.rs` (`PageFlags::GUARDED`); U_CET switch: `hal/arch/x86/src/x86_64/cet.rs` (`switch_user_cet`), called from `kernel/src/task/scheduler.rs`. The aarch64 cell cannot read `ID_AA64PFR1_EL1`, so no hardware-capability channel was added (user decision 2026-11-10, option B: `scripts/qemu-cfi-test.sh` judges BTI enforcement from the boot log).

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| Cell ELF `PT_GNU_PROPERTY` forward-edge CFI opt-in | K (+O: inside the signed image) | `p_type 0x6474e553`; one `NT_GNU_PROPERTY_TYPE_0` note, name `GNU\0`; aarch64 reads `GNU_PROPERTY_AARCH64_FEATURE_1_AND` (0xc0000000) bit 0 BTI, x86_64 reads `GNU_PROPERTY_X86_FEATURE_1_AND` (0xc0000002) bit 0 IBT; other machines/bits ignored | none (GNU gABI note) | segment ≤ 1024 bytes and inside the file; `n_descsz` a multiple of 8; each `pr_data` padded to 8; feature word `pr_datasz` = 4 | absent segment, ELF32, or bit clear → not opted in (unchanged behaviour); other `pr_type`s skipped | read from the admitted bytes before any mapping; more than one segment, wrong name/type, truncation, unsorted/duplicate `pr_type`, or a feature word of another size → spawn refused (`InvalidInput`), like Linux `ENOEXEC`. Armed only when the CPU implements the mechanism: aarch64 FEAT_BTI → executable pages mapped with stage-1 GP (bit 50); x86_64 CPUID IBT + CR4.CET → task `IA32_U_CET = ENDBR_EN \| NO_TRACK_EN` (legacy bitmap off), saved/restored with tracker state on switch, inherited by the Cell's threads (state bits cleared); a CPL3 `#CP` / EL0 EC 0x0D terminates only that Cell | approved 2026-11-10, CP2 confirmed 2026-11-10 |

## 16. Records from the CellOS Linear chrome slice (compositor appearance)

Approved 2026-11-11 by user decisions (new op chosen over a desktop-only theme; push event chosen over a start-up query; CP2 confirmation pending). Two new compositor opcodes and one new compositor event; no existing frame or discriminant changes. Wire type: `libs/api/src/services/display/appearance.rs`; sender: `ostd::display::ViSurface::set_appearance`; consumer: `cells/services/compositor/src/appearance.rs`.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `compositor_ops::SET_APPEARANCE` (0x12) `SetAppearance` | C | opcode byte 0x12; LE `opcode:u8, palette:u8, reserved:[u8;2], cap:u32, revision:u32` | none (fixed frame) | exactly 12 bytes; no reply | reserved ≠ 0 → refused; `palette` outside Light 0 / Dark 1 / Paper 2 → refused (`InvalidDiscriminant`) | the full frame decodes first; then `cap` must be a Panel surface owned by the sender (only the shell holds Panels) and, for the same owner, `revision` must exceed the last accepted one (a new Panel owner starts its own sequence). Only then does the chrome palette change and the screen repaint; a refused frame has no effect | CP1 2026-11-11 (user), CP2 pending |
| `compositor_events::APPEARANCE_CHANGED` (0xA3) `AppearanceChanged` | C | opcode byte 0xA3; LE `opcode:u8, palette:u8, reserved:[u8;2], generation:u32` | none (fixed frame) | 8 bytes, read from the head of a receive buffer | reserved ≠ 0, `generation` 0 or an unknown palette → ignored as not-this-event. `generation` starts at 1, so a `CREATE_SURFACE` reply (`cap:u32` + four zero bytes) is never taken for it (ostd `create_surface` relies on this) | compositor sends it to every surface owner after an accepted palette change and to a client after each successful `ATTACH_GRANT` reply; frames from one compositor arrive in order and the latest wins. ostd records it on every compositor receive path (`ostd::display::appearance`), ViUI paints with it (`viui::theme::system`); nothing else changes state | CP1 2026-11-11 (user), CP2 pending |
| `compositor_ops::SET_ICON` (0x13) `SetIcon` | C | opcode byte 0x13; LE `opcode:u8, icon:u8, reserved:[u8;2], cap:u32` | none (fixed frame) | 8 bytes, read from the head of the request | reserved ≠ 0 → refused; `icon` must be an `ostd::display::linear::icons::LineIcon` wire value known to this compositor (append-only enum), else refused | decoded first; `cap` must be a surface owned by the sender; only then the Cell Tab icon changes and the tab repaints. A surface that never sends it shows `LineIcon::Application` | CP1 2026-11-11 (user), CP2 pending |

## 17. Records from the Tier 3 Chrome launcher session (ADR-0017 §5d, plan phase 3)

Launcher ↔ hypervisor protocol `api::services::vmm_session` (sender `cells/apps/chrome/src/guest.rs`, consumer `cells/services/hypervisor/src/app_session_x86.rs`; events the other way from `virtio_gpu/client_scanout.rs`). The session binds to the sender of the first accepted LAUNCH of a hypervisor spawned as `/bin/hypervisor app`; every later frame from any other sender is dropped. CP1 pending (user), CP2 pending.

| Family | Class | Identity | Version | Length | Reserved / unknown | Validation before effect | CP |
|---|---|---|---|---|---|---|---|
| `VMM_OP_LAUNCH_APP` (1) request / `VmmLaunchReply` | C | opcode byte 1; LE `opcode:u8, reserved:[u8;3], width:u32, height:u32, app_name:[u8;32]`; reply LE `status, session_id, reserved, scanout_width, scanout_height` (u32 each) | none (fixed frame) | request 44 bytes, reply 20 | reply word 2 (formerly `surface_cap`) is now reserved, written 0 | the size must satisfy `display_size_valid` (64..=4096 per edge) before the session binds and the guest boots; a refused size answers `VMM_STATUS_INVALID_ARG` and binds nothing | CP1 pending |
| `VMM_OP_RESIZE_APP` (2) `VmmResizeRequest` | C | opcode byte 2; LE `opcode:u8, pad:[u8;3], session_id:u32, width:u32, height:u32` | none | 16 bytes; no reply | pad written 0, ignored | bound sender, `session_id` and `display_size_valid` checked before the GPU display size changes and the config-change interrupt is raised | CP1 pending |
| `VMM_OP_FORWARD_INPUT` (3) `VmmInputForwardRequest` | C | unchanged layout | none | 16 bytes | accepted pairs: `EV_KEY`/any code (value ≠ 0 = pressed), `EV_ABS`/`ABS_X`,`ABS_Y` (value clamped to `0..=ABS_MAX` 0x7fff), `EV_REL`/`REL_WHEEL`; relative `REL_X`/`REL_Y` are no longer accepted (the guest device is a tablet); others dropped | bound sender and `session_id` checked before the event is queued | CP1 pending |
| `VMM_OP_TERMINATE_APP` (5) | C | opcode byte 5, 3 pad bytes | none | 4 bytes | — | bound sender only; the hypervisor then stops the vCPU, posts `VMM_EVT_EXITED` and exits | CP1 pending |
| `VMM_EVT_SCANOUT` (0x81) / `VMM_EVT_DAMAGE` (0x82) / `VMM_EVT_EXITED` (0x83) `VmmEvent` | C | opcode byte; LE u32 words at 4..20: SCANOUT `grant, width, height, 0`; DAMAGE `x, y, width, height`; EXITED `code, 0, 0, 0` | none | 20 bytes, posted (never sent) | unused words 0; any other opcode is not an event | launcher maps only a Grant owned by the hypervisor tid it spawned (`sys_grant_copy_rows_owned_by`, leased per copy) and clips every damage rectangle to both the Grant and its surface before copying | CP1 pending |
| ostd `sys_grant_copy_rows_owned_by` + `GrantRows` | K (SDK) | safe facade over `GrantSlice` (owner-checked, leased) | — | — | — | every row bound-checked against the leased Grant length and the destination before any copy; refuses a Grant not owned by `owner_tid` | CP1 pending |
| Launch edge `chrome` → `/bin/hypervisor` | K | `kernel/src/loader/launch_profile` `chrome_profile` | — | — | — | Path route only, this one target; child ceiling = the hypervisor's reviewed row (same as the shell's `hv` edge) | CP1 pending |
