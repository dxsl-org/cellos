# Phase 04 — capability ceiling slice: `CapSet::ALL` → per-path `boot_ceiling`

Date: 2026-07-30 · Branch: `feat/wx-post-reloc-and-f1-signing` · Scope: requirements 1–5 only
(broker, shell manifest, POLICY.BIN re-bake, `with_path_caps` fold, raw-grant deletion: not done).

## 1. Path enumeration (derived, not guessed)

### 1a. Everything `init` spawns — `cells/tools/init/src/main.rs`

| Site | Path | Kind |
|------|------|------|
| `:144` | `/bin/block` | pre-VFS block driver probe |
| `:145`, `:174` | `/bin/nvme` | pre-VFS + retry after VFS is up |
| `:90-100` `paths[0..9]` | `/bin/vfs`, `/bin/config`, `/bin/input`, `/bin/net`, `/bin/compositor`, `/bin/silo`, `/bin/net-broker`, `/bin/supervisor`, `/bin/shell` | supervised table (`NSVC = 9`) |
| `:166`, `:167` | `/bin/virtio-net`, `/bin/e1000` | NIC drivers, spawned just before `/bin/net` |
| `:185` | `/bin/virtio-gpu` | GPU driver, before `/bin/compositor` |
| `:234` | `/bin/fb-console` | optional |
| `:240` | `/bin/hypervisor` | optional, AArch64 + virt builds |
| `:251-253` | `/bin/silo-test`, `/bin/vfs-test`, `/bin/srv-test` | CI-image-only |
| `:268` | `/bin/shell` | `paths[8]`, spawned last |
| `:348` | `paths[i]` | restart loop — same 9 paths, no new ones |

### 1b. Kernel-initiated (`Spawner::Root`) — the only spawns the table actually binds today

| Site | Path |
|------|------|
| `kernel/src/main.rs:680` | `/bin/platform` |
| `kernel/src/main.rs:708` | `/bin/init` (direct TCB write, not a path spawn) |
| `kernel/src/loader/elf_tests.rs:66,72,79,87` | `""`, `bin/shell`, `"/"*300`, `/bin/nonexistent-elf-for-test` — all rejected before the cap grant, none reach `spawn_gated` |

`kernel/src/cell/hotswap.rs:419` uses `Spawner::Ceiling`; every `syscall.rs` route uses
`Spawner::User`. So `Spawner::Root` = `/bin/platform` plus the four format tests.

### 1c. Paths matched in `with_path_caps` / `legacy_path_caps`

- `with_path_caps` (`cap.rs:259-278`): `pcie_driver` ← `/bin/nvme`, `/bin/e1000`,
  `/bin/virtio-net`, `/bin/block`, `/bin/input`, `/bin/virtio-gpu`; `platform` ←
  `/bin/platform`; `supervisor` ← `/bin/supervisor`.
- `legacy_path_caps` (`loader.rs:59-74`, manifest-absent fallback): `/bin/vfs` → `block_io` +
  `0b11`; `/bin/net` → `network`; `/bin/shell`, `/bin/init` → `spawn`. Of the boot cells only
  `/bin/compositor` has no `declare_manifest!`, and it matches none of these → EMPTY.

### 1d. Cross-check against the images

- `gen_disk.ps1:393-534` ships `/bin/init`, `/bin/shell`, `/bin/vfs`, `/bin/config`,
  `/bin/platform`, `/bin/block`, `/bin/nvme`, `/bin/e1000`, `/bin/virtio-net`,
  `/bin/virtio-gpu`, `/bin/input`, `/bin/net`, `/bin/net-broker`, `/bin/supervisor`,
  `/bin/compositor`, `/bin/fb-console` + demos/tools.
- `gen_disk.ps1` does **not** ship `/bin/silo`, `/bin/hypervisor`, `/bin/silo-test`,
  `/bin/vfs-test`, `/bin/srv-test`. `/bin/hypervisor` comes from
  `scripts/make-hypervisor-fs*.sh`; the other four are CI-image-only. init's spawn of a missing
  path just logs "cell not found — skipping", so a row for them costs nothing and covers the CI
  images. All five have rows.
- Cross-checked against `scripts/sign-policy.py:61-97` `DEV_POLICY` (the phase-03 enumeration).
  The two tables agree cap-for-cap on every path both list. Differences, all deliberate:
  `/bin/silo` (declares `hypervisor = true`, has a `boot_ceiling` row, has **no** `DEV_POLICY`
  entry — a phase-03 gap, flagged not fixed); `/bin/init` (`DEV_POLICY` grants `platform=1`,
  `boot_ceiling` does not — see the row note); demo/tool paths (`periph-demo`, `robot-demo`,
  `bench`, …) which are shell-spawned, never Root-spawned, so they are `DEV_POLICY`-only.

## 2. The table — `kernel/src/loader/boot_ceiling.rs`

`lookup(path) -> Option<CapSet>` (unknown → `None`); `boot_ceiling(path)` = `unwrap_or(EMPTY)`.
`lookup` returning `Option` is what lets the refusal log say "no row" instead of "row grants
nothing" — identical CapSets, opposite fixes.

| Path(s) | Caps | Justification |
|---|---|---|
| `/bin/init` | `block_io`, `network`, `spawn`, `hypervisor`, `mmio=GPIO\|UART`, `regions=0b1111`, `pcie_driver`, `supervisor`; **`platform: false`** | Root authority; each bit is named in-code with the child it is delegated to. `platform` dropped: the Platform Cell is kernel-spawned, and `CapSet::apply_to` never writes `platform_cap` (the singleton latch owns it), so init never actually held it and could never delegate it — dropping it is provably behaviour-neutral. |
| `/bin/platform` | `platform` | Manifest declares nothing; `with_path_caps` is the request signal. `try_grant_platform` still enforces one holder. |
| `/bin/vfs` | `block_io`, `regions=0b1111` | Manifest `block_io + part_data + part_lfs` → `0b111`; the 4th bit is the cell-store region (requirement 5 — ceiling runs before policy). |
| `/bin/net`, `/bin/net-broker` | `network` | Both `declare_manifest!(network = true)`. |
| `/bin/shell` | `spawn`, `mmio=GPIO\|UART` | Manifest `spawn + gpio + uart`; still the ceiling for the peripheral demos it launches (removing gpio/uart is req 5 of the phase, out of scope here). |
| `/bin/supervisor` | `spawn`, `supervisor` | Manifest `spawn = true` + `with_path_caps`. |
| `/bin/silo`, `/bin/hypervisor` | `hypervisor` | Both `declare_manifest!(hypervisor = true)`. `from_manifest` additionally gates the bit on `has_h_ext/has_el2/has_x86_virt`, so a row cannot conjure it on non-virt hardware. |
| `/bin/block`, `/bin/nvme`, `/bin/e1000`, `/bin/virtio-net`, `/bin/virtio-gpu`, `/bin/input` | `pcie_driver` | Manifests declare nothing; the install path is the request signal. Each claims a BAR/MMIO range and authorises DMA. |
| `/bin/config`, `/bin/compositor`, `/bin/fb-console`, `/bin/silo-test`, `/bin/vfs-test`, `/bin/srv-test` | EMPTY | Pure IPC clients (`/bin/compositor` has no manifest at all → `legacy_path_caps` = EMPTY). Listed explicitly so a report can distinguish "needs nothing" from "row missing". |
| anything else | EMPTY (`lookup` → `None`) | Fail-closed. |

**Erring generous, noted:** (a) rows for `/bin/silo`, `/bin/hypervisor`, `/bin/silo-test`,
`/bin/vfs-test`, `/bin/srv-test` exist even though `gen_disk.ps1` ships none of them; (b)
`/bin/init` keeps `hypervisor` and `pcie_driver` even on targets that never use them, because
the row is arch-independent; (c) `/bin/shell` keeps `mmio=GPIO|UART`, matching today's manifest
rather than the phase's eventual target of 0.

## 3. Changes

| File | Change |
|---|---|
| `kernel/src/loader/boot_ceiling.rs` (new, 187 L) | table + `lookup` + `boot_ceiling` + `log_refusal` |
| `kernel/src/loader/boot_ceiling/selftest.rs` (new, 113 L) | boot self-test (returns `bool`, never panics) |
| `kernel/src/loader.rs` | `pub mod boot_ceiling;`; `Spawner::Root` now intersects `boot_ceiling(path)` and calls `log_refusal` on any narrowing; policy exemption scoped to `Spawner::Root if !policy::is_resolved()` |
| `kernel/src/policy.rs` | `MMIO_MASK` += `DEV_CAN\|DEV_ADC`; `REGION_MASK` `0b111` → `0b1111`; new `pub fn is_resolved()`; `log::warn!` mirror of the `CapNarrowedByPolicy` audit; four new `self_test` mask cases |
| `kernel/src/task/cap.rs` | `CapSet::ALL` re-documented as a **self-test-only** reference upper bound (no longer granted to any task); `block_regions` `0b111` → `0b1111` to match the widened domain |
| `kernel/src/main.rs` | init's direct cap write now `boot_ceiling("/bin/init")`; boot-ceiling self-test registered before the first Root spawn |

### Loud, diagnosable refusal

`boot_ceiling::log_refusal` fires at `error` level (survives the `LevelFilter::Warn` the kernel
drops to) and prints: whether the path has **no row** vs. a **narrowing row**; `requested`,
`ceiling`, `granted` in full; then one line per refused cap named exactly as the struct field to
add (`refused: pcie_driver`, `refused: mmio_devices 0b000010`, …). Nothing has to be re-derived
from source to act on it. `policy.rs` gained a parallel one-line warn with an inline bitmask
legend, because the audit ring needs a live cell to read — the one thing missing when policy
strips a boot cell's caps.

## 4. What actually binds now, and what does not

Binds: a `Spawner::Root` spawn of an unknown path gets `EMPTY` instead of the full manifest;
`/bin/platform` gets exactly `{platform}`; a Root spawn is policy-bound once the policy is
resolved; the policy parser now accepts the 4-bit region encoding and CAN/ADC.

Does **not** bind yet: init's own spawns. They go through `sys_spawn_from_path` →
`Spawner::User(init_tid)`, so their ceiling is `CapSet::of_task(init)`, not the table. The
`/bin/init` row therefore still has to cover every child, i.e. it is union-shaped, and init's
effective delegation authority is unchanged. Requirement 2 as written covers only
`Spawner::Root`, and closing the rest means routing the root authority's `User` spawns through
the table — precisely the "boot breaks at cell N" High risk. Flagged, not done.

## 5. Verification

| Command | Result |
|---|---|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | PASS |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | PASS |
| `cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | PASS (links) |
| `cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | PASS |
| `cargo fmt --all --check` | **FAIL — other agents' files only** (`cells/services/vfs/src/access.rs`, `libs/api/src/abi/caller_identity.rs`). `rustfmt --check` on all six files of this slice: clean. |

`cargo check` was validated with a deliberate `const _PROBE: u32 = "…";` in the new module and
confirmed to report it, so the fast exit-0 is real compilation, not a skip.

**Out-of-tree host harness** (`scratchpad/bcheck`, symlinks to the real sources — the boot
self-test is an `assert!`-free `bool`, but a wrong expectation would still log FAIL on every
boot, so it was executed rather than reasoned about): 9 tests pass, including the real
`cap.rs` `#[cfg(test)]` tests (which no in-tree command compiles) and `policy::self_test()`
against the real `ed25519` backend. Mutation-checked twice:

- collapse `boot_ceiling` to the union of all rows → `boot_ceiling_self_test_passes`,
  `table_is_not_a_union`, `unknown_path_is_empty` all FAIL, with all eight "union collapse"
  lines printed.
- revert `MMIO_MASK`/`REGION_MASK` to `0b111` → `policy_self_test_passes` FAILS.

Not verifiable here (no QEMU, no cross toolchain): boot, the 3-arch suite, the ARM64 peripheral
test, and every runtime claim about cell bring-up. Treat those as UNVERIFIED.

## 6. Follow-ups this slice deliberately left open

1. **Route the root authority's `User` spawns through the table** — the remaining half of the
   deprivilege (see §4). Needs a boot to shake out missing rows.
2. `scripts/sign-policy.py` still has `MMIO_MASK = 0b111` / `REGION_MASK = 0b111`. It is now
   stricter than the kernel (safe), but `build_body` will `sys.exit` when step 7d tries to bake
   `/bin/vfs = 0b1111`. Widen it before the re-bake.
3. `/bin/silo` declares `hypervisor = true` and has no `DEV_POLICY` entry → under
   `policy-required` it boots with no caps. Phase-03 gap.
4. `kernel/src/task/p_trust_selftest.rs:55` still uses `CapSet::ALL` (correctly — as the widest
   ceiling, to prove a request is not over-tightened). If `ALL` is ever deleted, that file needs
   editing; it was outside this slice's ownership.

---

**Status:** DONE_WITH_CONCERNS
**Summary:** `CapSet::ALL` is no longer any task's authority — boot authority now comes from a
per-path `boot_ceiling` table (unknown path → `EMPTY`), `Spawner::Root` is intersected against it
and policy-bound once the policy resolves, and `MMIO_MASK`/`REGION_MASK` are widened so CAN/ADC
and the 4-bit region encoding validate. The table binds `Spawner::Root` only; init's own spawns
still use `CapSet::of_task(init)`, so init's delegation authority is unchanged.
**Verification:** `cargo check` riscv64 + x86_64, `cargo build` aarch64, `cargo clippy -D warnings`
riscv64 — all PASS (check validated against a deliberate type error). `cargo fmt --all --check`
fails only on two files owned by concurrent phases; `rustfmt --check` on this slice's six files is
clean. Table and policy masks executed for real via an out-of-tree host harness: 9/9 pass, both
mutation-checked. Boot, 3-arch suite, ARM64 peripheral test: UNVERIFIED (no QEMU/cross toolchain).
**Concerns/Blockers:**
- **Confirmed per-path, not a union.** `lookup` is a `match` on the path returning that path's own
  caps; `selftest.rs` pins eight rows that each *lack* a cap another row holds, and the harness
  additionally asserts no row equals the union of all rows. Collapsing the table to a union makes
  three harness tests fail with all eight "union collapse" lines — verified by mutation.
- **Least confident: `/bin/init` `hypervisor`.** If a `/bin/silo` or `/bin/hypervisor` build ever
  needs a cap I did not list, that cell loses it. Blast radius is small (silo = key isolation,
  hypervisor = optional guest) and the refusal path logs the exact missing field. Cannot verify
  without booting an AArch64 virt image.
- **Least confident and highest impact: requirement 3 applied to `/bin/platform`.**
  `policy::load_from_vifs1()` (`main.rs:563`) runs before the Root spawn at `main.rs:680`, so the
  Platform Cell is now policy-bound. Default build (POLICY.BIN absent → `NoEntry`,
  dev-permissive) is unchanged. But a `policy-required` build with no blob, or any build where the
  blob is `Invalid`, strips its `platform` cap → no PCIe ECAM scan → nvme/e1000 find no device →
  on x86_64 no block driver → VFS cannot mount. I did **not** add `/bin/platform` to
  `is_trusted_core` (widening the trusted core is a security decision, not mine to take
  unilaterally). Phase-03's `DEV_POLICY` already lists `/bin/platform` with `platform=1`, so a
  properly-provisioned fleet is fine; an unprovisioned `policy-required` image is not.
- **The `/bin/init` row is union-shaped by construction.** Not a defect in the table — a
  consequence of scoping requirement 2 to `Spawner::Root` while init spawns via `Spawner::User`.
  Reported plainly rather than papered over: the delegation half of "init is not root" is still
  open (§6.1).
- `CapSet::ALL` still exists, test-only, because `kernel/src/task/p_trust_selftest.rs` uses it and
  that file is outside this slice's file ownership.
