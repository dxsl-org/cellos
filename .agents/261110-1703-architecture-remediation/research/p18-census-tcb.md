# Phase 18 census, part 18b: Tier-1 TCB perimeter (S8b step 1, for steps 6–7)

Measured read-only on worktree `p18-drivers` at `539401b08f`. Paths are repo-relative. **[INFERENCE]** marks a claim that was not executed or observed. Everything else was read at the cited lines or computed by the method stated in the section. The only commands run were `scripts/cellos-sign --check` (exit 0: "154 crates and 1219 files scanned; unsafe confined to 91 allowlisted files", F5 "rustc f53b654a8882 is the pinned nightly-2026-05-01") and `cargo metadata --locked --offline --format-version 1 [--filter-platform T]`. Neither wrote to the tree.

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| T1 | **"Signer refuses" and the CI check are the same code.** `--check` and `--sign` both call `cli.run_check` → `policy.check` + `toolchain.check`. `--sign` forces strict. A refusal therefore bites in CI (`f1-admission`) and in every image recipe's sign step. It never bites in `cargo build` or in the kernel. | `scripts/cellos_sign/cli.py:55-76,79-105,108-134`; `.github/workflows/ci.yml:2219-2246` |
| T2 | **The allowlist `class` field is free text and no code reads it.** It is required and non-empty, but there is no enum. A typo such as `c_ffi` would pass schema and evade a class-keyed gate. | `scripts/cellos_sign/allowlist.py:27,88-106`; no other `klass` use in `scripts/cellos_sign/` |
| T3 | **The tier is chosen by the signed manifest byte.** A valid signature gives Tier 1 unless the manifest class is FFI or UNTRUSTED. An absent manifest gives Tier 1. The signature covers every ELF byte except its own 64, so the manifest class is signed. | `kernel/src/loader/governed_spawn.rs:56-83`; `kernel/src/signing.rs:7-12`; `scripts/sign-cell.py:202-211` |
| T4 | **Three crates mismatch: `service-net`, `service-compositor`, `app-c-math-smoke`.** The 16 other `c-ffi` crates already declare FFI or UNTRUSTED. `service-net` and `service-compositor` ship in every desktop-capable image. `app-c-math-smoke` ships nowhere. | §3 |
| T5 | **The Tier-1 closure has 7 in-tree crates with `unsafe`: 564 tokens in 54 files.** `ostd` 225, `posix-shim` 160, `virtio-drivers` (vendored under `kernel/third_party`) 160, `virtio-pci-transport` 12, `wire` 3, `cellos-fs` 2, `tensor-math` 2. `abi` and `types` have **zero**. | §4.2 |
| T6 | **109 linked third-party crates contain `unsafe`, 7 480 tokens in total.** The largest are `portable-atomic`, `zerocopy`, `hashbrown`, `memchr`, `lock_api`. | §4.3, Appendix A |
| T7 | **`libs/api` is gone.** It was removed by `077909a812` (2026-11-13, "libs/api is gone"). The plan's success criterion that names it is stale. | `ls libs/`; `git show -s 077909a812` |
| T8 | **`posix-shim` (160 tokens) is in the Tier-1 closure only through `app-c-math-smoke`.** Reclassifying or dropping that one test crate removes 160 tokens from the baseline. | §4.2 user column |
| T9 | **Spec 16 is wrong in six places** (dependency-tree claim, TCB table, FFI tier, `std`/`alloc` claim, miri target, F1 scope). **Spec 13 is stale on driver `forbid`, the registry shape and release-on-exit.** Spec 13 says nothing about NIC roles. | §5 |
| T10 | **The token rule misses three unsafe attributes.** rustc's `unsafe_code` lint fires on `#[no_mangle]`, `#[export_name]` and `#[link_section]`; the scanner only counts the `unsafe` keyword. In the closure: `posix-shim` 136 `no_mangle`; `ostd` 9 `no_mangle`, 3 `export_name`, 4 `link_section`; `abi` 5 `link_section`, 1 `export_name`; `wire` 1 `link_section`. | `scripts/cellos_sign/scan.py:28`; §4.2 |
| T11 | **Plan line references are stale.** The reviewer's `governed_spawn.rs:68-70` is now `:72-83`. | `kernel/src/loader/governed_spawn.rs:72-83` |

## 1. Signer and scan today

### 1.1 Entry points

| Element | Behaviour | Evidence |
|---|---|---|
| `scripts/cellos-sign` | Thin launcher. It puts `scripts/` on `sys.path` and calls `cellos_sign.cli.main`. | `scripts/cellos-sign:1-20` |
| Modes | Exactly one of `--check` or `--sign ELF...`. | `scripts/cellos_sign/cli.py:166-171` |
| Exit codes | 0 ok, 1 policy (F1/F5), 2 usage/config, 3 signing refused. | `scripts/cellos_sign/cli.py:1-4,18` |
| `run_check` | Loads the allowlist, runs `policy.check` and `toolchain.check`. "Pure source parsing: no build, no network." With `--strict`, a skipped F5 fails. | `scripts/cellos_sign/cli.py:55-76` |
| `run_sign` | Sets `args.strict = True`, runs `run_check`, refuses on failure, then calls `signing.sign_and_verify`. | `scripts/cellos_sign/cli.py:79-105` |
| `sign_route` | Python-only API for the x86 assembler. Same strict check, then a route-admitted dev-key batch. | `scripts/cellos_sign/cli.py:108-134`; `scripts/cellos_sign/signing.py:152-200` |
| Low-level signer | `scripts/sign-cell.py` refuses unless `_CHECKED` is set (only `sign_and_verify` sets it) or `--unchecked-dev-signature` is passed (dev key only, warns "must never be shipped"). | `scripts/sign-cell.py:80,312-335`; `scripts/cellos_sign/signing.py:243`; only user `scripts/test-cell-signing.sh:53-57` |
| Scope of the verdict | Per tree, not per binary. One violation anywhere refuses every target in the batch. | `scripts/gen-disk-aarch64-ci.sh:246-248`; `scripts/gen-disk-ci.sh:275-277` |

### 1.2 F1: two layers

| Layer | Rule | Evidence |
|---|---|---|
| Scope | `CELL_ROOTS = ["cells"]`. `libs/*` is "trusted TCB and out of F1 scope". | `scripts/cellos_sign/policy.py:28-32,44`; `scripts/cellos_sign/__init__.py:31-32` |
| Input set | Git-tracked `.rs` under `cells/` (filesystem walk only outside a checkout). | `scripts/cellos_sign/policy.py:89-94`; `scripts/cellos_sign/scan.py:53-77` |
| Reduction | Comments, strings, raw strings and char literals are blanked; line structure is kept. | `scripts/cellos_sign/lexer.py:1-28,85-130` |
| Attribute | Every crate root (lib path, bin paths, `src/main.rs`, `src/bin/*.rs`) must match `^[ \t]*#!\s*\[\s*forbid\s*\(…unsafe_code…\)\s*\]`, unless the crate name is in `[[crate]]`. `[lints.rust] unsafe_code = "forbid"` in `Cargo.toml` is **not** recognised. | `scripts/cellos_sign/scan.py:33-36,95-115`; `scripts/cellos_sign/policy.py:108-126` |
| Token | Any tracked file with `\bunsafe\b(?!_)` in reduced code must be a `[[file]]` path. | `scripts/cellos_sign/scan.py:28,43-45`; `scripts/cellos_sign/policy.py:128-149` |
| Hygiene | Unused `[[file]]`/`[[crate]]` entries are NOTEs. Stale entries are NOTEs, and violations under `--strict`. | `scripts/cellos_sign/policy.py:151-175` |
| Known gap | `forbid` does not see `unsafe` produced by another crate's macro; `forbid` is per crate. | `scripts/cellos_sign/policy.py:20-27` |

### 1.3 F5: toolchain pin

`toolchain.check` compares the commit hash of `rustc -Vv` with that of `rustc +<pin> -Vv`, where the pin is `[toolchain] channel` from `rust-toolchain.toml` (`nightly-2026-05-01`). No working `rustc` is "skipped" (a failure under strict). A missing pin or an uninstalled pinned toolchain fails. `scripts/cellos_sign/toolchain.py:40-99`; `rust-toolchain.toml:2`. CI installs the same nightly in the job. `.github/workflows/ci.yml:2235-2238`.

### 1.4 Allowlist schema and matching

| Field / rule | Behaviour | Evidence |
|---|---|---|
| `version` | Must be 1. | `scripts/cellos_sign/allowlist.py:119-120`; `scripts/unsafe-allowlist.toml:28` |
| `max_age_days` | 90. Entries without `review_by` older than this are stale. | `scripts/unsafe-allowlist.toml:33`; `scripts/cellos_sign/allowlist.py:45-53` |
| `[[file]]` key | `path`: exact repo-relative POSIX path, compared to the tracked path. | `scripts/cellos_sign/policy.py:139` |
| `[[crate]]` key | `name`: exact Cargo package name. | `scripts/cellos_sign/policy.py:111` |
| Required | `class`, `reason`, `approver`, `date` (non-empty). | `scripts/cellos_sign/allowlist.py:27,88-93` |
| Optional | `review_by`, `tracking`. Any other key is fatal. Duplicates are fatal. | `scripts/cellos_sign/allowlist.py:94-96,126-136` |
| `class` | Stored as `Entry.klass`; no enum, no reader. | `scripts/cellos_sign/allowlist.py:103` |

The six classes in use. Their meaning is not written down anywhere; it is inferred from the entries.

| Class | `[[file]]` | `[[crate]]` | Meaning (from the entries) | Example |
|---|---|---|---|---|
| `driver-mmio` | 45 | 20 | Volatile MMIO and DMA descriptor setup in a driver cell; the wall is the IOMMU, not LBI. | `scripts/unsafe-allowlist.toml:261-263` |
| `c-ffi` | 25 | 16 | Calls across the C ABI (doomgeneric, Lua, QuickJS, MuPDF, mlibc, posix-shim, C++ witnesses). | `scripts/unsafe-allowlist.toml:74-76` |
| `test-harness` | 10 | 3 | Deliberate unsafe that is the property under test, or host-only test code. | `scripts/unsafe-allowlist.toml:39-41,1091-1094` |
| `test-violation` | 2 | 6 | Deliberate CFI/W^X/Tier-2 violation witnesses. | `scripts/unsafe-allowlist.toml:632-634` |
| `hypervisor` | 5 | 2 | Guest mailbox and timer-register access in the hypervisor service and silo guest. | `scripts/unsafe-allowlist.toml:586-588,606` |
| `legacy-raw-ptr` | 4 | 2 | Raw pointers kept from older code: a Grant blit slice, and the net TLS borrow bridge. | `scripts/unsafe-allowlist.toml:650-673` |

Totals: 91 `[[file]]`, 49 `[[crate]]`; 41 `c-ffi` and 6 `legacy-raw-ptr` entries, as the request states.

### 1.5 Where the check runs

| Site | Mode | Evidence |
|---|---|---|
| CI job `f1-admission` | `test_cellos_sign.py`, then `cellos-sign --check --strict` | `.github/workflows/ci.yml:2219-2246` |
| `scripts/lib-sign-cells.sh` `sign_cells` | `--sign` (19 recipe scripts source it, e.g. `gen-disk-ci.sh:302`, `gen-disk-aarch64-ci.sh:252`, `build-x86_64-domain-test-ci.sh:152`) | `scripts/lib-sign-cells.sh:60-86` |
| `gen_disk.ps1` (Windows RISC-V image) | `--sign` | `gen_disk.ps1:239,335-341` |
| `scripts/build-aarch64-cells.ps1` | `--sign` | `scripts/build-aarch64-cells.ps1:328-330` |
| `scripts/build-test-hooks-cells.ps1` | `--sign` | `scripts/build-test-hooks-cells.ps1:71` |
| `scripts/assemble-x86-image.py` | `--sign` (private seed) or `cli.sign_route` (dev key) | `scripts/assemble-x86-image.py:357-376` |
| `scripts/build-cloud-images.sh` | `--sign` | `scripts/build-cloud-images.sh:104` |
| `scripts/flash-sd-physical.sh` | `--check --strict` | `scripts/flash-sd-physical.sh:103` |
| `cargo build` | never | — |
| Kernel at spawn | never; it trusts signature + manifest | `kernel/src/loader/governed_spawn.rs:71-83` |

`scripts/sign-policy.py` is a different tool. It signs the operator policy blob (`VPOL`), not cells, and runs no F1 check. `scripts/sign-policy.py:1-40`.

## 2. Tier at load, and what the signature covers

| Step | Behaviour | Evidence |
|---|---|---|
| Manifest parse | `__ViCell_manifest` is Absent, Valid (8-byte v1 or 16-byte v2) or Malformed. Malformed is denied. | `kernel/src/loader/manifest_section.rs:25-62`; `kernel/src/loader/governed_spawn.rs:56-67` |
| Signed | Valid signature → `is_domain = manifest is FFI or UNTRUSTED`. Absent manifest → `false` (Tier 1). | `kernel/src/loader/governed_spawn.rs:71-83` |
| Bad signature | Denied. Verify that could not run → refused with its error. | `kernel/src/loader/governed_spawn.rs:84-100` |
| Unsigned | Browser profile: denied. Release posture: denied. Dev posture: Tier 2. | `kernel/src/loader/governed_spawn.rs:102-120` |
| x86 PKU key | `granted_protection_class`: LEGACY → floor by caps (`TRUSTED_CORE` if any of block_io/network/spawn/hypervisor, else `STANDARD`); otherwise `max(requested, floor)`. | `kernel/src/loader/governed_spawn.rs:190-201`; `kernel/src/task/cap.rs:493-508` |
| Domain bind | `is_domain` binds a private address space at publication. Without `native-domains` + `cellos_wide` it returns `NotSupported`, so a signed FFI/UNTRUSTED cell is refused there. | `kernel/src/task/launch.rs:227-233,431-473,573-577` |
| Init | Embedded, no signature check, `is_domain = false`. | `kernel/src/loader/governed_spawn.rs:309-342` |
| Class values | `TRUSTED_CORE` 0, `STANDARD` 1, `FFI` 2, `UNTRUSTED` 3, `LEGACY` 0xFF. The tier-less `declare_manifest!` forms emit LEGACY. | `libs/abi/src/manifest_flags.rs:24-50`; `libs/abi/src/manifest_macro.rs:11-14,93-120` |
| Signature coverage | Every byte of the final ELF except the 64 signature bytes; section headers included. The manifest tier byte (offset 5) is therefore signed. | `kernel/src/signing.rs:7-12,216-260`; `scripts/sign-cell.py:202-211`; `libs/abi/src/manifest.rs:15` |
| Provenance | The CLI does not bind a target ELF to the checked tree; Spec 18 leaves that to the CI job. | `docs/specs/18-cell-trust-tiers.md:116-118` |

Consequence: the source-level class gate is enough to stop a mismatched crate from being signed by the pipeline. Nothing after signing can change the class. **[INFERENCE]** This holds only for ELFs built from the checked tree, which the CLI cannot prove (row "Provenance").

## 3. Allowlist × manifest join

### 3.1 Method

1. Load `scripts/unsafe-allowlist.toml`. Take every entry whose class is `c-ffi` or `legacy-raw-ptr`.
2. Map a `[[file]]` path to its crate with the signer's own ownership rule (`scan.discover_crates(repo, ["cells"])`, deepest package directory wins). A `[[crate]]` key is the crate. Every one of the 29 file entries maps to a crate that also has a `[[crate]]` entry of the same class.
3. Read the crate's manifest class from its reduced sources: the `tier = …` argument of `declare_manifest!` / `app_entry!`. All tier arguments in `cells/` are spelled `abi::manifest::PROTECTION_CLASS_FFI` (13) or `…_UNTRUSTED` (14). No tier argument → LEGACY. No macro at all → Absent.
4. Effective tier when signed: FFI/UNTRUSTED → Tier 2; LEGACY/STANDARD/TRUSTED_CORE/Absent → Tier 1 (§2).
5. Shipping: the binary name from `[[bin]]` (or the package name) was searched in `scripts/assemble-x86-image.py` profiles, `scripts/gen-disk-ci.sh`, `gen_disk.ps1`, `gen_disk_rpi3.ps1`, `scripts/gen-disk-aarch64-ci.sh`, `scripts/build-aarch64-cells.ps1` and the `scripts/build-*-ci.sh` / `scripts/qemu-*.sh` lane images.

The join covers 18 crates: 16 `c-ffi`, 2 `legacy-raw-ptr`.

### 3.2 Full join

| Crate | Class | Manifest class (cite) | Tier when signed | Shipped / used |
|---|---|---|---|---|
| `doom` | c-ffi | FFI `cells/demos/doom/src/main.rs:50-54` | 2 | RISC-V disk `scripts/gen-disk-ci.sh:415`; `gen_disk.ps1:323` |
| `tetris-c` | c-ffi | FFI `cells/demos/tetris-c/src/main.rs:44-48` | 2 | `scripts/gen-disk-ci.sh:417`; `gen_disk.ps1:325` |
| `tetris-lua` | c-ffi | FFI `cells/demos/tetris-lua/src/main.rs:39-43` | 2 | `scripts/gen-disk-ci.sh:418`; `gen_disk.ps1:612` |
| `lua` | c-ffi | UNTRUSTED `cells/runtimes/lua/src/main.rs:15-19` | 2 | `scripts/gen-disk-ci.sh:397,413`; `gen_disk.ps1:594,604` |
| `ocel-quickjs` | c-ffi | UNTRUSTED `cells/services/ocel-quickjs/src/main.rs:30-34` | 2 | `scripts/gen-disk-ci.sh:356,451`; `gen_disk.ps1:546` |
| `ocel-pdf` | c-ffi | UNTRUSTED `cells/services/ocel-pdf/src/main.rs:15-19` | 2 | `scripts/gen-disk-ci.sh:351,444`; `gen_disk.ps1:541` |
| `app-cpp-smoke` | c-ffi | FFI `cells/tests/cpp-smoke/src/main.rs:34-38` | 2 | x86 domain-test image `scripts/build-x86_64-domain-test-ci.sh:107,129` |
| `app-mlibc-smoke` | c-ffi | FFI `cells/tests/mlibc-smoke/src/main.rs:24-28` | 2 | `scripts/build-x86_64-mlibc-witness-ci.sh:65-70` |
| `app-mlibc-posix-x86` | c-ffi | UNTRUSTED `cells/tests/mlibc-posix-x86/src/main.rs:19-23` | 2 | `scripts/build-x86_64-mlibc-witness-ci.sh:65-70` |
| `app-drm-lite-x86` | c-ffi | UNTRUSTED `cells/tests/drm-lite-x86/src/main.rs:20-24` | 2 | no image script names it |
| `app-cpp-hosted-smoke` | c-ffi | UNTRUSTED `cells/tests/cpp-hosted-smoke/src/main.rs:19-23` | 2 | `scripts/build-x86_64-cpp-hosted-ci.sh:52-53,93` |
| `app-posix-shim-test` | c-ffi | FFI `cells/tests/posix-shim-test/src/main.rs:37-41` | 2 | `scripts/gen-disk-ci.sh:383,465`; `gen_disk.ps1:582`; `scripts/build-srv-test-ci.sh:105` |
| `c-pthread` | c-ffi | FFI `cells/tests/c-pthread/src/main.rs:9-13` | 2 | private disk copy `scripts/qemu-c-pthread.sh:39` |
| `app-c-spawn` | c-ffi | FFI `cells/tests/c-spawn/src/main.rs:15-19` | 2 | private disk copy `scripts/qemu-c-spawn.sh:90` |
| `app-c-spawn-child` | c-ffi | FFI `cells/tests/c-spawn-child/src/main.rs:12-16` | 2 | private disk copy `scripts/qemu-c-spawn.sh:90` |
| **`app-c-math-smoke`** | c-ffi | **LEGACY** `cells/tests/c-math-smoke/src/main.rs:19` | **1** | **none**: "not shipped to disk image" `cells/tests/c-math-smoke/Cargo.toml:6`; compiled by CI rv64/aarch64 workspace builds `.github/workflows/ci.yml:284-299`; no sign site names it |
| **`service-net`** | legacy-raw-ptr | **LEGACY** `cells/services/net/src/main.rs:38` | **1** | x86 headless + desktop `scripts/assemble-x86-image.py:124`; RISC-V `scripts/gen-disk-ci.sh:426`, `gen_disk.ps1:267`; AArch64 `scripts/gen-disk-aarch64-ci.sh:198`, `scripts/build-aarch64-cells.ps1:263`; Pi 3 `gen_disk_rpi3.ps1:183` |
| **`service-compositor`** | legacy-raw-ptr | **Absent** (no `declare_manifest!`/`app_entry!` anywhere under `cells/services/compositor/`) | **1** | x86 desktop `scripts/assemble-x86-image.py:158`; RISC-V `scripts/gen-disk-ci.sh:436`, `gen_disk.ps1:270`; AArch64 `scripts/build-aarch64-cells.ps1:279`; Pi 3 `gen_disk_rpi3.ps1:183` |

### 3.3 Mismatch table (the work list for owner triage)

| Crate | Allowlist class (entries) | Manifest class | Shipped where | Proposed exception reason |
|---|---|---|---|---|
| `service-net` | legacy-raw-ptr (`scripts/unsafe-allowlist.toml:657,664,673,1053`) | LEGACY → Tier 1, PKU floor `TRUSTED_CORE` (network cap) | every x86, RISC-V and AArch64 image | "Boot network service; holds the network capability. `tls/{dispatch,socket,transport}.rs` erase a `SocketSet` borrow into task-local raw pointers for one `poll_future` call. Tier 2 is not available on configurations without `native-domains` (`kernel/src/task/launch.rs:462-473`). Triage: replace the pointer bridge with an owned context, then drop the class." |
| `service-compositor` | legacy-raw-ptr (`:650,1044`) | Absent → Tier 1 | x86 desktop; RISC-V, AArch64 and Pi 3 desktop images | "Desktop compositor; blits from a shared Grant through a slice formed right after `GrantSlice` resolves it. Zero-copy SAS grants are a Tier-1 IPC mechanism (`docs/specs/18-cell-trust-tiers.md:51`). Triage: a safe bounded-copy accessor in `ostd`, then drop the class." |
| `app-c-math-smoke` | c-ffi (`:206,731`) | LEGACY → Tier 1 | not shipped, never signed | "Link-proof smoke test of `posix-shim`; never signed or shipped. Triage: declare `PROTECTION_CLASS_FFI` like the other 15 C cells, or delete." |

Other classes, outside the gate rule, for completeness: all 20 `driver-mmio` crates, `service-hypervisor` (`hypervisor`), `cfi-test`, `wx-test` and `tier2-rpc-driver` (`test-violation`), and `recovery-demo`, `app-cellbox` (`test-harness`) have Tier-1 manifests. That is by design for drivers (`docs/specs/18-cell-trust-tiers.md:51`). `app-cellbox`'s entry is host-only test code (`scripts/unsafe-allowlist.toml:1091-1094`).

## 4. Tier-1 dependency closure

### 4.1 Method

- `cargo metadata --locked --offline --format-version 1 --filter-platform T` for `x86_64-unknown-none`, `riscv64gc-unknown-none-elf`, `aarch64-unknown-none`.
- Roots: the 149 workspace members under `cells/`, minus the 27 whose manifest is FFI or UNTRUSTED (§3.1 rule): **122 Tier-1 roots**.
- Edges: resolve-graph dependencies with a `normal` kind only (no dev, no build). Traversal stops at proc-macro crates; they run on the host, and their output is counted where it lands (§4.4).
- Union over the three targets: 40 in-tree non-cell packages and 194 registry packages. The targets differ only in `cpufeatures` (x86 and AArch64).
- **[INFERENCE]** `cargo metadata` unifies features across the workspace, so the closure is a superset of what any one cell links.

**Counting rule.** For each package, every git-tracked `.rs` file it owns (deepest package directory wins, `target/` ignored) is reduced with `cellos_sign.lexer.strip_noncode`. A site is one match of the signer's `UNSAFE_RE = \bunsafe\b(?!_)` (`scripts/cellos_sign/scan.py:28`). This is exactly the F1 token rule. The split columns count `unsafe {` (block), `unsafe [extern] fn`, `unsafe impl`, `unsafe trait`, `unsafe extern {` and `unsafe(` (attribute); they sum to the token total in every row (no "other"). Files under `tests/`, `benches/`, `examples/` and a top-level `build.rs` are excluded; `#[cfg(test)]` modules inside `src/` are **included** (the rule cannot see cfg). For registry crates the same rule runs over the package source in the Cargo cache.

### 4.2 In-tree crates in the closure

"Tier-1 cells" = how many of the 122 roots link the crate on at least one target.

| Crate dir | Package | Tier-1 cells linking it | `.rs` files | Files with `unsafe` | `unsafe` tokens | blocks | fns | impls | traits | `unsafe(..)` attrs | `unsafe_code` lint |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `hal/soc/bcm27xx` | `hal-soc-bcm27xx` | 8 | 6 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/adc` | `hal-adc` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/can` | `hal-can` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/gpio` | `hal-gpio` | 14 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/i2c` | `hal-i2c` | 5 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/pwm` | `hal-pwm` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/spi` | `hal-spi` | 4 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `hal/traits/uart` | `hal-uart` | 3 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `kernel/third_party/virtio_drivers` | `virtio-drivers` | 5 | 28 | 17 | **160** | 110 | 30 | 19 | 1 | 0 | none |
| `libs/abi` | `abi` | 115 | 46 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` deny |
| `libs/agent-proto` | `agent-proto` | 6 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` forbid |
| `libs/ai-engine` | `ai-engine` | 1 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root, `[lints]` deny |
| `libs/ai-proto` | `ai-proto` | 4 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` deny |
| `libs/ai-sdk` | `ai-sdk` | 3 | 4 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` deny |
| `libs/ai-tokenizer` | `ai-tokenizer` | 1 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` deny |
| `libs/boot-config` | `cellos-boot-config` | 1 | 2 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `libs/cellos-fs` | `cellos-fs` | 1 | 9 | 1 | **2** | 1 | 1 | 0 | 0 | 0 | none |
| `libs/dom-arena` | `dom-arena` | 2 | 2 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/gguf-rs` | `gguf-rs` | 1 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root, `[lints]` deny |
| `libs/http-core` | `http-core` | 114 | 13 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | none |
| `libs/intel-gpu-core` | `intel-gpu-core` | 2 | 33 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/kms-codec` | `kms-codec` | 114 | 27 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` forbid |
| `libs/ocel-pdf-proto` | `ocel-pdf-proto` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/ocel-service-proto` | `ocel-service-proto` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/ostd` | `ostd` | 114 | 80 | 19 | **225** | 203 | 18 | 3 | 0 | 1 | none |
| `libs/posix-shim` | `posix-shim` | 1 | 12 | 11 | **160** | 7 | 144 | 1 | 0 | 8 | `[lints]` deny |
| `libs/scsi-block` | `scsi-block` | 2 | 8 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/tensor-math` | `tensor-math` | 1 | 3 | 1 | **2** | 2 | 0 | 0 | 0 | 0 | `[lints]` deny |
| `libs/terminal` | `terminal-sdk` | 10 | 13 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/text-engine` | `text-engine` | 4 | 26 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/types` | `types` | 118 | 6 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `[lints]` forbid |
| `libs/vigrep` | `vigrep` | 1 | 33 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/vimio` | `vimio` | 2 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/vipolling` | `vipolling` | 1 | 1 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/virtio-pci-transport` | `virtio-pci-transport` | 3 | 16 | 4 | **12** | 9 | 1 | 2 | 0 | 0 | none |
| `libs/vissh` | `vissh` | 2 | 5 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/vitui` | `vitui` | 1 | 7 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/viui` | `viui` | 7 | 73 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |
| `libs/wire` | `wire` | 115 | 38 | 1 | **3** | 0 | 3 | 0 | 0 | 0 | `[lints]` deny |
| `third_party/sunset` | `sunset` | 2 | 26 | 0 | **0** | 0 | 0 | 0 | 0 | 0 | `#![forbid]` root |

Total: **564 tokens in 54 files, 7 crates.**

Per-file sites (the baseline granularity proposed in §6.2):

| Crate | Files and tokens |
|---|---|
| `ostd` (225) | `syscall.rs` 145, `vmm.rs` 12, `executor.rs` 10, `session.rs` 9, `locked-heap-tests.rs` 9, `grant.rs` 8, `heap.rs` 6, `locked-heap.rs` 6, `startup.rs` 4, `ring_channel.rs` 3, `dma.rs` 2, `fs.rs` 2, `mmio.rs` 2, `ring-channel-tests.rs` 2, `clients/kms/ssh.rs` 1, `display/surface.rs` 1, `ipc.rs` 1, `repl.rs` 1, `task.rs` 1 (all under `libs/ostd/src/`) |
| `posix-shim` (160) | `sysio.rs` 51, `stdio.rs` 27, `setjmp.rs` 18, `alloc.rs` 13, `strings.rs` 13, `net.rs` 12, `vfs_at.rs` 10, `cxxabi.rs` 7, `math.rs` 4, `stdio_fmt.rs` 3, `entropy.rs` 2 |
| `virtio-drivers` (160) | 17 files under `kernel/third_party/virtio_drivers/src/`, incl. `hal/fake.rs` 10 |
| `virtio-pci-transport` (12) | `transport/mod.rs` 4, `regs.rs` 3, `tests/fake.rs` 3, `transport/ops.rs` 2 |
| `wire` (3) | `allocator.rs` 3 (three `unsafe fn` in a trait; the file opts out with `#![allow(unsafe_code)]`, `libs/wire/src/allocator.rs:12,108-120`) |
| `cellos-fs` (2) | `cow_experiment.rs` 2 (built only with `g2-cow-experiment` on a hosted target, `libs/cellos-fs/src/lib.rs:25-26`) |
| `tensor-math` (2) | `neon.rs` 2 (AArch64 + `neon` only, `libs/tensor-math/src/lib.rs:29-30`) |

Notes:

- **cfg-only sites are counted.** `#[cfg(test)]` modules: `ostd` 11 (`libs/ostd/src/locked-heap.rs:139-141`, `libs/ostd/src/ring_channel.rs:257-259`), `virtio-pci-transport` 3 (`libs/virtio-pci-transport/src/lib.rs:38-39`), `virtio-drivers` 10 (`kernel/third_party/virtio_drivers/src/hal.rs:1-2`). The signer's cell rule counts such files too.
- **Earlier counts differ by rule.** `rg -ow unsafe libs/ostd` gives 229 (comments included). The request's 239 and the review's 231 were taken on older trees or with a raw grep **[INFERENCE]**.
- **`#![forbid(unsafe_code)]` today**, either as a source root attribute or `[lints.rust] unsafe_code = "forbid"`: `types` (`libs/types/Cargo.toml:8`), `kms-codec` (`libs/kms-codec/Cargo.toml:8`), `agent-proto` (`libs/agent-proto/Cargo.toml:12`), `ai-engine` (`libs/ai-engine/src/lib.rs:39`), `dom-arena`, `gguf-rs`, `intel-gpu-core`, `ocel-pdf-proto`, `ocel-service-proto`, `scsi-block`, `terminal-sdk`, `text-engine`, `vigrep`, `vimio`, `vipolling`, `vissh` (each `libs/<dir>/src/lib.rs`, lines in the `rg` survey), `vitui` (`libs/vitui/src/lib.rs:13`), `viui` (`libs/viui/src/lib.rs:33`), `sunset` (`third_party/sunset/src/lib.rs:8`).
- **`deny` only** (a module can `#![allow]` it): `abi`, `wire`, `posix-shim`, `tensor-math`, `ai-proto`, `ai-sdk`, `ai-tokenizer` (`libs/<dir>/Cargo.toml` `[lints.rust]`). `posix-shim` allows it crate-wide (`libs/posix-shim/src/lib.rs:29`).
- **Zero `unsafe` and no `forbid`:** the 8 `hal-*` crates, `abi`, `ai-proto`, `ai-sdk`, `ai-tokenizer`, `cellos-boot-config`, `http-core`.
- `posix-shim` reaches the closure only through `app-c-math-smoke` (`cells/tests/c-math-smoke/Cargo.toml:14`). `virtio-drivers` reaches it through `driver-virtio-{blk,gpu,net,scsi}` and `service-input`. `virtio-pci-transport` through `driver-virtio-{blk,net,scsi}`.
- Eight Tier-1 roots do not link `ostd`: `driver-adc-sim`, `driver-can-loopback`, `driver-hid`, `driver-i2c-gpio`, `driver-pwm-gpio`, `driver-spi-gpio`, `edu-export-proto`, `tier2-rpc-proto` (rlib crates).

### 4.3 Registry crates

194 registry packages are linked; 109 contain `unsafe` (7 480 tokens). The full list is Appendix A. The ten largest: `portable-atomic` 1.14.0 (642), `zerocopy` 0.8.55 (486), `hashbrown` 0.17.1 (454), `memchr` 2.8.3 (333), `hashbrown` 0.16.1 (329), `hashbrown` 0.14.5 (305), `hashbrown` 0.15.5 (303), `lock_api` 0.4.14 (297), `heapless` 0.9.3 (283), `time` 0.3.54 (269). Crates linked by at least 114 of the 122 roots include `memchr`, `hashbrown` 0.15.5, `lock_api`, `allocator-api2`, `heapless` 0.8.0, `spin`, `linked_list_allocator`, `httparse`, `libm`, `byteorder`, `serde_json`, `postcard`, `serde`. **[INFERENCE]** Registry counts include code behind `cfg` that a bare-metal target never compiles (SIMD and `std` paths), so they overstate the linked surface.

### 4.4 Macro-expanded `unsafe`

- `ostd::declare_custom_heap!` expands one `unsafe` block into the calling cell (`libs/ostd/src/heap.rs:66-80`). 27 cells call it. `forbid` does not fire on an external macro's expansion (`scripts/cellos_sign/policy.py:20-24`). The site is counted once, at its definition in `heap.rs`.
- `ostd::cell_main!` emits `#[no_mangle]` (an `unsafe_code` lint trigger) without the keyword (`libs/ostd/src/entry.rs:40-53`). It is not counted by the token rule.
- `abi::declare_manifest!` emits `#[link_section]` (`libs/abi/src/manifest_macro.rs:19-21`). Not counted.
- Proc-macros on the host side (19, e.g. `serde_derive`, `zerocopy-derive`, `viui-macros`) produce code inside the calling crate, where the cell's own `forbid` and token scan see it.

## 5. Spec lines to correct

### 5.1 Spec 16 (`docs/specs/16-rustc-tcb.md`)

| Line(s) | Says | Why it is wrong | Correction |
|---|---|---|---|
| 25-29 | TCB column: `rustc`, kernel, `libs/abi` + `libs/types`. | `abi` and `types` have zero `unsafe`; `ostd`, `wire`, `virtio-drivers`, `virtio-pci-transport` (and `posix-shim` while `app-c-math-smoke` is Tier 1) are linked into Tier-1 cells and hold the unsafe. | Replace the third row with "Tier-1 dependency closure (`libs/ostd`, `libs/wire`, …; generated list)". |
| 40-42 | "rustc refuses to compile a Cell if any `unsafe` block exists anywhere in its dependency tree (excluding explicitly excepted crates)." | `forbid` is per crate; dependencies and external macro expansions are not linted (`scripts/cellos_sign/policy.py:20-27`). | Say `forbid` covers the cell crate only; the dependency closure is covered by the signer's closure scan with a per-file baseline. |
| 44-46 | "The kernel has no way to verify Cell binaries at load time — it trusts rustc." | The kernel verifies the Ed25519 signature and reads the signed manifest class (`kernel/src/loader/governed_spawn.rs:71-83`). | "The kernel trusts the signing pipeline (F1, F5, tier-class gate); it verifies only the signature and the manifest class." |
| 137-144 | §4 inventory lists `rustc`, kernel, `abi`, `types`. | Omits `ostd` (225 tokens, 114 Tier-1 cells), `wire`, the vendored `virtio-drivers`, `virtio-pci-transport` and 109 registry crates. | Add rows for each in-tree crate with `unsafe` and one row "registry crates (generated list)", with counts from the baseline file. |
| 148 | "Cell code (`#![forbid(unsafe_code)]`) — verified by the TCB, not part of it." | 29 cell crates with Tier-1 manifests carry 269 allowlisted `unsafe` tokens (driver-mmio, hypervisor, test classes, the 3 mismatches; the bare-metal `silo-guest` is not counted). | "Cell code outside the reviewed allowlist." |
| 149-150 | "Trusted C/Zig libraries via the Tier 1 `ffi-posix` profile." | C/FFI cells are Tier 2 (`docs/specs/18-cell-trust-tiers.md:45-47,58`); the new gate refuses a Tier-1 C cell without an exception. | "C/C++/Zig code runs in Tier 2; Tier-1 exceptions are listed in the allowlist." |
| 152 | "`std` / `alloc` — Cell-visible only via `libs/ostd`." | Cells link `alloc` directly (`cells/services/compositor/src/main.rs:16`). | "`alloc`/`core` internals are outside the scanned closure." |
| 189 | miri target "`libs/types` and `libs/abi` FFI boundary code". | Neither has `unsafe`; the boundary code is `libs/ostd/src/syscall.rs` (145) and `libs/posix-shim`. | Name `ostd` and `posix-shim`. |
| 222 | F1 row: scope is "Every Cell crate". | Phase 18 adds the closure scan and the tier-class gate. | Add F1b (closure ratchet) and F1c (tier-class gate), or widen F1's text. |

### 5.2 Spec 13 (`docs/specs/13-peripherals.md`)

Spec 13 says nothing about NIC roles or the NIC singleton. The singleton is documented only in the generated globals table (`docs/generated/mutable-globals.md:172`, "replaced on every RegisterNicDriver") and the changelog (`docs/project-changelog.md:668-674`). The per-BDF route change therefore needs no Spec 13 edit for NIC roles. These Spec 13 lines are stale today or become stale with the ownership record:

| Line(s) | Says | Status |
|---|---|---|
| 16-17 | Peripheral drivers are "true Driver Cells (`#![forbid(unsafe_code)]`)". | True for the bus drivers (gpio, uart, i2c-gpio …, none allowlisted). False for the 20 `driver-mmio` crates. After `libs/nic-driver`, NIC MMIO/DMA unsafe may move into the closure. State the split. |
| 32 | Diagram: Driver Cell box marked `#![forbid(unsafe_code)]`. | Same. |
| 45-48 | Registry is `BTreeMap<MmioRange, CellId>`, "release-on-exit (hook…)". | Code is `BTreeMap<usize, MmioRegion { len, owner_tid, class }>` keyed by base (`kernel/src/resource_registry.rs:107-119`), plus a separate `PCIE_BARS` map (`:149-150`). Phase 18 replaces both with one `DeviceRecord` keyed by BDF or MMIO base. |
| 100-103 | §5: exclusive grant per range; `BTreeMap<MmioRange, CellId>`; release-on-exit "design as hook"; v1 ranges hard-coded for ARM virt. | Release exists (`release_mmio_for`, `release_bdfs_for`, `kernel/src/resource_registry.rs:497,1094`). IRQ and DMA ownership are decided elsewhere. Rewrite §5 around the ownership record. |
| 194 | "Release-on-exit hook ⚠️ … implement later." | Implemented (row above). |

### 5.3 Adjacent statements the same change invalidates

| Location | Says | Correction |
|---|---|---|
| `docs/specs/18-cell-trust-tiers.md:112-114` | F1 covers `cells/`; "`libs/*` remains reviewed TCB rather than part of this ratchet". | Name the closure scan and the tier-class gate. |
| `scripts/cellos_sign/__init__.py:31-32` | "F1 covers `cells/` only: `libs/*` is trusted TCB, reviewed rather than ratcheted". | Same. |
| `scripts/cellos_sign/policy.py:28-32` | "`libs/*` is trusted TCB and out of F1 scope." | Same. |
| `docs/specs/15-kernel-boundary.md:226` | NIC home `cells/drivers/nic-*/`. | No such directory; NIC cells are `cells/drivers/{igc,ixgbe,…}` plus `libs/nic-driver`. |
| Phase plan success criterion | "Unsafe scan reports `libs/ostd` and `libs/api` entries". | `libs/api` no longer exists (T7). Use `libs/ostd` and `libs/posix-shim`. |

## 6. Design proposal

### 6.1 Tier-class gate (step 6)

**Rule.** For each crate under `cells/`: let *C* be the union of the classes of its `[[crate]]` entry and the `[[file]]` entries it owns. If *C* contains `c-ffi` or `legacy-raw-ptr`, the crate's manifest class must be FFI or UNTRUSTED, or a `[[tier_exception]]` entry must name it. Otherwise it is a violation of layer `tier-class`.

**Manifest class from source, fail closed.** A crate counts as FFI/UNTRUSTED only if it has at least one `declare_manifest!`/`app_entry!` invocation and every such invocation in its reduced sources passes `tier = …PROTECTION_CLASS_FFI|PROTECTION_CLASS_UNTRUSTED|TIER_TIER1B_FFI|TIER_UNTRUSTED`. Any other spelling (a local `const`, a re-export) reads as Tier 1. That is a false positive, which the threat model accepts (`scripts/cellos_sign/__init__.py:34-36`). Multi-binary crates are handled by the "every invocation" clause.

**Exception entry** (new table in `scripts/unsafe-allowlist.toml`, strict schema like the others):

```toml
[[tier_exception]]
crate = "service-net"
class = "legacy-raw-ptr"     # must equal the class that triggers the gate
manifest = "LEGACY"          # the manifest class accepted; a change re-opens review
reason = "…"
approver = "dmin"
date = "2026-11-16"
review_by = "2027-02-14"     # required: exceptions are a triage list
tracking = "phase-18 evidence: tier-gate triage"
```

"Signed" means an approver and a date in a reviewed diff, as for every allowlist entry. It is not a cryptographic signature. An exception that no longer matches a mismatch is reported unused, and is a violation under `--strict`, so the list cannot rot.

**Class enum.** `allowlist.py` rejects any `class` outside `{driver-mmio, c-ffi, legacy-raw-ptr, hypervisor, test-harness, test-violation}`. This closes T2. The unit tests that use `class = "c"` (`scripts/test_cellos_sign.py:83,156,158,160`) change to a real class.

**Placement.** Logic in a new `scripts/cellos_sign/tier_class.py`, called from `policy.check`, so there is still one implementation (`scripts/cellos_sign/policy.py:3-6`). `scripts/check-tier-class.py` (named in the plan) is a thin report entry that prints the §3.2 join for the evidence and has `--negative-control` (§6.3).

**Where it runs.**

| Site | Effect |
|---|---|
| CI `f1-admission` (`cellos-sign --check --strict`) | PR fails on a mismatch without an exception. |
| Every image recipe's sign step (§1.5) | Signing refused for the whole batch (exit 1). |
| `flash-sd-physical.sh` | Refused before flashing. |
| `cargo build`, kernel | Unaffected. Already-signed artifacts are unaffected. |

**Rejected: an artifact-level check at `--sign`** (read the tier byte from each target ELF). It needs a binary→crate map the signer does not have, and Spec 18 already leaves artifact provenance to the CI job (`docs/specs/18-cell-trust-tiers.md:116-118`). The source rule plus the signed manifest byte (§2) is enough for the plan's criterion.

### 6.2 Closure scan and ratchet (step 7)

**Closure file.** `scripts/gen-tier1-closure.py` runs §4.1 (three targets, normal edges, stop at proc-macros, Tier-1 roots by the §6.1 parser) and writes `scripts/tier1-closure.generated.toml`: in-tree package directories, and registry `name@version` with their token counts. `--check` regenerates and diffs. It runs in the `f1-admission` job, which already installs the toolchain (`.github/workflows/ci.yml:2235-2238`). Precedents: `scripts/catalog.generated.json` + `scripts/check-catalog.py`, and the mutable-globals drift gate (`.github/workflows/ci.yml:99-108,119-123`). The signer reads only the committed file, so it keeps "no build, no network" (`scripts/cellos_sign/cli.py:56`).

**Baseline file.** `scripts/tcb-unsafe-baseline.toml`, one `[[site]]` per file: `path`, `unsafe = N`, `approver`, `date`, `reason`. 54 entries at HEAD (§4.2). It is separate from the allowlist because baseline rows are ceilings, not approved holes, and they must not go stale every 90 days (`scripts/unsafe-allowlist.toml:33`).

**Check** (new `scripts/cellos_sign/tcb.py`, called from `policy.check`):

| Condition | Result |
|---|---|
| A tracked file of a closure package has more tokens than its ceiling, or has tokens and no row | violation (layer `tcb`) |
| A file has fewer tokens than its ceiling | NOTE; violation under `--strict` (CI and signing), so the ceiling is lowered in the same commit |
| A row names a file outside the closure or no longer present | NOTE; violation under `--strict` |
| Same lexer and `UNSAFE_RE` as the cell token rule | one counting rule in the repo |

This is the `check-hal-boundaries.sh` ceiling idea (`scripts/check-hal-boundaries.sh:67-85`) at file granularity, with the downward direction enforced, which that script only states in a comment.

**Attribute sites (T10).** Count `#[no_mangle]`, `#[export_name]` and `#[link_section]` (bare or inside `unsafe(…)` / `cfg_attr`) as a second ceiling per row (`attrs = N`). At HEAD: `posix-shim` 136, `ostd` 16, `abi` 6, `wire` 1. These are what `forbid(unsafe_code)` would reject.

**Registry crates.** The generated file pins `name@version → tokens`. The drift gate fails when a new `name@version` with tokens > 0 enters the closure. A version bump therefore shows up as a reviewed diff of the generated file. The signer cannot count registry sources offline, so this part runs in CI only.

**Not proposed:** forcing `forbid` onto the 14 zero-`unsafe` closure crates without it. With per-file ceilings, a crate with no rows cannot gain `unsafe` anyway.

### 6.3 Negative-control fixtures

No fixture crate is committed under `cells/`. A committed mismatched crate would fail the real check, or need an exception, and would join the workspace.

| Fixture | Where | Proves | Runs in |
|---|---|---|---|
| `TierClassTests` in `scripts/test_cellos_sign.py` | Throwaway tree, as `PolicyTests.build` does (`scripts/test_cellos_sign.py:102-116`): `cells/x` with an `unsafe extern "C"` call, `abi::declare_manifest!(block_io = false, network = false, spawn = false);`, and `c-ffi` `[[file]]`/`[[crate]]` entries. | `policy.check` reports a `tier-class` violation. Variants: `tier = abi::manifest::PROTECTION_CLASS_FFI` passes; a `[[tier_exception]]` passes; `legacy-raw-ptr` fails the same way; no manifest at all (the compositor case) fails; `class = "c_ffi"` raises `AllowlistError`; an unused exception fails under strict. | `f1-admission`, step "Unit-test the gate itself" (`.github/workflows/ci.yml:2242-2243`) |
| `SignRefusesTierMismatch` | Same tree; `cli.run_sign` with `toolchain.check` and `signing.sign_and_verify` mocked, as `SignPathStrictnessTests` does (`scripts/test_cellos_sign.py:389-418`), but with the **real** `policy.check`. | Exit `EXIT_POLICY` and no call to `sign_and_verify`: "signing a crate with a `c-ffi` entry and a Tier-1 manifest fails". | same |
| `check-tier-class.py --negative-control` | Real tree plus one synthetic in-memory crate record (class `c-ffi`, manifest LEGACY); nothing written. | The real join rejects it. Same shape as `scripts/check-kernel-residents.py:10-11,54-60`. | `f1-admission`, new step |
| `TcbBaselineTests` | Throwaway tree with a `libs/ostd`-shaped package in the closure file and a baseline row. | One added `unsafe` fails; one removed fails under strict; a new file with `unsafe` fails. | `f1-admission` |
| `cellos-sign` over a real-tree overlay | `tcb.py --negative-control`: reduce `libs/ostd/src/syscall.rs`, append one `unsafe {}` in memory, count. | The real baseline rejects it ("fails on a new unlisted `unsafe` block there"). | `f1-admission` |

### 6.4 Commits (inside steps 6 and 7)

1. Class enum in `allowlist.py`; fix the four test fixtures. No behaviour change on the real tree (all 140 entries use the six classes).
2. `tier_class.py` + `[[tier_exception]]` schema + three exception entries + tests + `check-tier-class.py`. CI stays green because the three exceptions land with the gate.
3. `gen-tier1-closure.py` + generated file + drift step in `f1-admission`.
4. `tcb.py` + `tcb-unsafe-baseline.toml` (54 rows) + tests + negative control.
5. Spec 16, Spec 13, Spec 18 §2.1 and the two docstrings (§5).

## 7. Risks

- **All-or-nothing refusal.** A missing exception or a stale baseline row stops every image recipe, not only the offending cell (§1.1 "Scope of the verdict"). This is today's F1 behaviour; it now covers more ground.
- **Strict downward ratchet at sign time.** A developer who deletes an `unsafe` in `ostd` cannot sign a local image until the ceiling is lowered in the same change. That is the intent, but it is new friction.
- **`libs/nic-driver` moves sites across the boundary.** The nine NIC cells hold 102 allowlisted tokens today, 24 of them in eight identical `dma_layout.rs` copies (3 each). If the shared library keeps the `unsafe`, cell tokens fall and closure tokens rise (for example −24 / +3). A ceiling taken at `539401b08f` would refuse the library. See question 2.
- **Registry drift.** `cargo update` can bring a new `name@version` with `unsafe` and fail CI for a change that touched no code.
- **Manifest parser false positives.** A cell that sets `tier` through a constant is read as Tier 1 and refused until it spells the class literally.
- **Feature unification.** The closure is a workspace-wide superset (§4.1). Some ceilings may cover code a given image never links.
- **Exceptions are not authority.** An exception lets the pipeline sign a Tier-1 cell that holds C-ABI or raw-pointer `unsafe`; it changes nothing in the kernel. `service-net` keeps network authority and a `TRUSTED_CORE` PKU floor on x86 (§2).
- **Moving the two shipped services to Tier 2 is not free.** Without `native-domains` + `cellos_wide` a signed FFI/UNTRUSTED cell is refused (`kernel/src/task/launch.rs:462-473`), so a reclassification could stop the network or desktop on some configurations.

## 8. Open questions for the owner

1. **Registry crates in "may only go down".** Per-site ceilings cannot hold across upstream version bumps. Proposed: registry crates are pinned at `name@version` granularity, and a new or bumped crate with `unsafe` needs a reviewed diff of the generated closure file; the downward ratchet applies to in-tree crates only. Alternative: exclude registry crates from the baseline and keep them under `cargo-deny`.
2. **Baseline point and moved sites.** Today's sites (`539401b08f`) or the tree after `libs/nic-driver` lands? Proposed: the monotonic quantity is the Tier-1 total — allowlisted `unsafe` in cells with Tier-1 manifests (269 tokens today) plus the closure (564) — so code moved from nine cells into one library passes when the total falls. Alternative: `libs/nic-driver` must be `#![forbid(unsafe_code)]` and its MMIO/DMA goes through `ostd`.
3. **`app-c-math-smoke`.** It is never signed or shipped. Proposed: declare `PROTECTION_CLASS_FFI` (one line in the cell; no `libs/abi` change) instead of an exception. That takes `posix-shim`'s 160 tokens out of the Tier-1 closure. The approval treats manifest changes as a stop point, so this needs a yes.
4. **Attribute sites.** Count `#[no_mangle]`/`#[export_name]`/`#[link_section]` in the closure baseline (159 sites at HEAD, 136 of them in `posix-shim`)? Proposed: yes, as a second ceiling per row.

No `libs/abi` or `libs/types` change is required by anything in this census.

## Appendix A — linked registry crates with `unsafe` (method §4.1)

| Crate version | `unsafe` tokens | files | Tier-1 cells | root attr |
|---|---|---|---|---|
| `portable-atomic` 1.14.0 | 642 | 39 | 1 | — |
| `zerocopy` 0.8.55 | 486 | 18 | 4 | — |
| `hashbrown` 0.17.1 | 454 | 18 | 1 | — |
| `memchr` 2.8.3 | 333 | 20 | 114 | — |
| `hashbrown` 0.16.1 | 329 | 18 | 1 | — |
| `hashbrown` 0.14.5 | 305 | 16 | 1 | — |
| `hashbrown` 0.15.5 | 303 | 18 | 114 | — |
| `lock_api` 0.4.14 | 297 | 4 | 114 | — |
| `heapless` 0.9.3 | 283 | 21 | 3 | — |
| `time` 0.3.54 | 269 | 32 | 1 | — |
| `aho-corasick` 1.1.4 | 227 | 10 | 4 | — |
| `allocator-api2` 0.2.21 | 214 | 12 | 114 | — |
| `heapless` 0.8.0 | 206 | 15 | 114 | — |
| `ppv-lite86` 0.2.21 | 168 | 5 | 2 | — |
| `compact_str` 0.9.1 | 159 | 15 | 1 | — |
| `wasmi` 1.1.0 | 123 | 27 | 2 | — |
| `spin` 0.9.9 | 117 | 7 | 114 | — |
| `zerocopy` 0.7.35 | 110 | 5 | 5 | — |
| `aes` 0.8.4 | 110 | 16 | 1 | — |
| `ascii` 1.1.0 | 91 | 4 | 2 | — |
| `linked_list_allocator` 0.10.6 | 90 | 3 | 114 | — |
| `slotmap` 1.1.1 | 89 | 6 | 1 | — |
| `lru` 0.16.4 | 82 | 1 | 1 | — |
| `zune-jpeg` 0.5.15 | 79 | 10 | 1 | — |
| `httparse` 1.10.1 | 78 | 8 | 114 | — |
| `generic-array` 0.14.7 | 78 | 7 | 7 | — |
| `smallvec` 1.15.2 | 72 | 2 | 1 | — |
| `zmij` 1.0.23 | 70 | 2 | 114 | — |
| `deranged` 0.5.8 | 68 | 3 | 1 | — |
| `chacha20` 0.10.2 | 66 | 6 | 2 | — |
| `aes` 0.9.3 | 60 | 11 | 3 | — |
| `libm` 0.2.16 | 56 | 9 | 114 | — |
| `regex-automata` 0.4.16 | 55 | 10 | 4 | — |
| `constant_time_eq` 0.4.2 | 54 | 5 | 1 | — |
| `castaway` 0.2.4 | 53 | 3 | 1 | — |
| `arrayvec` 0.7.8 | 52 | 4 | 2 | — |
| `toml_parser` 1.1.5+spec-1.1.0 | 51 | 5 | 1 | — |
| `poly1305` 0.8.0 | 50 | 4 | 2 | — |
| `poly1305` 0.9.1 | 50 | 5 | 2 | — |
| `once_cell` 1.21.4 | 47 | 5 | 1 | — |
| `winnow` 0.7.15 | 46 | 9 | 1 | — |
| `winnow` 1.0.4 | 44 | 8 | 1 | — |
| `byteorder` 1.5.0 | 40 | 2 | 114 | — |
| `sha2` 0.11.1 | 39 | 14 | 3 | — |
| `bstr` 1.13.1 | 39 | 10 | 1 | — |
| `hybrid-array` 0.4.15 | 37 | 5 | 3 | — |
| `simd-adler32` 0.3.10 | 36 | 6 | 1 | — |
| `curve25519-dalek` 4.1.3 | 35 | 6 | 2 | — |
| `curve25519-dalek` 5.0.0 | 33 | 4 | 3 | — |
| `inout` 0.2.5 | 30 | 3 | 3 | — |
| `sha2` 0.10.9 | 29 | 8 | 6 | — |
| `defmt` 1.1.1 | 28 | 4 | 1 | — |
| `ryu` 1.0.23 | 27 | 10 | 1 | — |
| `blake2` 0.10.6 | 26 | 6 | 1 | — |
| `stable_deref_trait` 1.2.1 | 25 | 1 | 114 | — |
| `cmov` 0.5.4 | 25 | 5 | 3 | — |
| `polyval` 0.7.3 | 24 | 4 | 2 | — |
| `polyval` 0.6.2 | 23 | 3 | 1 | — |
| `chacha20` 0.9.1 | 23 | 4 | 3 | — |
| `inout` 0.1.4 | 22 | 3 | 4 | — |
| `block-buffer` 0.12.1 | 21 | 3 | 3 | — |
| `bitflags` 1.3.2 | 19 | 1 | 1 | — |
| `smoltcp` 0.11.0 | 18 | 4 | 1 | deny |
| `zeroize` 1.9.0 | 17 | 4 | 8 | — |
| `critical-section` 1.2.0 | 17 | 3 | 1 | — |
| `no_std_io` 0.6.0 | 15 | 3 | 1 | — |
| `serde_json` 1.0.151 | 13 | 4 | 114 | — |
| `itoa` 1.0.18 | 13 | 1 | 114 | — |
| `zune-png` 0.5.2 | 12 | 4 | 1 | — |
| `postcard` 1.1.3 | 11 | 2 | 116 | — |
| `cpufeatures` 0.3.1 | 11 | 3 | 3 | — |
| `wasmi_core` 1.1.0 | 11 | 2 | 2 | — |
| `foldhash` 0.2.0 | 10 | 3 | 1 | — |
| `powerfmt` 0.2.0 | 10 | 2 | 1 | — |
| `itertools` 0.14.0 | 10 | 2 | 1 | — |
| `hash32` 0.3.1 | 9 | 1 | 114 | — |
| `cpufeatures` 0.2.17 | 9 | 3 | 7 | — |
| `ahash` 0.8.12 | 7 | 1 | 1 | — |
| `taffy` 0.14.0 | 7 | 3 | 1 | deny |
| `log` 0.4.33 | 6 | 1 | 8 | — |
| `volatile` 0.4.6 | 6 | 1 | 3 | — |
| `unicode-normalization` 0.1.25 | 5 | 1 | 114 | deny |
| `ab_glyph_rasterizer` 0.1.10 | 5 | 1 | 114 | — |
| `owned_ttf_parser` 0.25.1 | 5 | 1 | 114 | — |
| `wasmi_collections` 1.1.0 | 5 | 3 | 2 | — |
| `der` 0.8.1 | 4 | 4 | 1 | deny |
| `block-buffer` 0.10.4 | 4 | 2 | 6 | — |
| `base64ct` 1.8.3 | 4 | 1 | 3 | — |
| `base16ct` 0.2.0 | 4 | 2 | 2 | — |
| `crypto-bigint` 0.5.5 | 4 | 2 | 2 | deny |
| `foldhash` 0.1.5 | 3 | 1 | 114 | — |
| `scopeguard` 1.2.0 | 3 | 1 | 114 | — |
| `serde_core` 1.0.229 | 2 | 2 | 116 | — |
| `rand_core` 0.6.4 | 2 | 1 | 7 | — |
| `bitflags` 2.13.1 | 2 | 1 | 12 | — |
| `serde` 1.0.229 | 2 | 2 | 116 | — |
| `spinning_top` 0.2.5 | 2 | 1 | 114 | — |
| `cipher` 0.4.4 | 2 | 2 | 4 | — |
| `subtle` 2.6.1 | 2 | 1 | 7 | — |
| `either` 1.19.0 | 2 | 1 | 1 | — |
| `pem-rfc7468` 1.0.0 | 1 | 1 | 2 | deny |
| `wasmparser` 0.239.0 | 1 | 1 | 2 | — |
| `wasmi_ir` 1.1.0 | 1 | 1 | 2 | — |
| `universal-hash` 0.5.1 | 1 | 1 | 3 | deny |
| `static_assertions` 1.1.0 | 1 | 1 | 1 | — |
| `embedded-tls` 0.19.0 | 1 | 1 | 1 | — |
| `ed25519-compact` 2.3.1 | 1 | 1 | 1 | — |
| `rand_chacha` 0.3.1 | 1 | 1 | 2 | — |
| `const-oid` 0.10.2 | 1 | 1 | 1 | deny |

"Tier-1 cells" is the number of the 122 Tier-1 roots whose linked closure contains that `name@version` on at least one target.
