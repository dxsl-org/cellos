# Spec 23 — Native SDK Contract

> **Status**: Ratified 2026-08-21. This is the normative contract for the one
> Cellos Native SDK family. It classifies current
> evidence; it does not approve a runtime, API, ABI, or loader change.
>
> The SDK is shared source and named modules for Tier 1 and the future Tier 2.
> It is **not** a numbered SDK-tier system. Execution Tier, runtime profile,
> SDK module, stability, and availability are separate axes under
> [ADR 0003](../decisions/0003-application-tier-taxonomy.md).

---

## 1. Scope and non-claims

This contract covers public native developer surfaces: Foundation, runtime
profiles, service clients, UI/graphics, middleware/helpers, tooling, and
operations/observability. It applies to the Tier 1 Trusted SAS Cell now and
defines the constraints a future Tier 2 Native Domain Cell must meet.

This contract does **not**:

- implement `rust-std`, Tier 2, a native-domain transport, Manifest v3, or an
  observability facade;
- change the frozen ABI, public source API, Cargo package versions, manifest
  layout, loader, signing policy, or any runtime profile;
- make an unsigned Cell a Tier 2 Cell, make a Tier 1 profile a sandbox, or
  promise that an existing symbol is usable.

Manifest v2 remains the fixed 16-byte Rust record. Its compatibility field is
still named `tier`; it is a protection-class request, not this document's SDK
classification. Zig intentionally emits the legacy 8-byte v1 record and the
Rust loader's compatibility path upcasts it. No package version is silently
bumped by ratifying this contract: `api` remains 0.1.0 and `ostd`/`viui` remain
0.2.0 at ratification.

## 2. Vocabulary and evidence rule

### 2.1 Stability

Stability expresses the change promise for a public surface, not whether it can
run on a profile.

| Value | Contract |
|---|---|
| **FROZEN** | ABI/layout surface. Removal, rename, layout/discriminant change, or addition requires the ABI process, including 2× explicit confirmation. |
| **STABLE** | Supported source contract under §6. Compatible additive changes are allowed; breaking changes require a deprecation window and a major-compatible release plan. |
| **EXPERIMENTAL** | Publicly visible but may change or be withdrawn in a later 0.x release; callers must feature/profile-gate it. |
| **INTERNAL** | Repository implementation detail. It is not a supported SDK import path even if public for crate composition. |
| **PLANNED** | Design intent only. It makes no source, ABI, behavior, schedule, or compatibility promise. |

### 2.2 Availability

Availability is evaluated per matrix cell and may change only with evidence.

| Value | Contract |
|---|---|
| **USABLE** | The declared profile builds on a supported target, an in-tree test or runnable example exercises the path, and documented failure/capability behavior is present. |
| **PARTIAL** | Some evidence exists, but at least one required target, operation, integration chain, or negative boundary is missing. |
| **BLOCKED** | A required prerequisite is absent; callers cannot claim the capability. |
| **PLANNED** | No runnable implementation is claimed. |
| **UNSUPPORTED** | This profile/module combination is intentionally outside its contract; use a different profile or Tier. |
| **N/A** | The module has no meaningful surface for that profile. It is neither a failure nor a promise. |

A symbol, crate, feature flag, skeleton, compile check, CI configuration, or
documentation example **alone MUST NOT** promote a cell to **USABLE**.
Promotion requires every applicable witness class and the release gates below,
recorded as content-addressed evidence in the Phase 02 acceptance ledger:

| Witness class | Minimum record |
|---|---|
| Source | Commit/tree identifier, public path, and reviewed source anchor. |
| Compile | Exact compiler, target, `cfg`, rustflags, features, profile, command, and successful output/artifact digest. |
| Test/runtime | Named test or runnable example, target/board or emulator, expected result, and output/artifact digest. |
| Delivery | For every installable or published profile/module: manifest validation, package/image, signing and verification command/output. It may be N/A only for a non-deliverable development-only item, with that scope recorded explicitly. |

The current in-tree anchors in §9 are **source witnesses only** unless the
Phase 02 ledger binds them to the other classes. Therefore all current cells
that would otherwise be **USABLE** are **PARTIAL**. Downgrade is required when
any required witness no longer holds.

## 3. Public-surface denominator

The denominator for a Native SDK compatibility or availability claim is the
complete public surface that resolves under the pinned compiler window:

- the exact tuples in §3.1 bind compiler, target, `cfg`, rustflags, features,
  profile, and Tier; an unpinned or different tuple is outside the claim;
- target JSON/triple, `cfg`, Cargo feature set, and profile must be named;
- public modules, macros, types, traits, functions, and `pub use` re-exports
  reachable from `api`, `ostd`, `viui`, and supported bindings count;
- C, Zig, and Lua bindings count only where the respective binding, build path,
  manifest form, and runnable test/example are evidenced;
- in-tree examples, templates, build/package/sign/manifest validation tools,
  and generated `.vi` source count when documented as a developer path.

The `api::*` flat re-exports are part of this denominator. A reorganization MUST
preserve those paths or provide the §6 deprecation migration. Feature-gated
surfaces (for example `ostd::http`, `ostd::json`, and `viui/gles2`) are absent
when their feature is absent; they are not implied by another profile.

### 3.1 Ratification denominator tuples

`rust-toolchain.toml` pins `nightly-2026-05-01`. Its `targets` entry names
`riscv64gc-unknown-none-elf`, `aarch64-unknown-none`, and
`x86_64-unknown-none`; this is a component-install declaration, not a statement
that every production build uses those exact triples. The actual AArch64
Cellos build lane is `aarch64-unknown-none-softfloat`, installed and selected by
CI/build scripts. The tuple table is the complete ratification denominator;
each row is **PARTIAL** until Phase 02 records all witness classes in §2.2.

| Tuple | Exact binding | Current evidence status |
|---|---|---|---|
| RISC-V native | `nightly-2026-05-01`; `riscv64gc-unknown-none-elf`; `cfg(target_arch="riscv64")`; `-C relocation-model=pic`; profile/feature/package command recorded per Cell; Tier 1 `rust-no-std` | Source + CI configuration only: `rust-toolchain.toml:1-5`; `scripts/cargo-config-linux.toml:14-18,38-47`; `.github/workflows/ci.yml:27,63,97`. |
| AArch64 native | `nightly-2026-05-01`; `aarch64-unknown-none-softfloat`; `cfg(target_arch="aarch64")`; `-C relocation-model=pic -C target-feature=+bti,+paca,+pacg`; board feature named per command; Tier 1 `rust-no-std` | Source + CI configuration only: `scripts/cargo-config-linux.toml:32-36,49-61`; `.github/workflows/ci.yml:33,110-141`. The compiler component target `aarch64-unknown-none` is not a substitute for this build tuple. |
| x86_64 native | `nightly-2026-05-01`; `x86_64-unknown-none`; `cfg(target_arch="x86_64")`; Cell builds must explicitly use `-C relocation-model=pic` rather than the template's kernel static flags; package feature set named per command; Tier 1 `rust-no-std` | Source + CI configuration only: `scripts/cargo-config-linux.toml:63-85`; `.github/workflows/ci.yml:30,211-219`. |
| Rust SDK features | Same native tuple plus `api` `{default, posix, mlibc, posix+mlibc}`; `ostd` `{default, json, http, json+http}`; and `viui` `{default, gles2}`, exactly as selected | Source only: `libs/api/Cargo.toml:18-23`; `libs/ostd/Cargo.toml:21-33`; `libs/viui/Cargo.toml:18-22`. `api/posix` is an empty compatibility marker, not a capability gate; `mlibc` changes shim symbol exposure. |
| C `ffi-posix` | Native tuple is RISC-V or AArch64 only; `api::services::posix` is architecture-visible automatically on those arches; `mlibc` absent exposes shim symbols, while `mlibc` suppresses them and consuming Cells require `mlibc-shim`/libc.a; named C compiler/sysroot tuple | **PARTIAL** source/build-path witness only: `libs/api/src/services/posix.rs:13-53`; `cells/demos/tetris-c/build.rs:28-109`; `scripts/cargo-config-linux.toml:19-36`. `wasm32` is source-visible in the module cfg but outside this ratified native denominator; x86_64 shim is not advertised. |
| Zig `ffi-posix` | Zig ≥0.13; `riscv64-freestanding-none` or `aarch64-freestanding-none`; `build.zig`; legacy v1 manifest; mlibc smoke only on RISC-V/AArch64 with prebuilt `libc.a` | **PARTIAL** source/build-path witness only: `scripts/build-zig-cells.ps1:19-57`; `libs/zig-syscall/src/manifest.zig:1-32`; `cells/tests/zig-mlibc-smoke/build.zig:30-44`. The script does not bind signing. |
| Lua `lua` | Cargo `release` for the named native target, `cells/runtimes/lua/build.rs`, and an ELF-capable C compiler selected by `CC_*`/clang/cross-GCC | **PARTIAL** source/build-path witness only: `cells/runtimes/lua/build.rs:1-37,82-136,191-258`. Missing compiler yields a no-op stub only on x86_64/AArch64 discovery paths; the RISC-V compilation/link path may fail. |

The exact SDK feature lattice is `api` `{default, posix, mlibc, posix+mlibc}` ×
`ostd` `{default, json, http, json+http}` × `viui` `{default, gles2}`. The
default feature lists are empty. `api/posix` is an empty marker and does not
gate `api::services::posix`; `api/mlibc` is the symbol-suppression choice.
Only the remaining named feature combinations are opt-in module surfaces. For a
delivery claim, Cargo profile is `release`; the runtime profile is independently one of
`rust-no-std`, `rust-std`, `ffi-posix`, or `lua`. A host `cfg(test)` command is
a test witness, not a native delivery witness. Any additional package or board
feature is part of the tuple and must be recorded verbatim by Phase 02.

## 4. SDK layers and ownership

| Layer | Owner / contract boundary |
|---|---|
| Foundation | `api::abi`, manifest, entry/lifecycle, capability declaration, syscall and IPC/grant primitives. ABI owns binary compatibility; services may evolve separately. |
| Runtime profiles | Profile integration owns language/runtime startup and supported target set: `rust-no-std`, future `rust-std`, trusted `ffi-posix`, and trusted Rust-hosted `lua`. |
| Service clients | `ostd` owns typed IPC, VFS, net, input, and discovery facades; service owners own their wire operation semantics. |
| UI/graphics | `ostd` owns surface/input access; `viui` owns Elm and reactive-v2 APIs, `.vi` integration, and optional GPU surface. |
| Middleware/helpers | `ostd` owns `AppContext`, `CellRuntime`, RAII/typed capability and grant handles, and existing convenience helpers. |
| Tooling | Build/profile owners own target builds; `cell-build`, scripts, manifest macros/validation, signing, templates, and `.vi` compiler own their stated chain portions. |
| Operations/observability | Individual modules own logging, watchdog, timing/metrics, tracing, health, crash, and UI diagnostics until a unified facade is implemented. |

### 4.1 Exhaustive public-surface inventory

This appendix exhausts public **module and re-export paths**, not every method.
Every public item inside a listed module inherits that module's matrix row and
availability unless an exception is named below. A new public module or `pub
use` MUST be added here and classified before release. `INTERNAL` submodules are
not SDK imports even if Rust visibility supports crate composition.

| Crate/path group | Exhaustive public module/re-export inventory | Inherited classification / exceptions | Anchor |
|---|---|---|---|
| `api` root | `types::*`; `abi`; `services`; `abi::*`; `services::*`; `ViSyscall`; `TaskPriority` | Foundation row. `abi` is **FROZEN**; `services` and `types::*` are **STABLE** source contracts; all root re-exports preserve their current path. | `libs/api/src/lib.rs:19-31` |
| `types::*` items re-exported by `api` | `HalResult`, `Result`, `HalError`, `CellId`, `GrantId`, `GrantPerm`, `CellState`, `SemVer`, `PhysAddr`, `VAddr`, `ViResult`, `ViError`, `FileType`, `DirEntry`; paths `api::kms` and `api::silo` (originating from `types::kms` and `types::silo`) | Foundation row, **STABLE** source contract and **PARTIAL** availability, except `api::silo`, which is an **INTERNAL**, `test-hooks`-only development-provider wire vocabulary despite its Rust re-export visibility and is not an SDK import or hardware-capability contract. `api::kms` remains the service-wire model and sole application-facing signing boundary. A type becomes **FROZEN** only when a declared `api::abi` layout includes it. | `libs/api/src/lib.rs:19`; `libs/types/src/lib.rs:14-167`; `libs/types/src/kms.rs:13-24`; `libs/types/src/silo.rs:10-108` |
| `api::abi` | `caller_identity`, `cap`, `completion`, `dir_attestation`, `dir_handles`, `dir_handles_tests`, `disk`, `fast_ipc`, `hypervisor`, `manifest`, `manifest_flags`, `manifest_macro`, `manifest_parse`, `syscall`, `syscall_tests`, `task`, `vm` | Foundation row; **FROZEN**. `*_tests` are public code organization, not a separate runtime guarantee. | `libs/api/src/abi.rs:2-32` |
| `api::services` | `allocator`, `async_io`, `benchmark`, `block`, `cluster`, `config`, `dir_name`, `dir_name_tests`, `display`, `driver`, `fs`, `hotswap`, `input`, `ipc`, `net`, `posix`, `serde_helpers`, `vfs_file_handles` | Service-client row. Explicit exception: `dir_name_tests` is **INTERNAL** test organization; `posix` also follows the `ffi-posix` exception. | `libs/api/src/services.rs:2-25` |
| `ostd` root re-exports | `api::*`, `CapHandle`, `boxed`, `string`, `vec`, `Result`, `embedded_io`, `heapless`, `args`, `check_help`, `set_spawn_argv`, `MTIME_TICKS_PER_MS`, `run_app!` | Foundation/middleware row, **STABLE** source contract and **PARTIAL** availability. All nested `api::*` paths follow the `api` rows; `run_app!` follows lifecycle. | `libs/ostd/src/lib.rs:15-41,77-81,149-176` |
| `ostd` Foundation/lifecycle | `cap`, `entry`, `fast_ipc`, `grant`, `ipc`, `mmio`, `startup`, `sync`, `syscall`, `app`, `runtime`, `task` | Foundation/middleware rows, **STABLE** except future domain variants **PLANNED**; **PARTIAL**. `grant` and raw MMIO are Tier 1 SAS/capability-bound. | `libs/ostd/src/lib.rs:17-52,101-122,139-147` |
| `ostd` core helpers | `collections`, `heap`, `io`, `fs`, `repl`, `prelude`, `executor`, `font`, `font_atlas`, `input`, `dispatch` | Middleware/UI rows, **STABLE** source contract and **PARTIAL** availability. `heap` is **INTERNAL** pending implementation; prelude adds no separate promise. | `libs/ostd/src/lib.rs:29-69,86-90,103-113`; `libs/ostd/src/prelude.rs:1-12` |
| `ostd` service/security/operations helpers | `tls`, `display`, `service`, `system_info`, `hotswap`, `cluster`, `dma` | Service-client/operations rows, **EXPERIMENTAL** and **PARTIAL**. The former public/general `silo` helper is removed; Silo is now KMS-internal AArch64-QEMU `DEV_REFERENCE`, and production signing requires a separately selected, implemented, and qualified hardware provider through Phases 6–8. `dma` is Tier 1 driver support, and `tls`/`cluster`/`hotswap` retain service-specific behavior. | `libs/ostd/src/lib.rs:71-116` |
| `ostd` optional protocol modules | conditional `json`, conditional `http` | Service-client row, **EXPERIMENTAL** and **PARTIAL**; unavailable when the named feature is absent. | `libs/ostd/src/lib.rs:124-137`; `libs/ostd/Cargo.toml:21-33` |
| `ostd::clients` and prelude | `input`, `kms`, `net`, `vfs`, conditional `tls_stream`; `InputClient`, `KmsClient`, `NetClient`, `TcpStream`, conditional `TlsStream`, `VfsClient`; prelude re-exports `AppContext`, `AppEvent`, `ShutdownReason`, clients, collections, IO, `CellRuntime`, mutex, result, allocation/types/errors | Service-client and middleware rows. `TlsStream` requires `http` or `json`; prelude contains no additional stability promise. | `libs/ostd/src/clients.rs:26-49`; `libs/ostd/src/prelude.rs:1-12` |
| `viui` root | `animation`, `app_runner`, `canvas`, `dirty`, `elm`, `event`, `executor`, `font_context`, conditional `gles2_canvas`, `gpu_canvas`, `gpu_cmd`, `gpu_renderer`, `input_bridge`, `layout`, `navigation`, `node`, `node_widgets`, `overlay`, `prelude`, `render_ctx`, `renderer`, `response`, `signal`, `state_store`, `surface_renderer`, `theme`, `widget`, `widgets`, `window`; `CommandExecutor`, `CpuExecutor`, `GpuRenderer`, `vi_design`, `vstack!`, `hstack!` | UI/graphics row. Elm and reactive-v2 coexist as **EXPERIMENTAL**; `gles2_canvas` requires `gles2` and remains **PLANNED**. | `libs/viui/src/lib.rs:24-109` |
| `viui::widgets`, `node_widgets`, `navigation`, prelude | v1: `button`, `checkbox`, `column`, `image`, `label`, `row`, `scroll_area`, `space`, `text_edit` plus their re-exports; v2: `bar_chart`, `button`, `card`, `checkbox`, `column`, `dialog`, `divider`, `dropdown`, `flex_box`, `image`, `label`, `line_chart`, `list_view`, `progress_bar`, `row`, `scroll_area`, `slider`, `space`, `text_edit`, `toast`, `touch_area` plus listed re-exports; navigation `router`, `stack_nav`, `tab_nav` and re-exports; prelude’s canvas/Elm/event/layout/response/state/theme/widget re-exports | UI/graphics row, **EXPERIMENTAL**. This is the explicit exception to any inference that an Elm widget and its v2 peer have one canonical stable API. | `libs/viui/src/widgets.rs:1-21`; `libs/viui/src/node_widgets.rs:1-42`; `libs/viui/src/navigation.rs:1-19`; `libs/viui/src/prelude.rs:1-12` |

## 5. Capability matrix (seed for Phase 02 ledger)

Legend: `T1` is the current Tier 1 state. `T2` is the future Tier 2 rule/state,
not an implementation claim. Every cell is explicit. Every cited implementation
anchor in this matrix is a **source witness** only unless it is additionally
bound to compile, test/runtime, and delivery records in the Phase 02 ledger.

| ID / Module | `rust-no-std` | `rust-std` | `ffi-posix` | `lua` | T1 | Future T2 rule/state | Stability | Evidence | Blocker / limitation |
|---|---|---|---|---|---|---|---|---|---|
| C2-FDN / Foundation: ABI, manifest, lifecycle, capabilities, IPC/grants | **PARTIAL** | **PLANNED** | **PARTIAL** | **PARTIAL** | **PARTIAL** trusted SAS | **BLOCKED**: copied/domain-safe IPC and explicit grants only after Spec 22 | ABI **FROZEN**; lifecycle **STABLE** | Source witnesses: `libs/api/src/abi.rs:2-12`; `libs/api/src/abi/manifest.rs:1-33`; `libs/ostd/src/runtime.rs:3-11`; `libs/ostd/src/grant.rs:1-10` | Missing content-addressed compile/test/runtime/delivery ledger; current grants expose an SAS identity address. |
| C2-RNS / Runtime profile: `rust-no-std` | **PARTIAL** | **N/A** | **N/A** | **N/A** | **PARTIAL** | **BLOCKED** pending native-domain runtime evidence | **STABLE** | Source witnesses: `libs/ostd/src/lib.rs:8-15`; `rust-toolchain.toml:1-5`; `cells/demos/sdk-demo/src/main.rs:1-20` | Missing exact tuple compile/test/runtime/delivery ledger; supported only under §3.1. |
| C2-RST / Runtime profile: `rust-std` | **N/A** | **PLANNED** | **N/A** | **N/A** | **PLANNED** | **PLANNED** | **PLANNED** | Source/design anchors: `docs/decisions/0003-application-tier-taxonomy.md:102-106`; `libs/ostd/src/lib.rs:8-9` | G4 custom target and pure-Rust PAL do not exist; it is not `std` over mlibc. |
| C2-FFI / Runtime profile: `ffi-posix`, including Zig | **N/A** | **N/A** | **PARTIAL** | **N/A** | **PARTIAL**, trusted | **UNSUPPORTED** until a domain-safe FFI/runtime contract passes Spec 22 | **EXPERIMENTAL** | Source witnesses: `libs/api/src/services/posix.rs:1-53`; `cells/tests/zig-hello/src/main.zig:1-41`; `scripts/build-zig-cells.ps1:19-57` | Missing compile/test/runtime/delivery ledger; profile-specific and trusted; Zig uses v1 manifest (`libs/zig-syscall/src/manifest.zig:1-32`) and its builder does not compose a signing chain. |
| C2-LUA / Runtime profile: Lua | **N/A** | **N/A** | **N/A** | **PARTIAL** | **PARTIAL**, trusted Rust-hosted runtime | **UNSUPPORTED** until domain-safe native runtime design and evidence exist | **EXPERIMENTAL** | Source witnesses: `cells/runtimes/lua/src/main.rs:1-20`; `cells/runtimes/lua/build.rs:1-37`; `docs/scripting-guide.md:3-16` | Missing compile/test/runtime/delivery ledger; Lua is not containment; only x86_64/AArch64 missing-compiler discovery uses a no-op stub, while RISC-V compilation/link may fail. |
| C2-SVC / Service clients: IPC, VFS, net, discovery | **PARTIAL** | **PLANNED** | **PARTIAL** | **PARTIAL** | **PARTIAL** where declared service/capability exists | **BLOCKED** until copied IPC and explicit domain grants are implemented | **STABLE** | Source witnesses: `libs/ostd/src/clients.rs:3-29`; `libs/ostd/src/service.rs:15-42`; `libs/ostd/src/service.rs:95-128`; `libs/ostd/src/app.rs:136-230` | Missing content-addressed compile/test/runtime/delivery ledger; coverage remains service-specific. |
| C2-UI / UI/graphics: surface, input, ViUI, `.vi`, GLES2 | **PARTIAL** | **PLANNED** | **UNSUPPORTED** | **UNSUPPORTED** | **PARTIAL** | **BLOCKED** pending domain-safe surface/input/grant design | Elm **EXPERIMENTAL**; reactive-v2 **EXPERIMENTAL**; GLES2 **PLANNED** | Source witnesses: `libs/viui/src/lib.rs:1-30`; `libs/viui/src/lib.rs:37-76`; `tools/viui-build/src/lib.rs:1-53`; `libs/viui/src/gles2_canvas.rs:1-10` | Missing compile/test/runtime/delivery ledger; legacy Elm and reactive v2 coexist; GLES2 is a no-op skeleton. |
| C2-MID / Middleware/helpers: `AppContext`, runtime, RAII/typed handles | **PARTIAL** | **PLANNED** | **PARTIAL** | **PARTIAL** | **PARTIAL** | **BLOCKED** for identity-sensitive helpers; future variants must copy or use explicit domain grants | **STABLE** except domain variants **PLANNED** | Source witnesses: `libs/ostd/src/app.rs:130-230`; `libs/ostd/src/runtime.rs:143-190`; `libs/ostd/src/grant.rs:26-124` | Missing content-addressed compile/test/runtime/delivery ledger; `GrantHandle::from_raw` is unsafe and SAS-specific. |
| C2-TOL / Tooling: build/package/sign/manifest validate/templates/`.vi` compiler | **PARTIAL** | **PLANNED** | **PARTIAL** | **PARTIAL** | **PARTIAL** | **BLOCKED** until Tier 2 admission/package format is separately approved | **EXPERIMENTAL** | Source witnesses: `scripts/lib-sign-cells.sh:40-64`; `scripts/test-cell-signing.sh:49-117`; `tools/vi-compiler/src/main.rs:1-28`; `tools/viui-build/src/lib.rs:25-53` | Missing compile/test/runtime/delivery ledger; signing test uses a scratch dev path; no unified package pipeline. |
| C2-OBS / Operations/observability: logging, metrics/frame timing, tracing, health/watchdog, crash/UI diagnostics | **PARTIAL** | **PLANNED** | **PARTIAL** | **PARTIAL** | **PARTIAL** | **BLOCKED** until a domain-safe observability policy exists | **EXPERIMENTAL** | Source witnesses: `libs/ostd/src/runtime.rs:143-177`; `libs/ostd/src/dispatch.rs:88-107`; `cells/tools/init/src/main.rs:283-336` | Missing compile/test/runtime/delivery ledger; logging/watchdog exist but no unified facade. |

### 5.1 Deterministic Phase 02 handoff

Phase 02 creates one ledger record for each matrix row ID: `C2-FDN`, `C2-RNS`,
`C2-RST`, `C2-FFI`, `C2-LUA`, `C2-SVC`, `C2-UI`, `C2-MID`, `C2-TOL`, and
`C2-OBS`. It retains every matrix cell with source availability,
`required_for_c9`, and closure result. The result vocabulary is exactly
**PASS**, **BLOCKED**, or **PLANNED**; it MUST NOT replace source availability
or silently exempt a cell. No matrix row may be omitted or merged.

| Source availability in a profile/Tier cell | `required_for_c9` | Phase 02 closure result | Rule |
|---|---|---|---|
| **USABLE** | `true` | **PASS** only after all applicable source, compile, test/runtime, delivery, architecture, and Tier gates validate for the same tuple | Fail closed: an absent, stale, mismatched, or failed witness is **BLOCKED**. |
| **PARTIAL** | `true` | **BLOCKED** | Incomplete evidence or behavior cannot close. |
| **BLOCKED** | `true` | **BLOCKED** | The stated prerequisite remains an explicit blocker. |
| **UNSUPPORTED** | `false` | **BLOCKED** | Exact constraint retained; it can never be **PASS** and is excluded only from aggregate qualification. |
| **N/A** | `false` | **BLOCKED** | Exact non-applicability retained; it can never be **PASS** and is excluded only from aggregate qualification. |
| **PLANNED** | `true` | **PLANNED** | It becomes eligible only after implementation and the full gate set exist. |

Each row aggregates to **PASS** iff every `required_for_c9=true` cell is
**PASS**. A row with zero required cells is invalid and fails closed; it cannot
aggregate to **PASS**. A later decision to change a `false` applicability value
or an `UNSUPPORTED` constraint requires an amendment to this contract before
Phase 02 may change the ledger; it is never a silent exemption.

## 6. Compatibility, versions, and errors

1. **Frozen ABI.** `api::abi` is immutable under its existing 2× confirmation
   rule. Manifest v2 stays 16 bytes; no source-level SDK decision may repurpose
   reserved bytes or reinterpret its compatibility aliases.
2. **Source additions.** A new public SDK item is additive only when it does not
   alter existing behavior, ABI, feature resolution, or re-export paths. New
   service submodules follow the extensible-services rule and do not thereby
   change the frozen ABI.
3. **Breaking change.** Before removing or materially changing a **STABLE**
   source item, maintain the old path with a documented replacement for at
   least one supported 0.x release cycle, add a migration note and test, then
   remove only in a declared breaking release. `api::*` re-exports are old paths
   and MUST remain during that window.
4. **Experimental and planned.** **EXPERIMENTAL** may break in 0.x with a
   release note. **PLANNED** is no compatibility promise. **INTERNAL** may move
   without SDK compatibility treatment.
5. **Feature/profile binding.** Each release must state its supported compiler,
   target, Cargo features, runtime profile, and tier. A feature not selected,
   profile not qualified, or target not listed is **UNSUPPORTED**, not a fallback
   request.
6. **Failure contract.** Unsupported optional APIs fail at compile time when
   absent from a feature/profile; supported capability-dependent operations must
   return their established recoverable result/error or documented absence. They
   MUST NOT silently widen a capability, emulate Tier 2 in SAS, or fall back to
   raw cross-domain pointer transfer.

## 7. Security and tier behavior

Tier 1 is a Trusted SAS Cell. Rust LBI, admission policy, and the manifest
capability ceiling are load-bearing; `unsafe` FFI and Lua execution remain
trusted. `ffi-posix` and `lua` are runtime profiles, not sandbox labels.

No signature result, missing signature, Manifest protection-class byte, or
profile selection implies Tier 2. Tier 2 remains unavailable until the negative
evidence and implementation gates in [Spec 22](22-native-domain-cell-implementation-gate.md)
are met. A Tier 2 SDK claim then requires copied IPC by default and explicit,
revocable domain grants. Current SAS raw pointers, TIDs used as identity
authority, `GrantHandle::from_raw`, and identity-mapped grant addresses MUST
NOT cross that boundary as a portable SDK contract.

## 8. Conformance and release gates

For every profile/module proposed for **USABLE**, a release owner MUST retain
content-addressed evidence for all applicable gates below in the Phase 02
acceptance ledger. A missing gate makes the cell **PARTIAL** or **BLOCKED**,
never silently usable. Phase 02, not this source inventory, promotes a cell.

| Gate | Required witness |
|---|---|
| Build denominator | Pinned compiler, target, `cfg`, feature set, and public imports compile. |
| Functional witness | In-tree example or test exercises the advertised operation and its expected error/capability path. |
| Architecture witness | Required native architectures are separately recorded; one architecture does not imply another. |
| Source/API witness | Public/re-export snapshot or compile fixture detects removed/changed supported paths. |
| Delivery witness | For an installable/published profile/module, its declared build, manifest validation, package/image, signing, and verification chain succeeds; otherwise the record carries the explicit development-only N/A scope from §2.2. |
| Tier witness | Tier 1 requires its admission/capability evidence. Tier 2 additionally requires every applicable Spec 22 negative and containment gate. |

Phase 02 owns the authoritative
[`app-tier-acceptance-ledger.json`](../app-tier-acceptance-ledger.json) and its
review-only projection. This spec seeds the exact source rows; the ledger alone
records evidence cohorts and derives program qualification.

## 9. Evidence ledger and known gaps

| Claim | Current anchor |
|---|---|
| Frozen ABI and 2× confirmation | `libs/api/src/abi.rs:2-12` |
| Extensible, non-ABI services | `libs/api/src/services.rs:2-6` |
| Manifest v2 record and v1 compatibility | `libs/api/src/abi/manifest.rs:1-33` |
| Flat public API re-exports | `libs/api/src/lib.rs:19-31` |
| `no_std` SDK and optional features | `libs/ostd/src/lib.rs:8-15`; `libs/ostd/Cargo.toml:21-33` |
| Lifecycle, clients, discovery, and typed calls | `libs/ostd/src/app.rs:136-230`; `libs/ostd/src/clients.rs:19-49`; `libs/ostd/src/service.rs:95-128` |
| SAS grant identity limitation | `libs/ostd/src/grant.rs:1-10`; `libs/ostd/src/grant.rs:67-124` |
| ViUI dual APIs and GLES2 skeleton | `libs/viui/src/lib.rs:24-30`; `libs/viui/Cargo.toml:18-22`; `libs/viui/src/gles2_canvas.rs:1-10` |
| Zig v1 and non-unified build output | `libs/zig-syscall/src/manifest.zig:1-32`; `scripts/build-zig-cells.ps1:28-57` |
| Signing chain and tamper check | `scripts/lib-sign-cells.sh:40-64`; `scripts/test-cell-signing.sh:49-117` |
| Native Tier 2 remains a gated design | [Spec 22 §1–2](22-native-domain-cell-implementation-gate.md) |

Known gaps are deliberately not hidden by this contract: no `rust-std` PAL or
target; no Tier 2 runtime; no domain-safe FFI/Lua contract; no stable GPU
backend; no unified observability facade; no profile-independent build→package
→sign pipeline. The acceptance ledger exists, but its current blockers and
missing witnesses are the reason affected matrix cells are not **USABLE**.

## 10. Cross-references

- [ADR 0003 — Application tier taxonomy](../decisions/0003-application-tier-taxonomy.md)
- [Spec 17 — Cell IPC wire contract](17-ipc-wire-contract.md)
- [Spec 22 — Tier 2 native-domain implementation gate](22-native-domain-cell-implementation-gate.md)
- [Application development guide](../app-development-guide.md)
- [Security model](../security-model.md)
