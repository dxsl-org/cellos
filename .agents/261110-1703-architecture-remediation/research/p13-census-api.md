# Census for Phase 13 api split: `libs/api` and `libs/types` at 7df04c626

The handoff's numbers are wrong in three places:

- **Crate count.** 154 tracked Cargo.tomls depend on `api`, not 50. Six more sit under `.agents/`.
- **Kernel use of wire.** The kernel uses much more than `VfsRequest::Rename`: 22 files pull in `fs` traits, `block`, `ipc` and three `proto` modules.
- **"abi has no deps".** This is blocked by serde derives and `types` use inside `src/abi`.

The posix shim builds into every bare-metal user of `api`, because it is gated only by target cfg (`services/posix.rs:13-22`). `ostd` also re-exports all of `api` (`libs/ostd/src/lib.rs:21` `pub use api::*`).

## 1. Module tree → destination

| Module (libs/api/src) | Lines | Dest | Cross-split edges (file:line) |
|---|---|---|---|
| `lib.rs` | 32 | facade | `pub use types::*` :19, `pub use abi::*` :28, `pub use services::*` :29, `ViSyscall`/`TaskPriority` :31-32, `#![feature(c_variadic)]` :15 (needed only by posix `stdio_fmt`) |
| `abi.rs` + `abi/**` (anon_memory, block_route, caller_identity, cap, cell_image, cell_owner, cell_session{,/frontend,/trust}, completion, dir_attestation, dir_handles, disk, dma_export, fast_ipc, hypervisor, machine_facts, manifest*, pcie_function, retirement, service_binding, syscall, task, telemetry, vm) | 8 626 | **abi** | serde derive `abi/cell_session.rs:168` (ObjectRefV1), `abi/dir_handles.rs:50` (ViDirHandle). The orphan rule stops these impls moving to `wire`, so `abi` needs `serde`. `types::ViError` at `abi/dma_export.rs:15`. `use crate::*` (types glob: PhysAddr/VAddr/ViResult) at `abi/vm.rs:13`. Doc link `crate::services::machine_pin` at `abi/syscall.rs:406`. `include!(OUT_DIR/syscall_table.rs)` at `abi/syscall.rs:5`. `alloc` at `abi/machine_facts.rs:25` |
| `services.rs` + `services/**` except posix (allocator, async_io, benchmark, block, cluster, config, dir_name, display{,/appearance,/icon}, driver, fs, hostile_backend_recovery, hotswap, input, ipc, machine_pin, mime{,/registry,/types}, net, net_reply, proto/{app_envelope,compositor,config,input,net,nic,supervisor,tls,vfs}, ring_channel, serde_helpers, session, ssh_completion, vfs_file_handles, vmm_session{,/events}) | 8 811 | **wire** | wire→abi: `display.rs:19` cap; `fs.rs:175-184` cap; `ipc.rs:18,122-292,553-555,1083-1101` cell_session/dir_handles/caller_identity; `net-reply.rs:17` service_binding; `session.rs:5,190-198` cell_session. `crate::services::` self-paths at `proto/compositor.rs:6`, `proto/input.rs:11-12`, `proto/net.rs:6`. `use crate::*` (types glob) in allocator, async_io:12, benchmark, block, fs:10, hotswap:13, net, serde_helpers:10 |
| `services/cluster.rs` | 225 | wire, but **ABI-like** | The `__ViCell_cluster` ELF section (`cluster.rs:221`) is parsed by the kernel loader (`kernel/src/loader/governed_spawn.rs:26`, raw bytes). Candidate for `abi` |
| `services/posix.rs` + `posix/**` (alloc, cxxabi, entropy, math, net, setjmp, stdio, stdio_fmt, strings, sysio, vfs_at) | 3 540 | **posix-shim** | posix→abi: `cxxabi.rs:11`, `entropy.rs:7`, `net.rs:15-16`, `sysio.rs:8,260-266`, `vfs_at.rs:22`. posix→wire: `net.rs` (`crate::ipc`, `crate::net_reply::admit_net_reply`), `sysio.rs` (`ipc::{VfsRequest,VfsResponse,encode,decode,IPC_BUF_SIZE}`). `libm` used only by `math.rs`. 140 `#[no_mangle]` symbols |
| `src/c/printf.{c,h}` | — | posix-shim (dead) | Not compiled (`build.rs:103-106`) |

**Macros.** All rely on flat crate-root paths, which exist only because of the glob re-exports today.

| Macro | Def | `$crate` paths | Note |
|---|---|---|---|
| `declare_syscalls!` / `__declare_syscall_entry!` | `abi/syscall.rs:210,226` | `$crate::syscall::{VFS_MUTATE_DECLARATION_MASK,ViSyscall}` | `abi` must keep `syscall` at its root |
| `declare_manifest!` | `abi/manifest_macro.rs:15` | `$crate::manifest::CellManifest`, `$crate::declare_manifest!` | `abi` root needs `manifest` |
| `declare_cluster!` | `services/cluster.rs:217` | `$crate::cluster::{ClusterSection,ClusterMode}` | Used at `cells/services/net-broker/src/main.rs:44` |
| `impl_state_transfer!` | `services/serde_helpers.rs:161` | none (unhygienic `ViStateTransfer`, `ViSerializable`, `ViResult`) | Caller must import |

`api::declare_manifest!` / `api::declare_syscalls!` call sites span 121 files.

## 2. Kernel `api::` use (92 files, no brace imports; `kernel/Cargo.toml:9` has no features)

| Dest | Paths (count) |
|---|---|
| abi | completion (85+), syscall::{service 75, ipc_status 44, service_flags, events, cap_mask, system_power, ViMemInfoV1, ProcessInfo/V2, virtio_probe, result_sentinel, grant_slice, ViFstatV1, ViSpawnArgs, IPC_STATUS_*}, cell_owner (62), caller_identity, cell_session, hypervisor, cap, manifest, machine_facts, cell_image, retirement, block_route, telemetry, service_binding, pcie_function, dma_export, disk, dir_handles, dir_attestation, anon_memory, `TaskPriority`, `ViSyscall`. Not used: fast_ipc, vm |
| **wire** | `fs::{ViFile,ViFileSystem,SeekFrom,OpenMode,Stat,BoxFuture,FileResult,FileHandle}`: cell/cap_registry.rs:11-67, cell/cspace.rs:644-687, fs.rs:8, fs/fat.rs:16,413-417, fsapi.rs:99,296, system_config.rs:8, task/tcb.rs:204, fstat_selftest/fixture.rs:4,109, metadata.rs:2, syscall/fs.rs:646,726 |
| wire | `block::ViBlockDevice`: dev/block.rs:2, dev/mmc.rs:10, dev/ramdisk.rs:1, fs/fat.rs:15 |
| wire | `ipc::IPC_BUF_SIZE`: ipc/ipc_wire.rs:17, async_ipc.rs:2279. `ipc::REACTOR_WAKE_OPCODE`: task.rs:2797,2816. `ipc::{VfsRequest::Rename,encode,decode,VfsResponse}`: syscall/fs.rs:543-572. postcard reaches the kernel only through `api` |
| wire | `services::proto::supervisor::{KERNEL_SENDER,POWER_BUTTON_REQUEST}` acpi/sci.rs:19; `proto::input::{EV_*,PRODUCER_FRAME_LEN}` dev/console_drv.rs:3, dev/ps2_keyboard.rs:6; `proto::app_envelope` task/gpu-scanout.rs:16, syscall/authority.rs:251, thread_cap_selftest.rs:23 |
| posix | none |
| types | CellId, ViError, VAddr, ViResult, PhysAddr, DirEntry, GrantPerm, FileType, SemVer (direct `types` dep `kernel/Cargo.toml:8`; no kms) |

The `fs`/`block` traits are also implemented or used by cells: vfs `backend_fat.rs`, `block_stream.rs`, `drivers/disk`, hypervisor `persistent_disk.rs`, `ostd/src/fs.rs`, and others. So they cannot simply move into the kernel.

## 3. Per-crate destinations

154 crates; 646 files in-crate, 670 repo-wide `.rs` files with `api::`.

| Needs | n | Crates (files) |
|---|---|---|
| abi+wire | 58 | kernel(92) compositor(56) vfs(52) cellbox(25) net-broker(22) kms(17) net(16) hypervisor(13) libs/terminal(11) input(10) init(10) recovery-demo(9) supervisor(8) net-tools(7) ocel(6) chrome(5) intel-gpu(5) sys-tools(5) c2c-render(4) files(4) dwc2-usb(4) vfs-test(4) kms-ssh-admin(4) desktop(3) terminal(3) e1000e(3) virtio-net(3) vmxnet3(3) xhci(3) httpd(3) silo(3) sshd(3) robot-demo(2) bcm57xx(2) e1000(2) i40e(2) igb(2) igc(2) ixgbe(2) r8169(2) config(2) phase04-terminal-driver(2); 1 each: fb-console, hypha/core, vm-runner, async-echo, tetris, tokio-smoke, viui-demo, input-test, phase03-filter, s2tw-probe, srv-test, tier2-rpc-driver, tier2-rpc-provider, top-load, libs/vimio, libs/vipolling |
| abi only | 54 | virtio-blk(4) libs/text-engine(4) libs/virtio-pci-transport(4) pvscsi(3) virtio-scsi(3) llm-gateway(2) ahci(2) nvme(2) mesa-anv(2) backend(2); 1 each: haily-agent, hypha/tools/{fs,spawn,sys}, adc/can/hello/https/periph/pwm/sensor/spi/std-smoke demos, bcm-display, display-ssd1306, imu-mpu6050, serial-x86, virtio-gpu, ai, budget-authority, ocel-js, placement-controller, platform, workload-router, ai-test, atomic-publication-probe, edu-holder, export-client, futex-test, gpio-test-rv, hypervisor-test, periph-test, phase02-session-{child,parent}, pipe-{peer,test}, silo-test, tier2-{exploit,grant-owner,grant-receiver,smoke}, tls-test, tools/wasm, libs/boot-config |
| wire only | 9 | libs/viui(9) hid(2) window-policy-probe(2) robot-dashboard, drivers/disk, drivers/gpu, test-isolation, libs/port-platform, tests/integration (1 each) |
| abi+posix-shim | 7 | ocel-pdf, ocel-quickjs (both `extern crate api`), c-math-smoke (`extern crate api`), c-pthread, c-spawn, c-spawn-child, cpp-smoke (cc builds) |
| abi+wire+posix-shim | 5 | doom, tetris-c, tetris-lua, runtimes/lua (`extern crate api` main.rs:7), posix-shim-test(3; the only explicit `api::posix` user, `fixture.rs:22,27`) |
| abi, mlibc (posix off) | 4 | cpp-hosted-smoke, drm-lite-x86, mlibc-posix-x86, mlibc-smoke (`features=["mlibc"]` + `extern crate api`) |
| abi+wire+types (via `api::` re-export) | 2 | bench(33), libs/ostd(46) |
| types only (via `api::`) | 2 | hotswap-demo-v2, libs/ai-sdk; hotswap-demo-v1 is abi+types |
| dependency never used in source | 12 | cfi-test, hello-cell, http-smoke, sdk-demo, gpio, gpio-bcm, gpio-sifive, serial, drivers/wasm, power, wx-test, fuzz |

Qualified-path use: `api::services::` appears in 150 files (proto 143, session 52, mime 32, vmm_session 21, cluster 8, ssh_completion 7, ipc 6, machine_pin 3, posix 1, hostile_backend_recovery 1). `api::abi::` appears in 0 files. Via the ostd glob there are only 2 wire uses (`ostd::services`); the rest are types.

## 4. Features

| Feature | Def | Read by | Enabled by |
|---|---|---|---|
| `posix` | `libs/api/Cargo.toml` `posix = []` | **nothing**: no `cfg(feature="posix")` in any `.rs` | nobody |
| `mlibc` | Cargo.toml | `services/posix.rs:27-63` (turns off every submodule and re-export) | cpp-hosted-smoke:15, drm-lite-x86:14, mlibc-posix-x86:14, mlibc-smoke:15 |
| `hostile-backend-recovery` | Cargo.toml | `services.rs:19` | bench:14 direct; forwarded by init:75, hypervisor:14, supervisor:19; switched on by `scripts/make-hypervisor-fs-x86.sh:49-50` and `scripts/build-native-workload-ci.sh:34` |

## 5. libs/types (6 353 lines)

| Module | Lines | Kind | Importers |
|---|---|---|---|
| `kms/**` | 5 130 (**80.7%**; 2 453 of it is `kms/tests`) | KMS codec | `types::kms` in services/kms (50 files), libs/ostd (5: `clients/kms.rs:12,23`, `relay.rs:1`, `relay_enroll.rs:1`, `ssh.rs:17`, `trust.rs:16`), net(2), net-broker(2), kms-ssh-admin(2), silo-test(1), guests/silo-guest(1). In libs/api only as comments (`ipc.rs:249,344`) |
| `silo{,/wire,/tests}` | 633 | Silo wire | kms(2), silo(5), silo-test(1) |
| `block_wire.rs` | 226 | block driver wire | ahci(4), nvme(4), virtio-blk(4), pvscsi(2), virtio-scsi(2), vfs(7) |
| `c2c.rs` | 98 | C2C values | net-broker(4), ostd(2), bench(1) |
| `lib.rs` | 266 | core (HalError, CellId, GrantId, GrantPerm, CellState, SemVer, PhysAddr, VAddr, ViError, ViResult, FileType, DirEntry) | everyone |

`types` has no dependencies and `unsafe_code = "forbid"` (`libs/types/Cargo.toml:7-10`).

## 6. Things keyed by crate name or path

| Item | Breaks on rename/move? | Where |
|---|---|---|
| `scripts/unsafe-allowlist.toml` | **No.** Keys are cell paths (`[[file]] path`) and cell crate names (`[[crate]] name`, :709-763). `libs/*` is outside F1 (`cellos_sign/__init__.py:31`, `policy.py:28-29`). Prose only at :85 | — |
| Signed artifacts | **No.** Names come from cell crates (`sign-cell.py:18-22`); libs are not artifacts. `catalog/cells.toml` does not exist at this commit | — |
| `clippy.toml` | **No.** Only `ostd::syscall::sys_send` (:9) | — |
| Law-1 digests | **Yes** | `check-lookupservicebound-law1-digests.sh:26-29,44`, plus `require_in "$ABI_MOD" "pub mod service_binding;"` against `libs/api/src/abi.rs` (:118). `check-multi-source-wait-law1-digests.sh:32-33,54`, plus **literal kernel text `api::completion::source::…` at :154,156,162,178,183,185**, which breaks when kernel imports become `abi::`. `check-ai-law1-digests.sh:22` |
| syscalls.toml / table | **Yes** | `libs/api/build.rs:14,109-111`; `kernel/build.rs:14,445-448`; `kernel/src/task/syscall/golden_tests.rs:116-125`; `gen-syscall-headers.py:22` (writes `libs/port-platform/include/cellos_syscall.h`, `libs/zig-syscall/src/syscall.zig`, mlibc `syscall.h`); `mutable-globals.toml:558`; comments in ci.yml:133,138 |
| Hash pins | **Yes** | `qemu-phase04-terminal-test.sh:319` (sha256 of `libs/api/src/abi/cell_session.rs`); `:281` regex `api::declare_(manifest\|syscalls)!` over sys-tools bins vs HEAD |
| Path-keyed checkers | **Yes** | `check-disk-layout-literals.py:23,100` and `disk-layout.json:2` (abi/disk.rs); `tools/test_cell_table.py:37-38,94-97`; `check-posix-ledger.py:56`; `test_check_posix_ledger.py:25`; `gen-posix-shim-contract.py:8`; `check-service-wire-literals.py:26-34` (also types kms/silo/block_wire); `app_tier_acceptance/public_api.py:12,14,125`; `manifest_abi_predesign/common.py:51-52`; `corpus.py:21-23`; `verify-cell-contract-p01.py:31`; `qemu-phase09-ssh-test.sh:187` |
| `-p api` / manifest path | **Yes** | `ci.yml:1998-1999`; `dev-setup.sh:151,189`; `build-cellos-sysroot.sh:63` |
| Lockfiles | **Yes**, because CI runs `cargo metadata --locked` (ci.yml ~146-149) | `Cargo.lock`, `tests/integration/Cargo.lock`, `fuzz/Cargo.lock` (untracked) |
| CI path filters | No | `docs.yml:6` uses `libs/**` |
| Licence by path | **Decision needed** | `LICENSE:19-20,59` (`libs/api/` is Apache-2.0); `libs/api/Cargo.toml` is Apache-2.0, but `build.rs:1` and `services/posix.rs:1` carry MPL-2.0 SPDX headers |
| Tracked evidence | Historical pins of `libs/api/*` paths | `build/exact-delta-package-manifest-initial.json:11564+`, `build/g2-j-cow/pre-implementation-manifest.json:11579+` |
| Non-workspace crates | Break if built | 6 `.agents/**/Cargo.toml` with `api` path deps |

## Facade strategy (one cycle)

- **`libs/abi`.** Moves `src/abi/**` with the modules flat at the root, so `$crate::syscall` and `$crate::manifest` keep resolving. Also moves `build.rs`, and `abi/{syscalls.toml,table.rs,syscalls.golden.json}` (update the ~10 path refs above in the same commit). Dependencies: `types` and `serde` (derive, no default features). Dropping serde instead means hand-written impls in abi or newtypes in wire; that is an owner decision.
- **`libs/wire`.** Moves `services/**` minus posix. Dependencies: abi, types, serde, postcard. Feature `hostile-backend-recovery`. Rewrite `crate::{cap,dir_handles,cell_session,caller_identity,service_binding}` to `abi::…`, `crate::services::X` to `crate::X`, and replace `use crate::*` with explicit `types` imports.
- **`libs/posix-shim`.** Moves `services/posix/**`, `#![feature(c_variadic)]` and `libm`. Dependencies: abi, wire. Feature `mlibc`.
- **`api` facade.**
  - Root: `pub use types::*; pub use abi::*; pub use wire::*;`
  - `pub mod services { pub use wire::*; pub use posix_shim as posix; }`, for the 150 `api::services::` files.
  - Top level: `pub use posix_shim as posix;`
  - Features forward: `mlibc = ["posix-shim/mlibc"]`, `hostile-backend-recovery = ["wire/hostile-backend-recovery"]`.
  - Keep posix-shim on by default during the cycle. Today's C cells get the symbols through `ostd → api`, and only 8 of them say `extern crate api`.
- **Kernel.** Depend on `abi` directly. Its 22 wire files still need wire unless these are moved or reimplemented: `fs` and `block` traits (shared with vfs, disk and ostd), `IPC_BUF_SIZE`/`REACTOR_WAKE_OPCODE`, the Rename postcard encode, and the `proto` supervisor/input/app_envelope constants.

**Exact breaking spots**

1. abi serde derives: `cell_session.rs:168`, `dir_handles.rs:50`.
2. abi `types` use: `dma_export.rs:15`, `vm.rs:13`.
3. Doc link `syscall.rs:406`.
4. Kernel wire use: the 22 files listed in §2.
5. multi-source-wait digest literals: :154-185.
6. lookupservicebound `ABI_MOD` path: :29 and :118.
7. The syscalls.toml/table path set in §6.
8. The phase04 sha256 pin: :319.
9. `-p api` in ci.yml:1999 and dev-setup.sh:151.
10. Lockfiles.
11. The checker paths in §6.
12. The `posix` feature, which nothing reads.
13. ostd `pub use api::*` (`lib.rs:21`), which keeps the whole facade reachable as `ostd::*`.