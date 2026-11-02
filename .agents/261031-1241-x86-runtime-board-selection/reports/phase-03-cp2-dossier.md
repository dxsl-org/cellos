# Phase 03 CP2 dossier — machine fingerprint pinning (Law-1 confirmation 2)

Status: **implementation complete for the x86 scope; awaiting CP2 approval. Not committed.** 2026-11-02.
Plan: [plan.md](../plan.md) · [phase 03](../phase-03-fingerprint-pinning.md) · [CP1](../phase-03-cp1.md) · [approval](../phase-03-cp1-approval.md) · [handoff](../phase-03-lock-handoff-draft.md). Evidence ceiling: QEMU 8.2.2 / TCG / q35 / SeaBIOS, one vCPU. No physical machine, UEFI, durability or tamper-resistance claim.

## 1. Exact Law-1 delta (all additive; no existing number, byte or meaning changed)

| Item | Where | Contract |
|---|---|---|
| `ViSyscall::QueryMachineFacts = 433` | `libs/api/src/abi/syscall.rs` (variant, `433 =>` decode, `allowlist_bit() → None`) | a0 = out_ptr, a1 = out_len (must equal 2336), a2 = a3 = 0 → `total_len`. Errors: `usize::MAX` no SpawnCap; `usize::MAX-3` bad out_len / a2 / a3 / unwritable buffer; `usize::MAX-2` no facts (non-x86, no profile bound, >256 PCI functions). |
| `ViMachineFactsV1`, `MachineFactsRecord`, `PciFunctionId`, `FactString`, constants | new `libs/api/src/abi/machine_facts.rs` (+ `machine_facts_tests.rs`), `mod` line in `abi.rs` | CP1 §4.1 layout byte for byte: 256-byte header, 8·n PCI entries, 32-byte SHA-256; `MACHINE_FACTS_CAPACITY = 2336`, `PCI_MAX = 256`, `FLAGS_KNOWN = 0x7F`. Decoder refuses every §4.1 violation; digest not verified (no hash in `api`). |
| `service::MACHINE_PIN = 20` | `libs/api/src/abi/syscall.rs` | "next free" doc now 21. |
| `api::services::machine_pin` | new `libs/api/src/services/machine_pin.rs`, `mod` line in `services.rs` | `MachinePinRequest::{Status, Confirm{digest}}` (48 B), `MachinePinResponse::{Status{state,current,pinned}, Recorded{digest}, Refused(Refusal)}` (80 B), `PinState` 1..=6, `Refusal` 1..=4 as CP1 §4.3. |
| `ostd::syscall::sys_query_machine_facts()` | `libs/ostd/src/syscall.rs` | returns a fully decoded `MachineFactsRecord` keeping the exact bytes. |

**Detail refinement inside the approved shape (needs CP2 OK):** the frame is a fixed-length magic + version frame (`MACHPINQ`/`MACHPINR`, the `ocel_service_proto` pattern) rather than a postcard `api::ipc` encoding. Reason: init receives a zero-padded 64-byte payload with no length, so postcard could not refuse trailing bytes, and the magic keeps the frame apart from exit records in init's loop. Variants, fields and codes are exactly CP1 §4.3.

## 2. Files

Law-1 / SDK: as §1. `libs/api/src/abi/syscall_tests.rs` (433 maps, no bit, 434 unmapped, MACHINE_PIN = 20).

Kernel: `kernel/src/machine_facts.rs` + `machine_facts_tests.rs` (new; encoder, SHA-256 via `crate::sha256`), `kernel/src/main.rs` (`mod` line; binding call passes `X86BootFacts`), `kernel/src/board.rs` (`X86BootFacts`, write-once `X86_MACHINE_FACTS`, `x86_machine_inputs()`, `bind_x86_profile(board, facts)`), `kernel/src/task/drivers/pcie_ecam.rs` (`fingerprint_functions()` re-reads vendor/device/class by BDF from config space; host test), `kernel/src/task/syscall.rs` (433 variant/map/always-permitted/typed errors/dispatch; `TRUSTED_INIT_TID`, `publish_trusted_init`, `RegisterService(MACHINE_PIN)` restriction; host tests), `kernel/src/loader/governed_spawn.rs` (`spawn_init` publishes init's tid), `scripts/mutable-globals.toml` + regenerated `docs/generated/mutable-globals.md`.

Init: `cells/tools/init/src/machine_pin.rs` (gate, policy, safe mode, confirm), `machine_pin/diff.rs`, `machine_pin/store.rs`, `machine_pin/tests.rs` (new); `main.rs` (gate between ordering and the first `launch_order`; `own_tid()`); `supervisor.rs` (`run(services, Option<&mut Endpoint>)`, pin frames routed before the activator); `activation.rs` (`PAYLOAD_BYTES` and `deliver_frame` made crate-visible).

VFS: `cells/services/vfs/src/access/machine_pin.rs` (new, tests), `access/rules.rs` (two rows), `access.rs` (decide branch, shared provider-namespace helpers, `is_machine_pin_writer`), `dispatch.rs` (narrow mutate exception, see §4), `docs/vfs-api.md`.

Cellbox: `cells/tools/cellbox/src/cmd_machine.rs` (new, tests), `executor.rs`, `main.rs`; amendments in §4.

Fixtures / lanes: `scripts/x86/machine_pin_disk.py` (new), `tests/integration/tests/machine-pin-x86.rs` (new), `tests/integration/src/lib.rs` (`boot_x86_bios_for_machine_pin`, `-nic none` when no NIC), `tests/integration/tests/iommu-dmar-x86.rs` (amendment, §4), `.github/workflows/ci.yml` (new "Machine pin lanes" step after the PC image build).

Docs: `docs/abi-boundary-records.md` (K row `ViMachineFactsV1`, P row pin file, C row frame), `docs/hardware-compatibility-list.md` (Profile / pin digest column + legend; R3 note), `boards/pc/x86_64-pc/README.md` (re-confirm procedure), `CHANGELOG.md`.

Image: `kernel/src/embedded-x86_64/{init,kernel_fs.img,manifest.json}` re-assembled by `python3 scripts/assemble-x86-image.py --profile headless --out-dir kernel/src/embedded-x86_64` (exit 0, 19 cells dev-key signed, `kernel_fs.img` sha256 `7cef95d3…94ee`). The assembler script is unchanged, so no recipe re-pin.

## 3. Evidence

**Host tests** (`--target x86_64-unknown-linux-gnu`):
- `cargo test -p api --lib`: 181 passed (7 `machine_facts` tests: offsets, round trip, min/full records, encoder refusals, 28 decoder refusal cases, UUID/flag rule, multiset; 4 `machine_pin` frame tests; 433 syscall test).
- `cargo test -p cellos-kernel`: 377 passed, 0 failed, no warnings. New: 5 encoder tests (all facts + SHA-256 of body, PCI permutation invariance of bytes and digest, each input incl. profile source changes the digest, absent facts clear flags, 257 functions → no facts), `fingerprint_reads_identity_and_class_from_config_space`, `query_machine_facts_is_gated_on_spawn_with_typed_errors` (no SpawnCap → `usize::MAX`; 5 bad arg shapes → `MAX-3`; unbound → `MAX-2`), `register_service_admits_machine_pin_only_from_trusted_init_itself`.
- `cargo test -p app-init`: 41 passed (13 machine-pin tests: decision table incl. absent+failed write, unreadable/unknown-version/oversized never written, unavailable never written, q35 vs PC store policy, no facts; diff + escaping; PCI cap and multiset; safe set from shipped `config/*.toml` → input + `cellbox sh`, withholds net/silo; confirm rule order; `only_a_complete_listing_without_the_entry_proves_absence`).
- `cargo test -p service-vfs`: 133 passed (machine-pin access tests incl. `mutate_exception_covers_only_the_live_provider_inside_the_namespace`; 7 warnings are pre-existing host-cfg dead code).
- `cargo test -p app-cellbox`: 231 passed after the catalog one-shot row was added (5 `cmd_machine` tests).

**`-D warnings` kernel builds** (RUSTFLAGS from `build-x86_64-prod-refusal-ci.sh` + `-D warnings`, `CARGO_TARGET_DIR=target/p03-dwarn`, unset afterwards): default exit 0, 0 warnings; `--features policy-required` exit 0, 0 warnings. Logs `build/p03-pin/dwarn*.log`.

**mutable-globals `--check`:** 603 declarations, 0 problems (after classifying `X86_MACHINE_FACTS` and `TRUSTED_INIT_TID` and `--write`).

**QEMU lanes** (`build/vicell-x86.iso` q35 and `build/x86-pc-lane/vicell-x86-pc.iso`, one kernel built with the manifest features `policy-required`): `machine-pin-x86` 5/5 passed (`build/p03-pin/machine-pin-x86.log`, 179 s). Serial lines (em dash shown; the capture maps bytes 1:1):

| Criterion | Evidence |
|---|---|
| First boot pins | PC + VT-d: `Init: machine pin: first boot — recorded a63f97eab14c`, `Init: net launched.`, `Init: OS ready.`; console `machine pin: state=first-boot-recorded current=a63f97ea…f6e85e pinned=a63f97ea…f6e85e`; `cat /srv/.cellos-machine/pin` → `Permission denied`, `ls /srv/.cellos-machine` → `cannot access` |
| Unchanged reboot matches | `Init: machine pin: match a63f97eab14c`, `net launched`, `OS ready`; `machine pin --confirm` → `machine pin: refused NotSafeMode`; `rm /srv/.cellos-machine/pin` → `rm: cannot remove`; `state=match` |
| Added e1000 → PCI diff, safe mode, confirm, next boot matches | `mismatch — pinned a63f97eab14c, current ad6d41736257`, `pci +8086:100e 02:00:00` (only line; no `pci -`, no `changed`), `SAFE MODE — machine pin mismatch; … run 'machine pin --confirm' then reboot.`, `SAFE MODE — required services withheld: net`; only `input` and `cellbox` launched; no `net launched`, no `e1000 launched`, no `OS ready`. Console `machine pin --confirm` → `machine pin: recorded ad6d4173…439f978f; reboot to start services`, init `re-confirmed ad6d41736257`. Next boot with e1000: `match ad6d41736257`, `net launched`, `OS ready` |
| Corrupt record → safe mode | seeded 64×`0x5a`: `record unreadable (UnsupportedVersion)`, `SAFE MODE — machine pin record unreadable`, no first-boot re-pin, no net |
| PC without an IOMMU → safe mode | `[iommu] DMA isolation REQUIRED by board x86_64-pc but no remapper was discovered — refusing untranslated DMA`, `store unavailable (/srv not mounted)`, `SAFE MODE — machine pin store unavailable`, shell present |
| q35 without a store warns and continues | `store unavailable (/srv not mounted)`, `machine pin disabled — no store; continuing under qemu-q35-x86_64.`, `net launched`, `OS ready` |
| VT-d field diff (q35) | pin with `intel-iommu` (`first boot — recorded e5faed15a56e`), boot without: `changed dmar.present: 1 -> 0`, `changed vtd.active: 1 -> 0`, `SAFE MODE`, no net |

**Final x86 run after the absence fix** (image re-assembled, exit 0; kernel and both ISOs rebuilt; logs `build/p03-pin/r2-*.log`): `machine-pin-x86` 5/5; `iommu-dmar-x86` 3/3 (PC VT-d lane pins its first boot on a P5 disk and still reaches DHCP; no-remapper lane asserts `SAFE MODE`); `igb-x86` 2/2; `serial-x86` 1/1; `nvme-x86` 11 passed (8 `*_09c_*` self-skip: `VICELL_AHCI_FAULT_ISO` unset); `ahci-x86` 15 passed (10 self-skip, same reason); `BOOT_WINDOW=90 qemu-x86_64-test.sh` → `PASS: x86_64 shell prompt reached — full boot successful`; `build-x86_64-domain-test-ci.sh` exit 0 and `X86_CPUS=2 qemu-domain-test.sh` → `PASS: x86-domain-test-cpu2 boot markers present`.

`xhci-x86` 3/4: `xhci_injected_key_decodes_in_cell_x86` fails (keys decode in the xHCI cell, `shell: command not found: q` never appears). **Pre-existing:** the same test fails identically on an ISO built from unmodified HEAD `4b39e87a6` in a scratch worktree (`build/p03-pin/base-xhci-key.log`). The q35 boot in that lane takes the warn-and-continue path (`machine pin disabled — no store`).

**aarch64:** `gen-disk-aarch64-ci.sh` exit 0; init changed, so `kernel/src/embedded-aarch64/init` and the gitignored `kernel_fs.img` were refreshed from `target/aarch64-prod-ci-embedded/`; plain `cargo build --release --target aarch64-unknown-none-softfloat -p cellos-kernel` (RUSTFLAGS pic) exit 0; `qemu-aarch64-test.sh` → `PASS: aarch64 shell prompt reached`; serial (`build/p03-pin/aarch64-serial.log`): `Init: machine pin: not applicable on this architecture.`, `Init: OS ready.`, `Cellos >`.

**RV64:** `gen-disk-ci.sh` exit 0. Boot reaches `Init: machine pin: not applicable on this architecture.`, `Init: OS ready.`, `Cellos >` (`build/p03-pin/rv64-launch-profile.log`). The `launch-profile` lane then fails at its first marker: it waits for `Init: service registry verified.`, but init prints `Init: service registry verified (init registration is publication, not application readiness).` That line is unchanged from HEAD, so the lane's marker is stale independent of phase 03. `tier2-fault-isolation` was not run.

## 4. Deviations and file-set amendments

1. **VFS mutate exception (approved by Main 2026-11-02, option b).** Init runs permit-all, so the kernel never attests `CALLER_FLAG_VFS_MUTATE` for it and the VFS central gate refused its `Mkdir`/`Write`. `dispatch.rs` (outside the set) now lets exactly `Write`/`Mkdir` inside canonical `/srv/.cellos-machine/**` through when the caller is the live `MACHINE_PIN` provider (`AccessTable::is_machine_pin_writer`); `can_write` uses the same predicate. Every other request kind, path and caller still needs the flag; remove-dir/tree stay refused even for the provider. Test: `mutate_exception_covers_only_the_live_provider_inside_the_namespace`.
2. **Cellbox registry (approved by Main):** `oneshot.rs` (TABLE row + counts), `ledger.rs` (row + count), `typed.rs` (BUILTINS), `.agents/261020-1750-cellbox-cli/command-catalog.md` (ledger row and one-shot row).
3. **`tests/integration/tests/iommu-dmar-x86.rs`** (not in the set; anticipated by CP1 §7 "PC-ISO lanes without P5 … must assert that"): PC VT-d lane uses a P5 disk; no-remapper lane asserts safe mode.
4. **`TRUSTED_INIT_TID` lives in `kernel/src/task/syscall.rs`**, not `governed_spawn.rs`: `governed_spawn` is a private module of `loader`, and re-exporting would have touched `loader.rs`. `spawn_init` publishes; the static and its reader sit next to the restriction.
5. **Init module split:** `machine_pin.rs` plus `machine_pin/{diff,store,tests}.rs` (Law 5 layout, file-size guideline).
6. **Frame encoding:** §1 refinement.
7. **Diff arrow is ASCII `->`** (CP1 showed `→`), so serial captures and greps stay ASCII.
8. **Store absence evidence (tightened per Main):** the VFS returns the same IO error for an absent path and a failed backend, so a failed `Stat` never proves absence. "Absent" needs `/srv` to stat as a directory and then a successful, complete `ListDir` of the parent (`/srv` for the directory, `/srv/.cellos-machine` for the file) without the entry; a listing ≥ 3800 bytes counts as possibly truncated. Any other failure is `StoreUnavailable` (no write; safe mode under PC). Conditional save (D03) is not used (it needs SHA-256 in init, i.e. a `Cargo.lock` edit).
9. **Test harness:** without a NIC the helper passes `-nic none`; otherwise QEMU's default q35 e1000e appears and the e1000 diff also shows `pci -8086:10d3` (seen in the first run).
10. **ADR-0029** consequences amendment written directly (section "Amendment 2026-11-02"), not through `/skill:hc-adr`. Plan row and phase 03 status text updated.

## 5. Open risks

- **Absence (residual).** Fixed for transient stat errors (§4.8). A store whose `/srv` listing is complete but wrong (backend bug) could still read as absent; not expected, not proven.
- **Init tid publication order.** `publish_trusted_init` runs right after `publish_prepared`; init registers `MACHINE_PIN` much later (after platform, block drivers, VFS, config), so the window is theoretical, not closed by construction.
- **No durability claim.** A torn pin write gives `RecordUnreadable` → safe mode; the operator re-confirms.
- **Trust.** Change detection only; `limine.conf`/kernel are unmeasured (HCL R3 note). A raw-disk writer can delete the pin → next boot is "first boot".
- **Pre-existing lane failures** seen here, not caused by phase 03: `xhci_injected_key_decodes_in_cell_x86` (fails on HEAD too) and RV64 `launch-profile` (stale marker text). `tier2-fault-isolation` not run.
- **Platform scan completeness** is not attested: a short PCI set gives a mismatch (fails closed).
- `own_tid()` keeps init's name-based lookup with fallback 2; the kernel restriction keys on the real tid, so a wrong lookup fails registration → `FactsUnavailable` → safe mode (fail closed).

## 6. Rollback

Revert the commit set. 433 and id 20 return to unmapped; the pin file is inert data; the VFS rule and mutate exception go with the revert.
