# Handoff — architecture-description work, 2026-07-31

Written for an agent picking this up with **no prior conversation context**. Read this file
first, then the two files named in §2.

**Branch**: `feat/wx-post-reloc-and-f1-signing` · **HEAD**: `b21dcd78` · working tree clean
apart from four build artefacts (`build/vicell-x86.iso`, `build/x86-iso-root/boot/kernel.elf`,
`kernel/src/embedded/init`, `tests/integration/.gitignore`) that are not part of this work.

## 1. What this work is

An audit of the Cellos architecture *description* against the code, driven by a finding that
the specs and the implementation had drifted in both directions: specs promising mechanisms
that do not exist (Metadata Registry, `catch_unwind`, SASan), and code containing things specs
denied (`fast_ipc`, RedoxFS on `/srv`, a priority scheduler, TLSF, littlefs, ViUI v2).

39 disagreements were catalogued as **numbered decisions D1–D25** (plus D1b), each phrased as a
pick-one question for the architect rather than a recommendation. Eight are closed. The
governing rule, stated at the top of the docket, is: *where a spec and the code disagree, the
question is which one is wrong* — several specs describe a better system than the one built,
several a worse one.

## 2. The two files that matter

| File | What it is |
|---|---|
| `.agents/reports/decision-docket-260730.md` | **The worklist.** Part 0 = open actions A1–A4. Parts 1–3 = decisions D1–D25, closed ones marked RULED/MEASURED/VERIFIED with evidence inline. |
| `.agents/reports/qemu-build-unblock-260731.md` | **Read before building anything.** Two small problems made three prior phases conclude "cannot build/boot here", which is false. |

Supporting analyses, referenced from the docket where relevant:
`d1-fast-ipc-analysis-260731.md` (IPC measurements), `d5-cell-scale-measurement-260731.md`
(cell-scale measurements), `spec-unresolved-inventory-260730.md` (the original 450-line sweep),
`plan-inflight-inventory-260730.md` (76 plan dirs, which are stale/conflicting).

`.agents/` is **gitignored** — none of this ships. If it must outlive the working copy, move
A1–A4 to GitHub issues or `docs/TODO.md`.

## 3. Closed decisions — do not re-litigate these

Each was closed on evidence recorded in the docket. Re-opening needs *new* evidence, not a
fresh argument.

- **D1** — Spec 17 is the IPC model of record; `fast_ipc` is to be **rewritten for Tier 1**,
  not restored. The existing code is unreachable by construction (two disjoint statics, a
  documented bridge that was never built, a privileged CSR on a path a U-mode cell must take).
  Measured: marshalling is 1.3 % of a round trip; the cost is the rendezvous. Saving from
  running a handler on the caller's thread ≈ 82–98 %.
- **D1b** (new) — `ipc_send_recv` p99 = 86.6 µs fails the 50 µs PDR target. Still needs a
  ruling on whether that target is a TCG or a hardware figure.
- **D2** — Tier 2 is accepted-but-unbuilt; it *adds* a containment tier and does not reverse
  the "untrusted code → Tier 3" advice. Docs amended.
- **D3** — kernel size measured under five definitions (nLOC excluding tests = 18 494; "core"
  = 14 679). Still needs: pick one definition, and decide whether Spec 15's ≤5 000-core target
  survives (it is off by ~3×).
- **D4** — Instant-On snapshot is real and wired; the KASLR "contradiction" was spurious (the
  KASLR lane is the one lane where snapshot is not compiled). A latent whole-RAM-corruption
  hazard found and **fixed** in `a2c9685e`.
- **D5** — the "reject BEAM-scale" position was withdrawn as too broad; a **per-request server
  profile** is now committed alongside the large-app one (Spec 19 §3). See §5 below for the
  measurement that reordered the work.
- **D6** — F1 reads "absolute outside the reviewed allowlist"; the gate of record is
  `scripts/cellos-sign --check --strict`. Verified by injecting `unsafe` into a clean cell:
  fails with exit 1 and names the file. Four documents corrected.
- **D7** — the code caught up to the spec (not the reverse). W^X **runtime-verified for the
  first time**: `wx-text-write` 2/2 PASS, `boot` suite 54/54 PASS. `02-memory.md §5` now states
  three limits *of the guarantee*: code integrity only (cross-cell **data** still writable), no
  cross-hart TLB shootdown, and no enforcement on bare-physical arches.

**Next unanswered decision is D8.** D9 (is RK3588 ARMv8.2 or v8.5?) is worth doing early: it is
a hardware fact that two documents state differently, and Spec 19's entire "must be built from
page tables" argument depends on the answer. D25 (`machine_id` is self-asserted and spoofable
to win Primary) is a live security hole, not a documentation issue.

## 4. Environment — the part that will waste your day if skipped

QEMU (all three arches), `riscv64-unknown-elf-*` and `pwsh` are installed and working. A full
RV64 image builds and boots to the shell. Two problems, both small:

1. **Toolchain name.** Every C-dependent `build.rs` hardcodes `riscv-none-elf-gcc` when the
   per-target `CC` env var is unset; Ubuntu installs `riscv64-unknown-elf-*`. Fix: a directory
   of symlinks named `riscv-none-elf-<tool>`, prepended to `PATH`.
2. **`gen_disk.ps1` composes `CFLAGS_riscv64gc_unknown_none_elf` but it does not reach cargo**,
   so littlefs2-sys fails on a missing `string.h`. Fix: export the four variables from the
   shell before invoking the script.

Working invocation, and the boot recipe, are both in `qemu-build-unblock-260731.md`.

**Integration tests on Linux need `--target x86_64-unknown-linux-gnu`** — the checked-in
`.cargo/config.toml` defaults to a Windows host target, so a bare `cargo test` fails to find
`core` for `x86_64-pc-windows-msvc` before QEMU is ever launched.

## 5. Traps found the hard way

- **The git index is shared between concurrent sessions.** `git add <one-file>` does *not*
  guarantee a single-file commit: another session's staged deletion was already in the index
  and would have shipped a deleted CI script without its workflow change. Always check
  `git diff --cached --name-only` before committing, not just `git status`.
- **`bench-probe` is a separate binary with its own role dispatch.** Adding a role only to
  `cells/tests/bench/src/main.rs` leaves the peer exiting and the caller blocked forever in
  `sys_send`. Register in both.
- **The shell prompt `ViCell > ` has no trailing newline**, so a line-oriented harness blocks
  forever waiting for it. Trigger on `=== ViCell shell ready ===`, then pause before writing —
  the input service only delivers to a cell already parked in `Recv`.
- **An orphaned QEMU holds a write lock on the disk image**; the next boot fails with "Failed
  to get write lock" rather than anything about locks being stale.
- **The bench cell declares its syscalls explicitly** (`main.rs:12-25`) and holds neither
  `LookupService` nor `RecvTimeout`, so it cannot discover the VFS tid. Widening that allowlist
  changes the system under measurement — prefer a peer the bench spawns itself.
- **`sys_get_time` is a syscall**, and the shared bench runner brackets every `run_once` with
  two of them. Anything syscall-scale must amortise over an internal loop or the measurement
  overhead swamps the signal.

## 6. Measurement lessons — twice the intuition was wrong by an order of magnitude

Both were closed only because they were measured. Treat "obviously the bottleneck is X" as a
hypothesis here.

- **D1**: predicted the win from a direct call was avoiding serialization. Measured: encode
  259 ns, decode 359 ns, bare `ecall` 1 861 ns, full round trip 46 674 ns. Marshalling is
  **1.3 %**; the rendezvous is everything.
- **D5**: predicted the cell ceiling was the 512 KiB stack. Measured: refusal at **n = 9** with
  a 2 GiB guest, `MAX_CELLS` raised to 512 and 512 VA slots free. Cause: `kernel/src/boot.rs`
  hardcodes the usable region at **190 MiB** and never reads the DTB. The revised work order
  puts the memory map *first*, ahead of image sharing and demand-paged stacks.

## 7. Built worktrees — reusable, but living in a temp directory

`git worktree list` shows both. They are fully built (disk image + kernel), so A4 and any
re-measurement cost a boot rather than a 15-minute build — **while they last**. They sit under
`/tmp`, so a reboot or a scratchpad sweep removes them; `git worktree prune` afterwards.

| Path | Checkout | Contains |
|---|---|---|
| `/tmp/claude-1000/-home-dmin-cellos/668cf6f2-68cb-4088-b22b-bcaa4a03f49d/scratchpad/bench-main` | `main` @ `562634bd` | The cell-scale experiment: `cells/tests/bench/src/scenarios/spawn_scale.rs` and `MAX_CELLS = 512` in `kernel/src/memory/cell_quota.rs`, neither committed anywhere else |
| `/tmp/claude-1000/-home-dmin-cellos/668cf6f2-68cb-4088-b22b-bcaa4a03f49d/scratchpad/wx-verify` | detached @ `4f11e6ae` | W^X verification, plus the snapshot guard copied in by hand |

Also under `…/scratchpad`: `shim/` (the `riscv-none-elf-*` symlinks §4 needs), `run-bench.py`
and `run-scale.py` (QEMU drivers that already handle the no-newline prompt), and the raw logs
behind every number quoted in the reports.

**Working build invocation** — run from inside a worktree; the two exports are the §4 fixes:

```bash
export PATH="/tmp/claude-1000/-home-dmin-cellos/668cf6f2-68cb-4088-b22b-bcaa4a03f49d/scratchpad/shim:$PATH"
export CC_riscv64gc_unknown_none_elf=riscv-none-elf-gcc
export AR_riscv64gc_unknown_none_elf=riscv-none-elf-ar
export OBJCOPY=riscv-none-elf-objcopy
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
pwsh ./gen_disk.ps1
```

`gen_disk.ps1` **exits 0 even when the cargo build inside it fails** — check its output for
`FATAL` rather than trusting the status code. A worktree also needs `.cargo/config.toml` copied
in from the main checkout before anything builds.

The reusable half is already committed: `vfs_getfile_breakdown.rs` in `3ef6da45`. If the
worktrees are gone, recreate the scale experiment from `d5-cell-scale-measurement-260731.md`
§Method — it is about twenty lines plus one constant.

## 8. Where to pick up

1. **A1 (DTB memory map)** — highest leverage and not just a server-profile concern: every
   deployment currently discards RAM above 190 MiB.
2. **A4** — re-run the runtime gates phases 09 and 11 left open, while the worktrees last.
3. **A2, A3** — small, and both make the next capacity question directly measurable instead of
   inferable.
4. **D8 onward**, or jump to **D9** and **D25** for the reasons in §3.
