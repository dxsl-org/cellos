# Phase 13 census: init endpoints, init death, and hot-swap (commit `7df04c626`, read-only)

**Answer.** Init answers inbound IPC from any task through one wildcard receive loop. That loop applies two request parsers, Ocel activation and machine pin, then a death-record fallback. Init cannot be killed from userspace, but the kernel has no init-death policy. If init exits or faults, nothing restarts and the kernel does not react.

On hot-swap: no production service handles Snapshot or Restore. The supervisor sends those envelopes as ordinary posts, and the kernel attests them as coming from the supervisor. `serve` could route them with one added branch. But init's `restart = "always"` for config would respawn over the swapped-in instance, so init has to change too.

## A. Init (`cells/tools/init`, 3,765 source lines including 391 lines of tests)

### Modules

| Module | Role |
|---|---|
| `src/main.rs:74-79` | `cell_main`. It runs `boot::start_after_platform`; on error it prints and **returns**, which is a clean init exit with code 0 (`libs/ostd/src/startup.rs:36+`). |
| `src/main.rs:81-174` | `start_services`. Order: block drivers → VFS bootstrap (`:87-94`; on failure it returns, so init exits) → config load → ordering → **pin gate `:136-141`** → `launch_order` `:143` → **`OCEL_ACTIVATOR` registration `:148-154`** → `boot::spawn_test_fixtures` `:158` → autoload `:166` → `supervisor::run` `:173`. |
| `src/main.rs:178-188` | `own_tid`: looks itself up by the name "init"; falls back to tid 2. |
| `src/supervisor.rs:101-203` | `run` loop: pin handler first (`:112-116`), then activator (`:117`), then death handling with restart policy, storm budget (`:170-181`) and verdict gate (`:187`). Hypervisor exit relay `:236-244`. |
| `src/service_table.rs:19,43,112-150` | `Service` table, the fixed `bootstrap_vfs` row, and spawn plus register (init registers as the namespace owner). |
| `src/activation.rs` | Activation endpoint. Wildcard attested receive `:33-53`; handler `:133-184`; engine start `:298-329`; VFS ELF fallback `:346-386`; `deliver_frame` `:438-456`; `task_alive` `:508-531`. |
| `src/activation_state.rs:1-70` | Bounded lease state: `leases[16]`, `providers[2]`, token and generation counters. |
| `src/machine_pin.rs` (+ `store.rs`, `diff.rs`, `tests.rs`) | ADR-0029 gate `:159-196`, safe mode `:243-268`, endpoint `handle` `:272-297`, `confirm` `:299-346`. The VFS-backed store is `store.rs:86-151`. |
| `src/configuration.rs:8-9,13-40` | Reads `/etc/cellos/{services,autoload}.toml` and `wait_for_vfs` (client side only). |
| `src/boot.rs` | Platform wait `:130-233`, block drivers `:428-595`, and the fixtures `:3-128`, `:653-762`. |

### What init answers over IPC

Init receives with mask 0 (`activation.rs:33-53`), so any task that knows init's tid can reach these parsers. Two registry ids resolve to that tid.

| Endpoint | Parser | Clients | State kept |
|---|---|---|---|
| `OCEL_ACTIVATOR`, id 19 (`libs/api/src/abi/syscall.rs:405`) | `ocel_service_proto::Request::decode`, 32-byte frame, magic `OCLSREQ1` (`libs/ocel-service-proto/src/lib.rs:9,58`), called at `activation.rs:134` | `cells/apps/ocel/src/lease.rs:157,221,243` | `Activator`: 32 deferred messages, poisoned VFS tid, `State` (`activation.rs:71-88`) |
| `MACHINE_PIN`, id 20 (`syscall.rs:409`) | `MachinePinRequest::is_claimed` / `decode` (`libs/api/src/services/machine_pin.rs:168,184`), called at `machine_pin.rs:274-277` | `cells/tools/cellbox/src/cmd_machine.rs:126-161` | `Endpoint {gate, safe_mode, shell index, VfsPinStore}` (`machine_pin.rs:228-234`) |
| Death records (kernel `NotifyOnExit`) | `Message::reason` (`activation.rs:66-68`); only trusted for senders with no trailer and confirmed dead (`supervisor.rs:123-127`) | Kernel `exit_task` delivery (`kernel/src/sched/scheduler.rs` ~1905-1930) | `services: Vec<Service>` |
| Nested replies during engine spawn | `VfsResponse` decode (`activation.rs:352,373`); other messages are deferred | VFS | as above |

Outbound only (not served): the owner-exit notice to the supervisor (`supervisor.rs:236-244`). The supervisor accepts that notice only from **tid 1** (`cells/services/supervisor/src/main.rs:154`), while init falls back to tid 2 (`main.rs:187`). [INFERENCE] On x86 the platform cell is spawned before init, so init may not be tid 1 there.

### What moving each endpoint into its own cell needs

| Need | Activator cell | Machine-pin cell |
|---|---|---|
| Catalog row | New `/bin/activator` row with boot ceiling `{spawn}` in `kernel/src/catalog.rs` (no `catalog/cells.toml` exists; it was planned in Phase 05 but never created). Engine rows already exist at `catalog.rs:444-446`. | New `/bin/machine-pin` row; it needs facts plus a VFS store. x86 only (`machine_pin.rs:160`). |
| Launch edges | Move `/bin/ocel-js`, `/bin/ocel-quickjs`, `/bin/ocel-pdf` from `init_profile` (`loader/launch_profile/profiles.rs:30-36`) to a new `"activator"` profile in `launch_profile/mod.rs:64-110`. | Init → `/bin/machine-pin` edge (`profiles.rs:22-27`). |
| Registering service ids | `OCEL_JS` (16) and `OCEL_PDF` (18) are below `PRIVATE_ID_BASE` (0x100). A spawner-only registrar is refused for those (`kernel/src/cell/service_registry.rs:121`), and well-known ids need trusted init or `SupervisorCap` (`syscall/authority.rs:627`). This needs a narrow kernel rule. Init can register `OCEL_ACTIVATOR` itself. | `MACHINE_PIN` is accepted only from trusted init naming itself (`authority.rs:564-567`). This must become "trusted init registering its catalog-named child". |
| Other authority | `NotifyOnExit` and `ForceExit` are `SpawnCap`-gated (`syscall/spawn.rs:622`); `GetProcs`. The ELF-from-VFS fallback only names the child for trusted init (`spawn.rs:96-101`), so keep the Path route (`activation.rs:299`). | `QueryMachineFacts` (433) requires `SpawnCap` (`syscall/dev.rs:468-471`); this needs a narrower gate. VFS store access is keyed on the live `MACHINE_PIN` provider (`cells/services/vfs/src/access/machine_pin.rs:41-48`), and the mutate-flag exception (`vfs/src/access.rs:99-103`, `vfs/src/dispatch.rs:56-60`) is justified as "init is permit-all". |
| Boot order | After the VFS, before autoload: today registration sits at `main.rs:148`. Init must launch it as a fixed bootstrap row because `boot-config` reserves id 19 (`libs/boot-config/src/lib.rs:303`). | After block drivers and VFS, **before** any configured service, because the gate decides whether those services start (`main.rs:136-141`). So it is also a fixed row, not `config/services.toml`. |
| Trust root | n/a | ADR-0029 amendment (`docs/decisions/0029-…md:72-75`): "the kernel accepts [the provider] only from trusted init naming itself". Moving the endpoint changes this and needs an ADR amendment. Confirm currently requires "the shell init started" (`machine_pin.rs:305-312`), so init must hand the cell the shell's identity, or the rule must become "attested `cellbox` program". |

### Fixture and test-profile code compiled into init

| Code | Feature | Enabled by (CI job or tier) |
|---|---|---|
| `boot.rs:676-690` tier2-smoke, tier2-exploit, cpp-smoke | `tier2-entry` | `scripts/build-aarch64-test-hooks-ci.sh:121` (manual, `scripts/lanes.toml:14`); `scripts/build-x86_64-domain-test-ci.sh:54` (CI job `x86-c2c`, `.github/workflows/ci.yml:1349`) |
| `boot.rs:698-720` tier2-rpc provider and driver, registers 0x7A01 | `tier2-rpc-entry` | `build-x86_64-domain-test-ci.sh:54` (`x86-c2c`) |
| `boot.rs:724-745` edu-holder and export-client, registers 0x7AE0 | `edu-export-entry` | `scripts/x86/qemu-edu-export-test.sh:61` (manual, `lanes.toml:377`) |
| `boot.rs:21-128,750` grant pair at `/bin/silo` and `/bin/net-broker` | `tier2-grant-pair` | `build-aarch64-test-hooks-ci.sh:121` (manual) |
| `boot.rs:753-761` | **No feature gate** (non-rpi3): silo-test, ai-test, vfs-test, srv-test, std-smoke | Every non-rpi3 image; it relies on those binaries being absent from the image |
| Cargo `test-hooks` | **Dead**: no `cfg` use in `src`, and no script enables it | — |
| `input`, `ui`, `ai`, `supervisor`, `usb-host`, `tier3-autostart`, `hv-autostart` | No init code; only read by `scripts/generate-boot-config.py:45-71` | `make-hypervisor-fs-x86.sh:37` (`qemu-x86-hypervisor-boot`, `ci.yml:1425`); `make-hypervisor-fs.sh:74` (`qemu-hypervisor-machinery`, `ci.yml:830`); `make-hypervisor-fs-rpi3.sh:150-166` (manual) |
| `tier3`, `hostile-backend-recovery`, `development-silo-provider`, `c2c-broker` | Compile-error rules only (`main.rs:12-19`, `boot.rs:21-26`, `build.rs`) | `make-hypervisor-fs-x86.sh:49`; `build-aarch64-test-hooks-ci.sh:105`; `run-c2c-broker-oracle-qemu.sh:104` (`ci.yml:595`) |
| `board-rpi3` | `boot.rs:430,521,753` | `build-aarch64-cells.ps1:214`, rpi3 scripts (manual) |

`Cargo.toml` has **17 features** plus `default` (the handoff says 21). The fixture targets are still production catalog rows (`catalog.rs:451-458,468-471`) and init launch edges (`profiles.rs:25-27,37-49`).

## B. Kernel: trusted init and what happens when it dies

| Fact | Location |
|---|---|
| Latch is set once (first publication wins); never cleared; tids are never reused | `kernel/src/task/syscall/mod.rs:464-468`, `spawn.rs:59-67`; published from `task/launch.rs:296-300`; set via `loader/governed_spawn.rs:294-345` (`trusted_init = true` at `:342`); called from `main.rs:1659` |
| What the latch keys | Launch identity "init" (`spawn.rs:297-300`), child naming (`:101`), registration flags (`authority.rs:551`), `MACHINE_PIN` (`:564`), namespace registrar (`:627`) |
| Init is marked `is_critical` | Argument at `governed_spawn.rs:322`; `KillCell` refuses it (`authority.rs:956`); freeze refuses it (`kernel/src/cell/hotswap.rs:245`); `ForceExit` refuses it because it holds block_io and network (`spawn.rs:629-633`). There is **no userspace path to kill init**. |
| Death path today | The generic `exit_task` (`sched/scheduler.rs:1648`). Its only per-task special case is the aarch64 test-hooks `vfs-test` exit (`:1698-1705`). Init's registry rows are cleared (`:1845`). Children keep running: there is no parent tree, and nothing in the kernel sends `SHUTDOWN_PARENT_DIED` (`app_envelope.rs:28` is only decoded, at `ostd/src/app.rs:474`). No panic, no reset. |
| How init can die | Fault (`scheduler.rs:1487-1510`); clean return from `cell_main` (`main.rs:76-79`, `:88-92`); watchdogs, which apply only if armed. |
| Existing records (none survive a reset) | Audit ring with `CellFault`/`CellExit` (`kernel/src/audit.rs:1-25,231`; emitted at `scheduler.rs:1497,1556`, `spawn.rs:661`, `hotswap.rs:499`); retirement verdict table of 64 records (`kernel/src/cell/retirement_verdict.rs:1-23`); `[fault] Cell …` log line (`scheduler.rs:1487`). |
| x86 reset | `acpi::power::reboot()`: FADT reset register → port 0xCF9 → 8042 pulse → triple fault, always diverges (`kernel/src/acpi/power.rs:702-742`); used by syscall 436 (`dev.rs:199-211`); tested by `power-x86` (`tests/integration/tests/power-x86.rs:112-134`, `pr` tier). |
| RISC-V reset | `sbi::system_reset(SBI_RESET_COLD_REBOOT)` (`hal/arch/riscv/src/common/sbi.rs:152-172`); used by the panic handler (`main.rs:1941`). |
| aarch64 reset | **None.** `hal/arch/arm/src/aarch64/psci.rs` has only `version`/`cpu_on` (`:101,108`). Only SYSTEM_OFF exists, hard-coded via `hvc` (`dev.rs:862-870`). On x86 and aarch64 a kernel panic halts (`main.rs:1942-1953`). |

## C. Hot-swap

| Item | Finding |
|---|---|
| Supervisor flow (`cells/services/supervisor/src/hotswap.rs:115-190`) | Pause service `:125` → Snapshot post `:128` → wait for the stash key; a timeout here is tolerated (`:135-136`) → freeze `:145` → `sys_spawn_replacement` `:152` → Restore post `:165` → wait for `sys_query_hotswap_ready` `:169` → `sys_commit_hotswap` `:177` → `KillCell` `:184`. Rollback is at `:192-197`. |
| Envelopes | Built with `[0xAC, 0xF0, swap_id LE]` and `[0xAC, 0xF1, 64-byte decimal key]` and sent with `ostd::ipc::post` (`supervisor/src/transfer.rs:9-25`; constants at `libs/api/src/services/proto/app_envelope.rs:6-23`). They come **from the supervisor, not the kernel**. Attested receives carry the sender's trailer (`authority.rs:158-178`). `AppContext` accepts both kinds from **any** sender (`ostd/src/app.rs:411-437`). |
| Target map | vfs, net, compositor, input, hotswap-demo (`supervisor/src/main.rs:201-209`). None of the four production services handles 0xF0/0xF1 (grep of `cells/services` finds only the demos). |
| Who may request a swap | Only the signed program `hotswap` (`main.rs:48,98`), i.e. `cells/tools/sys-tools/src/bin/hotswap.rs`. Bench `native_stateful.rs:378-396` sends `OP_HOTSWAP` directly, so [INFERENCE] it is refused; that lane is manual. |
| `ostd::hotswap::ViStateTransfer` (`libs/ostd/src/hotswap.rs:201`, `stash_transfer`/`restore_transfer` `:246,268`) | Implemented by `cells/demos/hotswap-demo-v1/src/main.rs:52` and `hotswap-demo-v2/src/main.rs:46`, which are the only callers of `sys_hotswap_ready`. |
| `api::hotswap::ViStateTransfer` (`libs/api/src/services/hotswap.rs:31`; doc describes a kernel-driven protocol and cites a missing `docs/architecture/03-runtime-model.md`) | Implementations **are never called**: config `cells/services/config/src/main.rs:147-219`; vfs `cells/services/vfs/src/manager/state_transfer.rs:15` (mod at `vfs/src/manager.rs:16`); cellbox `cells/tools/cellbox/src/state_transfer.rs:43,126` (mod at `main.rs:123`, plus `history.rs:2-4` dead-code allowances); macro `libs/api/src/services/serde_helpers.rs:161`; `libs/api/tests/contract_test.rs:9,30`. |
| Config today | Runs `serve` (`config/src/main.rs:114,124-136`). `on_request` decodes everything as `ConfigRequest`, so a Snapshot would get a Malformed reply posted back into the supervisor's mailbox. Its allowlist lacks `HotSwapReady` (bit 32) (`:19-29`); `StateStashClear` shares bit 34 with `StateRestore` (`libs/api/abi/syscalls.toml:1161-1209`). Restart policy is `restart = "always"` (`config/services.toml:56-60`). |
| Can `serve` route the envelopes? | Yes: attested non-kernel messages reach `on_request` (`libs/ostd/src/service/serve.rs:212-260`); sender 0 goes to `on_unattributed` (`:221-227`). The sender can be authenticated with `sender_is_live_provider(SUPERVISOR, sender)` (`ostd/src/service/mod.rs:267-276`). Stash reads are sealed to the replacement (`kernel/src/cell/state_stash.rs:8-15`; `spawn.rs:1314-1320`). |
| Init does not know about swaps | On the old tid's death (KillCell always delivers reason 0xAAAA_AAAA, `hotswap.rs:493-503`), init **restarts per policy** (`supervisor.rs:145-198`) and re-registers over the replacement (`service_table.rs:126-131`). The demo avoids this only by using `restart = "never"` (`config/autoload.toml:22-28`). |
| Replacement path | `supervisor_profile` requires a boot or reviewed-target row (`profiles.rs:370-376`). `/bin/config` is `boot_only` (`catalog.rs:464`). A `/bin/config-v2` would need a new row, packaging and signing. |
| QEMU tests that do a swap | `tests/integration/tests/hotswap-smoke.rs:86-165` (RV64, manual, `lanes.toml:525`); `native-workload.rs:48-88` with bench `native_stateful` (manual); bench `hotswap_supervisor.rs` and `hotswap_cli_probe.rs`. **None run in `ci.yml`.** |
| Spec claims to correct | Spec 03 §3 (`docs/specs/03-runtime.md:27-35`): "critical Cells must implement `StateTransfer`", kernel pause, "Symbol Re-linking" — all false. Spec 12 axis 5 (`docs/specs/12-reliability.md:89`): ~50%, cites `kernel/src/cell/hotswap.rs` as the protocol. Axis 3 row (`:87`): "hotswap + state-stash". Also the net-broker comment (`cells/services/net-broker/src/main.rs:33`). |

## Proposed designs

### 1. Moving the endpoints out of init

1. Add fixed init-launched rows after `bootstrap_vfs`, in this order: VFS → `/bin/machine-pin` → (gate) → `/bin/activator` → plan services.
2. Init stays the registrar as namespace owner and gains **no inbound parser**. It learns safe mode by calling the pin cell as a client with the existing `MachinePinRequest::Status` frame. It sends the console shell's tid one way to the pin cell after `enter_safe_mode`.
3. Kernel changes:
   - `authority.rs:564`: allow trusted init to register `MACHINE_PIN` for a live child whose signed launch identity is `machine-pin`.
   - `dev.rs:469`: gate 433 on that provider instead of `SpawnCap`.
   - A narrow registration rule letting the `activator` identity publish exactly ids 16 and 18 for children it spawned.
   - A new `"activator"` launch profile; remove the engine paths from `init_profile`.
4. Rewrite the VFS justification and amend ADR-0029 §3 for the trust root.
5. `supervisor::run` then keeps only death records; delete `Activator` and `Endpoint` from init.
6. Move the fixtures to a test cell with test-only catalog rows, and remove the unconditional fixture spawns at `boot.rs:753-761`.

### 2. Init-death policy

1. Hook the root retirement where `tid == TRUSTED_INIT_TID` inside `exit_task` (`scheduler.rs:1648`). Add a `pub(crate)` accessor next to `spawn.rs:65`.
2. Under the lock, write a fixed-size `InitDeath {tid, cell_id, generation, exit_reason, fault cause/pc when present, ticks}`, emit the existing `CellFault`/`CellExit` audit event, and print one stable `[init-death]` line. Set an atomic flag.
3. Reset after quiescence from housekeeping (`note_housekeeping`), not under `SCHEDULER`:
   - x86: `acpi::power::reboot`;
   - RISC-V: `sbi::system_reset(COLD)`;
   - aarch64: add PSCI `SYSTEM_RESET` (0x8400_0009) to `psci.rs`.
4. **Reboot-loop risk:** init exits with code 0 on its boot-stop paths (`main.rs:76-79,88-92`). Either give those an explicit boot-stop code that leads to halt with the record, or bound resets. This bears on owner question 3.
5. x86 fixture: nothing in userspace can kill init (critical flag plus the `ForceExit` gate). Use a test-hooks-only init feature that faults after reporting OS ready, and assert the second banner the way `power-x86.rs:112-134` does.

### 3. Config hot-swap through `serve` hooks

1. Add `Handler::on_snapshot(&mut self) -> Option<Box<[u8]>>` and `on_restore(&mut self, version, &[u8])`, both off by default. In `dispatch` (`serve.rs:230`), when `payload[0..2]` is `[0xAC, 0xF0|0xF1]` **and** the sender is the live `SUPERVISOR` provider:
   - Snapshot: `stash_transfer(hotswap_key(id))`;
   - Restore: parse the decimal key, run `restore_transfer`, apply, then `sys_hotswap_ready`.
   - **Never reply**: a post has no operation, so a reply would land in the supervisor's mailbox.
2. Config: implement `ostd::hotswap::ViStateTransfer` for `ConfigStore`, reusing the current encoding (`main.rs:145-219`) without its inner version field; add `HotSwapReady` to its syscall list.
3. Supervisor map: `"config" => service::CONFIG`. Drop vfs, net, compositor and input. Keep `hotswap-demo` as the test target, or drop it.
4. Init: on a death with reason 0xAAAA_AAAA where the registry now names a live, different tid, adopt it: set `tid`, call `notify_on_exit(new)`, mark ready, do not restart (`supervisor.rs:128-153`).
5. Swap target: same path `/bin/config` (already has a boot row), or add a `config-v2` row.
6. x86 image needs `ai` (selects config), `supervisor`, and `sys-tools/hotswap`. Fixture: `export K=V` → `hotswap config /bin/config` → `env` (builtins at `cellbox/src/executor.rs:1073,1076`; [INFERENCE] that `env` reads config).
7. Delete the api trait, the three unused impls, the macro, and `contract_test`; correct Spec 03 and Spec 12.

## Not found
- `catalog/cells.toml` and `catalog/fixtures.toml`: the catalog is still `kernel/src/catalog.rs`.
- Any persistent fault or panic record across reset (searched `pstore|crash_record|fault_record|boot_reason`).
- An aarch64 reset path.
- Orphan reparenting, or any kernel sender of `SHUTDOWN_PARENT_DIED`.
- Snapshot/Restore handlers in vfs, net, compositor or input.
- `tests/architecture-validation/` has no `Cargo.toml` and is not a workspace member, so its `ViStateTransfer` "PASS" documents are stale text and are not compiled.

Checks to run after edits: `power-x86` and `machine-pin-x86` (`pr` tier), `ocel-browser` and `ocel-quickjs` (`pr`, these exercise the activator), `x86-c2c`, plus a new config swap lane.