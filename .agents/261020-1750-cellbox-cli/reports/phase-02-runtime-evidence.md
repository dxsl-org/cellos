# Phase02 runtime evidence — private signed x86 fixture

Scope: D01 child context/publication and D02 streams/session slice approved by [phase-02-cp1-approval.md](../phase-02-cp1-approval.md) (annex SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`, amendment A1 `ae6be1c39453287d19cf7027681f5e24c19b112cad387b3f0ff919c80aa676e0`). This is **private-fixture evidence, not CP2, not canonical rollout and not an independent acceptance sign-off**. All three quarantined artifacts re-verified unchanged after the work (cellbox `64eb28f3…f84a39`, terminal `636497ac…e52fa2`, canonical `kernel_fs.img` `447d0348…298e2c`).

## What ran

| Lane | Command | Result |
|---|---|---|
| Private signed-x86 image | `CELLBOX_PHASE02_WORK=/tmp/cellbox-phase02-* bash scripts/qemu-phase02-session-test.sh` | **48 `P02 PASS`, 0 FAIL**; runner's independent verdict `observations complete` |
| Kernel regression cases (test-hooks image, dev trust anchor) | same, `PHASE02_KERNEL_TESTS=1` | **35/35 `ATOMIC_PUBLICATION_*` PASS**, including new `AP-PM/PR/PT/PA/PH/PL/PU/PC/PF/PK/PB/PE/PI/PS` over real owner code |
| Host suites | `cargo test --offline --locked -p api -p ostd -p terminal-sdk -p text-engine --target x86_64-unknown-linux-gnu` | api 124 (baseline 109), ostd 41 (41), terminal-sdk 21 (14), text-engine 51 (51), doctests 19+2 ignored, 0 failed |
| VFS binder host tests | `cargo test -p service-vfs --target x86_64-unknown-linux-gnu` | 76 passed |
| Bare-metal consumers | `cargo check --release --target x86_64-unknown-none -p app-shell -p service-input -p service-vfs -p app-phase02-*` | pass |
| Kernel compile | x86_64 default / `test-hooks` / `phase02-terminal-profile`; riscv64gc; aarch64 | pass (only dead-field warnings) |

Baseline before edits ([phase-02-build-baseline](phase-02-build-baseline.md)): selected Rust suites 243 passed/6 ignored; CSpace harness 65; full-workspace host build already failed in `app-posix-shim-test/build.rs` (unchanged, not a regression); bare-metal consumer check passed. Negative controls were real: earlier runs of the same fixture failed on genuine defects (query buffer too small, LF/CR, sealed-path cwd, descendant lifetime) and exposed them before they were fixed.

Provenance (final run): HEAD `fabd6ab7…509888dd` plus dirty tree; [provenance.json](phase-02-evidence/provenance.json) records toolchain, `Cargo.lock` `119fee53…`, per-cell/init/image/kernel/ISO SHA-256, the per-run throw-away key identity and the exact QEMU argv ([qemu-argv.txt](phase-02-evidence/qemu-argv.txt): q35, TCG, `-nic none`, no writable disk, owned chardev socket). Cells signed with a per-run random Ed25519 key after the F1/F5 check; the private kernel trusts only that key (`phase02-terminal-profile`), refuses canonical embedded inputs and requires `signing-required`. No `/POLICY.BIN` exists in the image; the kernel logged it absent (development-permissive). The shell prompt was reached from this image and the fixture parent was launched by the real serial shell. Observations: [fixture-observations.txt](phase-02-evidence/fixture-observations.txt), [kernel cases](phase-02-evidence/kernel-atomic-publication-cases.txt). Owned QEMU processes only were terminated; keys and sockets were removed after copying sanitized evidence.

## Observation → criterion map

| Acceptance | Observed (real owners, serial log) |
|---|---|
| CBX-03 session isolation, typed delegation, restoration | `two-session-routing`, `two-session-no-leak`, `resize-*`; `foreign-child-cannot-touch-private-endpoint` (read/write/duplicate/close/cancel all `WrongOwner`); `forged-endpoint-denied`; `typed-input-to-foreground-only`; restoration via owner proof after `foreground-restored-after-exit`, `fault-…`, `raw-mode-fault-…`, `cancel-…`, `ctrl-c-…`, each followed by baseline input and an unaffected second session |
| CBX-04 stdio/argv/cwd/env | `context-exact-at-first-instruction` (argv with empty and spaced items and UTF-8, cwd `/` with directory grant, env incl. empty value, read from the committed context at the child's first instruction), `stderr-separate-from-stdout`, `eof-reaches-child-outputs-preserved`, `non-utf8-nul-bytes-preserved` (0xFF/NUL/DEL/ESC…), `non-utf8-cwd-refused-before-launch` (`UnsupportedInput`), `argv-limit-refused-before-launch` (`QuotaExceeded`); output travels over owner streams, never the console log |
| CBX-05 EOF/revocation/bounds | `timeout-restores-foreground-and-mode`, `partial-write-bounded`, `full-ring-would-block`, `timeout-distinct-from-eof`, `retained-duplicate-prevents-eof`, `last-writer-eof`, `no-reader-write-refused`, `descendant-and-duplicate-hold-writer`, `descendant-bytes-arrive`, `eof-after-descendant-exit`, `holder-death-closes-writer`, `consumer-close-reaps-blocked-producer-subtree`, `root-cancel-reaps-blocked-consumer-subtree`, `cancelled-subtree-endpoints-revoked`, `unrelated-session-survives-cancel`, `abort-then-commit-refused`, `expired-prepare-refuses-commit`; orphan child ceased within 0.5 s of its parent's exit (4 heartbeats, then 13.6 s silence) |

## Independent review and fixes

Two independent read-only reviews of the *implementation* produced 27 findings (kernel 14, services 13). The confirmed ones were fixed and the **whole** image fixture and the kernel AP lane were re-run afterwards without regression (48/48 and 35/35). Fixed: unauthenticated request-ledger growth and non-reaped ledgers, un-drained receipts/bootstraps of dead or replaced issuers, per-principal prepared cap, atomic bootstrap activation, unbound-receipt commit, missing `SpawnFromPath` gate, premature `observed`, sweep-before-actor, console fallback of a context child, child-operation/lane exhaustion (shell now closes operations), fatal `Lost` event, permanently blocked lane on a pending retained query, secret-redacting `Debug`, VFS request-ID burn and generation guard, input physical capture only without a compositor, idle spin (input; VFS still yields), per-caller outcome budget, baseline-EOF, Ctrl-C latch. Directly exercised by a test: ledger/guessed-ref exhaustion and unbound-receipt commit (`AP-PL`, `AP-PU`), VFS generation guard (host test). **Not separately exercised on the image:** >32 consecutive external commands, a stray escape at the prompt, the pending-query retry, and the VFS ID-roll-back path.

## Limitations: fixed in the follow-up pass
Re-verified by the final TCG **and KVM** image runs (48/48 each, including 40 consecutive external commands and an undecodable escape at the prompt) and the 34-case kernel lane:

- **Superlinear cancel/close (#6):** id-indexed tables, per-operation depth, flat worklist cancel, bounded maintenance (`AP-PK`: 24-deep nesting and consumer-loss cascade).
- **Measurement log exhaustion (#8):** per-creator share of 64; a long-lived creator evicts only its own oldest detailed entry (aggregate keeps advancing), refusal only for its own outstanding promises (`AP-PC`). This test exposed and fixed a real accounting bug: a committed reservation never released the creator's promised unit, which would have refused a creator after 64 launches.
- **Whole-operation cancel on consumer close (#9):** only the ring's holders and that session's committed child are cancelled; sibling session and controller holders survive (`AP-PB`).
- **Reclamation needed full quiescence (#10):** records are reclaimed unless a retained outcome names them (`AP-PE`).
- **FIFO position after a steal (#14):** exact slot tracked per bucket (`AP-PF`, pure queue plus production steal path).
- **Issuer replacement/death after publication:** operation cancelled, peers revoked, receipt drained (`AP-PI`).
- **Secret hygiene:** zeroizing helpers (`ostd::session::{zeroize*,Scrub}`), input event/paste/raw-ring/outcome scrubbing (retained secret requests are sealed to a SHA-256, replay still resolves, changed bodies stay Malformed), terminal SDK buffer scrubbing and redacted `Debug`; `read -s` takes an echo-off lease (refused on the physical UART). Host tests cover the helpers and scrubbing logic.
- **Interactive builtins:** serial `top` (without `-b`) and `rm -i` are refused inside the admitted dispatcher instead of blocking forever.
- **Not previously exercised, now exercised:** KVM (1 hart), 40 consecutive external commands (child operations/lanes closed per reap), stray escape at the prompt.

## Amendment A2 (approved) — serial/stream limitations closed
Verified by the same lanes (kernel AP 35/35 incl. `AP-PX`; TCG and KVM image 48/48 plus two new serial checks: `ctrl_j_not_enter`, `top_quit`; host api 127 tests):
- **UART Enter is CR end to end.** The x86 RX ring no longer rewrites CR→LF; the legacy no-input-service fallback converts CR→LF itself (pure `route_for`, `AP-PX` over all 256 bytes), and input no longer maps LF→CR, so a physical Ctrl-J is Key(j,Control) and two commands joined by LF form one line (image check).
- **SerialDecoder wipes its pending bytes after every completed or rejected sequence** (safe-Rust fill + `black_box`; three host tests). The paste flag is intentionally left as cross-sequence framing.
- **Builtin I/O through the session sink:** ~160 direct `ostd::io::print*` calls in the shell builtins now use `shell_print`/`shell_stderr` (capture-aware; legacy console when no serial session). `rm -i` and interactive `top` read through the session (echo-free key reads, Ctrl-C cancels); the earlier refusal guards are removed. Image check: `top` over serial exits on `q`. **`rm -i` is exercised on the image** over `/tmp` (RAM mount): answer `n` keeps the file, Ctrl-C keeps it, answer `y` removes it (`rm-i.json`, all true; `/data` has no writable volume in this image and was not used). 49 observation checks pass in that run.
- **Disclosed extra edit:** `cells/tools/shell/src/top/render.rs` (one direct clear-screen print) was changed to make interactive `top` work but is **not named in A2's file list**; **ratified by the user (“duyệt cả 2 việc”).**

## Not claimed / still open
- **Physical echo-off** cannot be enforced by any Cell (the host terminal echoes); physical sessions keep refusing it and `read -s`.
- **Design limits:** in headless images any cell may bind the physical serial (needs a kernel-attested console principal); cross-process frontend attach and GUI/Ssh frontends; operation state `Completed` is never produced.
- **Not exercised:** issuer death on the live image (kernel case `AP-PI` only); a wall-clock concurrent commit-vs-abort race; multi-hart; `cellos_g1_cpu` build; riscv64/aarch64 runtime (compile-checked, including the shared console path).
- CP2, canonical packaging/rollout, the GUI bridge, D03/D04 and Phases 03–10 remain blocked.

Correction (2026-10-21): the kernel lane previously stopped at `AP-PS` and silently skipped later cases, so the counts quoted above (34/35) omitted some; the lane now waits for `AP-PX` and reports 36/36.
