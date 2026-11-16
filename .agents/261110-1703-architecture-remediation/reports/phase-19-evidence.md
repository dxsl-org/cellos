# Phase 19 evidence: spec reconciliation, hygiene and closure

**Date:** 2026-11-16 · **Phase:** [phase-19-closure.md](../phase-19-closure.md) · **Handoff:** [S8c approval](../s8c-handoff-approval.md) · **Closure matrix:** [closure-matrix.md](closure-matrix.md)

## 1. Success criteria

| Criterion | Result |
|---|---|
| Every Critical and High row in the closure matrix is `closed` with an evidence link, or `residual` with the owner's written acceptance; no row is `closed` on prose alone | Critical 2/2 closed. High: 9 closed, 5 residual (H-2, H-5, H-8, H-13, H-14) accepted in writing by the owner on 2026-11-16 (`ea771d0131`). Every `closed` row cites an evidence file or commit and a re-run of the review's own check (`8425d956e3`, `9858c4bec0`). |
| `check-spec-anchors.py` and `check-plan-status.py` exit 0 | Both exit 0 on the closing head (§4). |
| Review re-check numbers recorded beside the originals | Closure matrix § Re-check numbers: `target_arch` outside `kernel/src/arch/` 1,807 → 692 (ceiling 692); `sys_send(` in services and drivers 67 → 0; kernel `"/bin/…"` literals 149 → 152; tracked files over 1 MB 17 → 19; `libs/ostd` `unsafe` lines 231 → 225; allowlist entries due before 2027-03-01 91 → 0. |
| Checkpoint: all `pr` and `nightly` lanes green on the closing commit; independent end-to-end review recorded | Not fully met. Every `pr` and `nightly` lane passes on the closing commits except `c2c-broker-oracle` (N-8) and six `bootsuite` tests (N-9). Both fail the same way on `1d13abd68e`, before Phase 18, and their closure needs the owner's written acceptance (§6). The end-to-end review is recorded in §5. |

## 2. What changed

| Step | Result | Commits |
|---|---|---|
| 1. Closure matrix | One row per review finding (2 Critical, 14 High, 37 Medium clauses, 8 Low) plus 9 new findings; each check re-run on the review's base and on today's tree | `d22c9694ab`, `efc68c4052`, `8425d956e3`, `9858c4bec0`, `4aff7facc9`, `80e925b9b7` |
| 2. Spec sweep | Specs 00, 02, 03, 04, 05, 07, 09, 10, 11, 13, 15, 16, 17, 18, 19, 21, 22, 23 fixed to match code; dated "historical (2026-11-16)" banners only where an earlier phase deliberately changed behaviour. `scripts/generate-arch-status.py` writes `docs/generated/arch-status.md` from probes of the tree; Specs 02/18/19/22 link it; CI runs its `--check` | `e27189e855`, `9c0d5db8dc`, `db9b0aa38a`, `cef2e41738`, `ecc8972d92`..`4222b57ce7`, `6a2f6d3271`..`1bd612c89e`, `bb74b7a810`, `93a4c4586b`, `cddb03faa7` |
| 3. Spec numbering | `00-fork.md` → `27-fork.md`, `14-viui.md` → `28-viui.md`; links, `llms.txt`, anchors fixed | `e94bc96802`, `478d2dbb12` |
| 4. Residue | Nine root junk files and the stray compositor file; dead `ViGlobalMemoryManager`, `spawn_from_file`, `memory::tests`; stale `write-phase19-docs.py` and the dead `docs.yml` step. Each deletion followed a reference grep | `c4a2040950`, `e8cb852c80`, `f25c02f7c8`, `32e5a942f8` |
| 5. Contributor docs | `Dockerfile` builds Cellos; `.dockerignore` (context 5 GB → 196 MB); target names (`aarch64-unknown-none-softfloat`, host integration target); `CONTRIBUTING.md`; false `mod.rs` ban removed from six docs; `lanes.toml` rows for the tier3 chrome/display scripts | `a727b9bcc4`, `1aa37b7621`, `5e1e4305a1`, `8d8ae5f55f`, `70b096c26a`, `c6714a5aab`, `6d4219819e`, `70cf9502e3` |
| 6. Allowlist re-review wave | 109 entries and 3 tier exceptions re-read site by site; none due before 2027-03-01; next dates staggered 2027-05-14/21/28; six wrong reasons corrected | `708b80877b`, `fef6f2ff93`, `1420ea6984`, `f923f4e322` |
| 7. `.agents/` retention | Report only (owner): 7,442 tracked files, 140 MB tracked, 204,227 tracked Markdown lines, 5.1 GB on disk. Nothing moved | — |
| 8. Living docs | `security-model.md` (authority, postures, Tier-1 perimeter, devices/DMA, `CapRevoke`), `system-architecture.md`, `codebase-summary.md`, `current-focus.md` (hand-back list), `project-roadmap.md`, `code-standards.md` | `164b0d7e7a`, `dd72d78b92`, `805fd50ee6`, `09794bf309`, `ca94ced5f1`, `da47be525d`, `f43abb4ca5`, `f053ff4d99` |

### Lanes that failed on `main`

| Lane | Root cause | Fix |
|---|---|---|
| `vfs-quota` | The RV64 test-hooks image lacked `/bin/input`, so the shell's serial session failed with `NotFound`; the test also expected `name=shell`, but the task is `cellbox` | `d4610efa6b` |
| `x86hv-iso` | The rollout guard refused the hypervisor kernel at the default kernel path | `a2d77056e1`: staged under `build/x86-hv-lane/` |
| EL2 test-hooks | At EL2 the context switch ignores private roots, so two root-entering self-tests ran under the kernel root and faulted | `268db39da4`: both log `SKIP (EL2 host has no private roots)`; Silo mode boots on cortex-a76. Both modes pass |
| `rust-std-promotion` (2 tests) | The 2026-08-21 approval package pinned paths the remediation moved | `bd7bc7f4c4`, `f5fa986da2`: owner-approved re-pin with re-bind notes; inventory 6 → 11 entries (`syscall/grant.rs` added after review), manifest 106 → 111 inputs, digest `635eab30…`; 33/33 pass |
| `shellutils` | The image check looked for `SFN SHELL` after the cellbox rename | `961cb9ca4f` |
| AI oracles (riscv64, aarch64) | The oracle image was built without `--config-features ai` | `299c23e083` |
| C2C broker oracle | The oracle image had no input service, so the shell never booted | `1c7fbecd8c`: the shell boots; the NET_RX proof still fails (N-8) |

### Other fixes

- **N-1** (`c7a36575d2`): x86 virtio-blk now latches `poisoned` on a stall and refuses every later request before claiming a bounce slot.
- **EL2 posture** (`1017e68b0f`): the Silo lane also requires `Tier 2 admission: DISABLED`, so an EL2 host positively refuses private roots.
- **`iommu-publication`** host test, broken by Phase 18's `DmaRemapper` change, compiles and passes again (`e8d50f89d0`).
- **aarch64 handoff tests** had silently skipped on the old target path; they run and pass (`8d8ae5f55f`).
- **Generated metrics** went stale with `f25c02f7c8` and would have failed CI `lint`; regenerated in `73f0215eed`.

## 3. Owner decisions

Recorded in the [Deviation Log](../phase-19-closure.md):
- **S8c approval:** staged and continuing; failing lanes diagnosed; retention report-only; allowlist wave in phase.
- **Closure-matrix answers:**
  - H-2, H-5, H-8, H-13 and H-14 are accepted residuals.
  - Phases 03 and 03b are residuals tied to the pushed-CI condition (`49cf531481`).
  - M-K4, M-A1, M-I4, M-U3, M-U4, M-B2, M-B4, M-B5b and L-6 are accepted residuals handed back.
  - N-1, the `rust-std` re-pin and the EL2 gating are fixed.

## 4. Lanes and guards

Runner `~/worktrees/p16-cp/pr.sh` (now with `nic2` and the `nightly` `bench` lane), each run on a snapshot worktree. Logs under `~/worktrees/p16-cp/<run>/`.

| Run (commit) | Lanes | Result |
|---|---|---|
| `p19-a` (`9858c4bec0`) | Every `pr` and `nightly` lane: kbuilds (13), hosttests (13), clippyx, `x86bootp14` (17), `nic2`, `x86c2cp14` (7), `x86domp16` (TCG 1/2/4 + KVM 4), `rvquick`, `rv64disk`, `a64boot` (8), `a64th`, `thbuild`, `vfsquota`, `srvp14`, `shellutils`, `bootsuite`, `ruststd` (3 arches), `x86hvfull`, `hvarm`, `tier3py`, `tier3e2e`, `tier3hostile`, `aioracle`, `c2coracle`, `idt`, `bench` | PASS except: `shellutils`, `bootsuite` (6 tests), `aioracle`, `c2coracle` (all also fail on `1d13abd68e`), and `x86hvfull`/Tier-3 (environment: Alpine download HTTP 429; the snapshot lacked `.alpine-cache-x86`). `vfs-quota` passes (fixed). 5998 s. |
| `p19-b` (`9858c4bec0`) | `x86hvfull`, `tier3py`, `tier3e2e`, `tier3hostile` with the Alpine cache linked | PASS: hypervisor smoke reaches the Alpine `/ #` prompt; python, nginx, virtio-e2e and hostile pass. |
| `p19-c` (`1c7fbecd8c`, final code) | `shellutils`, `aioracle`, `c2coracle`, `a64th` | PASS except `c2c-oracle` (N-8: `oracle passed without proving interrupt-driven NET_RX completion`). |
| `P19Diag` (`f5fa986da2` + `1017e68b0f` script) | AArch64 test-hooks, Silo (EL2) mode | PASS: EL2 VHE, `Tier 2 admission: DISABLED`, both root-entering self-tests SKIP, vfs-test 96/0. |

Guards on the closing head (`1c7fbecd8c`), all exit 0: `check-spec-anchors --check`, `check-plan-status --quiet-legacy`, `generate-arch-status --check`, `cellos-sign --check --strict`, `check-lanes`, `check-hal-boundaries`, `check-kernel-residents`, `generate-code-metrics --check`, `check-catalog`, `check-host-test-crates`, `mutable-globals --check` (708 declarations, 0 problems), `rust-std-promotion` unittest (33/33). Kernel host tests: 753.

## 5. Reviews

All reviews were independent, read-only `haily-reviewer` runs.

| Range | Verdict | Fixed in |
|---|---|---|
| `main..9858c4bec0` (end to end) | BLOCKED: 1 HIGH (stale generated metrics would fail CI `lint`), 4 MED (H-9 closed against an amended criterion; M-D3 closed on prose; Spec 00 Laws 5–6 bannered without a log entry; the re-pin dropped `syscall/grant.rs`), 6 LOW | `73f0215eed`, `4aff7facc9`, `8e1e70940e`, `f5fa986da2`, `1017e68b0f`; LOWs logged or recorded as N-5…N-7 |

The review confirmed: the virtio-blk latch gates every x86 I/O path without a new lock; EL2 detection is right and a misdetection fails the lane either way; the deleted code was dead; the lane fixes, test shim, target names and Dockerfile are sound; the re-pin changed digests only and left signed text alone; 16 sampled matrix rows resolve; its re-run of the review's checks matches the matrix.

## 6. Residuals for closure

Awaiting the owner's written acceptance (closure matrix § For owner closure): M-K2, M-K3, M-A2, M-A4, M-W5 and N-2 to N-9, plus acknowledgement of H-9 (closed against Phase 14's amended criterion).

Handed back to the portfolio (`docs/roadmap/current-focus.md`):
- the accepted residuals;
- Phases 03 and 03b;
- the carried Phase 16–18 residuals;
- the open items of the 2026-11-05 security audit;
- physical qualification.

Further notes:
- `target_arch` lines outside `kernel/src/arch/` sit at the 692 ceiling.
- Any later change to a file the `rust-std` approval package pins needs another re-pin.
- Evidence ceiling: host and QEMU (TCG, KVM). No physical hardware.
