# Lock-handoff requests — x86 Tier-2 runtime slices R1, R2, R3, R4, R6 (R5 lane-dependent)

**Status: approved by the owner on 2026-11-04 ("duyệt"), with H-1…H-6 as proposed ([record](handoff-and-xsave-approval.md)).** Written 2026-11-04 (phase R0 step 4).
- Basis: owner decision D-R2 ([approval](owner-decisions-approval.md)), recorded in the lock inventory as "R1–R4 and R6 beside the WIP slice, on new or user-space files only, one handoff request per slice. R5 … needs the lane" (`.agents/cell-contract-0024-locks.md:171`).
- The R1, R2, R3, R4 and R6 handoffs are effective now for exactly their §3 file sets. R3 still needs the large-memory implementation for its runtime witness. R5 waits for the lane.
- Every `file:line` was read in this session. Line numbers in `cell-contract-0024-locks.md` are as of Main's 2026-11-04 edits. `[INFERENCE]` marks unread claims.

## 1. Inputs checked

**S1 file set** (held, Intel WIP lane): CP1 §7 (`.agents/261103-2218-x86-platform-breadth/cp1-pcie-irq-power-abi.md:122-138`) plus the phase-05 extension (`cell-contract-0024-locks.md:159`). Paths that matter here:
- `hal/arch/x86/**`;
- `kernel/src/main.rs` (x86 init);
- `kernel/src/loader/boot_ceiling.rs` + selftest;
- `kernel/src/loader/launch_profile/{profiles,tests}.rs`;
- `kernel/src/task/cap.rs`;
- `config/services.toml`;
- `scripts/assemble-x86-image.py`, `scripts/sign-policy.py`;
- `scripts/qemu-x86_64-test.sh`, `scripts/x86/{make-iso-ci.sh,make_iso.py,qemu-domain-test.sh}`;
- `tests/integration/src/lib.rs` and the named x86 tests;
- `.github/workflows/ci.yml` (`qemu-x86_64-boot` job);
- `docs/abi-boundary-records.md`, `CHANGELOG.md`.

**Approved, not yet effective:** GPU DMA-export and large-memory slices (`cell-contract-0024-locks.md:163-171`). Their kernel and `libs/api` files are not touched by R1–R4 or R6.

**Working tree** (`git status --short`, this session):
- `tools/cellos-cc` is modified, and `meson/cellos-x86_64.ini` and `cmake/cellos-x86_64.cmake` are untracked: Main's R1 work in progress.
- `Cargo.toml`, `Cargo.lock`, `config/services.toml`, `kernel/src/loader/launch_profile/{profiles,tests}.rs`, `kernel/src/loader/boot_ceiling.rs` and `docs/project-changelog.md` are modified by other sessions.
- No file under `third_party/mlibc/`, `libs/mlibc-shim/`, `docs/guides/`, `docs/posix-ledger.md`, `docs/specs/`, `cells/tests/` or `kernel/src/loader/launch_profile/targets.rs` is dirty.

**Related lock rows**, ruled compatible for every slice below. The controlling area is P07, "Optional POSIX facade and runtime adaptation" (`cell-contract-0024-locks.md:102`).
- `260821-0642-app-tiers-completion` (`:200`, "admission, runtime, native-domain …"): its runtime phase excludes mlibc (`.agents/260821-0642-app-tiers-completion/phase-06-tier1-rust-std-pal.md:14`). Its final gate only reads `tier1b-c-zig.md` (`phase-09-final-completion-gate.md:22`). Compatible: no admission, manifest or promotion change.
- `260722-0917-g4-full-std-tier1` (`:192`, std/PAL): no Rust std/PAL change. Compatible.
- `260615-1325-vicell-app-sdk-v1` (`:182`) and `260827-1004-hardware-independent-roadmap` (`:207`): no SDK, ServiceRef or contract-scheduling change. Compatible.
- Evidence rows `260922-1549-cell-native-portability-program` (`:252`: TLS/futex are P03/P07 evidence) and `260902-posix-cwd-path-slice` (`:245`): reused as primitives, not redesigned.

## 2. Cross-slice findings and owner decisions still needed

| # | Finding | Effect | Proposal |
|---|---|---|---|
| H-1 | **The launch edge for Tier-2 witnesses is in S1-held kernel files.** Init launches Tier-2 fixtures only through the reviewed list in `kernel/src/loader/launch_profile/profiles.rs:39-54` (`/bin/tier2-smoke`, `/bin/cpp-smoke`, …). The same names are mirrored in `targets.rs:29-75` and `tests.rs:986-988` (also `tests.rs:293-294`, `:1286`). `profiles.rs`/`tests.rs` are in S1's extension (`cell-contract-0024-locks.md:159`) and dirty. `targets.rs` is clean but is a kernel file, outside "new or user-space files only" (`:171`) | Each witness Cell (`x86-fpu-preserve`, mlibc-smoke on x86, `mlibc-posix-x86`, `cpp-hosted-smoke`, `drm-lite-x86`) needs a reviewed row `[INFERENCE: needed for an init or shell launch, as for every other test Cell]` | **Owner decision.** (a) The S1 integrating session lands the rows as append-only `CapSet::EMPTY` Tier-2 fixture rows under its extension, supplied verbatim by each slice; or (b) the rows wait until S1 releases these files. Recommendation: (a). No other kernel edit is requested. |
| H-2 | `Cargo.toml` and `Cargo.lock` are shared and dirty | New test-Cell members | Append-only member lines, landed by the integrating owner. This is the same exception the iGPU plan recorded (`.agents/261104-0801-x86-igpu-compute/plan.md:46`) |
| H-3 | `CHANGELOG.md` is S1-held (`cp1-pcie-irq-power-abi.md:138`); `docs/project-changelog.md` is dirty | Changelog lines | Each slice hands its changelog text to Main (integrating owner) instead of editing |
| H-4 | The test harness is S1-held: `tests/integration/src/lib.rs`, the x86 QEMU scripts, the CI boot job | Witness runners | New standalone scripts only. `scripts/x86/qemu-domain-test.sh` is invoked read-only. `scripts/build-x86_64-domain-test-ci.sh` (not S1-held; it builds the `test-hooks` + `native-domains` domain lane, `:1-20`) is used as the pattern, not edited. No CI job in these slices |
| H-5 | Shared files across R slices: `third_party/mlibc/sysdeps/vicell/**`, `docs/posix-ledger.md`, `docs/guides/tier1b-c-zig.md` | Edit collisions | Strict slice order R2 → R3 → {R4, R6}. One slice open on these files at a time, with one integrating owner |
| H-6 | R3 planned to add `--build-id` in `libs/cell-build` | It would change the link of every Cell | Deviation proposal: emit `-Wl,--build-id=sha1` from each mlibc Cell's own `build.rs` (`cargo:rustc-link-arg`). `libs/cell-build` leaves R3's file set |

## 3. Requests per slice

### R1 — x86 C/C++ toolchain and the SSE2 float boundary ([phase](phase-01-x86-toolchain-and-float-abi.md)); integrating owner: Main

- **Create:**
  - `meson/cellos-x86_64.ini`, `cmake/cellos-x86_64.cmake` (already started, untracked);
  - `scripts/check-float-ffi-boundary.py` + its fixtures and tests;
  - `cells/tests/x86-fpu-preserve/**`;
  - the shared Tier-2 runtime witness lane: `scripts/build-x86_64-tier2-runtime-test.sh` and a runner `scripts/qemu-x86-tier2-runtime-test.sh`. The names are proposals, and R1 fixes them. Later slices add their Cells to this lane.
- **Modify:** `tools/cellos-cc` (x86 row); `docs/guides/tier1b-c-zig.md` (x86 float rule section only).
- **Shared (H-1, H-2, H-3):** launch row `/bin/x86-fpu-preserve`; workspace member line; changelog text.
- **Not touched:** `targets/*.json`, `libs/api/**`, `hal/**`, `kernel/**` (except H-1), `cells/services/ocel-quickjs/**` (the float-ABI defect is reported, not fixed here).
- **Check:** no overlap with S1 beyond H-1…H-4. `tools/`, `meson/`, `cmake/` and `scripts/check-*` are outside S1's set.

### R2 — mlibc x86_64 base ([phase](phase-02-mlibc-x86-64-base.md))

- **Create:**
  - `third_party/mlibc/sysdeps/vicell/crt-x86_64/crt1.S`;
  - `third_party/mlibc/sysdeps/vicell/include/mlibc/sysdeps.hpp`;
  - the x86 syscall header under `third_party/mlibc/sysdeps/vicell/include/`.
- **Modify:**
  - `third_party/mlibc/sysdeps/vicell/{meson.build,generic/*.cpp}` (port to `Sysdeps<Tag>`, all three architectures);
  - `scripts/build-mlibc.sh` (pin + x86 row; today it clones unpinned, `:39`);
  - `scripts/setup-mlibc.ps1`;
  - `libs/mlibc-shim/build.rs` (x86 build directory; today only aarch64 is special-cased, `:24-28`);
  - `third_party/README.md` (pin);
  - `cells/tests/mlibc-smoke/**` (x86 markers, `__thread`, `errno`);
  - `docs/posix-ledger.md` (Tier-B rows `:261-294`);
  - `docs/guides/tier1b-c-zig.md` (`:483-485` Level-B note).
- **Shared:** a launch row for mlibc-smoke on x86 (H-1) and a lane entry in R1's witness script.
- **Not touched:** `libs/api/**`. If the `mlibc` feature needs an x86 change, the slice stops for a Law-1 CP1.
- **Check:** clean in the working tree; not in S1's set. The RV64/AArch64 Tier-B regression run is part of the slice, because the sysdep port changes all three architectures.

### R3 — mlibc `posix` ([phase](phase-03-mlibc-posix-enablement.md))

- **Modify:** `third_party/mlibc/sysdeps/vicell/{meson.build,generic/*.cpp,include/**}`; `docs/posix-ledger.md`; `docs/guides/tier1b-c-zig.md` (Level-B section).
- **Create:** `cells/tests/mlibc-posix-x86/**` (its `build.rs` emits `--build-id`, per H-6).
- **Shared:** launch row `/bin/mlibc-posix-x86` (H-1), lane entry, member line.
- **Split:**
  - R3a (threads, futex, time, environment, ELF) can start after R2.
  - R3b (memory, memfd) waits for the large-memory slice's CP2. That slice is approved but not yet effective (`cell-contract-0024-locks.md:169`), and it delivers `AnonMemory = 440` in `libs/api` and the kernel, not here.
- **Not touched:** `libs/api/**`, `kernel/**`, `libs/cell-build/**` (H-6).
- **Check:** no S1 overlap. P03 evidence row `260922-1549` (`:252`) is reused, not reopened.

### R4 — hosted C++ runtime ([phase](phase-04-hosted-cxx-runtime.md); [ADR-0034](../../docs/decisions/0034-hosted-cxx-runtime-profile.md))

- **Create:** `scripts/build-llvm-runtimes.sh` (pinned llvm-project tag); `third_party/llvm-runtimes/README.md` (pin, licence, NOTICE text); `cells/tests/cpp-hosted-smoke/**`.
- **Modify, at R4 close, as listed in ADR-0034 Consequences:**
  - `docs/guides/tier1b-c-zig.md` (`cpp-hosted` section; fix `:288-290`);
  - `docs/specs/05-application.md` §3.2 (`:186-214`);
  - `docs/specs/18-cell-trust-tiers.md:58-60`;
  - `docs/posix-ledger.md` (Tier-B C++ ABI rows);
  - `NOTICE`, only when an image ships the runtime.
- **Shared:** launch row `/bin/cpp-hosted-smoke` (H-1), lane entry, member line.
- **Not touched:** `cells/tests/cpp-smoke/**`, `scripts/qemu-cpp-smoke.sh` (the freestanding profile is unchanged), `libs/api/**`, `kernel/**`.
- **Check:** the `NOTICE` and spec files are not in S1's set and are clean.

### R6 — libdrm-lite and fd provider ([phase](phase-06-libdrm-lite-and-fd-provider.md))

- **Create:** `third_party/libdrm/**` (pinned MIT subset + `README.md`); `libs/drm-cellos/**`; `cells/tests/drm-lite-x86/**`.
- **Modify:** `third_party/mlibc/sysdeps/vicell/{generic/*.cpp,include/**}` (fd provider registry, `Ioctl`, fd-aware `VmMap`; after R3); `docs/posix-ledger.md`.
- **Shared:**
  - launch row `/bin/drm-lite-x86` (H-1), lane entry, member lines (`libs/drm-cellos`, test Cell);
  - the provenance row in `.agents/261104-0801-x86-igpu-compute/provenance.md`, added by the integrating session.
- **Not touched:** Mesa, `libs/api/**`, `kernel/**`, the G3 transport (`cells/runtimes/mesa-anv/**`, owned by iGPU G3).
- **Check:** no S1 overlap. The interface is reviewed with the G3 owner before close.

### R5 — XSAVE/AVX (lane-dependent; not requested now)

The file set is [cp1-x86-xsave-user-state.md](cp1-x86-xsave-user-state.md) §7. It needs:
- the owner's CP1 check (X-1…X-7);
- the Intel WIP lane;
- S1's release of `hal/arch/x86/**` (`cp1-pcie-irq-power-abi.md:130`; `cell-contract-0024-locks.md:171`).

It queues with the GPU DMA-export and large-memory slices in an order the owner sets. Its launch row follows H-1.

## 4. Constraints common to every request

- Every CELL-CONTRACT-0024 whole-plan relation stays.
- No `libs/api`/`libs/types` edit, no syscall, no kernel or HAL edit beyond H-1 rows.
- Evidence ceiling: host plus QEMU TCG and QEMU/KVM (local) in the `test-hooks` domain lane.
- Each slice is one revertable commit set. Each closes with a report to Main, which holds the plan, portfolio, lock-inventory and ABI-record edits.
