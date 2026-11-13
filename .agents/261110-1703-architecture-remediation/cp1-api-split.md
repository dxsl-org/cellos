# CP1 — init reduction, hot-swap, `api` split (Phase 13)

**Date:** 2026-11-13 · **Base:** `7df04c626` · **Handoff:** [request](s6b-handoff-request.md), [approval](s6b-handoff-approval.md) · **Census:** [api](research/p13-census-api.md), [init and hot-swap](research/p13-census-init.md)

## 1. Census: what changes the plan

1. **`api` is bigger and more tangled than the plan assumed.**
   - 154 crates depend on it (not 50).
   - `ostd` re-exports all of it (`pub use api::*`).
   - The posix shim (140 `#[no_mangle]` symbols) links into every bare-metal user, because only target cfg gates it; the `posix` feature is read by nothing.
2. **The kernel needs more of the service wire than `Rename`** (22 files):
   - the `fs` and `block` traits (`ViFileSystem`, `ViBlockDevice`, shared with VFS, `drivers/disk`, hypervisor and `ostd`);
   - `IPC_BUF_SIZE` and `REACTOR_WAKE_OPCODE`;
   - the frames it emits itself: the power-button request, input producer frames and App SDK envelopes.
3. **`abi` cannot have zero dependencies.** Two ABI records derive serde (`ObjectRefV1`, `ViDirHandle`) and are embedded in postcard enums, and three modules use `types`.
4. **Init answers IPC from any task**: Ocel activation (`OCLSREQ1` frames) and machine-pin requests, through one wildcard receive.
   - Moving them out needs kernel rule changes, because three things are keyed to "trusted init naming itself": `MACHINE_PIN` registration, the `QueryMachineFacts` gate, and the registration of well-known ids 16/18.
   - It also needs an ADR-0029 amendment (the pin's trust root).
   - Five test fixtures spawn in every non-rpi3 image with no feature gate.
5. **When init dies, nothing happens.** No reset, no record, no restarter. Init also exits with code 0 on its own boot-stop paths, so a naive "init died → reset" policy would reboot-loop a machine whose VFS never comes up.
6. **No production service handles hot-swap.**
   - Config has an unused implementation of the *other* `ViStateTransfer` trait.
   - Init would restart config over a swapped-in replacement, because init does not know about swaps.
   - None of the swap lanes runs in CI.
   - aarch64 has no reset path (PSCI `SYSTEM_RESET` is missing).

## 2. Proposed Law-1 deltas

No syscall number, allowlist bit or frame byte moves.

| # | Item | Delta | Compatibility |
|---|---|---|---|
| A1 | Crate split | `libs/abi` takes `src/abi/**`, `build.rs` and `abi/{syscalls.toml,table.rs,syscalls.golden.json}`. Modules sit at the crate root, so `$crate::syscall`/`$crate::manifest` in `declare_syscalls!`/`declare_manifest!` keep resolving. Dependencies: `types` and `serde` (derive, no default features). `libs/wire` takes `services/**` minus posix (dependencies: abi, types, serde, postcard; feature `hostile-backend-recovery`). `libs/posix-shim` takes `services/posix/**`, `c_variadic` and `libm` (feature `mlibc`). | `api` becomes a facade for one cycle (`pub use types::*; pub use abi::*; pub use wire::*; pub mod services { pub use wire::*; pub use posix_shim as posix; }`), so every import path keeps compiling. Then imports migrate and `api` is deleted (step 6). |
| A2 | Kernel-facing contracts move to `abi` | The kernel's ABI owns what the kernel itself speaks:<br>• the `fs`/`block` traits (`ViFile`, `ViFileSystem`, `SeekFrom`, `OpenMode`, `Stat`, `FileHandle`, `ViBlockDevice`, …);<br>• `IPC_BUF_SIZE`;<br>• `REACTOR_WAKE_OPCODE`;<br>• the kernel-emitted frames `proto::app_envelope`, `proto::supervisor::{POWER_BUTTON_REQUEST, KERNEL_SENDER}`, `proto::input` producer kinds.<br>`wire` re-exports them at their current paths. | Paths keep working through `wire`/`api`. The Phase 12 literal gate's owner list follows. |
| A3 | Kernel `Rename` | The kernel stops linking postcard and `wire`. `kernel/src/task/syscall/fs.rs` encodes `VfsRequest::Rename` and decodes `VfsResponse::{Ok, Err}` by hand. A kernel test (dev-dependency on `wire`) pins those bytes to postcard's encoding. | Same bytes on the wire, pinned. `cargo tree -p cellos-kernel` then shows no `wire`, `posix-shim` or `kms-codec`. |
| A4 | `types` | KMS codecs (5 130 lines) move to `libs/kms-codec`. `types` keeps the core types, `silo`, `block_wire` and `c2c`. The `types::kms` path stays for one cycle (`pub use kms_codec as kms`). | Paths keep working, then migrate. |
| A5 | Init-death record | New kernel record `[init-death] tid=… cell=… gen=… reason=… pc=…` (one line plus audit `CellFault`/`CellExit`). Not stored across the reset (no persistent store exists; out of scope). | New log contract only. |
| A6 | Hot-swap | `ostd::service::Handler` gains `on_snapshot(&mut self) -> Option<Vec<u8>>` and `on_restore(&mut self, &[u8]) -> bool`, off by default. `serve` routes `[0xAC, 0xF0/0xF1]` only from the live `SUPERVISOR` provider, never replies to them, and calls `sys_hotswap_ready` after a successful restore. `api::services::hotswap::ViStateTransfer` and `impl_state_transfer!` are deleted; `ostd::hotswap::ViStateTransfer` is the only trait. | The envelopes are unchanged. The supervisor's target map becomes `config` (plus `hotswap-demo` for the demo lane). |

## 3. Non-Law-1 design

### Endpoints out of init (step 3)

- **New cells:**
  - `cells/services/machine-pin` (x86) carries ADR-0029's gate, safe mode and confirm logic, and the VFS store, moved from `init/src/machine_pin*`.
  - `cells/services/activator` carries the Ocel engine activation, moved from `init/src/activation*`.
- **Init** launches both as fixed bootstrap rows after VFS: `machine-pin` first, then the gate check, then `activator`.
  - Init keeps the registrar role and no inbound parser.
  - It asks the pin cell for safe mode as a client (`MachinePinRequest::Status`).
  - After safe mode it sends the console shell's tid to the pin cell one way.
- **Kernel rules (catalog and authority; no ABI number):**
  - Trusted init may register `MACHINE_PIN` for its live child whose signed launch identity is `machine-pin`.
  - `QueryMachineFacts` (433) is gated on the live `MACHINE_PIN` provider instead of `SpawnCap`.
  - The `activator` identity may publish exactly ids 16 and 18 for children it spawned.
  - A new `activator` launch profile takes the Ocel engine edges from `init_profile`.
- **ADR-0029 amendment:** the trust root is "the provider trusted init launched under identity `machine-pin`", not "init itself".
- **VFS:** the pin store and mutate-flag rules name the pin cell, not init.
- **Fixtures** (`tier2-*`, `edu-export`, grant pair, and the five ungated test spawns) move to a `cells/tests/fixture-launcher` cell. It has test-only catalog rows and is launched only by the lanes that use it. Init loses `tier2-entry`, `tier2-rpc-entry`, `edu-export-entry`, `tier2-grant-pair` and the dead `test-hooks` feature.

### Init-death policy (step 4)

1. In `exit_task`, when `tid == TRUSTED_INIT_TID`: write the A5 record under the lock, set a flag, and reset from housekeeping after the lock is released.
   - x86: `acpi::power::reboot`.
   - RISC-V: SBI cold reset.
   - aarch64: new PSCI `SYSTEM_RESET`.
2. **No reboot loop:** init's boot-stop paths exit with a new reserved code (`INIT_BOOT_STOP`). For that code the kernel records and **halts**. Any other init death (fault, watchdog) records and resets.
3. **Fixture:** a test-hooks-only init feature faults after "OS ready". The x86 lane asserts the record and a second boot banner, as `power-x86` does.

### Config hot-swap (step 5)

- **Config:** implements `ostd::hotswap::ViStateTransfer` through `serve`'s A6 hooks, reusing its current encoding, and gains `HotSwapReady`.
- **Init adopts a swapped service:** when a supervised service dies with the swap reason (`0xAAAA_AAAA`) and the registry names a live, different provider, init watches the new provider instead of restarting.
- **Target:** the same `/bin/config` path (it already has a boot row), so no new catalog row or signing.
- **Supervisor map:** `config` and `hotswap-demo`.
- **Deleted:** the vfs, net, compositor and input entries; the dead impls (`vfs/manager/state_transfer.rs`, cellbox `state_transfer.rs`, config's `api` impl); `libs/api/tests/contract_test.rs`.
- **Fixture (x86):** `export K=V` → `hotswap config /bin/config` → the key is still there, and config's tid changed.
- **Specs:** Spec 03 §3 and Spec 12 axes 3 and 5 say exactly that.

### Import migration (step 6)

Mechanical, per directory, in its own commit:
- `api::` → `abi::`/`wire::`/`types::`/`kms_codec::`/`posix_shim::`;
- `ostd`'s `pub use api::*` becomes explicit re-exports;
- C cells that need the shim depend on `posix-shim` explicitly;
- `libs/api` and its lockfile entries are deleted.

Path-keyed tooling moves with the files in the same commit:
- the digest scripts, `gen-syscall-headers.py` and `kernel/build.rs`;
- `golden_tests`, `check-disk-layout-literals.py` and `check-posix-ledger.py`;
- the phase-04 sha256 pin and `-p api` in `ci.yml` and `dev-setup.sh`.

## 4. Proof obligations (CP2 evidence)

1. **Kernel dependencies:** `cargo tree -p cellos-kernel` shows no `wire`, `posix-shim`, `kms-codec` or `postcard`.
2. **Byte tests:** the kernel `Rename` encoding equals postcard's; every moved wire type keeps its layout (the existing proto, kms and tombstone pins move with their crates).
3. **Init death on x86:** killing init produces the `[init-death]` record and a second boot. A boot-stop exit records and halts.
4. **Config swap on x86:** a key written before the swap is read after it; config has a new tid; init did not restart it.
5. **Endpoints:** `machine-pin-x86`, `ocel-browser` and `ocel-quickjs` pass with the endpoints out of init. Init's binary has no `OCLSREQ1` or pin parser.
6. **Checkpoint:** the full `pr`-tier suite on three arches, `x86-c2c`, a release-posture boot and an independent review. The digest scripts are re-pinned, and each re-pin is listed.

## 5. Order

1. This CP1.
2. A1–A4: crate split behind the facade, kernel on `abi` only. The x86 and RV boot lanes after.
3. Endpoints out of init; fixtures to the launcher cell. Boot lane, `machine-pin-x86`, Ocel lanes.
4. Init-death policy and its fixture.
5. A6 hot-swap: config wired, map trimmed, dead impls removed, specs corrected.
6. Import migration; `libs/api` deleted.
7. CP2.

## 6. Risks and rollback

- **Init changes can brick boot.** Every init step runs the x86 and RV boot lanes before the next one. Rollback is per-commit revert.
- **A reset on init death** could hide a boot bug behind a reboot loop. Boot-stop halts instead of resetting, and the record names the reason.
- **Moving the pin endpoint changes a trust root** (ADR-0029). Until the amendment is confirmed, the kernel rule accepts only the init-launched `machine-pin` identity.
- **Step 6 touches about 150 crates.** It is mechanical behind the facade, and the facade can stay if the cycle runs long.
- **The other session works in the supervisor** (hostile kill). Step 5 is rebased just before it lands.

## 7. Questions to the owner

1. Confirm A1–A6 as the Law-1 delta of Phase 13.
2. **A2/A3:** move the kernel-facing contracts into `abi` and hand-encode `Rename`, so the kernel links neither `wire` nor postcard (recommended). The alternative is to let the kernel depend on `abi` + `wire`, which still keeps posix and kms out.
3. **Init death:** halt (not reset) on init's own boot-stop exit, to avoid reboot loops.
4. **ADR-0029 amendment:** the machine-pin trust root becomes "the provider trusted init launched as `machine-pin`".
