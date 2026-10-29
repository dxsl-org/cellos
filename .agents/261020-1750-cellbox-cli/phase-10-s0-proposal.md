# Phase 10 S0 — canonical cutover, containment lift and qualification (CP1 record)

Status: **S10-3a/b/c/d and S10-3 CP2 APPROVED2026-10-28**, qualified read-only dev-posture Intel x86 QEMU/TCG candidate **`4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821`**; S10-0/1/2a approvals retained. **S10-4 limited-result CP2 approved2026-10-28** covers S10-4.8's actual partial measurements/deviations only. Later separately approved host-observer CP1 slices preserve those historical verdicts. **Latest S10-4.15 executed2026-10-29, partial-result CP2 pending:**191distinct offlinePASSaggregate(retained144once+current47new, failed iterations disclosed); ONEdesktop realGUI/idle/typedexact/originalEnter and healthy transport/proof recovery, then conservative temporal oracle STOP on missing physical-row coverage at20th exact sample, ~31.15s within original120s. Lastfb/read/epoch retained; no finite two-frame/newprompt/status0/fullschedule proof. No historical runtime comparator/full-memory peak/RSS/exactstack or CBX27/criterion4 closure. Further complete-output route needs separate explicit CP1; no rerun authorized. Candidate/canonical/userVMX unchanged; S10-5/canonical promotion/containment remain blocked.


**S10-5 CP2 GRANTED by the user 2026-10-29** (relayed by Main): accepts S10-5.13 (P0–P9, V1–V7, deviations 1–8) and acknowledges the new `command-catalog.md` digest `f06f38ce7447758f7b49e58d6e98dc8309b583515bf1ffd3608e665011a5fd0f`. The qualified candidate's desktop bytes are promoted to the canonical x86 paths at the dev-posture Intel x86 QEMU/TCG ceiling; Phase 04 criterion 4 is checked (`completed-private-evidence`) and Phase 10 is `closed-partial-private-evidence`. **Unchanged and not approved:** CBX-27 unmet, CBX-23/24/25 partial, Phase 10 criteria 1 and 4 open, no physical/VMX/production/remote claim, no cloud formats, non-Intel recipes refused, all 38 lock relations. Commit is a separate step.
**Latest, S10-4.17 EXECUTED 2026-10-29 (CP1 approved as written): Class B `SUCCESS_METRIC_ONLY`; limited-result CP2 APPROVED by user 2026-10-29; S10-4 CLOSED PARTIAL (closed-partial).** Frontend-consumed `ByteSink::feed` ledger proved, in ONE desktop run, two complete JSON frames, fresh prompt, then the separate original `echo st=$?` epoch `st=0` and new prompt, each cross-checked to the exact final GUI screen. The retained original schedule then STOPped at a latent `Surface.status()` check (two `st=` rows visible after the collected-cat cancel; left unfixed), so the full workload and full memory peaks remain unobserved. Accepted at stated ceilings: static footprint, six untraced timing runs, headless controls, observed partial memory bounds, S10-4.9..S10-4.17 diagnostic findings, offline iterations and deviations 1-4. **Not approved/unchanged:** full-workload peaks, RSS, exactstack, kernel stack, matched historical runtime, remaining observed phases. **CBX-27/criterion 4 stay UNMET**; S10-5/canonical promotion/containment stay blocked and need their own CP1. No rerun.

**Approval boundary (2026-10-28).** User CP2 covers the implementation/evidence ceilings and recorded deviations of S10-3a/b/c/d plus the complete S10-3 battery, including SC6's explicitly authorized document-audit-only correction with both verdicts retained. It does not waive known COMPLETION-QUEUE/xHCI failures, warnings, D60-3/Delete, incidental DMA-free/DSTRD gaps, SSH guest/publickey gaps or the “synced, not crash-proof” ceiling. Acceptance rows now reflect S10-3.14; CBX-23/24/25 remain partial, CBX-27/28 await S10-4/S10-5. No canonical byte or guard is changed by this approval.

Parents: [Phase 10](phase-10-qualification.md), [Phase 04](phase-04-multicall-cutover.md) criterion 4, [closure review](phase-04-closure-review.md) §1 row 4, [rollout containment](reports/rollout-containment.md), [acceptance](acceptance.md), [baseline](baseline.md) §"Embedded filesystem inventory", [Phase 01 closure dossier](phase-01-closure-dossier.md) step 8.

How this was prepared: by reading only. No source, script or record was edited except this new file. No build, signing, QEMU or network action was run. The canonical artifacts below were hashed and their FAT contents listed with the repository reader `scripts/generate-boot-config.py` `image_files()`, in memory. Claims marked `[INFERENCE]` come from reading code and were not executed.

---

## 1. Exact current state

### 1.1 Guards

| # | Guard (file:line) | What it refuses | What it leaves open |
|---|---|---|---|
| G1 | `scripts/package-desktop-x86.py:18-24` | Unconditional `sys.exit("ROLLOUT BLOCKED …")` at the top of `main()`. It runs before the root is resolved, before signing (`:49-52`) and before the image is read or replaced (`:57`, `:103`). | Nothing; there is no override. |
| G2a | `scripts/cellos_sign/signing.py:102-122` `preflight_blocked_artifacts`, called at `:136` before any key is loaded | A target named `cellbox`, `app-cellbox` or `terminal` (`:35`; both `target.name` and the resolved symlink name) unless `private_fixture_allowed` (`:90-99`) holds: `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` **and** an explicit seed that is not the dev seed `43×32` (`:42`). The whole batch is refused before any target is written. | Other names. |
| G2b | `signing.py:36-39,116-122` | Any file whose SHA-256 is `64eb28f3…2a39` (old cellbox) or `636497ac…2fa2` (old terminal), under any name, env or seed. | — |
| G2c | `signing.py:143-147` | The dev key signing any fixture name (a second check after the key is loaded). | — |
| G2d | `signing.py:58-75` `guard_prod_key` (this predates containment) | A non-dev seed outside CI unless `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1`. Every private lane sets this flag. | — |
| G3 | `scripts/x86/make-iso-ci.sh:25-33` | `KERNEL` resolving (through `realpath -ms` or `-m`) to the default `target/x86_64-unknown-none/release/cellos-kernel` (`:22,28`). Refused before xorriso runs or staging is written. | An explicit `X86_KERNEL` at any other path. The private lanes and `ci.yml:1040-1046` use this. A **copy** of the canonical kernel would also pass `[INFERENCE]`. |
| G4 | `kernel/build.rs:128-150` | A `phase02-terminal-profile` kernel embedding from `src/embedded-x86_64` or from any input under it. | Not a containment guard: it keeps fixture kernels out of canonical bytes. **Stays.** |
| G5 | Lane alias guards: `qemu-phase02-session-test.sh:78-81`, `qemu-phase03-pipeline-test.sh:70-73`, `qemu-phase04-terminal-test.sh:165-168`, `qemu-phase04-gui-test.sh:137-163` (which also hashes the canonical files before and after the run) | A work directory that aliases `kernel/src/embedded-x86_64`, `target/x86_64-unknown-none/release` (or `build/` for the GUI lane). | **Stay.** |
| T | `scripts/test_cellos_sign.py:350-575` (49 tests recorded in containment 2026-10-22) | Regression tests for G1–G3 and G2a–c, including `test_quarantined_hashes_stay_refused_under_the_allowance` (`:562`). | — |

**Effect on the canonical recipes `[INFERENCE]`.** Every canonical recipe signs `cellbox` with the dev key (no `--seed-hex`), so G2a stops each of them at preflight:

- `scripts/build-x86_64-cells.ps1:197-203`;
- the x86 job in `.github/workflows/ci.yml:954-959` and the aarch64 job at `:621-625`;
- `scripts/gen-disk-ci.sh:260-282` (RV64);
- `scripts/make-hypervisor-fs-x86.sh:106` and `make-hypervisor-fs.sh`;
- `scripts/build-aarch64-cells.ps1`.

The S4a CP2 dossier recorded this for the RV64 and aarch64 recipes ("the containment signer refuses canonical cellbox signing"). Every recipe that builds an ISO from the default kernel also stops at G3: `ci.yml:986,1143` and `scripts/build-cloud-images.sh` step 3. That covers the 12 integration consumers of `build/vicell-x86.iso` (`tests/integration/tests/{x86_64-boot,ahci-x86,xhci-x86,igb-x86,iommu-dmar-x86,serial-x86,nic-x86,nvme-x86,virtio-x86,pcie-multibus-x86,handoff,desktop-x86}.rs`). In short, the CI x86/aarch64 image jobs cannot be green while containment holds. This was not run.

### 1.2 Quarantined bytes: where they are today (observed 2026-10-26, read-only)

| Location | Observed | Contains old cellbox `64eb28f3` / terminal `636497ac`? |
|---|---|---|
| `kernel/src/embedded-x86_64/kernel_fs.img` | 16,777,216 B, SHA-256 `447d0348…298e2c`, mtime 2026-10-21 15:51:49 | **Yes**: `/bin/cellbox` 88,576 B = `64eb28f3…`, `/bin/terminal` 595,032 B = `636497ac…`. The hash again equals the original containment table, so the 2026-10-21 "no longer matches" update is stale. |
| `target/x86_64-unknown-none/release/cellos-kernel` | 19,769,472 B, `12cb0bac…`, mtime 2026-10-25 19:05 | **Yes**: it embeds the whole `447d0348` image. G3 refuses exactly this path. |
| `target/x86_64-unknown-none/release/cellbox` | 3,083,840 B, `006b8504…`, mtime 2026-10-25 22:16, **no `__ViCell_sig`** | **No.** The bytes the containment record calls "quarantined in place" were overwritten by later builds. Old bytes survive only inside images. |
| `target/x86_64-unknown-none/release/terminal` | 950,760 B, `018db0cc…`, no signature | **No** (same note). |
| `build/x86-iso-root/boot/kernel.elf` (git-tracked, modified) | 19,602,560 B, `dbd862d6…` (= the 2026-10-20 baseline kernel) | No (4 KiB byte-slice scan). |
| `build/cellos-x86_64-desktop-fixture.iso` (`6af523ec…`), `build/cellos-x86_64-desktop-wip.qcow2` (`9aa29105…`) | — | **Yes** (4 KiB byte-slice scan at ELF offset 4096; a heuristic, not a full-file hash match). Other `build/*.iso/qcow2/vmdk`: no slice found. |

### 1.3 What the canonical x86 image ships today (`447d0348`, 41 files)

- Split shell and legacy autoload: `/bin/shell` 881,120 B (`225fb29f…`). `/etc/cellos/autoload.toml` starts `desktop` and **`shell` → `/bin/shell`**, not `/bin/cellbox`.
- Unsafe inbound service: `/bin/sshd` 197,048 B. `services.toml` starts `sshd` (`after = ["net"]`, `restart = "always"`). CBX-25 and verification.md forbid starting this.
- Quarantined `/bin/cellbox` and `/bin/terminal` (§1.2). No `/bin/cellbox-ssh`.
- Retained workers `ls`, `cat`, `echo`, `ps`, `kill`. Their hashes equal the baseline table, so the **980,528 B baseline subset is still physically present** in this image.
- `/bin/config` is present but not started (no services row).

No current recipe installs `/bin/shell` or `/bin/sshd` (repository grep). They survive because `package-desktop-x86.py:57` overlays new files onto the **existing** image. So these files can only be removed by assembling the image fresh; editing the packager's map is not enough.

### 1.4 Canonical recipes and the split/retained tools

| Recipe | Ships today | Gaps for criterion 4 |
|---|---|---|
| `scripts/build-x86_64-cells.ps1` (headless x86 base; writes `kernel/src/embedded-x86_64/{kernel_fs.img,init}`) | `/bin/cellbox` (`:159`), workers `ls cat echo ps kill shutdown` (`:171-176`), config/net/input/drivers, tier2 fixtures; dev-key signing `:197-203`; `mkfat32 --config-features input` `:255-256` | No `/bin/cellbox-ssh`. A failed build of cellbox (`:71`) or sys-tools (`:133`) only prints `Write-Warning`, and the required list (`:226`) checks presence only, so **stale bytes can be packaged**. This violates Phase 04 step 5. |
| CI x86 job, `ci.yml:915-976` (Linux mirror of the above) | cellbox, workers `ls cat echo ps` | Drifts from the ps1 (no `kill`, `shutdown`, `net`, `serial`); no alias. |
| `scripts/package-desktop-x86.py` (desktop overlay) | `apps_map` `:29-40`: compositor, input, desktop, files, **terminal**, chrome, ocel, net, vfs, cellbox; hand-written autoload `:67-81` (desktop + `cellbox sh`) | It overlays the old image (§1.3). It does not regenerate `services.toml` from templates with `ui`, and init is built `input` only. A fresh base would therefore not start the compositor `[INFERENCE]`. The GUI lane builds `app-init --features input,ui` (`qemu-phase04-gui-test.sh:277-278`). No alias. |
| `scripts/build-cloud-images.sh` | ISO, qcow2, vmdk and KubeVirt from the default kernel | Distribution formats; blocked by G3. |
| Non-Intel: `gen-disk-ci.sh`, `build-aarch64-cells.ps1`, `gen-disk-aarch64-ci.sh`, `make-hypervisor-fs*.sh`, CI aarch64/RV64 jobs | cellbox plus workers | Out of the Intel scope (ADR-0022); blocked by G2a. |

**Under the frozen catalog only `/bin/shell` is a retired split tool.** The catalog is `f2a82fce…9339`, and `ledger.rs:126-143` gives all 11 sys-tools and 6 net-tools the disposition `RetainedWorker`. The brief's phrase "removal of retired split tools from image lists" therefore means removing `/bin/shell` (plus the `sshd` and quarantined residue). Removing worker ELFs would require unfreezing the catalog (decision D10-6).

### 1.5 Boot configuration and authority already in place

- `config/autoload.toml` starts `cellbox` with `args = ["sh"]` and `restart = "on-failure"` (catalog §6 cites `:23-25`).
- `config/services.toml:38-42` has a `config` row (service_id 4). `generate-boot-config.py:43-44` selects `/bin/config` and `/bin/ai` **only for feature `ai`**. Without it, `export` reports `config unavailable` with status 1 (Q-right CP2 deviation 1).
- `/bin/config` and `/bin/cellbox` are in `BOOTSTRAP_CELLS` (`kernel/src/loader/early.rs:35-41`).
- Launch rows for the alias already exist and are CP2-approved (S9-2):
  - `sign-policy.py`: `/bin/cellbox-ssh` gets `NET_CLIENT`;
  - `launch_profile/profiles.rs:75`: only the shell's Path edge to it carries net rights;
  - `task/cap.rs:499`;
  - the boot-ceiling self-test, `selftest.rs:276-291`.
  
  Only the packaging is missing. The P09 lane ships the alias as a **second full FAT copy** (`s9-close-evidence/fat-layout.txt`: `cellbox` and `cellbox-ssh`, each 3,081,696 B, on different clusters).
- Kernel default features (`kernel/Cargo.toml:70`): `dev-policy-key`, `dev-signing-key`, `dev-weak-rng`, `signing-required`, `native-domains`. The canonical image is a **dev-posture** image: dev cell key `0x43×32`, dev policy key `0x42×32` (`sign-policy.py:53`), and a weak-RNG fallback when RDSEED/RDRAND are absent.
- Entry point (`cells/tools/cellbox/src/main.rs:132-141`): the only argv entry is `ssh` (the `/bin/cellbox-ssh` child); everything else runs the shell. An explicit `/bin/cellbox <command>` invocation from argv is **not implemented**, though the catalog §1 and `docs/cellbox-cli.md:36` describe it as canonical (S4a dossier, limitation 6). This affects CBX-10 (decision D10-11).
- Stale wording: stock `/bin/env` prints `SHELL=/bin/shell` (`cells/tools/sys-tools/src/bin/env.rs:21`).

### 1.6 Private lanes depend on fixture-only kernels

| Lane | Last recorded result | Fixture dependency |
|---|---|---|
| P02 session (`qemu-phase02-session-test.sh`); its test-hooks mode is the "kernel lane" | 48; kernel 39/39 + 7/7 VFS self-tests | `phase02-terminal-profile` (private signing pubkey, fixture routes) or `test-hooks` |
| P03 pipeline | 67 | adds `phase03-filter-profile` (`/bin/p03-filter` edge) |
| P04 terminal | 466/466 | adds `phase04-driver-profile` (the driver replaces the serial shell) |
| P04 GUI | 290 | `phase02` + `phase03` profiles; private autoload |
| P09 SSH | 103/103 | cellbox built with the lane-only `p09-faults` feature (D79-15c), so its bytes differ by construction |

Each lane builds in an isolated `CARGO_TARGET_DIR` and signs with a per-run random seed. **No lane boots a canonical-feature kernel or canonical cell bytes.** That gap is what S10-3 must close.

### 1.7 Lock relations (`.agents/cell-contract-0024-locks.md`, 38 rows)

Phase 10 needs no lock narrowed if it stays within the scope proposed here: no ABI change, dev posture, Intel QEMU only. The relevant locks stay in force and bound what Phase 10 can claim:

- `260825-1726-kms-silo-production-root` (protected signer, production root) and `260826-1605-phase4-dev-reference-authority`: these block a release/production key and the KMS custody needed by S9-5.
- `260711-1917-tier3b-x86-vtx`, `260712-0952-tier3b-vm-hardening-compat` and `260722-2330-tier3b-finish-g5-lite`: these block the Tier-3 guest endpoint.
- `261004-1957-x86-pc-lane` (P07 procurement gate) and the controlling plan's P10 (physical qualification): these block physical Intel claims.
- `260712-1000-cell-package-distribution`: this blocks any installer/cell-store distribution path. Phase 10 packages only the embedded VIFS1 image.
- `260712-1902-dice-attestation-identity` and `260821-1700-tier1-baseline-admission`: no attestation or production-admission claims.

S10-5 restates that all 38 relations remain. If D10-2 option C is chosen, it needs the locks file's unfreeze procedure and is out of this phase.

---

## 2. Slices

Order: **S10-0 → S10-1 → S10-2a → S10-3 → S10-4 → S10-5 (including S10-2b)**. Every canonical write happens last, as a hash-verified copy of bytes that were already qualified. CP structure:

- one CP1 per slice;
- a CP2 for S10-0 before S10-1 starts, so the candidate image carries the approved S10-0 bytes;
- a joint CP2 for S10-1 + S10-2a before any candidate image is signed;
- a joint CP2 for S10-3 + S10-4;
- a final CP2 for S10-5, which is the containment-lift decision itself.

### S10-0 — `/bin/cellbox <command>` for catalog commands (**CP1 approved 2026-10-26**: DS0-1..DS0-6 as recommended; **CP2 approved 2026-10-26**, including deviations 1–7 and the catalog digest; results in S10-0.11)

Added by the user's D10-11 decision (2026-10-26). Source change inside `cells/tools/cellbox` plus the Phase 04 terminal lane. No kernel, launch-profile, policy, syscall-allowlist or Cargo change.

#### S10-0.1 How the entry works today (read 2026-10-26)

- **Entry** (`cells/tools/cellbox/src/main.rs:126-142`): if `ChildContext::read_own()` succeeds and the first context argv word is `ssh` (`ssh.rs:24` `SSH_ENTRY`), run the ssh client child (`ssh.rs:168` → `child::run`, `ssh.rs:738`). In every other case run `run_shell()`.
- **Context argv excludes the program path.** The shell starts its ssh child with `["ssh", …]` (`ssh.rs:76-78`), the GUI Terminal with `["sh"]` (`cells/apps/terminal/src/session.rs:24-25`), the Phase 04 driver with `["sh"]` (`cells/tests/phase04-terminal-driver/src/main.rs:202`). The first context argv word is therefore the subcommand. Below, "argv[0]" means that word.
- **Context is read through the kernel session call `ReadOwnContext`** (`libs/ostd/src/child_context.rs:90-100`), not through `StateRestore`. The S1 finding that argv needs `StateRestore` applies only to the legacy stash (`ostd::args` → `sys_spawn_args`, `libs/ostd/src/syscall.rs:1819-1821`), and that syscall is not in the allowlist (`main.rs:27-68`). Init launches without a context, so an init-launched cellbox cannot read its advisory `args = ["sh"]` at all.
- **`run_shell()`** (`main.rs:153-190`, `context_session.rs:62-74`):
  - no context (`Denied`) → serial (or legacy compositor) shell;
  - a context with a frontend grant → frontend shell;
  - any other context → refused, exit 125. A context launch without a frontend therefore cannot run anything today.
- **Dispatch tables:**
  - `executor.rs:940-948` `BUILTINS`: 60 names, used for completion;
  - `dispatch_builtin` (`executor.rs:2119-2272`): a legacy table `2130-2163` (`ViResult` → 0/1) plus a match `2171-2234`;
  - **falls through to `spawn_external`** (`2268`) for any unknown name;
  - `[` and `.` are dispatched outside `BUILTINS`;
  - typed stages (`select`, `where`, `sort-by`, `take`, `to`, `from`) and `--records` producers are classified in `typed.rs:178-209`;
  - `ledger.rs:63-143` is the host-test-only disposition ledger (56 builtins + `edit`/`ssh`/`completion`/`rg` = 60, plus 11 sys-tool and 6 net-tool `RetainedWorker` rows).
- **Output paths:**
  - command stdout goes to `shell_state::write_out` (`shell_state.rs:68-90`): the innermost `CaptureGuard`, else `ostd::io::print`, which is the **legacy UART logger**;
  - diagnostics go to `executor::shell_stderr` (`executor.rs:503-511`): the session stderr when a session exists, else `ostd::io::print`;
  - the REPL captures each line, then writes it to the session (`main.rs:221-229`).
  - So without a session, every byte would reach the UART. This is the S3-output defect class.
- **Retained workers** write through `ostd::io::ProcessOutput` (`libs/ostd/src/io.rs:132-220`), which:
  - requires both Stdout and Stderr grants, otherwise refusing with no output;
  - uses a finite deadline per write; stock tools use 30 s (`cells/tools/sys-tools/src/bin/echo.rs:24-34`).
- **Typed default sink** (`typed.rs:17-19,814-824`): a terminal gets an aligned plain table; a redirect or non-terminal gets **typed JSONL**; `--text` selects the plain table (search text for `rg`).

#### S10-0.2 Who can reach it (same path, same rights)

| Launcher → `/bin/cellbox` | Edge (file:line) | Child ceiling | Context argv | Reaches one-shot? |
|---|---|---|---|---|
| init (boot/autoload) | `profiles.rs:16-60` with `boot_ceiling("/bin/cellbox")` | `SHELL_EDGE_Q` | none (no context) | No: the serial shell is unchanged |
| GUI Terminal | `profiles.rs:153-158` | `SHELL_EDGE` | fixed `["sh"]` | No: frontend shell unchanged |
| Phase 04 driver (fixture kernel only) | `profiles.rs:173-184` | `SHELL_EDGE` | any | **Yes**: the lane proof route |
| `cellbox` itself, desktop, others | none: `cellbox_profile` (`profiles.rs:63-80`) falls back to `reviewed_user_target_ceiling`, which has no `/bin/cellbox` row (`targets.rs:29-142`) | — | — | No |

- **No kernel row changes.** The one-shot child is the same ELF at the same path, with the same allowlist, the same path-requested caps (`config_write` included, unused by any one-shot command) and the same caller name `cellbox`, so it has the same launch edges as the shell.
- **What does change is a code-level rule: a one-shot never launches a child.** There is no `spawn_external` fallback, no `exec`, no `hv` and no ssh launch from a one-shot. One-shot authority is therefore a strict subset of `cellbox sh`. The kernel cannot tell a one-shot from a shell (same name), so this rule is enforced by the closed table and checked by tests.
- **On a canonical image, no production launcher reaches the process-level form**: init has no context, and the Terminal always passes `sh`. The explicit form is still reachable there through the in-shell prefix (S10-0.6). Process-level reach for new callers needs a separately reviewed launch edge (DS0-1).

#### S10-0.3 Launch classification (in `main`, after reading the context once)

| Context | argv | Result |
|---|---|---|
| none (`Denied`) | — | serial shell (unchanged) |
| unreadable (not `Denied`) | — | exit 125 (unchanged) |
| any | `ssh …` | ssh client child (unchanged and checked first). Under `/bin/cellbox` it has no network rights and no terminal, so it refuses as today. |
| frontend grant | `[]` or `["sh"]` | frontend shell (unchanged) |
| frontend grant | anything else | exit 2: `cellbox: a terminal frontend runs only 'sh'` on the holder's stderr writer |
| no frontend | `["sh", …]` | exit 125: the shell needs a terminal (today's refusal) |
| no frontend | `[]`, or argv[0] starting with `-` other than `-h`/`--help` | exit 2 with usage on stderr |
| no frontend | `["-h"]` or `["--help"]` | usage on stdout, exit 0 |
| no frontend | argv[0] in the one-shot table | run it (S10-0.4) |
| no frontend | anything else, including `/bin/ls` and retained-worker names | exit 127: `cellbox: NAME: unknown command`; **no external fallback** |

#### S10-0.4 Which catalog commands are runnable (one closed table, `oneshot.rs`)

The table covers every `BUILTINS` name plus `[`, `.` and the six stage words, each exactly once (62 + 6 entries).

| Class | Names | One-shot behavior |
|---|---|---|
| **Run** (42) | `awk` `blktest` `cat` `cp` `date` `echo` `edit` `env` `find` `free` `grep` `head` `help` `ifconfig` `ip` `kill` `ls` `mkdir` `mv` `ps` `pwd` `rg` `rm` `rmdir` `sed` `shutdown` `sleep` `snapshot` `sort` `tail` `tee` `test` `[` `top` `touch` `uname` `uniq` `uptime` `vappend` `vcat` `vwrite` `wc` | Same implementation and status as in the shell, through a closed `dispatch_catalog` that never reaches `spawn_external`. `kill`, `snapshot` and `shutdown` keep their refusals (status 1). `blktest` keeps its denied effect. `edit` without a terminal already exits 1 (`edit.rs:58-61`). Interactive `top` without a session already exits 2 naming `-b`; `top -b` and `top --records` run. |
| **State** (16): refused, exit 2 | `alias` `unalias` `cd` `export` `unset` `exit` `source` `.` `read` `history` `jobs` `fg` `bg` `break` `continue` `completion` | `cellbox: NAME changes or reads shell state; run it inside 'cellbox sh'` on stderr. No effect. A `KEY=VALUE` argv[0] or a shell-function name is not in the table, so it gets 127. |
| **Terminal** (1): refused, exit 2 | `clear` | `cellbox: clear needs an interactive terminal` (CBX-29: no screen control to a non-terminal) |
| **Launch** (2): refused, exit 2 | `exec` `hv` | `cellbox: NAME launches a Cell; run it inside 'cellbox sh'` |
| **Reserved** (1) | `ssh` | the existing child entry (S10-0.3), not dispatched here |
| **Stage** (6): refused, exit 2 | `select` `where` `sort-by` `take` `to` `from` | `cellbox: NAME is a pipeline stage; use 'cellbox sh'` |

- **No shell grammar.** argv is used verbatim: no parsing, variable expansion, globbing, redirection, `|`, `;` or `&&`. Empty and space-containing arguments survive. One invocation is one command; callers needing pipelines use `cellbox sh` or compose streams themselves.
- **Context:**
  - cwd comes from the context, defaulting to `/` when unset (`context_session::ensure_default_cwd`, as for frontend shells);
  - the context environment is not imported (`env` prints the same fixed list as in the shell);
  - `CELLBOX_STREAMS` is ignored: one-shot streams are always separate.

#### S10-0.5 Streams and status without an interactive shell

- **Output grants.** Installed before anything is written: one `ProcessOutput` from the same context.
  - Missing or invalid Stdout/Stderr grants → exit 1 with **no byte anywhere**, including the UART (S3-output negative-grant rule).
  - While installed, `write_out` (no capture) and `shell_stderr` route to these streams, never to `ostd::io::print`.
  - Each write has a finite 30 s deadline (the S3-output stock-tool value).
- **stdin.**
  - Read only by commands whose table row is a stdin consumer **and** that got no file operand: `cat`, `grep`, `wc`, `head`, `tail`, `sort`, `uniq`, `tee`, `sed`, `awk`, `rg -`.
  - It is read from the Stdin grant to EOF before dispatch and published through `shell_state::set_stdin`, as a pipe stage is.
  - The bound is `typed::MAX_SINK_BYTES` (4 MiB). If exceeded → exit 1 before the command runs, with `cellbox: stdin exceeds 4194304 bytes`.
  - No Stdin grant = empty input. Non-consumers never touch stdin, so an open producer cannot block `ls`.
- **Bytes.** stdout and stderr carry exactly what the command produces in the shell: the same byte-preserving `cat`/`tee` and the same text renderers. No banner, no prompt, no ANSI.
- **Typed records to a non-terminal.**
  - The default sink is **typed JSONL** (`DefaultSink::TypedJsonl`): one row, or one frame envelope, per line.
  - `--text` selects the plain table (search text for `rg`).
  - Typed producers (`rg`, `ps --records`, `ls --records`, `top --records`) **stream** item by item to stdout, through the existing per-item delivery used for search text (`typed.rs:1087-1137`), not a whole-buffer collection. A consumer close therefore stops the producer (CBX-08/18/20).
  - Legacy byte builtins keep their existing bounds (e.g. `CAT_READ_MAX` 1 MiB) and write through the routed stream.
- **Exit status** (`sys_exit`):
  - the command's own status, unchanged (catalog N: 0, 1, 2; `grep`/`rg`/`test` 0/1/2; refusals 1);
  - S10-0.3/0.4 refusals: 2;
  - unknown command: 127.
  - **Output failure** (stdout `Closed`/`Revoked`, or the deadline passing) stops typed producers at once, and makes legacy commands exit 1 after they return (a sticky flag). It never retries a committed prefix (`io.rs:196-199`). One line goes to stderr only if stderr is still writable (DS0-4).
  - **Cancellation**: the parent cancels the operation; the kernel ends the child, and the parent reports 130 (catalog N), as the shell already does for its children.
- **Never interactive.** `interactive_terminal()` is false (no session exists), so TUIs refuse truthfully (CBX-29). Nothing reads keys.

#### S10-0.6 In-shell explicit form (CBX-10 on canonical images)

- **Normalization.** Inside `cellbox sh`, a simple command whose first word is `cellbox` or `/bin/cellbox` is normalized **in process, without a spawn**, by the same table:
  - **Run** names: the prefix is dropped, so `cellbox ls /` is exactly `ls /`. It keeps shell grammar for its own line (redirects, pipelines, typed stages), since the shell parsed it.
  - Every other class: the same refusal and status as the one-shot (e.g. `cellbox cd /` → 2, cwd unchanged).
  - `cellbox` alone, or `cellbox sh` → 2.
- **Where it applies.** Normalization happens on the expanded argv before `typed::admit` and `exec_cmd`, so `cellbox ps --records | select tid` admits as typed. `typed::might_be_typed` (`typed.rs:203-207`), which looks at the **unexpanded** first word, must see through the prefix too.
- **Today** these lines are a denied external launch (no `cellbox → /bin/cellbox` edge) `[INFERENCE: spawn_external → cellbox_profile → None]`.

#### S10-0.7 Files

- `cells/tools/cellbox/src/main.rs` — read the context once; classification (S10-0.3) before `run_shell`.
- `cells/tools/cellbox/src/context_session.rs` — take the already-read context; frontend plus non-`sh` argv refused.
- `cells/tools/cellbox/src/oneshot.rs` (new) — table, classification, stdin policy, output install, status mapping, host tests.
- `cells/tools/cellbox/src/executor.rs` — closed `dispatch_catalog` split out of `dispatch_builtin` (the shell keeps its `spawn_external` fall-through); one-shot output mode in `shell_stderr`/`shell_console`; prefix normalization in `exec_cmd` and pipeline admission.
- `cells/tools/cellbox/src/shell_state.rs` — `write_out`/`write_out_bytes` route to the installed one-shot stdout instead of `ostd::io::print`.
- `cells/tools/cellbox/src/typed.rs` — prefix-aware `might_be_typed`/`classify_stage`; one-shot streamed typed-JSONL delivery.
- `cells/tests/phase04-terminal-driver/src/main.rs` — markers (S10-0.8).
- `scripts/qemu-phase04-terminal-test.sh` — marker list, verdict checks, fixture files, re-pinned digests.
- Docs: catalog §1/§3.1 one-shot column (a catalog revision with a new digest; needs your acknowledgement), `docs/cellbox-cli.md:36`, `CHANGELOG.md`.
- **Unchanged:** the kernel (launch profiles, boot ceiling, allowlist), `sign-policy.py`, `Cargo.toml`/`Cargo.lock`, `ledger.rs` data, and the Terminal.

#### S10-0.8 Tests and evidence

**Host** (`cargo test -p app-cellbox --target x86_64-unknown-linux-gnu`; 189 recorded at the Config CP2):

- the one-shot table covers `BUILTINS` ∪ {`[`, `.`} ∪ the stage words exactly once, and its builtin names equal the ledger's `Builtin` rows;
- every `SysTool`/`NetTool` name as an absolute path → 127;
- the S10-0.3 classification table, row by row;
- argv literal preservation (`""`, `"a b"`, `|`, `;`, `$HOME`, `*`, `>`);
- the stdin policy (consumers only; bound exceeded → 1 before dispatch);
- one-shot default sink = typed JSONL, `--text` → table, and no ESC byte in either;
- status mapping (output failure → 1, refusals → 2, unknown → 127);
- prefix normalization (`cellbox ls /` ≡ `ls /`; `/bin/cellbox cd /` → 2; `cellbox` / `cellbox sh` → 2; `might_be_typed` through the prefix);
- the dispatch path for unknown names never reaches the spawn path.

**Phase 04 terminal lane.** The driver launches `/bin/cellbox` directly with a prepared context, as `stock_negative_grants` does (`main.rs:997-1061`), on the fixture edge. New markers:

| Marker | Observes |
|---|---|
| `oneshot-echo-exact` | `["echo","a b",""]` → stdout `a b \n` exactly, empty stderr, exit 0 |
| `oneshot-argv-literal` | `["echo","$HOME","*","|",";"]` printed verbatim |
| `oneshot-ls-fixture` | exact entries of a fixture directory, exit 0 |
| `oneshot-cat-stdin-bytes` | Stdin grant with NUL and non-UTF-8 bytes → identical stdout |
| `oneshot-grep-status` | match 0 / no match 1 / bad pattern 2, with the bytes |
| `oneshot-records-jsonl` | `ps --records` → every line parses as typed JSONL, no ESC, exit 0 |
| `oneshot-records-text` | `ps --records --text` → plain table, no ESC |
| `oneshot-rg-records` | `rg` over the S7-4 fixture → records and status 0/1/2 |
| `oneshot-top-batch` | `top -b` one text frame, exit 0 |
| `oneshot-tui-refused` | `top` → 2 naming `-b`; `edit f` → 1; stdout has no ESC; no key read |
| `oneshot-state-refused` | `cd`, `export`, `exit`, `alias` → 2, message on stderr only, no effect |
| `oneshot-launch-refused` | `exec /bin/echo x`, `hv` → 2; task set unchanged (`ps --records` before/after) |
| `oneshot-stage-refused` | `select tid` → 2 |
| `oneshot-unknown-127` | `nope`, `/bin/ls` → 127; no child started |
| `oneshot-mutate-effect` | `mkdir`/`touch`/`rm` in a writable fixture path → the effect is observed by a later `ls` |
| `oneshot-denials-same-as-shell` | the S2-K secret read and the S2-T/W sandbox mutate are denied exactly as in the shell; bytes unchanged |
| `oneshot-stderr-separate` | a failing `cat missing` writes only stderr, exit 1 |
| `oneshot-missing-stdout` | a context without Stdout → exit ≠ 0, no byte on either stream or the UART |
| `oneshot-consumer-close` | the reader closes after the first chunk of `rg` on a >64 KiB file → the child exits within a bound, status 1 |
| `oneshot-cancel` | `op.cancel` during `sleep 30` → the child ends, 130 reported, no orphan in `ps` |
| `oneshot-frontend-argv-refused` | `host.launch(SHELL, ["ls"])` with a frontend → 2 |
| `shell-explicit-prefix` | in the shell: `cellbox echo hi` and `/bin/cellbox ls /p04` equal the short forms; `cellbox cd /` → 2 with cwd unchanged |
| `oneshot-not-on-uart` | no one-shot payload token appears on the serial log |

All existing terminal markers stay green (466 at the last run). Run once afterwards: P02 48, P03 67, GUI 290 (the Terminal argv is unchanged), P09 103 (the `ssh` entry is unchanged). The canonical L0 lane (S10-3) exercises only the in-shell prefix.

**Acceptance:** CBX-10 (explicit subcommand), CBX-29 (one-shot non-terminal never draws or reads keys), CBX-07 (JSONL without decoration), and CBX-11 (forged mode/alias: argv never grants authority). These are evidence for the S10-3 audit; S10-0 closes no row alone.

#### S10-0.9 Risks

- **Accidental external fallback.** `dispatch_builtin` ends in `spawn_external`. A one-shot named `cellbox` holds the shell's launch edges (`/bin/cellbox-ssh` with `NET_CLIENT`, `/bin/hypervisor`, `/bin/shutdown`, the reviewed table). The closed table, a host test and the `oneshot-unknown-127`/`oneshot-launch-refused` task-set checks guard this.
- **UART leakage.** Any byte written before the output mode is installed goes to `ostd::io::print` (the S3-output class). The output mode is installed first; `oneshot-not-on-uart` and `oneshot-missing-stdout` check it.
- **Cost per invocation.** Each one-shot is the whole ~3.08 MB ELF with the 6 MiB heap arena (bss about 7.3 MB), against about 20 KB for a stock worker. Startup and memory are measured in S10-4. Whether bss is committed at load is `[INFERENCE: unverified]`.
- **Identity.** Telemetry, `top` and name-keyed kernel checks see `cellbox` for both shell and one-shot. This is the same rights by design, but audit name-keyed checks on `cellbox` before CP2.
- **`ssh` is reserved as argv[0]** for the child entry: `/bin/cellbox ssh …` keeps today's behavior (no rights, no terminal → refusal 255). It is not a one-shot.
- **The stdin preload is not streaming.** A consumer reads up to 4 MiB to EOF before running, so an endless producer feeding `grep` blocks until cancelled. This is documented.
- **Typed-admission drift.** The prefix must be stripped consistently in `might_be_typed`, `admit` and `exec_cmd`, or `cellbox ps --records | select` could take the byte path. Host tests cover it.
- **Catalog freeze.** The one-shot column revises the frozen catalog `f2a82fce…`. It needs a new digest and your acknowledgement.

**Rollback:** `git revert` of the listed files. There is no state, kernel or image effect. Private lane evidence is retained.

#### S10-0.10 Decisions for S10-0 CP1

| ID | Question | Recommendation |
|---|---|---|
| DS0-1 | Process-level reach: no new launch edge (only context launchers that already exist, i.e. the fixture driver; the Terminal stays `sh`-only) **or** a new reviewed `cellbox → /bin/cellbox` Path edge (`SHELL_EDGE`, a kernel row, own CP) | **No new edge.** The in-shell prefix serves canonical images. |
| DS0-2 | In-shell `cellbox CMD` / `/bin/cellbox CMD`: in-process through the same table (refusing state commands as the one-shot does) **or** keep them as the denied external launch they are today | **In-process, same table**, so the explicit form means the same thing everywhere. |
| DS0-3 | Status for State/Terminal/Launch/Stage refusals: **2** (catalog "unsupported operand") **or** 126 ("denied/nonexecutable launch") | **2.** 127 stays for unknown names. |
| DS0-4 | Output failure (closed stdout or 30 s deadline): exit **1** like retained workers (S3-output) **or** 124 for a deadline | **1.** |
| DS0-5 | stdin: preload to EOF with the 4 MiB bound for consumers only **or** stream stdin into consumers (a larger executor change) | **Preload, bounded.** |
| DS0-6 | Record the one-shot column as a catalog revision (new digest) | **Yes.** |

#### S10-0.11 Results (implementation, 2026-10-26; for the S10-0 CP2)

**Files.** `cells/tools/cellbox/src/{main.rs, context_session.rs, oneshot.rs (new), executor.rs, shell_state.rs, typed.rs}`; `cells/tests/phase04-terminal-driver/src/{main.rs, oneshot.rs (new)}`; `scripts/qemu-phase04-terminal-test.sh`; `command-catalog.md` (§1, §3.1, new §3.2, revision log); `docs/cellbox-cli.md`; `CHANGELOG.md`; this file. Unchanged: kernel, launch profiles, allowlist, `Cargo.toml`/`Cargo.lock`, `ledger.rs` data, Terminal, libs. The P04 runner's pinned authority digests are unchanged (no pinned file changed), so nothing was re-pinned.

**Catalog revision (DS0-6).** `command-catalog.md` SHA-256 `2a4c3e6652c90b5dad840c5a403276a3a8fa4236b8bab12ca1b319bc237cbd70` (was `f2a82fce…5339`). The §3.2 block is host-tested against `oneshot::TABLE`. The user acknowledged it with the S10-0 CP2 on 2026-10-26.

**Evidence** (`reports/s10-0-evidence/`):

- **Host:** `cargo test -p app-cellbox --target x86_64-unknown-linux-gnu` 205 passed, 15 of them new (`host-tests.log`).
- **Builds:** `x86_64-unknown-none` release builds of `app-cellbox` (default and `shell_test`) and `app-phase04-terminal-driver` are green with no new warning.
- **Phase 04 terminal lane:** **494/494, 202 driver markers**, run `/tmp/cellbox-phase04-NiEn`. This includes 22 new markers plus the runner checks `oneshot-not-on-uart`, `oneshot-consumer-close-bounded` and 4 `host-exact:oneshot-*`.
- **Earlier runs** are kept in `phase04-earlier-runs/`: one infrastructure no-verdict (an NVMe I/O timeout before init read its config) and two runs that corrected driver expectations (below).
- **Reruns:** GUI 290/290; P02 48; P02 kernel regression cases PASS (VFS self-test 7/7); P03 67 PASS.
- **P09 ssh lane** run before CP2: **103/103**, `/tmp/cellbox-phase09-KSz1`, archived in `reports/s10-0-evidence/p09/` (`verdict.json`, `serial.log`, `sshd.log`).

**Deviations for CP2.**

1. **Builtin `cat`/`tee` are not byte-preserving** (S10-0.5 premise): they print stdin only when it is valid UTF-8 (NUL included) and a non-UTF-8 stdin prints nothing, as in the shell. Unchanged (changing it would weaken the shell's hostile-output path); `oneshot-cat-stdin-bytes` asserts UTF-8+NUL exact and non-UTF-8 → empty, 0.
2. **Consumer close is a kernel stop, not status 1.** When the final reader of a one-shot's stdout closes, the kernel closes the producer and cancels its operation (stop reason 3; a shell reports 141, `typed::reaped_status`). The child cannot write a stderr line. `oneshot-consumer-close` asserts reason 3 within 20 s with less than 64 KiB delivered. Status 1 (DS0-4) stays for write errors the child sees (deadline, revocation).
3. **stdin is read on the command's first demand**, not before dispatch: only consumer rows install the reader, and the builtin asks only when it got no file operand, so "no file operand" is the command's own decision; a bound overrun exits 1 at that demand, before the command uses any input. A deferred stdin is handed out once (a consumed stream), which also avoids a second 4 MiB copy in the 6 MiB arena.
4. **In-shell `cellbox ssh …` drops the prefix** (runs the shell's `ssh` builtin) instead of answering as the one-shot's child entry; `cellbox sh` in the shell → 2 (`SHELL_DOES_NOT_NEST`).
5. `classify_stage` is not prefix-aware: the executor normalizes the expanded argv (`explicit_stage`) before every `admit`, `background_refusal` and `exec_cmd`, so admission never sees the prefix; `might_be_typed` (unexpanded) is prefix-aware.
6. One-shot admission refusals (e.g. `rg -P`) print the shell's `shell: …` diagnostic; table refusals print `cellbox: …`.
7. Driver: a parent that keeps a stdin writer open after a non-consuming child exits is itself stopped by the kernel (same reason-3 rule, with its output rings); the driver therefore writes stdin that fits the ring before the commit. `oneshot-denials-same-as-shell` compares status and bytes with the shell; the legacy `vwrite` reports its refused write with status 0 in both. `oneshot-cancel` maps the cancelled record to 130.

**CP2 record.** The user approved the S10-0 CP2 on 2026-10-26 (relayed by Main). It covers:
- the as-built files above;
- deviations 1–7;
- the evidence above (P04 494/494, GUI 290, P02 48, kernel lane pass, P03 67, P09 103, host 205);
- the catalog revision `command-catalog.md` SHA-256 `2a4c3e6652c90b5dad840c5a403276a3a8fa4236b8bab12ca1b319bc237cbd70`, acknowledged as the current frozen catalog (DS0-6).

CBX-10 is met on private evidence; the canonical-image evidence is pending S10-3. Rollout stays blocked.

### S10-1 — canonical recipe cutover, no canonical bytes written (**CP1 approved 2026-10-26**: D11-1..D11-6 as recommended; **CP2 approved 2026-10-26**, including deviations 1–11; results in S10-1.7)

**Goal.** Every x86 canonical recipe produces one fresh image from source. That image has:
- `/bin/cellbox` and `/bin/cellbox-ssh` (the same signed file twice, D10-5);
- the D10-6 worker set;
- template-generated boot config;
- no `/bin/shell`, no `/bin/sshd` and no quarantined bytes (D10-7).

It fails on any build failure, missing artifact or stale input. In this slice the image is written only to `/tmp`.

Unchanged in this slice:
- G1 (`package-desktop-x86.py:18-24`), G2 (`signing.py`) and G3 (`make-iso-ci.sh`) stay exactly as they are; that is S10-2.
- No kernel, launch profile, policy row, Cargo manifest or `config/*.toml` change (D10-4 A).
- No ISO and no boot of a canonical-feature kernel.

#### S10-1.1 Current recipes (read 2026-10-26)

**`scripts/build-x86_64-cells.ps1`** (headless base; the documented entry for 8 integration-test SKIP hints and `ci-x86-integration.ps1:14,43`):

| Lines | Today | Problem |
|---|---|---|
| 65-71 | build `app-cellbox`; failure only `Write-Warning` | a failed build packages the stale `$buildDir/cellbox` |
| 73-93 | `service-vfs`, `service-config`, `service-input`: warn only | same |
| 95-112 | `service-net`: invalidates `$netSrc` **and the image** (`:100-107`), then `throw` on failure | fresh, but it deletes the existing image **before** the build. That would destroy the quarantined image that D10-14 says to archive. |
| 114-124 | the PCIe/driver set: warn only | stale |
| 128-134 | `app-sys-tools`: warn only | stale |
| 141-146 | `app-init --features input`: warn only | stale init |
| 149-155 | `tier2-smoke`/`tier2-exploit`: warn only | stale |
| 158-179 | the image list (no `/bin/cellbox-ssh`) | missing alias |
| 181-204 | one `cellos-sign` batch, dev key, including `cellbox` | refused by G2a today `[INFERENCE]` |
| 206-210 | copies init into `kernel/src/embedded-x86_64/init` **before** the image exists | partial output on a later failure |
| 211-230 | presence-only required list (no workers, no alias) | nothing proves the bytes are fresh |
| 232-251 | dev-signed `POLICY.BIN` via `sign-policy.py` | keep |
| 255-260 | `mkfat32.py --config-features input` straight into `kernel/src/embedded-x86_64/kernel_fs.img` | writes in place, not atomically |
| 262-273 | layout asserts only `POLICY.BIN` and `net` | no absence, alias or quarantine check |

**CI x86 job** (`.github/workflows/ci.yml`):
- `:915-933` build: cellbox, vfs, config, input, platform, nvme, e1000, igb, ahci, xhci, sys-tools; init `input`. No `service-net`, no `driver-serial-x86`, no tier2.
- `:943-976` `sign_cells` with the dev key, then `mkfat32` straight into `kernel/src/embedded-x86_64/kernel_fs.img`. Workers are `ls cat echo ps` only: no `kill`, `shutdown`, `net`, `serial` or alias.
- `:978-986` kernel and ISO (G3 refuses the ISO step).
- No assertion step.

**`scripts/package-desktop-x86.py`** (desktop overlay):
- `:25-27` paths;
- `:29-40` `apps_map`;
- `:42-45` presence check of **prebuilt** binaries in the canonical `target/` (it never builds them);
- `:47-52` dev-key signing of all of them, including `cellbox` and `terminal`;
- `:54-58` **reads the existing canonical image** and overlays it, which is how `/bin/shell`, `/bin/sshd`, `sshd` autostart and the quarantined bytes survive (§1.3);
- `:66-83` a hand-written autoload;
- `:85-103` atomic replace;
- init is never rebuilt with `ui`, and `services.toml` is never regenerated.

**`cells/tools/sys-tools/src/bin/env.rs:21`**: `SHELL=/bin/shell`. The cellbox builtin already prints `SHELL=/bin/cellbox` (`cmd_sys.rs:93`). No lane asserts the worker's text (repository grep). The terminal runner's sys-tools check (`qemu-phase04-terminal-test.sh:265-276`) compares only `declare_manifest!`/`declare_syscalls!` declarations, which this fix does not touch.

#### S10-1.2 Fresh-assembly design: one assembler, three thin callers

**New `scripts/assemble-x86-image.py`** (stdlib Python, importable for tests). It is the single source of the x86 image recipe (D11-1). Usage:

`assemble-x86-image.py --profile headless|desktop --out-dir DIR [--seed-file PATH] [--objcopy BIN]`

It runs these steps in order and stops on the first failure. Any failure exits non-zero, writes nothing to `--out-dir` and leaves an existing image there byte-identical.

1. **Resolve.**
   - The cargo target directory comes from `CARGO_TARGET_DIR` (default `target`); `REL = <target>/x86_64-unknown-none/release`.
   - Toolchain variables (`CC_*`, `CFLAGS_*`, `BINDGEN_*`) are the caller's, as they are today in the ps1 (`:23-48`) and in CI (`:917-922`).
   - The assembler **never sets** `CELLBOX_PRIVATE_FIXTURE_SIGNING` or `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI`.
2. **Invalidate inputs.** Delete every manifest binary and `app-init` in `REL`, then verify each is absent. This is the ps1 `:99-112` rule applied to every input, so a failed build can never leave a stale file to package. The out-dir image is **not** deleted (D11-4).
3. **Build.** `RUSTFLAGS="-C relocation-model=pic" cargo build --release --locked --target x86_64-unknown-none -Z build-std=core,alloc -p …`, then `-p app-init --features <profile features>`. A non-zero exit, a missing artifact or an artifact older than the step's start time → abort.
4. **Sign.**
   - One `scripts/cellos-sign --quiet --objcopy OBJCOPY --sign …` batch covering every signed cell and `app-init`.
   - `--seed-file` adds `--seed-hex` (private seed). Without it the dev key is used, the canonical default (D10-2).
   - The tier2 fixtures stay unsigned, as in ps1 `:185-187`.
   - G2 decides the outcome. Until S10-2a, a dev-key batch containing `cellbox` or `terminal` is refused before any target is written, so default runs stop here.
5. **Policy.** `sign-policy.py --out <tmp>`, unchanged: dev fleet key, root `/POLICY.BIN`.
6. **Input checks**, before any image byte exists:
   - the SHA-256 of every input file must not be in `signing.BLOCKED_ARTIFACT_SHA256` (imported from `scripts/cellos_sign/signing.py`, not copied);
   - `/bin/cellbox` and `/bin/cellbox-ssh` must come from the same host file;
   - the destination set must equal the profile manifest exactly.
7. **Image.** `tools/mkfat32.py --config-features <profile features> --config-output-dir <tmp>/boot-config <tmp>/kernel_fs.img <pairs…>`, writing into a temp directory **inside `--out-dir`** (same filesystem, so the final rename is atomic).
8. **Output checks** (read back with `generate-boot-config.py` `image_files()`):
   - the path set equals the manifest plus `/etc/cellos/{system,services,autoload}.toml`, with nothing extra and nothing missing;
   - every file is byte-equal to its input;
   - `/bin/cellbox` and `/bin/cellbox-ssh` are byte-equal;
   - no file hash is in the blocked set;
   - `/bin/shell` and `/bin/sshd` are absent;
   - `autoload.toml` has exactly one row with `path = "/bin/cellbox"`, `args = ["sh"]`, and no row for `/bin/shell`, `/bin/sshd` or `/bin/cellbox-ssh`;
   - `services.toml` has no `sshd` row and no `config` row (D10-4 A);
   - desktop profile only: `compositor` and `input` in services, `desktop` in autoload.
9. **Publish atomically.**
   - `os.replace` `kernel_fs.img` then `init` into `--out-dir`, plus `boot-config/` and `manifest.json`.
   - `manifest.json` records: the profile; the sha256 and size of every packaged file, of `kernel_fs.img` and of `init`; the key class (`dev` or `private`); `git rev-parse HEAD` plus a dirty-file digest; the cargo and rustc versions; the build start and end times.
   - S10-3 builds the kernel with `EMBEDDED_OVERRIDE=<out-dir>` and checks the image hash against this manifest. The kernel build stays outside the assembler, as today (ps1 `:277-279`).

**Profile manifests** (the exact image paths; the binary names are those cargo produces today):

| Profile | cargo `-p` | Image paths | init features |
|---|---|---|---|
| headless (= ps1 `:158-179` + alias) | `app-cellbox service-vfs service-config service-input service-net service-platform driver-nvme driver-e1000 driver-igb driver-ahci driver-xhci driver-serial-x86 app-sys-tools tier2-smoke tier2-exploit` | `/bin/{cellbox,cellbox-ssh,vfs,config,net,input,platform,nvme,e1000,igb,ahci,xhci,serial,ls,cat,echo,ps,kill,shutdown,tier2-smoke,tier2-exploit}`, `/POLICY.BIN` | `input` |
| desktop (= headless + `package-desktop-x86.py:29-40` + `fb-console`) | headless + `service-compositor fb-console desktop files terminal chrome ocel` | headless + `/bin/{compositor,fb-console,desktop,files,terminal,chrome,ocel}` | `input,ui` |

Notes on the manifests:
- Workers follow D10-6. `app-sys-tools` builds 11 binaries; only these 6 are packaged.
- `fb-console` is added because the template autoload starts it under `ui` and the GUI lane packages it (`qemu-phase04-gui-test.sh:276,289`).
- Not reproducible by any recipe, and therefore dropped (D11-2): `/bin/ocel-quickjs`, `/bin/vm-runner`, `/apps/chrome/chrome.{lua,toml}`, `/data/ocel-*` and `/welcome.md`.

**Callers:**

- **`build-x86_64-cells.ps1`:**
  - keeps `:1-63` (toolchain environment and Python discovery);
  - replaces `:65-276` with one call: `& $python scripts/assemble-x86-image.py --profile headless --out-dir $OutDir --objcopy $objcopyBin`, `throw` on a non-zero exit;
  - new parameters: `-OutDir` (default `kernel/src/embedded-x86_64`) and `-SeedFile` (optional, passed through);
  - the `:277-279` tail message is kept.
- **`ci.yml` x86 job:**
  - `:915-933` and `:943-976` become one step: `python3 scripts/assemble-x86-image.py --profile headless --out-dir kernel/src/embedded-x86_64`, with the existing environment block `:917-922` and the `cryptography` install `:939-941`;
  - this step stays red, refused by G2a, until S10-2b admits the CI route (§S10-2; recorded, not hidden);
  - `:978-986` are unchanged.
- **`package-desktop-x86.py`:**
  - G1 `:17-24` stays untouched (S10-2b);
  - the body below it, `:25-105`, becomes `assemble(profile="desktop", out_dir=…)` with `--out-dir`/`--seed-file` arguments.
  - The overlay of the existing image, the hand-written autoload and the prebuilt-binary dependency are removed (clean cutover, no fallback).

**`env` fix:** `cells/tools/sys-tools/src/bin/env.rs:21` → `SHELL=/bin/cellbox`. The ceiling and declarations are unchanged.

**Census** (read-only, recorded in evidence):
- `/bin/shell` appears in no recipe, CI step, sign list, launch row, config template or current doc. Allowed exceptions: historical `.agents`/changelog text and the lane negative tests that assert its absence or refusal (`qemu-phase0{2,3}-*.sh`, `qemu-phase04-gui-test.sh:399`).
- Every x86 canonical caller goes through the assembler.

#### S10-1.3 Scratch-build evidence plan (private seed, `/tmp` only)

1. `W=$(mktemp -d /tmp/cellbox-s10-1-XXXX)`. Record the canonical hashes (read-only): `kernel_fs.img` `447d0348…`, `embedded-x86_64/init` `0c2f607b…`, `target/…/cellos-kernel` `12cb0bac…`, `target/…/cellbox` `006b8504…`, `target/…/terminal` `018db0cc…`.
2. Generate a 32-byte private seed in `$W/keys/seed.hex` (mode 0600), as the lanes do (`qemu-phase04-terminal-test.sh:299-311`).
3. Headless run:
   - `CARGO_TARGET_DIR=$W/target CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 python3 scripts/assemble-x86-image.py --profile headless --out-dir $W/headless --seed-file $W/keys/seed.hex`;
   - the two variables are set on this command only.
4. Desktop run: the same command with `--profile desktop --out-dir $W/desktop`.
5. Wrapper equivalence:
   - `pwsh -NoProfile -File scripts/build-x86_64-cells.ps1 -OutDir $W/headless-ps1 -SeedFile $W/keys/seed.hex` with the same target directory and variables;
   - its `manifest.json` file set and hashes must equal step 3 (deterministic Ed25519 signing over the same rebuilt bytes; any difference is reported).
6. Layout evidence: `tools/inspect_fat.py` for both images, the verifier report, and `sha256sum` of `cellbox` versus `cellbox-ssh`.
7. Negative runs against the real assembler (scratch only):
   - an injected extra `/bin/shell` pair, a `/bin/sshd` pair and a blocked-hash input (via the test hook) → each refused, out-dir unchanged;
   - a dev-key run without `--seed-file` → G2a refusal before signing, out-dir unchanged.
8. Canonical hashes after == before. Archive under `reports/s10-1-evidence/` (no seed). `rm -rf $W`.
9. **Optional early boot smoke** (D11-3): build a `phase02-terminal-profile` kernel with `EMBEDDED_OVERRIDE=$W/headless` and `CELLOS_PHASE02_CELL_PUBLIC_KEY=$W/keys/cell.pub`, TCG, to `Cellos >`. Repeat for desktop to compositor ready. This tests only the ~24 MiB VIFS1 size risk early. It is not qualification (fixture kernel, private key); S10-3 does that.

#### S10-1.4 Tests

**New `scripts/test_assemble_x86_image.py`** (host, no cargo; fake builder, signer and policy hooks, real `mkfat32`/reader):
- the manifests are exactly the S10-1.2 tables; the alias shares one source; the worker set is D10-6;
- the output checks refuse: an extra `/bin/shell`; an extra `/bin/sshd`; a missing `/bin/cellbox-ssh`; alias bytes differing; a blocked hash (patched set) in any file; an autoload row for `/bin/shell`; a `services` row for `sshd`; a desktop image without `compositor`;
- a builder failure, a missing artifact or an artifact older than the build start → non-zero exit with the out-dir image byte-identical;
- a signer refusal (`SigningRefused`) → out-dir untouched;
- a successful run publishes `kernel_fs.img`, `init`, `boot-config` and `manifest.json` together, and the manifest hashes match the files;
- the invalidation step removes stale inputs before the build.

**Existing and other checks:**
- `python3 -B scripts/test_cellos_sign.py`: all 49 pass unchanged. G1 still exits; the packager test (`:390-399`) keeps its `ROLLOUT BLOCKED` assertion.
- `pwsh` parse of `build-x86_64-cells.ps1` (`[System.Management.Automation.Language.Parser]::ParseFile`); `python3 -m py_compile`; a YAML load of `ci.yml`.
- `cargo build --release -p app-sys-tools --target x86_64-unknown-none -Z build-std=core,alloc` in the scratch target directory (`env` fix).
- No lane rerun is needed for `env.rs` (no marker reads it). The P04 terminal lane runs once, as the S10-1 CP2 regression for the `/bin/env` bytes in its image list.

#### S10-1.5 Risks

- **Recipe refactor.** The ps1 becomes a wrapper. Windows and Linux `pwsh` callers and the integration-test SKIP hints keep the same entry, but the behaviour changes from warn-and-continue to fail-fast, which may surface previously masked build failures.
- **Toolchain parity.** The ps1 defaults `CC` to `clang` (`:29-35`) and CI to `cc` (`ci.yml:920`). The assembler inherits whichever the caller sets, so bytes can differ between hosts; the manifest records the versions. S10-3 qualifies the bytes it actually builds.
- **VIFS1 size.** About 18.4 MB of desktop payload makes a ~24 MiB auto-sized FAT, and the kernel ELF grows by the same amount. Boot headroom is unverified until D11-3 or S10-3.
- **Desktop composition change.** The template autoload (`config/autoload.toml`) replaces the hand-written one (D11-5):
  - desktop `restart = "never"`, `after = ["compositor"]` (was `always`, after `compositor`/`input`);
  - `fb-console` is started;
  - `cellbox` restarts `on-failure` (was `always`, after `compositor`/`input`).
- **Dropped residue (D11-2):** demo pages and the JS engine disappear from the canonical desktop image.
- **Ocel/Chrome on the default kernel (D11-6).** `build-cloud-images.sh` adds `large-cell-va` for the desktop Ocel profile. Whether `ocel`/`chrome` launch on a kernel without it is unverified `[INFERENCE]`.
- **The CI x86 job stays red** until S10-2b. This is already true today, at signing and at G3.
- **`--locked`.** It fails if `Cargo.lock` disagrees with the manifests. The lanes already build with `--locked --offline`.
- **Atomic replace instead of pre-delete.** A failed canonical run leaves the previous image in place rather than none. S10-3 and S10-5 therefore verify the image hash against `manifest.json` before any kernel build.

**Rollback:** `git revert` of `scripts/assemble-x86-image.py`, `scripts/test_assemble_x86_image.py`, `scripts/build-x86_64-cells.ps1`, `.github/workflows/ci.yml` (x86 job), `scripts/package-desktop-x86.py` (body below G1) and `cells/tools/sys-tools/src/bin/env.rs`. No canonical artifact is written; the scratch directory is deleted after archiving.

**Acceptance contribution:** Phase 04 criterion 4 (recipe half); CBX-10 (image list); CBX-26 (packaging half); CBX-28 (caller routes, `env`). None closes in this slice.

#### S10-1.6 Decisions for S10-1 CP1

| ID | Question | Recommendation |
|---|---|---|
| D11-1 | **One assembler** (`scripts/assemble-x86-image.py`) called by ps1, CI and the desktop packager, **or** edit the three recipes in place with a shared verifier only | **One assembler**: no list drift. |
| D11-2 | Residue that no recipe produces (`ocel-quickjs`, `vm-runner`, `/apps/chrome/*`, `/data/ocel-*` demos, `/welcome.md`): **drop**, **or** add explicit inputs (only the `/data/ocel-*` sources exist, in `tests/fixtures/`) | **Drop.** Owners may re-add through a reviewed recipe. |
| D11-3 | Optional early boot smoke of the scratch images on a fixture kernel (TCG, `/tmp`) to test the ~24 MiB size | **Yes**: cheap, catches the size risk before S10-3. |
| D11-4 | Out-dir write: **atomic replace** after all checks, **or** today's pre-delete of the image (ps1 `:100-107`) | **Atomic replace.** It keeps D10-14 archiving possible and leaves no half state. |
| D11-5 | Desktop boot config: **template-generated** (`input,ui`), **or** keep the hand-written autoload | **Template**: one source. Accept the restart and `fb-console` differences listed in S10-1.5. |
| D11-6 | `ocel`/`chrome` in the canonical desktop image: **keep** (as in `package-desktop-x86.py:29-40`; S10-3 adds launch smokes and settles the `large-cell-va` question), **or** drop them | **Keep**: no silent reduction of the independent apps. |

**CP1 record.** The user approved the S10-1 CP1 on 2026-10-26 (relayed by Main) with every recommendation: D11-1 one assembler, D11-2 drop the residue, D11-3 early boot smoke, D11-4 atomic replace, D11-5 template boot config, D11-6 keep `ocel`/`chrome`.

#### S10-1.7 Results (implementation, 2026-10-26; for the S10-1 CP2)

**Files.**
- New `scripts/assemble-x86-image.py`: the S10-1.2 steps 1–9 and the two profile tables.
- New `scripts/test_assemble_x86_image.py`: 33 host tests.
- `scripts/build-x86_64-cells.ps1`: toolchain block and Python discovery kept; new `-OutDir`/`-SeedFile`; one assembler call; tail message kept.
- `.github/workflows/ci.yml` (x86 job): the build and assemble steps become the `cryptography` install plus one assembler step with the existing environment block; kernel/ISO steps unchanged.
- `scripts/package-desktop-x86.py`: G1 untouched (still exits first); the body forwards `--out-dir`/`--seed-file`/`--objcopy` to the assembler with `--profile desktop`. The overlay, the hand-written autoload and the prebuilt-binary map are gone.
- `cells/tools/sys-tools/src/bin/env.rs`: `SHELL=/bin/cellbox`.
- `CHANGELOG.md` and this file.

Unchanged: G1–G5 (`signing.py`, `make-iso-ci.sh`, `kernel/build.rs`, lane guards), kernel, launch profiles, policy rows, `Cargo.toml`/`Cargo.lock`, `config/*.toml`, non-Intel recipes.

**Evidence** (`reports/s10-1-evidence/`, index in `summary.txt`). Scratch `/tmp/cellbox-s10-1-XXXX`, private per-run seed (mode 0600), the two signing variables on each command only. The scratch directory was deleted after archiving, and no seed is in the archive.
- **Headless:** `kernel_fs.img` 12,582,912 B `a770850d…2a94389`, 25 files; `init` `01ec7bf9…1842`; `/bin/cellbox` = `/bin/cellbox-ssh` 3,076,760 B `7b3b47b9…70b6`. Autoload: only `cellbox` `["sh"]` `on-failure`. Services: `e1000 igb serial xhci input net`.
- **Desktop:** 25,165,824 B `6858a766…f407eb90`, 32 files; alias pair 3,076,824 B `c23a3b72…16f0`. Autoload: `fb-console`, `desktop` (both after `compositor`, `never`), `cellbox` `["sh"]`. Services: the headless set plus `compositor`.
- **Read-back:** both images pass the step-8 checks and an independent read-back (`readback-verify.txt`): no `/bin/shell`, `/bin/sshd` or blocked hash; `boot-config/` equals the image config.
- **Wrapper:** `pwsh -File scripts/build-x86_64-cells.ps1 -OutDir … -SeedFile …` (pwsh on Linux) produced a manifest equal to the direct run in `files`, `kernel_fs_img` and `init`. Final-code reruns of both profiles were equal to the first runs.
- **Negative runs** (`scratch/negative/`): each exits 1 with the out-dir snapshot unchanged:
  - `/bin/shell` injected;
  - `/bin/sshd` injected;
  - a blocked-hash input (`--test-block-sha256` with the unsigned `tier2-smoke` hash);
  - a dev-key run without `--seed-file`: `REFUSED: ROLLOUT BLOCKED: unqualified Cellbox/terminal artifact`, before any key is loaded, and the release `cellbox` stays unsigned.
- **D11-3 boot smoke** (`phase02-terminal-profile`, private key, TCG, fixture only):
  - headless reaches `Cellos >` at 2.7 s;
  - desktop: `[compositor] Compositor v0.2` at 2.0 s, first scanout at 2.3 s, `[desktop] Ready` at 3.0 s, `Cellos >` at 5.0 s;
  - fixture kernel ELF: 15,595,648 B (headless) and 28,178,560 B (desktop); the 24 MiB VIFS1 boots.
- **P04 terminal lane:** **494/494, 202 driver markers**, `/tmp/cellbox-phase04-X5gq` (`phase04-lane/`).
- **Tests and builds:**
  - assembler tests 33 OK;
  - `test_cellos_sign.py` 49 OK, unchanged, with the packager still `ROLLOUT BLOCKED`;
  - `cargo test -p app-cellbox` (host) 205 passed;
  - `app-sys-tools` release build green with 0 warnings; the `env` bytes carry `SHELL=/bin/cellbox`;
  - `py_compile`, the ps1 `ParseFile` (0 errors), the `ci.yml` YAML load and `check-cargo-config-parity.py` all PASS.
- **Canonical artifacts unchanged** (`canonical/before|after.sha256`, embedded listing with mtimes identical):
  - `kernel_fs.img` `447d0348…298e2c`;
  - embedded `init` `0c2f607b…4104`;
  - `target/…/cellos-kernel` `12cb0bac…ac67`;
  - `target/…/cellbox` `006b8504…a1f4`;
  - `target/…/terminal` `018db0cc…30ad`;
  - `build/vicell-x86.iso` `eb1cb813…ef09`;
  - `build/cellos-x86_64.iso` `b9a08fcb…5b76`;
  - `build/x86-iso-root/boot/kernel.elf` `dbd862d6…7731`.
  No file under `target/`, `build/` or `kernel/src/` was written.

**Census** (`scratch/census-bin-shell.txt` plus a source grep). No recipe, CI step, sign list, launch row or config template names `/bin/shell`. It remains only in:
- the lane negative tests (P02/P03 autoload asserts, P03 `s1-old-shell-path-refused`, the GUI/terminal private-autoload asserts);
- the assembler's refusal list and its tests;
- a VFS read-table test path (`cells/services/vfs/src/access/tests.rs:14`);
- a historical comment (`cells/tools/cellbox/src/main.rs:22`);
- dated logs under `docs/evidence/` and the dated rpi3 run narrative in `docs/baremetal/load-cellos.md:243,656`.

`/bin/sshd` keeps its policy row (`NET_LISTENER`) in `sign-policy.py:184`; this slice changes no policy row, and neither image contains `/bin/sshd` or starts it. Whether the row stays is for S10-2/S10-5.

**Deviations for CP2.**
1. The cargo package for `/bin/chrome` is `app-chrome` (binary `chrome`); the S10-1.2 table said `chrome`. The first desktop run stopped at cargo (`package ID specification chrome did not match`) and published nothing.
2. Freshness is checked in four parts:
   - (a) the invalidation is verified;
   - (b) cargo exits 0;
   - (c) every artifact appears as an `executable` in that cargo run's JSON `compiler-artifact` messages;
   - (d) its POSIX `st_ctime` is not before a build-start marker.
   
   An mtime check would reject correct cached builds, because cargo re-links unchanged artifacts by hard link with their old mtime. Python on Windows has no change time, so there (d) is skipped and (a)–(c) carry freshness `[INFERENCE: not run on Windows]`.
3. Each profile builds its cells in one cargo invocation, as specified. Feature and metadata unification therefore makes all 20 headless cells differ in bytes between the headless and desktop images (from −120 to +224 B; the unsigned `tier2-*` differ too). Each image's own bytes are what S10-3 must qualify.
4. Two refusal-only test hooks on the real CLI:
   - `--test-inject HOST=/DEST`, which accepts only paths outside the manifest and is always refused;
   - `--test-block-sha256`, which only adds to the imported blocked set.
5. Checks added beyond S10-1.2:
   - `services.toml` also refuses a `shell` row; `autoload.toml` also refuses a second `cellbox` row;
   - `--seed-file` refuses the dev seed (so `key_class` cannot say `private` for dev bytes);
   - mkfat32 runs without `CELLOS_CONFIG_DIR`/`CELLOS_INIT_FEATURES` (template-only, D11-5) and cargo without `CARGO_ENCODED_RUSTFLAGS`;
   - inputs are copied into the work directory after signing, so the hashed, checked and packaged bytes are one set;
   - an out-dir the assembler created is removed on failure.
6. The manifest adds per-file package/binary/signed, `objcopy`, `rustflags` and the caller's toolchain variables. Its dirty digest covers `git status --untracked-files=all` (1,428 entries at the run).
7. A desktop image without `/bin/compositor` cannot be written at all: the template generator refuses `fb-console`/`desktop` `after = ["compositor"]`. The unit test asserts that refusal, plus the step-8 refusal of an image generated without `ui`.
8. `/POLICY.BIN` is signed with the dev **policy** key (`0x42`, S10-1.2 step 5, unchanged). Every cell and `init` was signed with the private seed; the dev **cell** key signed nothing.
9. The CI step keeps `CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS` from the existing block, although the assembler's `RUSTFLAGS` takes precedence.
10. The assembler needs Python ≥ 3.11 (`tomllib`, `hashlib.file_digest`).
11. The fixture-kernel boot logs show `[selftest] COMPLETION-QUEUE: FAIL`, which is pre-existing (also in `s8-2-evidence/phase02-kernel-serial.log` and `s3-gui-rerun-evidence`). It is not caused by this slice and is not fixed here.

CI x86 job: still red by design (G2a refuses the dev-key `cellbox` batch until S10-2b; G3 unchanged). Not run in CI. Rollout stays blocked.

**CP2 record.** The user approved the S10-1 CP2 on 2026-10-26 (relayed by Main). It covers:
- the as-built files above;
- deviations 1–11;
- the evidence in `reports/s10-1-evidence/`: P04 494/494; scratch headless and desktop images boot on the fixture kernel; canonical hashes unchanged.

§2 had planned one CP2 for S10-1 and S10-2a together. S10-1's CP2 was given on its own, so S10-2a gets its own CP2, before any dev-key candidate image is used in S10-3.

### S10-2 — guard change (reviewed source change; two stages)

#### S10-2a — dev-key signing of `cellbox`/`terminal` only through the named candidate routes (**CP1 approved 2026-10-26**: D2a-1..D2a-4 as recommended; **CP2 approved 2026-10-26**, including deviations 1–10; results in S10-2a.8)

**Goal.** After S10-2a, the S10-1 assembler can sign a candidate image with the **dev cell key** (D10-2), but only into an isolated candidate directory outside the repository. Every other dev-key signing of `cellbox`, `app-cellbox` or `terminal` stays refused, including:
- the canonical recipe route (`kernel/src/embedded-x86_64`, the CI step);
- the `cellos-sign` CLI;
- any environment variable.

The quarantined hashes stay refused forever. G1 (`package-desktop-x86.py:18-24`) and G3 (`make-iso-ci.sh:25-33`) do not change in S10-2a.

##### S10-2a.1 Guards today and the exact new rule

**`scripts/cellos_sign/signing.py`** (the S3-gui exception, uncommitted, +60 lines over HEAD):

| Line(s) | Today | After S10-2a |
|---|---|---|
| `34-35` | `CELLBOX_FIXTURE_ENV`, `PRIVATE_FIXTURE_NAMES = {cellbox, app-cellbox, terminal}` | unchanged |
| `36-39` | `BLOCKED_ARTIFACT_SHA256` = {`64eb28f3…2a39`, `636497ac…2fa2`} | **unchanged, forever** (S10-2a.3) |
| `42` | `DEV_SEED_HEX = "43"*32` | unchanged |
| `58-75` | `guard_prod_key`: a non-dev seed outside CI needs `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI` | unchanged (routes use the dev key only) |
| `90-99` | `private_fixture_allowed`: env `=1` **and** a non-dev seed | unchanged: the private-fixture path for lanes and S10-1 scratch runs |
| `102-122` | `preflight_blocked_artifacts(targets, seed)`: fixture names refused unless `private_fixture_allowed`; blocked hashes refused unconditionally | gains a keyword-only `route=None` parameter. Order: (1) the blocked-hash check for **every** target, first and unconditional; (2) fixture names admitted iff `private_fixture_allowed(seed)` **or** `route_admits(route, targets, seed)` (S10-2a.2) |
| `125-136` | `sign_and_verify(repo, targets, objcopy, seed)` preflights the whole batch before any key is loaded | gains `route=None`, passed to the preflight |
| `143-147` | after the key is loaded: the dev key never signs a fixture name | admitted **only** when the same `route` passed `route_admits` in this call; re-checked here, not trusted from the preflight |

**`scripts/cellos_sign/cli.py`:**
- `run_sign` (`:79-105`) and the parser (`:108-134`) are **unchanged**. The CLI gets no route option, so `cellos-sign --sign …/cellbox` with the dev key stays refused exactly as today.
- New `sign_route(repo, route, candidate_root, targets, objcopy) -> int` is a Python API only. It runs the same strict F1/F5 check as `run_sign` (`:87-92`; still no path to signing that skips it), then calls `signing.sign_and_verify(…, seed_hex=None, route=Route(route, candidate_root))`.

**`scripts/assemble-x86-image.py`:**
- `sign()` (`:289-299`): with `--seed-file`, unchanged (CLI subprocess, private-fixture path). Without `--seed-file` (dev key), it calls `cli.sign_route` in-process with route `x86-candidate-<profile>` and `candidate_root` = its work directory.
- **Copy, then sign** (D2a-3): `_assemble` `:414-429` today signs in `REL` (the cargo target directory) and then copies into `work/inputs`. It changes to copy first, then sign the copies in `work/inputs` (and `work/init`). The cargo target binaries stay unsigned, and every byte that is signed lives inside the candidate root.
- **Isolation checks before the build**, dev-key runs only (S10-2a.4): `--out-dir` and `CARGO_TARGET_DIR` (`:260-263`) must both resolve outside the repository.

##### S10-2a.2 How a named route is authenticated (no environment bypass)

`route_admits(route, targets, seed)` is true only if **every** condition holds. Any failure → `SigningRefused("ROLLOUT BLOCKED: …")` before a key is loaded or a target is written.

1. **In-source route table.** `route.name` is a key of `QUALIFIED_ROUTES`, a reviewed constant in `signing.py`:
   - `x86-candidate-headless` → recipe `scripts/assemble-x86-image.py`, recipe SHA-256 `<pinned>`, fixture names `{cellbox}`;
   - `x86-candidate-desktop` → the same recipe and pin, fixture names `{cellbox, terminal}`.

   Adding a route, or a name to a route, is a source change. No route admits `app-cellbox`, which is not an artifact name.
2. **Recipe binding.** The running program is the recipe: `sys.modules["__main__"].__file__`, after `realpath`, equals `<repo>/scripts/assemble-x86-image.py`.
   - Its file SHA-256 equals the pin in the table (D2a-1), so an edited assembler cannot sign `cellbox` until the pin is re-reviewed.
   - `ps1`, `package-desktop-x86.py` and CI all run the assembler as its own process, so this holds for every reviewed caller.
   - Imports from other programs, a REPL or `python -m` fail.
   - Tests patch the private `_route_caller()` lookup; production code has no parameter for it.
3. **Dev key only.** `seed is None`. A route call with any seed is refused; private seeds keep `private_fixture_allowed`.
4. **Names.**
   - the batch's fixture names ⊆ the route's fixture names (so `terminal` is refused on the headless route);
   - every target basename ∈ that profile's signed binaries ∪ {`app-init`};
   - names are compared on both `target.name` and the resolved name, as at `:111`.
5. **Candidate root.** `route.candidate_root`, after `realpath`, is outside the repository root, so it is never under `kernel/src/`, `target/` or `build/`, and it exists as a directory. A symlinked root that resolves into the repository is refused.
6. **Targets.** Each target:
   - is a regular file and not a symlink (`lstat`);
   - has `st_nlink == 1`, so in-place signing cannot write through a hard link into another tree, such as cargo's `deps/`;
   - after `realpath`, lies inside the candidate root.
7. **Quarantine.** Already checked in step (1) of the preflight, for every target and on every path.

**No environment bypass.**
- No new variable is read anywhere.
- `CELLBOX_PRIVATE_FIXTURE_SIGNING`, `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI` and the CI markers neither enable nor disable a route.
- A route is not reachable from the `cellos-sign` CLI.

This is still an accident guard, as the containment record says (`rollout-containment.md:28`): a developer who edits `signing.py` and the pin can do anything. What it buys is that dev-key `cellbox`/`terminal` signatures appear only from the reviewed recipe, byte-identical to its pin, writing outside the repository.

##### S10-2a.3 Quarantined hashes stay refused forever

- `BLOCKED_ARTIFACT_SHA256` keeps exactly its two values. A test pins both literals and `len == 2`. Any edit to that constant fails `test_cellos_sign.py` and needs its own explicit approval (which is not proposed).
- The hash check runs first for every target, under every name, seed, environment and route, and also at `:143-147` after the key is loaded.
- The assembler also refuses the blocked set before (step 6) and after (step 8) packaging, using the imported constant (S10-1).

##### S10-2a.4 Candidate isolation (canonical paths never written)

- **Signing:** with copy-then-sign, the only files the dev key writes are under `<out-dir>/.assemble-*/inputs|init`, and the route refuses any target outside that root.
- **Assembler:** on dev-key runs, it refuses before invalidation or build when `--out-dir` or the cargo target directory resolves inside the repository. This includes the default `kernel/src/embedded-x86_64` and `target/`, and therefore the CI step and a default ps1 run. The out-dir check uses `realpath`, so symlinks into the repository are refused too.
- **Unchanged guards:** G1 still exits, and G3 still refuses the default kernel. The kernel build with `EMBEDDED_OVERRIDE=<candidate out-dir>` and an explicit `X86_KERNEL` belongs to S10-3, outside the repository.
- **Evidence of isolation:** canonical hashes before and after the S10-2a smoke runs, plus a listing of `target/`, `build/` and `kernel/src/` showing identical mtimes.

##### S10-2a.5 Tests

**`scripts/test_cellos_sign.py`** (49 today; the existing tests keep their assertions, and only `sign_and_verify`/`preflight` calls gain the keyword where needed).

*Positive:*
- the headless route signs a batch of `{cellbox, platform, …, app-init}` under a temporary candidate root outside the repository with the dev key; the caller is patched to the recipe with a matching pin. The run reaches the signer, and with real `objcopy` the signature verifies against the dev public key;
- the desktop route admits `terminal`.

*Negative* (each asserts `ROLLOUT BLOCKED`, that the signer is not reached, and that every target is byte-unchanged):
- no route plus the dev key, for each of `cellbox`/`app-cellbox`/`terminal` (existing test);
- an unknown route name;
- `terminal` on the headless route; `app-cellbox` on any route; a non-profile basename in a route batch;
- the caller is not the recipe (the default `__main__` of the test runner); the recipe digest is mismatched;
- a private seed or the explicit dev seed hex with a route;
- the candidate root is inside the repository (`kernel/src/embedded-x86_64`, `target/`, `build/`), or is a symlink resolving into the repository;
- a target is outside the root, is a symlink, or is hard-linked (`st_nlink == 2`);
- a quarantined hash, via the patched set, on each route, with `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` and `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1` also set;
- **no environment bypass:** with every relevant variable set (fixture, prod-override, `GITHUB_ACTIONS`, `CELLOS_SIGN_CI`) and no route, the dev key still refuses `cellbox`;
- the `cellos-sign` CLI rejects `--route` (argparse exit 2), and a dev `--sign …/cellbox` still exits 3;
- batch atomicity: one bad target in a route batch leaves the earlier targets unchanged;
- `BLOCKED_ARTIFACT_SHA256` equals the two literals exactly;
- G1 and G3: the existing refusal tests are unchanged.

**`scripts/test_assemble_x86_image.py`** (33 today):
- a dev-key run with `--out-dir` inside the repository or `CARGO_TARGET_DIR` inside the repository → refused before invalidation and build;
- copy-then-sign: the fake signer sees only paths under the work directory, and the cargo-target fakes stay unsigned;
- the `--seed-file` path is unchanged.

**Real smoke** (scratch `/tmp/cellbox-s10-2a-XXXX`, out-dir and `CARGO_TARGET_DIR` both outside the repository):
- both profiles with the **dev key** → images published with `key_class: dev`;
- every cell signature verifies against the dev public key (`sign-cell.py` verify);
- real refusals, each exiting non-zero:
  - `assemble-x86-image.py --profile headless --out-dir kernel/src/embedded-x86_64` → refused before build;
  - `cellos-sign --sign <scratch>/cellbox` with the dev key → exit 3;
  - the same with all the variables set → exit 3;
- canonical hashes before and after (§1.2 list plus the S10-1 additions).

Booting dev-key images needs a canonical-feature kernel and belongs to S10-3.

##### S10-2a.6 Risks

- **Accident guard only.** It cannot stop an edit to `signing.py`, a copied dev-key-signed file, or a manual copy into an image (`rollout-containment.md:28`). The dev cell key is a public constant (`43×32`), so a dev-key signature never meant secrecy; the guard only prevents accidental release.
- **Pin friction.** Every edit to the assembler needs a re-reviewed pin in `signing.py` (like the runner's authority pins). The S10-2a change itself pins the final assembler.
- **`__main__` binding** refuses legitimate but unusual invocations: `python -m`, IDE debuggers, or an import from another tool. This is by design; the error names the expected recipe.
- **Windows.** The `realpath`, `st_nlink` and `lstat` semantics on NTFS are untested `[INFERENCE]`; the ps1 wrapper runs on Linux `pwsh` in evidence.
- **Dev-key-signed candidate bytes now exist outside the repository** (`/tmp`). They are evidence for S10-3 and must be recorded by hash, kept only as long as needed and never copied into a canonical path before S10-5.
- **Copy-then-sign changes S10-1 behaviour.** The cargo target binaries stay unsigned. No recipe consumes signed binaries from the target directory any more (S10-1 census).
- **CI stays red** until S10-2b. That is unchanged and expected.

**Rollback:** `git revert` of `signing.py`, `cli.py`, `assemble-x86-image.py` and both test files. This returns to the S10-1 state: dev-key `cellbox`/`terminal` signing is refused everywhere, and the quarantined hashes are refused unchanged. Delete the scratch candidate directories after archiving their hashes.

**Acceptance:** none directly. It enables S10-3 (CBX-26).

##### S10-2a.7 Decisions for S10-2a CP1

| ID | Question | Recommendation |
|---|---|---|
| D2a-1 | Route authentication: **in-source table + recipe binding (`__main__` path) + pinned recipe SHA-256**, or the table plus path binding only (no pin), or a `cellos-sign --route` CLI flag | **Table + binding + pin.** A CLI flag is rejected (it is a CLI bypass surface). |
| D2a-2 | Candidate location: **outside the repository** (out-dir and cargo target dir), or a dedicated ignored directory inside it (e.g. `build/candidates/`) | **Outside the repository**: the simplest rule, and it matches the lane alias guards. |
| D2a-3 | **Copy, then sign** inside the out-dir work directory (the cargo target stays unsigned), or sign in place in an isolated `CARGO_TARGET_DIR` | **Copy, then sign.** |
| D2a-4 | Routes are **dev key only**, or routes also accept private seeds | **Dev key only.** Private seeds already have the fixture path. |

**CP1 record.** The user approved the S10-2a CP1 on 2026-10-26 (relayed by Main) with every recommendation:
- D2a-1: table + recipe binding + pin;
- D2a-2: candidates outside the repository;
- D2a-3: copy, then sign;
- D2a-4: dev key only on routes.

##### S10-2a.8 Results (implementation, 2026-10-26; for the S10-2a CP2)

**Files** (as-built SHA-256):
- `scripts/cellos_sign/signing.py` (`723ab6bf…56bc`):
  - adds `REPO_ROOT`, `QualifiedRoute`, `QUALIFIED_ROUTES` (`x86-candidate-headless` `{cellbox}`, `x86-candidate-desktop` `{cellbox, terminal}`; recipe `scripts/assemble-x86-image.py`; pin `X86_CANDIDATE_RECIPE_SHA256`; each route's signed binaries plus `app-init`), `Route`, `_route_caller()` and `route_admits()`;
  - `preflight_blocked_artifacts(…, *, route=None)` runs the blocked-hash check first for every target, then route admission or the unchanged private-fixture rule;
  - `sign_and_verify(…, *, route=None)` re-checks the blocked hashes and the route after the signer is loaded and before the key is derived; without a route the old dev-key refusal stays;
  - `BLOCKED_ARTIFACT_SHA256`, `private_fixture_allowed`, `guard_prod_key` and the env names are unchanged.
- `scripts/cellos_sign/cli.py` (`ed92c59a…6efa`): new Python-only `sign_route(repo, route, candidate_root, targets, objcopy)`, which runs the strict F1/F5 check first. `run_sign` and the parser are unchanged, so there is no `--route` option.
- `scripts/assemble-x86-image.py` (`6d22da98…94a5` = the pin):
  - copy, then sign: cells go to `work/inputs/<binary>` and init to `work/inputs/app-init`, the copies are signed, and the cargo target stays unsigned;
  - dev key → `signing_cli.sign_route(…, "x86-candidate-<profile>", work, …)` in-process; `--seed-file` → the CLI subprocess, as before;
  - `check_isolation()` refuses a dev-key `--out-dir` or `CARGO_TARGET_DIR` inside the repository (`realpath`) before invalidation or build;
  - the manifest adds `signing_route`.
- `scripts/package-desktop-x86.py` (`286bef7a…cabcc`): G1 is untouched. The body below it now runs the assembler as its own process (deviation 1).
- `scripts/test_cellos_sign.py`: 49 → 66 tests. `scripts/test_assemble_x86_image.py`: 33 → 38 tests.
- `CHANGELOG.md` and this file.

Unchanged: `make-iso-ci.sh` (G3), G1, the blocked set, `kernel/build.rs`, the lane guards, kernel, config, Cargo.

**Evidence** (`reports/s10-2a-evidence/`, index in `summary.txt`). Scratch `/tmp/cellbox-s10-2a-XXXX` with out-dir and `CARGO_TARGET_DIR` inside it; no fixture or override variable on the candidate runs; deleted after archiving, and the candidates are recorded by hash.
- **Tests:**
  - `test_cellos_sign.py` 66 OK: the 49 existing tests unchanged, plus 17 new positive and negative route tests from S10-2a.5, including a real `objcopy` round trip that `sign-cell.py --verify` accepts with the dev key; 0 skipped;
  - `test_assemble_x86_image.py` 38 OK, 5 new:
    - in-repository out-dir or target directory refused with no cargo call;
    - the private-seed rule unchanged;
    - copy-then-sign leaves the cargo target unsigned;
    - real sign-hook dispatch;
    - the route table equals the profiles.
- **Dev-key candidates** (`key_class: dev`, `signing_route` set):
  - headless `kernel_fs.img` `fed9ce5b…2b99` (12,582,912 B), 19 cells signed on `x86-candidate-headless`;
  - desktop `5726b180…1f69` (25,165,824 B), 26 cells signed on `x86-candidate-desktop`;
  - each passed the step-8 read-back.
- **Signatures:** `sign-cell.py --verify` (dev public key) passes for every packaged signed cell and `init` in both images. The `tier2-*` fixtures stay unsigned, and the 28 cargo-target binaries carry no valid signature. The alias pair is byte-equal; there is no `/bin/shell`, `/bin/sshd` or blocked hash.
- **Real refusals** (`scratch/refusals/summary.txt`):
  - `--out-dir kernel/src/embedded-x86_64` (dev key) → exit 1 before any cargo build; the scratch release binaries are not invalidated;
  - `cellos-sign --sign <scratch>/cellbox` (dev) → exit 3 `ROLLOUT BLOCKED`, and exit 3 again with `CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 GITHUB_ACTIONS=true CELLOS_SIGN_CI=1`;
  - `cellos-sign --route …` → exit 2 (unrecognized argument);
  - the assembler imported and run from another program → refused by the route (`running program is not the recipe`), with no out-dir created;
  - `package-desktop-x86.py` → G1 `ROLLOUT BLOCKED`.
- **Boot smoke on the matching kernel** (requested by Main for this slice; S10-2a.5 had placed it in S10-3; not qualification):
  - a default-feature kernel (`dev-signing-key`, `dev-policy-key`, `signing-required`) per candidate, built with `EMBEDDED_OVERRIDE=<candidate>` into a `/tmp` target directory, with an explicit `X86_KERNEL` ISO in `/tmp`, under TCG;
  - headless: policy verified at 1.2 s, `Cellos >` at 3.3 s, and an `echo` round trip through the shell;
  - desktop: compositor at 1.9 s, first scanout at 2.2 s, `[desktop] Ready` at 2.9 s, `Cellos >` at 5.0 s, `echo` round trip;
  - no panic, signature or spawn-failure lines.
- **Canonical artifacts unchanged** (`canonical/`). The before and after hashes are identical for `kernel_fs.img` `447d0348…`, embedded `init` `0c2f607b…`, `target/…/{cellos-kernel 12cb0bac…, cellbox 006b8504…, terminal 018db0cc…}`, `build/vicell-x86.iso` `eb1cb813…`, `build/cellos-x86_64.iso` `b9a08fcb…`, the desktop fixture ISO `6af523ec…`, the wip qcow2 `9aa29105…` and the iso-root `kernel.elf` `dbd862d6…`. The listings with mtimes of the embedded directory, `target/x86_64-unknown-none/release/` and `build/` are identical, and no file under `target/`, `build/` or `kernel/src/` is newer than the start marker.

**Deviations for CP2.**
1. **`package-desktop-x86.py` body** (outside the S10-2a file list; G1 untouched). The S10-1 body loaded the assembler in-process through `importlib` without registering it in `sys.modules`. That fails at import (the `dataclass` lookup) and could never pass the route binding. The body now runs the assembler as a subprocess, as S10-2a.2 assumed. The defect was unreachable behind G1, and the S10-1 evidence never ran it.
2. **Route signature.** `route_admits` raises `SigningRefused` (with a reason) instead of returning a bool. It resolves the recipe against `signing.REPO_ROOT` (the repository holding the reviewed module), never a caller-supplied root.
3. **`init` is signed as `work/inputs/app-init`**, so its basename is the route's `app-init`, and is published as `<out-dir>/init`.
4. **Post-load re-check.** The blocked-hash and route re-checks run after the signer module loads and before the key is derived from the seed. The test asserts that `_priv_from_seed` is never reached.
5. **The route table duplicates the signed-binary sets.** A test in `test_assemble_x86_image.py` keeps them equal to the profiles, and another test pins `X86_CANDIDATE_RECIPE_SHA256` to the assembler file, so any assembler edit fails the signing tests until it is re-pinned.
6. **Test-only file reads.** The real-objcopy route test copies `target/x86_64-unknown-none/release/cellbox` (read-only, hash checked unchanged) into a temporary root; it is skipped when that file is absent. Root-inside-repository tests name the existing `kernel/src/embedded-x86_64`, `target/` and `build/` directories without writing to them.
7. **The boot smoke ran here, not in S10-3**, at Main's request: dev-key candidates on a default-feature kernel, everything in `/tmp`. It is not lane or qualification evidence.
8. **Refusal (b) not run for real.** A dev-key run with the default `CARGO_TARGET_DIR` (repository `target/`) was not run, because a regression there would invalidate canonical release binaries. It is covered by the unit test (no cargo call, stale file kept).
9. **One evidence-file correction.** The scratch `refusals/summary.txt` first recorded a harness error for the import-caller case (the harness did not register the module). It was corrected with `sed` on the scratch copy after a correct rerun; `import-caller.log` is that rerun.
10. **CI comment updated** (`.github/workflows/ci.yml`, x86 assemble step; comment only). The step now fails at the assembler's in-repository refusal before building, no longer at the signer, so the comment says so. The command and env are unchanged.

CI x86 job: still red (the in-repository out-dir is refused until S10-2b). Rollout stays blocked.

**CP2 record.** The user approved the S10-2a CP2 on 2026-10-26 (relayed by Main). It covers:
- the as-built files and digests above;
- deviations 1–10;
- the evidence in `reports/s10-2a-evidence/`: signing tests 66; assembler tests 38; both dev-key routes verified in `/tmp`; boot smoke OK; canonical hashes unchanged.

Deviation 9, the `sed` correction of the scratch `refusals/summary.txt` after a correct rerun, was disclosed and accepted; `import-caller.log` holds the rerun. From now on, evidence summaries are regenerated by the harness, not edited by hand.

#### S10-2b (inside S10-5, only after the S10-3/S10-4 CP2)

- Replace G1's unconditional exit with positive preconditions: fresh-base assembly, alias present, no residue, no quarantined hash in any packaged file.
- Change G3 so the default kernel is admitted only when its embedded `kernel_fs.img` contains no quarantined hash and no `/bin/shell` or `/bin/sshd`. A small Python helper reads the image the kernel was built from; it does not scan the ELF.
- Non-Intel routes stay refused (per D10-10).
- The assembler's canonical route (`assemble-x86-image.py` writing `kernel/src/embedded-x86_64`, which the CI x86 job uses, S10-1.2) is admitted here, not in S10-2a. Until then that CI step stays red. It is added as a route-table entry with its own root rule.
- **Files:** `scripts/cellos_sign/signing.py` (canonical route entry), `scripts/package-desktop-x86.py` (G1), `scripts/x86/make-iso-ci.sh` (G3), `scripts/assemble-x86-image.py` (in-repository out-dir admitted for that route only), and the tests.
- **Risk:** reopening G3 reopens 12 CI integration lanes that will boot the new image.
- **Rollback:** revert those files; containment returns exactly.
- **Acceptance:** none directly. S10-2b plus the release record changes the containment state.

### S10-3 — signed candidate image and lane battery (**CP1 approved 2026-10-26; CP2 approved 2026-10-28**, including S10-3.14's full `4fae8b20…` battery and recorded deviations; canonical rollout still blocked)

**Goal.** Build the exact bytes S10-5 would promote, then qualify those bytes:
- the headless and desktop images, kernels and ISOs, all built with the dev key through the S10-2a routes, outside the repository;
- on serial and GUI, under TCG;
- with every existing lane counted only where its binaries are proven to be the candidate's.

No canonical path is written, and G1/G3 stay closed. Product source is not changed; a defect found stops the slice and gets its own CP1.

#### S10-3.1 Candidate build (scratch root `W=/tmp/cellbox-s10-3-XXXX`, outside the repository)

| Step | Command (exact source state recorded) | Recorded by hash |
|---|---|---|
| C0 | canonical hashes and listings before (the S10-2a.8 list) | `canonical/before.sha256` |
| C1 | `CARGO_TARGET_DIR=$W/target python3 scripts/assemble-x86-image.py --profile headless --out-dir $W/cand/headless` (dev key, route `x86-candidate-headless`; no seed, no fixture or override variable) | `manifest.json`, `kernel_fs.img`, `init`, every packaged file |
| C2 | the same with `--profile desktop --out-dir $W/cand/desktop` (route `x86-candidate-desktop`) | the same |
| C3 | kernel per profile: the CI kernel command (`ci.yml` kernel step: `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc`, **default features**), with only `EMBEDDED_OVERRIDE=$W/cand/<p>` and `CARGO_TARGET_DIR=$W/ktarget-<p>` added | kernel ELF; proof that the ELF embeds the exact `kernel_fs.img` and `init` bytes (byte search, as in §1.2) |
| C4 | ISO per profile: `X86_KERNEL=<C3 kernel> X86_ISO_ROOT=$W/iso-root-<p> bash scripts/x86/make-iso-ci.sh $W/cand/<p>.iso` (G3 admits an explicit non-default kernel) | ISO |
| C5 | PC-profile ISO for the `iommu-dmar-x86` consumer: the headless image with `--features board-x86-pc`, as CI does (`ci.yml:1040-1046`) | kernel and ISO |
| C6 | `candidate.json`, the single record of the candidate | itself; its SHA-256 is quoted in the CP2 |

`candidate.json` records, per profile:
- git HEAD and the dirty-file digest; the toolchain; the pinned assembler `6d22da98…`, `signing.py`, `cli.py`;
- `manifest.json`;
- for every packaged file: its **sha256** and its **signed-payload sha256** (`sign-cell.py` `_signed_payload`, the ELF minus the 64 signature bytes; `null` for the unsigned `tier2-*` and `POLICY.BIN`);
- `init`, `kernel_fs.img`, the kernel ELF with its feature list, and the ISO.

Every later step verifies its inputs against this file and refuses on any mismatch.

The candidate is **retained** for S10-5 (D3-3), because S10-5 promotes these exact bytes and a rebuild would give different ones (S10-1 deviation 3).

#### S10-3.2 L0 — the new canonical runner (exact candidate bytes, no fixture kernel)

**Files** (new):
- `scripts/qemu-phase10-canonical-test.sh`: setup, guards, QEMU ownership, verdict;
- `scripts/phase10_canonical_drive.py`: the serial and GUI driver. The GUI oracle is imported from `scripts/phase04_gui_drive.py` (pixel-exact `screendump` against the `phase04-gui-glyphs` model, with negative controls), not copied.
- registered in `verification.md`.

**Runner rules:**
- **Input checks:** the ISO, kernel and image hashes must equal `candidate.json`, or the runner refuses. Work directories that alias `kernel/src/`, `target/` or `build/` are refused, as in the lane guards (`qemu-phase04-gui-test.sh:137-140`).
- **One QEMU per run,** with private unix QMP and serial sockets. The ISO is attached read-only.
- **Network:** no `hostfwd` and no inbound port. `-netdev user` uses `restrict=on`, except in the ssh scenario, which uses the D79-1 fixture (Docker OpenSSH on `127.0.0.1:<random>`, reached as `10.0.2.2`) with `-cpu qemu64,+rdrand,+rdseed`.
- **Data** comes only from disposable NVMe fixture disks built in `$W` with the existing tools:
  - `scripts/phase07_rg_fixture.py`: the >64 KiB multi-chunk files and the ignore, hidden, binary and denied cases;
  - `scripts/phase09_trust_disk.py`: the protected trust store (D79-10);
  - a FAT data volume for `cat`/`edit`/VT fixtures (the bytes from `qemu-phase04-gui-test.sh:307-341` and `phase05_vt_hostile.py`).

  Mount points are as in the P04/P09 lanes `[INFERENCE: confirmed while writing the runner]`. The image itself carries no fixture.
- **Output:** `verdict.json` (one record per marker), serial logs, judged frames, `provenance.json` (candidate hashes, QEMU version and arguments, disk hashes) and `evidence.sha256`. A no-secret check runs on every log: the trust seed, the fixture password and private keys must be absent.
- **TCG is mandatory.** KVM runs are recorded separately and never replace TCG.

**Headless (serial `cellbox sh` on the physical UART, headless ISO).** Markers `P10C-*`:

| Marker | Observes | CBX |
|---|---|---|
| `P10C-boot` | policy verified, signatures admitted, `Cellos >` at time T, no panic/`DENY`/spawn failure; `ps --records` shows exactly the services and autoload of `manifest.json`; no `sshd`, no `shell` | 26, 10 |
| `P10C-image-list` | `ls /bin` equals the manifest's `/bin` set; `/bin/cellbox` and `/bin/cellbox-ssh` present | 10, 26 |
| `P10C-explicit-forms` | `ls /`, `cellbox ls /`, `/bin/cellbox ls /` give equal bytes and status; `cellbox cd /` → 2 | 10 |
| `P10C-state-script` | variables, functions (there is no `return` builtin; a function ends at its last command), `if`/loops, `source` of a disk script, `$?`, `exit` codes; the unsupported `a \| b &` and `a & b` forms are refused before effect (a plain `cmd &` runs in the background) | 09 |
| `P10C-typed` | `ps --records \| where … \| select … \| sort-by … \| take …`, `to jsonl`/`from jsonl` round trip, a bad type refused before effect, no ANSI in JSONL | 06, 07 |
| `P10C-bytes` | `cat` of a disk fixture through a raw external stage (`/bin/cat`, `/bin/echo` workers): exact bytes and status | 04, 08 |
| `P10C-rg` | multi-chunk `rg` over the fixture: records, non-UTF-8 lines kept, partial-error status 2, `take 1` stops the walk | 19, 20, 08 |
| `P10C-top` | `top -b` and `top --records \| take --frame 2` (bounded frames); interactive `top` on the serial session draws and `q` restores (S6-4 form) | 17, 18, 13, 29 |
| `P10C-edit` | `edit` create, then save, then reopen equal bytes and line endings, on the data volume; a dirty-exit prompt | 21, 22 |
| `P10C-selective-tui` | one-shot commands on the terminal emit no screen controls; `x=$(top)` → 2 | 29 |
| `P10C-denials` | `/bin/shell` → not found/denied; `kill`/`snapshot` → 1 with no effect (task set unchanged); `export K=V` → `config unavailable` 1 (D10-4); `ssh` with `--auth publickey` → refused; a denied read on the `/srv` `cellos/` fixture → 1 | 11, 24 |
| `P10C-ssh-serial` | `ssh` with password on the physical UART fails closed without echo `[INFERENCE: the S5-4a secret-input refusal on the UART applies]`. If it does not, the full ssh scenario runs here as well. | 24 |
| `P10C-no-inbound` | no host forward exists; `ps` shows no listener Cell | 25 |
| `P10C-ctrl-c` | `sleep 30` then Ctrl-C → 130, then the prompt; endless `rg`/`cat` sink then Ctrl-C | 05, 09 |
| `P10C-not-on-uart` | command data of the GUI/one-shot runs never appears on the UART; no secret in any log | 24, 26 |

**Desktop (desktop ISO; serial `cellbox sh` also present).** Markers `P10G-*`, judged by the pixel-exact oracle:

| Marker | Observes | CBX |
|---|---|---|
| `P10G-launch` | taskbar click → Terminal window → banner and prompt screen match exactly | 14, 26 |
| `P10G-real-output` | `ls` of a disk directory, `date` twice (changes), `ps` (the Terminal's own Cell present), file create then `ls` shows it | 14, 12 |
| `P10G-two-sessions` | a second Terminal; state set in one is absent in the other; input does not cross | 03, 14 |
| `P10G-vt` | the S5-2 `vt-sample.bin`, alt-screen enter/exit and S5-4 hostile fixtures render exactly | 15 |
| `P10G-tui` | interactive `top` draws and `q` restores; `edit` on a file then save/reopen | 13, 21 |
| `P10G-ssh` | to the D79-1 fixture with password, known host (trust disk), PTY `echo` and resize; exec with `--command` returns remote bytes and status; a changed host key is refused; a wrong password is refused | 23 (other-host part), 24 (partial) |
| `P10G-apps` | `files`, `chrome` and `ocel` launch and draw their first window (D11-6 smoke; no functional claim) | 26 |
| `P10G-delete-key` | Delete key behaviour recorded either way (known defect) | — |

Both profiles also run the **Phase 10 step 3 negative set** where the canonical image can express it. Negatives that need a fixture kernel row (forged child modes, sandbox X/M/W probes, the cross-session driver) come from L3 (S10-3.3).

#### S10-3.3 Existing lanes: which count, and how the digest match is proven

**Import mode (D3-1 A).** Each runner gains an optional `--candidate <candidate.json>` argument; without it, behaviour is unchanged. With it:
1. The runner does **not** build the shared cells. It copies them from the candidate profile (headless for P02/P03/P04-terminal/P09; desktop for P04-GUI) and verifies each copy's sha256 against `candidate.json`.
   - Shared = the intersection of the lane's image paths and the candidate's, plus `init`: `cellbox` (and `/bin/cellbox-ssh` for P09), `vfs`, `config`, `input`, `platform`, `nvme`, `e1000`, `net`, the six workers, `init`, and for the GUI lane `compositor`, `fb-console`, `desktop`, `files`, `terminal`.
2. It re-signs those copies with the lane's per-run private seed, the key its fixture kernel trusts, through the existing private-fixture path (`CELLBOX_PRIVATE_FIXTURE_SIGNING=1` on that one call, as today).
3. It computes the **signed-payload sha256** of every re-signed copy and requires it to equal the candidate's. This proves that only the 64 signature bytes differ. The result is written to `payload-match.json` (per binary: the candidate sha256 and payload, the lane sha256 and payload, `equal`). **Any mismatch fails the run before boot.**
4. Lane-only binaries are built as today and listed as lane-only in `payload-match.json`: the session parent and child, `p03-filter`, `phase04-terminal-driver`, `top-load`, `s2tw-probe`, the workers that are not canonical (`free`, `date`, `env`, `uname`, `hotswap`) and the net-tools.

| Lane | Runner | Mode | Counts as |
|---|---|---|---|
| L1 | `qemu-phase02-session-test.sh` (and its `test-hooks` kernel-lane mode) | import (headless) | payload-matched cell evidence for CBX-03..05 and 29; the kernel lane counts only for kernel self-tests at fixture features |
| L2 | `qemu-phase03-pipeline-test.sh` | import (headless) | CBX-06..09 |
| L3 | `qemu-phase04-terminal-test.sh` | import (headless) | CBX-09..12, 17..22, 29; the S2-K/N/T/W denial matrix (CBX-11); one-shot markers (CBX-10) |
| L4 | `qemu-phase04-gui-test.sh` | import (desktop) | CBX-12..15, 21; the D10-3 6/6 rule (S10-3.5) |
| L5 | `qemu-phase09-ssh-test.sh` with a new `--no-faults` switch | import (headless; `cellbox` without `p09-faults`) | CBX-23 (other-host), CBX-24 (partial) |
| L5f | the same runner, unchanged (`p09-faults`) | lane-only bytes | S9-6 fault, rekey and no-replay evidence at its approved lane ceiling (CBX-24/25 parts); **never** matched-image evidence |

**Fallback (D3-1 B).** If `objcopy` re-signing is not payload-idempotent, step 3 fails for every binary. That would be the first observation (`[INFERENCE]`: `--remove-section` plus `--add-section` of the same 64-byte section should reproduce the layout).
- The fallback builds the fixture kernels with `CELLOS_PHASE02_CELL_PUBLIC_KEY` = the **dev public key** (the kernel accepts any 32-byte key file, `kernel/build.rs:77-85`).
- The lanes then boot the candidate cells byte-identical (full sha256 equality) and dev-sign their lane-only fixture cells. Those names are not guarded.

**Rule for fixture-only kernel profiles.** Fixture kernels are `phase02-terminal-profile`, `phase03-filter-profile`, `phase04-driver-profile` and `test-hooks`; `p09-faults` is a cellbox feature. They are built from the same commit, and their features are recorded in each verdict.
- They count **only** for the behaviour of payload-matched **cells**.
- They never count for kernel-level claims: dev-key policy and signature admission, the canonical boot and service plan, VIFS1 size, boot timing, or the canonical launch-edge set. Those come **only** from L0 and L6 on the candidate kernel.
- Edges that only the fixture profiles add (driver → `/bin/cellbox`, `cellbox` → `/bin/p03-filter`, session fixture routes) are excluded from any CBX-11 claim.
- A CBX row that cites a lane must name the lane's fixture features.

#### S10-3.4 L6 — the 12 ISO consumers

**Runs:** each test runs with its ISO pointing at a candidate ISO in `$W`, never at `build/`.
- **Env-override consumers:**
  - `ahci-x86` (`VICELL_AHCI_ISO`), `xhci-x86` (`VICELL_XHCI_ISO`), `serial-x86` (`VICELL_SERIAL_ISO`) and `nic-x86` (`VICELL_NIC_ISO`) → headless ISO;
  - `igb-x86` (`VICELL_IGB_ISO`, `VICELL_IGB_VTD_ISO`) → headless ISO;
  - `iommu-dmar-x86` (`VICELL_IOMMU_ISO` → headless; `VICELL_PC_ISO` → the C5 ISO);
  - `desktop-x86` (`CELLOS_DESKTOP_ISO` → desktop ISO, `CELLOS_DESKTOP_ACCEL=tcg`).
- **Hard-coded `build/vicell-x86.iso` consumers:** `x86_64-boot`, `nvme-x86`, `virtio-x86`, `pcie-multibus-x86` and `handoff` (`repo_root().join("build/vicell-x86.iso")`). D3-2 A adds one shared `VICELL_X86_ISO` override to these five (test-only change) instead of writing `build/`.
- **Also run:** `scripts/qemu-x86_64-test.sh <headless ISO>` (the CI boot check, `Cellos >`).

**Regression classification:**
- Every consumer also runs once against the current `build/vicell-x86.iso` (`eb1cb813…`, not quarantined: §1.2 scan). That run is read-only and serves as the before-baseline.
- A test that passes before and fails after is a **regression** and stops the slice. A failure on both is pre-existing and is recorded with its log.
- `x86_64-boot`'s two Tier-2 refusal tests need `CELLOS_X86_PROD_REFUSAL_ISO`, which `build-x86_64-prod-refusal-ci.sh` cannot build while it dev-signs `cellbox` (G2). They skip loudly and are recorded (D3-6).

#### S10-3.5 GUI 6/6 rule (D10-3)

The desktop profile, including `/bin/terminal`, counts as qualified only if all of these pass:
- **L4 6/6:** six consecutive payload-matched runs of `qemu-phase04-gui-test.sh`, five click launches plus one autoload (the S3/C5 practice), each at its full check count (290 at the last run) with 10/10 strict stderr iterations;
- **L0 desktop 6/6:** six consecutive TCG runs of every `P10G-*` marker except `P10G-delete-key`.

Any failure, including an infrastructure flake, resets the count. There is no best-of-N. If 6/6 is not reached, the desktop canonical image is withheld (D10-3 option B) pending a decision, and only the headless image can go on to S10-5.

#### S10-3.6 CBX map (what S10-3 can close; rows flip only at the S10-3 CP2)

| CBX | Evidence in S10-3 | Expected state |
|---|---|---|
| 01, 02 | the Phase 10 step-1 audit: every delta's CP1/CP2 and digest (delta index), and `cargo deny` rerun on the candidate source | stays met |
| 03–05 | L1 (matched) + `P10G-two-sessions` + `P10C-ctrl-c` | met |
| 06–08 | L2/L3 + `P10C-typed`/`P10C-bytes`/`P10C-rg` | met |
| 09 | L2/L3 + `P10C-state-script` | met |
| 10 | `P10C-image-list`/`P10C-explicit-forms` + L3 one-shot markers | met on the candidate (already met privately) |
| 11 | the L3 denial matrix (non-fixture edges) + `P10C-denials` | met |
| 12 | the headless candidate has no UI Cell (`P10C-boot`) + L4 C5 parity | met |
| 13, 15 | L4 + `P10G-vt`/`P10G-tui` + `P10C-top` | met |
| 14 | L4 6/6 + `P10G-launch`/`P10G-real-output`/`P10G-two-sessions` | met if S10-3.5 holds |
| 16 | audited from archived P05 evidence (the throwaway consumer was removed by design, S5-4e); not rerun | stays as audited |
| 17, 18 | L3 `top-load` markers + `P10C-top`. D60-3 (clock units) stays open and is recorded against 18. | met, with D60-3 noted |
| 19, 20 | L3 `rg-*` + `P10C-rg` | met |
| 21, 22 | L3 `edit-*` + L4 EDIT + `P10C-edit`/`P10G-tui`; durability as "synced, not crash-proof" (D79-12) | met at that durability statement |
| **23** | L5 (other host) + `P10G-ssh` | **partial**: the Tier-3 guest endpoint is blocked (§4) |
| **24** | L5/L5f + `P10G-ssh` + `P10C-denials` | **partial**: publickey/KMS (S9-5) deferred |
| **25** | L5f no-replay + `P10C-no-inbound` | **partial**: the VM endpoint route is blocked |
| 26 | L0 headless + desktop + L4 6/6 + L6 | met if all pass |
| 27 | S10-4 | not here |
| 28 | S10-5 | not here |
| 29 | L1/L3 + `P10C-selective-tui` + the one-shot TUI refusals | met |

#### S10-3.7 Time budget (TCG, one full pass; estimates `[INFERENCE]` from recorded runs)

| Item | Estimate |
|---|---|
| C1–C5 builds (two cell profiles, three kernels, three ISOs) | 1–1.5 h |
| L0 runner and drive script development, plus the import mode in five runners and the `VICELL_X86_ISO` test override | the main engineering item; one working session |
| L0 headless (1 run) + desktop (6 runs) | ~1.5 h |
| L1 (+ kernel lane), L2, L3 | ~1 h (the L3 boot window is 1800 s) |
| L4 ×6 | ~0.75 h |
| L5 + L5f (Docker fixture) | ~0.5 h |
| L6 (13 tests, before and after) | ~1.5 h |
| **Machine time, one pass** | **~6–7 h**, plus reruns of any failure (each a full run, as S10-3.5 requires) |

#### S10-3.8 Risks

- **Payload idempotence** of re-signing (D3-1); the fallback is defined.
- **Five runner edits.** The import mode is opt-in; each runner also runs once without `--candidate` to show unchanged default behaviour.
- **Serial secret input** may refuse ssh passwords on the UART `[INFERENCE]`. The positive ssh scenario is then GUI-only, and the GUI run must still reach 6/6.
- **Ocel/Chrome on the default kernel** (`large-cell-va`, D11-6). If `P10G-apps` fails, stop and decide (D3-4).
- **Pre-existing `[selftest] COMPLETION-QUEUE: FAIL`** (S10-1 deviation 11) on the candidate kernel (D3-5).
- **TCG timing and flakes:** the GUI strict-stderr iterations and the Docker sshd start (seen in S9 closure). S10-3.5 resets on any flake.
- **Consumers may assume the old image** (prompt or help strings, `sshd`). L6 regressions are triaged against the before-baseline.
- **`dev-weak-rng`:** ssh runs only with `+rdrand,+rdseed` (TCG) or a host CPU (KVM); recorded per D10-12.
- **Retained dev-key candidates** live outside the repository until S10-5 (D3-3). They are recorded by hash and must never be copied into a canonical path before the S10-5 CP2.
- **Disk space:** the candidate, three kernel target directories and the lane target directories need several GB in `/tmp`.

**Rollback:** revert the runner and test changes. Delete `$W` (except the retained candidate, if D3-3 A). Canonical paths are untouched. Lane evidence is retained.

#### S10-3.9 Decisions for S10-3 CP1

| ID | Question | Recommendation |
|---|---|---|
| D3-1 | Digest matching: **A** import the candidate cells, re-sign them with the lane key and prove signed-payload equality, **or** B fixture kernels trust the dev public key and boot the candidate cells byte-identical | **A**, with B as the defined fallback if A's payload check fails |
| D3-2 | The five hard-coded `build/vicell-x86.iso` consumers: **A** add a shared `VICELL_X86_ISO` override (test-only change), **or** B run them from a scratch copy of the tree | **A** |
| D3-3 | Candidate retention until S10-5: **A** keep `$W/cand` and `candidate.json` outside the repository (e.g. `$HOME/.cache/cellos-candidates/<headless-img-sha>/`, mode 0700), **or** B rebuild at S10-5 (different bytes, so it would need a new S10-3) | **A** |
| D3-4 | If `ocel`/`chrome` fail on the default kernel: stop and decide then between adding `large-cell-va` to the canonical desktop kernel (rebuild, rerun L0 desktop and L4) and dropping them (reversing D11-6) | **Stop and decide**: no pre-authorization |
| D3-5 | The pre-existing `COMPLETION-QUEUE: FAIL` self-test line on the candidate kernel: **A** record it as a known pre-existing defect (release notes, own plan) that does not block S10-3, **or** B block until it is fixed | **A**, disclosed; your call |
| D3-6 | x86 lanes that still dev-sign `cellbox` outside the routes (`build-x86_64-prod-refusal-ci.sh`, `-domain-test-ci.sh`, `-c2c-lifecycle-ci.sh`, `make-hypervisor-fs-x86.sh`) stay refused; their tests skip and are recorded. **Accept**, or add routes for them (new S10-2 work) | **Accept** for S10-3; revisit in S10-5 |
| D3-7 | KVM: optional and recorded separately; TCG is mandatory | **As stated** |

**CP1 record.** The user approved the S10-3 CP1 on 2026-10-26 (relayed by Main) with every recommendation D3-1..D3-7. For D3-5, `[selftest] COMPLETION-QUEUE: FAIL` is a known defect and does not block.

#### S10-3.10 Results (2026-10-26): stopped at the L6 regression rule

**Outcome.** The L6 regression rule (S10-3.4) stopped the slice. Two consumers that pass on the `build/vicell-x86.iso` baseline fail deterministically on the candidate, and the CI boot check fails on the candidate kernel's Tier 2 posture. Per Main's instruction the slice stopped and was not worked around: no product source was changed, and no further lanes were run. **No CBX row flips.** Evidence is in `reports/s10-3-evidence/`. Every summary in it (`l6/*.json`, `l6/rerun/summary.txt`, `audit/audit.json`, `candidate/candidate.json`) is harness output.

**Candidate** (C0–C6; `W=/tmp/cellbox-s10-3-7yWa`, outside the repository, retained read-only):
- `candidate.json` SHA-256 `51260f38a2bebf0082c83cca06cb3185550b770b80fec095ec4e783608cd7f15`, with no failures recorded;
- headless: `kernel_fs.img` `fed9ce5b…2b99`, kernel `7697a4d2…6073` (15,599,632 B; default features), ISO `46975715…6276`;
- desktop: `kernel_fs.img` `5726b180…1f69`, kernel `8481416b…dffb` (28,182,544 B), ISO `c621d461…908d`;
- PC profile: headless image, kernel `default+board-x86-pc` `7424bfc9…3090`, ISO `6b3e5f70…9940`;
- every signed cell and `init` verifies against the dev public key; each kernel ELF embeds the exact `kernel_fs.img` and `init` bytes; each file's signed-payload SHA-256 is recorded;
- built by the S10-2a routes, the dev key and the CI kernel command (with `EMBEDDED_OVERRIDE` and a private `CARGO_TARGET_DIR` added). The scripts are in `scripts/`.

**L6 (12 consumers + `qemu-x86_64-test.sh`)**, before = `build/` ISOs read-only, after = candidate ISOs; `l6/l6-classification.json`:
- First pass: 35 pass→pass; 1 pre-existing failure (`xhci_injected_key_decodes_in_cell_x86`); 4 skips (`handoff_x86_32_*`); 1 ignored (`x86_virtio_blk_initialises`); **4 classified REGRESSION**.
- Serial reruns (`l6/rerun/summary.txt`, two rounds per side):
  - `nic_x86_e1000_dhcp`: after passed 2/2. The first failure was an NVMe I/O timeout at boot under parallel load, so it is a **flake, not a regression**.
  - `ahci_fat32_persistence_reboot_x86`: before 3/3 pass, after 3/3 fail. **REGRESSION A.**
  - `nvme_fat32_mount_x86`: after 3/3 fail at the same step; before 2/3 pass (one mount timeout, a baseline flake). **REGRESSION A.**
  - `qemu-x86_64-test.sh`: before 3/3 pass, after 3/3 fail. **REGRESSION B.**
- **REGRESSION A — the UART shell does not run a line ended by LF.** The integration harness's `send_line` ends lines with `\n`.
  - The baseline image ran the split `/bin/shell`, which accepted LF.
  - On the candidate, `cellbox sh` echoes the typed text but executes nothing (`l6/diag/`, `diag_vwrite.py` on the same SATA FAT32 layout):
    - with CR, `vwrite` returns at once with status 0, and `vcat`/`ls /mnt/sd` show the file;
    - with LF, none of five commands runs, whether sent at once or after a 1 s settle.
  - `cellbox`'s `read_line` itself accepts `\n` (`async_utils.rs:225`), so the LF is lost earlier on the UART/input/session path `[INFERENCE: not root-caused]`.
  - Consequence `[INFERENCE]`: the `x86_64-boot` `echo`/`ls`/`ps` tests pass vacuously on the candidate, because they match their own typed echo.
- **REGRESSION B — the Tier 2 admission posture.**
  - The candidate kernel (the CI command, default features) logs `Tier 2 admission: ENABLED (development profile)`.
  - The baseline ISO's kernel logs `DISABLED (fleet profile)`, so it was built with `policy-required` or `production-relay-image`.
  - `qemu-x86_64-test.sh:162-171` requires DISABLED. A canonical kernel built by the CI kernel step therefore fails CI's own boot check, and it would admit domain-class (Tier 2) artifacts.
  - The canonical kernel's feature set and posture need a decision (D10-1 says dev posture; Tier 2 admission is not covered).

**Other runs:**
- **Import mode (D3-1 A):**
  - re-signing is payload-idempotent (cellbox, vfs, ls and init re-signed with a random seed keep identical signed payloads), so fallback B is not needed;
  - `qemu-phase02-session-test.sh --candidate`: 48 PASS markers, with 12 shared cells payload-equal and 2 lane-only;
  - the P02 kernel lane in import mode: kernel self-tests and VFS self-tests PASS, 11 shared cells byte-identical;
  - both runs are in `import-mode/` (run by the ImportMode worker; the runner's stdout was not archived, so `payload-match.json`, provenance and serial logs are the record);
  - `test_phase10_candidate_import.py` 18 OK. P03, P04-terminal, P04-GUI and P09 are syntax-checked only and were not run.
- **CBX-01/02 audit** (`audit/audit.json`, PASS):
  - the five candidate documents equal the reconciled digests, with the catalog at `2a4c3e66…`;
  - every delta-index row has a CP1 and CP2 or a recorded exception;
  - the recorded scoped `cargo deny` on the cellbox graph gives `advisories ok, bans ok, licenses ok, sources ok`;
  - the workspace-wide `cargo deny` fails as recorded before (ocel-pdf AGPL, `crypto-bigint 0.5.5` RUSTSEC, wildcard path deps). **Finding:** that graph includes crates the canonical images now package (`ocel`, `service-net`), which no CBX row covers.
- **L0:** not implemented (stopped before any file was written). Exploration of the candidate by the L0 worker (TCG) recorded:
  - (i) **desktop:** bytes typed on the UART go to the focused GUI Terminal, not the serial `cellbox sh` (routing to decide; affects `P10C-not-on-uart` and cross-session claims);
  - (ii) `DENY launch edge … target=/bin/ocel` is logged, yet `ocel` launches and paints; `ocel` and `chrome` both draw on the default kernel, so **D3-4 did not trigger**;
  - (iii) one intermittent NVMe `RDY wait failed` at boot;
  - (iv) spec forms that differ from S10-3.2: `top --records | take 2` is refused (needs `take --frame N`); `return` is not a builtin; only `a | b &` and `a & b` are refused (`sleep 1 &` runs in the background); the ssh password path on the UART needs the Docker network.
- **Not run:** L0 (headless, desktop ×6), L2, L3, L4 ×6, L5, L5f.

**Files** (test and harness only):
- `tests/integration/src/lib.rs`: new `x86_iso_path()` with the `VICELL_X86_ISO` override (D3-2 A), used by `x86_64-boot`, `nvme-x86`, `virtio-x86`, `pcie-multibus-x86` and `handoff`;
- `tests/integration/tests/desktop-x86.rs`: captures go to the temp dir instead of `build/` (deviation 1);
- the five runners: opt-in `--candidate`, and `--no-faults` for P09;
- new `scripts/phase10_candidate_import.py` and `scripts/test_phase10_candidate_import.py`.

No product source changed.

**Canonical artifacts unchanged** (`canonical/`): before and after are identical for the S10-2a list plus `build/x86-pc-lane/vicell-x86-pc.iso` (`273a3be0…`); the embedded directory hashes and listings with mtimes are identical; no file under `target/x86_64-unknown-none`, `build/` or `kernel/src/` is newer than the start marker.

**Deviations for CP2.**
1. `desktop-x86.rs` wrote `build/screen-test-*.ppm`, which would change `build/`. Its captures now go to the system temp directory (test-only change).
2. The L6 harness runs each consumer with `--test-threads=1` and libtest JSON output (`-Z unstable-options --format json --show-output`). The first before and after passes ran in parallel with each other and with worker builds; the four flagged tests were then rerun serially.
3. The P02 import-mode validation was run by a worker, and its runner stdout was not archived.
4. The candidate was not moved to `$HOME/.cache/cellos-candidates/` (D3-3 A). Regressions A and B likely require a rebuild, so it stays at `W/cand` (read-only) until the decision.
5. `tools/cellos-fs-provision/target/x86_64-unknown-linux-gnu/debug/cellos-fs-provision.d` (an in-repository host-tool dep-info file, not a canonical artifact) is newer than the start marker; a lane fixture build touched it `[INFERENCE: the P02 import-mode run]`.

**Decisions needed before S10-3 resumes:**
- (a) Regression A: a product fix for LF line submission on the UART (own CP1), or accept that LF clients (the integration harness) must send CR (a test change, reclassifying these consumers).
- (b) Regression B: the canonical kernel's features and Tier 2 posture (for example the fleet or `policy-required` profile the baseline was built with). This changes the candidate kernel and invalidates the C3–C5 bytes.
- (c) Whether the UART input routing on desktop is acceptable.
- (d) Amending the S10-3.2 marker forms to the as-built commands.

**Decisions recorded (user, 2026-10-26, relayed by Main):**
- **(a)** Fix the product, in a new slice **S10-3a** (below): a line submitted with a lone LF over the UART must run.
- **(b)** The canonical kernel uses the **fleet profile, with Tier 2 admission DISABLED**, matching the baseline and the CI boot check.
- **(c)** UART input going to the focused GUI Terminal on desktop is **accepted** and must be documented. Serial is qualified on the headless image; the desktop is qualified through the GUI.
- **(d)** The S10-3.2 marker commands are amended to the real forms: `take --frame N`, no `return` builtin, and the exact `&` refusal forms (done in the S10-3.2 table above).

#### S10-3a — UART lone-LF line submission and the fleet-profile kernel (**CP1 approved 2026-10-26**: D3a-1..D3a-4; **CP2 approved 2026-10-28**, including S10-3a.8 deviations and decision (e) superseding P02 A2 item 1)

Prepared 2026-10-26 by reading and by `/tmp` probes only; no product source was changed and no canonical byte was written. Probe evidence is archived in `reports/s10-3a-investigation/`:
- the probe scripts, serial logs and GUI launch records;
- the api test log;
- the hashes of the fleet kernels and ISOs and of the extracted baseline kernel.

The binaries themselves stay in `/tmp/cellbox-s10-3a-inv/`. The LF reproduction logs are in `reports/s10-3-evidence/l6/diag/`.

##### S10-3a.1 Root cause: where a lone LF stops counting as Enter

Path of one UART byte to the shell (x86, headless, `cellbox sh` holding the physical serial session):
1. **Kernel.** `kernel/src/task/drivers/console_drv.rs:69-77` relays each UART byte unchanged to the input service as one `EV_ASCII` event (`relay_ascii_to_input`, `:232`).
2. **Input service.** `cells/services/input/src/main.rs:564-570`: when a physical serial session is active, the byte goes to `sessions.serial_byte` (`cells/services/input/src/session.rs:1168-1179`). The comment there (`:1172-1173`) states the design: *"Enter arrives as CR (the frozen Enter byte) and Ctrl-J as LF"*.
3. **Decoder (the point where LF stops being Enter).** `api::services::session::SerialDecoder` maps single C0 bytes in `scalar_event`, `libs/api/src/services/session.rs:245-249`:
   - `b'\r'` → `Key(ENTER)`;
   - `1..=26` → `Key(0x60 + byte, CONTROL)`, so `0x0a` becomes **`Key('j', CONTROL)`**.

   This is the frozen grammar of `spec.md:86` (*"01..1a→Control+a..z except 09→Tab/0d→Enter … 0a is Control+j, not platform Backspace/Enter aliases"*). It is pinned by the host test `raw_lf_is_control_j_and_cr_is_enter` (`session.rs:590-602`), which passes (reproduction: `cargo test -p api raw_lf_is_control_j_and_cr_is_enter`, run in `/tmp`).
4. **Shell (the point where the event is dropped).** `cells/tools/cellbox/src/executor.rs:614`, `terminal_read_line`, matches only `EventKind::Key { code, modifiers: 0, … }`, and only `0x110001` (Enter) submits (`:626`). `Key('j', CONTROL)` falls through to `_ => {}` (`:691`): the typed text has been echoed, and the line is never submitted. The same drop happens in:
   - `read_secret_events` (`:715-716`);
   - `prompt_step` (`:872-873`, the `rm -i` and confirmation answers);
   - the ssh password reader (`cells/tools/cellbox/src/ssh.rs:718-719`).

   `TerminalKeys::classify` (`:799-800`) is the key-level TUI reader and is not a line reader.
5. **Reproduction on the S10-3 candidate** (TCG; `diag_vwrite.py`, `l6/diag/`):
   - `cmd\r` runs, with status 0 and the file written;
   - `cmd\n` (sent at once, or after a 1 s settle) is echoed and never runs (5/5 commands).

   The integration harness `send_line` (`tests/integration/src/lib.rs:2264-2270`) sends `\n`.

**How the old `/bin/shell` handled it.** The split shell (`HEAD:cells/tools/shell/src/async_utils.rs:178,225`) read raw bytes with `sys_read(0)` and ended a line on `ch == b'\r' || ch == b'\n'`. Its input came through the input service's legacy focus path (`cells/services/input/src/main.rs:575-586`, which maps `0x0D | 0x0A → KeySym::Return`), because it held no physical session. `cellbox` still carries the same `read_line` (`cells/tools/cellbox/src/async_utils.rs:225`), but the admitted serial path never reaches it: the shell holds the D02 physical session, so input goes through the decoder.

##### S10-3a.2 Minimal fix (cellbox only; no ABI, decoder, input-service or kernel change)

- New pure helper in `cells/tools/cellbox/src/executor.rs`: `fn submits_line(code: u32, modifiers: u32) -> bool`. It is true for `(ENTER, 0)` and `('j', CONTROL)`, so Control+J is accept-line, as in readline.
- The four line readers use it instead of the literal `0x110001` arm: `terminal_read_line`, `read_secret_events`, `prompt_step`, and `ssh::read_secret`.
- Unchanged:
  - paste stays inert (the `!paste` guards stay; an LF inside a paste is not a Key event at all, `spec.md:84`);
  - TUI key reading (`TerminalKeys::classify`) keeps Control+J as a key;
  - the GUI Terminal and SSH frontends.
- Effects:
  - **CRLF clients** get one command plus one empty line (the prompt is printed twice). This matches the old shell; there is no CR-LF coalescing.
  - In the GUI Terminal, Ctrl+J now also submits a line.
- **Files:**
  - `cells/tools/cellbox/src/executor.rs`, `cells/tools/cellbox/src/ssh.rs`;
  - `docs/cellbox-cli.md` (one input note);
  - `spec.md`, one sentence after `:86` (D3a-1). The decoding itself is unchanged.
  - `command-catalog.md`, `Cargo.toml`/`Cargo.lock`, the launch profiles and the kernel are unchanged.

##### S10-3a.3 Fleet-profile kernel (decision b)

**Provenance of the baseline.**
- `build/vicell-x86.iso` (`eb1cb813…`, 2026-10-17, `baseline.md:229`) contains `/boot/kernel.elf` `e1ee1362…e788` (11,189,712 B; extracted read-only with `xorriso`).
- That kernel has the `Tier 2 admission: DISABLED (fleet profile)` string, which is compiled only with `policy-required` or `production-relay-image` (`kernel/src/main.rs:1254-1266`). It also loads and verifies the dev-key `/POLICY.BIN` (57 entries).
- `production-relay-image` cannot be the profile: `kernel/src/main.rs:18-29` refuses it at compile time together with `dev-policy-key`/`dev-signing-key`/`dev-weak-rng`.
- **So the baseline is the default features plus `policy-required`** (`kernel/Cargo.toml:70,88`). This is the same set `scripts/build-cloud-images.sh:21` uses for `build/cellos-x86_64.iso`.

**Why the CI kernel is not fleet today.** The CI kernel step (`.github/workflows/ci.yml:938-943`) and the PC step (`:1002`) build with default features only. On x86, `domain_admission::enable_for_boot` (`kernel/src/loader/domain_admission.rs:336-366`, with `smp.rs:706-709`) qualifies, so the kernel logs `ENABLED (development profile)` (`main.rs:1219-1240`). `scripts/qemu-x86_64-test.sh:162-171` refuses that, so CI's own boot check fails on a default-feature kernel built from today's source.

**Probe** (`/tmp/cellbox-s10-3a-inv/fleet-probe.sh`): fleet kernels (`--features policy-required`) around the unchanged S10-3 candidate images.
- Headless kernel `8b2d3245…`, ISO `e6a7055a…`:
  - `qemu-x86_64-test.sh` **PASS** with `Tier 2 admission: DISABLED (fleet profile)` and the policy verified (58 entries);
  - the shell runs with CR;
  - `export` → `config unavailable` 1.
- Desktop kernel `871f1ab7…`, ISO `ec883f0d…`: the compositor, `fb-console`, `desktop` (ready) and the shell start.
- A taskbar click launches Terminal (ready), Files (ready), Chrome and Ocel, the same as on the default kernel (`gui-probe.py`).
- **Behaviour change under `policy-required`:** every path without a `DEV_POLICY` row gets `NoEntry` → `CapSet::EMPTY` except the trusted core (`kernel/src/policy.rs:930-956`, `:910-915`).
  - The canonical paths without a row are `/bin/{ls,cat,echo,ps,kill,shutdown,config}` (headless) plus `/bin/{terminal,files,chrome}` (desktop).
  - On the probe they log `[policy] narrowed … dropped 0b01110000000000000`, which means `vfsw`, `mem` and `tasks` are dropped.
  - Those cells still launched and `/bin/ls`, `/bin/cat`, `/bin/echo` worked.
  - Whether the GUI Terminal's session, `ps` in the Terminal and Files writes still work is **unverified**: L4 and `P10G-*` must show it (D3a-2).

**Change:**
- CI: the x86 kernel step builds with `--features policy-required` and the PC step with `--features board-x86-pc,policy-required`. `build-x86_64-cells.ps1`'s closing hint names the same command.
- `scripts/assemble-x86-image.py` gains `CANONICAL_KERNEL_FEATURES = ("policy-required",)` and records it in `manifest.json` (`kernel_features`), so the CI step, the S10-3 candidate builder and S10-5 read one source.
- That edit changes the assembler's digest, so the route pin `X86_CANDIDATE_RECIPE_SHA256` in `scripts/cellos_sign/signing.py` is re-reviewed and re-pinned in the same change (S10-2a.6).

##### S10-3a.4 Tests

- **Host (cellbox):**
  - `submits_line` truth table (Enter and Ctrl-J submit; other Control+letters, Alt+Enter, releases and pastes do not);
  - `prompt_step` with `Key('j', CONTROL)` → `Enter`;
  - a composition test feeding `b"echo x\n"` and `b"echo x\r"` through the real `api::services::session::SerialDecoder` into the classifier (one submit each), plus `b"a\r\n"` → submit then an empty submit;
  - the existing 205 cellbox host tests and the api `raw_lf_is_control_j_and_cr_is_enter` test stay unchanged and pass.
- **Assembler and signing:**
  - `test_assemble_x86_image.py`: `kernel_features` recorded in the manifest;
  - `test_cellos_sign.py`: the pin test passes after the re-pin (66).
- **Runtime:**
  - the L6 consumers `ahci_fat32_persistence_reboot_x86` and `nvme_fat32_mount_x86` pass on the fleet candidate;
  - `qemu-x86_64-test.sh` passes (DISABLED);
  - L0 gains `P10C-lf-submit` (`cmd\n`, `cmd\r` and `cmd\r\n` each run once; the empty submission is recorded);
  - the P04 terminal lane reruns as the regression lane for the shell bytes.
- **Docs:** `docs/cellbox-cli.md` gets the LF/CR input note and the decision (c) note (UART bytes on desktop go to the focused GUI Terminal; serial is qualified on headless).

##### S10-3a.5 Rebuilt candidate and reruns

The fix changes `cellbox` (and `/bin/cellbox-ssh`); the fleet profile changes all three kernels. The S10-3 candidate (`51260f38…`) is therefore **superseded and deleted after archiving**.
- **New candidate:** the S10-3.1 steps C0–C6 with the S10-3a source and the kernel built with `--features policy-required` (PC: `board-x86-pc,policy-required`).
- **Reruns:** the full S10-3 battery from the start on the new `candidate.json`:
  - L6 before and after (the before-baseline is unchanged);
  - L0 headless, and desktop ×6;
  - L1 (and the kernel lane), L2, L3, L4 ×6, L5 and L5f, all in import mode;
  - the CBX-01/02 audit (`spec.md` gets a new digest if D3a-1 A is chosen).

  No earlier S10-3 run counts for the new bytes.

##### S10-3a.6 Risks

- **Spec wording.** `spec.md:86` calls Control+J "not … an Enter alias". The fix keeps the decoding and changes only what the shell's line readers do with it; D3a-1 decides whether the spec records that.
- **CRLF double submission** (an empty line) for CRLF clients, as in the old shell. A coalescing rule would differ from both the old shell and readline.
- **Fleet narrowing** of the workers and desktop apps that lack policy rows (S10-3a.3). If L4 or `P10G-*` fail because of it, adding `DEV_POLICY` rows (`scripts/sign-policy.py`) is a policy change that needs its own decision (D3a-2 B).
- **Re-pin friction.** The assembler edit forces the S10-2a pin update in the same change.
- **Unrelated working-tree change** `[INFERENCE: observed in git diff, not caused by S10]`: `kernel/src/main.rs` currently has the VMX "hypervisor present → cap closed" guard removed (uncommitted). TCG `qemu64` exposes no VMX, so the lanes do not exercise it. It is part of the candidate source state and is listed for S10-5's source audit.

**Rollback:** revert the cellbox, docs, spec, CI, assembler and pin changes; the S10-3 regressions return. No canonical byte is written.

##### S10-3a.7 Decisions for S10-3a CP1

| ID | Question | Recommendation |
|---|---|---|
| D3a-1 | Where lone LF becomes a submit: **A** in cellbox's line readers (Control+J = accept-line; decoder and ABI unchanged) plus one `spec.md` sentence; **B** the same without the spec sentence; **C** change the decoder/input service so LF → Enter for physical serial sessions (changes the frozen `spec.md:86` grammar and the api test, and affects every typed consumer and TUI) | **A** |
| D3a-2 | Fleet-profile policy coverage: **A** keep the current `DEV_POLICY` rows (baseline parity; workers and desktop apps run narrowed) and let L4/`P10G-*` decide; **B** add rows for the 10 uncovered canonical paths granting their current caps (`sign-policy.py` change) | **A**, with B only if a lane shows breakage |
| D3a-3 | Where the canonical kernel features live: **A** `CANONICAL_KERNEL_FEATURES` in the assembler, recorded in `manifest.json` and used by CI (re-pin); **B** CI and docs only, assembler untouched (no re-pin) | **A**: one source |
| D3a-4 | CRLF: **A** no coalescing (old-shell parity); **B** ignore a Control+J that directly follows an Enter | **A** |

**CP1 record.** The user approved the S10-3a CP1 on 2026-10-26 (relayed by Main) with every recommendation:
- D3a-1 A: cellbox line readers plus the `spec.md` sentence;
- D3a-2 A: no new policy rows, stop if a lane breaks;
- D3a-3 A: `CANONICAL_KERNEL_FEATURES` in the assembler, with a re-pin;
- D3a-4 A: no CRLF coalescing.

##### S10-3a.8 Results (implementation 2026-10-26; **CP2 approved 2026-10-28** with recorded deviations)

**Files** (as-built SHA-256 in `reports/s10-3a-evidence/as-built.sha256`):
- `cells/tools/cellbox/src/executor.rs`:
  - new `submits_line` (Enter, or Control+J) and `line_key`;
  - `terminal_read_line`, `read_secret_events` and `prompt_step` act on `line_key`;
  - three host tests: the truth table, the prompt on Control+J, and serial `\n`/`\r`/`\r\n` plus paste through the real `SerialDecoder` and SDK `Event::decode`.
- `cells/tools/cellbox/src/ssh.rs`: the password reader uses `submits_line`.
- `.agents/261020-1750-cellbox-cli/spec.md`: one paragraph after the grammar line. The new digest `f9c550f7…2988` is recorded in `contract-deltas.md`.
- `docs/cellbox-cli.md`: an LF/CR input note and the decision (c) note on desktop UART routing.
- `scripts/assemble-x86-image.py`: `CANONICAL_KERNEL_FEATURES = ("policy-required",)`, recorded in `manifest.json` as `kernel_features` (`6ea17736…5cb3`).
- `scripts/cellos_sign/signing.py`: the route pin is re-pinned to that digest.
- `.github/workflows/ci.yml`: the x86 kernel step and the PC step build with `--features` read from the manifest (`policy-required`; PC: `board-x86-pc,policy-required`).
- `scripts/build-x86_64-cells.ps1`: the closing hint prints those features.
- `scripts/test_assemble_x86_image.py`: +1 test.
- The proposal, `plan.md`, the Phase 10 log and `CHANGELOG.md`.

Unchanged: the decoder (`libs/api`), the input service, the kernel, the launch profiles, the policy rows, `Cargo.*` and `config/`. The user's uncommitted `kernel/src/main.rs` VMX-guard change was left untouched (`git diff --stat`: 2 insertions, 12 deletions).

**Tests** (`reports/s10-3a-evidence/logs/`):
- cellbox host 208 (205 + 3); api 153; `test_cellos_sign.py` 66; assembler 39 (38 + 1); `test_phase10_candidate_import.py` 18 — all OK;
- ps1 `ParseFile` 0 errors; `ci.yml` YAML load OK;
- `app-cellbox` x86 release build: three pre-existing warnings, none in the new code.
- **P04 terminal lane** (fresh `/tmp/cellbox-phase04-RJq4`, the regression check for the source change): **494/494, 202 driver markers** (`phase04-lane/`).

**Deviations for CP2.**
1. **Phase 02 A2 conflict (omitted from the CP1).** `phase-02-limitations-amendment.md` item 1 (user-approved) made UART Ctrl-J deliberately not Enter (*"Ctrl-J becomes Key(`j`,Control) again"*). The P02 runner asserts it as `ctrl_j_not_enter` (`scripts/qemu-phase02-session-test.sh:374-376,547`). S10-3a reverses that rule for the shell's line readers, so the P02 session lane now fails on exactly that check (48 other passes), in both import and default mode. The check was **not** changed; this is decision (e) in S10-3.11.
2. The ssh password reader calls `crate::executor::submits_line` (`pub(crate)`), so `ESCAPE` and `BACKSPACE` now also require no modifiers (as before).
3. CI reads `kernel_features` from the manifest the assembler step just wrote. The PC step keeps its existing command shape and gains only the features.

#### S10-3.11 Resumed S10-3 battery on the fleet-profile candidate (2026-10-26): stopped on product defects

**Candidate** (`W=/tmp/cellbox-s10-3-1EGP`, read-only, outside the repository; it supersedes `51260f38…`). `candidate.json` is `a9f384a6a3c3ff8fbec8150ad0cf780aed229f1152434d2d84a047020865c07c`, with no failures recorded. Kernel features are `default+policy-required` (PC: `+board-x86-pc`).

| Profile | `kernel_fs.img` | Kernel | ISO |
|---|---|---|---|
| headless | `54fd4917…2d8c` | `8aeea9bb…b2dd` (15,595,584 B) | `eb660754…c512` |
| desktop | `47d952b3…11a6` | `92b255fa…b8b2` (28,178,496 B) | `95d07827…7a29` |
| PC | — | `18fa6710…d8c6` | `99c5a415…1ded` |

Every signed cell and `init` verifies against the dev key, and each kernel embeds its exact image and `init`. Evidence is in `reports/s10-3-evidence/rerun-fleet/` (harness-produced `summary.jsonl`, `l6-classification.json`, `audit.json` and L0 `verdict.json`).

| Lane | Mode / fixture features | Result |
|---|---|---|
| L0 headless | candidate ISO (fleet kernel), TCG | **15/16**: every `P10C-*` passes except **`P10C-ctrl-c` FAIL** (`/tmp/cellbox-phase10-CGkK`, 754 s) |
| L0 desktop | candidate ISO, TCG | **8/9**: every `P10G-*` passes except **`P10G-vt` FAIL** on the hostile sub-check (`/tmp/cellbox-phase10-wUJw`, 440 s); 6/6 not attempted (deterministic defect) |
| L1 P02 session | import (headless), `phase02-terminal-profile` | **FAIL**, 48 passes plus one failed check, `ctrl_j_not_enter` (deviation S10-3a.8-1); the default mode fails the same way |
| L1 P02 kernel lane | import, `test-hooks` | PASS: kernel regression cases, VFS self-tests 7/7 |
| L2 P03 | import, `+phase03-filter-profile` | PASS, 67 checks; 14 cells payload-equal |
| L3 P04 terminal | import, `+phase04-driver-profile` | **PASS 494/494** (202 markers) |
| L4 GUI ×6 | import (desktop), `phase02+phase03` | **6/6 consecutive** (5 click + 1 autoload), 290/290 each; the default mode also gives 290 |
| L5 P09 | import, `--no-faults` | PASS 34/34 (18 markers); the default `--no-faults` run also gives 34 |
| L5f P09 | lane-only bytes (`p09-faults`) | PASS 103/103 |
| L6 | 12 consumers + `qemu-x86_64-test.sh`, before/after | **PASS**: 39 pass→pass, 1 pre-existing failure (`xhci_injected_key_decodes_in_cell_x86`), 4 skips, 1 ignored, **0 regressions**; the boot check passes with `Tier 2 admission: DISABLED (fleet profile)` |
| Audit | CBX-01/02 | PASS (`spec.md` at the S10-3a digest; scoped `cargo deny` all ok) |

**Stop conditions:**
- **SC1 (product defect, pre-existing).** Builtin `sleep` ignores Ctrl-C on the UART (`cells/tools/cellbox/src/cmd_sys.rs:121-128`, `top::wait_ns` with no interrupt poll): `sleep 30` returns after 30 s with `$?` 0. The old `/bin/shell` sleep did not poll either, and no lane ever tested it. In the same class, an endless typed stream into builtin `cat` (which buffers to EOF) ignores Ctrl-C. Ctrl-C works for `rg`, the typed sink and interactive `top` (130).
- **SC2 (product defect).** Builtin `cat` of a non-UTF-8 (or larger than `CAT_READ_MAX`) file on a VFS mount falls back to the boot-FS lookup and prints a false `No such file or directory` (`cells/tools/cellbox/src/commands.rs:386-432`). The S5-4 hostile VT fixture therefore cannot be shown from a disk on the canonical image.
- **SC3 (contract conflict).** S10-3a versus Phase 02 A2 item 1, as above.
- **D3a-2: no fleet-profile rights breakage observed.** L4 6/6 and every `P10G` marker except the SC2 one pass with the narrowed Terminal/Files/Chrome, so no policy rows are needed.

**Other findings (recorded, not stops):**
- the Delete key does not delete in the line editor (a known defect);
- ssh with a password on the UART prints the prompt and fails closed at once (255) without reading a key;
- an alias keeps only its first word;
- stock `/bin/cat` reads only the boot image and takes no stdin, and the shell refuses a builtin after an external stage (`P10C-bytes` uses the recorded alternative forms);
- CRLF gives two prompts, as specified.

**L0 deviations** (the worker's report):
1. Each run is two boots: the main boot with `restrict=on`, then an ssh boot with the Docker fixture and `+rdrand,+rdseed`.
2. The data tree goes on P1 of the `phase07_rg_fixture` disk (the image mounts one FAT volume).
3. `P10C-bytes` uses the alternative forms listed above.
4. The bounded check is `top --records -d 1 --frames 5 | take --frame 2`, and plain `take 2` is refused.
5. The "cat sink" is the typed stream into builtin `cat`.
6. The UART ssh behaviour is as described in the findings.
7. PTY resize is not exercised in the GUI (there is no resize control); L5 covers it.

**CBX outcomes on this candidate.** These are proposals only; no row flips while the slice is stopped.
- **Would be met:**
  - 01/02 (audit);
  - 06–08 (L2, L3, `P10C-typed`/`bytes`/`rg`);
  - 09 (`state-script`);
  - 10 (`image-list`, `explicit-forms`, L3 one-shot);
  - 11 (L3 matrix, `denials`);
  - 12 (headless has no UI Cell; L4);
  - 13 (L4, `P10G-tui`, `P10C-top`);
  - 17/18 (L3 `top-load`, `P10C-top`; D60-3 noted);
  - 19/20 (L3, `P10C-rg`);
  - 21/22 (L3, L4, `P10C-edit`, `P10G-tui`; synced, not crash-proof).
- **Partial as planned:** 23 (L5 + `P10G-ssh`), 24 (L5/L5f + `P10G-ssh` + `denials`), 25 (L5f + `P10C-no-inbound`).
- **Not met:**
  - **03–05** (`P10C-ctrl-c` FAIL, SC1; L1 fails on SC3);
  - **14** and **26** (L0 desktop 6/6 is unattainable while SC2 stands; L0 headless has the SC1 failure);
  - **15** (`P10G-vt` hostile, SC2);
  - **29** (pending SC3, since the L1 Ctrl-J check is part of its evidence).
- 16 stays as audited; 27 and 28 are not part of S10-3.

**Canonical artifacts unchanged** (`rerun-fleet/canonical/`): before and after are identical for the full list including the PC ISO; the embedded directory and listings with mtimes are identical; no file under `target/x86_64-unknown-none`, `build/` or `kernel/src/` is newer than the start marker. The run also wrote: `scripts/__pycache__` bytecode of the imported fixture modules (removed), and `tools/cellos-fs-provision/target/…/cellos-fs-provision.d` (an in-repository host-tool dep-info file, touched by the lanes' fixture build; not canonical).

**Decisions needed before S10-3 can finish:**
- (e) SC3: confirm that S10-3a supersedes Phase 02 A2 item 1 for line readers (then the P02 check is updated to expect `cjA` and `cjB` as two commands), or withdraw it.
- (f) SC1: fix interruptible `sleep` and blocking-sink Ctrl-C (own slice), or accept and amend `P10C-ctrl-c`.
- (g) SC2: fix builtin `cat` for non-UTF-8 and large VFS files (own slice), or accept and amend `P10G-vt`.

The candidate stays retained read-only until these are decided; any product fix means a rebuilt candidate and a rerun of the battery.

**Decisions recorded (user, 2026-10-26, relayed by Main):**
- **(e)** S10-3a **supersedes Phase 02 amendment A2 item 1** (UART Ctrl-J is no longer a non-Enter key for the shell's line readers; the decoder grammar is unchanged).
  - The P02 check `ctrl_j_not_enter` was replaced by `ctrl_j_submits` (`scripts/qemu-phase02-session-test.sh:374-380,479,549`).
  - A first version sent `/bin/echo cjA\n/bin/echo cjB\r` in one burst. It failed (`/tmp/cellbox-phase02-fgyv1u5a`): `cjA` ran, but `cjB` was typed ahead while `cjA` was running and was discarded (finding F-TA in S10-3b.1).
  - The check now sends each LF-terminated command on its own. Rerun in import mode on `a9f384a6…` (`/tmp/cellbox-phase02-_2x8tjut`): **PASS**, 48 passes plus `ctrl_j_submits: true` (`rerun-fleet/lanes/L1-p02-session-after-e-v2/`).
- **(f)** and **(g)** are fixed in a new slice **S10-3b** (below).

#### S10-3b — Ctrl-C for `sleep` and blocked sinks; `cat` of non-UTF-8 and oversized VFS files (**CP1 approved 2026-10-26**: D3b-1 B, D3b-2 A, D3b-3 A; **CP2 approved 2026-10-28**, including S10-3b.8 deviations 1–4)

Prepared 2026-10-26 by reading the source and the S10-3.11 evidence. The user approved this CP1 with every recommendation on 2026-10-26 (relayed by Main), and it was implemented as in S10-3b.8.

##### S10-3b.1 Root causes (exact lines)

**SC1-a — builtin `sleep` ignores Ctrl-C.**
- `cells/tools/cellbox/src/cmd_sys.rs:121-128`: `cmd_sleep` calls `crate::top::wait_ns(delay_ns)` (`:126`).
- `cells/tools/cellbox/src/top.rs:109-119`: `wait_ns` loops on the monotonic clock with one-tick `SetTimer` parks and **never looks at the session queue**.
- `sleep` is dispatched as `Catalog::Legacy` (`executor.rs:2249`), which can only return 0 or 1 (`:2211-2217`), so it could not report 130 even if it noticed the interrupt.
- `cmd &` runs builtins synchronously (`executor.rs:1246-1262`), so `sleep N &` blocks the shell the same way.

**SC1-b — a blocked sink (typed stream into builtin `cat`) ignores Ctrl-C.**
- `top --records … | to jsonl | cat` is a **Collected** typed run (`executor.rs:1889-1891`): `typed::run_rows_within` → `encode_all` (`cells/tools/cellbox/src/typed.rs:932-979`) pulls every item up to the 4 MiB `COLLECT_BOUND` with **no interrupt check**.
- `run_typed` already holds the job's interrupt latch (`executor.rs:1745`) and checks `latch.is_set()` after the run (`:1916-1919`), but nothing polls it during a Collected run. Only the streamed `TerminalSink` polls it (`:1600-1626`).
- The producer itself blocks too: `TopSource::next_frame` waits `crate::top::wait_ns(self.options.delay_ns)` (`native_rows.rs:271`), and `row_producer` hands the latch only to `rg` (`native_rows.rs:40-66`).
- The same uninterruptible wait sits in `top -b` (`top.rs:159`).

**SC2 — builtin `cat` prints a false "No such file or directory".** In `cells/tools/cellbox/src/commands.rs:386-393`, `cmd_cat` reads a VFS file with `read_file_vfs_owned(…, CAT_READ_MAX)` and prints it only `if from_utf8(&bytes)` succeeds. In two cases it silently falls through to the BootFS fallback (`:395-432`), where `sys_open` fails and the false `No such file or directory` is printed (`:427-431`):
- a non-UTF-8 file: the read succeeds but `from_utf8` fails;
- a file over `CAT_READ_MAX` (1 MiB; `cmd_fs.rs:827-828`): `read_file_vfs_owned` returns `InvalidArgument` (`cmd_fs.rs:816-817`).

The BootFS branch already emits lossy UTF-8, one U+FFFD per invalid sequence (`emit_utf8_lossy`, `commands.rs:441-476`). The P04 lanes rely on that for the S5-4 hostile fixture (`qemu-phase04-terminal-test.sh:430,853`). `cmd_fs::read_path_bytes_exact` (`cmd_fs.rs:830-867`) already maps VFS stat results to the right reasons (`Is a directory`, `File too large`, `Permission denied`, `Input/output error`).

**Finding F-TA (related, pre-existing; not in scope unless decided).** `poll_session_interrupt` (`executor.rs:1541-1566`) drains up to 16 queued session events per poll and **discards every non-Interrupt event**. Text typed ahead while a foreground command, typed sink or `rg` runs is therefore lost; this is what the first (e) check hit. The SC1 fix adds the same poller to `sleep` and Collected runs, so type-ahead during those is also discarded (D3b-3).

##### S10-3b.2 Minimal fix (cellbox only; no kernel, decoder, input-service, ABI or catalog change)

1. **Interruptible wait.** In `top.rs`, `wait_ns` becomes `wait_until(delay_ns, interrupted: &mut dyn FnMut() -> bool) -> Waited { Elapsed, Interrupted }`: the same one-tick parks, plus a poll before each park. A pure core takes an injected clock, park and poll so it can be host-tested. Callers:
   - `cmd_sleep`;
   - `TopSource::next_frame` (`row_producer` passes the job latch to `top`, as it does to `rg`);
   - `top -b` (D3b-1 B).
2. **`sleep` returns 130.** `sleep` moves from `Catalog::Legacy` to `Catalog::Status`. On Interrupted it prints `^C` on the console (as `cmd_top` does, `top.rs:145-147`) and returns `INTERRUPTED_STATUS` (130). It polls only through `job_latch()`, i.e. only when a serial or context session exists. A one-shot `sleep` stays cancellable by the kernel Operation cancel (S10-0).
3. **Collected runs poll the latch.** `run_rows_within` gains an `interrupted` callback, polled before each item is pulled. On interrupt it stops, closes the chain (the producer is released) and returns no bytes. `run_typed` passes `|| latch.poll_now()` and its existing `latch.is_set()` branch returns 130 with `^C`. A Collected run writes nothing on interrupt (no redirect is opened), as today.
4. **`cat` of VFS files.** When `stat_file_vfs(resolved)` says the VFS has the path, `cmd_cat` never falls through to BootFS:
   - a directory gives `Is a directory`;
   - a file over `CAT_READ_MAX` gives `File too large`, status 1 (D3b-2 A);
   - otherwise the bytes are emitted through the **existing lossy emitter** (valid UTF-8 unchanged, one U+FFFD per invalid sequence), the same policy as the BootFS branch (D3b-2 A).

   A VFS refusal stays `Permission denied`. A missing path keeps the BootFS branch unchanged, including its streaming of BootFS files of any size and its `No such file or directory`.

**Files:** `cells/tools/cellbox/src/{top.rs, cmd_sys.rs, executor.rs, native_rows.rs, typed.rs, commands.rs}` and their host tests; `docs/cellbox-cli.md` (cat/Ctrl-C notes); `CHANGELOG.md`.

##### S10-3b.3 The non-UTF-8 and oversized-file choice (D3b-2)

| Option | Non-UTF-8 VFS file | File over 1 MiB on VFS | Notes |
|---|---|---|---|
| **A (recommended)** | lossy text, one U+FFFD per invalid sequence, exactly as the BootFS branch | `cat: PATH: File too large`, status 1 | One output policy for both branches. Raw C1/invalid bytes never reach the terminal; valid-UTF-8 escapes go to the terminal's existing S5-4 hostile-output handling, as they do from the ramdisk. Small change; the 1 MiB bound is the existing `CAT_READ_MAX`. |
| B | refuse: `cat: PATH: not UTF-8 text`, status 1 | as A | Matches builtin `cat`'s stdin rule (S10-0 deviation 1) but differs from the BootFS branch, and the S5-4 hostile fixture could never be shown from a disk. |
| C | exact bytes | stream in chunks, unbounded | Raw C1/invalid bytes would reach the terminal (weakens the hostile-output policy), and a chunked VFS reader is new code. Not recommended. |

##### S10-3b.4 Tests

- **Host (cellbox):**
  - the wait core: Interrupted on the n-th poll, Elapsed at the deadline, Elapsed on a clock error, and no park after an interrupt;
  - `sleep` returns 130 on interrupt and 0 when elapsed;
  - `run_rows_within` with an endless fake source and an interrupt after k items: no bytes, the chain is closed, the interrupt outcome is reported;
  - the `cat` emitter on mixed input (valid UTF-8 unchanged, raw 0x9b/0x9d/0x9c and truncated sequences → U+FFFD, ESC sequences passed through);
  - the existing tests stay (208 pass today).
- **L0** (on the rebuilt candidate):
  - `P10C-ctrl-c` as specified: `sleep 30` plus Ctrl-C returns 130 within 2 s; the endless typed stream into `cat` returns 130;
  - new sub-checks: `top -b -d 5 -n 10` plus Ctrl-C returns 130; `cat` of the 7 MiB disk file gives `File too large` with status 1; `cat` of a non-UTF-8 disk file gives the exact lossy text with status 0;
  - `P10G-vt` hostile from the disk fixture renders exactly as in the P04 GUI lane.
- **Lanes:** the P04 terminal lane (the hostile relay and `oneshot-*` cat checks are unchanged in expectation) and the P02 session lane (`ctrl_j_submits`, `rg` Ctrl-C).

##### S10-3b.5 Rebuilt candidate and reruns

`cellbox` (and `/bin/cellbox-ssh`) changes, so the candidate `a9f384a6…` is superseded. Steps:
1. Rebuild C0–C6 with the same fleet kernel features.
2. Rerun the full S10-3 battery from the start:
   - L0 headless ×1, and desktop ×6 (consecutive);
   - L1 P02 session and kernel lane, L2, L3, L4 ×6, L5 and L5f, all in import mode;
   - L6 before and after;
   - the CBX-01/02 audit.
3. Rerun the default-mode runs once, as before.

No earlier run counts for the new bytes.

##### S10-3b.6 Risks

- **Type-ahead loss (F-TA):** the new pollers in `sleep` and Collected runs discard typed-ahead input, as every existing poller already does (D3b-3).
- **`sleep` status change:** an interrupted `sleep` now returns 130 instead of finishing with 0. Scripts that relied on an unkillable `sleep` change behaviour (intended).
- **`cat` output change:** a non-UTF-8 VFS file now prints lossy text where it used to print a false ENOENT; a directory now gives `Is a directory`.
- **Poll granularity:** the poll runs once per one-tick park, so the Ctrl-C latency is about one timer tick plus the session round trip.

**Rollback:** revert the six cellbox files and the tests; SC1 and SC2 return. No canonical byte is written.

##### S10-3b.7 Decisions for S10-3b CP1

| ID | Question | Recommendation |
|---|---|---|
| D3b-1 | Scope of the interruptible wait: **A** only `sleep` and the Collected sink; **B** also the `top` record producer's inter-frame wait and `top -b` (same root cause, `top.rs:109-119`) | **B**: one wait for every caller |
| D3b-2 | `cat` of a non-UTF-8 or oversized VFS file: **A** lossy text plus `File too large` (S10-3b.3); **B** refuse non-UTF-8; **C** exact or streamed bytes | **A** |
| D3b-3 | F-TA type-ahead: **A** keep today's behaviour (pollers discard non-interrupt events; documented); **B** in this slice, keep non-interrupt events in a bounded queue that the next line read consumes first | **A** now, with B as its own later slice if wanted |

##### S10-3b.8 Implementation and results (2026-10-26; **CP2 approved 2026-10-28** with deviations 1–4)

**Product changes** (cellbox only, as S10-3b.2; whole-file hashes in `reports/s10-3b-evidence/as-built.sha256`):
- **Interruptible wait (`top.rs`).**
  - `wait_ns` is replaced by `wait_until(delay_ns, interrupted) -> Waited { Elapsed, Interrupted }`.
  - The loop itself is `wait_core`, which takes an injected clock, park and poll. It polls before every one-tick park, and an unavailable clock counts as elapsed, as before.
  - `top -b` (`run_batch`) waits through the job latch; on Ctrl-C it prints `^C` and returns 130.
- **`sleep` (`cmd_sys.rs`, `executor.rs` catalog).** `sleep` is now `Catalog::Status(cmd_sleep)`. It waits through `job_latch()`, which is now `pub(crate)`. It returns 0 when the time elapses, and `^C` with 130 when interrupted.
- **`top --records` (`native_rows.rs`).** `row_producer` passes the job latch to `TopSource` as well as to `rg`. An interrupted wait between frames closes the source and ends the stream, and the consumer reports 130.
- **Collected runs (`typed.rs`, `executor.rs`).**
  - `run_rows_within` takes an `interrupted` poll. Every Collected sink (encoder, table, search text) reads through an `Interruptible` adapter that polls before each pull.
  - On interrupt the chain is closed, no bytes are returned, and the failure carries 130.
  - `run_typed` passes `latch.poll_now()`; `run_rows` and the tests pass `|| false`.
- **`cat` (`commands.rs`).**
  - When the VFS stats the path, `cat` no longer falls through to a false "No such file or directory":
    - a directory gives `cat: PATH: Is a directory`;
    - a file of 1 MiB or less is read whole and printed through the existing lossy emitter;
    - a read error gives `Input/output error`.
  - A file over 1 MiB goes to the BootFS branch. A boot-image file (the VFS shows the VIFS1 ramdisk as `/bin`) still streams at any size; any other file now reports `File too large` (status 1) instead of ENOENT.
  - A path the VFS does not have, or refuses, keeps the BootFS branch unchanged.
- **Docs:** `docs/cellbox-cli.md` has a new paragraph on Ctrl-C and on builtin `cat` of disk files.

**Tests:**
- **Host (cellbox): 213 pass** (208 before, plus 5 new):
  - `wait_core`: elapses at the deadline, stops at the n-th poll with no park after it, notices a pending interrupt even for `sleep 0`, and treats no clock as elapsed;
  - `sleep_status`: 0 or 130;
  - a Collected run over an endless source with an interrupt after 5 pulls, for three sink kinds: 130, 5 pulls, closed once (plus the bound control);
  - the whole-buffer lossy decode equals the chunked one, with ESC sequences passed through;
  - the `cat` VFS plan (Read / Directory / TooLarge at the 1 MiB edge).
  - Log: `reports/s10-3b-evidence/logs/cellbox-host-tests.log`.
- **P04 terminal lane (default mode, the regression run): PASS 494/494** (202 markers; `/tmp/cellbox-phase04-Sn1u`; `reports/s10-3b-evidence/phase04-lane/`).
- **L0 driver (`scripts/phase10_canonical_drive.py`):**
  - new data fixture `p10/lossy.txt`;
  - `P10C-bytes` gains `disk-non-utf8-lossy-text`, `disk-non-utf8-uart-bytes`, `disk-big-fixture-over-1mib`, `disk-oversized-file-too-large-1` and `disk-directory-is-a-directory-1`;
  - `P10C-ctrl-c` gains `top-batch-ctrl-c-prompt` and `top-batch-ctrl-c-130`.
- **On the rebuilt candidate** (S10-3.12; tabulated by code in `rerun/lanes/s10-3b-checks.json`), every S10-3b sub-check passes in every booted L0 run that reached it:
  - `sleep 30` plus Ctrl-C gives a prompt 0.3 s later and `$?` 130;
  - `top -b -d 5 -n 10` plus Ctrl-C gives 130;
  - the endless typed stream into `cat` returns 130 and writes nothing;
  - the lossy disk text is exact;
  - the 7,340,032 B disk file gives `File too large` with status 1;
  - a directory gives `Is a directory`;
  - all 27 `P10G-vt` HOSTILE checks pass in each of the 6 booted desktop runs that reached them (162/162).

  The only S10-3b sub-check failures are `sleep-ctrl-c-prompt` in headless runs 7 and 10. Both are part of the F2 cascade (S10-3.12): the editor was still open on its "save outcome unknown" prompt.

**Deviations (for the S10-3b CP2):**
1. **Oversized VFS file.** S10-3b.2 item 4 said a VFS file over 1 MiB gives `File too large`. The VFS also serves `/bin` from the boot image, so applying that rule to every VFS path would have broken the CP1's own guarantee that boot-image files keep streaming at any size. An oversized VFS path therefore tries BootFS first, and only a file BootFS does not have gets `File too large`.
2. **One adapter for every Collected sink.** The interrupt poll is a `RowSource` adapter (`Interruptible`) that covers all three sink kinds, rather than a check inside `encode_all` alone.
3. **New L0 fixture.** The lossy check uses `p10/lossy.txt` (a C1 CSI byte, `FF FE`, a truncated 4-byte sequence and a valid `€`) instead of `p10/raw.bin`. The raw payload contains ESC and NUL bytes, so its UART text cannot be compared exactly. The data tree grows from 11 to 12 files.
4. **Ctrl-C latency** is one timer tick plus the session round trip, as stated in S10-3b.6; 0.3 s was observed.

#### S10-3.12 S10-3 battery rerun after S10-3b (2026-10-26): stopped at SC4, L0 6/6 not reached

**Candidate** (`/tmp/cellbox-s10-3b-tsbn/cand/candidate.json`, SHA-256 `9c4d16ebcf61b8554aaa64e38ab1e0fda637dfe29943f21c671aaefb128e77bb`, no failures recorded). Built with the same script, the S10-2a routes and the fleet kernel (`policy-required`):

| Profile | Image | Kernel | ISO |
|---|---|---|---|
| headless | `29e3a0d4…` | `93f57333…` (15,595,584 B) | `3cd9ee4d…` |
| desktop | `b521055a…` | `929e5b42…` (28,178,496 B) | `a026edc2…` |
| pc | — | `2c711a14…` | `4d7c09e2…` |

Compared with `a9f384a6…`, only `/bin/cellbox` (= `/bin/cellbox-ssh`) changed in each profile, along with the images, kernels and ISOs that embed it.

**Observation O-1:** the headless `init` also changed (`7b13bbde…` → `30428a76…`) although no init input changed since the last candidate. The difference is `.strtab` and `.init_array` layout only, and the desktop `init` is byte-identical. `[INFERENCE: app-init is not reproducible across CARGO_TARGET_DIR copies]`; this matters for S10-2b/S10-5 determinism.

Evidence: `reports/s10-3b-evidence/rerun/`. Every summary there is produced by code: `lanes/summary.jsonl`, `lanes/l0-classification.json`, `lanes/s10-3b-checks.json`, `l6/l6-classification.json` and `audit/audit.json`.

| Lane | Result |
|---|---|
| L0 headless | **12 runs, 7 PASS**. 10 runs booted the candidate and 7 of those passed. The longest consecutive streak is 4. Failures: F1 ×2, F2 ×2, F4 ×1. |
| L0 desktop | **12 runs, 6 PASS**. 7 runs booted and 6 of those passed. The longest consecutive streak is 2. Failures: F1 ×5, F3 ×1. |
| L1 P02 session | PASS: 48 plus `ctrl_j_submits` (import mode); the default mode also passes. |
| L1 P02 kernel lane | PASS: atomic publication 39/39, VFS self-tests 7/7. |
| L2 P03 | PASS 67 (the default mode also 67). |
| L3 P04 terminal | PASS 494/494 (202 markers). |
| L4 GUI | **6/6 consecutive** (5 click + 1 autoload), 290/290 each; the default mode also 290. |
| L5 P09 | PASS 34/34; the default `--no-faults` run also 34. |
| L5f P09 | PASS 103/103. |
| L6 | **PASS**: 39 pass→pass, 1 pre-existing failure (`xhci_injected_key_decodes_in_cell_x86`), 4 skips, 1 ignored, **0 regressions**. The CI boot check passes on both sides. |
| Audit (CBX-01/02) | PASS: the five documents are at their digests and the scoped `cargo deny` is all ok. The workspace-wide `cargo deny` fails exactly as recorded before. |

**L0 failure classes.** Every failure is classified by code (`classify_l0.py`); none is unclassified. None is in a path that S10-3b changed.
- **F1: the fixture sshd did not start** (7 runs). The Docker OpenSSH fixture runs `apk add openssh-server` when the container starts, and no `Server listening` line appeared within 120 s. The runner stops before it boots the candidate, so no product code ran.
- **F2: NVMe I/O timeout** (headless runs 7 and 10).
  - `[nvme] io timeout opc=0` (a Flush) appeared after 50,000,000 polls, during the `P10C-edit` save.
  - The editor failed closed correctly: "save outcome unknown … edits kept and still unsaved", then the unsaved-changes prompt.
  - The driver does not answer that prompt, so every later marker of that boot fails.
  - The NVMe driver's budget is a poll count, not a time. The pre-S10-3b battery saw the same class at boot (S10-3 L6: an NVMe I/O timeout under load, and a baseline mount timeout on `build/vicell-x86.iso`).
- **F3: NVMe admin timeout at boot** (desktop run 5). `/mnt/sd` was not mounted, so `P10G-real-output`, `P10G-vt` and `P10G-tui` timed out on disk reads. The same signature is in `reports/s10-3-evidence/l6/rerun/before-nvme-x86-2.log` on the baseline ISO.
- **F4: the driver's idle detection** (headless run 2, `P10C-ssh-serial`).
  - The ssh command failed closed as specified: password prompt, 255, `authentication failed`, then the prompt.
  - Asynchronous service lines (`[net-bridge] NIC driver reply timeout…`, `[serial] serial cell ready…`) followed the prompt, so `Shell.idle()` (the UART must end on the prompt) never became true.
  - This is a weakness of the harness.

**SC4 (stop condition, S10-3.5).**
- Neither profile reached six consecutive passes within the bounded 12 runs per profile. Under S10-3.5, any failure (including an infrastructure flake) resets the count, and there is no best-of-N.
- This is not an S10-3b defect: every failure is F1–F4, and every S10-3b sub-check passes wherever it was reached.
- The rule requires a decision: the desktop image is withheld until it is taken.
- The headless count asked for by the S10-3b.5 plan (×1) is met; Main asked for ×6 consecutive.

**CBX outcomes on this candidate** (proposals only; no row flips while the slice is stopped):
- **Would be met:**
  - 01/02 (audit);
  - 03–05 (L1, `P10G-two-sessions`, `P10C-ctrl-c`; SC1 fixed);
  - 06–08;
  - 09;
  - 10;
  - 11;
  - 12;
  - 13 and 15 (L4, `P10G-vt` hostile 162/162, `P10G-tui`, `P10C-top`; SC2 fixed);
  - 17/18 (D60-3 noted);
  - 19/20;
  - 21/22 (synced, not crash-proof; F2 shows the editor failing closed on a timed-out flush);
  - 29 (L1 `ctrl_j_submits`, L3, `P10C-selective-tui`).
- **Partial as planned:** 23, 24, 25.
- **Not met:** **14** and **26**: L4 6/6 and L6 pass, but L0 6/6 is not reached (SC4).
- 16 stays as audited; 27 and 28 are not part of S10-3.

**Canonical artifacts unchanged** (`rerun/canonical/`).
- `before.sha256` and `after.sha256` are identical:
  - `kernel_fs.img` `447d0348…`, `init` `0c2f607b…`;
  - kernel `12cb0bac…`, `cellbox` `006b8504…`, `terminal` `018db0cc…`;
  - `vicell-x86.iso` `eb1cb813…`, `kernel.elf` `dbd862d6…`, `cellos-x86_64.iso` `b9a08fcb…`;
  - desktop fixture ISO `6af523ec…`, wip qcow2 `9aa29105…`, PC ISO `273a3be0…`.
- The embedded directory and the listings with mtimes are identical.
- No file under `target/x86_64-unknown-none`, `build/` or `kernel/src/` is newer than the start marker.
- The user's uncommitted `kernel/src/main.rs` change (2 insertions, 12 deletions) is untouched.

**S10-3 deviations for its CP2** (cumulative):
1. Fleet-profile kernel (`policy-required`, Tier 2 DISABLED), decision (b).
2. Desktop UART input goes to the focused GUI Terminal, decision (c).
3. Marker forms amended, decision (d).
4. The P02 check `ctrl_j_not_enter` is replaced by `ctrl_j_submits`, decision (e).
5. L0 deviations 1–7 (S10-3.11).
6. `P10G-delete-key` is excluded from the desktop count (a known defect).
7. The `x86_64-boot` Tier-2 refusal tests skip (D3-6).
8. D3-5, the completion-queue self-test FAIL line, is known and non-blocking.
9. L0 counts:
   - planned: headless ×1, desktop ×6;
   - run: 12 per profile (six first, then a continuation from the current streak, bounded at 12; `stream_d.sh`).
10. Harness:
    - `run_lane.py` substitutes `{work}` for the L0 `--work` argument;
    - the S10-3b driver checks (S10-3b.8 deviation 3);
    - F4 is a known driver weakness.
11. O-1: the headless `init` hash changed with no input change.
12. SC4 itself, pending the decision below.

**Decision needed (D3-8, SC4):**

| Option | What | Notes |
|---|---|---|
| **A** | **Harness hardening only, then rerun L0 6/6 in both profiles on the same candidate `9c4d16eb…`.** (i) Start the sshd fixture from a pre-built local image (no `apk add` at start), or retry the start before boot; a fixture that never started is not a run, because the candidate was not booted. (ii) `Shell.idle()` ignores asynchronous `[service]` lines after the prompt. (iii) F2 and F3 still count as failures. | No product change. F2/F3 alone hit 3 of 17 booted runs, so 6/6 may still fail; if it does, C follows. |
| B | A, plus count F2/F3 NVMe poll-budget timeouts as infrastructure flakes that do not reset the count | Amends the S10-3.5 "no best-of-N" rule. |
| C | A new slice: NVMe driver deadlines by time instead of poll count (own CP1), then rerun | A product change outside cellbox; a rebuilt candidate and a full rerun. |
| D | Accept as is: withhold the desktop image (D10-3 B); headless proceeds on its ×1 plan count | CBX-14/26 stay not met for desktop. |

Recommendation: **A** (and C only if NVMe timeouts break 6/6 again).

**Decision recorded (user, 2026-10-26, relayed by Main): D3-8 = C, started at once.** The harness is fixed (a prebuilt ssh fixture image, a run that never booted does not count, prompt detection that ignores background log lines), and the NVMe driver's timeouts become time-based. Both go into a new slice **S10-3c** (below), which needs its own CP1. S10-3 then reruns on a rebuilt candidate.

#### S10-3c — time-based NVMe timeouts and L0 harness hardening (**CP1 approved 2026-10-27**: D3c-1 A, D3c-2 B, D3c-3 A, D3c-4 A, D3c-5; **CP2 approved 2026-10-28**, including S10-3c.11 deviations and retained SC5 history)

Investigation prepared 2026-10-26. The user approved every recommendation on 2026-10-27 and explicitly requested continuation. The implementation reuses the partial NVMe/init/fixture/prompt changes already present; the approved verification results are recorded below. No canonical promotion or remaining containment-guard removal is authorized by this slice.

##### S10-3c.1 The poll-count timeouts (exact lines)

All of them are in `cells/drivers/nvme/src/controller.rs`. There is one budget, `POLL_WARN_ITERS = 50_000_000` (`:58`), with a `yield_now()` every `POLL_YIELD_EVERY = 8192` polls (`:62`):

| Wait | Lines | Message on expiry | Seen in S10-3.12 |
|---|---|---|---|
| reset: `CSTS.RDY` → 0 after `CC.EN=0` | `:111-129` | `reset wait failed … in N polls` | — |
| enable: `CSTS.RDY` → 1 (`CSTS.CFS` is checked) | `:153-178` | `RDY wait failed … in N polls` | — |
| every admin command (`admin_cmd`: Identify controller/namespace, Create CQ/SQ) | `:360-410` (expiry `:402-408`) | `admin timeout after N polls` | **F3**, desktop run 5: the first Identify after `dma iovas: identify` |
| every I/O command (`submit_io`: read, write, **flush**) | `:434-508` (expiry `:487-506`) | `io timeout opc=… after N polls (…)` | **F2**, headless runs 7 and 10: `opc=0` = Flush |

The comment at `:50-57` already admits the problem. A Flush is a host `fsync`, and the budget was raised from 1e6 to 5e7 so that it would act like "wall-clock time". It is not wall-clock time:
- Each poll is one volatile read of the completion slot in RAM.
- A `yield_now()` returns at once when no other Cell is runnable. In an idle guest (the editor waiting on its save), 5e7 polls therefore last a short, CPU- and TCG-dependent time.
- During a busy boot, each yield can give away a whole tick, so the same budget becomes much longer.

`[INFERENCE]` In the idle case 5e7 polls under TCG last seconds rather than tens of seconds. S10-3c logs the elapsed time, so this is measured, not assumed.

**Host fsync on this machine** (a 256 MiB sparse image in `/tmp`, 64 random 4 KiB writes, then `fdatasync`, 20 samples, idle host): 82 ms minimum, 141 ms median, 486 ms maximum. The L0 fixture disks are 256 MiB images in `/tmp`, and other QEMU and Docker work raises this tail.

**A second count-based wait at boot.** `cells/tools/init/src/boot.rs:140-155` (`start_block_drivers`) waits at most **400 `yield_now()`** for any `BLOCK_DRIVER` registration, then starts the VFS anyway. In F3 the VFS mounted while the NVMe cell was still waiting for Identify: `[vfs] WARNING: FAT32 /mnt/sd mount failed` (line 215) comes 174 lines before `[nvme] admin timeout` (line 389). The VFS never retries a mount, so even an Identify that is only slow costs `/mnt/sd` for the whole boot.

**Who waits on the driver.**
- `cells/services/vfs/src/blk_router.rs` (`blk_read`/`blk_write`/`blk_flush`, `:63-158`) blocks in `sys_recv(tid)` with no timeout.
- The editor's save goes through `VfsRef::call` → `service_call_typed`, which has no client deadline (`libs/ostd/src/service.rs:167-207`, `ipc.rs` `exchange(…, None)`).
- So the driver's budget is the only bound on a save.

##### S10-3c.2 Clock source available to the driver

- The NVMe Cell declares no `declare_syscalls!` section, so it keeps the `u64::MAX` allowlist (`kernel/src/task/syscall.rs:4365`, "Cells without that section retain their TCB's `u64::MAX` allowlist"). That makes `GetTime` available.
- `ostd::syscall::sys_get_time_ms()` (`libs/ostd/src/syscall.rs:1856-1863`) gives monotonic milliseconds since boot, normalised by the kernel. It returns `None` when the kernel or the allowlist does not provide it.
- AHCI already uses it for its COMRESET delay (`cells/drivers/ahci/src/controller.rs:812-834`). `ostd::session::monotonic_ns()` (GetTime ticks + GetTimerFreq) is the finer alternative.
- Proposed: `sys_get_time_ms()`, because millisecond granularity is enough for second-scale budgets. When it returns `None`, the driver falls back to today's poll cap, logged once, so no board loses its bound.

##### S10-3c.3 Proposed budgets (D3c-1)

| Wait | Budget | Why |
|---|---|---|
| reset and enable (`CSTS.RDY`) | `CAP.TO` × 500 ms, at least 1 s | The NVMe specification's own worst case for RDY transitions. The driver already reads `CAP` (`:89-93`) and only needs to keep the `TO` field (bits 31:24). `[INFERENCE: QEMU reports CAP.TO = 0xF, i.e. 7.5 s]`; the driver logs the value. |
| admin commands | **60 s** | Linux's `nvme_core.admin_timeout` default. Admin commands run only at init. |
| I/O: read, write and **flush** | **30 s** | Linux's `nvme_core.io_timeout` default. More than 60× the worst idle host `fdatasync` measured above (486 ms). |

Unchanged:
- the 8192-poll yield cadence;
- the CID-skip handling of late completions (`:367-380`, `:441-457`);
- the `Err(IO)` returned on expiry.

New diagnostics:
- every expiry logs its elapsed milliseconds;
- the first few completions slower than 1 s log `[nvme] slow completion opc=… ms=…`, so later evidence shows how close a run came to a budget.

##### S10-3c.4 Boot ordering (D3c-2)

With time budgets alone (A), a slow Identify still loses the race with the VFS mount. In B, `start_block_drivers` keeps the `/bin/nvme` tid that `sys_spawn_from_path` returns and waits on a monotonic deadline:
- until any `BLOCK_DRIVER` registers;
- or until the NVMe Cell has exited (no controller: it exits after ≤200 yields at `main.rs:59-69`), after which the old 400-yield window applies for the other drivers;
- or until **60 s** have passed (one NVMe admin budget); after that the VFS starts as today.

`task_terminated` already exists in init (`boot.rs:117`). The AHCI Cell idles without exiting when there is no controller, which is why the wait follows the NVMe tid and not "all drivers".

##### S10-3c.5 Effect on "save outcome unknown"

- **Today.** A Flush that runs out of the poll budget makes the driver reply non-zero. Then:
  - `blk_flush` returns false;
  - CellosFS `sync()` fails inside `publish_if` (`cells/services/vfs/src/backend_cellosfs.rs:306-319`), and the backend is **poisoned for the rest of the boot**;
  - the reply is `SaveOutcome::Indeterminate`;
  - the editor reports `save outcome unknown … edits kept and still unsaved` (`cells/tools/cellbox/src/edit/store.rs:40-48`, `editor.rs:367`).

  Run 10 shows the poisoning afterwards: `rg: /srv/p10ed/new.txt: not found or I/O error`.
- **With S10-3c:**
  - A flush that takes up to 30 s completes. The save becomes `Committed`, and the volume is not poisoned.
  - Only a flush still unanswered after 30 s takes today's path (Indeterminate and poisoned). That path stays correct and fail-closed, and its semantics, wording and the "synced, not crash-proof" statement (D79-12) are unchanged.
  - Cost: the VFS is single-threaded and blocks in `blk_flush`, so a slow flush now stalls every VFS client for up to 30 s, where today it fails after the poll budget. The shell cannot be interrupted during a save, as today.

##### S10-3c.6 Harness fixes (no product change; part of decision C)

- **H1, a prebuilt ssh fixture image.**
  - Today `scripts/phase09_ssh_fixture.sh:73-90` starts `alpine@sha256:ce64758a…` and runs `apk add -q openssh-server` inside the container. That needs the network and a mirror on every start, and it timed out in all 7 F1 runs.
  - Proposed: `start` builds, once, a local image `cellbox-p09-sshd:<content hash>` from that same pinned base. The build installs `openssh-server` and records its version in `DIR/sshd-version` as today. Later starts reuse the image and install nothing.
  - The build is the only step that needs the network. It runs before any boot and fails as a fixture error.
  - The image tag is the SHA-256 of the build recipe (base digest + package list), so a changed recipe gives a new tag.
  - The P09 lanes (L5/L5f) use the same script and gain the same robustness. Their checks (host key, ports, `RekeyLimit` server, `p9` helper, `PerSourcePenalties no`) are unchanged.
- **H2, a run that never booted does not count.**
  - `scripts/qemu-phase10-canonical-test.sh` exits with a distinct status **3** ("candidate not booted: fixture") when the failure happens before the first QEMU start. Today this is status 1 at `:202`.
  - The L0 stream treats status 3 as not a run: it neither resets nor extends the streak, and it does not use up one of the bounded runs. Three consecutive status-3 runs stop the stream as an infrastructure blocker.
  - This amends S10-3.5 ("any failure resets") for runs that never booted the candidate, and only for them.
- **H3, prompt detection ignores background log lines.**
  - `Shell.idle()` (`scripts/phase10_canonical_drive.py:293-296`) requires the UART to end on the prompt.
  - Proposed: after the last `Cellos > `, it skips asynchronous lines: a Cell line whose text starts with a `[service]` tag (`^\[[a-z0-9-]+\] `, e.g. `[net-bridge]`, `[serial]`, `[nvme]`) and kernel log lines (`KERNEL_LINE`).
  - It also requires that the prompt count has increased, as `wait_prompt` already does. A command's own output after its prompt cannot exist, so this cannot pass a still-running command.
  - The skipped lines stay in the logs and in `logical_lines`, so no check loses them.

##### S10-3c.7 Tests

- **Host, NVMe (D3c-3):**
  - Today `cargo test -p driver-nvme` does not link on the host, because of an unconditional `#![no_main]` (`main.rs:16-17`).
  - Gate it as `app-net-tools`/`app-wasm` do (`cfg_attr(not(test), …)`), and add the crate to CI's host unit-test step.
  - Test a pure `Deadline` over an injected clock:
    - expiry at the budget;
    - no expiry before it;
    - a `None` clock falls back to the poll cap;
    - `CAP.TO` → budget, with the 1 s floor;
    - the slow-completion threshold.
  - The CID-skip logic stays as it is.
- **Host, init (D3c-2 B):** the wait decision as a pure function over (registered, NVMe exited, elapsed). Cases: registered, NVMe exited then the 400-yield window, and expiry at 60 s.
- **Harness:** `scripts/test_phase10_canonical_drive.py` (new, host-only) covers `idle()`:
  - a prompt followed by `[net-bridge]` and `[serial]` lines is idle;
  - a prompt followed by command output is not;
  - a line containing `Cellos >` that is not a prompt line is not.

  Also check the fixture image tag derivation and the stream's handling of status 3 (a dry run over a fake `summary.jsonl`).
- **Lanes (on the rebuilt candidate):**
  - the L6 `nvme-x86` consumer (3 tests) and `ahci-x86`;
  - the P02 kernel lane (VFS self-tests);
  - L3, which exercises the `/mnt/sd` and `/srv` saves;
  - L0 `P10C-edit` with the new `slow completion` lines collected by `classify_l0.py`.

  F2/F3 are not injected; see D3c-4.

##### S10-3c.8 Rebuild and full rerun

1. Build: the nvme Cell (and init, if D3c-2 B) change, so rebuild C0–C6 with the same fleet kernel features. This gives a new candidate; `9c4d16eb…` is superseded.
2. Run the full S10-3 battery from the start, with H1–H3 in place:
   - L0 headless **×6 consecutive** and desktop **×6 consecutive** (12 counted runs per profile at most; status-3 runs not counted);
   - L1 P02 session and the kernel lane;
   - L2;
   - L3;
   - L4 ×6;
   - L5 and L5f;
   - L6 before and after, with the CI boot check;
   - the CBX-01/02 audit;
   - the default-mode runs once each.
3. Prove canonical hashes before and after. `classify_l0.py` gains the `slow completion` and elapsed-time fields.

No earlier run counts.

##### S10-3c.9 Risks

- **Longer stalls on a dead device.** A device that never answers now holds the single-threaded VFS for 30 s per I/O command, or 60 s per admin command, before `IO`. A FAT walk issues many reads, so a dead disk can take minutes to surface. AHCI poisons its controller after an unrecoverable fault; that is not proposed for NVMe here.
- **Boot delay (D3c-2 B).** On hardware whose NVMe hangs during init, the VFS starts up to 60 s later. Today it starts at once, without the disk.
- **No clock.** On a profile without `GetTime` the poll cap stays the bound, as today, and is logged once.
- **F3 may not be slowness.** If the Identify completion was lost rather than slow, 60 s ends the same way. The elapsed-time log then tells the two apart, and a recurrence becomes a new finding, not a flake.
- **Fixture image.** The build needs the network once; a host without it fails before boot (status 3). The image lives in the local Docker store; deleting it only means a rebuild.
- **H3.** A `[tag]`-looking line printed by a command after its own prompt cannot exist; the stricter "prompt count increased" check stays.
- **Scope.** This touches the NVMe Cell (every x86 image), possibly init (every image, all arches, because `start_block_drivers` is shared) and CI's host-test list. There is no ABI, policy or catalog change.

**Rollback:** revert `controller.rs`/`main.rs` (and `boot.rs`) and the harness files. F2/F3 return as flakes. No canonical byte is written.

##### S10-3c.10 Decisions for S10-3c CP1

| ID | Question | Recommendation |
|---|---|---|
| D3c-1 | Budgets: **A** RDY by `CAP.TO` (≥1 s), admin 60 s, I/O and flush 30 s (the Linux defaults); **B** 10 s for everything; **C** A, but flush 120 s | **A** |
| D3c-2 | Boot ordering: **A** driver only (F2 fixed; a slow Identify still loses `/mnt/sd`); **B** also init's block-driver wait by time (until registered, or the NVMe Cell exits, or 60 s) | **B**: F3 is the same count-based pattern, in init |
| D3c-3 | NVMe host tests: **A** gate `no_main` for tests and add `driver-nvme` to CI's host unit tests; **B** put the deadline helper in `ostd` | **A**: keeps the change in the driver |
| D3c-4 | Fault injection: **A** none, rely on host tests, the elapsed/slow-completion logs and 6/6; **B** an extra lane-only run with the fixture disk behind a QEMU `throttle` filter, to force a slow flush `[INFERENCE: whether throttle delays Flush is unverified]` | **A** |
| D3c-5 | H2 limits: status-3 runs are uncounted, and 3 in a row stop the stream (as S10-3c.6) | as proposed |

**CP1 record (2026-10-27).** The user approved D3c-1 **A**, D3c-2 **B**, D3c-3 **A**, D3c-4 **A**, and D3c-5 as proposed. In particular, no fault-injection lane is added; status-3 attempts do not consume or reset counted L0 runs, and three consecutive such attempts are an infrastructure stop.

##### S10-3c.11 Implementation and exercised verification (2026-10-28; **CP2 approved 2026-10-28** with deviations/findings below)

**Implemented.** The prior partial driver/init changes were reused, not rebuilt from a second design. `deadline.rs` uses `sys_get_time_ms()` at the existing 8192-poll yield cadence: reset/enable `CAP.TO × 500 ms` with a 1 s floor, admin 60 s, all I/O including Flush 30 s. No-clock waits retain the former 50,000,000-poll cap with one driver log. Timeout logs include elapsed time; the first eight ≥1 s command completions are logged without an underflowing counter or formatting allocation. Existing CID handling, `Err(IO)`, Indeterminate save outcomes and volume poisoning are unchanged.

Init tracks the NVMe tid, not AHCI lifetime: a registered driver or 60 s ends the timed wait; an exited/absent NVMe uses the former 400-yield window for the remaining drivers, also used without a clock. NVMe/init host entry gates and CI's host-test entry are enabled.

H1 reuses a prebuilt pinned-Alpine image tagged by the full recipe SHA-256; cached starts install nothing and forbid pulls. H2 returns status 3 for preparation failures before actual QEMU launch; the resumable stream leaves streak/count unchanged and stops after three consecutive status-3 attempts. H3 skips asynchronous service/kernel lines, requires a new prompt count, and frames the genuine unprefixed prompt immediately after explicit alternate-screen restoration. Raw logs are retained unchanged.

| Exercised check | Actual result | Evidence |
|---|---|---|
| Host `cargo test -p driver-nvme -p app-init -p app-cellbox -p service-vfs --target x86_64-unknown-linux-gnu` | **6 / 13 / 213 / 109 passed**, zero failures | `reports/s10-3c-evidence/logs/host-tests.log` |
| Host harness behavior | Initially 25 passed; **27 passed** after the retained-UART TUI-framing regression tests | `logs/harness-tests*.log` |
| Default P04 terminal regression | **494/494, 202 driver markers**; `/tmp/cellbox-phase04-jyc28jns` | `rerun/lanes/regression-p04-terminal/` |
| Default P02 kernel regression | **PASS**, VFS self-tests **7/7**; `/tmp/cellbox-phase02-y9quex7v` | `rerun/lanes/regression-p02-kernel/` |
| C0–C6 fleet candidate | **PASS**, both images and three kernels/ISOs; exact image/init embed checks true, no candidate failures | `rerun/candidate/candidate.json`; build logs |
| Canonical before/after | **469 files unchanged** (embedded directory, release files, build ISOs/qcow2/kernel ELFs); user `kernel/src/main.rs` SHA-256 unchanged: `a9636fee…08c64` | `canonical/before.json`, `after.json`, `comparison.json` |

The retained candidate is `/tmp/cellbox-s10-3c-resume/cand/`; its files are read-only. `candidate.json` SHA-256 is **`0718a96542e7a3f4d223b02d5ad0511a2e23d83b8cbdb690450aea7662a62863`**. Kernels use `default,policy-required`, plus `board-x86-pc` for PC.

| Profile | `kernel_fs.img` SHA-256 | Kernel SHA-256 | ISO SHA-256 |
|---|---|---|---|
| headless | `c2ac47697ccc3607aaad34fca12e727994090e8c2e271782071598b88accc1e4` | `1de2547348f2213f867b78a9349634180766ecea5fc24f869ff979890fccfaca` | `e6713da24de982ea6c7d30550606cd21a06efa6c317b280add088053bf73888c` |
| desktop | `ce9d36eb856dce34c287f3a2a4e0b1410f5a637519c6297ae2395739dea5e024` | `f16d276bb75085efd6a84dc7ea9d6dcf7f96acf39bb3cf37812af2af47eff765` | `637b753ffdae379b3ef63c4e3dadd535e9a1bd5e99bcbe0958ec79c6e214742a` |
| PC (headless image) | same headless image | `7454f0e2ba0d66feb16863db5273cca46843950a9007c8abda812dec0cfbd3ee` | `df14c24d9b884543ff6de446f9d8f479b85b1420d790728f181abf13cebd68fe` |

**Deviations/findings for CP2.**
1. The first L0 headless attempt exposed an H3 harness regression, not a product failure: Ratatui's alternate-screen exit and mode restores are immediately followed by `Cellos > ` without a newline/`USER:` prefix. Exact-line prompt recognition rejected all five such exits (top and editor), though status/save/read-back checks passed. `logs/h3-tui-diagnosis.json` derives all five boundary mismatches from the retained raw UART. The fix frames only the explicit terminal-restore boundary, retaining rejection of arbitrary output containing the prompt. Two new host tests pass. The original failed attempt remains counted and its verdict is untouched.
2. The candidate was **not rebuilt** after that host-harness-only correction; no candidate product bytes changed. The source digest in `candidate.json` therefore predates the final H3 correction; final harness source hashes and snapshots are archived separately (`logs/execution.json`, `harness/`).
3. Observed QEMU `CAP.TO` gives the **1000 ms floor**, not the earlier unverified 7.5 s inference. The SSH boot's actual Identify expiry is **60000 ms**; no slow-completion event was observed. This distinguishes the new SC5 from the old short poll budget, without claiming why the device failed to complete.
4. Host suites pass with the existing cellbox/VFS unused-code warnings. Both harness host runs also emit an **unclosed mocked QEMU stderr-file ResourceWarning**; it is disclosed, not presented as a warning-free run.
5. The full rerun is **incomplete because SC5 required an immediate stop**, not because its remaining requirements were removed. No timeout increase, retry/recovery path, fault injection, canonical write or containment change was added. `[selftest] COMPLETION-QUEUE: FAIL` remains the separately accepted pre-existing non-blocking defect.

#### S10-3.13 S10-3 rerun after S10-3c (2026-10-28): stopped at SC5, full-budget Identify timeout

Evidence is archived under **`reports/s10-3c-evidence/`**, with the S10-3 rerun in **`rerun/`**. Summaries (`logs/execution.json`, `rerun/lanes/summary.jsonl`, `l0-classification.json`, candidate metadata and canonical comparison) are generated by the retained harness scripts, never manually edited.

- **L0 headless attempt 1**: **14/16 markers**, exit 1, `/tmp/cellbox-phase10-s6pjo8ow` (949.2 s). Only top/editor exit-prompt checks fail; the H3 framing defect above was diagnosed from retained logs, fixed and host-tested before continuation.
- **L0 headless attempt 2**: **15/16 markers**, exit 1, `/tmp/cellbox-phase10-w5t1anar` (735.4 s). All main-boot markers, including top, editor saves/read-back and Ctrl-C, pass. The SSH boot fails `P10C-ssh-serial:no-echo-after-password-prompt` because storage never became available—not because a password echoed.
- **SC5 evidence**: `serial-ssh.log:187` submits Identify; **`:201` logs `[nvme] admin timeout opc=6 after 60000 ms`**; `:202–203` logs controller-init IO and NVMe exit; `:212–215` shows failed FAT/CellosFS mounts; `:362` reports `ssh: host trust store unavailable`. The boot never reaches the expected password prompt. This is the S10-3c.9 full-budget recurrence: **a new storage/product finding, not an accepted flake**. Whether the completion was lost or the device never completed is not established.
- Headless streak is **0**, with **2 counted attempts**, no status-3 attempts. **Desktop has 0 attempts**. No 6/6 qualification is claimed.
- **Not run after SC5**: imported L1 P02 session/kernel, L2 P03, L3 P04 terminal, L4 GUI ×6, L5/L5f P09, L6's 12 consumers and CI-boot baseline comparison, CBX-01/02 audit, and default P02 session/P03/GUI/P09. The earlier S10-3b results do not count for this candidate. Only the default P04 terminal and P02 kernel regressions listed above were exercised before the rebuild.

**Per-CBX disposition (no acceptance row flips).**

| CBX | Outcome on this rerun |
|---|---|
| 01, 02 | Existing private status retained; fresh candidate audit/cargo-deny **not run** |
| 03, 04, 05 | P04 regression and headless main observations only; imported L1/desktop evidence **not run**, no new integrated closure |
| 06, 07, 08, 09 | Main headless typed/byte/rg/state/Ctrl-C checks and P04 regression pass; imported L2/L3 **not run** |
| 10, 11, 12 | Main headless image/forms/denials checks and P04 regression pass; full matched/desktop evidence **not run** |
| 13, 15 | Main headless top/editor TUI checks pass after H3 correction; desktop/L4 **not run** |
| **14** | **Not qualified**: desktop/L4 6/6 not run |
| 16 | Existing audited P05 evidence unchanged; not rerun by design |
| 17, 18 | Main headless top checks and P04 regression pass; imported lane **not run**; D60-3 unchanged |
| 19, 20 | Main headless rg and P04 regression pass; imported L3 **not run** |
| 21, 22 | Main headless saves/read-back and P04 regression pass; desktop/L4 **not run**; still “synced, not crash-proof” |
| 23, 24, 25 | Remain **partial/blocked** at their existing ceiling; SSH boot failed SC5, L5/L5f **not run**; guest/publickey gaps unchanged |
| **26** | **Not qualified**: L0 failure; desktop/L4/L6 requirements unmet |
| 27, 28 | Outside this slice (S10-4/S10-5), unchanged |
| 29 | Main headless selective-TUI and P04 regression pass; imported L1/L3 **not run** |

**Decision required (D3c-6).** Recommended **A**: authorize a separately scoped NVMe Identify/completion investigation and a new fix CP1 if product changes are needed, then rebuild and rerun the complete battery. **B**: keep this read-only candidate and stop qualification pending that work. Neither choice waives SC5, raises the timeout speculatively, or permits promotion. S10-3a/b/c CP2 remains pending; S10-3/S10-4/S10-5 and rollout remain blocked.

**D3c-6 decision (user, 2026-10-28, relayed by Main): investigate now and propose a concrete fix; no product fix until a new CP1.** The investigation below reuses the SC5 artifacts and candidate. Disposable timing/trace probes are diagnostic evidence only, not resumed qualification.

#### S10-3d — serialize PCI enumeration before driver use; repair stale-CQ progress (**CP1 approved 2026-10-28**: D3d-1 A, D3d-2/3/4; **CP2 approved 2026-10-28**, including S10-3d.7 deviations/disclosures)

##### S10-3d.1 Root-cause finding and confidence

**Root cause of the reproduced failure mechanism:** the x86 kernel's fallback PCI scan exposes the NVMe device before the concurrently running Platform Cell finishes its own destructive BAR-size probing. Platform temporarily clears PCI Command.MemorySpace and writes all ones to BAR0/BAR1. NVMe can then read zero from the temporarily unmapped old BAR, silently lose reset/AQA/ASQ/ACQ/enable writes, and later ring a doorbell against the controller's still-active firmware queues after Platform restores the BAR. Its intended CQ receives no completion. The 60 s timeout is correct; waiting longer does not reconstruct the lost programming.

**Confidence: CONFIRMED for this mechanism** (controlled scheduling reproduction + device-side PCI/MMIO trace + physical SQ/CQ/Identify snapshots; a CPU-poll visibility miss is eliminated in that reproduction). **Attribution to the original SC5 boot: PROBABLE**, not retrospectively proven: its `CAP.TO` floor/registration ordering matches the reproduced mechanism, but no QEMU trace or DMA snapshot was captured at that original failure.

Evidence is archived in **`reports/s10-3d-investigation/`**; raw diagnostic outputs/binaries remain in **`/tmp/cellbox-s10-3d-investigation/`**. No repository product source, canonical byte, candidate byte or user's `kernel/src/main.rs` edit changed during this investigation.

| Boundary | Source/evidence | Finding |
|---|---|---|
| Early discovery | `kernel/src/main.rs:828–842`; `kernel/src/task/drivers/pcie_ecam.rs:739–823` | Kernel fallback publishes PCI_DEVICES before cells execute. NVMe need not await Platform's registration. |
| Concurrent destructive probe | `cells/services/platform/src/scan.rs:146–176,235–279` | Probe clears MemorySpace, writes all ones, restores BAR/command; no serialization with owner-driver MMIO. Duplicate registration does not prevent probing. |
| Original SC5 ordering | retained `serial-ssh.log:163–187,201–215` | NVMe CAP budget **1000 ms**, admin register log before Platform's NVMe registration, then Identify timeout **60000 ms**. CAP=0 is consistent with an unmapped read, not proof of QEMU's normal capability value. |
| Working control | diagnostic `probe-1/nvme.trace:4562–4586`; `probe-1/serial.log:165` | Real QEMU CAP.TO gives **7500 ms**; trace sees reset, new ASQ/ACQ and first Identify CID 1 with a successful completion. |
| Natural concurrent mutation observed | `probe-2/qemu.stderr.log:4787–4808` | Device-side trace observes Platform clearing command `0x107 → 0x105`, probing/restoring NVMe BARs while the owner driver can already run. |
| Forced first-Identify overlap | `probe-forced/serial.log:164,184–196,205`; `probe-forced/qemu.stderr.log:4873–4907` | Existing BAR-all-ones interval is held for 200 ms; diagnostic NVMe adds only 50 initial yields. Driver logs CAP floor **1000 ms**, but QEMU never receives its new ASQ/ACQ programming. After restore its tail=1 causes QEMU to fetch **firmware CID 3/CNS 0**, not intended CID 1/CNS 1. Timeout is **60111 ms**. |
| Missing versus missed CQE | `probe-forced/admin-sq.bin`, `admin-cq.bin`, `identify.bin`; generated `summary.json` | Intended SQE contains opcode 6/CID 1/valid PRP/CNS 1. Intended CQ and Identify response are **all zero**. The device did not complete into this queue; this is not a matching completion missed by the CPU's phase/CID predicate. |

The controlled reproduction changes **only disposable source copies**: Platform's existing BAR-probe interval is widened and ordered early; NVMe's initial scheduling is delayed, with controller, queue, DMA and deadline code unchanged. The probe image is diagnostic-only (original candidate remains `0718a965…`), not a proposed delay-based fix. Primary sources for the device-side interpretation: [QEMU v8.2.2 NVMe command fetch/completion](https://github.com/qemu/qemu/blob/v8.2.2/hw/nvme/ctrl.c#L6990-L7035) and [unassigned reads return zero / writes ignored](https://github.com/qemu/qemu/blob/v8.2.2/system/memory.c#L1309-L1324).

**A second confirmed defect, exposed after the I/O overlap:** `admin_cmd` and `submit_io` cache CQ head and phase before their loops (`controller.rs:380–399,444–466`). On an unexpected/late CID they update queue fields and `continue`, but never refresh the cached slot or phase and bypass `poll_spent`/yield. They can ring the same head indefinitely and ignore their timeout. In `probe-overlap`, the unchanged NVMe driver times out its first VFS read after **30000 ms**, then its CQ contains late CIDs 2/1/3; the device trace repeats the same I/O-CQ head doorbell without progress. The raw trace has over 153 million lines, retained in `/tmp`; archive includes generated hash/line count and first/last excerpts, not a manually edited verdict. This bug is **not the cause of SC5's first Identify** (zero CQ/no earlier command), but must not remain as a false “bounded wait” claim after an actual timeout.

##### S10-3d.2 What was excluded; incidental findings

- First-Identify buffers/queues remain live and pinned through submission; no free/reuse occurs before the missing completion. Returned IOVAs match the logged physical addresses in this identity-mapped Q35 lane. AHCI owns a distinct BDF/BAR; no cross-controller reset or overlapping legal MMIO claim was found.
- SQ/CQ entry layouts, initial zeroing/phase, opcode/CID/PRP/CNS encoding and volatile CQ reads are consistent with the successful control and physical snapshot. x86 BAR mappings are cache-disabled; no observed cache-coherence or release-ordering failure explains the controlled lost programming.
- Init/VFS handoff is downstream, not the first failure: original NVMe exits IO, then VFS starts without a registered disk and cannot mount the protected SSH trust store. No password was leaked.
- **Incidental, not silently added to this fix:** authorized Identify buffers call `DmaBuf::free`, but pinned `GrantFree` is refused and its return is ignored (`libs/ostd/src/dma.rs:62–64`); this is a confirmed lifetime/resource leak, also visible in successful logs, not first-Identify reuse. CAP-derived I/O doorbells can exceed the fixed 16 KiB MMIO claim for large DSTRD because raw accesses bypass bounds checks; not applicable to this QEMU stride. These need separately scoped remediation, not an ABI/lifetime expansion hidden in S10-3d.
- `pcie_ecam::is_platform_registered()` is **not** a completion barrier: despite its comments, `RegisterPcieBar` marks it on the first BAR (`syscall.rs:7865–7868`). It must not be reused as scan-complete evidence.

##### S10-3d.3 Diagnostic execution and limitations (actual, not qualification)

- **Eight** unchanged-candidate QEMU-side trace controls (`probe-1` through `probe-8`) reached the shell; they capture normal fetch/completion and/or natural config mutation, **not eight qualifying L0 passes**.
- The widened decode-off I/O overlap (`probe-overlap`) reproduces a lost I/O doorbell and the stale-CID spin, with unchanged driver bytes. Its 100 s diagnostic window ends without shell; original I/O budget remains 30 s.
- An intermediate first-Identify probe (`probe-identify-2`) accidentally used GetTime outside Platform's explicit syscall allowlist, panicked with the BAR un-restored, then timed out Create-CQ opcode 5. **Excluded from causal proof**; source/output are retained. One earlier `probe-identify` launch hit a host QMP socket readiness race and never produced guest evidence.
- The final controlled first-Identify probe adds GetTime **only to the disposable Platform copy**, uses 200 ms scheduling instrumentation and the initial NVMe yield delay, and reproduces opcode 6 with a zero CQ as described above. No queue algorithm, deadline, retry or recovery change was used.
- Source disk and each ISO hash before/after each completed probe agree. Canonical comparison and source hashes are generated by `archive.py`; no full host/regression/qualification suite was rerun for this investigation.

##### S10-3d.4 Approved product changes (CP1 2026-10-28)

**A. A boot ordering barrier in init, before any block-driver spawn (recommended).**

Files: **`cells/tools/init/src/boot.rs`**, **`cells/tools/init/src/main.rs`**. On x86, sample the existing process table before `start_block_drivers`: while the bootstrap task named exactly `platform` is live, yield and keep waiting. Only after it is absent/terminated may init spawn `/bin/block`, `/bin/nvme`, `/bin/ahci`, VFS and all subsequent configured services. This is a lifetime ordering barrier, not a sleep or retry. The boot already has only trusted Platform/init tasks at this point; process-name matching is used only for scheduling, not to grant authority. Non-x86 flow remains unchanged.

- Use the existing GetProcs/GetTime calls and pure decision helper, not a new syscall, service ID, policy right, kernel flag or spawn argument.
- No live Platform (absent image or already exited) retains the existing kernel-fallback boot path.
- A listing failure, ambiguous/full process table, unavailable clock while Platform remains live, or a live Platform at **60 s** is a **fail-closed boot stop before driver spawning** with a diagnostic; do not silently proceed into overlapping access. A listed terminated task is no longer executing config writes. This does **not** certify a successful scan or invent a success result for a previously reaped task; explicit scan-success attestation would require a separately designed handoff and is not needed to order the normal bootstrap scan.
- Do not use `is_platform_registered()`, a fixed number of yields or a first-device registration as a completion signal. Do not wait for AHCI's exit.
- This addresses all PCI driver starts reached through the single init storage-bootstrap path, not only one NVMe command. No kernel main or loader modification is proposed.

**B. Correct CQ-consumption progress in both NVMe command loops (recommended in the same slice).**

Files: **`cells/drivers/nvme/src/controller.rs`**, **`cells/drivers/nvme/src/queue.rs`** (if a small shared cursor/consume helper is used). On every polling iteration, read the current queue head and phase; on a completed unexpected CID, consume that slot exactly once, advance/toggle phase at wrap and ring the new head, then return through the ordinary poll/deadline/yield accounting before examining the next slot. Matching CID/status semantics remain unchanged. The same original command deadline spans all skipped completions; do not restart it. Keep submission CID allocation and all 1 s/CAP/60 s/30 s budgets unchanged. No abort/reset/retry or resubmission is added.

**C. Exact validation diagnostics (recommended).** In `controller.rs`, log raw CAP once and at admin timeout include actual current CQ head/phase/raw phase-status/CID plus CC/CSTS/AQA/ASQ/ACQ values. Use allocation-free `print_fmt` and only init/timeout logging, not per-poll telemetry. This makes a future recurrence distinguish lost register programming from missing DMA publication without relying on a capability-floor inference. It does not turn invalid configuration into success.

**CP1 record (user, 2026-10-28, relayed by Main): D3d-1 A and D3d-2/3/4 approved exactly as proposed.** Product implementation is authorized only within this scope. The disposable probe delays are **not** copied into product code; incidental pinned-Identify and DSTRD findings remain out of scope.

##### S10-3d.5 Acceptance and verification after approval

1. **WHEN Platform is live, init SHALL spawn no block/PCI driver or VFS.** Host tests inject live→terminated/absent process snapshots and record spawn order; test absent/already-exited, full table/lookup error, no clock and exact 60 s expiry. Failure cases SHALL stop before every driver spawn. Include non-x86 unchanged behavior.
2. **WHEN a late CQE precedes the expected completion, each command loop SHALL consume the late CQE once and inspect the next current slot.** Behavioral host tests over the actual shared cursor/poll logic: late→matching, multiple late CIDs, wrap at depth−1 with phase toggle, matching success/status error, and empty/wrong-phase next slot. Both admin and I/O paths SHALL use the tested logic, not a copied test-only model.
3. **WHILE unexpected completions arrive, the original deadline and 8192-poll yield cadence SHALL remain effective.** Inject a clock that expires during an always-ready stale-CID stream; verify bounded IO error, no deadline reset and no repeated acknowledgement of one slot. Test no-clock fallback.
4. Run host tests once after edits: `driver-nvme`, `app-init`, `service-platform`, `app-cellbox`, `service-vfs`, plus H1–H3/stream behavior tests. Then default P04 terminal and P02 kernel regressions. Preserve the accepted COMPLETION-QUEUE defect separately.
5. Run the controlled overlap diagnostic on a scratch image containing the approved fix: the delayed Platform probe SHALL finish before any driver CAP/register/DMA activity, then intended first Identify CID/PRP/CQ completes. This is a discriminating regression probe, **never qualifying L0 evidence**. Raw traces/physical snapshots and no-overlap event order are required; no fixed sleeps in the product.
6. Rebuild **C0–C6** fleet candidate from the approved source under a new `/tmp` root; retain `0718a965…` as the blocked historical candidate. Preserve canonical hashes before/after and the user's VMX source hash; record exact signing/payload/embedding and source/harness digests. Retain the new candidate read-only.
7. Restart the **entire S10-3 battery**: L0 headless ×6 consecutive and desktop ×6 consecutive (≤12 counted/profile; status 3 uncounted; three consecutive infrastructure stops); imported P02+kernel, P03, P04 terminal, GUI ×6; P09 L5/L5f; all 12 L6 consumers before/after plus CI boot comparison; CBX-01/02 audit; default-mode runs. No S10-3c attempt, diagnostic boot or previous candidate result counts. Any new stop condition stops and is reported, never waived.

##### S10-3d.6 Risks, rollback and CP1 decisions

The init lifetime barrier is a bounded x86 boot behavior change: a hung Platform now prevents drivers/VFS from starting rather than letting them race. The full 256-bus scan may delay disk/network/display startup; measure it in the ordered trace. It proves absence of concurrent scan writes, **not** the health of a missing/reaped scanner. CQ repair makes late completions progress, but does not authorize reuse of timed-out DMA buffers, reconstruct missed programming or replay a save. Test wrap and deadline starvation explicitly. Additional timeout diagnostics must remain cold-path and allocation-free. The known pinned Identify-free and large-DSTRD defects stay separately recorded.

Rollback: revert only the approved init/driver changes and their tests; earlier ordering/late-CID defects return, so qualification remains blocked. No image is promoted and no persistent user data is touched. Canonical artifacts, protected ABI/policy allocations, production signing, VMX, all remaining containment guards and CBX acceptance states remain unchanged.

| ID | Decision | Recommendation |
|---|---|---|
| D3d-1 | **A:** x86 init lifetime barrier before all block drivers, using existing process table/clock; fail closed on uncertain/live-over-budget scanner. **B:** design a new explicit successful-scan handoff first (new ABI/service allocation/owner scope), rather than infer success from lifetime. | **A** for the confirmed concurrent-access defect; do not claim successful scan attestation |
| D3d-2 | Repair both admin/I/O stale-CID cursor/phase progression and enforce original deadline/yield on every path; no retry or budget change | **Approve**, required for the observed post-timeout infinite loop |
| D3d-3 | Cold raw-CAP/admin-state diagnostics and the controlled `/tmp` interleaving regression, then full C0–C6 rebuild/full S10-3 rerun | **Approve**; diagnostic boots never qualify |
| D3d-4 | Keep incidental pinned-grant-free and large-DSTRD defects outside this minimal slice with explicit disclosure; no hidden DMA/ABI expansion | **As stated**; separately scope before claiming those properties fixed |

**CP1 approved 2026-10-28 for D3d-1 A and D3d-2/3/4; CP2 approved later on 2026-10-28**, including the results/deviations below. S10-4/S10-5 retain separate CP1/CP2 gates; rollout remains blocked.

##### S10-3d.7 Implementation and exercised results (2026-10-28; **CP2 approved 2026-10-28**, including SC6 correction and all disclosures)

**Implemented within the approved scope.**
- `init/src/boot.rs` and `main.rs`: the x86 bootstrap lifetime barrier wraps the actual driver/VFS startup flow. It observes exact zero-padded `platform` names; live tasks yield until absent/terminated. Failed/full/ambiguous snapshots, loss/unavailability of clock while live and exact 60 s expiry stop boot before spawning. Non-x86 bypass remains unchanged. It neither treats the first BAR as completion nor claims a successful scan from an absent/reaped task.
- `nvme/src/queue.rs`: both admin and I/O waits use one production `CompletionCursor::wait` flow. Every consumed completion advances the current head, toggles phase at wrap and acknowledges once; a late CID passes through the original deadline/yield accounting before the next current slot. No cached-slot spin, new retry, budget reset or allocation.
- `nvme/src/controller.rs`: both command paths use that flow; raw CAP is logged at init, and admin timeout logs current CQ fields plus CC/CSTS/AQA/ASQ/ACQ. No kernel/main, policy, ABI, buffer-lifetime, DSTRD, signing-route or save-semantics change.

**Host and default regression checks (actual):**

| Check | Result | Evidence under `reports/s10-3d-evidence/` |
|---|---|---|
| NVMe host | **11 passed**: 6 deadline tests plus 5 behavioral CQ progress/wrap/status/empty/always-stale bound tests | `logs/host-tests.log` |
| Init host | **20 passed**, including injected live→absent/terminated startup ordering, fail-closed cases and non-x86 bypass | same |
| Platform / cellbox / VFS host | **9 / 213 / 109 passed** | same |
| H1–H3 and stream host | **27 passed** | `logs/harness-tests.log` |
| Default P04 terminal | **494/494, 202 driver markers**, `/tmp/cellbox-phase04-57dkzacx` | `rerun/lanes/regression-p04-terminal/` |
| Default P02 kernel | **PASS**, VFS **7/7**, `/tmp/cellbox-phase02-nwy__cl0` | `rerun/lanes/regression-p02-kernel/` |

**Controlled interleaving regression: PASS, diagnostic-only.** A scratch image combines the retained Platform binary that widens its existing decode-off probe with the approved fixed init/NVMe. `controlled/verdict.json` records shell boot and no NVMe timeout; raw serial/PCI/MMIO trace and physical DMA snapshots are retained. `controlled/dma-order-proof.json`, derived from that same exercised boot without a rerun, proves scan completion precedes **both CAP and first DMA grant**, intended first SQE `CDW0=0x10006`, CNS=1 and PRP matches the Identify allocation, first CQE CID=1/status-phase=1, and 133 nonzero Identify response bytes. No scheduling delay was added to product code.

**C0–C6: PASS.** New candidate `/tmp/cellbox-s10-3d-resume/cand/candidate.json` SHA-256 **`4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821`**. All candidate files are retained read-only. Kernel features are `default,policy-required`; PC adds `board-x86-pc`. Both exact image/init embed checks are true, candidate failures empty.

| Profile | `kernel_fs.img` SHA-256 | Kernel SHA-256 | ISO SHA-256 |
|---|---|---|---|
| headless | `384c04fa298c4d234bacee149af8787cc917aeb4541dea912365073ae790fcb3` | `7efb030f3ff8f0f5cac965e12112e1dd567585eb59bc935f1c16e416266efb74` | `49b32f3442f40fd52295ccf12f0009e9e46a99cd32615ffeb3a5a7ec598a753a` |
| desktop | `3848cefb25f0e7c4abc67feccf3d8886292e25da60a88b4633552651292479ec` | `dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a` | `d207031983d53460fed88a8578975b13517dd4c412e4544c0b833ce06afe7334` |
| PC (headless image) | same headless image | `3253bc701ecfbfd7a990a643f8aeea7f2da448b54ddcfb4736cb782e964e9da6` | `3cdf536ae9430194f157c4c73a3cddd249ae319e2dbf44a91f146d5e29dd8a8a` |

**Canonical proof:** generated `canonical/comparison.json` reports **469 files unchanged**, covering the full embedded directory, direct canonical release files and build ISOs/qcow2/kernel ELFs. User `kernel/src/main.rs` remains SHA-256 **`a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64`**. Historical candidate `0718a965…` remains untouched.

**CP2 deviations/disclosures:**
1. **SC6, final document audit harness input error.** The first audit reused an older S10-3 script pinned to pre-S10-3a spec `cde24178…`; it failed only `spec.md`, current `f9c550f7…`. This was reported immediately, not waived. `contract-deltas.md:170–172` already records that exact spec revision under approved S10-3a/supersession (e), and the S10-3b audit script already pins it. Main explicitly directed use of that **exact prior S10-3b script**. Only the document audit reran, reusing the same cargo-deny logs; **PASS**, all five document pins equal and no missing CP rows. `audit-stale-harness.json`, stale script/log and corrected script/log are preserved alongside the generated final audit. No spec/criteria/pin approval was invented; no runtime rerun or candidate rebuild occurred.
2. The sequential wrapper exited at SC6 as required. Its exit 1 remains evidence of that reported stop; the separately authorized corrected audit completes the battery evidence. No fabricated “whole wrapper exit 0” claim.
3. The controlled proof's original verdict covers boot/order/no-timeout; detailed DMA/order fields were generated afterwards from its existing snapshots, not from a second probe. Both original and derived outputs remain.
4. Host tests retain unused-code warnings and the known mocked-QEMU-stderr ResourceWarning. Scoped cargo-deny passes with warnings; workspace cargo-deny is separately disclosed, not used to substitute for the scoped gate.
5. `[selftest] COMPLETION-QUEUE: FAIL` remains the accepted pre-existing nonblocking defect. L6 retains one before/after xHCI key-decode failure, four handoff x86-32 skips and one virtio ignored test; none was suppressed. D60-3, GUI Delete, incidental pinned Identify-free leak, large-DSTRD bounds gap, guest SSH and publickey/KMS gaps remain.
6. At the original result handoff, all results were unapproved implementation evidence and no acceptance row flipped. **User CP2 on 2026-10-28 now accepts their exact candidate-only ceilings**; no Phase 04 criterion 4, containment guard or canonical byte is promoted. S10-4/S10-5 have not been attempted.

#### S10-3.14 Full S10-3 battery after S10-3d (2026-10-28): **CP2 approved 2026-10-28**, candidate-only qualification

Evidence: **`reports/s10-3d-evidence/`**, S10-3 rerun in **`rerun/`**; generated execution summary `logs/execution.json`, append-only lane records, raw verdicts/payload matches, L0 classifier, L6 before/after results and both audit verdicts. Earlier candidates/diagnostic boots do not count.

| Lane | Actual result |
|---|---|
| **L0 headless** | **6/6 consecutive**, attempts 1–6 each **16/16 markers**, TCG |
| **L0 desktop** | **6/6 consecutive**, attempts 1–6 each **9/9 runner markers**, TCG; Delete-key behavior remains a known-defect observation, not a fix claim |
| L0 infrastructure/timing | **0** status-3 attempts, no retries/reset, no NVMe timeout or slow-completion events recorded; all provenance/no-secret/ISO-unchanged checks pass |
| L1 imported P02 session | **48 passes + `ctrl_j_submits: true`**, 12 payload-equal imported cells |
| L1 imported P02 kernel | **39/39 kernel cases**, VFS **7/7**, 11 payload-equal imported cells (`test-hooks`) |
| L2 imported P03 | **67 checks PASS**, 14 payload-equal imported cells (`phase03-filter-profile`) |
| L3 imported P04 terminal | **494/494, 202 driver markers** |
| **L4 imported GUI** | **6/6 consecutive at 290 checks**, five click then one autoload; every run **10/10** strict stderr iterations; 14 payload-equal imported cells |
| L5 imported P09 no-faults | **34/34**, 18 driver markers |
| L5f lane-only P09 faults | **103/103**, 36 driver markers; remains lane-only fault/rekey/no-replay evidence, not candidate-byte evidence |
| Default-mode runners | P02 **48 + `ctrl_j_submits`**; kernel regression PASS; P03 **67**; P04 terminal **494**; GUI **290**; P09 no-faults **34** |
| **L6 12 ISO consumers + CI boot, before/after** | **0 regressions**: 39 pass→pass, 1 pre-existing fail→fail (xHCI injected-key decode), 4 skip→skip (x86-32 handoff), 1 ignored→ignored (virtio block init). CI boot passes both. NVMe **3/3**, AHCI **5/5**, bus-1 NVMe **2/2**, remapper contract **3/3** pass on candidate |
| CBX-01/02 audit | **PASS after explicitly authorized stale-harness correction**; initial FAIL retained; all five current document identities match approved pins; CP row omissions empty; scoped cargo-deny advisories/bans/licenses/sources pass |

**Per-CBX disposition approved at S10-3 CP2 (2026-10-28):** [acceptance.md](acceptance.md) records these exact candidate/private ceilings; SSH partial rows and S10-4/S10-5 pending rows are not expanded.

| CBX | Evidence outcome / retained ceiling |
|---|---|
| 01, 02 | Corrected approved-pin/delta/scoped dependency audit PASS; stale-harness failure/correction disclosed |
| 03, 04, 05 | Matched L1/L3, desktop two-session and headless Ctrl-C checks PASS |
| 06, 07, 08, 09 | Matched L2/L3 and L0 typed/bytes/rg/state/script checks PASS |
| 10, 11 | Candidate image/form/denial checks and matched one-shot/denial evidence PASS; fixture-only kernel launch edges remain excluded |
| 12 | Headless no-UI service plan and matched GUI parity PASS |
| 13, 15 | Matched GUI/terminal plus canonical top/editor/VT checks PASS |
| **14** | Both L0 desktop and L4 meet **6/6** required evidence; **met on qualified candidate at user CP2 2026-10-28**, not a canonical promotion |
| 16 | Existing archived P05 consumer audit retained; not rerun by design |
| 17, 18 | Matched top-load and canonical top checks PASS; D60-3 clock-unit issue remains disclosed |
| 19, 20 | Matched and canonical rg checks PASS |
| 21, 22 | Matched and canonical editor saves/read-back PASS; still **“synced, not crash-proof”** |
| **23, 24, 25** | Remain **partial**: other-host SSH/fail-closed/no-replay/no-inbound evidence PASS; Tier-3 guest/publickey/KMS/VM route gaps unchanged |
| **26** | Required L0/L4/L6 candidate evidence completed with zero new regressions/disclosed known defects; **met on qualified candidate at user CP2 2026-10-28**; canonical packaging/rollout remain gated |
| 27, 28 | Not evaluated here; S10-4 footprint and S10-5 promotion remain blocked |
| 29 | Matched L1/L3 and canonical selective-TUI/one-shot refusal checks PASS |

**CP2 record (user, 2026-10-28, relayed by Main): APPROVED S10-3a, S10-3b, S10-3c, S10-3d and S10-3**, including all recorded slice-specific deviations and S10-3d.7 disclosures 1–6. This accepts the qualified retained candidate and exercised CBX ceilings, not a canonical packaging/promotion, containment lift, crash-durability claim, physical/VMX/production qualification or permission to run S10-4/S10-5 before their own gates.

### S10-4 — footprint measurement against the 980,528 B baseline (**CP1 approved by user 2026-10-28**, original method below; execution/evidence CP2 pending)

**Method (matched):**

- Same toolchain (`nightly-2026-05-01`, unchanged).
- Same target and flags: `x86_64-unknown-none`, release, `-C relocation-model=pic`, `-Z build-std=core,alloc`.
- Same key class: dev key, one `__ViCell_sig` section.
- Same measuring tool: FAT file size from `inspect_fat.py`, plus `size`-style text/data/bss.

**Baselines:**

- **B1 (recorded):** the six baseline ELFs, still byte-identical inside `447d0348` (§1.3). Their signing provenance is unverified.
- **B2 (rebuilt):** `git worktree` at HEAD `3471028bc`, which still has `cells/tools/shell`. Build `shell` and `ls cat echo ps kill` with the identical flags and sign with the dev key in that worktree's own target directory.

**Report** (regressions shown as regressions):

- executable count and bytes for:
  - the before set: `shell` plus five workers;
  - the after set: `cellbox` plus five workers, with the `/bin/cellbox-ssh` copy shown separately;
- suite-only bytes (shell 881,120 against cellbox, currently about 3.08 MB unsigned);
- total VIFS1 and ISO bytes;
- cold boot-to-prompt time;
- heap and stack (bss about 7.3 MB, including the 6 MiB `rg` arena, D76-3);
- idle redraw;
- long-stream bounds from L0 and L3.

File size is reported separately from resident memory. Feature non-equivalence (rg, edit, ssh, top, typed pipelines, Ratatui) is stated explicitly; no "smaller" claim is made.

**Files:** none in the repo. The worktree is created under `/tmp` and removed. The report is written to `reports/s10-4-evidence/`.

**Risks:** HEAD's `shell` may not reproduce the B1 bytes. Both baselines are reported and the difference is explained, not hidden.

**Rollback:** none needed.

**Acceptance:** CBX-27, and Phase 10 criterion 4.

#### S10-4.1 Initial execution stop (2026-10-28; no result CP2 requested)

The original short S10-4 method above was approved by the user (relayed by Main); the later proposed detailed protocol had not been written into this document when approval arrived. No extra observation mechanism, new feature-equivalence criterion or relaxed gate is inferred.

- Created detached worktree `/tmp/cellbox-s10-4/baseline-worktree` at exact **`3471028bc8b4db14275553b4740c17e24fa4f32b`**, with own `/tmp/cellbox-s10-4/baseline-target`. The checkout used a plain-copy fallback because WSL did not support reflink; source/HEAD were not changed.
- Generated after/comparison proof: **all 546 paths unchanged**, including **469 canonical files**, user's VMX source and retained `4fae8b20…` candidate. Archived summary/status derives from the actual failed command and this comparison; no verdict was hand-edited.
- Generated before-hash record covers **546 paths**, including all **469 canonical files**, user's kernel main and every retained qualified-candidate file. No candidate rebuild or signing/artifact write.
- Toolchain metadata succeeds for `rustc +nightly-2026-05-01 -Vv` and `cargo +nightly-2026-05-01 -V`. Historical/current release profiles both declare `panic="abort"`, `lto=true`, `opt-level=2`; PIC/target/build-std/C environment match the qualified build. Historical packages are `app-shell` (default binary `app-shell`) and `app-sys-tools` (`ls cat echo ps kill` selected).
- **New stop: B2 build exits 101 before compilation.** That exact historical workspace lists `cells/apps/vm-runner`, but its `Cargo.toml` is absent. Read-only workspace census finds **only this missing member**, and `git ls-tree -r` for it at the approved HEAD is empty: this is a missing historical source prerequisite, not a checkout/submodule fetch omission.
- Command/log evidence: generated `build-commands.json`, `stop.json`, `logs/b2-{rustc,cargo,build}.log` under `/tmp/cellbox-s10-4/`, archived in `reports/s10-4-evidence/`. The original failure is not overwritten. **No second build attempt, manifest exclusion, stub, different revision, unchecked signature or guard removal.**
- **Not run after the stop:** B2 ELF production/signing/section measurements, fresh static report, cold boot-to-prompt, idle redraw or any new runtime lane. Existing L0/L3 behavior is not relabeled as startup/heap/stack performance. B1's 980,528 B and candidate metadata remain recorded provenance, not a newly executed S10-4 result.
- Read-only runtime scoping additionally finds no candidate-compatible retained observer for resident/peak heap or stack high-water. ELF `.bss`, 6 MiB arena reservation, host counting-allocator bounds and no-OOM markers cannot substitute for those measurements. B1/B2 have no historical matched runtime baseline. Any report must label these absent rather than claim memory savings or full equivalent-feature comparison.

**Disposition requested from Main/user:** authorize a precise minimal workspace repair (exclude the absent unrelated `vm-runner` member only in the disposable B2 manifest, explicitly label the comparator non-pristine and preserve the diff), or keep B2 blocked pending the missing historical source. Independently choose whether runtime memory reporting is reservation-only with unmeasured peaks (an explicit CBX-27/criterion-4 ceiling) or requires a separately approved candidate-preserving observer/protocol. No choice is presumed; **CBX-27 and Phase10 criterion4 remain unmet, S10-5/containment unchanged**. The worktree is retained only for this disposition and must be removed after the authorized B2 stage finishes; the qualified candidate remains read-only.

#### S10-4.2 Authorized B2 manifest exclusion, second stop and static results (2026-10-28; incomplete/unapproved results)

**User decision 1 (2026-10-28, relayed by Main): authorize only exclusion of the nonexistent `cells/apps/vm-runner` member from the disposable worktree's root `Cargo.toml`.** Applied exactly one deleted member line; `b2-manifest.patch` and original/repaired manifests are archived. B2 is explicitly a **non-pristine historical comparator**, not pristine HEAD or B1 reproduction. A conservative recursive local-path dependency census (including optional/dev/build tables) covers **8 manifests** for `app-shell` and `app-sys-tools` and finds **no vm-runner compile dependency**. Thus exclusion cannot remove code from the measured shell+five-worker build graph. The shell does contain a runtime `spawn_external("vm-runner")` fallback for user apps; no claim that all historical shell behaviors are independent of vm-runner.

**New stop after the authorized exclusion:** the matched build with `--locked` exits **101** because historical `Cargo.lock` needs updating. No Cargo.lock edit/unlocked/offline retry, new revision, stub or signing bypass attempted. Original missing-member failure and repaired lock failure are separate retained logs/command records. **New disposition required:** permit offline lock reconciliation in the disposable worktree with full lock diff/resolved-version audit, or retain B2 blocked. The user has not yet decided runtime peak observer scope; Main directs reservation-only reporting with explicit unmet ceiling in the meantime.

**Executed static measurement only:** `measure-static.py` reads B1's six original ELFs byte-identically from canonical `447d0348` and exact qualified `4fae8b20…` profile FAT images. It uses the existing image reader/`inspect_fat.py`, GNU `size -B -d`, complete ELF sections and dev-signature verification. All six B1 and all selected after copies verify under the dev key now; this does not reconstruct historical build/signing provenance. Candidate files remain untouched; derived files are disposable `/tmp` copies. Generated source/report/logs are archived in `reports/s10-4-evidence/static/`, with `measurement.json` authoritative.

| ELF | B1 signed file B | B1 section-removed derivative B | Headless signed file B | Headless section-removed derivative B | Desktop original unsigned B | Desktop signed file B |
|---|---:|---:|---:|---:|---:|---:|
| shell → cellbox | 881,120 | 880,976 | 3,093,952 | 3,093,808 | 3,093,912 | 3,094,016 |
| ls | 20,952 | 20,808 | 30,288 | 30,152 | 33,576 | 30,288 |
| cat | 20,976 | 20,840 | 30,328 | 30,192 | 33,576 | 30,328 |
| echo | 20,952 | 20,816 | 30,296 | 30,152 | 33,584 | 30,296 |
| ps | 15,432 | 15,288 | 30,192 | 30,056 | 33,448 | 30,192 |
| kill | 21,096 | 20,952 | 30,360 | 30,216 | 33,584 | 30,360 |
| cellbox-ssh second physical copy | absent | absent | 3,093,952 | 3,093,808 | 3,093,912 | 3,094,016 |

**Unsigned provenance:** B1/headless original pre-sign ELFs are unavailable. Their section-removed derivatives are **not original unsigned ELF measurements**, nor is signed-file-minus64 (canonical signed payload) an unsigned ELF size. Retained target outputs match the desktop stable signing payload for all six binaries; only those original unsigned sizes are reported. They do **not** match the headless payload, so are not silently reused there. Desktop section-removed sizes remain separately in JSON. GNU objcopy can change ELF layout/file size beyond the64-byte signature, explaining why some signed workers are smaller than their original unsigned artifact; this is not a code/memory saving.

| Static accounting | B1 recorded six | Qualified headless | Qualified desktop |
|---|---:|---:|---:|
| suite-only signed file B | 881,120 | 3,093,952 | 3,094,016 |
| five independent workers signed B | 99,408 | 151,464 | 151,464 |
| six selected ELFs signed B | **980,528** | **3,245,416 (+2,264,888)** | **3,245,480 (+2,264,952)** |
| physical selected payload with SSH alias B | 980,528 (no alias) | **6,339,368 (+5,358,840)** | **6,339,496 (+5,358,968)** |
| complete FAT file payload B | historical inventory12,919,090; current container13,003,810 | 8,032,782 | 18,604,628 |
| packaged file/ELF counts | historical40/29; six measured here | 25/21 | 32/28 |
| FAT container B | 16,777,216 | 12,582,912 | 25,165,824 |
| qualified kernel ELF B | no matched B1/B2 comparator | 15,599,680 | 28,182,592 |
| qualified ISO B | no matched B1/B2 comparator | 19,472,384 | 32,055,296 |

PC reuses headless FAT/init, kernel **15,599,672 B**, ISO **19,472,384 B**, with additional `board-x86-pc`; full hashes/features are in generated JSON. The original inventory image was `699e1df…`; the six measured B1 ELFs remain identical inside later `447d0348`, but its other files/payload are a different container. Historical ISO inventory is not a matched source/profile baseline; no whole-image before/after optimization claim. The SSH alias counts **twice** as two packaged files even though byte-identical; no assumption of shared FAT allocation or runtime sharing.

**Memory/behavior ceiling:** current Cellbox GNU-size text/data/bss **2,616,808 /161,560 /7,340,360 B** (both profiles). B1 shell **744,578 /17,960 /1,048,658 B**. These are section/address reservations, not resident memory or allocator/stack peaks. Cellbox arena6MiB plus ostd arena1MiB are structural reservations only. `resident_bytes`, `peak_heap_bytes`, `peak_stack_bytes` remain null/unmeasured. Cold boot-to-prompt, fresh idle redraw and new runtime lanes have **not** run after this build stop; retained L0/L3 behavioral bounds are not mislabeled as throughput/startup/memory measurements.

**Retained long-stream/redraw evidence (not rerun):** generated `behavior.json` preserves source hashes and exact L3 verdict checks: live endless-sink delivery, Ctrl-C130/shell recovery/no-OOM, consumer close, rg fixture exactness and frontend reattach/redraw all pass. Recorded fixture Ctrl-C→prompt **9ms**, consumer-close **56ms**; raw rg elapsed field **301519** for **7,340,032B** is retained without converting it into universal throughput. L3 is a private imported fixture with matched cell payload, not canonical-kernel resident-memory proof. Existing L0 six-consecutive per-profile behavior is retained by source hash; whole-fixture `seconds` are **not boot-to-prompt**. Idle redraw correctness is not an idle-cost/rate measurement. Final static-stage generated comparison again proves **546/546 paths unchanged, including469 canonical files**, user's VMX main and the entire qualified candidate; both original and post-static comparisons are archived.

**Regressions and feature non-equivalence:** selected executable file footprint increases by the exact positive deltas above. Cellbox adds rg/edit/outbound-SSH/top record+TUI/typed pipelines/Ratatui and changed authority/save/deadline/bootstrap behavior; neither suite nor full-system profiles are feature-equivalent. **No “smaller”, universal optimization, peak-memory or physical-performance claim.** B2 results remain unavailable, not zero. **CBX-27/Phase10 criterion4 remain unmet; no S10-4 result CP2, canonical promotion or containment lift.** Worktree remains only for explicit lock-disposition continuation and removal after the authorized B2 stage.



#### S10-4.3 Authorized offline lock fallback; B2 unsigned build complete, checked signing stopped (2026-10-28)

**Main's explicit disposition:** the authorized missing-member exclusion extends to a minimal disposable lock change; prefer removing a vm-runner package entry while keeping all other pins, otherwise allow offline reconciliation with full old→new package/graph audit. The original lock has **no vm-runner package entry**, so the preferred removal is impossible; no guessed entry was removed. Executed fallback exactly `cargo +nightly-2026-05-01 metadata --offline --format-version 1 --filter-platform x86_64-unknown-none` with `CARGO_NET_OFFLINE=true`, own target directory. No network fetch.

**Lock delta:** adds only local workspace **`app-chrome0.1.0`, `files0.1.0`, `terminal0.1.0`**, each source absent/local, **none in the measured shell/five-worker resolved dependency graph**. Removes no package and changes no existing package metadata/version/source; all **407 original package blocks remain byte-identical**. The lock's stale local membership was additional historical inconsistency, not a registry update or evidence that vm-runner was a compile dependency. Full package records, resolved graph, original/reconciled locks and exact patch are generated/archived in `lock-audit.json`, `b2-lock.patch`, `b2-measurement.json` and source snapshots. **B2 remains non-pristine; this offline fallback is an explicit deviation**, not hidden baseline equivalence.

**Build PASS:** selected `app-shell`, `ls cat echo ps kill` with identical nightly/release/PIC/target/build-std/C environment, own target dir, now **`--locked --offline`**. One completed build exits0, release log34.53s. No tests or product edits were added.

**New stop at checked dev signing:** historical `python3 scripts/cellos-sign --sign …` runs its actual workspace gate: **F5 PASS** (exact pinned compiler), **F1 FAIL** because unrelated `cells/services/ocel-quickjs/src/native_layout.rs` contains an unallowlisted unsafe token. It **REFUSES signing, exit1**. Overdue allowlist review notes are retained; they are not the failure being suppressed. No allowlist/source edit, `_CHECKED` manipulation, unchecked signature, alternative guard or retry. `/tmp/cellbox-s10-4/b2-signed` staging copies remain **unsigned**, not signed artifacts. All B2 signed fields are **null/unavailable**; no hash is presented as signed qualification.

| B2 ELF | Original unsigned file B | GNU text B | GNU data B | GNU bss B | Signed result |
|---|---:|---:|---:|---:|---|
| shell | 880,992 | 744,578 | 17,960 | 1,048,658 | unavailable/F1 refusal |
| ls | 24,424 | 7,227 | 496 | 1,048,625 | unavailable/F1 refusal |
| cat | 24,424 | 6,727 | 520 | 1,048,625 | unavailable/F1 refusal |
| echo | 24,424 | 6,307 | 496 | 1,048,625 | unavailable/F1 refusal |
| ps | 15,304 | 2,699 | 320 | 0 | unavailable/F1 refusal |
| kill | 24,496 | 7,826 | 568 | 1,048,625 | unavailable/F1 refusal |
| **six total** | **994,064** | — | — | — | **not0; absent** |

B2 suite-only unsigned **880,992B**, five workers **113,072B**. B1 **980,528B is signed packaged bytes**, so subtracting it from B2 unsigned is **not** a matched signing-state delta or reproduction claim. B1 original pre-sign files are unavailable, and rebuilt worker text differs slightly; no byte-reproducibility assertion. Qualified desktop matching original-unsigned six total **3,261,680B**, **+2,267,616B vs B2 unsigned**; this is a file-footprint regression across explicitly different features. Headless original-unsigned gap remains, and alias bytes remain charged twice in packaged totals. No runtime-memory/feature-equivalence inference.

**Evidence/state:** generated `reports/s10-4-evidence/summary.json` now says `STOP_B2_CHECKED_SIGNING_F1`; initial/lock-stop summaries and all three attempt logs remain. New after-B2 comparison again proves **546/546 paths unchanged, including469 canonical**, user's VMX main and the entire read-only qualified candidate. No QEMU/runtime/peak/startup/idle-cost run; retained L0/L3 bounds remain at their old ceiling.

**Disposition required:** retain the real unsigned B2 comparator and incomplete signed measurement, or explicitly authorize a properly scoped audited historical signing route. No new allowlist approval or guard bypass is inferred from build approval. Runtime observer decision2 is still pending, so report reservations with null resident/peak/stack fields. **CBX27/criterion4 unmet; no S10-4 result CP2/promotion/containment change.** Worktree retained for this signing disposition only; remove after authorized B2 stage finishes, retaining results/patches outside it.

#### S10-4.4 Consolidated runtime-observer CP1 and historical scoped-signing disposition (**CP1 APPROVED by user2026-10-28: D4-Obs1/Obs2/Obs3 A and D4-Sign1 A**; implementation/results require CP2)

**User decision (2026-10-28, relayed by Main): first design runtime protocol, then explicitly APPROVE the proposed protocol (D4-Obs1/2/3 A) and mirror signing scope (D4-Sign1 A).** The later approval authorizes execution exactly below: host-only observer, unchanged qualified candidate, explicit incomplete metrics/baseline ceiling; source-exact mirror with original allowlist/strict F1/F5. No rebuild, guard bypass, unrelated policy fix, baseline-runtime carrier or promotion is authorized. Earlier “design only” evidence/history remains intact.

##### A. Read-only feasibility evidence, collected now

Generated evidence is `reports/s10-4-evidence/protocol/{proposal-evidence,inventory,heap-instructions,signing-scope-census,retained-startup}.json`, full `nm`/ELF-header listings and source hashes. Only static inspection/census ran; **zero new runtime runs, zero new scoped F1/F5 invocations, zero signatures**.

| Unchanged ELF | `ostd::heap::HEAP` relative VA / size | Arena symbols | Symbols/debug metadata |
|---|---|---|---|
| B1 signed shell / B2 original-unsigned shell | `0xbc018` /48B | default `HEAP_MEM` at `0xbc050`,1MiB | `.symtab` present, no `.debug*`/DWARF |
| Candidate headless Cellbox | `0x8a80c0` /48B | custom `0x2a8060`,6MiB; default `0x8a80f0`,1MiB | `.symtab` present, no DWARF |
| Candidate desktop Cellbox | same heap/arena relative VAs | same reservations | `.symtab` present, no DWARF; code PCs differ from headless |
| B1/B2 `ps` | no retained allocator symbols | none observed | `.symtab` present, no DWARF; **absence is not runtime zero-allocation proof** |
| Candidate kernels | `SCHEDULER`, quota `IN_USE` and allocator symbols present | kernel state, not a Cellbox peak ledger | no DWARF; default profile does **not** include `test-hooks` or stack-watermark symbols |

Pinned `linked_list_allocator0.10.6` source says `Heap.used` increases/decreases by **aligned allocation layout size** on successful allocation/free. Actual machine code corroborates that word at **HEAP+0**, not an assumed Rust-layout offset. The full static census finds400 exact counter references in each B1/B2 shell and1026 in each candidate Cellbox: all are zero-initialization stores except **one add and one subtract**. There is **no persistent peak counter**. Post-update hardware-execute observation PCs (relative VA) are:

| ELF | after successful add | after subtract/free |
|---|---|---|
| B1/B2 shell | `0x3dd31` | `0x3db86` |
| headless Cellbox | `0xe40b1` | `0xe3f06` |
| desktop Cellbox | `0xe30c1` | `0xe2f16` |

These are proposed observation sites, **not exercised debugger coverage**. ELF hashes, instruction bytes, every exact counter reference and successor PC are archived; changed hash/layout, a nonzero write outside that census or an ambiguous relocation stops execution.

Other actual metric boundaries:
- `ProcessInfoV2.heap_bytes` is kernel **Cell quota charge**, not the static Cellbox allocator's live usage. Current telemetry separately exposes quota charge/limit, task user-stack usable bytes and allocated segment bytes (`task/syscall.rs:3425–3442`, `task/telemetry.rs:269–308`). Quota may already include stack charge (`STACK_IN_QUOTA`); **never add it again** or label it heap/RSS.
- User/kernel `Stack` reservations are distinct. Current Cellbox policy selects16 usable pages =64KiB per stack, plus2 unmapped guard pages per stack. The default for other names is64 usable pages; a hypothetical legacy shell under today's policy is not automatically a matched64KiB comparator. Existing watermark code requires **`test-hooks`**, absent from the qualified candidate; enabling it or painting a sentinel would change the experiment and is excluded.
- ELF load mappings include `.bss`; the loader allocates page backing eagerly. ELF section sizes still are not runtime residency. A runtime page-table census can establish **mapped guest physical backing at that instant**, not host RSS, working-set residency, private/proportional set or peak allocator use.
- QEMU is **8.2.2**. Its [version-pinned debugger manual](https://github.com/qemu/qemu/blob/v8.2.2/docs/system/gdb.rst) documents unlimited hardware-assisted breakpoints/watchpoints for **TCG system emulation**, Unix sockets and relocation caveats. [Pinned gdbstub source](https://github.com/qemu/qemu/blob/v8.2.2/gdbstub/system.c) reports the **configured watchpoint VA**, not a byte-precise effective write address. Therefore a whole-stack watchpoint cannot magically reveal exact byte-depth. GDB is **not installed** here; propose a small host Python-stdlib RSP client after approval, not an unapproved package install. Version-doc lookup's Node helper was unavailable; the cited source was fetched directly at the exact installed QEMU tag.
- Existing headless L0 boot markers already retain coarse first-substring timings **3.1/2.8/2.5/2.5/2.5/2.5s**, rounded0.1s from `time.time()`. Preserve them as historical observations, **not** monotonic/exact-idle measurements and **not** the672s whole-fixture durations. Desktop has no equivalent retained timestamped pixel-readiness series.

##### B. Proposed candidate-preserving runtime protocol

**Invariant:** exact candidate JSON `4fae8b20…2821`, all profile files/manifest/kernel/ISO hashes and embeddings remain unchanged. No assembly, signing, relinking, test-hooks, kernel/cell/source patch, ELF replacement, load-path alias, policy/config override or guest memory/register write. Use fresh private `/tmp/cellbox-s10-4-runtime-*` work dirs and disposable clones of the already-qualified main data/config fixture disks; ISO attached read-only. No SSH fixture, secrets, forwards, external endpoint or host network exposure. One owned QEMU at a time: q35/TCG/single vCPU/256MiB/same CPU flags/restricted SLIRP as L0. This is fresh-VM QEMU evidence, **not cold host cache, physical Intel or VMX**.

**Two separate experiment modes; never blend their timings:**

1. **Untraced timing/idle mode**,3 fresh headless boots and3 fresh desktop boots. No gdbstub/debug pause/heap breakpoints. Record `monotonic_ns` immediately before `Popen`, every serial receive/predicate observation and corresponding screenshots; retain polling intervals, raw timestamps, argv, QEMU PID and before/after hashes.
   - Headless end: first exact empty prompt accepted by existing `Shell.idle(raw,count=1)`, not arbitrary `Cellos >` substring; prompts start at0. Report each boot and min/median/max, not an unsupported performance pass threshold.
   - Desktop timestamps separately: first exact `[desktop] Ready. Entering event loop.`; taskbar Terminal click; first exact pixel-oracle banner/prompt/cursor accepted by `Desk.launch`. Preserve the existing3s pre-click wait and report it explicitly. Publish **boot→desktop-ready**, **click→Terminal-prompt**, and **boot→prompt including automation delay** as different metrics, never a hidden subtraction. Reuse the retained qualified glyph oracle at `/tmp/cellbox-phase10-a3hg7abb/host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs` with recorded provenance; if unavailable/mismatched, stop rather than silently rebuild/change the oracle.
   - After a stable ordinary prompt,10s idle, no user input. Sample the GUI Terminal content at10Hz with the existing oracle, exclude only the recorded cursor cell/blink, retain all frames/diffs and serial control/output bytes. Record whole-owned-QEMU `/proc` CPU-time delta separately, including sampling overhead. **Content-change frames are not redraw calls; whole-VM CPU is not shell-only CPU or memory.** Also record a10s interactive `top` interval using the existing real TUI path, with fixed command/cursor inputs and exit status.
   - Long-stream timing on headless uses the existing canonical finite control `top --records -d 1 --frames 2 | to jsonl | cat`, then existing endless `top --records --frames 100000 | to jsonl | cat`, with10s streaming after first live output, Ctrl-C, required **new** prompt/status130 and4s output-quiescence observation. Reuse `rg -F p10-literal-that-occurs-nowhere-0123456789 /mnt/sd/rg/big.log` and the pinned7,340,032B fixture for finite search/cancel bounds. Keep existing L0 predicates/time limits; do not invent throughput or universal latency from one fixture. Retain L3's imported-fixture9ms/56ms evidence at its original ceiling, not as new canonical timings.

2. **Observed memory/event mode**,one fresh boot per profile with the same data/input sequence, **not a startup/performance run**. Add only a private gdbstub chardev Unix socket under a mode0700 directory (no TCP listener) and `-S`; implement the host RSP observer only after CP1.
   - Read-only command policy: capability/register/memory reads, continue and **hardware** execute/write watchpoints (`Z1`/`Z2` with matching removes). **Reject software breakpoints (`Z0`), memory/register write packets, injected function calls, sentinel writes, `set $pc`, guest agent/plugin or replacement code.** No GDB software-breakpoint fallback. All requests/replies and pause intervals are recorded.
   - Find the real module before its first instruction, not by guessing a load base. Default profile reserves **512 ×32MiB PIE slots beginning4GiB**; ELF entry0 and source-pinned allocator rules permit hardware entry breakpoints across the finite slot set. At a hit, verify three unique non-relocated executable-byte fingerprints against the exact candidate ELF, current translation/CR3 and entry. Bind an observer-owned instance ID `(CR3,entryVA,entry ordinal)`, not a fabricated CellId/generation. If needed, correlate existing `ps --records` once in a separately labeled observer phase; its allocations are not hidden in workload metrics. Restart, slot reuse, unexpected concurrent instance or fingerprint ambiguity stops that instance's measurement.
   - **Live/peak aligned allocator bytes:** install execute breakpoints at both relocated post-add/post-sub sites above, before user execution continues, plus one8-byte HEAP+0 **write-watch coverage guard**. The guard accepts only the archived zero/add/sub writer PCs or their verified successors; an unknown writer, unsupported stop-before/after semantics or unexpected reinitialization stops the experiment. Peak values come from the **post-update execute stops**, never an ambiguous pre-store watch value. Read HEAP+0 at every completed event/phase boundary, store ordered `{instance,phase,pc,counter,host_time}` and derive maximum only from the complete stream. This measures **peak simultaneously live aligned allocation bytes in the Cellbox allocator**, not requested payload, whole Cell memory, allocation churn, resident pages or stack. Initial zero stores cannot raise a peak; capture initial value/HEAP_INIT and active arena bounds. Missing coverage/transport loss/no hardware support never degrades to “peak” from samples.
   - **Stack write-depth at4KiB resolution:** at the verified first user entry read initial RSP/top; confirm actual usable-stack extent/mappings and64KiB telemetry/source policy before applying it. Add one CPU-write watchpoint per usable4KiB page, not guard pages. On a page's first hit, record configured page VA/physical mapping/instance and remove that page's watchpoint. This yields a bounded **CPU write-attempt page footprint** with at most16 first-hit stops, including zero stores, without assuming a watched store has retired or RAM stays nonzero. Record normal workload completion separately. Report page count and deepest-address depth bound (4096B resolution); include faulted/incomplete intervals only as incomplete witnesses. **Not exact byte/RSP high-water, retired-store proof, kernel-stack peak, DMA-write coverage or persistent watermark.** No “true stack peak” claim. Native-domain translation ambiguity or reuse stops measurement.
   - **Mapped backing snapshot:** at first prompt and workload boundaries, read current CR3/page tables through debugger reads, walk actual present mappings for the selected ELF PT_LOAD union and user-stack range, and deduplicate physical4KiB frames. Record raw PTE witnesses, permissions/large-page handling and included/excluded ownership. Label `mapped_segment_plus_user_stack_backing_bytes`; exclude kernel stack, kernel metadata/quota allocations, page tables and unrelated/shared cells, or report them separately only with ownership proof. **Not RSS/resident/working set/dirty pages/peak heap.** QEMU dirty-page counts are expressly not substitutes.
   - Heap phases: initial prompt; ordinary idle10s; finite two-frame control; endless live stream10s then Ctrl-C/quiescence; pinned7MiB rg workload; interactive top10s. In desktop use the real focused GUI Terminal, not an invented UART route. Record raw input/output/status/new-prompt boundaries. Optional hardware entry counts for existing Ratatui `Terminal::try_draw`/`flush` (headless `0xa0c00`/`0xa1040`, desktop `0x9fea0`/`0xa02e0`) are **attempt/call counts in the observed interval**, not framebuffer changes, bytes emitted or unperturbed redraw cost. Report observer pause overhead; do not mix these counts' host duration into untraced timing.

**Safety/verification gates after CP1:** host-only RSP/ELF/page-walk parser tests and a packet allow/deny audit first; no Cell/kernel build. Verify all sites/fingerprints against archived binaries, actual QEMU TCG capability responses and private socket permissions before guest measurement. Every phase must complete within the existing900s host boot window and its original command limits; no enlarged NVMe budget, retry or suppression of new failures. Lost coverage, unsupported packet, source/artifact/hash drift, timeout, changed memory/PC packet, observer perturbation causing a new guest failure, ambiguous identity or missing oracle is a **new stop reported to the user**. Preserve partial raw evidence; never synthesize zero/success.

**Evidence/rollback:** host-only helper/command snapshots, protocol unit results, packet logs, source/ELF fingerprints, raw heap events, page-first-hit records/PTE witnesses, screenshots/serial, monotonic timestamps, input-disk/ISO identities and generated summaries go under `/tmp` then `reports/s10-4-evidence/runtime/`. Summaries generated only by harness. Stop only owned QEMU, detach/remove host watchpoints/sockets, discard disposable data clones and remove the historical worktree after the approved B2 stage; retain read-only candidate/results. No canonical/artifact rollback, guard change, secret or permanent runtime code is needed.

##### C. Baseline comparability and acceptance ceiling

B1/B2 expose useful allocator symbols but have **no approved matched runtime carrier**. Candidate lacks `/bin/shell`; current init/Terminal/launch profiles select `/bin/cellbox`, historical shell's name/launch contract is different, B2 lacks valid signatures, and legacy canonical images retain forbidden inbound/contained artifacts. Do **not** boot that unsafe baseline image, rename/replace Cellbox, alter autoload/policy, inject a baseline ELF through a forbidden route, or claim that changing only the executable creates equivalent whole-system features.

Recommended initial protocol is **candidate runtime characterization + B1/B2 static comparisons**, with before-runtime cells explicitly **unavailable**. Full before/after startup/memory comparison and Phase10 criterion4's equivalent-feature clause therefore remain **unmet**. A genuinely matched legacy carrier requires its own reviewed image/policy/launch/feature design; it is not implicitly authorized by this observer CP1. Even a successful observer would not automatically close CBX27 or supply byte-exact/kernel-stack peaks. No universal “smaller/faster”, crash durability, physical/VMX, SSH/publickey or production claim follows.

##### D. Concrete audited signing option (separate approval; not executed)

**Existing capability, not a claimed built-in package-scoped route:** `scripts/cellos-sign --repo ROOT` runs unchanged F1 over **all Cells in ROOT**, with the existing release-tarball filesystem fallback when ROOT is outside Git. There is no `--package`/`--scope` flag. Named S10-2a candidate routes also run whole-root F1 first and bind only their pinned assembler; they **cannot** be borrowed for historical shell or used to waive its workspace failure.

**Recommended D4-Sign1 A: explicitly reviewed source-exact reduced mirror + unchanged existing CLI.** After user approval only:
1. Create `/tmp/cellbox-s10-4/signing-review-root` outside every Git checkout, mode0700. Copy **all tracked files of both complete Cell crates**, `cells/tools/shell` and `cells/tools/sys-tools` (38 Rust sources,12 roots including **all11 worker bins**, not just the five signed workers), and the **8 total local crate directories** in the conservative closure (those2 Cell crates +6 trusted libraries/helpers), including manifests/sources. Include exact reconciled Cargo.lock/root manifest, rust-toolchain pin, unchanged historical `unsafe-allowlist.toml`/low-level signer and needed CLI support; record which existing current/historical tool hashes run. Make snapshot source files read-only, require regular files/no symlink or hardlink substitution, and compare its full membership/hashes before and after checks/signing. No source deletion within a selected crate, conditional-file omission, new exception or edited allowlist/date. This explicitly scoped release-tarball-style **source view** is not a pristine full workspace or build root.
2. Generate complete source/snapshot SHA manifests against the historical worktree/resolved graph and already-built six original unsigned hashes. Registry/version/source pins remain as audited; `libs/*` and registry crates retain the policy's **trusted-TCB, outside-F1 boundary**, not newly certified memory safety. Stop on dependency/source/hash mismatch or missing file. No compiler run/rebuild in the mirror.
3. Preserve the original whole-workspace F1 failure. Run the current unchanged CLI using its existing `--repo` interface: `python3 /home/dmin/cellos/scripts/cellos-sign --repo /tmp/cellbox-s10-4/signing-review-root --check --strict`, then `--sign` exactly six fresh regular single-link copies under `/tmp/cellbox-s10-4/b2-reviewed-signed/{shell,ls,cat,echo,ps,kill}`. The existing signing path reruns F1/F5 strictly before loading a key; no `_CHECKED` manipulation, monkeypatch, environment bypass, new qualified route or unchecked mode.
4. **Exact F1 coverage:** whole38-file Cell corpus including files outside module graphs; root forbid layer for the selected crates/all worker roots; **existing `app-shell` crate exception** (`temporary-audit`, cellos-maintainers2026-07-30, review_by2026-10-28, unchanged) remains explicit. No selected file exceptions were found. This does **not** turn full historical workspace F1 into PASS. Require the scanner's file/root census to equal the reviewed view; retain hygiene/overdue notes, never fabricate an exception approval.
5. **Exact F5/signature coverage:** active rustc full commit `f53b654a8882fd5fc036c4ca7a4ff41ce32497a6` equals installed nightly-2026-05-01 and actual build metadata; skipped/unverified is failure. Existing current preflight blocked-hash checks remain; same dev public-key SHA `7db0b01f…e666f`, no seed/prod/fixture override. One64B `__ViCell_sig` section, canonical final-layout payload, independent existing verifier read-back per ELF; retained unsigned originals stay unchanged. Report actual signed file/section sizes against B1/after only after all six pass.
6. Archive reviewed source membership, exact tool/source hashes, strict-check/sign/refusal logs, signed/unsigned hashes and unchanged canonical/candidate proof. Scope-qualified signatures are **historical measurement artifacts only**, not a new deployable image/candidate or authority grant. Remove mirror/worktree after finishing and archive results; preserve every earlier refusal.

Alternatives: **B** retain actual unsigned B2 and mark signed comparison incomplete (no scope change/signing); **C** separately audit/fix the unrelated historical ocel-quickjs F1 exception under a new product/security approval (not recommended inside footprint measurement). A new allowlist entry or modified global gate is **not** part of optionA.

##### E. One consolidated user CP1 request

| Decision | Choice/recommendation | Consequence |
|---|---|---|
| **D4-Obs1** | **A recommended:** approve the host-only candidate-preserving protocolB (3 fresh untraced boots/profile +1 observed boot/profile, exact workloads/stop rules); B defer | Host observer implementation/execution only after explicit approval; no candidate rebuild/runtime patch |
| **D4-Obs2** | **A recommended:** approve exact meanings: aligned allocator peak;4KiB CPU-stack-write depth bounds; mapped backing; null exact RSP/kernel-stack/RSS peaks. B require a separately designed stricter observer | A does **not** claim missing peaks measured; no dirty-page substitution |
| **D4-Obs3** | **A recommended:** characterize candidate only; retain unavailable B1/B2 runtime comparator and **CBX27/criterion4 unmet**. B require a separate matched historical-carrier CP1 before runtime comparison | No unsafe baseline boot, hidden feature equivalence or automatic closure |
| **D4-Sign1** | **A recommended:** approve source-exact38-file reduced-mirror F1/F5 signing scopeD; B retain unsigned/incomplete; C unrelated full-workspace audit outside this slice | New scope must be approved explicitly; original F1 refusal/containment remain |

**CP1 record (user2026-10-28, relayed by Main): APPROVED D4-Obs1/Obs2/Obs3 A and D4-Sign1 A.** Execute exactly the protocol/scoped source view above; missing exact stack/RSS/kernel-stack and matched baseline runtime remain unmeasured, CBX27/criterion4 unmet. Actual results/deviations require separate user CP2; **no canonical promotion or containment lift**.

#### S10-4.5 Scoped historical signing implementation/results (2026-10-28; CP2 pending)

**D4-Sign1 A executed after explicit user approval.** `sign-mirror.py` created a source-exact read-only release-tarball-style mirror:254 files, complete2 Cell crates/38 Rust sources/12 roots and8 total local crate closure, original app-shell exception/allowlist unchanged. Exact membership/content before/after SHA **`65af0e93744fce172c854d2de7589087a50362deba4efbbe69a7f2ae9a6d23c0`**. Existing current CLI `--repo` strict **F1 PASS** at that explicitly approved scope, **F5 PASS** at full compiler commit `f53b654a8882fd5fc036c4ca7a4ff41ce32497a6`. Whole-workspace F1 refusal remains unchanged; no global policy PASS, allowlist amendment, compiler/candidate rebuild or bypass.

All six fresh original-unsigned copies were signed once with the same approved dev key, independently verified by the existing verifier and canonical-payload Ed25519 verification, and contain exactly one64B non-ALLOC `__ViCell_sig` section. **Six signed total980,528B, unsigned994,064B**. Same size as B1 is not blanket byte reproduction: shell alone is byte-identical; all five rebuilt workers differ in hash.

| ELF | B2 original unsigned B | B2 scoped signed B | Scoped signed SHA-256 |
|---|---:|---:|---|
| shell | 880,992 | 881,120 | `225fb29f45a6bf486357dac791dd4ea9cc76e9120f2edae047d2bbe2acbedf90` |
| ls | 24,424 | 20,952 | `3a99ac33ff89f237c3fd908bcdb0b3e233bcb99c5570440d3a5b6a7f02adcffc` |
| cat | 24,424 | 20,976 | `9fb4d1ff159ea0faf3d373043ccd25cc2bcea72b60346636d84f1208a7c0dc72` |
| echo | 24,424 | 20,952 | `861ca14a7b28eab867eee713ebc9c110701873bc41edff2055a3ad415bb574c0` |
| ps | 15,304 | 15,432 | `b5371860a45303815dd77a2b562d08055024ac7eead9cd0e143e30333341544a` |
| kill | 24,496 | 21,096 | `b5fe3980d032c912b90447e886b4f8ac708dc5872a7714cad5c1a5f7a4cc2851` |

**Matched signing-state static comparison is now available:** B1 and B2 each six980,528B (suite881,120 + workers99,408). Thus the qualified after deltas vs **either signed baseline** remain headless **+2,264,888B** /desktop **+2,264,952B** for six files, or **+5,358,840B/+5,358,968B** including the separately charged SSH copy. Explicit feature non-equivalence/regressions and absent historical runtime comparator remain; this does not establish a smaller/faster image or any memory saving.

**Recorded harness deviations, not policy waivers:**
1. Initial generic preservation helper wrongly demanded single-link canonical inputs; corrected only read-only input hashing. Mirror/source-output single-link requirements remained. Initial evidence retained.
2. Intermediate source-census ordering and rustc `.comment` nine-character-prefix comparisons were overconstraints; full38-file membership/full active/pinned/build-log commit remained verified. Both original diagnostics and corrected evidence retained.
3. An extra allocated-section metadata-identity comparison stopped after signing. Static diagnosis showed only GNU objcopy's `.init_array` type1→14/entsize0→8 normalization and zero-file BSS PT_LOAD offset→0 (exceptps PHDR exact); allocated bytes/VA/flags/sizes/alignment unchanged. Parent explicitly continued **existing-verifier readback only**, no resign. All six valid canonical signatures/actual layout deltas retained; **no PHDR-byte-identity claim**.
4. Signing archiver initially wrote its own evidence under root `reports/` instead of the phase archive. Moved only its created evidence to **`reports/s10-4-evidence/signing/` relative to this phase directory**, retained executed/corrected harness snapshots, and removed its empty ancestry; unrelated root reports untouched. No verification/signing rerun.

Generated evidence: `signing/verified/{summary,scope-comparison}.json`, complete prior attempts under `signing/`; before/after proof covers **546 protected paths unchanged**, original unsigned/historical source/tools/refusal unchanged. After evidence was secure, parent restored only its2 disposable manifest edits and plain `git worktree remove` **exit0**; mirror and baseline compilation cache removed. `cleanup.json` records actual commands/results. Six signed/unsigned measurement ELFs remain outside repo for continuation; qualified candidate remains read-only.

**Host observer tests:** actual one-time `python3 -m unittest -v test_rsp test_measurement` passes **43/43** (packet policy/framing/XML/registers/physical-view restoration, page walks/large pages/cache, transient allocator peak preservation, fail-closed bounds and finite slot census). Synthetic host tests are not guest observation evidence. Original tool result is retained for generated archive extraction; runtime measurements/results follow separately. No product tests/builds were repeated for this host-only slice.

#### S10-4.6 Runtime harness premise errors and partial observations (2026-10-28; no product defect inferred)

**Initial attempt:** genuine shell prompt appeared normally, but host polling examined only cumulative UART after an `Init: service exited cleanly` line coalesced behind it. The final buffer was not idle, so the host incorrectly waited900s. Preserved raw receipt offsets/failed verdict. Narrow host correction evaluates each received prompt-prefix boundary with the **unchanged** `Shell.idle(raw_prefix,count1)` and timestamps the containing receive; no accepted prompt grammar/background-ignore policy change. Three new host behavioral cases pass (coalesced Init record, arbitrary substring rejected, partial prompt rejected). Original43 tests unchanged; no product/candidate rebuild.

**Corrected attempt partial actual measurements:** fresh headless VM→first exact idle-prefix receive **2,925.614379ms**;100 idle samples over **10,000.222692ms**, whole-owned-QEMU CPU **9.41s** including sampling/boot tail,184 additional UART bytes (background service messages). This is not shell-only CPU, cold-host-cache startup, redraw count or memory. Finite `top --records -d 1 --frames 2 | to jsonl | cat` emits exactly2 frames/status0.

**Second harness stop, clarified before classifying product behavior:** the approved new observer protocol incorrectly required live JSON from `top --records --frames 100000 | to jsonl | cat`;120s produced none. No product hang is established: [S10-3b.8](#s10-3b8-implementation-and-results-2026-10-26-cp2-approved-2026-10-28-with-deviations-14) explicitly approves collected byte sinks writing nothing until completion and on Ctrl-C returning130/no bytes. `executor.rs:1875–1895` selects Collected for `|cat`; `:1919–1922` returns130 without writing. `typed.rs:915–931,959–1001` polls before each pull, closes the chain and returns interrupted130. **Existing qualified L0 raw verdict explicitly notes “output before Ctrl-C:0 rows” and cat cancellation0.3s/status130 PASS**. Its separate `endless-sink-flowing` check is the **no-cat terminal-streamed** command. Therefore the new live-cat predicate was an **observer assumption error**, not a new product defect, waiver or reason to change the buffered contract. Original failed runtime verdict/source/serial remain intact.

**Main disposition (2026-10-28): proceed with the contract-correct observer fix inside approved protocol; no new product/acceptance approval needed.** Retain finite2-frame cat/status0; measure endless **collected** cat10s with no premature prompt/no JSON, Ctrl-C→**new** prompt/status130 and4s quiescence. Separately measure existing terminal-streamed **no-cat** `top --records --frames 100000 | to jsonl`, require ≥3 live rows,10s observation, Ctrl-C/new prompt130 and4s quiescence. This distinguishes the already-approved buffered cancellation vs streaming contracts without weakening either, changing product bytes or raising a deadline. Both900s-prefix and120s-live-cat failures remain **observer bug deviations**, not product defects; fresh exact-candidate schedule proceeds, not a rerun to confirm either failed premise.

**Continuation actual progress:** all **3/3 headless+3/3 desktop untraced** boots/workloads complete on the unchanged candidate. Headless exact-prompt startup2,589.808195/2,646.040699/2,923.023544ms. Desktop VM→ready3,872.364604/4,146.777226/4,170.796745ms; click→exact Terminal prompt4,169.392301/4,209.187360/4,307.725282ms. Headless all three finitecat2frames/status0, bufferedcat10s/noJSON→CtrlC130, separate ≥3-row live stream10s→CtrlC130/4squiescence and largefixture rg cancellation130 complete; these are separate contracts, not performance-equivalent baseline claims. Whole-QEMU idle/top CPU and GUI sampled frames remain in raw per-run evidence.

**Observed setup compatibility bug:** first observed VM never resumed: captured QEMU target.xml uses conventional `xi:include` without `xmlns:xi`, strict ElementTree rejects “unbound prefix”, surfaced as malformed XML. Host parser now binds only that known prefix on target, still rejects entities/unknown prefixes/bad include paths/depth/invalid registers; **2new exact-packet/security regression tests PASS**. Original stopped observation/packets preserved as third CP2 harness deviation. Resume only two observed lanes; **no timing rerun/product rebuild**. Until generated observed verdicts, heap/stack/backing remain unmeasured; CBX27/criterion4 and S10-5/containment unchanged. Latest completed preservation comparison **546/546 paths unchanged**.

#### S10-4.7 Native hardware-breakpoint step-over amendment (**user CP1 approved 2026-10-28**; result CP2 pending)

**Confirmed observation-method blocker, not a product defect:** after XML correction, persistent hardware entry breakpoints repeatedly stop at Platform `0x100000000` and Init `0x102000000` on each `c`; current native instruction does not advance, other task may receive timer dispatch. First10,000 trace lines contain408 RIP reads with only those two values; no Cellbox instance/heap event/snapshot before900s host limit. Original method allowed continue but rejected native step. No retry, raised deadline, software breakpoint, unguarded continuation or product fix. Observer's raw thread shutdown says `COMPLETE` only because no internal exception; **whole run STOP and zero identified instances mean no qualified memory result**, not zero memory.

**User-approved amendment, no implicit guard change:** while stopped at an enrolled hardware execute breakpoint at exact current RIP, remove **only that current hardware breakpoint**, execute **exactly one native instruction**, restore the same breakpoint **before any continue**. Other execute sites and heap/stack watch guards stay installed. No arbitrary/address/signal/vCont stepping, injected instruction/function, guest memory/register write, `set $pc`, Z0, coverage relaxation or product/canonical change. A step failing to advance, unexpected signal, unknown writer/stop semantics or restore failure stops observation. A watchpoint or another enrolled execute site reached by the native instruction is processed before further continuation.

**Host behavioral proof before fresh observed boots:** **7/7 PASS**: exact remove→one` s`→restore→continue order; enrolled/current-RIP gating and continued raw-step/write/Z0 refusal; no-progress fail-closed restoration; interleaved write-watch preservation; observer entry transition, watched-write dispatch before continuation and unexpected-signal stop. Earlier43+3+2tests remain exercised separately, not silently rerun.

**Evidence/retention:** generated `runtime/hardware-resume-observer-failure/diagnosis.json` records raw trace SHA-256 `46f873d62eed60b9a6e9e68c3ebbc64d4df1841ad4529f631b9769f67d8e270a`, **4,282,115,315B** raw retained outside repo; lossless deterministic gzip **431,927,301B** at `/tmp/cellbox-s10-4/runtime/failed-hardware-resume-trace.jsonl.gz` (not copied into repo). Small actual packet excerpt, serial/argv/empty capture/status and executed collector are archived. Two corrected collector assertions had over-narrowly assumed only Init in sample, while actual scheduler alternates Platform/Init; corrected evidence uses both actual values, no runtime rerun. Main directs two fresh observed lanes only after host tests; all six completed untraced results retained without rerun. Original unsupported result and explicit amendment are CP2 deviations, not a claim of memory completeness or criterion4 closure.

#### S10-4.8 Actual partial measurement delivery and observer prerequisite (**limited-result CP2 approved by the user 2026-10-28; not full completion**)

**Completed deliverables:** generated static/per-ELF/section/signature/FAT/kernel/ISO provenance; historical original-unsigned6=994,064B and strict scoped-signed6=980,528B (six valid signatures, original whole-workspace refusal retained); all **six fresh untraced runs COMPLETE**. Headless candidate6=3,245,416B/alias-inclusive7=6,339,368B; desktop6=3,245,480B/7=6,339,496B. These are **regressions** against B1/B2's980,528B signed6, with richer features/different workers/carriers; no optimization/equivalent-system baseline claim.

| Fresh untraced metric (ms) | Run1 | Run2 | Run3 | Min / median / max |
|---|---:|---:|---:|---|
| Headless VM spawn→exact idle prompt receipt | 2589.808195 | 2646.040699 | 2923.023544 | 2589.808195 / 2646.040699 / 2923.023544 |
| Desktop VM spawn→ready marker | 3872.364604 | 4146.777226 | 4170.796745 | 3872.364604 / 4146.777226 / 4170.796745 |
| Desktop taskbar Terminal click→exact pixel prompt | 4169.392301 | 4209.187360 | 4307.725282 | 4169.392301 / 4209.187360 / 4307.725282 |
| Desktop VM spawn→prompt, **includes retained3s pre-click automation delay** | 11186.309965 | 11436.829514 | 11558.978615 | 11186.309965 / 11436.829514 / 11558.978615 |

**Idle/top actual scopes:** headless100 UART samples/10.0002s each; ordinary whole-QEMU CPU9.44/9.54/9.41s with246/246/183 background UART bytes, interactive-top0.59/0.65/0.60s CPU and1686/804/1507 UART bytes. No headless framebuffer metric. Desktop ordinary idle100/100/99 samples over10.059/10.095/10.080s, **0/0/0 content-change frames** excluding only recorded cursor; whole-QEMU CPU10.13/10.16/10.16s. Interactive-top98/97/97 samples over10.084/10.072/10.098s, **10/10/10 content-change frames**, CPU10.16/10.14/10.18s. GUI UART0 in these intervals. Sampled pixel changes are **not redraw-call counts**; whole-owned-QEMU CPU includes all guest/rendering/sampling work, not shell-only CPU. Actual target10Hz gaps retained, no fabricated100samples.

**Bounded stream behavior (headless3/3):** each finite Collected cat emits2frames/status0; endlesscat10s emits no JSON/no premature prompt, Ctrl-C→new prompt130 in20.539531/20.610945/20.566818ms,4s quiescence. Separate no-cat stream shows≥3live rows then10s, Ctrl-C130 in20.842973/20.869692/20.750409ms,4s quiescence. Pinned7,340,032B rg fixture cancellation130 in142.873525/163.226411/183.686325ms. This is controlled workload boundedness, **not generic throughput or matched baseline runtime**. Desktop untraced schedule measures startup/idle/top only; desktop observed stream/memory **unrun**, no substitute UART route.

**Observed memory is partial, never a completed-workload peak:** corrected-amendment headless run captured209 post-update events/210 coverage writes, aligned-used **29,216B initial prompt→33,312B after ordinary idle**, event peak **43,056B over captured partial interval only**, mapped ELF PT_LOAD+64KiB user-stack union **10,199,040B** at both snapshots,9 stack page write attempts →depth bound **32,769–36,864B**. Earlier echo-parser-failed *separate* run captured1489events/partial peak59,416B,12stack pages→45,057–49,152B; do not merge runs or label either full-run peak. `.bss` arenas and these mapped pages are not RSS/working-set/whole-cell physical peak; no exact stack/kernel-stack measurement or matched historical runtime baseline. Observed timings/CPU are not performance results.

**New genuine observer-method STOP:** at relocated post-add `RIP=0x1080e40b1` (exact ELF `eb02 jmp0x1080e40b5`), watch trap reports heap `0x1088a80c0`, value38,104B. Observer removes only currentZ1, issues exactlyone native`s`; bareT05 returns **same RIP**, then restoreZ1 succeeds. No second candidate step, continuation, dropped watch or deadline increase. Guard stops; the surface's later120s wait is secondary to stopped observer, **not a product hang or defective pipeline**. Headless full observed workloads incomplete, desktop observed unrun.

**Focused host-only diagnosis (Main-authorized, no candidate rerun):** pinned [QEMU8.2.2 watchpoint code](https://github.com/qemu/qemu/blob/v8.2.2/system/watchpoint.c) can queue `CPU_INTERRUPT_DEBUG`; [cpu-exec](https://github.com/qemu/qemu/blob/v8.2.2/accel/tcg/cpu-exec.c) consumes it before next instruction even withSSTEP_NOIRQ and can signal debug on a non-executed TB. [gdbstub/system.c](https://github.com/qemu/qemu/blob/v8.2.2/gdbstub/system.c) reports then clears `watchpoint_hit`; bareT05 is not affirmative retirement proof. Hoststep is CPUState/cflags, not guestTF-register injection; failed-event RF/TF values **not sampled**, no inference. **Isolated Q35/TCG watched-add+postZ1 reproducer advances normally2/2 variants+1000/1000 repeated first native steps**; generic watch+execute alone is insufficient to reproduce actual no-progress. Existing packet/source evidence cannot identify hidden pending-debug/cpu-exit state or safely establish extra candidate step semantics. Source URLs/SHA/excerpts, actual packets, isolated binaries/scripts/argv/results and incidental socket-listen readiness failure are generated/archived. **Unavailable prerequisite:** independently demonstrated instruction-retirement/pending-debug disambiguation or a separately approved bounded semantic amendment. Do not remove no-progress guard based on plausible pendingtrap.

**Evidence/CP2 deviations:** `reports/s10-4-evidence/summary.json`, `evidence.sha256`, static/B2/signing logs; runtime raw UART/receipts/PNG/diffs/RSP/PTE/heap/stack witnesses; original-prefix/live-cat/XML/native-resume/echo-parser failures, user-approved native-step amendment and finalunsupported trap all preserved. Distinct host behavioral tests **59PASS** (43+3+2+7+4; not59 reruns). Disposable historical worktree/mirror/cache removed after evidence secure; original measurementELFs/read-only qualified candidate retained in/tmp. Final generated proof **546/546 protected paths including469 canonical, every candidate file and user's VMX source unchanged**. Known COMPLETION-QUEUE/xHCI/other prior ceilings remain disclosed.

**Request limited CP2** for these actual static/signing/timing/idle/controlled-headless-stream/partial-observed results and listed deviations/stop, **not** approval of unrun desktop observation or complete memory peaks. **CBX27 and Phase10 criterion4 remain unmet** (now also full observed-method coverage blocked); CBX14/26 stay qualified-candidate-only, SSH23–25partial,28pending. No universal optimization, complete-memory, baseline-runtime, canonical promotion, containment lift, physical/VMX/production or S10-5 closure claim.

#### S10-4.9 Bounded observer execution-semantics diagnostic (**user CP1 approved 2026-10-28; one attempt executed, stopped; no additional control approved**)

**Approval record (user, relayed by Main):** exactly **one** fresh headless diagnostic attempt on unchanged qualified candidate `4fae8b20…`, reusing the existing observed workload prefix and original 900s/command deadlines. Stop at the first same-PC native-step failure, workload completion or deadline. This is diagnostic evidence, not a full memory/timing qualification rerun; no desktop run, product/candidate rebuild or canonical/containment change.

**Retained evidence motivating this diagnostic:** the complete final failed-run RSP trace has 221 native steps, 220 advancing and one unchanged. The identical relocated post-add `0x1080e40b1` advances to `0x1080e40b5` on 123 prior steps, then returns the same bare `T05thread:01;` without advancing on its 124th occurrence. All stops identify thread01; no EFLAGS, whole-register, CPU-selector or single-step-mode queries were retained. The 4.28GB compressed trace belongs to the earlier persistent-breakpoint failure, not this final failure. No generic watch/step reproducer is repeated.

**Approved observation only:** retain every existing hardware execute/write guard and the original no-progress/restoration logic. Around each already-approved one-`s` step-over, while RSP-stopped, enable only stock QEMU `exec_tb`/`exec_tb_exit` tracing and verify event states; retain gdbstub command/reply/watch/break and `breakpoint_singlestep` events in one private, TID/timestamp-prefixed log. Record read-only QMP `query-cpus-fast`/`query-status`, raw stop packets, XML-derived RIP/EFLAGS/CS/CR3/RSP and instruction bytes, plus exactly read-only `qqemu.sstepbits`/`qqemu.sstep` extensions. No mode setter, guest memory/register write, injected call, Z0, extra/drain/retry step, guard relaxation or post-failure continue. Host behavioral tests for the wrapper/policy/ordering run once before the one attempt.

**Evidence interpretation gate:** complete enabled tracing with no TB-entry event between a native `s` and its stop can establish no guest TB execution in that interval, subject to trace-health/source-path checks; a TB-entry event alone is not retirement proof, and missing TB-exit alone proves nothing. Missing/incomplete metadata or trace, unsupported controls, context mismatch or non-reproduction is **inconclusive**, never permission for another step/run. Actual hidden pending DEBUG versus other pre-execution/early-exit causes remains unproven unless independently captured. Public CPU/status/EFLAGS metadata alone cannot expose consumed QEMU CPUState. Matching host DWARF/debugger introspection is a separate unavailable prerequisite, not installed or substituted in this diagnostic.

**Preservation/delivery:** generate ordered trace-scope, RSP/QMP/context/trace-health records and before/after canonical/candidate/user-VMX proofs under private `/tmp`, archive the small generated evidence and exact host helper/tests in `reports/s10-4-evidence/runtime/observer-diagnostic/`, retain the earlier compressed trace untouched. No new work is committed/pushed by this CP1. Actual results/new decision follow separately; limited S10-4 CP2 and **CBX27/criterion4 unmet, S10-5 blocked** remain unchanged.

##### Actual diagnostic result and next gate (2026-10-28)

The new host wrapper's **14/14 synthetic behavioral tests passed once** before execution. Exactly **one** fresh unchanged-candidate headless attempt ran, stopping promptly at the first no-progress error during the finite-stream command. There were **551 native steps: 550 advancing trace-health scopes, then one same-PC stop**. Every scope retains identical before/after execute/watch censuses; the failed current Z1 restored successfully. Recorded RSP has no guest-write/Z0/injection packets and **no continue or step after the failed `s`**. No generic reproducer, second candidate attempt, desktop, rebuild or product verification was run.

**New discriminating fact, not the old pending-DEBUG guess:** failure551 is at **post-sub `0x1080e3f06`, opcode`58` (`pop %rax`)**, not the earlier post-add `eb02` jump. The same CPU/context/instruction/EFLAGS/RSP are retained before and after: GDBthread01 = QMPvCPU0/hostTID2134370, CS`0x23`, CR3`0x59000`, EFLAGS`0x202` (**TF/RF both clear**), native mode`0x7`. The previous scope at the identical pop advances RIP+1/RSP+8 and has exitflags0. Final ordered trace has exactly one `exec_tb` at that PC, then **the identical TB pointer** in `exec_tb_exit flags=0x3`, then bareT05 and restoration. Events are enabled before/after the interval; host TID matches the sole vCPU, and all550 prior scopes have flags0. This is complete positive early-exit evidence, not an inference from missing log lines.

**Confidence: CONFIRMED for this attempt's unstarted-TB/zero-retirement mechanism; hidden exit-request origin and old-jump attribution UNPROVEN.** [QEMU8.2.2 `TB_EXIT_REQUESTED=3`](https://github.com/qemu/qemu/blob/v8.2.2/include/tcg/tcg.h) explicitly means return before the pointed-to TB started, with CPU state synchronized back. [`cpu_tb_exec`](https://github.com/qemu/qemu/blob/v8.2.2/accel/tcg/cpu-exec.c) can subsequently generate EXCP_DEBUG for enabled host single-step without first requiring instruction retirement. Static inspection of the installed build-ID`ce72f123…` binary confirms its flag>1/synchronization and single-step/debug branch structure; exact objdump output is archived. Thus **this one `s` retired no instruction**, despite entering the TCG dispatcher. It does not establish who set `exit_request`, actual pendingDEBUG state, or the cause of the earlier jump's failure.

**Preserved verdicts/evidence:** the executed conservative classifier remains `INCONCLUSIVE_SAME_PC` (it only recognizes zero-dispatch, not unstarted returned TBs); separately generated `interpretation.json` records `TB_NOT_STARTED_REQUESTED_EXIT_OBSERVED` under the explicit source contract. Original `summary.json` is not rewritten to manufacture success. `reports/s10-4-evidence/runtime/observer-diagnostic/` contains helper/tests, raw RSP/QMP/shared trace/XML/context, failed and preceding-success trace excerpts, source interpretation, offline collector and generated proofs. New proof **546/546 protected paths unchanged, including469 canonical, every candidate file and user's VMX source**; old431,927,301B compressed trace unchanged, executing host sources unchanged. Owned writable diagnostic clone removed; no commit/push.

**New decision required:** keep observation stopped, or separately approve a bounded semantic amendment. A candidate-preserving amendment would need to admit **at most one additional native`s` only after complete same-TB exitflag3 evidence proving zero retirement**, stable CPU/CR3/CS/RIP/instruction/context, successful restoration and intact watch coverage; a second unstarted/no-progress result, missing witness or unexpected stop must stop, never loop/drain or authorize a third step. That policy and its tests need their own explicit CP1; **not implemented/executed here**. Direct hidden-state origin attribution still requires a separately reviewed host-introspection prerequisite. Neither option supplies full-workload peaks, desktop observation or a matched historical carrier. **CBX27/criterion4/S10-5/canonical/containment remain blocked.**

#### S10-4.10 Evidence-gated one-additional-step diagnostic (**CP1 approved2026-10-28; host correction/replacement authorized2026-10-29; one recovery demonstrated, diagnostic stopped**)

**Approval record (user, relayed by Main):** implement the S10-4.9 next-gate boundary and execute one fresh headless diagnostic attempt on the unchanged qualified candidate/workload prefix. At the first original no-progress stop, **at most ONE additional native`s` in the entire attempt** is authorized, only when complete ordered same-TB exitflag3 evidence proves that first attempted TB never started, current context is stable, restoration succeeded and all execute/watch coverage is intact. Missing proof, any unexpected stop or a second no-progress result stops; **no third step, drain loop or retry run**.

**Recurrence is not inferred:** S10-4.9's proposal/user approval did not explicitly authorize repeated per-occurrence recovery. Therefore this diagnostic stops/reports **also when that single additional step advances**, with its current breakpoint restored and without another workload continue. Further recurring use or a resumed full observed workload needs its own explicit approval. If no admissible stop occurs before the existing workload completes/deadline, report non-reproduction/inconclusive; do not spend the allowance elsewhere.

**Admission proof (host overconstraints corrected by Main/user disposition2026-10-29):** retain the failed original step and original classifier/raw evidence unchanged. Verify raw trace SHA/boundaries and ordered remove-current-Z1/OK→one`s`/mode7→one TB entry→same-pointer exitflag3→bare thread01T05→step disable→restore-current-Z1/OK, with no interleaved guest-control/write/watch removal. Require same sole QMP CPU/host TID, stable XML-derived RIP/CS/CR3/RSP/EFLAGS/instruction/sstep modes across before/after/fresh pre-additional reads; TF/RF clear; exact pre/post/current full breakpoint/watch census equality. An empty watch census is allowed **only with explicit live observer lifecycle proof that no watched Cellbox exists yet**, full finite entry-breakpoint coverage and no verified instance/entry ordinal/focus; empty watches after Cellbox enrollment remain a stop. Retained source-exact same-binary positive trace health plus actual enabled-logging acknowledgments suffice; a preceding successful same-site scope in this boot was **not a user condition** and is not required. No same-address instruction-byte identity is assumed across different cells. Recheck actual enrollment/coverage/context/lifecycle immediately before the additional native operation. Reuse the original native-step guard/restoration and host trace collector; no guest memory/register writes, injected code/calls, Z0, candidate/product change, dropped coverage, deadline extension, desktop or unrelated qualification runs.

**Verification/delivery:** new synthetic host tests cover proof completeness/pairing/order, context/coverage/enrollment/restoration, at-most-two total operations at that boundary, missing proof, unexpected stops and second no-progress fail-closed behavior; run once after edits and before the attempt. Preserve S10-4.9's archive, earlier failures/classifier/offline interpretation and compressed trace byte-for-byte, plus canonical/candidate/user-VMX before/after proof. New helper/tests/raw/generated results go under `reports/s10-4-evidence/runtime/observer-step-amendment/`. No commit/push. Actual result and any next decision remain separate; **CBX27/criterion4/S10-5/canonical/containment blocked**.

##### Initial host admission stop and authorized correction (2026-10-29)

Initial implementation's16 synthetic tests passed, but its one diagnostic attempt stopped on **the first native operation at Init entry`0x102000000`**, before any Cellbox instance, heap events or snapshots. Actual current bytes begin`4883e4f0`; stable before/after context and complete same-pointer TB exitflag3 were retained. **Zero additional native steps were issued.** The host gate wrongly demanded a nonempty watch census, although the expected pre-Cellbox census was empty and unchanged; it also demanded earlier successful same-site progress within this boot. These were extra harness restrictions, **not lost coverage, missing user permission or a new product blocker**. Original raw result `PROOF_REJECTED_BOUNDARY_STOP`, failed helper/tests,16-test output, earlier S10-4.9 evidence and compressed trace are preserved unchanged.

**Explicit Main/user disposition2026-10-29:** correct only those host overconstraints to stage-appropriate equality/lifecycle and retained same-binary positive trace health with actual logging acknowledgments. Add expected-empty versus dropped-watch/missing-lifecycle/proof tests and run them once after correction. **One replacement fresh headless diagnostic is authorized to exercise the still-unused ONE global additional-step allowance**, not a recurring retry rule: stop immediately after its outcome, no subsequent workload continue, third step, drain, desktop/build or other qualification run. Store replacement evidence in sibling `reports/s10-4-evidence/runtime/observer-step-amendment-corrected/`; preserve the original stopped archive and all protected bytes. Correction/replacement results pending; no commit/push.

##### Corrected replacement result (2026-10-29)

**Corrected host behavior:** **21/21 synthetic tests PASS once after edits**, including expected-empty lifecycle versus dropped/enrolled heap-watch coverage, missing lifecycle/logging/retained health, positive ordered same-TBflag3 proof, restoration/context, single global allowance, unexpected stop and second-no-progress/no-third-step stops. Initial16-test output and executed helper/test snapshots remain separate historical artifacts, not added to21 as independent passes. The initial Init bytes are verified byte-for-byte against the qualified Init ELF/hash; retained S10-4.9 ordinal1 at that same verified Init entry also advanced. No different-cell code identity is inferred from address reuse.

**Exactly one authorized replacement fresh headless run:** **186 native packets/scopes**:184 advancing ordinary operations, original same-PC failure185, then **one** proof-admitted additional operation186. At actual post-add **`0x1080e40b1`, bytes`eb02` (`jmp +2`)**, first trace has exactly one TB entry and the same pointer exiting withflag3; RIP/CS/CR3/RSP/EFLAGS/instruction/native-mode context is unchanged. QMP soleCPU0/hostTID2215270 matches trace; CS`0x23`, CR3`0x59000`, EFLAGS`0x206`, TF/RFclear, mode7. One verified Cellbox exists with its heap watch retained; full before/after/current execute/watch censuses match. Retained same-binary/source-exact550 successful trace scopes and actual enabled logging acknowledgments admit the operation without inventing current-boot same-site progress.

**Additional step outcome: `ADDITIONAL_ADVANCED_BOUNDARY_STOP`.** CurrentZ1 is restored after failure185, removed only for the approved additional`s`, then restored again. Operation186 returns bareT05, **RIP advances to`0x1080e40b5` (+4)** with RSP/context unchanged, and trace exitflag0. This demonstrates completion/progress for this exact native jump after independently proven zero retirement of the first attempt, **not** a universal recovery or workload-completion claim. Original no-progress error/raw scope remains preserved. The observer then intentionally stops: **no third step, no post-boundary continue, no recurrence, no desktop or full-workload rerun**. Hidden exit-request origin/pendingDEBUG and historical untraced failure causality remain unproven.

**Evidence/preservation:** sibling `reports/s10-4-evidence/runtime/observer-step-amendment-corrected/` contains the exact corrected helper/test snapshots,21-test result, raw RSP/QMP/XML/context/shared trace, lifecycle/admission/retained-health records, both native-scope excerpts, offline interpretation and generated manifests. New proof **546/546 protected paths unchanged including469 canonical, candidate and user VMX source**; S10-4.9 archive, initial failed-admission raw/results/executed snapshots and old compressed trace unchanged; executing host sources unchanged during the run. Only the owned replacement disk clone was removed. No commit/push/product/candidate build or guard change.

**Next gate:** the one global additional-step allowance is now spent. A new explicit CP1 is required for any recurring per-occurrence use and a fresh complete observed workload; nothing here authorizes continued execution. Headless memory remains partial, desktop observed unrun, matched baseline/RSS/exact stack/kernel-stack absent; **CBX27/criterion4/S10-5/canonical/containment remain blocked**. Existing limited S10-4 CP2 does not automatically approve these new diagnostic results.

#### S10-4.11 Recurring conditional observer recovery and original complete observed schedule (**CP1 approved by user2026-10-29; two attempts stopped with partial results; CP2 pending**)

**Approval record (user, relayed by Main2026-10-29):** the S10-4.10 global one-extra diagnostic is complete and preserved. NEW authorization permits recurring **conditional** one-extra recovery for each independently proved original no-progress event, and successful recovery may now return to the original approved observer workload. This is not a blanket retry rule or retrospective expansion of S10-4.10.

**Per-event fail-closed contract:** before EACH additional native`s`, require complete ordered same-pointer TB entry/exitflag3 source-verified zero-retirement evidence, acknowledged enabled/healthy logging and retained source-exact same-binary health, exact stable soleCPU/instruction/context, TF/RFclear/native mode7, intact stage-appropriate execute/watch censuses with explicit lifecycle proof, and successful original currentZ1 restoration. Recheck fresh context/lifecycle/censuses immediately before the additional operation. At most ONE extra native`s` for that event. Additional no-progress, missing/corrupt proof, context/coverage/restoration drift or unexpected stop is terminal: no third step/drain, no further workload continuation. Only an additional bareT05 with actual RIP advancement and preserved coverage/captured trace may clear the event's terminal latch and resume. Every original error and per-event decision remains archived.

**Exact run scope:** add deterministic host-only tests for recurrence, one-extra-per-event, continued workload only after success, proof/census/lifecycle failure and terminal second-no-progress/unexpected-stop behavior; run once after edits. Then **one fresh headless and one fresh desktop observed attempt, sequentially**, unchanged candidate/fixtures/GUI oracle and exact S10-4.4 original `Surface` schedule: initial prompt/snapshot, ordinary idle10s, finite two-frame collected-cat control, endless collected-cat10s/cancel/quiescence, live no-cat10s/cancel/quiescence, pinned7MiB rg/cancel, real interactive top/idle10s/cancel, completion snapshot. Existing900s window and all original command deadlines remain unchanged. Throughput/observer overhead exceeding them is a real incomplete result, not permission to extend/retry. Do not redo six completed untraced lanes or rebuild/sign B2.

**Invariants/evidence:** no guest memory/register writes, software breakpoints/Z0, instrumentation/plugin, candidate/product/source rebuild, coverage relaxation or timeout increase. New helper/tests and per-profile raw RSP/QMP/XML/TB/context/lifecycle/decision/heap/stack/PTE/input/screenshot evidence go under `reports/s10-4-evidence/runtime/observer-recurring/`, with protected546-path before/after proof and all previous diagnostic/failure archives/compressed trace exact. Remove only owned disposable disk clones. Record full schedule completion separately from complete event coverage and metric bounds; incomplete event/coverage intervals are not true-workload peaks, stack write-attempt page bounds are not exact depth, mapped backing is not RSS, unknown historical runtime remains unavailable. No automatic CBX27/criterion4/S10-5/canonical/containment closure, commit or push. Actual results/deviations and CP2 decision are recorded after both attempts.

##### Actual two-profile results and retained-only diagnosis (2026-10-29)

**Host verification:** **27/27 deterministic host tests PASS once after behavior edits, before either attempt**. Tests cover exact existing proof/coverage/lifecycle admission, independent recurring events with one extra each, resumption only after captured successful advancement, first/later second-no-progress terminal behavior, missing/fresh-context/trace/coverage witness, forbidden controls and unchanged deadline enforcement. These tests did **not** exercise concurrent GUI/observer ownership of the existing QMP socket. Exact approved pre-result protocol excerpt/source hash and pre-run four-document hashes are retained.

**One headless attempt, original deadlines, `STOP`:**9099 actual native packets/scopes, **31 original same-PC failures and31 independently proof-admitted additional steps, all advancing and returning to the workload**. Sixteen events are post-add`0x1080e40b1`;15 are post-sub`0x1080e3f06`. All complete ordered sameTBflag3 proofs recompute from raw bytes; every recorded step's full execute/watch before/after census matches. Finite two-frame cat returns0; collected-cat buffering10s/cancel, live no-cat10s/cancel and pinned rg cancel return130 with required prompt/quiescence evidence. Recorded observed Ctrl-C→prompt intervals are1.280s/1.117s/9.042s, **not untraced performance claims**. Interactive top renders and its10s idle sample completes, but its **original10s new interrupt-prompt wait expires**. No complete/final snapshot; clean observer-thread shutdown does not turn this workload failure into completion. No rerun/budget increase.

**Headless deadline diagnosis, retained evidence only:** exact Ctrl-C-send/guest-consumption time is not logged (`Shell.send` only calls serial`sendall`). Last idle mapping snapshot timestamp415498015267604 precedes the wait; shutdown-only persistent heap-watch removal415508055118692 follows its timeout. Thus actual wait start is bounded within39.851088ms, and every possible original10s window contains a9.960148912s common interval. RSP stopped-receipt→next`s`/`c`packet-send intervals prove **at least7.355372922s of that actual10s was debugger-stopped**, leaving at most2.644627078s not proven stopped. That common interval still has692 native packets/691 continues and690 completed heap events. Whole-run `observer_pause_ns`103.061s sums handling intervals including native execution/snapshots; it is **not pure guest pause or an amount to subtract from this deadline**. The real wall-clock deadline was heavily consumed by observation; retained data **cannot establish a genuine product interrupt failure, guest Ctrl-C receipt time, or the counterfactual10-unpaused-second outcome**. No pause-excluded deadline is inferred.

**One desktop attempt, `STOP` on host QMP ownership defect:** real GUI launch/initial prompt and ordinary idle complete, but finite-command typing raises JSONDecodeError; concurrent observer metadata capture raises TypeError after `query-status` consumes a CPU-list return. **46 actual native packets,47 scope records,0 additional steps**: record47 fails during pre-step QMP capture, before its before/trace/native packet, so terminal census proof is missing. Earlier46 complete step censuses match; missing final proof is **not evidence of actual watch removal**. Source ownership is exact: `Machine.start` creates one`machine.qmp`; `Desk.__init__` binds that same object; GUI input/screendump uses unlocked `Qmp.cmd`; observer `TraceControl.query` directly reads/writes the same unbuffered socket file without lock/request IDs. Raw malformed GUI JSON bytes were not retained, but response misattribution and both failures are retained. This is a **host transaction-serialization bug, not a product finding**; no fix/replacement run was applied.

| Retained instance/interval only | Successful post-events / counter-write guards | Maximum retained aligned allocator value B (**not complete workload peak**) | CPU stack write-attempt pages / depth bound B |
|---|---:|---:|---|
| Headless foreground | 9053 /9054 | 284,064 | 12 /[45,057,49,152] |
| Desktop autoload background | 5 /6 | 25,272 | 4 /[12,289,16,384] |
| Desktop foreground Terminal | 25 /26 | 34,048 | 9 /[32,769,36,864] |

Each verified usable user-stack reservation is65,536B; page bounds are partial CPU write-attempt footprints, not byte-exact/RSP/kernel-stack peaks. Headless retains9 mapped snapshots, desktop foreground2; each selected ELF+user-stack snapshot has **10,199,040B present mapped backing**, **not RSS/residency/quota/whole-cell memory**. Headless draw/flush entry counts2/1 are partial calls, not redraw cost; desktop has no recorded entries, not proof of zero draws. Full-workload peaks and exact stack/kernel-stack/RSS/historical-runtime comparator remain **unavailable**.

**Preservation/evidence:** `runtime/observer-recurring/{summary,interpretation,retained-failure-diagnosis}.json`,27-test output, exact run helpers, raw per-profile RSP/QMP/TB/XML/context/lifecycle/heap/coverage/PTE/serial/screens and reproducible offline collectors retained. New proof **546/546 protected paths unchanged incl469 canonical+candidate+userVMX**, all three earlier diagnostic/failure archives and old compressed trace exact, executing host sources unchanged; only owned disposable disks removed. No untraced redo, B2/product/candidate build, guest writes/Z0/instrumentation/coverage relaxation, commit or push.

**CP2 / exact next decision:** accept/report these **partial** results and real stops only; no automatic CBX27/criterion4/S10-5/canonical/containment closure. Proposed but **not implemented/authorized** host fix is one shared per-QMP`RLock` across each entire GUI`cmd` and observer direct-query write-through-return/error transaction, installed before either owner starts; locking writes/readlines separately is insufficient. Existing`cmd(name, **args)` conflicts with trace arguments also named`name`, so preserve that UI API and lock the existing observer query body with the same mutex rather than misrouting trace commands. Test deterministic GUI input/screendump↔CPU/status/trace interleaving, response/event ownership, error/EOF/timeout unlock, no lock held across snapshot/waits, plus existing recurrence/proof/deadline invariants. Only a **separately approved bounded fresh desktop replacement after those tests** may execute; no headless rerun, silent timeout increase, pause-excluded clock or wider controls are included. Hidden exit-request origin and historical failure causality remain unproven.

#### S10-4.12 Host QMP transaction serialization and ONE desktop observed replacement (**CP1 approved2026-10-29; host fix tested; one replacement STOP on input run-state admission; partial-result CP2 pending**)

**Approval record (user, relayed by Main2026-10-29):** fix the confirmed host shared-QMP transaction ownership defect, then execute **exactly ONE bounded fresh desktop observed replacement**. No headless rerun, blanket retries, deadline increase or pause-excluded clock. Previous S10-4.11 partial results and all earlier diagnostic/failure bytes, executable sources and manifests remain exact.

**Minimal host-only fix:** fork wrapper/evidence under `runtime/observer-qmp-serialized/`; do not edit retained helpers or product sources. Install one shared per-actual-`machine.qmp` `threading.RLock` before observer/GUI use. Wrap existing instance `cmd(name, **args)` over its entire request-through-return/error transaction so GUI keys/buttons/screendumps use the same lock. A new TraceControl.query override holds that identical lock around the unchanged direct-query body. Preserve the existing API and trace QMP argument`name`; no call through conflicting`cmd(name, **args)`, packet-policy change, per-readline-only lock, independent lock or lock held across GUI waits/snapshot queues.

**Verification then exact run:** deterministic two-owner request/reply interleaving and asynchronous-event ownership, trace`name` preservation, QMP-error/EOF/timeout unlock, no snapshot-wait deadlock and existing recurrence/proof/census/one-extra/deadline tests, once after edits. Only after pass, one fresh desktop runs the original S10-4.4/S10-4.11 complete observed schedule under unchanged900s and all original command limits. Existing recurring conditional ONE extra per independently proved sameTBflag3 zero-retirement event remains; success-only continuation, missing witness/second-no-progress/unexpected stop terminal/no third/drain. A new host assumption failure is an actual STOP, not permission to fix-and-rerun.

**Invariants/report:** unchanged candidate/fixtures/GUI oracle; no guest writes/Z0/instrumentation/product/candidate/B2 rebuild, coverage/proof relaxation, untraced redo, commit or push. Preserve546protected canonical/candidate/userVMX paths plus all four earlier observer archives/manifests/compressed trace; remove only owned disposable clone. Record complete workload and complete event coverage separately from partial event maxima/stack-page bounds/mapped-not-RSS snapshots. Historical comparator/exact stack/kernel-stack/RSS and CBX27/criterion4/S10-5/canonical/containment remain unavailable/blocked absent independent evidence. Actual results/deviations and CP2 gate follow the one attempt.

##### Actual serialized replacement result and next gate (2026-10-29)

**Host fix/verification:** new `runtime/observer-qmp-serialized/serialized.py` installs one actual-QMP`RLock`; GUI's unchanged`cmd(name, **args)` and new TraceControl.query override hold that **same lock for the entire transaction**, preserving direct trace`name` arguments and inherited policy/proof behavior. No retained/product sources edited. **34/34 host tests PASS once after behavior edits, before execution:**7new tests cover both-owner interleaving/asynchronous-event ownership, exact API/trace-name behavior, error/EOF/timeout unlock, reentrant/single-install and snapshot-wait safety;27existing recurrence/proof/coverage/one-extra/deadline tests remain passing.

**Exactly ONE fresh desktop attempt: `PARTIAL_DESKTOP_OBSERVED_RESULT`, STOP.** Real GUI launch/initial prompt and ordinary idle10s complete. During finite-command typing, existing input raises **`StepFailed: QMP input-send-event: {'class': 'GenericError', 'desc': 'VM not running'}`**. This is a valid QMP rejection, not JSON corruption. **583 recorded observer QMP transactions** (83CPU,83status,253trace-state reads,164trace-state sets) all have complete correctly typed responses, no ownership mismatch or pending request. **41 actual native packets/scopes, all censuses equal, zero same-PC events/additional steps**; no claim that recurring recovery was exercised by this run. Observer/owned-QEMU cleanup is clean. No second attempt, headless run, budget increase or input replay.

**Distinct host admission assumption:** a transport mutex prevents concurrent readers but does not ensure the VM is running when GUI input is submitted; an all-stop hardware observer may have stopped it. The earlier assumption that every GUI key batch is immediately admissible is exposed only after fixing reply ownership. Upstream version-pinned [`qmp_input_send_event` in QEMU8.2.2](https://raw.githubusercontent.com/qemu/qemu/v8.2.2/ui/input.c) returns `"VM not running"` before handler validation/event dispatch when neither running nor suspended. Installed error/binary identity are retained, but Ubuntu-patched`ui/input.c` was not independently reconstructed and raw failed GUI batch/reply bytes were not newly logged. Do not infer exact batch delivery or replay a command prefix. This is a **host run-state/input-admission finding, not a product defect**.

**Actual bounds, separate from prior attempts:** desktop background5post-events/6guards, retained maximum25,272B,4stack-write-attempt pages/[12,289,16,384]B; foreground19/20, retained maximum34,048B (last34,016B),9pages/[32,769,36,864]B. These are **partial event maxima**, not full-workload peaks. Both usable stacks65,536B; two foreground prompt/idle PTE snapshots map10,199,040B ELF+user-stack backing each, not RSS/residency/quota/whole-cell memory. No finite workload completion, complete event coverage, later phases or exact stack/kernel-stack/historical-baseline result follows.

**Evidence/preservation:** new sibling contains exact wrapper/tests,34-test output, pre-result approved protocol/hash, lock-installation record, raw desktop serial/screens/RSP/QMP/TB/XML/heap/coverage/PTE, generated summary/interpretation and `host-input-admission-finding.json`. **546/546 protected paths unchanged incl469canonical+candidate+userVMX; all4prior observer archives/executable sources/manifests and old compressed trace byte-exact; executing sources unchanged.** Only owned clone removed. No guest writes/Z0/instrumentation/product/B2 rebuild/untraced redo/coverage relaxation/commit/push.

**CP2 / new decision required:** accept this real partial STOP and tested serialization fix without declaring desktop completion or closing CBX27/criterion4/S10-5/canonical/containment. A **separate input-admission CP1** would need deterministic running/debug/stop-race/exact-batch/modifier-order/deadline tests before any separately bounded fresh desktop replacement. A status precheck alone races asynchronous hardware stops even with the QMP mutex held. If considering deferred delivery, first establish installed-source pre-dispatch non-delivery for this exact rejection; preserve the exact not-yet-accepted batch, never repeat an accepted prefix or retry ambiguous transport/EOF/timeout/other errors. Use only original ordinary observer resume opportunities—no extra guest step/continue solely to deliver input, coverage reduction, timeout increase or alternate input route. **No such fix, deferral/retry or replacement is authorized/implemented here; no headless rerun.** Previous headless deadline finding and all unknown metric/baseline ceilings remain unchanged.

#### S10-4.13 Source-backed coordinated GUI input admission and ONE desktop observed attempt (**CP1 approved2026-10-29;79hosttests PASS; one attempt STOP before GUI at original900s window; CP2 pending**)

**Approval record (user, relayed by Main2026-10-29):** implement the single combined protocol retained in `runtime/observer-input-admission-analysis/next-protocol.json`, then exactly **ONE bounded fresh desktop original observed attempt**, not a serial fix/retry loop. Exact installed Ubuntu QEMU source prerequisite is now resolved: package`1:8.2.2+ds-0ubuntu1.17`, installed file checksum match, published DSC/orig/Debian hashes,128active-patch census with no changes to input/dispatch/main-loop files. `ui/input.c145–147` returns the exact VM-not-running error before any events from **that batch** dispatch; prior accepted command batches are not undone. Historical failed batch remains absent/unreplayable; future batches must be frozen/journaled before first send.

**One host-only state machine:** new sibling `runtime/observer-input-coordinated/`, retained archives/sources untouched. SAME per-QMP`RLock` covers complete observer paused/native/proof/restoration critical sections and full QMP transactions; phase/condition coordination permits input only on original ordinary-run opportunities. GUI waiting releases the lock completely; no lock over ordinary `wait_stop`, GUI/snapshot condition waits or joins. Only the observer's original resume callback runs; no extra`c`/`s`/QMPcont or alternate input route. Running/RESUME/status prechecks are readiness hints, **not atomic acceptance**; the source-backed matching target response is authoritative.

**Exact batch/deadline contract:** freeze/journal original complete GUI event array/routing/raw bytes, modifier/down-up order, batch/attempt/epoch/time and every structured response. A success commits that batch once. Defer/resubmit the **identical unaccepted batch only** on exact structured GenericError`VM not running`, with source/health/owned-stop proof; never replay accepted prefix, formatted-error-string guesses or ambiguous EOF/timeout/malformed/other errors. One pending batch, **max64attempts including first within ONE minimum(original5s QMP-call budget, remaining caller budget, original900s window)** established before lock acquisition; all waiting/readiness queries/attempts consume it, no reset. Failure/exhaustion is terminal, no blanket retry or subsequent candidate attempt.

**Verification/run/preservation:** deterministic race/exactly-once/batch-order/phase-lock/deadline/deadlock/error tests plus retained source/proof/census/conditional-one-extra tests run once after edits. Then one fresh desktop exact original launch/idle/finite/collected-cat/live-no-cat/cancel/rg/top/complete schedule with unchanged limits. Existing independently proved sameTBflag3/healthy trace/stable context/lifecycle/full coverage/restoration admission and maxONE additional native step per event remain intact. No headless/rebuild/productsource/guestwrites/Z0/instrumentation/guarddrop/budget increase. Preserve546canonical/candidate/userVMX paths plus five prior observer archives and source-analysis evidence/manifests; archive exact GUI journal, raw metric/coverage evidence, actual completion or STOP and generated bounds. Full-memory/equivalent-feature baseline/exactstack/RSS/CBX27/criterion4/S10-5/canonical/containment do not automatically close. No commit/push.

##### Actual coordinated attempt: pre-GUI STOP and retained stop-frame evidence (2026-10-29)

**Implementation/host verification:** separate `admission.py` plus `coordinated.py` implements the approved shared paused/native/proof/restoration/QMP gate, original-resume-only phase changes, frozen exact batch/structured raw journal, success-commit-once/rejection-only deferral and single5s/caller/900 deadline/max64. **79/79 synthetic host tests PASS once after all behavior edits and before execution:**38new gate tests +7observer/Surface integration tests +34retained transport/recurrence/proof/coverage tests. They cover state races, immutable compound key/mouse/Ctrl-C ordering, accepted-prefix exclusion, ambiguous outcome stops, deadline/cap/lock contention, recursive condition release/snapshot/waitstop safety, benign QMP events and original control-count invariants. This is host behavior evidence, not observed workload completion.

**Exactly ONE fresh desktop actual result:** `PARTIAL_DESKTOP_OBSERVED_RESULT`, **`Stop: 900s measurement window expired` while waiting for `[desktop] Ready. Entering event loop.`** No readiness marker, Terminal launch, GUI input or workload phase was reached. **14native packets/scopes all advance with matching execute/watch censuses;15ordinary RSP`c` packets match15natural-resume epochs;0same-PC/extra operations.** Last successful scope14 is desktop module entry`0x10c000000→0x10c000004`. **205observer QMP transactions** (29CPU/29status/91trace-state reads/56sets) complete with correct response types/no mismatched or pending reply. Subsequent owned shutdown interrupt reports **`RspTimeout: RSP receive timed out`**; QEMU cleanup succeeds. Original900s readiness window was not extended; roughly5s afterward is bounded cleanup, not workload continuation. No rerun, source repair or additional execution.

**Runtime admission/metrics explicitly NOT EXERCISED:** **0GUI batches,0input attempts/commits,0verified Cellbox instances,0post-events,0snapshots**. Batch ledger is`[]`; no GUI journal is created because no input occurred. Empty-set audit truths are labeled `NOT_EXERCISED_PRE_GUI_STOP`, **not runtime success or zero memory usage**. No new aligned allocator maximum, stack footprint, mapped backing, full-workload peak, exact stack/kernel-stack/RSS or baseline value exists. Prior partial measurements remain separate and unchanged.

**Retained-only source-grounded new finding — PROBABLE host RSP receive/ACK deadline loss, not a product boot-hang attribution:** at **421442766530027ns,22.668266244s after VM start**, raw debugger RX contains the complete valid **`$T05thread:01;#07`**, checksum07 verified. There is **no subsequent positive ACK TX, parsed`packet_rx` or stopped-state record**; only cleanup raw interrupt occurs **877.362516731s later**. Serial had reached desktop initialization/display confirmation and Cellbox loader mappings, but no Cellbox entry was fingerprint-verified. The unprocessed bare stop has **no retained RIP**, so do not assign it a Cellbox PC.

Exact executing `runtime/rsp.py180–207` removes a complete frame from persistent buffer at200 **before** ACK send204/parsed-event record206; `_send130–139` checks the same deadline at132 **before** ACK TX logging/send. An ordinary0.1s poll can therefore consume a valid received frame then expire before ACK; `Observer.run163–167` catches any`RspTimeout` as empty polling, leaving the lost stop unprocessed. The raw discrepancy matches that path. **Exact poll deadline/first swallowed exception was not logged, and no new reproduction was run**; confidence remains PROBABLE (wire evidence + source ordering), not a fabricated confirmed exception trace. Coordinated input cannot be the failed GUI operation because none occurred; no transport code was changed after this STOP.

**Evidence/preservation:** new sibling retains exact helpers/tests,79-test result, approval/source-binding records, raw boot/RSP/QMP/TB/XML, empty batch ledger, generated interpretation with explicit exercise ceiling, and `retained-stop-framing-finding.json`. **546/546 protected incl469canonical+candidate+userVMX unchanged; all5prior observer archives plus exact-source analysis/executable sources/manifests and old compressed trace byte-exact; executing host sources unchanged.** Only owned disposable disk removed. No headless/product/B2 rebuild/guestwrites/Z0/guarddrop/timeoutincrease/commit/push.

**CP2 / next gate:** report/accept only this actual pre-GUI STOP and79-test implementation evidence; no runtime admission or metric/qualification closure. A separate CP1 is needed for a host-only **receive-frame/ACK poll-atomicity correction**, preserving complete frames across short receive-poll expiry and distinguishing benign empty receive timeout from uncertain ACK-send failure. Deterministic full/fragmented/coalesced-frame-at-deadline, checksum/malformed/ACK-uncertainty/no-duplicate-delivery tests plus all current gate/proof/control-count tests must precede any separately approved single fresh desktop. No extra guest`c`/`s`, reply repair/replay on this terminated VM, timeout increase, guard relaxation or speculative retry. **No such fix/run is authorized or implemented here.** Prior headless deadline, missing equivalent-feature comparator and CBX27/criterion4/S10-5/canonical/containment remain blocked.

#### S10-4.14 Deterministic RSP poll/ACK atomicity diagnosis, gated host fix and ONE desktop (**CP1 approved2026-10-29; cause reproduced/fixed;144hosttests PASS; one desktop finite-screen STOP; CP2 pending**)

**Approval record (user, relayed by Main2026-10-29):** FIRST reproduce the exact complete-frame deletion/ACK deadline gap deterministically with the unchanged host RSP implementation and fake byte transport/clock. Only a confirmed causal reproduction authorizes the host fix; if not confirmed, investigate reachable retained evidence without speculative edits. Retain actual failure-before/pass-after outputs separately. No generic candidate repro or extra guest control is part of this diagnostic.

**Authorized correction after confirmation only:** new sibling `runtime/observer-rsp-atomic/`, leaving all previous executable sources/archives/manifests exact. Preserve complete received frames across benign receive-poll expiry, with explicit pending decode/ACK/dispatch state; a successfully acknowledged frame is delivered once, no duplicate ACK/dispatch or silent stop loss. Fail closed on ambiguous ACK send, partial write, malformed/checksum/EOF/transport failures; do not treat uncertain send as an empty receive poll. No deadline increase, background retry or added candidate controls.

**Proof then exactly one now-approved desktop:** after deterministic reproduction and correction, run the full/fragmented/coalesced frame/deadline/checksum/no-duplicate/EOF/ambiguous-send behavioral matrix plus retained79/regression tests once after implementation. Only after that proof, exactly **ONE fresh desktop original observed replacement** under unchanged workload/deadlines. S10-4.13 input coordination/immutable journal/exact-zero-dispatch rejection-only64attempts within SAME5s/caller/900 budget remains; source-proved sameTBflag3/stable context/healthy trace/lifecycle/intact coverage/one extra native step per event remains unchanged. No headless/product build/candidate modification/guest memory-register write/Z0/coverage waiver or speculative rerun.

**Evidence/gate:** preserve546canonical/candidate/userVMX plus six prior observer runtime archives and source-analysis/manifests/compressed trace. Record deterministic cause confirmation, before failure/after pass, exact corrected host sources and raw single-desktop input/RSP/QMP/metric/coverage results. New blockers STOP with retained-only analysis. Runtime-exercised workload/metrics must be distinguished from unavailable/vacuous results; no automatic historical comparator/exactstack/RSS/CBX27/criterion4/S10-5/canonical/containment closure. No commit/push.

##### Actual deterministic proof and exercised desktop result (2026-10-29)

**Cause confirmed before fix:** unchanged `rsp.py` plus deterministic fake byte transport delivers exact checksum-valid`$T05thread:01;#07` at the original0.1s poll deadline. **Before regression exits1 as expected:** first poll raises deadline-expired with buffer already empty/ACK0; second original0.1s poll times out, stop never dispatched. This eliminates missing/malformed input as an alternative for the synthetic case; **CONFIRMED causal mechanism**, while the historical S10-4.13 first swallowed exception remains inferred from its matching retained signature. Only after this proof was new`atomic_rsp.py` authored.

**Correction/after proof:** explicit validated decoded pending-frame state and original wire bytes remain across a benign pre-ACK deadline expiry. On next original poll, successful ACK is followed by one consume/dispatch without another deadline check; attempted ACK failure/uncertain partial delivery is poisoned terminal`RspError`, never swallowed as empty receive polling or resent. **Same fixture after correction PASS:** first buffer retains exact full frame/ACK0; second poll delivers stopped event with **one total receive,one ACK,one dispatch**, same0.1s deadlines/no guest controls. **144/144 hosttests PASS once after implementation:**20boundary +43unchanged RSP regressions instantiated on fixed class +2exact-cause/actual-client inheritance checks +79retained coordination/proof tests.

**Exactly ONE fresh desktop attempt, `PARTIAL_DESKTOP_OBSERVED_RESULT`:** real desktop/Terminal prompt and ordinary idle10s complete; **`PASS finite-stream-typed-line-exact`** proves original command text pixels before Enter. All **55original GUI batches commit once**,55attempts,98key/8absolute-mouse/4button events;2batches freeze while gate paused then transmit only under ordinary-run phase. No exact-rejection/retry occurs in this attempt, so runtime success/phase coordination is exercised but rejection deferral remains host-test evidence. Last batch55 is original Enter down/up, accepted once. **2065natural-resume epochs=2065original`c` packets**; no input-induced extra guest execution.

**RSP/proof/coverage actually exercised:** **54,880validated frames=54,880positive ACK attempts=54,880parsed dispatches**, no pending frame/terminal wire error at cleanup; **28,657observer QMP transactions** correctly typed/complete, no mismatch/pending reply. **2046native packets/scopes**,2independent first-attempt same-PC events, each source-exact ordered sameTBflag3 proof recomputes and admits exactlyONE additional native step: desktop post-add`0x1180e30c1→0x1180e30c5` and post-sub`0x1180e2f16→0x1180e2f17`. Both advance/bareT05/restored; all recorded full execute/watch censuses equal. No third/drain/guard relaxation.

**Actual STOP and retained ceiling:** original **120s finite-stream read-screen predicate expires**, clean observer/QEMU cleanup, no rerun/deadline increase. No finite exitstatus/two-frame-completion/new-prompt acceptance or later collected-cat/live/rg/top/complete phase follows. Retained foreground allocator events continue through finite phase, but allocator progress is **not an output/completion proxy**. `at_fresh_prompt1080–1083` requires both exact prompt/cursor and typed command echo still visible; long collected JSON may scroll the echo away, a **possible host oracle false-negative**, not proof of the missing actual final screen. `CheckedDesk.wait_for` raises`common.Stop`, whereas`read_until` catches different`StepFailed`; last local read is not serialized, and`grab` deletes temporary PPM. No timeout screenshot survives. Therefore retained data cannot distinguish still-running command, absent/unreadable/scrolled prompt, predicate failure or observer timing; **do not claim product failure or finite success**.

| Actual retained instance interval (**not full workload peak**) | Post-events /write guards | Maximum aligned bytes | CPU stack write-attempt pages /depth bound B |
|---|---:|---:|---|
| Desktop background | 5 /6 | 25,272 | 4 /[12,289,16,384] |
| Desktop foreground | 2022 /2023 | 67,568 (last41,880) | 12 /[45,057,49,152] |

Usable stack reservation65,536B each; two foreground prompt/idle snapshots each map10,199,040B selected ELF+user-stack backing, **not RSS/residency/quota/whole-cell memory**. Exact stack/kernel-stack peaks/full-workload allocator peak/historical matched runtime remain unavailable. No recorded draw-entry event is not a zero-redraw claim.

**Evidence/preservation:** new`runtime/observer-rsp-atomic/` includes deterministic before-exit1/raw and after-pass/raw,`deterministic-cause-proof.json`,144-test output, exact new host sources,55batch raw journal/ledger, all profile RSP/QMP/TB/context/coverage/heap/PTE/screens, generated interpretation and`retained-finite-screen-finding.json`. **546/546protected incl469canonical+candidate+userVMX unchanged; all6prior runtime archives plus exact-source analysis/executable sources/manifests and old compressed trace byte-exact; executing sources unchanged.** Owned clone removed; no headless/product/B2 build/guest memory-register writes/Z0/commit/push.

**CP2 / exact next gate:** accept actual confirmed host transport fix, runtime admission/recovery evidence and **partial** bounds/finite-screen STOP only. Any future finite GUI oracle work needs separate CP1: first deterministic renderer/transcript proof for long-output scroll/prompt behavior, preserve exact two-frame/status/new-prompt contract without requiring scrolled-off echo, and retain last screenshot/parsed read on every timeout type; original120s/global900s and all observer/input/proof guards stay intact. Historical missing final screen cannot be synthesized. No predicate fix/new desktop/headless run is implemented or authorized here. CBX27/criterion4/S10-5/canonical/containment and absent baseline/full metrics remain blocked.

#### S10-4.15 Temporal GUI output proof, all-path timeout evidence and ONE desktop (**CP1 approved2026-10-29;191distinct offlinePASSaggregate; one desktop observability STOP with retained finalscreen; partial-result CP2 pending**)

**Approval record (user, relayed by Main2026-10-29):** implement the consolidated `runtime/observer-finite-oracle-analysis/next-protocol.json` only, then one fresh desktop original observed replacement after offline proof/tests. Offline retained-renderer counterexample now **CONFIRMS synthetic false-negative**:20valid headless-fixture JSON records/two complete frames require62wrapped rows on79×26; old exact reader accepts fresh-prompt/status0 screens but original finalecho predicate rejects. This is not the missing historical desktop completion; frozen journal proves original command/Enter only, no status command/finalscreen.

**Temporal acceptance, no weakened contract:** new sibling `runtime/observer-gui-temporal/`, previous sources immutable. Verify/store exact typed-line pixels BEFORE Enter, arm epoch only after original single committed Enter batch, retain ordered exact GUI samples/reads and source-pinned wrap/scroll provenance. Require actual two distinct complete JSON frames/rows/end markers followed by new exact prompt/cursor, then original status0/newprompt in its own phase. Drop ONLY continued final-echo presence; reject stale old prompt, missing/ambiguous ledger, incomplete/duplicate/missed output, wrong command/cursor/unreadable proof. No inference from requested`--frames2`, allocator/native progress, elapsed time or status alone.

**Observability gate:**62rows>26visible, sparse samples may miss output. Offline complete/scroll/duplicate/drop/coalesced fixtures must prove **no false positive**, with missed/ambiguous output terminal`observability-incomplete`, not fabricated/reduced two-frame acceptance. If full original observation cannot be established without expanding scope, report that prerequisite, not a knowingly weakened oracle. Retain last framebuffer/rawread/cursor/epoch/ledger/reasons on ANY timeout/observer/input exception before cleanup, rethrow original failure; no extra guest action to obtain evidence.

**Verification then one bounded run:** new deterministic temporal/real-renderer/timeout-retention tests plus all144existing regressions run once after edits. Then exactlyONE fresh desktop original launch/idle/finite/collectedcat/live-no-cat/cancel/rg/top/complete schedule, unchanged120sfinite/60sstatus/900sglobal and original command limits. RSPatomic/QMPcommonlock/inputexactjournal/rejection-only64withinSAME5s/caller/900 and positive flags3/context/lifecycle/fullcoverage/one-extra-per-event policies remain. No headless/rebuild/candidatewrite/hiddenoutputroute/syntheticstatus/inputrewrite/newcontrols/guarddrop/deadlineincrease.

**Preservation/result ceiling:**546canonical/candidate/userVMX plus all7prior observer runtimes and both source/oracle analyses/executable sources/manifests/compressed trace exact. Archive actual phases/completeeventcoverage or STOP/partialbounds and raw timeout evidence; historical missingfinalscreen/comparator/exactstack/RSS/fullcriterion closure never synthesized. CBX27/criterion4/S10-5/canonical/containment independent/blocked; actual results/deviations/CP2 follow, no commit/push.

**Pre-run exercised offline gate/deviation2026-10-29:** `observer-gui-temporal/verification-summary.json` records **191distinct PASS aggregate=retained144 once+current47new**, not a single clean191-test invocation. Initial new-host import owner error ran0tests; first187suite retained144PASS but new1FAIL/8ERROR; correctednew45suite2ERROR; finalnew46suite1ERROR; three affected dynamicfixtures thenPASS. All raw failures/corrections retained. Stronger complete nonblank stable-row overlap replaces insufficient prefix-only shift25; only exact source-pinned Unicode banner is opaque history; offline renderer Desk.model setup corrected. Two real ambiguous fixtures `[1,22]` repeatedmetadata and `[0,24]` copiedrowbody are retained as **expected rejection**, not forced acceptance; distinctper-cell metrics preserve3→11dynamicrows/longmetadata success. Actual qualified-renderer ordered62row/echooffscreen→freshprompt→ownstatus0 succeeds, dropped/coalesced failclosed. No candidate run yet at this record; original observer/input/time guards unchanged. Proceed only with the one already-approved fresh desktop; any missedactualoutput remains STOP, not inferred closure.

##### Actual single-desktop result and retained-only observability ceiling (2026-10-29)

**Exactly ONE desktop, `PARTIAL_DESKTOP_OBSERVED_RESULT`, STOP; no retry/headless.** Real launch/initial prompt and ordinary idle10s/100samples complete. Original finite command exact typed-line pixels PASS, original Enter batch55 committedonce; temporal epoch arms only after that receipt. **20ordered exact GUI samples retained**, including all PNG/rawreads:0–2 show original typed line,3–18 show submitted newline/blank output,19 jumps to a26row partial JSON view. The last view contains tail of an earlier row/end marker, then sample3 begin (`meta.total_cells13`) and partial payload, cursor`(19,25)`; no initial complete firstframe, newprompt or status proof. The last interval is1.604456358s; no permissible overlap survives. Ledger returns terminal **`oracle-observability-incomplete: missing physical-row coverage or non-append screen change`** at~31.153s within original120s, **not an expired deadline or product failure**. Zero ledger records accepted, no`echo st=$?`/laterworkload sent. Collectedcat/live-no-cat/cancel/rg/top/complete remain unrun.

**Failure evidence is now actual, not synthesized:** `desktop/evidence/gui-failure-01.png` SHA256`530fb50c51e63207a9c6748133445646dc5e3aef87f4d6a2e377d2e9dc6ff087` plus rawJSON/epoch/reasons and20image/read samples. Retention uses the last already-observed framebuffer, no extra grab/resume/input; original exception rethrown before clean owned observer/QEMU shutdown. Runtime exercises the general failure path; common.Stop/StepFailed/observer/input-timeout variants are **offline-tested only**, not claimed as actual timeout outcomes. S10-4.14's historical missing finalscreen remains unknown.

**Actual transport/conditional-recovery evidence:**55GUIbatches allcommitonce;56attempts include1source-proven zero-dispatch rejection/deferral,6batches frozen while debuggerpaused, key98/abs8/btn4; all requests/ownership/frozenbytes/SAME5s/caller/900/max64 checks true, no resendaftercommit. Naturalresumeepochs2065=original`c` packets. **54938validated RSPframes=ACKattempts=decodeddispatch,0pending/terminalwireerrors**;28689observerQMPtransactions typed/owned correctly. **2048native scopes include4independently proved extras**, all`ADDITIONAL_ADVANCED_CONTINUE`: first194/859 add`0x1180e30c1`→extra195/860`...c5`, first1293/1769 sub`0x1180e2f16`→extra1294/1770`...17`. All sameTBflags3/context/lifecycle/execute+watch census/restoration proofs recomputed true; no unproved control afterterminal or forbiddenpacket.

| Retained interval only | Postevents / counterwrite guards | Maximum aligned allocator B (**NOT full peak**) | Last value B | Stack write-attempt pages / depth bound B |
|---|---:|---:|---:|---|
| Autoload/background | 5 / 6 | 25272 | 25272 | 4 / [12289,16384] |
| Foreground Terminal shell | 2022 / 2023 | 67544 | 41872 | 12 / [45057,49152] |

Both initialprompt/ordinaryidle snapshots retain10199040B guest present segment+userstack backing (**notRSS/residency/wholecell/quota/kernelstack**). Exact stack/RSP/kernelstack/fullworkload peaks and historical comparator remainnull. Observed142.329740407s hostinterval includes setup/cleanup and19.069186382s completed all-stop intervals, not startup/performance equivalence; no pausedtime subtraction or deadline increase.

**Retained-only source-grounded next gate:** Terminal`session.rs41-42,182-191,275-295` permits32×512B perstream to be fed into grid before pump returns; `main.rs342-354` renders afterpump, not perrow. Bounded grid scrollback exists, but no GUI`scrollback_line` caller was found; PageUp/Down forward tosession. Therefore faster screenshots alone cannot **guarantee** capture of intermediate rows neverpresented; this source capability is not a reconstruction of actual pump boundaries. Actualdesktop totaloutput/firstframe metadata are unknown; offline62row fixture is not substituted for actual13cell sample. `presentation-observability-finding.json` and `next-protocol.json` preserve exactfacts/limits.

**Preservation/CP2:**546/546 protected paths including469canonical/candidate/userVMX, all9prior observer/source/oracle archive trees and nestedmanifests, oldcompressedtrace and executinghostsources unchanged. New sibling/raw failures/offline proofs/runtime/interpretation/new+parent manifests retained; owned diskclone removed only, no product/candidate edits/build/commit/push. Accept actual conservative observation/failure-retention/healthytransport/recovery and **partial bounds only**. Next choice: keep limited CP2/blocked qualification, or separately approve **retained-only** feasibility of an explicit complete-output route (candidate-bound visible route, or explicitly authorized readonly Terminal grid/scrollback/object snapshot/newviewport diagnostic). No new guestroute, resize/scrollback input, faster-sampling rerun, candidate execution, hiddenoutput, proxy, guarddrop or budgetincrease authorized here. CBX27/criterion4/S10-5/canonical/containment remain blocked.

#### S10-4.16 Candidate-bound complete-output route investigation (**CP1 approved by user2026-10-29; RETAINED-ONLY/read-only; no observer implementation/tests/new candidate execution**)

**Approved boundary (user, relayed by Main2026-10-29):** inspect exact qualified Terminal/cellbox/kernel ELF symbols/assembly/source, retained buffers and stream ownership, existing debug observations and possible complete read-only stream interception. Establish commandepoch+Cell/session/operation identity, byte ordering/loss detection, producer-vs-frontend semantics, coverage/breakpoint burden and privacy/security. Prefer reuse of existing host observer interfaces, not broad debugger architecture. Source/ELF/retained-data inspection only: no product/candidate patch/rebuild, guestwrites/newinputroute/control/VMrun, fixes/tests/builds/commit/push.

**Deliverable/next gate:** evidence-backed route alternatives and concrete recommended next CP1 with exact interfaces/symbol addresses/version binding, order/epoch/status proof, host test matrix and bounded replacement prerequisites; implementation or execution needs separate protocol approval. Preserve all prior archives/sources/canonical/candidate/userVMX and their nestedmanifests. S10-4.15 missing rows/completion remain unrecoverable unless actual retained bytes prove otherwise; no artificial output/status/proxy inference or qualification/containment closure.

**S10-4.16 outcome (recorded 2026-10-29):** the retained-only investigation's artifacts are in `runtime/observer-output-route-analysis/` (ELF binding, Terminal/cellbox/kernel assembly, symbol dumps, `retained-read-coverage.json`, receipts). The agent that ran it failed (provider) before writing findings into this file; its recommended next gate is consolidated, with the evidence it produced plus a few extra read-only static checks, in S10-4.17 below. S10-4.16 itself implemented and ran nothing.

#### S10-4.17 Terminal `ByteSink::feed` observation hook: ONE complete protocol, offline gate and ONE desktop run (**CP1 APPROVED by user 2026-10-29 exactly as written; EXECUTED: 269-test single clean offline gate, ONE desktop run, Class B SUCCESS_METRIC_ONLY; limited-result CP2 APPROVED by user 2026-10-29; S10-4 CLOSED PARTIAL**)

**Approval record (user, relayed by Main 2026-10-29):** CP1 approved exactly as written below. Execution order, and no other: (1) implement only under `runtime/observer-feed-hook/`, T1 first (re-derive and archive static Terminal ELF checks, confirm or withdraw the stdout/stderr mapping); (2) offline gate T1-T11 plus the retained 144 and 47 tests in ONE clean invocation, zero failures/errors/skips, frozen source hashes; fix offline and rerun offline only, every offline iteration recorded honestly, no VM before one clean pass; (3) go/no-go against section 5, then exactly ONE desktop attempt with original deadlines and `RUN_CONSUMED`, no headless run, no new controls; (4) classify A–E, NO fix-and-rerun for any outcome including host failure, close S10-4 partial; (5) update the four docs, manifests and the 546/canonical/candidate/userVMX/prior-archive preservation proof. No commit/push. CBX-27/criterion 4 stay unmet unless the evidence really closes them.

**What the user is asked:** approve or decline this single CP1. Approval covers, in order: (1) implement the host-only protocol below in ONE new sibling directory; (2) run the complete offline gate in section 4 (only after it passes, section 5 go criteria); (3) at most ONE fresh desktop run. There is no follow-up fix-and-rerun authority: any outcome, including failure, is final (section 6). No questions are open (section 8).

##### 1. Purpose and honest value

**Exactly what it completes.** The S10-4.15 desktop run stopped because the host could only sample 26-row GUI frames while the original two-frame command wrote far more than 26 rows between samples. The hook adds a second, *frontend-side* witness: every byte the Terminal process hands to its VT parser (`ByteSink::feed`), in order, from process start. From that byte ledger, and only from it, the oracle may prove for the original commands: (a) `top --records -d 1 --frames 2 | to jsonl | cat` produced exactly two complete JSON frames (begin, rows counted against `meta.total_cells`, end with `complete:true, dropped:0`), then the fresh prompt `Cellos > `; (b) the separate original `echo st=$?` produced `st=0` then a new prompt. Nothing else is claimed.

**What it does NOT do (value is limited, stated plainly):**

- It does not read or reveal any memory metric. RSS/residency, exact stack peak, kernel-stack peak, a matched historical runtime comparator and the B2 equivalence stay `null` whatever the result; they depend on prerequisites unrelated to output visibility. **CBX-27 and Phase 10 criterion 4 stay unmet even on full success.**
- Its only downstream effect on memory evidence is indirect: if the oracle passes, the original observed desktop schedule can continue past the finite epoch, so the allocator aligned-peak, the stack write-attempt page-depth bound and the mapped-backing snapshots *might* cover the whole original workload instead of two partial intervals. They would still be bounds with the documented scopes, not exact peaks. The tail phases (collected-cat, live stream, rg cancel, interactive top, complete snapshot) have never run under a desktop observer; they carry their own unretired GUI/overhead risk, so even this indirect gain is not assured.
- It does not prove which shell session/operation produced the bytes (section 3.8 explains the limit), does not cover Terminal `notice()` text written straight into the grid, and does not prove a single `feed` call returned (entry-only hook).
- **Assessment:** completing the finite-stream/status oracle is real but narrow; for CBX-27 closure it is marginal. It is worth approving only if a complete observed desktop workload with (still partial) memory bounds is wanted. Declining it leaves S10-4 closed partial, exactly as it stands.

##### 2. Evidence base and route choice

Retained, read-only (all under `reports/s10-4-evidence/runtime/`): `observer-output-route-analysis/{binary-binding.json,terminal-drain.asm,terminal-feed-entry.asm,terminal-endpoint-read.asm,terminal-poll-*.asm,cellbox-*.asm,kernel-*.asm,retained-read-coverage.json}`, `observer-gui-temporal/{presentation-observability-finding.json,next-protocol.json,desktop/evidence/rsp-traffic.jsonl}`, and the host sources `rsp.py`, `observer.py`, `common.py`, `observer-rsp-atomic/atomic_rsp.py`, `observer-qmp-serialized/serialized.py`, `observer-input-coordinated/{admission,coordinated}.py`, `observer-recurring/recurring.py`, `observer-step-amendment-corrected/amendment.py`, `observer-gui-temporal/{temporal,gui_temporal,run_temporal}.py`.

| Fact | Source | Status |
|---|---|---|
| Terminal ELF sha256 `816718a20188bb797a65622d17087010e8d41c7a933248dc303847c7d11ff3cc`, 950600 B, ET_DYN (PIE), first (R+X) LOAD va `0..0x2f1dc`, so ELF va 0 is the slot base; Cellbox `8ea134442bb27719cf4c2ce5a8f3613f5667f88f1762a1d731cba9fd83ab54a2`; kernel `dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a` | `binary-binding.json`; Terminal re-hashed 2026-10-29 at `/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal` | observed |
| `<terminal::sink::ByteSink>::feed` at ELF `0x1da80` (symbol, `t`), ends before `ground` at `0x1e620`; first instruction `55 push %rbp` | `nm`, `terminal-feed-entry.asm` | observed |
| Exactly FOUR direct callers, all inside `terminal::session::drain::<FrontendHost>` (`0x40..0x283`): `call` at `0x146`, `0x1a0`, `0x1e0`, `0x228` → return addresses `0x14b`, `0x1a5`, `0x1e5`, `0x22d`; no other `call`/`jmp`/relocation refers to `0x1da80` | `terminal-drain.asm`; whole-ELF `objdump` text scan and `readelf -r`, 2026-10-29 transient read-only | observed; **not yet archived** (test T1 must re-derive and retain) |
| Register contract at entry (Rust ABI as compiled here): `RDI`=`&mut ByteSink`, `RSI`=`&mut TerminalGrid`, `RDX`=bytes pointer, `RCX`=length; prologue forms `end = RDX+RCX`, `R14=RDX`, `R12=RDI` | prologue `0x1da91..0x1da9a` | observed |
| At every call site `RDX = drain_RSP+0x18` (stack buffer `[u8;512]` zeroed at `0x5a`) and `RCX` is the just-read length, tested `!=0` and `<=0x200` before the call; with `call` pushing 8 bytes, entry `RDX == RSP+0x20` and `1 <= RCX <= 0x200` | `terminal-drain.asm` `0x126..0x146`, `0x185..0x1a0`, `0x1af..0x1e0`, `0x20d..0x228` | observed |
| Source order: stdout drained first (first call + loop of 31 = `DRAIN_FRAMES` 32), then stderr (same). Sites `0x14b`,`0x1a5` use endpoint `host+0xc8` and `0x1e5`,`0x22d` use `host+0xe8` | `session.rs:42,275-296`; `terminal-drain.asm` | endpoint-to-name mapping **[INFERENCE]** from loop order; test T1 must confirm or the stream tag is reported only as "first/second endpoint" |
| One `ret` only, at `0x1e5f3` (loop exit `0x1e5e2`); calls to `core::panicking::panic_bounds_check` at `0x1e600`,`0x1e60c` | transient `objdump` of `0x1da80..0x1e620` | observed; not yet archived |
| Terminal slot entry was observed in S10-4.15 (scope ordinal 21): base `0x116000000`, CR3 `0x59000`, CS 35, first 48 text bytes read back equal the Terminal ELF's (they differ from Cellbox's), future hook PC would be `0x11601da80`; the old observer correctly ignored it as non-Cellbox | `retained-read-coverage.json` | observed |
| No hook was installed; no Terminal buffer/grid/scrollback page was ever read; UART has no frontend transcript; no live memory remains | same | observed |
| `main.rs:342-354` renders once after `pump()`; one pump can fold up to 32x512 B per stream into the grid before any paint; no GUI scrollback viewer | `presentation-observability-finding.json` | observed (source); actual pump boundaries unknown |
| Shell runs in merged-streams mode (`CELLBOX_STREAMS=merged`), stderr ring normally empty | `session.rs:29-34` | source |

**Route alternatives considered (S10-4.16 mandate):**

| Route | Verdict |
|---|---|
| Terminal `ByteSink::feed` entry (this CP1) | **Chosen.** One site, frontend-consumed bytes, ordered, stream recoverable from the return address, buffer address independently predictable (`RSP+0x20`), exactly four static callers. |
| Faster/finer GUI sampling | Cannot guarantee rows never presented (S10-4.15 finding). Rejected. |
| Terminal grid/scrollback memory snapshot | No object address/layout/lifetime retained; scrollback is bounded and overwritten; needs a new layout proof. Rejected. |
| Terminal `Endpoint::read` (`0x26e60`, `kernel_call`) return | Fires on every empty poll of every pump (constant overhead) and carries status/length structs. Strictly worse than `feed`. Rejected. |
| Kernel `cell_session::stream_io` / `State::endpoint` / `pipe::session_read` (`0xffffffff80051040`, `…51660`, `…80117200`) | Would give Cell/session identity, but covers every Cell's streams, is kernel-side, and measures the producer path, not what the frontend consumed. Rejected for coverage/overhead; identity limit accepted instead (3.8). |
| Cellbox `terminal_write` (`0x17be0`) / `relay_device` (`0x12120`) | Producer side, not frontend-consumed, interacts with the existing Cellbox watch set. Rejected. |

##### 3. Complete protocol (host-only, read-only observation)

**3.1 Hook address binding.** Hook PC = `T_base + 0x1da80`, where `T_base` is the user entry PC of the Terminal slot (`0x100000000 + slot*32 MiB`, as the existing 512 slot entry breakpoints deliver). Nothing is hard-coded to `0x116000000` or to scope ordinal 21; that was one run's slot. The hook is a hardware `Z1,<pc>,1` through the existing allow-listed client; no `Z0`, no `Z2` on Terminal, no `M`/`X` packet, no guest byte written.

**3.2 Identity gate before arming (all must hold, else terminal STOP `observability-incomplete`, nothing armed).**

1. Candidate identity: the existing `X.CAND_SHA` check; the `terminal` file taken from the candidate inputs (not from `/tmp` hearsay) hashes to `816718a2…cc`. If neither the retained path nor the immutable candidate image can supply it, it is a no-go before the VM.
2. Entry-scope identity: the stop is a slot-entry stop (`is_entry`), phase is `terminal-launch`, `CS&3==3`, this is the FIRST Terminal-fingerprint match of the run, and its 48-byte first-window equals the ELF's `4883e4f0488d1d05a40d004c8d25fea30d004c39e37408ff134883c308ebf3e86cee010048c7c03c0000004831ff0f05` while the Cellbox check (`super()._entry`) declined. A second Terminal entry, slot reuse or a Terminal before `terminal-launch` is a STOP.
3. Text windows (relocation-free, chosen offline from the ELF): entry 48 B; hook prologue 64 B at `0x1da80`; epilogue/ret 16 B around `0x1e5f3`; the four 14-byte call-site windows ending at `0x146`,`0x1a0`,`0x1e0`,`0x228`. All must equal the ELF bytes. Their pages must be present user-text per `walk_page` under the entry CR3.
4. Single vCPU (the existing QMP `query-cpus` witness) and CR3/`initial_rsp`/stack window pinned from this entry (same 64 KiB window rule used for Cellbox).

The Terminal hook state lives in its own `observer.hook` object and is NOT added to `observer.instances` or `entry_ordinal`: the retained `lifecycle()`/`admit()` proof requires `verified_instance_count == len(instances) == len(heap_watches)`, so a heap-less Terminal there would break every later recovery admission.

**3.3 Per-hit procedure** (runs in the observer thread inside the existing `gate.paused()`/all-stop `_stopped`, before the `unattributed execute stop` branch; passive, no writes):

1. Stop reply `T05…` via the unchanged atomic RSP receive; `rip == hook_pc`.
2. Read `rip, rsp, rdi, rsi, rdx, rcx, cr3, cs` with the existing per-register `p` packets.
3. Invariants (any failure is a terminal STOP): `cs&3==3`; `cr3==pinned`; `rsp` inside the pinned stack window; `rdi`/`rsi` equal the values pinned at the FIRST hit; `1 <= rcx <= 0x200`; `rdx == rsp+0x20`.
4. `m rsp,8` → return address `ret`; `ret - T_base` must be one of `{0x14b,0x1a5,0x1e5,0x22d}`.
5. `m rdx,rcx` → payload, accepted only as exactly `rcx` bytes of valid hex (the 4096-char packet size holds 512 B = 1024 hex chars; a read error, short or long reply is a STOP). The buffer is the caller's stack buffer and cannot change while the VM is stopped; the bytes are the ones `feed` will consume, read before its first instruction.
6. Append the record: sequence number, host monotonic ns, stop-frame id, `rsp`, `ret` offset, stream tag, length, sha256, bytes; one JSON line, flushed and `fsync`ed before the step-over (so a later crash/STOP retains it).
7. Resume through the existing path (`_stopped` sees `pc in breaks` → retained native `step_over_breakpoint`, with its trace scope, zero-retirement recovery and census rules). The hook adds a site to those mechanisms; it changes none of them.

**3.4 Return sites and consumption.** The hook is entry-only (one stop per feed call; a second hook on the single `ret` at `0x1e5f3` would double stops and overhead). Consequently: static proof that `feed` has exactly one `ret` and a loop that consumes every byte with no early exit, so bytes handed over are consumed unless the process panics at `0x1e600`/`0x1e60c` (a bounds failure, visible as a dead Terminal/GUI). The completion of the LAST feed is proven by the final-screen cross-check (3.8, F5), not by a return hook. The four return addresses above are used only to tag the stream and to check order (3.6).

**3.5 Stdout vs stderr.** The stream tag is the return-address offset: `{0x14b,0x1a5}` and `{0x1e5,0x22d}`, mapped to stdout/stderr by the offline-confirmed endpoint mapping (T1). The `ByteSink` is shared, so the ledger order IS the frontend consumption order across both streams (stdout burst, then stderr burst, per pump, as `drain` runs). Expected stderr bytes: none (merged mode). Any stderr bytes are recorded, tagged and kept in order; they enter the JSON validator in consumption order and so normally fail it, which is correct.

**3.6 Loss, duplicate and overlap detection** (no single mechanism is claimed sufficient):

- *Stop protocol:* each hit corresponds to exactly one checksum-valid stop frame, ACKed once (atomic RSP, unchanged); the step-over guard guarantees `rip != hook_pc` afterwards, so a stop cannot be recorded twice; `feed` is not recursive and has no jump target at its first instruction (static, T1).
- *Drain grammar:* hit sequences must follow `drain`: `0x14b` starts a stdout burst followed by at most 31 `0x1a5`; then optionally `0x1e5` followed by at most 31 `0x22d`; then a new burst or idle. A `0x1a5` without a stdout predecessor, `0x22d` without a stderr predecessor, or a run longer than 31 proves a lost/duplicated stop. A single lost mid-run hit is NOT provable by grammar alone.
- *Three-way count:* ledger hits == native-step scopes with `addr == hook_pc` == traced TB entries at `hook_pc` that the retained `trace_interval` reports in those scopes. Any difference is a STOP.
- *Payload structure:* a lost, duplicated or reordered chunk must still leave every frame well-formed (begin, rows == `total_cells`, unique cell ids, end `complete:true dropped:0`, no trailing bytes) and the exact prompt/`st=0`; otherwise `observability-incomplete`.
- *Independent frontend cross-check:* the final GUI exact screen must equal the tail of the ledger text under the source-pinned 79-column wrap/LF rule (3.8 F5).
- *Residual:* a loss/duplication that preserves all structure, counts and the screen tail would be undetected. This is judged negligible but not impossible **[INFERENCE]**, and is recorded as a limit of any success.

**3.7 Order, deadlock, overhead.** Lock hierarchy: QMP `RLock` (S10-4.12) > RSP client lock > ledger lock (leaf; held only to append to/copy an in-memory list, never across RSP, QMP, file I/O or GUI calls). The surface thread reads a copy. Per hit the cost is about 8 register reads, 2 memory reads, one JSONL `fsync` and one native-step scope. Crude reference: the S10-4.15 desktop run issued 54938 RSP frames over 2048 native scopes (about 27 frames per scope) in 142.3 s of total wall time including setup and cleanup (an upper-bound average of about 69 ms per scope, not a per-hit measurement), so on that scale roughly 1700 hits would fill the 120 s window. The hit count for banner, the typed commands (46 + 10 characters if echoed one `feed` each) and two frames is expected to be far lower (tens to low hundreds) **[INFERENCE, unmeasured]**; hard caps are 2048 hits and 512 KiB of payload until disarm. All original deadlines stay host wall-clock with no pause subtraction.

**3.8 Command-epoch binding and acceptance oracle (`HookOracle`, pure function over the ledger + gate receipts + GUI reads).** The retained gate supplies the original single committed Enter batch (id, commit receipt ns); the retained `TemporalDesk` supplies the exact pre-Enter typed-line GUI read. For each of the two original epochs (finite, then status):

- **F1** the ledger bytes from the previous prompt up to the epoch cut (the last hit before Enter commit) tokenize, with only printable ASCII and the optional line-redraw token `\r ESC[2K Cellos > ` (`executor.rs:676,698`) allowed, to exactly `Cellos > <typed command>`; no backspace/ESC other than that token is accepted here. The GUI typed row read is the same `Cellos > <command>` (same typed-line pixel gate as S10-4.15).
- **F2** no ledger hit lies in `(typed_exact_grab_ns, enter_commit_ns]`; a hit there is ambiguity, not evidence.
- **F3** post-cut bytes tokenize to: `\n` (Enter echo), then the output, then the final `Cellos > `. The tokenizer is a whitelist, NOT a terminal emulator: printable ASCII, UTF-8, `\n`, `\r`, and the single exact `SOFT_RESET` (`ESC[?1049l ESC[?25h ESC[?2004l ESC[0m`, `screen_reset.rs:22`) immediately before the prompt; any other ESC/control byte is `observability-incomplete`. Finite epoch: the text between the echo and the prompt must pass the retained record rules (`temporal.py::_validate_records`: begin/sample_id uniqueness, row counts, duplicate rows/cells, end flags, exactly two frames, finite JSON). Status epoch: exactly `st=0\n`. An exact-original command and `kind` check is kept.
- **F4** the ledger ends with the prompt, with no later hit for a 3.0 s host quiescence window.
- **F5** GUI cross-check: two consecutive exact reads at least 1.5 s apart are identical, the last row is `Cellos > ` with the cursor at column 9, and the visible rows equal the last rows of the ledger text wrapped by the source-pinned rule of `temporal.py` (79 columns, `\n` ends a row, a full 79-char row followed by `\n` adds no blank row). Intermediate GUI samples continue at the original cadence but are retained evidence only and do NOT gate (the `lost physical row` gate that stopped S10-4.15 is not used for these two epochs). A mismatch that persists to the epoch deadline is `observability-incomplete`.

Identity limit, stated: `feed` exposes no Cell, session or operation id. Attribution rests on (a) the one verified Terminal instance (CR3/base), (b) one interactive shell and one command at a time, (c) the typed-line echo and exact Enter epoch cut, (d) payload self-identification (`sample_id`, two frames, `st=`). It does not rule out other output reaching the same Terminal during the window; it would show up as extra/ill-formed bytes and fail F3. Writes that bypass `feed` (`TerminalSession::notice`, e.g. `[terminal: …]`, `[shell exited…]`) are invisible to the hook; a GUI row starting with `[terminal` or `[shell` in any gate read is a STOP.

**3.9 Disarm.** After the status epoch passes F1-F5, the hook is removed once (`z1`), through the same request-queue-at-stop mechanism the retained snapshot uses, with an exact breakpoint-census update. The rest of the original schedule then runs exactly as before, without Terminal-side overhead. If disarm fails the run STOPs as host failure, with the metric result kept as it stood (class B in section 6). *Rejected alternative:* leaving the hook armed through live-stream phases, whose hit volume is unknown and would risk the original 120 s/900 s deadlines for a metric that is already decided.

**3.10 Coverage and recovery.** Breakpoint census stages: pre-Terminal (512 slot entries + existing Cellbox sites) → plus exactly one `(hook_pc,1)` after the identity gate → minus it after disarm. The retained `admit()` already treats every enrolled `Z1` site generically (`(addr,1) in census`, before==after==current, one TB entry/exit, expected `z1,s,Z1` controls), so a flags-3 zero-retirement at the hook PC uses the unchanged per-event conditional ONE extra step; an unproved second event is terminal. Cellbox allocator events, heap watches, stack-page watches, input gate (S10-4.13), atomic RSP and shared QMP lock are unchanged and must keep their retained tests.

**3.11 Privacy and security.** Read-only host debug observation; no guest write, no new input route, no new control or network path; packet classes are exactly those already allowed (`p`, `m`, `Z1/z1`, `s`, `c`, `qqemu.*`, existing QMP set). The ledger holds exactly what the Terminal displayed for scripted commands (banner, typed commands, JSON process/memory records, `st=0`); no credential prompt exists in the schedule. It is stored in the new directory with owner-only permissions, never inlined into docs/CHANGELOG (only counts and sha256 there), and has no more sensitivity than the retained PNG frames. Hook state and ledger are discarded with the QEMU process; the guest keeps no trace.

##### 4. Offline test plan (EVERY class must pass before any VM)

Fixtures: the retained Terminal/Cellbox/kernel ELF bytes and retained assembly; the retained `rsp-traffic.jsonl`; synthetic RSP (scripted GDB server speaking real framing/ACK/checksum into the unchanged `AtomicReceiveMixin` client) and synthetic QMP servers; a deterministic synthetic Terminal-feed generator that reproduces exact `drain` call grammar (first+31 loop per stream, 512 B chunking, page-crossing buffers) over retained real JSON-record bytes.

| Class | Must demonstrate |
|---|---|
| T1 static ELF identity | sha256s; ET_DYN loads; `feed` symbol/size; prologue 64 B; exactly 4 direct callers with return offsets above; no other call/jmp/relocation/data pointer to `0x1da80`; single `ret`; panics only at the two bounds calls; the call-site prelude (`RDX=RSP+0x20`, `RCX` in `1..0x200`) re-derived from disassembly; endpoint `+0xc8/+0xe8` to stdout/stderr mapping confirmed or the stream names withdrawn; windows relocation-free; all derived facts written to a new `static-identity.json` and compared byte-for-byte with retained assembly |
| T2 entry/identity | Terminal fingerprint accepted only for the first Terminal entry in phase `terminal-launch`; Cellbox entries byte-identical to the retained packet sequence; non-Terminal, wrong-window, second Terminal, wrong phase, unmapped page, user/kernel CS cases rejected; `instances`/`entry_ordinal` untouched |
| T3 per-hit invariants | every register/return/length/`RDX` invariant individually violated; pinned `RDI/RSI/CR3` drift; short/long/non-hex/`E`-error memory replies; 512 B page-crossing read; zero-length |
| T4 ledger integrity | drain-grammar accept/reject matrix; stream tagging; run-length 31/32; duplicate and dropped-stop injections; three-way count reconciliation; lock-order/deadlock watchdog (5 s bound, thread dump); fsync-before-step ordering; STOP retains ledger |
| T5 oracle accept/reject | accept exactly the true stream for finite and status; deterministic mutation suite over the success stream (delete, duplicate, reorder, split-at-every-boundary, coalesce, flip, insert stderr/ESC/NUL, truncate frame, wrong `total_cells`, `dropped!=0`, three frames, missing prompt, `st=1`, stale old prompt, pre-Enter hit, hits in the F2 window) must NEVER accept; boundary splits of the true stream must still accept; verdict parity with the retained `_validate_records` over a shared fixture corpus |
| T6 GUI cross-check | tail-wrap model vs real renderer fixtures (retained `rendered_proof` approach), including 62-row output on 79x26, full-width rows, UTF-8, `SOFT_RESET`; unreadable/unstable/mismatching final reads reject; intermediate non-gating samples cannot change a verdict |
| T7 integration on retained traffic | replay the retained S10-4.15 RSP traffic up to scope 21: the new observer emits identical Cellbox packets and, at the Terminal entry, only the added fingerprint reads and `Z1,11601da80,1`; synthetic continuation drives hits through the full finite+status+disarm run on a synthetic guest model |
| T8 existing guarantees | atomic RSP, QMP serialization, input journal/rejection-only, per-event one-extra recovery, census, all S10-4.15 retained/current tests re-run unchanged; zero-retirement injected at the hook PC admits/rejects exactly as at Cellbox sites; second event terminal |
| T9 budgets | cap and deadline arithmetic from measured per-hit packet counts; cap, quiescence and deadline expiry produce the labelled classes |
| T10 failure retention | every STOP path (identity, invariant, grammar, count, oracle, deadline, transport, cleanup) flushes ledger, last registers/RSP frames, last GUI frame/read, epoch and reasons BEFORE cleanup, rethrows the original error, performs no extra guest action |
| T11 preservation/policy | only the new sibling directory is written (`O_EXCL`); 546 protected paths, 9 prior archives, executing sources and compressed trace hash-stable; no `M`/`X`/`Z0`/`Z2`-on-Terminal/new QMP command can be emitted; `RUN_CONSUMED` guard refuses a second launch |

**Single clean invocation requirement.** One command, one fresh interpreter (`python -B`, `PYTHONDONTWRITEBYTECODE=1`), discovers and runs ALL classes T1-T11 together with the retained 144 and the 47 S10-4.15 tests, as one unittest run with total test count recorded and exactly `failures=0, errors=0, skipped=0, expected failures=0`, raw stdout/stderr and a receipt (argv, cwd, environment hash, start/end, per-source sha256) written once. Aggregating partial or per-module runs does not count (that was the S10-4.15 deviation). Failed development iterations before the final invocation are allowed and must be disclosed; after the final passing invocation the executing sources are frozen into `frozen-sources.sha256`, and any later byte change voids the gate and requires the whole invocation again. The runner refuses to start if any frozen hash differs.

##### 5. Go / no-go before the VM

GO only if ALL hold: (1) this CP1 is approved by the user; (2) the single clean invocation passed and `frozen-sources.sha256` matches; (3) T1 `static-identity.json` matches the ELF supplied by the candidate image; (4) candidate and canonical hashes, 546 protected paths, prior archives, compressed trace and retained QEMU health (same-binary/source-exact) verified immediately before launch; (5) the run directory and `RUN_CONSUMED` marker do not exist and are created exclusively; (6) no stray QEMU and the disk clone area is writable; (7) docs record the approval before execution. Any failure is NO-GO: no VM runs, nothing is retried as a run, S10-4 stays closed partial, and the no-go is recorded.

##### 6. The ONE run, STOP criteria and final classification

**Bounds.** Exactly one fresh desktop run of the original observed schedule (launch, idle 10 s/100 samples, finite stream, collected cat, live stream, cancel, rg, interactive top, complete snapshot) under the original 120 s finite, 60 s status and 900 s global limits and all original command limits; no headless run; no pause-excluded clock; no deadline increase; no new control beyond those already approved (new *instrumented site* only, by this CP1).

**Terminal STOP (no retry, no resume past it):** identity-gate failure; second Terminal entry; any 3.3 invariant; unknown return site; drain-grammar violation; memory read/short-reply error; hit/scope/trace count mismatch; ledger write failure; cap exceeded; F1-F5 failure; a `[terminal`/`[shell` notice row; any pre-existing S10-4.15 stop (flags/TB admission rejected, second no-progress, census change, input-gate rejection, QMP/RSP error); disarm failure; any original deadline expiry; protected-byte drift; cleanup error. On STOP the failure evidence is retained first, then clean owned shutdown.

| Final class | Condition | Consequence |
|---|---|---|
| **A SUCCESS_COMPLETE** | F1-F5 pass for finite and status epochs, hook disarmed, the rest of the original schedule completes through the `complete` snapshot, all retained proofs true | Actual output oracle and full observed-workload bounds recorded. **CBX-27/criterion 4 still unmet** (RSS, exact/kernel stack, historical comparator null). |
| **B SUCCESS_METRIC_ONLY** | F1-F5 pass for both epochs, then any later STOP | Metric proven; memory remains partial bounds; S10-4 closes partial. |
| **C OBSERVABILITY_INCOMPLETE** | healthy host, but identity/invariant/grammar/count/oracle/cap/quiescence/final-screen failure, unsupported byte grammar, or a deadline expiry whose ledger cannot attribute cause | Recorded as is; no inference of success or of product failure. |
| **D OBSERVED_NEGATIVE_RESULT** | ledger integrity proven (prompt reached) and content contradicts (not two frames, `dropped!=0`, `complete:false`, `st!=0`, error text) | A real candidate behaviour finding, final. |
| **E HOST_FAILURE** | RSP/QMP/QEMU/gate/harness exception, unattributed stop, recovery rejected/second event, protected-byte drift, cleanup failure | Host defect recorded; not a candidate verdict. |

Precedence: if a failure occurs before BOTH epochs pass F1-F5, the class is the highest applicable of E, then C, then D. If both epochs passed, the class is A or B by how far the schedule got, and any later E/C reason is recorded as the tail STOP reason, not as a downgrade of the proven metric. **No follow-up fix-and-rerun loop is authorized, for any class including E.** Any failure is recorded, the evidence archived, and S10-4 closes partial. A further attempt would need an entirely new CP1.

##### 7. Files, preservation, rollback, risks

**Create only** (new sibling): `reports/s10-4-evidence/runtime/observer-feed-hook/` containing `hook_ledger.py` (pure tokenizer/validators/oracle), `hook_observer.py` (additive `Observer` subclass: Terminal entry, hook, disarm), `hook_surface.py` (epoch/GUI wiring over the retained `TemporalDesk`), `run_hook.py` (single-run runner with `RUN_CONSUMED` and frozen-hash checks), `test_*.py`, `fixtures/`, `static-identity.json`, `verification-summary.json` plus raw single-invocation receipt, `frozen-sources.sha256`, and after the run `desktop/`, `summary.json`, `interpretation.json`, `evidence.sha256`. Nothing under `kernel/`, `cells/`, canonical/candidate/userVMX or any prior archive is edited; prior sources are imported unchanged. Docs entries (plan/qualification/CHANGELOG/this file) are the only other writes. Work/disk clone under `/tmp`, removed after.

**Rollback:** none to the guest or product (breakpoints live only in the QEMU process). If the CP1 is withdrawn or the run is a NO-GO, the new directory stays as inert evidence and the docs entry is marked withdrawn/no-go; no deletion of retained evidence.

**Risks (carried into any result):** per-hit overhead against the 120 s window; zero-retirement at a new site (existing bounded recovery or STOP); the Terminal's desktop byte grammar is not retained (headless serial is different), so the whitelist is fail-closed and may reject real bytes; stdout/stderr mapping inferred until T1; entry-only hook cannot prove the last `feed` returned; no session/operation identity; `notice()` bypass; the tail workload phases are untested under a desktop observer; the retained binaries live in `/tmp` and must be re-supplied from the candidate image.

##### 8. Decisions

None beyond approving or declining this CP1. Design choices made and recorded here: entry-only hook; `Z1` only; disarm after the status epoch; caps 2048 hits/512 KiB; 3.0 s quiescence; GUI intermediate samples non-gating.

##### 9. Acceptance ceiling

CBX-27 and Phase 10 criterion 4 remain **unmet** unless every missing element, including RSS/residency, exact stack and kernel-stack peaks and the matched historical comparator, is separately evidenced. S10-5, canonical promotion and containment remain blocked. Candidate, canonical and user VMX hashes stay unchanged; no commit/push.

##### 10. Actual execution results (2026-10-29)

**Result: Class B `SUCCESS_METRIC_ONLY`.** Evidence: `runtime/observer-feed-hook/` (`summary.json`, `interpretation.json`, `desktop/`, `evidence.sha256`). No headless run, no rerun, no fix after the run.

**Implementation (only under `runtime/observer-feed-hook/`; prior sources imported unchanged):** `feed_static.py` (T1), `hook_ledger.py` (pure ledger/tokenizer/oracle), `hook_observer.py` (additive `Observer` subclass), `hook_surface.py` (finite/status epochs gated by the ledger), `classify.py`, `run_hook.py` (preflight, `RUN_CONSUMED`, the one run), `run_gate.py`, `collect_result.py`, `test_t1…t7_t11` files, `static-identity.json` and fixtures.

**T1 (first):** `static-identity.json` re-derived live and archived: Terminal sha256 `816718a2…cc`, `feed` at `0x1da80`, exactly four direct callers in `drain` (call `0x146/0x1a0/0x1e0/0x228`, return `0x14b/0x1a5/0x1e5/0x22d`), no other call/jmp/relocation/pointer, one `ret` at `0x1e5f3`, two `panic_bounds_check` calls, `RDX==RSP+0x20` and `1<=RCX<=0x200` re-derived from the call-site prelude, seven relocation-free windows. **stdout/stderr mapping CONFIRMED as a program-order proof** (stage 1 `host+0xc8` first, stage 2 `host+0xe8` reachable only after stage 1 exits; with `session.rs` `for stderr in [false,true]`); field names were not read from a struct layout. The real run agrees: all 30 hits came from the stdout sites `0x14b/0x1a5`, none from stderr.

**Offline gate (ONE clean invocation):** `gate-attempts/attempt-02` = **269 tests = retained 144 + S10-4.15 47 + 78 new (T1–T11), 0 failures, 0 errors, 0 skipped, 0 expected failures, 0 unexpected successes**, frozen executing-source hashes (`frozen-sources.sha256`, `verification-summary.json`). Honest iteration record (`offline-iterations.json`): module development runs failed several times on test-expectation errors and one real preflight defect (`X.verify_inputs`); attempt-01 also passed (269) but the go/no-go preflight then found a defect in my prior-archive manifest comparison, so sources changed, attempt-01 was superseded and the whole gate rerun as attempt-02. Prior archives were hash-identical before and after the gate. Recorded residual limit exercised by T5: a structure-preserving reorder/flip that is scrolled off the 26-row screen would not be detected.

**Deviations/clarifications from the approved text (disclosed):** (a) the stack-window sanity check is 1 MiB below the pinned stack top, not the Cellbox 64 KiB rule, because the Terminal stack extent was not retained (RDX/return-site/pinned RDI/RSI remain the strong checks); (b) the Enter echo may be `\n` or `\r\n`; (c) `hook_ledger.evaluate` has an offline-only `until_ns` bound; (d) memory-reply errors in hook reads are hook stops (class C), not host exceptions. Nothing else was relaxed: F1–F5, caps 2048 hits/512 KiB, 3.0 s quiescence, two identical final reads at least 1.5 s apart, one run, original deadlines.

**Go/no-go:** all 13 criteria true (`go-no-go.json`): approval recorded, single clean invocation and frozen-source match, static identity equals the candidate ELF, candidate/Terminal sha, 546 protected paths equal to the S10-4.15 record, S10-4.15 manifest intact, QEMU same-binary/source-exact health and input-source binding, run not consumed, no stray QEMU, disk space, docs recorded before execution. `RUN_CONSUMED` was claimed (O_EXCL) before the VM.

**The one desktop run:** 232.0 s of the original 900 s window; finite epoch armed to complete 36.1 s of 120 s; status epoch 8.4 s of 60 s. Terminal entry fingerprint matched once at slot base `0x116000000` in phase `terminal-launch`; seven text windows and pages verified; hook `Z1` at `0x11601da80` installed before any feed.

- **Ledger:** 30 hits, 7053 B, all stdout, 0 stderr, no violation, every grammar/invariant check passed, JSONL fsynced with mode 0600; byte sha256 `b9d5443d…c66f`.
- **Finite epoch (F1–F5 all pass):** typed line exact in the ledger and on screen before Enter; Enter batch 55 committed once; 15 post-Enter hits, 6916 B = LF, **two complete frames** (begin, rows equal `total_cells`, end `complete:true dropped:0`), then `Cellos > `; 3.0 s quiescence; the 79x26 tail (68 scrolls, 26 rows) equalled the GUI exact read on two reads 1.5 s apart (sample 22 of 23; intermediate samples non-gating, only 2 matched).
- **Status epoch (own epoch):** batch 66 committed once; 2 hits, 15 B = `\nst=0\nCellos > `; screen matched twice; **status 0 proved**.
- **Three-way count** at disarm: ledger 30 = hook 30 = successful native scopes at the hook 30 = traced TB entries at the hook PC 30 = `steps.jsonl` 30; 0 failed primary scopes; hook removed once, breakpoint census 521 to 520.
- **Transport/recovery unchanged:** 3220 native scopes (30 at the hook), 7 conditional extra steps all at Cellbox sites (none at the hook), all advanced; 125 GUI batches committed once; 85812 RSP packet pairs; 32.8 s observer pause.

**Tail STOP (recorded, not fixed):** after the finite and status epochs the retained original schedule ran idle, the collected-cat `top ... | to jsonl | cat` 10 s zero-JSON check and its Ctrl-C, then `Surface.status()` raised `bad status rows`: that retained check requires exactly one `st=N` row on the whole visible screen, but the screen correctly shows both the earlier `st=0` and the new `st=130`. This is a latent defect of the retained desktop schedule (`runtime/common.py`), unreachable until the finite epoch could complete; it is neither a candidate failure nor a hook failure (tail class E). The live-stream, rg, interactive-top and complete phases were therefore **not run**. No fix-and-rerun, as approved.

**Memory (partial bounds only, NOT full workload, RSS or exact stack):** foreground Cellbox aligned allocator events peak 67544 B (finite-stream phase; collected-cat 56176 B; last 42112 B), background 25272 B; stack write-attempt page-depth bounds [45057, 49152] B and [12289, 16384] B; mapped-backing snapshots 10199040 B at initial prompt, idle, finite-stream and collected-cat (guest present segment+user-stack backing, not RSS). These equal the S10-4.15 maxima; the later phases that might change them did not run. RSS/residency, exact stack peak, kernel-stack peak and the historical comparator remain `null`.

**Preservation:** 546 protected paths (469 canonical plus candidate and user VMX) hash-identical before/after, all 25 prior archive trees (every `runtime/*` except the new directory) and the compressed old trace unchanged, executing host sources unchanged, owned disk clone removed, no product/candidate edit or build, no commit/push (`preservation-comparison.json`).

**Residual limits of this success:** entry-only hook (last-feed completion proven by the exact final-screen match, not a return hook); `feed` carries no Cell/session/operation id; Terminal `notice()` bypasses `feed` (none visible); off-screen structure-preserving loss would be undetectable; the hook was disarmed after the status epoch by design.

**CP2 requested:** accept Class B and the exercised bounds only. **CBX-27 and Phase 10 criterion 4 stay unmet; S10-4 closes partial; S10-5, canonical promotion and containment stay blocked.**


### S10-5 — release record and promotion (includes S10-2b) — **CP1 APPROVED by the user 2026-10-29 (DS5-1..DS5-4 as recommended, including the G3 ELF binding check and the 11 non-desktop L6 consumers on the desktop ISO); EXECUTED 2026-10-29: P0–P9 and V1–V7 all PASS, results in S10-5.13; CP2 GRANTED by the user 2026-10-29 (deviations 1–8 and the catalog digest `f06f38ce…` accepted)**

This section is a design only. It was prepared by reading the repository, the retained candidate and the recorded evidence, plus read-only `sha256` and byte-offset checks (S10-5.1). No source, canonical file, guard, build or VM was touched. Claims marked `[INFERENCE]` were not executed.

**Gates that this CP1 does not waive.** S10-4 is closed partial, so **CBX-27 and Phase 10 criterion 4 (measurement) stay unmet**. S10-5 needs its own explicit approval (§5, last bullet). Promotion is the first canonical write since containment: it may copy only bytes that were already qualified and recorded (§2). A copy is **not** a new qualification. All 38 CELL-CONTRACT-0024 lock relations remain. No production, physical Intel, VMX/EPT or remote claim follows.

#### S10-5.1 What is promoted (inputs, read-only checks on 2026-10-29)

Candidate `4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821` = `/tmp/cellbox-s10-3d-resume/cand/candidate.json` (read-only; built from HEAD `3471028bc` with 2490 dirty files, digest `6b713f3e…`; dev key, route `x86-candidate-<profile>`; kernel features `default,policy-required`, PC adds `board-x86-pc`; no failures recorded).

| Artifact | SHA-256 | Bytes |
|---|---|---:|
| headless `kernel_fs.img` | `384c04fa298c4d234bacee149af8787cc917aeb4541dea912365073ae790fcb3` | 12,582,912 |
| headless `init` | `fc1e6559c47f271f59609197c714dc130f4ce9e8ea001e8b682a11caf5ded7ab` | 406,976 |
| headless kernel / ISO | `7efb030f…efb74` / `49b32f34…a753a` | 15,599,680 / 19,472,384 |
| **desktop `kernel_fs.img`** | `3848cefb25f0e7c4abc67feccf3d8886292e25da60a88b4633552651292479ec` | 25,165,824 |
| **desktop `init`** | `c0bd14580fda0d697a2edd66b19168107fa6877a190bc06c1a8a482b467b23f3` | 406,976 |
| **desktop kernel** | `dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a` | 28,182,592 |
| desktop ISO | `d207031983d53460fed88a8578975b13517dd4c412e4544c0b833ce06afe7334` | 32,055,296 |
| PC kernel / ISO (headless image + `board-x86-pc`) | `3253bc70…e9da6` / `3cdf536a…d8a8a` | 15,599,672 / 19,472,384 |
| manifests | headless `4066a0e2…ef5c`, desktop `af55e602…e8d` | 6,444 (headless) |

Observed now, read-only: every hash above equals `candidate.json`; each kernel contains its profile's `kernel_fs.img` at ELF offset 2,064,272 and `init` at 1,655,200 as exact byte runs; the 469 canonical paths of `s10-3d-resume/canonical/before.json` re-hash with **0 differences**; `kernel/src/main.rs` = `a9636fee…` (the user's VMX edit, unchanged, status `M`); the repository's `assemble-x86-image.py` (`6ea17736…`), `signing.py` (`a1d25650…`), `cli.py` (`ed92c59a…`) and `make-iso-ci.sh` (`0e62af27…`) equal the candidate's `recipes_sha256`; `build/vicell-x86.iso` = `eb1cb813…` (the S10-3 L6 before-baseline); `xorriso` and `qemu-system-x86_64` are installed; git HEAD is `bfd05ec02`, `git status --short` lists 732 paths.

**The candidate lives in `/tmp` and is volatile** (D3-3 A asked for `$HOME/.cache/cellos-candidates/<sha>/`; that move never happened, S10-3.10 deviation 4). Step P0 makes a durable, read-only, hash-verified copy before anything else.

**Canonical paths today** (§1.2/§1.3, unchanged): `kernel/src/embedded-x86_64/{kernel_fs.img (447d0348…298e2c, 16,777,216 B, tracked, modified vs HEAD), init (0c2f607b…04104, 393,152 B, tracked, clean), boot-config/{autoload,services,system}.toml (gitignored, stale, still start sshd)}` and `target/x86_64-unknown-none/release/cellos-kernel` (`12cb0bac…ac67`, 19,769,472 B, embeds `447d0348`). `manifest.json` does not exist there. HEAD's committed blob is `a4cba963…ef5f3` (12,582,912 B, 33 files: `/bin/shell`, `/bin/sshd`, `ocel`, `ocel-quickjs`, `welcome.md`; no `/bin/cellbox`), so the tracked canonical image has always been a desktop-class ramdisk (`make-iso-ci.sh:25` also calls it "the canonical desktop ramdisk").

#### S10-5.2 Which profile becomes canonical (D10-3) and how the bytes map to the single canonical paths

The repository has **one** embedded directory. The candidate has **two profiles whose bytes are disjoint**: of the 25 paths both images carry, only `/POLICY.BIN` and `/etc/cellos/system.toml` are byte-identical. All 21 shared binaries (`cellbox` 3,093,952 B vs 3,094,016 B, `ls`, `net`, `vfs`…) and both boot-config files differ, and `init` differs (`fc1e6559…` vs `c0bd1458…`, signed payloads differ). Headless-only lanes therefore qualify the headless bytes; the desktop bytes are qualified by different lanes (S10-5.6).

| Path in the repository | Receives (recommended, **desktop**) | Source in the candidate |
|---|---|---|
| `kernel/src/embedded-x86_64/kernel_fs.img` | desktop image, 25,165,824 B, `3848cefb…479ec` | `cand/desktop/kernel_fs.img` |
| `kernel/src/embedded-x86_64/init` | desktop `init`, 406,976 B, `c0bd1458…23f3` | `cand/desktop/init` |
| `kernel/src/embedded-x86_64/boot-config/` | the 3 desktop template-generated files (replaces the stale ones) | `cand/desktop/boot-config/` |
| `kernel/src/embedded-x86_64/manifest.json` (new, untracked; the CI step and `build-x86_64-cells.ps1:67-68` read `kernel_features` from it) | desktop manifest, `af55e602…` | `cand/desktop/manifest.json` |
| `target/x86_64-unknown-none/release/cellos-kernel` (gitignored default kernel path that G3 gates) | desktop kernel, `dfbe8c55…dc1a` | `cand/desktop.kernel.elf` |

Why desktop: D10-3 A is the recorded user decision (the GUI Terminal ships in the canonical desktop profile); L4 6/6 and L0 desktop 6/6 both passed on payload-matched bytes, the precondition D10-3 set; the tracked path and the G3 comment already mean "desktop". The desktop image is a superset of the headless one: the same `cellbox sh` autoload runs on the serial UART as well.

What this means, stated plainly:
- The **headless qualified bytes are not committed anywhere**. They stay in the retained candidate (durable copy, hash record) and in the release record. The CI x86 job assembles a *fresh* headless image into the same path inside its own checkout (`ci.yml:936`); that is a different byte set from the qualified headless bytes (also, `init` is not reproducible across `CARGO_TARGET_DIR` copies, O-1 in S10-3.12: only `.strtab` and `.init_array` differ). CI results are regression signals for the recipe, **not** qualified-byte evidence.
- The desktop `init` (`c0bd1458…`) is not the headless `init` (`fc1e6559…`) and neither equals the tracked `init` (`0c2f607b…`); the tracked `init` changes in the working tree at promotion.
- Copying `kernel_fs.img` and `init` is two file replacements, not an atomic pair. Consistency is by order, a compare-and-swap on the expected old hashes, and the final binding check (S10-5.5 P3, S10-5.6 V2).

#### S10-5.3 Kernel and ISO treatment

What the candidate binds:
- Each candidate kernel embeds its profile's `kernel_fs.img` and `init` byte for byte (verified above). The embedded bytes are the signed cells that the kernel's dev-policy admits; the kernel features are `default,policy-required`, so Tier 2 admission is disabled and `dev-weak-rng` is on (D10-12 A).
- Each candidate ISO is `make-iso-ci.sh` over that kernel, `scripts/x86/limine.conf` and the Limine 8.7.0 files. It carries `boot/kernel.elf` by value. ISO bytes also carry volume timestamps, so **a rebuilt ISO will not match `d2070319…` byte for byte** `[INFERENCE: the script sets no SOURCE_DATE_EPOCH]`. Equality is therefore judged on the ISO tree (S10-5.6 V5), not on the ISO file hash.

| Treatment | Preserves the qualified hash | Verdict |
|---|---|---|
| **Copy** `desktop.kernel.elf` to the default kernel path, hash-verified | Yes: `dfbe8c55…` is the exact qualified kernel | **Chosen** (recommended; the only option that promotes bytes already qualified) |
| Rebuild the kernel from the promoted embedded directory (CI command, `--features policy-required`) | **No.** The ELF would embed the same image and `init`, but differs in toolchain/target-path strings (`CARGO_TARGET_DIR`, `OUT_DIR`) `[INFERENCE]`, in every working-tree change since the candidate (2490 dirty paths then, 732 now) and in any change to the user's uncommitted VMX `kernel/src/main.rs`. Dirty state would contaminate it, and it would need a new L0/L4/L6 battery, which S10-5 does not authorise. | Rejected for S10-5 |
| Rebuild the ISO through the reopened G3 from the copied kernel | The ISO's `boot/kernel.elf` stays `dfbe8c55…`; only timestamps differ | Used for the P5 ISO (written to a scratch path, never to `build/`) and for the L6 run |

The headless and PC kernels/ISOs stay candidate artifacts. The PC ISO for the `iommu-dmar-x86` consumer is the **candidate** `pc.iso` (`3cdf536a…`, hash-checked), not a rebuild; it embeds the headless image, not the promoted one. Any later `cargo build -p cellos-kernel` at the default target path replaces the copied kernel; G3 then checks the binding (S10-5.4) and the qualification record covers only the recorded hash.

#### S10-5.4 S10-2b: which guards re-open, which never do (exact files, current line numbers)

| Guard | Decision | File and lines |
|---|---|---|
| **G2b** quarantined hashes `64eb28f3…`, `636497ac…` | **Kept forever, unchanged.** Stays first and unconditional in every route | `scripts/cellos_sign/signing.py:38-41` (set), `:206-214`, `:226`, `:261`; `test_cellos_sign.py` set test (`:636-640`) and `:564`, `:760` |
| **G2a** fixture names need a private seed | Unchanged for every non-route caller (the CLI, env, other recipes) | `signing.py:217-236` |
| **G2c, G2d, G4, G5, T** | Unchanged (G4 `kernel/build.rs:128-150`, lane alias guards stay) | — |
| **G1** unconditional exit in the desktop packager | **Re-opens**: delete the `sys.exit("ROLLOUT BLOCKED …")` block. No new precondition code: the packager only `exec`s the assembler as its own process, which already assembles fresh (never overlays), requires `/bin/cellbox-ssh` byte-equal to `/bin/cellbox`, forbids `/bin/shell`, `/bin/sshd`, a started `config`/`sshd` row and any blocked hash (`assemble-x86-image.py:203-255`). This replaces the S10-2b text "positive preconditions" by delegation | `scripts/package-desktop-x86.py:18-25` |
| **G3** default kernel refused | **Re-opens by content, not by name.** Replace the unconditional refusal with `scripts/x86/check_default_kernel.py <kernel> kernel/src/embedded-x86_64`, which exits non-zero with `ROLLOUT BLOCKED …` unless: (a) the image read with `generate-boot-config.py image_files()` has no file whose hash is in `signing.BLOCKED_ARTIFACT_SHA256` (imported, never copied), no `/bin/shell` or `/bin/sshd`, and `/bin/cellbox` equals `/bin/cellbox-ssh`; and (b) **the kernel ELF contains the exact `kernel_fs.img` and `init` bytes** (mmap search, ~55 MB). Part (b) is a **change to the approved S10-2b wording** ("it does not scan the ELF"): with a copied kernel there is no "image the kernel was built from", so without (b) a kernel embedding something else would pass on the strength of a clean file on disk. An explicit non-default `X86_KERNEL` stays admitted without the check (the lane route, unchanged) | `scripts/x86/make-iso-ci.sh:25-33` (comment `:25-27`); new `scripts/x86/check_default_kernel.py` |
| **Canonical assembler route** (CI x86 step, `build-x86_64-cells.ps1`, `package-desktop-x86.py`) | **Re-opens** as two new route-table entries (`x86-canonical-headless`, `x86-canonical-desktop`). Admitted only when `candidate_root` resolves **exactly** to `<repo>/kernel/src/embedded-x86_64` (candidate routes keep "outside the repository"), the cargo target dir is `<repo>/target` or outside the repo, the key is the dev key, and the pin matches. Every other in-repository root (`kernel/src/` siblings, `target/`, `build/`, symlinks into them) stays refused. The pin is re-recorded because the assembler changes | `signing.py:62-82` (route table, `:66` pin) and `:182-186` (root rule); `assemble-x86-image.py:298-300` (`route_name`) and `:414-424` (`check_isolation`, message "admitted only by S10-2b") |
| **Cloud images** (D10-9 A) | **Stay out.** G3 reopening would otherwise let `build-cloud-images.sh` build `qcow2`/`vmdk` from the default kernel. Add an explicit refusal at its top, and note that `:99` rewrites the canonical image in place (`package-ocel-profile.py`) | `scripts/build-cloud-images.sh:99,109` |
| **Non-Intel recipes** (D10-10 A) | **Stay refused**; their CI jobs stay red and are recorded | `gen-disk-ci.sh`, `build-aarch64-cells.ps1`, `make-hypervisor-fs*.sh`, `ci.yml` aarch64/RV64 jobs |
| Other default-path ISO callers | Re-evaluated by the helper: any kernel that does not embed the canonical image stays refused. In particular the x86 hypervisor job (`ci.yml:1097-1106`, `EMBEDDED_OVERRIDE=embedded-hv-x86` at the default path) stays refused | `ci.yml:1106`, `build-image.sh:225` |

Also edited: the stale CI comment (`ci.yml:922-930`, "Until S10-2b admits this canonical route …"), `signing.py:34-35` comment. **Tests:** `scripts/test_cellos_sign.py` (G1 test `:391-402` becomes "the packager only runs the assembler"; G3 tests `:403-440` become helper-driven; `:642-656` pin test; new canonical-route cases: exact root admitted, sibling/`target/`/`build/`/symlink roots refused, candidate route still refused in-repo, G2b refuses on every route and environment); `scripts/test_assemble_x86_image.py:431-455` isolation tests; new `scripts/test_check_default_kernel.py` (clean image admitted; `/bin/shell`, `/bin/sshd`, each blocked hash, missing alias, image bytes not in the ELF, truncated image: refused).

**Order inside S10-5 (see P2):** S10-2b lands before the bytes move, because the helper must be proven against the **real quarantined kernel** (`12cb0bac…`, must be refused) before it is allowed to admit anything. The helper reopens G3 only for kernels that really embed clean bytes, so landing it first admits nothing that is quarantined.
**Risk:** reopening G3 reopens the 12 CI consumers (L6) and the canonical recipe route lets any dev-key re-assembly overwrite the promoted bytes with unqualified ones (the recorded qualification then covers only the recorded hashes). **Rollback:** restore the files from the recorded pre-S10-2b copies (S10-5.8).

#### S10-5.5 Ordered procedure

Every step stops the slice on any mismatch. Evidence goes to `reports/s10-5-evidence/` and is generated by retained harness code (`harness/`), never edited by hand. No commit or push is part of S10-5.

| # | Step | Detail |
|---|---|---|
| P0 | Pre-flight (read-only) | Run the S10-5.10 checklist. Make the durable candidate copy `$HOME/.cache/cellos-candidates/4fae8b20…/` (mode 0700, files 0444/0555), `cmp`-verify against `/tmp/cellbox-s10-3d-resume/cand`, and re-verify every `candidate.json` entry from the copy. |
| P1 | Quarantine archive (D10-14 A), nothing deleted | `$HOME/.cache/cellos-quarantine/cellbox-s10-5-<UTC>/` outside the repository (mode 0700, files 0400) with `MANIFEST.sha256` and `QUARANTINE.txt` ("quarantined, not a safe boot set, never boot, sign or distribute; contains `64eb28f3…`, `636497ac…`, `sshd` autostart"). Contents: `kernel_fs.img` (`447d0348…`), `init` (`0c2f607b…`), the three old `boot-config/*.toml`, the target kernel `12cb0bac…`, plus a copy of every file S10-2b will edit (their pre-edit hashes). `build/vicell-x86.iso` (`eb1cb813…`, not quarantined, the L6 before-baseline) is only hash-recorded: S10-5 does not write it. Re-hash the archive; `cmp` against the originals. The containment sentence "do not boot, copy or distribute these quarantined candidates as a release" is about release; this retention copy is the D10-14 A archive. |
| P2 | S10-2b source change and tests (no canonical byte yet) | Apply S10-5.4. Run the signing, assembler and helper unit tests once. **Real negative controls**, each leaving outputs unchanged: `check_default_kernel.py` on the real quarantined kernel `12cb0bac…` and canonical image `447d0348…` → refused; `make-iso-ci.sh` with the default kernel → refused before xorriso; the G2b batch tests on both hashes through every route. The packager and the canonical route are tested only with the fake assembler or a temp repository copy; **the real packager is never run** (it would rebuild and overwrite). |
| P3 | Promote bytes (copy only) | Harness, compare-and-swap: refuse unless the destination hashes are the recorded pre-promotion hashes (or already the promoted ones, so a rerun is idempotent). Write each file to a temp name in the same directory, `fsync`, verify its hash, then `os.replace`: (1) `init`, (2) `kernel_fs.img`, (3) `boot-config/` (old directory removed after P1), (4) `manifest.json` last (the assembler's completion convention), (5) the kernel ELF to `target/x86_64-unknown-none/release/cellos-kernel` (mode 0755), (6) `promotion.json` (source `candidate.json` hash, every promoted path with sha256 and size, UTC time, archive path). Between (2) and (5) the old kernel coexists with the new image; the reopened G3 refuses that pair. |
| P4 | Verify the copy | V1–V3 of S10-5.6 (hashes, kernel binding, 469-path delta). |
| P5 | CI ISO through the reopened G3 | `X86_ISO_ROOT=$S/iso-root bash scripts/x86/make-iso-ci.sh $S/vicell-x86.iso` with the **default** kernel path (the helper must admit it), `$S` a scratch directory outside the repository. `build/vicell-x86.iso` and the tracked `build/x86-iso-root` are in the protected set and are not written. Then V5. |
| P6 | One P04 terminal run | V6. |
| P7 | L6 once | V7, 12 consumers and the CI boot check. |
| P8 | Records | S10-5.7, including `evidence.sha256` and the final re-hash of the 469 paths (only the intended paths changed, S10-5.6 V3). |
| P9 | CP2 request | Present results and every deviation. No commit or push without separate authorisation. |

If P2 fails, no canonical byte has moved: revert S10-2b and stop. If P3 or P4 fails, stop; the retained candidate is untouched, so P3 may be repeated after the cause is fixed; going back is S10-5.8.

#### S10-5.6 Verification after the copy, and which lanes may be cited

| # | Check | Pass rule |
|---|---|---|
| V1 | Byte identity | `kernel_fs.img` = `3848cefb…`, `init` = `c0bd1458…`, `manifest.json` = `af55e602…`, the three `boot-config` files equal `cand/desktop/boot-config/`; `cmp` and sha256. The image read with `image_files()` has the 32 manifest paths, every file's sha256 equal to `candidate.json`, no `/bin/shell`/`/bin/sshd`, no blocked hash, `/bin/cellbox` = `/bin/cellbox-ssh`, autoload starts exactly one `/bin/cellbox` `["sh"]`. |
| V2 | Kernel binding | `cellos-kernel` = `dfbe8c55…`; its bytes contain the promoted `kernel_fs.img` at offset 2,064,272 and `init` at 1,655,200; `check_default_kernel.py` admits it. |
| V3 | No collateral change | Re-hash the 469 canonical paths: the only differences are the intended ones (`init`, `kernel_fs.img`, three `boot-config` files, `cellos-kernel`; `manifest.json` added). The candidate directory, the durable copy, the archive and `kernel/src/main.rs` (`a9636fee…`) are unchanged. |
| V5 | CI ISO | `$S/vicell-x86.iso` built by P5. Extracted `boot/kernel.elf` = `dfbe8c55…`; the other tree files equal those of `cand/desktop.iso`; ISO file hash recorded (expected to differ, timestamps). |
| V6 | **One P04 terminal run** | `qemu-phase04-terminal-test.sh` in its unchanged **default mode** (no `--candidate`), TCG, once, expected 494/494. It builds its own fixture image and kernel in a scratch directory. It proves that the lane still passes with the promoted tree present and that the lane (G5 alias guard) leaves the promoted files byte-identical (V1 hashes re-checked after). It is **not** qualification of the promoted bytes. |
| V7 | **L6 once** | The 12 consumers and the CI boot check (`qemu-x86_64-test.sh`) on `$S/vicell-x86.iso` through the existing overrides (`VICELL_AHCI_ISO`, `VICELL_XHCI_ISO`, `VICELL_SERIAL_ISO`, `VICELL_NIC_ISO`, `VICELL_IGB_ISO`, `VICELL_IGB_VTD_ISO`, `VICELL_IOMMU_ISO`, and `VICELL_X86_ISO` for the five hard-coded consumers); `VICELL_PC_ISO` = candidate `pc.iso`; `desktop-x86` with `CELLOS_DESKTOP_ISO=$S/vicell-x86.iso` and `CELLOS_DESKTOP_ACCEL=tcg`; same harness and classification as S10-3.14 L6 (before-baseline = the recorded S10-3.14 results on `eb1cb813…`). Expected: 39 pass→pass, 1 fail→fail (xHCI injected-key decode, pre-existing), 4 skip, 1 ignored. A pass→fail is a stop. **New configuration:** the 11 non-desktop consumers ran on the headless ISO in S10-3.14 and now boot the desktop ISO. Their results are recorded as new evidence for this ISO; a failure is not waived by the headless result. |

**May be cited as covering the promoted desktop bytes** (identical hashes): L0 desktop 6/6 (9/9 runner markers); L4 6/6 at 290 checks on its 14 payload-equal imported cells (the exact set is each run's `payload-match.json`; cells outside it, such as `chrome`, `ocel`, `ahci`, `igb`, `xhci`, `serial`, `tier2-*`, have no payload-matched lane); the S10-3.14 L6 `desktop-x86` result on `desktop.iso` (same kernel and image as promoted); V1–V3 and V5–V7 of this slice.
**May not be cited as covering the promoted bytes** (headless or fixture bytes, same source, different binaries): L0 headless 6/6 (16 `P10C` markers), L1 (48 + 39), L2 (67), L3 (494), L5 (34), L5f (103, lane-only by definition), the headless and PC L6 ISOs, and every fixture-kernel edge. They stay cited **only for the headless profile**, with the profile named in each acceptance row (S10-3.3 rule: a row that cites a lane names its profile). `chrome`, `ocel`, `ahci`, `igb`, `xhci`, `serial` and `tier2-*` in the desktop image have no payload-matched lane; they rest on L0 desktop boot, `P10G-apps` smoke and L6.
**Not covered by anything:** KVM-only behaviour, hardware, production, crash durability, the Tier-3 guest, publickey/KMS.

#### S10-5.7 Records, release notes and wording

Update only to the exercised state; keep history at its ceiling.

| Record | Change |
|---|---|
| `acceptance.md` | Row by row with provenance (candidate hash, profile, lane, promotion hashes). CBX-10/14/26 gain "promoted dev-posture canonical bytes, QEMU/TCG" only if V1–V7 pass. **CBX-28 met** only when every record below is consistent with delivered evidence. CBX-23/24/25 stay partial; **CBX-27 stays unmet**. |
| Phase 04 file and `phase-04-closure-review.md` §1 row 4, §4 | Criterion 4 ("one suite executable packaged; no obsolete fallback; independent apps/services still work") checked as **x86 dev-posture canonical bytes, QEMU/TCG only, non-Intel recipes not covered (D10-10)**; Phase 04 `completed-private-evidence`. |
| `phase-10-qualification.md`, `plan.md` (frontmatter `executionGate`, rows 04 and 10) | Criteria and status per S10-5.9. |
| `.agents/plan-portfolio.md` (`:67`, `:135`) | Restate "38 locks remain"; no lock text edited. |
| `reports/rollout-containment.md` | New section "Update <date>: promotion (S10-5)": routes lifted (G1, G3 by content, canonical route), **G2b kept forever**, the 2026-10-21 hash update corrected (§1.2: `447d0348` was the hash all along), the "quarantined in place" bytes that were already overwritten (`target/.../cellbox` `006b8504`, `terminal` `018db0cc`; old bytes survive only inside images), the archive path and hashes, the rollback caveat of S10-5.8, and the fixture `build/*.iso`/`qcow2` that still contain the quarantined bytes (§1.2) and stay untouched. |
| `verification.md` `:3-8` | Replace the "acceptance withdrawn; rollout blocked" header (stale since S10-3) with the exercised disposition and the remaining gates. |
| `command-catalog.md` §6 (`:409`) | As-built: promoted hashes, desktop profile at the canonical path, headless by CI recipe, the stale "canonical image, ISO kernel and rollout are unchanged and blocked" bullet replaced. |
| `CHANGELOG.md`, `docs/cellbox-cli.md` (top status, containment bullets) | Release notes below; the doc's "implementation pending" header and the "`package-desktop-x86.py` exits before signing" bullets are replaced with the promoted state. |
| Proposal §1.1 guard table, §5, status line | As-built after S10-2b. |

**Release notes content (no new file; in `CHANGELOG.md` and `docs/cellbox-cli.md`):**
- What shipped: a **dev-posture** (D10-1) canonical x86 desktop image with one signed `cellbox` suite (`/bin/cellbox` and the second copy `/bin/cellbox-ssh`), the GUI Terminal, `cellbox sh` as autoload, the ps1 worker set; no `/bin/shell`, no `/bin/sshd`, `/bin/config` not started (D10-4 A, `export` → `config unavailable`, status 1). Dev cell/policy/signing keys; `dev-weak-rng` kept (D10-12 A: SSH needs RDSEED/RDRAND).
- Qualification scope: Intel x86 **QEMU/TCG only**, the retained candidate `4fae8b20…` and the hashes of S10-5.2. Headless profile qualified separately and not committed.
- Phase 09 partial (D10-8 A): `ssh` with password, known host and confirmation to another host; `--auth publickey` refused; CBX-23/24/25 partial (Tier-3 guest endpoint blocked by `tier3b-x86-vtx`, `tier3b-vm-hardening-compat`, `tier3b-finish-g5-lite`; publickey/KMS S9-5 behind `kms-silo-production-root`, `phase4-dev-reference-authority`).
- Known defects, listed not fixed: D60-3 clock units; GUI Delete key; `COMPLETION-QUEUE: FAIL` self-test line (D3-5); xHCI injected-key decode failure (pre-existing at L6); x86-32 handoff skips and virtio block-init ignore; the incidental DMA-free/DSTRD gaps of the S10-3 disclosures; latent `Surface.status()` st=N check in the S10-4 desktop schedule; "synced, not crash-proof" storage.
- **CBX-27 unmet**: footprint measurement is partial (S10-4 closed partial: no RSS, exact stack, kernel stack, matched historical runtime or full-workload peaks). The file footprint grew: desktop six qualified binaries 3,261,680 B (+2,267,616 B against the B2 unsigned comparator, across different features) and the canonical image 16,777,216 B → 25,165,824 B. No optimisation claim.
- Not claimed: physical Intel, VMX/EPT, production posture or key, remote C2C, cloud images (D10-9 A), non-Intel images (D10-10 A, CI jobs stay red), KVM qualification, crash durability, CI results (not run by this slice), publishing or tagging.

#### S10-5.8 Rollback, stated honestly

- **Code:** restore the files S10-2b edited from their recorded pre-edit copies (P1 archive). That re-closes G1, G3 and the canonical route exactly as before; G2b never opened.
- **Bytes:** restore the archived `kernel_fs.img`, `init`, `boot-config/` and kernel from the quarantine archive. This only returns the tree to its **previous, quarantined state**. Containment §Recovery says no matched known-safe source/kernel/init/Cell/policy/image set exists; `447d0348…` is **not** one (it contains the quarantined `cellbox` and `terminal` and starts `sshd`), and neither is HEAD's `a4cba963…` (it contains `/bin/shell` and `/bin/sshd`). Restoring them is a containment-state restore, not a safe boot set, and they must not be booted or distributed as a release.
- The only known-qualified set is the retained candidate; the practical "rollback" of a bad promotion is therefore a forward correction from that candidate (P3 again), not a return to the quarantined bytes.
- Not undone by any rollback: trust-store or file writes and remote effects made by lanes, bytes already copied elsewhere (cached ISOs, artifacts), lane evidence, CI runs and anything a later `run-desktop.sh --rebuild` did. Note that `run-desktop.sh:20-36` temporarily rewrites the canonical image and restores it by trap; do not run it during S10-5.

#### S10-5.9 Closing status (proposed; decided at CP2, no overclaim)

| Item | Proposed | Why |
|---|---|---|
| Phase 04 criterion 4 | **checked**, qualified "dev-posture x86 canonical bytes, QEMU/TCG; non-Intel recipes not covered" | It is a packaging criterion; it does not depend on CBX-27. It stays unchecked if any of V1–V7 fails. |
| Phase 04 status | `completed-private-evidence` | All six criteria checked on that ceiling. |
| Phase 10 criterion 1 (CBX-01..29 actual evidence, no silent exclusions) | **unmet** | CBX-23/24/25 partial, CBX-27 unmet. |
| Phase 10 criterion 2 (real effects) | checked at the candidate/promoted ceilings (user judgement at CP2) | L0/L4/L6; guest SSH excluded. |
| Phase 10 criterion 3 (one suite artifact, apps/services package) | checked after V1–V7 | |
| Phase 10 criterion 4 (equivalent measurement) | **unmet** | S10-4 closed partial. |
| Phase 10 criterion 5 (docs, notices, help, routes, portfolio) | checked after S10-5.7 | |
| Phase 10 status | **`closed-partial-private-evidence`** (criteria 1 and 4 open), like Phase 09 | Not `completed`. |
| `plan.md` status | follows Phase 10; `executionGate` rewritten to the remaining gates | Locks and production/physical gates remain. |

#### S10-5.10 Pre-promotion checklist (re-verify the facts; stop on any difference)

1. User CP1 for S10-5 recorded, with the answers to S10-5.11.
2. Candidate: `sha256(candidate.json)` = `4fae8b20…`; every candidate file in S10-5.1 re-hashed; durable copy verified.
3. **469 canonical paths** re-hashed against `canonical/before.json`: 0 differences (done once on 2026-10-29; repeat immediately before P3). Expected current values: `kernel_fs.img` `447d0348…`, `init` `0c2f607b…`, kernel `12cb0bac…`, `boot-config/*` `07849a42…`, `5c97bad0…`, `74ecea5c…`.
4. `kernel/src/main.rs` = `a9636fee…` and still `M`; it is never edited, staged or reverted by S10-5.
5. Working tree: record `git rev-parse HEAD` (`bfd05ec02…`), the `git status --short` count (732 on 2026-10-29) and the list of modified paths under `kernel/`, `cells/`, `scripts/` and `.github/`. Because promotion copies bytes and never rebuilds, this dirtiness cannot enter the promoted bytes; it **would** enter any rebuilt kernel or image (including the user's VMX edit), which is why a rebuild is rejected. The S10-2b files are already modified by S10-2a; record each file's hash before editing so the S10-2b diff is attributable.
6. `assemble-x86-image.py` `6ea17736…`, `signing.py` `a1d25650…`, `cli.py` `ed92c59a…`, `make-iso-ci.sh` `0e62af27…` unchanged since the candidate; any difference is explained before P2.
7. No running QEMU or cargo/pytest process in the repository; free disk for the archive (~60 MB), the durable candidate copy (~125 MB) and the P5–P7 scratch.
8. `xorriso`, `qemu-system-x86_64` (TCG) and `limine/limine-8.7.0/bin` present; Docker/OpenSSH fixtures are not needed (P7 does not run L5).
9. `build/vicell-x86.iso` = `eb1cb813…` (the L6 before-baseline); S10-5 does not write it.

#### S10-5.11 Decisions for CP1 (only those that change what is done; recommendations in bold)

| ID | Question | Options | Recommendation |
|---|---|---|---|
| DS5-1 | May S10-5 promote while **CBX-27 / Phase 10 criterion 4 stay unmet**? | **A** yes; ship with CBX-27 unmet, the measured file-footprint growth disclosed and Phase 10 closing `closed-partial-private-evidence`; **B** hold S10-5 until a separately approved CBX-27 disposition | **A.** Phase 04 criterion 4 and CBX-28 do not depend on measurement; B would keep the stale quarantined bytes canonical indefinitely. The user may still choose B on footprint grounds. |
| DS5-2 | Which profile occupies the single canonical paths? | **A** desktop (D10-3 A); headless stays candidate/CI-built; **B** headless (matches CI, L1–L5 payload coverage and the 11 non-desktop L6 consumers, but withholds the canonical Terminal, i.e. D10-3 B) | **A**, accepting that L1/L2/L3/L5 and L0 headless do not cover the desktop bytes (S10-5.6). The disjointness of the profiles was not visible when D10-3 was decided. |
| DS5-3 | Kernel at the default path | **A** copy the qualified desktop kernel; G3 admits by content plus ELF binding (the S10-2b wording change); **B** rebuild the kernel from the promoted directory (no hash claim; needs a new L0/L4/L6 battery, outside S10-5) | **A.** |
| DS5-4 | S10-2b scope | **A** G1, G3 (content rule) and the canonical assembler route, as in the approved S10-2b text; **B** G1 and G3 only, canonical route stays closed (CI x86 step stays red) | **A.** B leaves the recipes cut over in S10-1 unusable in their canonical route. |

Defaults (no decision requested): archive and candidate-copy paths as in P0/P1; no commit, push, tag or release publication (the new untracked `kernel/src/embedded-x86_64/manifest.json`, the replaced tracked `init`/`kernel_fs.img` blob of 25,165,824 B and the S10-2b source edits are left for the user to stage); no run of the real packager or any rebuild; the other records edited by this draft (`plan.md`, `phase-10-qualification.md`, `CHANGELOG.md`) carry only the "CP1 proposed" line.

#### S10-5.12 Risks

- The desktop profile at the canonical path is covered by fewer lanes than the headless one (S10-5.6); the release notes say so.
- The 11 non-desktop L6 consumers run on the desktop ISO for the first time; a failure stops the slice.
- The helper's ELF-binding rule is a design change to approved S10-2b text and needs this CP1's approval.
- Reopening the canonical route lets any dev-key re-assembly replace the promoted bytes; the record then covers the recorded hashes only. CI and `build-x86_64-cells.ps1` produce a headless image that is *not* the qualified headless bytes.
- Two-file replacement is not atomic; the order, compare-and-swap and the V2 binding check bound a crash window, they do not eliminate it.
- Promotion is the first canonical write since containment. **Acceptance of this slice:** CBX-28; Phase 04 criterion 4; Phase 10 criteria 2, 3, 5 and the honest closure of 1 and 4 as unmet.

#### S10-5.13 Execution results (2026-10-29; user CP1 approved; **CP2 GRANTED 2026-10-29**)

Executed exactly as S10-5.5 (P0–P9 then V1–V7), no stop condition hit. Evidence: **`reports/s10-5-evidence/`** (`p0-preflight.json`, `p1-archive.json`, `promotion.json`, `p4-verify-*.json`, `v5-iso/`, `l6/`, `logs/`, `harness/`, `evidence.sha256` generated by code). Scratch/outputs: `/tmp/cellbox-s10-5-scratch` (ISO, extracted trees, L6 cargo target); the protected `build/` was not written. Nothing staged, committed, pushed or tagged. `kernel/src/main.rs` (`a9636fee…`, the user's VMX edit) untouched.

**Pre-flight (P0), before any canonical write:** candidate `candidate.json` = `4fae8b20…`; 67 candidate files re-hashed = `candidate.json`; 469 canonical paths re-hashed: 0 differences (re-checked again immediately before P3); expected old hashes (`kernel_fs.img` `447d0348…`, `init` `0c2f607b…`, kernel `12cb0bac…`, boot-config `07849a42…`/`5c97bad0…`/`74ecea5c…`) all present; git HEAD `bfd05ec02`, 736 modified/untracked paths (kernel/cells/scripts/.github lists in `p0-preflight.json`); no QEMU/cargo/pytest process; `xorriso`, `qemu-system-x86_64`, Limine 8.7.0 present. **Durable candidate copy** `~/.cache/cellos-candidates/4fae8b20…/` (76 files, 195,646,631 B, mode 0500/0444/0555), byte-compared against `/tmp/cellbox-s10-3d-resume/cand`.

**P1 quarantine archive:** `~/.cache/cellos-quarantine/cellbox-s10-5-20261029T041842Z/` (0700/0400, `QUARANTINE.txt`, `MANIFEST.sha256`): `kernel_fs.img` `447d0348…`, `init` `0c2f607b…`, the three old `boot-config` files, kernel `12cb0bac…`, plus the pre-edit copies of the nine S10-2b files (`pre-s10-2b/`). Each copy hash- and `cmp`-verified; nothing deleted. `build/vicell-x86.iso` (`eb1cb813…`) only hash-recorded (not written).

**P2 S10-2b (edit tool only; files and new hashes):**

| File | After (sha256) | Change |
|---|---|---|
| `scripts/package-desktop-x86.py` | `d80517a8…` | G1: unconditional exit removed (thin caller of the assembler only) |
| `scripts/x86/make-iso-ci.sh` | `8f5bef77…` | G3: default kernel admitted by `check_default_kernel.py` content rule; explicit `X86_KERNEL` unchecked, as before |
| `scripts/x86/check_default_kernel.py` (new) | `150f40a8…` | image content rule plus ELF binding (the approved change from the S10-2b text) |
| `scripts/cellos_sign/signing.py` | `ba971293…` | `QualifiedRoute.in_repo_root`, `X86_CANONICAL_ROOT`, routes `x86-canonical-headless/desktop`, root rule, pin re-recorded `b74cc997…`; **G2b untouched** |
| `scripts/assemble-x86-image.py` | `b74cc997…` (was `6ea17736…`) | `is_canonical_out_dir`, `route_name`, `check_isolation` (canonical route only for `kernel/src/embedded-x86_64` with target dir `<repo>/target` or outside) |
| `scripts/build-cloud-images.sh` | `ee4885b9…` | explicit refusal (D10-9 A); the script still rewrites the canonical image in place otherwise |
| `.github/workflows/ci.yml` | `0d77f752…` | stale comment `:928-930` only |
| `scripts/test_cellos_sign.py`, `scripts/test_assemble_x86_image.py`, `scripts/test_check_default_kernel.py` (new) | `a08c603b…`, `d78b4eeb…`, `eff3ddc7…` | G1/G3 tests, canonical-route tests, helper tests |

Host tests, run once at the end (`logs/p2-*.log`): `test_cellos_sign.py` **71 OK**, `test_assemble_x86_image.py` **40 OK**, `test_check_default_kernel.py` **8 OK**. **Real negative controls (before P3, the real quarantined bytes):** `check_default_kernel.py` on kernel `12cb0bac…` + image `447d0348…` → exit 1 `quarantined bytes in the image: /bin/cellbox`; default-path `make-iso-ci.sh` → exit 1 before xorriso, no ISO or ISO root written; `build-cloud-images.sh` → exit 1 `ROLLOUT BLOCKED`. The helper control is re-recorded on the archived quarantined files in `logs/p2-controls.log`. The real packager and the canonical route were never run.

**P3 promotion** (`harness/p3_promote.py`, order init, `kernel_fs.img`, `boot-config/`, `manifest.json`, kernel; compare-and-swap; every copy verified against `candidate.json`; old `boot-config` moved to the scratch directory, not deleted):

| Path | Before | After |
|---|---|---|
| `kernel/src/embedded-x86_64/init` | `0c2f607b…` (393,152 B) | `c0bd1458…` (406,976 B) |
| `kernel/src/embedded-x86_64/kernel_fs.img` | `447d0348…` (16,777,216 B) | `3848cefb…` (25,165,824 B) |
| `kernel/src/embedded-x86_64/boot-config/autoload.toml` | `07849a42…` | `db435065…` (358 B) |
| `kernel/src/embedded-x86_64/boot-config/services.toml` | `5c97bad0…` | `eff37d25…` (897 B) |
| `kernel/src/embedded-x86_64/boot-config/system.toml` | `74ecea5c…` | `74ecea5c…` (identical; replaced, unchanged) |
| `kernel/src/embedded-x86_64/manifest.json` | absent | `af55e602…` (7,956 B, untracked) |
| `target/x86_64-unknown-none/release/cellos-kernel` | `12cb0bac…` (19,769,472 B) | `dfbe8c55…` (28,182,592 B) |

**Verification:**

| V | Result |
|---|---|
| V1 byte identity | PASS (`p4-verify-v1-v3.json`, 30 checks): `cmp` and sha256 of `init`, `kernel_fs.img`, `manifest.json`, the three `boot-config` files equal the candidate; the image has the 32 manifest paths, each file's sha256 equals `candidate.json`; no `/bin/shell`, `/bin/sshd` or quarantined hash; `/bin/cellbox` = `/bin/cellbox-ssh`; autoload starts one `/bin/cellbox` `["sh"]`; no `sshd`/`shell`/`config` service row |
| V2 kernel binding | PASS: kernel `dfbe8c55…` embeds `kernel_fs.img` at 2,064,272 and `init` at 1,655,200; `check_default_kernel.py` admits it |
| V3 no collateral change | PASS: 469-path delta is exactly **five** changed paths (`init`, `kernel_fs.img`, `boot-config/autoload.toml`, `boot-config/services.toml`, `cellos-kernel`); `manifest.json` added; `system.toml` identical (**deviation 1**); candidate (`/tmp` and durable), archive, `build/vicell-x86.iso`, `kernel/src/main.rs` unchanged. Re-run after V6 (`after-v6`) and after V7 (`after-v7`): ALL OK |
| V5 CI ISO | PASS: `X86_ISO_ROOT=$S/iso-root make-iso-ci.sh $S/vicell-x86.iso` with the **default kernel** (helper admitted it). ISO `a22e99f7…` (32,055,296 B, differs from `d2070319…` only by timestamps); the extracted trees of both ISOs are **file-for-file equal** (`boot/kernel.elf` `dfbe8c55…`, `limine.conf` `362a5a50…`, Limine files) |
| V6 one P04 terminal run | PASS: `qemu-phase04-terminal-test.sh` default mode, TCG, once (1843 s): **494 checks, 202 driver markers**, exit 0; promoted files byte-identical afterwards (`after-v6`). Not qualification of the promoted bytes (fixture kernel) |
| V7 L6 once | PASS: `p7_l6.py` on `$S/vicell-x86.iso` (11 non-desktop consumers on the desktop ISO, `desktop-x86` on it with TCG, `iommu-dmar` PC lane on candidate `pc.iso` `3cdf536a…`) and the CI boot check, classified against the S10-3.14 before-baseline: **39 pass→pass, 1 pre-existing fail→fail (`xhci_injected_key_decodes_in_cell_x86`), 4 skip (x86-32 handoff), 1 ignored (virtio block init), 0 regressions**; CI boot check passes on the desktop ISO. The new configuration (non-desktop consumers on the desktop ISO) changed no result |

**Lanes run:** V6 (P04 terminal default mode) and V7 (L6: `x86_64-boot`, `nvme-x86`, `virtio-x86`, `pcie-multibus-x86`, `handoff`, `ahci-x86`, `xhci-x86`, `serial-x86`, `nic-x86`, `igb-x86`, `iommu-dmar-x86`, `desktop-x86`, `qemu-x86_64-test.sh`). No L0/L1–L5 lane was rerun. **Citable as covering the promoted desktop bytes** (identical hashes): L0 desktop 6/6, L4 6/6 at 290 (14 payload-matched cells), the S10-3.14 desktop L6, V1–V3/V5–V7. **Not citable:** L0 headless, L1, L2, L3, L5/L5f, headless/PC L6 ISOs, fixture-kernel edges (headless bytes; no shared binary is byte-identical across the profiles).

**Deviations for CP2:**
1. The V3 expectation in the harness was first written as six changed paths; `boot-config/system.toml` is byte-identical in the old and promoted sets (`74ecea5c…`), so five paths change. The first `p4_verify.py` run therefore reported a harness-expectation failure, which was corrected in the harness (not in the product bytes) and rerun; both outputs are described here, the corrected one is `p4-verify-v1-v3.json`. No product state changed between the two runs.
2. `promotion.json` is kept in `reports/s10-5-evidence/`, not in the embedded directory (it would add an untracked file beside the kernel inputs).
3. P5 and L6 write only to `/tmp/cellbox-s10-5-scratch`; `build/vicell-x86.iso` and `build/x86-iso-root` (protected, tracked or in the 469 set) were not written, so the CI ISO is a scratch path, not `build/vicell-x86.iso`.
4. The durable candidate copy is 195.6 MB (S10-5.10 estimated ~125 MB), because it keeps the whole candidate directory.
5. The negative control "`make-iso-ci.sh` default path refuses the real quarantined kernel" was captured from the live run before P3 (transcript above), not in a saved log; the helper control is re-recorded on the archived files.
6. The P04 terminal run used a fresh `mktemp /tmp/cellbox-phase04-*` work directory and a sanitised environment (no fixture-signing or prod-key-override variables), as the S10-3 default-mode runs did.
7. S10-2b design points differing from the approved text were those approved at CP1: the helper's ELF binding (S10-5.4) and L6 on the desktop ISO (S10-5.6 V7). The packager's G1 reopening has no new precondition code (delegation to the assembler), as proposed.
8. The kernel `target/…/cellos-kernel` is a copy, so any later `cargo build -p cellos-kernel` or `run-desktop.sh --rebuild` replaces it; the qualification record covers only the recorded hash.
9. The records edit changed the document digests the S10-3 CBX-01/02 audit pins: `command-catalog.md` (§6 only; ledger block, §3 and contracts untouched) is now `f06f38ce7447758f7b49e58d6e98dc8309b583515bf1ffd3608e665011a5fd0f` (the S10-0 and Phase 04 closure digests quoted in older records are historical). **Acknowledged by the user at the S10-5 CP2 (2026-10-29)** and recorded in `contract-deltas.md`. `acceptance.md`, `plan.md`, `phase-04`, `phase-10`, `plan-portfolio.md`, `rollout-containment.md`, `verification.md`, `docs/cellbox-cli.md` and `CHANGELOG.md` were edited with the edit tool only.
10. The plan, Phase 04 and Phase 10 frontmatter statuses (`completed-private-evidence`, `closed-partial-private-evidence`) were set as S10-5.9 proposes before CP2; **CP2 is granted, so they stand** (no revert).

**CP2 record (user, 2026-10-29, relayed by Main): APPROVED S10-5**, including deviations 1–8 above and the acknowledgement of the catalog digest `f06f38ce…`. Documentation-only recording: no VM, build, source or canonical change. Not approved / unchanged: CBX-27 (unmet), CBX-23/24/25 (partial), Phase 10 criteria 1 and 4 (open), physical Intel, VMX/EPT, production, remote, KVM or crash-durability claims, cloud formats, non-Intel recipes (refused), the 38 CELL-CONTRACT-0024 lock relations. Commit is a separate step; nothing was staged or committed.

**Not done / not claimed:** no measurement, no CBX-27 closure; no CI run; no non-Intel, cloud, KVM, physical, VMX, production or remote claim; no commit, stage, push or tag. §1.1 above is the historical pre-S10-2b guard table; the as-built state is the P2 table here.

---

## 3. Decisions for the user

| ID | Question | Options | Recommendation |
|---|---|---|---|
| D10-1 | What does "release" mean here? | **A** a canonical **dev-posture** repository image (dev cell, policy and RNG keys, QEMU-qualified); **B** a production-posture image | **A.** B needs the production root and the protected signer (locked, §1.7). |
| D10-2 | Which key signs the canonical cells? | **A** the dev key `0x43×32` through the S10-2a route (matches the kernel's default `dev-signing-key`); **B** a private per-build seed (needs the fixture `phase02-terminal-profile` kernel, so the image is no longer canonical); **C** a release/KMS key (blocked by locks) | **A.** |
| D10-3 | Does the GUI Terminal ship in the canonical desktop profile? | **A** yes: desktop profile with `/bin/terminal` and `cellbox sh` (GUI 290 and S3/C5 evidence; Delete-key defect listed); **B** headless only now, desktop canonical stays blocked | **A**, if the L4 6/6 runs pass on payload-matched bytes. |
| D10-4 | Should `/bin/config` start by default without `ai`? | **A** keep it `ai`-only, so `export` reports `config unavailable` (status 1) on canonical; **B** default-on for every image (shared `generate-boot-config.py:43`, affects all plans' images); **C** on only when the image ships `/bin/cellbox` | **A**: no new boot service outside Cellbox scope; the Q right is already lane-proven. **C** is the alternative if `export` must work on canonical. |
| D10-5 | How is `/bin/cellbox-ssh` packaged? | **A** a second full FAT copy (about +3.08 MB; as in the P09 lane); **B** FAT cross-linked entries (an invalid FAT; rejected); **C** omit the alias, so `ssh` is unavailable | **A.** |
| D10-6 | Which retained workers go in canonical images? | **A** keep the current ps1 set (`ls cat echo ps kill shutdown`; CI aligned to it); **B** all 11 sys plus 6 net workers; **C** drop workers (requires unfreezing the catalog) | **A.** |
| D10-7 | What happens to the legacy residue (`/bin/shell`, `/bin/sshd`, `sshd` autostart)? | **A** fresh assembly drops it, with the absence asserted; **B** keep `sshd` | **A**: CBX-25 forbids starting it. |
| D10-8 | How is the Phase 09 partial treated in the release notes? | **A** ship ssh (password, known-host and confirmation); `--auth publickey` is refused; CBX-23/24/25 marked partial with exact blockers; **B** hold ssh back | **A.** |
| D10-9 | ISO and cloud images? | **A** reopen G3 for the qualified kernel only (CI ISO and its 12 consumers); cloud qcow2/vmdk/KubeVirt stay out; **B** also cloud formats | **A.** Distribution formats are a separate decision. |
| D10-10 | Non-Intel recipes (RV64, aarch64, CI jobs)? | **A** stay refused (ADR-0022; their CI jobs stay red); **B** reopen as compatibility maintenance with no qualification claim | **A**, with the red jobs recorded. |
| D10-11 | CBX-10's "explicit subcommand": argv `/bin/cellbox <command>` does not exist (`main.rs:132-141`) | **A** record CBX-10 as partial and amend the catalog/doc wording to "explicit form inside `cellbox sh`"; **B** add a slice implementing argv one-shot dispatch | **Decided 2026-10-26: B, as slice S10-0** (§2), before the cutover. |
| D10-12 | RNG posture of the canonical kernel | **A** keep `dev-weak-rng` and document that ssh needs RDSEED/RDRAND; **B** build canonical without the weak fallback (fail-closed; affects net TLS on CPUs without RDRAND) | **A**, with the limitation stated. |
| D10-13 | What counts as qualification? | The rule in S10-3: L0 is required, L1–L5 count only with payload match, TCG is mandatory, GUI needs 6 runs | Approve as written. |
| D10-14 | Archive the quarantined canonical image before promotion? | **A** archive with hashes outside canonical paths; **B** overwrite | **A.** |

### 3.1 Recorded decisions (user, 2026-10-26, relayed by Main)

- **Slice plan CP1 approved:** the slices and the order in §2, with S10-0 added first. Each slice keeps its own CP1 and CP2.
- **D10-1..D10-10 and D10-12..D10-14: every recommendation taken.**
  - D10-1: dev-posture repository image.
  - D10-2: dev key through the S10-2a route.
  - D10-3: the GUI Terminal ships in the desktop profile.
  - D10-4: `/bin/config` stays `ai`-only.
  - D10-5: `/bin/cellbox-ssh` as a second full FAT copy.
  - D10-6: the current ps1 worker set.
  - D10-7: fresh assembly drops `/bin/shell` and `/bin/sshd`.
  - D10-8: ssh ships with password and known-host only; CBX-23/24/25 are partial.
  - D10-9: G3 reopens for the qualified kernel only; no cloud formats.
  - D10-10: non-Intel recipes stay refused.
  - D10-12: keep `dev-weak-rng` and document the limitation.
  - D10-13: the S10-3 qualification rule.
  - D10-14: archive the quarantined image before promotion.
- **D10-11: B**, implemented as slice **S10-0** (`/bin/cellbox <command>` for catalog commands). **S10-0 CP1 approved 2026-10-26** (user, relayed by Main): DS0-1..DS0-6 as recommended.
- **S10-1 CP1 approved 2026-10-26** (user, relayed by Main): D11-1..D11-6 as recommended. Implementation and scratch evidence: S10-1.7; CP2 approved 2026-10-26 with deviations 1–11 (see the status line).
- **S10-2a CP1 approved 2026-10-26** (user, relayed by Main): D2a-1..D2a-4 as recommended. Implementation and scratch evidence: S10-2a.8; CP2 approved 2026-10-26 with deviations 1–10.
- **S10-3 CP1 approved 2026-10-26** (user, relayed by Main): D3-1..D3-7 as recommended, with D3-5 (`COMPLETION-QUEUE: FAIL`) recorded as a known non-blocking defect. The run stopped at the L6 regression rule (S10-3.10).
- **S10-3 decisions (user, 2026-10-26):**
  - (a) fix lone-LF submission in the product (new slice S10-3a);
  - (b) fleet-profile canonical kernel with Tier 2 DISABLED;
  - (c) UART input to the focused GUI Terminal on desktop is accepted and documented, with serial qualified on headless and desktop through the GUI;
  - (d) S10-3.2 marker forms amended.

  S10-3a: **CP1 approved 2026-10-26** (D3a-1..D3a-4); **CP2 approved 2026-10-28**, including recorded deviations/supersession (e). The historical fleet-candidate SC1–SC3 stop remains in S10-3.11.
- **S10-3 decisions (e)–(g) (user, 2026-10-26):**
  - (e) S10-3a supersedes Phase 02 amendment A2 item 1; the P02 check is updated to `ctrl_j_submits`, and the rerun PASSes;
  - (f) and (g) go to a new slice **S10-3b** (`sleep`/blocked-sink Ctrl-C → 130; no false ENOENT from `cat`).

  S10-3b: **CP1 approved 2026-10-26** (D3b-1 B, D3b-2 A, D3b-3 A); **CP2 approved 2026-10-28**, deviations 1–4. Historical `9c4d16eb…` SC4 remains in S10-3.12.
- **D3-8 (user, 2026-10-26): C, at once.** S10-3c **CP1 approved 2026-10-27** (D3c-1 A, D3c-2 B, D3c-3 A, D3c-4 A, D3c-5); **CP2 approved 2026-10-28** with recorded deviations. Historical SC5 remains in S10-3.13.
- **D3c-6 (user, 2026-10-28): investigate Identify/completion; product fix gated on new CP1.** Mechanism confirmed, original-event attribution probable; S10-3d CP1 D3d-1 A/D3d-2/3/4 and later **S10-3d plus full S10-3 CP2 approved 2026-10-28** with all disclosures, including SC6's audit-only correction. S10-4 **CP1 approved 2026-10-28** for the original method above; no S10-5 promotion authorized.

---

## 4. What stays blocked

- **Tier-3 guest SSH endpoint**, the guest half of CBX-23 and CBX-25 (D79-2), behind locks `tier3b-x86-vtx`, `tier3b-vm-hardening-compat` and `tier3b-finish-g5-lite`.
- **S9-5 publickey/KMS custody** (D79-14). `--auth publickey` stays refused; pipeline exec and exact non-UTF-8 stdin (S9-3 deviation 3) and the publickey half of CBX-24 stay open. Behind `kms-silo-production-root` and `phase4-dev-reference-authority`.
- **Production or release key, production posture, distribution:** cloud images and cell-store installers (D10-1/2/9; locks in §1.7).
- **Physical Intel, VMX/EPT, remote C2C:** no claim from QEMU TCG/KVM (`x86-pc-lane` P07 procurement; controlling plan P09/P10).
- **CellosFS crash safety:** no `CommittedDurable` path; first trust is "synced, not crash-proof" ([cellos-fs crash-safety plan](../261025-cellosfs-crash-safety/plan.md)).
- **Known defects outside this phase:** D60-3 clock units and the GUI Delete key. They are listed, not fixed; each needs its own CP1.
- **Non-Intel canonical recipes** (D10-10 A).
- **All 38 CELL-CONTRACT-0024 lock relations remain.**

## 5. Requested now

- **S10-3a/b/c/d and S10-3 CP2 granted by user 2026-10-28** with all recorded deviations/disclosures; no further request for those already-granted approvals.
- **S10-4 limited-result CP2 granted by the user 2026-10-28:** accepts S10-4.8's actual static/scoped-signing/six untraced measurements/headless controlled streams, partial headless memory and recorded deviations, including the retained unsupported-observer stop. It does not approve unrun desktop observation, complete workload peaks, unavailable historical runtime equivalence or extra debugger control. **CBX27/criterion4 remain unmet**; independently demonstrated safe observer semantics or a separately approved disposition is still required.
- **S10-4.15 actual partial-result CP2 requested:** accept191distinctofflinePASSaggregate with all failed iterations disclosed, ONEdesktop's actual GUI/idle/typedexact/Entercommit/transport/recovery and retained finalscreen **observability STOP**, plus partial bounds only. Full two-frame/newprompt/status0/fullschedule and complete memory remain unproved. Next complete-output-route feasibility is **proposed only**, requiring separate CP1; no automatic new candidate run, hiddenroute, proxy, guarddrop or deadline increase. See S10-4.15 and `runtime/observer-gui-temporal/next-protocol.json`.
- **S10-4.17 limited-result CP2 GRANTED by the user 2026-10-29 and S10-4 CLOSED PARTIAL:** accepts Class B `SUCCESS_METRIC_ONLY` (269-test single clean gate; ONE desktop run proved two frames/prompt/`st=0` from the Terminal `feed` ledger; retained schedule then stopped at the latent `Surface.status()` st=N check, left unfixed) and all exercised partial memory bounds, diagnostic findings S10-4.9..S10-4.17, offline iterations and deviations 1-4 at their stated ceilings. Not approved/unchanged: full-workload peaks, RSS, exactstack, kernel stack, matched historical runtime, remaining observed phases. **CBX27/criterion4 remain unmet**; S10-5/canonical promotion/containment need their own CP1. See S10-4.17 section 10.
- *(Historical, superseded by the S10-5 CP1/CP2 below.)* **S10-5 remained blocked** after the limited-result CP2: CBX27/criterion4 were unmet and its own explicit promotion approval had not been given.
- **S10-5 CP1 APPROVED by the user 2026-10-29 (relayed by Main): DS5-1 (promote while CBX-27/Phase 10 criterion 4 stay unmet), DS5-2 (desktop profile), DS5-3 (copy the qualified desktop kernel; G3 by content plus ELF binding), DS5-4 (S10-2b scope A), all as recommended.** Execution was authorised exactly as written in S10-5.5 (P0–P9, then V1–V7), stopping on any hash mismatch, lane/L6 failure or unexpected guard behaviour. No stage, commit, push or tag; the user's `kernel/src/main.rs` VMX edit and the protected `build/` stayed untouched. **CBX-27/criterion 4 stay unmet.**
- **S10-5 CP2 GRANTED by the user 2026-10-29 (relayed by Main):** accepts S10-5.13 including deviations 1–8 and acknowledges the new `command-catalog.md` digest `f06f38ce…`. Nothing further is requested for S10-5. Unchanged/not approved: CBX-27 unmet, CBX-23/24/25 partial, Phase 10 criteria 1 and 4 open, no physical/VMX/production/remote claim, no cloud formats, non-Intel recipes refused. Commit is a separate step.

## 6. S10-6 CBX-27 disposition — CP1 DRAFT (status AWAITING CP1; design only, nothing run, built, edited in source or committed)

**Row text (acceptance.md CBX-27):** "Measured before/after executable/image sizes, startup, memory and bounded long-stream behavior are recorded with feature/image provenance; no unsupported universal optimization claim." Phase 10 criterion 4 adds "equivalent feature sets" and "host/QEMU/physical ceilings distinguished".

**Evidence already retained (S10-4.8..S10-4.17, CP2-accepted at stated ceilings):** static footprint (B2 scoped-signing 980,528 B / unsigned 994,064 B vs. promoted desktop six binaries 3,261,680 B, +2,267,616 B, non-equivalent feature sets), six untraced startup/idle/top runs, headless controlled stream/cancel, observed partial allocator/stack-write bounds (fg 67,544 B, bg 25,272 B; stack write-attempt pages [45057,49152] B), mapped backing 10,199,040 B (not RSS), Terminal `feed` ledger finite-stream proof.

**Missing and why it is not reachable by more observer work:**
| Element | Blocker |
|---|---|
| RSS/residency | no guest-side provider; needs kernel/ABI change (protected, separate CP) |
| Exact stack / kernel-stack peak | needs instrumentation not present in the qualified bytes; write-attempt bounds only |
| Matched historical runtime comparator | B2 workspace source absent (`vm-runner` member); only static comparator exists |
| Full-workload peaks, remaining phases | schedule stops at latent `Surface.status()` st=N defect; S10-4 reruns not authorized |

**Options (decision DS6-1):**
- **A. Keep unmet (recommended default, no work).** Row stays unmet; S10-4 stays closed partial. Honest, zero risk; CBX-27 blocks Phase 10 criteria 1 and 4 indefinitely.
- **B. Disposition: accept as "recorded with provenance at stated ceiling".** Documentation only. The row's literal text asks for recording with provenance and no universal claim; both hold. Mark CBX-27 `met-at-ceiling` listing every null metric (RSS, exact/kernel stack, comparator, full-workload peaks) explicitly and keep criterion 4 "equivalent feature sets" unchecked or reworded by your explicit decision. Risk: it reads as scope reduction of "memory ... measured" and needs your explicit approval; nothing is silently narrowed. Criterion 4 and CBX-01..29 criterion 1 would still stay open because CBX-23/24/25 are partial.
- **C. New measurement program.** Separate CP1s for (i) a guest-side RSS/stack provider (kernel/ABI delta, protected paths, new candidate build/qualification), (ii) recovering or reconstructing the B2 source, (iii) fixing the `Surface.status()` schedule defect and one new observed run. Largest cost; only C can honestly close the memory half.

**Requested:** DS6-1 choice. Under B, approval also covers editing only `acceptance.md`, `phase-10-qualification.md` criteria text, `plan.md`/portfolio rows and CHANGELOG wording; no VM, code, rerun, commit or push. Nothing here is approved until you answer.

- **Status 2026-10-29:** CBX-27 remains **UNMET** until DS6-1 is answered.

- **DS6-1 answered by the user 2026-10-29: option A. CBX-27 stays UNMET; S10-4 stays closed partial; no further work, VM run or doc change authorized.**

## 7. S10-7 full CBX-27 closure program (DS6-1 option C) — CP1 DRAFT (status AWAITING CP1; design only; nothing run, built, edited in source or committed)

**Supersedes DS6-1 = A at the user's request (2026-10-29: "phương án C giải quyết trọn vẹn CBX-27").** CBX-27 closes only if every element below is actually evidenced non-null; any element that cannot be is reported as such and the row stays unmet. Nothing here is approved until the user answers D7-1..D7-5.

**Goal.** Close the five nulls retained by S10-4: (1) RSS/residency, (2) exact user-stack peak, (3) kernel-stack peak, (4) matched historical runtime comparator (B1/B2 startup/idle/top/heap/stack on equal harness), (5) full-workload allocator/stack peaks through the complete original observed schedule (finite, collected-cat, live-no-cat, cancel, rg, top, idle, `complete` snapshot).

### Slices (strict order; each stops with evidence on a failed assumption, no auto-retry)
| Slice | Deliverable | Touches | Gate |
|---|---|---|---|
| S10-7.0 | Retained-only feasibility probe: can RSS, user-stack depth and kernel-stack depth be derived by host introspection of a paused QEMU guest (page-table walk of the task CR3: present+accessed/dirty pages; stack-region scan for first non-zero/canary beyond RSP; kernel-stack region scan) using the existing PTE-witness machinery? Does kernel zero stack pages at allocation (else depth scan is invalid)? | read-only: `runtime/*` evidence, kernel stack/frame allocator source, candidate ELF | Report with go/no-go for 7.2; no VM. If scan validity cannot be proved, 7.2 must use option D7-2-B (kernel delta). |
| S10-7.1 | Runnable B1/B2 comparator images: resolve the retained signing refusal (F1: unallowlisted unsafe token in unrelated `cells/services/ocel-quickjs/src/native_layout.rs`) **only inside the disposable historical worktree**, assemble B1/B2 headless+desktop disks, boot-check. | disposable `/tmp` worktree only; no canonical/candidate/user VMX path | Both comparators reach the same prompt/idle state under the untraced harness; diffs and the guard decision archived; comparator labelled non-pristine. |
| S10-7.2 | Memory provider. **D7-2-A (recommended, no protected change):** host-side snapshot introspection per 7.0. **D7-2-B:** guest-side kernel/ABI counter (per-task resident pages, user and kernel stack high-water) as a new Law-1 delta: own CP1 digest, owner/codec/allocation dossier, candidate rebuild, re-qualification and a second canonical promotion CP1/CP2 (S10-5 pattern). | A: new sibling `runtime/observer-memory/` only. B: protected kernel source (user's uncommitted `kernel/src/main.rs` VMX edit must be preserved) | A: offline tests of walker/scan on retained snapshots incl. known-answer fixtures. B: its own gate list. |
| S10-7.3 | Harness defect fix: `Surface.status()` must accept the whole-screen history containing `st=0` and `st=130` (select the row for the current command epoch), keeping fail-closed behaviour for ambiguous/duplicate/missing status. Offline tests, then the single clean-gate rule (retained 269 + new, one invocation, 0 fail/error/skip). | new sibling host sources only | Clean gate before any VM. |
| S10-7.4 | Measurement runs: ONE candidate desktop observed run of the complete original schedule with original deadlines (120 s/60 s/900 s), memory provider armed; B1/B2 comparator runs on the same harness for the shared command subset; headless candidate run for the headless profile. | VM runs, disposable disks | Classes A–E pre-declared (A = all phases + all five elements non-null). Per-run attempt budget stated in D7-3; no fix-and-rerun beyond it. |
| S10-7.5 | Report + records: `summary.json`, per-element table with unit, provenance (candidate/B1/B2 hashes, profile, QEMU/TCG ceiling), comparator non-equivalence list, regressions as well as improvements; update acceptance/plan/portfolio/CHANGELOG only if evidence closes the row. | docs only | CP2 request. |

### Honest limits that remain even on full success
- All runtime numbers are QEMU/TCG guest numbers; no physical Intel/VMX/production claim. RSS means guest-resident mapped+touched pages, not host RSS and not a physical-memory figure.
- B1/B2 are non-equivalent feature sets (Cellbox adds rg/edit/outbound SSH/top record+TUI/typed pipelines/Ratatui). The comparison is the shared command subset (`sh`, `ls cat echo ps kill`, idle redraw), with explicit regressions; no universal optimization claim. The file footprint already grew (+2,264,888 B headless / +2,264,952 B desktop for six files); a closed row does not mean "smaller".
- Criterion 1 of Phase 10 still stays open while CBX-23/24/25 are partial; criterion 4 can be checked only if CBX-27's measurement text is fully met.
- Cost/risk: S10-4 consumed 17 sub-slices and many failed runs on this harness; 7.4 is the same class of schedule. Hence the explicit attempt budget and stop rule below. D7-2-B adds a protected kernel change plus re-promotion and is substantially larger.

### Decisions requested
| ID | Question | Options | Recommended |
|---|---|---|---|
| D7-1 | Approve S10-7.0 first as a no-VM read-only probe before committing to the rest? | yes / no | yes |
| D7-2 | Memory provider route | A host introspection (no protected change); B guest kernel/ABI delta (new CP1+re-promotion) | A, falling back to B only by a new CP1 if 7.0 says no-go |
| D7-3 | Run budget for 7.4 | exactly one desktop + one headless candidate run and one run per comparator profile, no rerun; or N reruns with offline fix between | one each, no rerun (matches S10-4.17 rule) |
| D7-4 | F1 refusal in the disposable B1/B2 worktree | allowlist/skip only in that worktree with the diff preserved; or keep B2 runtime comparator unavailable | disposable-worktree-only exception, labelled |
| D7-5 | If any element ends null | row stays unmet and S10-7 closes partial; or open a further CP1 | stays unmet, new CP1 required |

**Preservation (all slices):** 546 protected paths (469 canonical + candidate + user VMX) and all prior archives/manifests/compressed trace byte-exact; new sibling directories only; no product/candidate/canonical edit under D7-2-A; no commit/push without a separate step; no deadline increase, guard drop, hidden control or coverage waiver.

- **Status 2026-10-29:** S10-7 is **proposed only**. CBX-27 and Phase 10 criteria 1 and 4 remain **UNMET**.

- **S10-7 CP1 APPROVED by the user 2026-10-29 exactly as recommended: D7-1 yes, D7-2 A (host introspection; B only by new CP1), D7-3 one run each/no rerun, D7-4 disposable-worktree-only F1 exception labelled, D7-5 row stays unmet if any element null. Execution order: 7.0 (read-only, no VM) then STOP for go/no-go before 7.1.**

### S10-7.0 result (2026-10-29): retained-only/read-only probe, no VM, no source edit — PARTIAL GO, one new decision (D7-6)
Sources read: `kernel/src/task/stack.rs`, `kernel/src/task/elf_prepare.rs`, `kernel/src/task.rs` (stack-baseline emitter), `observer.py` page-walk helper. Findings, each with its ceiling:
- **RSS/residency: derivable by construction, no kernel change.** Stack allocation is eager, physically contiguous, identity-mapped (SAS; `stack.rs` allocate comment: "no amount of lazy commit helps"), no swap or demand-paging path found by grep of `kernel/src`. Mapped backing therefore equals resident backing for stacks/ELF; observer `walk_page` already counts these (10,199,040 B in prior snapshots). Report as "resident = mapped, eager SAS" with the heap arena reported separately (6 MiB reservation, allocator `Heap.used` bounds already measured). Ceiling: guest-resident bytes under QEMU/TCG, not host RSS. Static claim from source reading, not yet proven on the qualified bytes: 7.2 must add a known-answer check (snapshot page count vs allocator/stack sizes).
- **Kernel stack exact depth: scan valid on the ELF spawn path.** `elf_prepare.rs:177` zeroes the whole kernel-stack usable range at spawn, so the lowest non-zero byte below the saved trap frame bounds the depth (lower bound: zero-valued stores invisible). Unverified: that `cellbox` and the workers all spawn through `elf_prepare` rather than `scheduler.rs:805/1061` (7.2 must prove per cell).
- **User stack exact depth: NOT derivable by scan.** No zeroing of user-stack frames in the ELF path (`elf_prepare.rs:182`), and stale non-zero frames are possible; this is why S10-4 only has page-granular write-attempt bounds. User/kernel A/D bits are preset (`ACCESSED|DIRTY` in `stack.rs:190-204`), so they cannot show touched pages.
- **Existing exact mechanism, but in a different binary:** with Cargo feature `test-hooks` the kernel already primes both stacks with a sentinel and emits `[stack-baseline] name=cellbox ... used_bytes=...` (`task.rs:712`, `stack.rs:319`). That is exact user+kernel watermarks with no new kernel source, but the kernel ELF differs from the qualified candidate.
**Go/no-go:** 7.1, 7.3 unaffected (GO). 7.2 is GO for RSS and kernel-stack depth. User-stack exact depth needs one of: **(i)** host debugger pattern-prime of the user-stack usable range before first run = a guest memory write, which every earlier S10-4 CP forbade; **(ii)** a separately built, labelled `test-hooks` measurement kernel (same Cellbox ELF, different kernel; disposable build, no source change, no canonical write); **(iii)** guest kernel/ABI delta (D7-2-B, new CP1 + re-promotion). Recommendation: (ii), measured Cellbox/cell stack numbers reported as "test-hooks measurement kernel, not the promoted kernel"; it also still needs one proof that the sentinel does not alter stack_pages_for/spawn paths. D7-6 (user-stack route i/ii/iii) is requested; no 7.1+ step has started.

- **D7-6 answered by the user 2026-10-29: route (ii), separately built labelled `test-hooks` measurement kernel (same Cellbox ELF; disposable build; no source change; no canonical write). 7.0 go/no-go = GO for 7.1, 7.2 (RSS, kernel-stack scan, user stack via test-hooks watermark), 7.3.**

### S10-7.1 pre-check (2026-10-29, read-only): new decision D7-7 before any build
- Retained B2 evidence has **no signed or unsigned ELFs** (only logs, sizes, `b2-{manifest,lock,all-authorized-deltas}.patch`, `build-b2.py`, `signing/sign-mirror.py`); the disposable worktree `/tmp/cellbox-s10-4` no longer exists. A runtime comparator therefore needs a full replay: detached worktree at `3471028bc8b4db14275553b4740c17e24fa4f32b`, apply the three retained patches, `--locked --offline` build with the pinned nightly, scoped dev-signing (the F1 refusal on `ocel-quickjs/native_layout.rs` is the D7-4 exception), plus an **image to boot them in**.
- The boot question is the open design choice: **(a) historical-matched:** build that commit's own kernel+`init`+image and boot it (true "before", but needs a historical kernel/image build and signing route that S10-4 never exercised; may fail on toolchain/guard drift); **(b) mixed:** run the B2 `shell`/`ls cat echo ps kill` ELFs on the current qualified kernel (cheap, but ABI/launch-contract compatibility is unproven and the result would compare userland only on a different kernel, so it is not a "before/after" of the system). Both are reported with their ceiling; (a) is the only one that supports the words "historical runtime comparator".
- Run budget stays D7-3: one run per comparator profile, no rerun. Not started.

### S10-7.1 result (2026-10-29): B2 historical-matched images BUILT (no VM run)
Replay at commit `3471028bc…` with only the three authorized deltas (manifest patch, lock patch, scoped F1 signing; whole-workspace F1 still refuses, retained). Own kernel/init/`kernel_fs.img`/ISO/qcow2 for headless and desktop; 62 static checks pass (listed in `result.json`). Artifacts in `/tmp/cellbox-s10-7/b2-image/` (hashes in `reports/s10-7-evidence/b2-image/manifest.json`, evidence 74 files). Hashes: headless ISO `1321f5ec…`, kernel `28ba57c8…`; desktop ISO `780ffa3f…`, kernel `5959acd7…`; six signed B2 ELFs equal the S10-4 B2 hashes. Disclosed deviations (7): scripts replayed step-by-step instead of run (cannot take `--locked --offline`/scoped signing), desktop base is tracked historical bytes with only `ocel`/`service-net` rebuilt, six B2 tools substituted into the desktop image (tracked shell was 876,200 B not 881,120 B; reversible on request), host `limine` compiled from the worktree, headless qcow2 extra, PIC/kernel rustflags from CI template, C env matches S10-4. **Boot behaviour unverified** (tracked init/services vs fresh kernel not exercised). S10-7.3 offline fix `runtime/observer-memory/status_select.py` (epoch-aware status row, fail-closed on missing echo/no row/duplicate) with 6 tests PASS.

### S10-7.2 result (2026-10-29, offline/static; no VM run)
- **Measurement kernels BUILT** (`reports/s10-7-evidence/test-hooks-kernels/`, disposable sets under `/tmp/cellbox-s10-7/measure/`): candidate desktop `360a5c10…` (control without hooks = promoted `dfbe8c55…`, byte-identical), candidate headless `3e279f9e…`, B2 desktop `8d876239…`, B2 headless `19a30055…`; only the `test-hooks` feature differs, `kernel_fs.img`/`init`/ELFs identical to non-hooks builds, ISO trees equal except `boot/kernel.elf`. Disclosed: B2 desktop first attempt (build-std) discarded and retained; embedded `init` unaligned in hooks kernels (transient 406,976 B heap copy at spawn).
- Priming: 0xA5, `[base+2 pages, top)` for kernel and user stacks of every task on every spawn path (cellbox/workers/B2 shell 16 pages, vfs/terminal/others 64). `[stack-baseline]` emits only at first sample after tick 1500 and never at end of workload, so final watermarks come from host scans of live stacks (implemented below). B2 hook equivalence inferred, not line-by-line reviewed.
- **Hooks side effects:** boot-time corpora (about 25 prepares of canonical init, handoff/shootdown fixtures, forced Tier-2 admission log `S22-X86-ADMISSION-ENABLED: FAIL`, AP-15 asserts) that can panic the boot and have never run against the 407 KB canonical init; per-tick name scan after tick 1500. Nothing found that changes cellbox behaviour beyond fill pattern and CPU overhead (timing results from hooks kernels are NOT startup comparators; untraced timings stay on the non-hooks bytes).
- **Memory observer** `runtime/observer-memory/` (`guest_view.py`, `stack_watermark.py`, `resident.py`, `memory_snapshot.py`, `test_memory.py`): 35 tests PASS in one invocation incl. status_select (kernel scan semantics mirrored and checked against a transcription of the kernel loop on 200 random images). Not wired into a live run; synthetic fixtures do not prove a live QEMU physical read returns identical bytes.
- **Not started:** 7.4 runs. New decision D7-8 (boot smoke of hooks kernels before the one-run budget) is requested.

### S10-7 D7-8 boot smokes (2026-10-29; user approved one boot-only smoke per measurement set; evidence `reports/s10-7-evidence/boot-smoke/`)
- **cand-headless READY** (`Cellos > `), **b2-headless READY** (7.26 s), **cand-desktop READY** (5.51 s; 1280x800 screendump, 852 colours). Both candidate sets log `S22-X86-ADMISSION-ENABLED: FAIL` (expected forced-admission hook log); `COMPLETION-QUEUE: FAIL` and `X86-VMM-SMOKE: FAIL` appear in all four sets and boot continued where it reached ready.
- **b2-desktop FAILED: kernel panic**, no ready marker: `USER: OOM: cell heap exhausted — size=4341072`, task 5 exit 238, then `tlb_shootdown.rs:389 shared mapping invalidation unconfirmed: Timeout{hart:0,epoch:71118}`. Cause NOT diagnosed (one attempt per set; no rerun). Candidates not separated: `test-hooks` side effect, B2 desktop image as assembled in 7.1 (tracked historical services + substituted six tools, never booted before), or the 7.1 deviation 3 shell substitution. A non-hooks control boot of B2 desktop has not been run.
- Driver note: smoke driver raised `SameFileError` after the cand-headless boot; its `result.json` was reconstructed from `serial.log` (upper-bound timing); no boot repeated. Desktop smoke used `-display none`, no hostfwd, VNC 127.0.0.1:37, private QMP and disk copies.
- **Consequence:** B2 desktop measurement set is a recorded failure; the desktop historical comparator is null unless the user separately approves a diagnostic boot. Next (not started): 7.4 harness wiring and the single measurement runs for cand-desktop, cand-headless, b2-headless.

- **D7-9 answered by the user 2026-10-29:** ONE non-hooks diagnostic boot of the B2 desktop set (boot-only, same smoke rules; no rerun) AND continue to 7.4: wire the memory observer into the harness, one clean offline gate, then exactly one measurement run per boot-able set (cand-desktop, cand-headless, b2-headless; B2 shared command subset only).

### S10-7.3/7.4 results (2026-10-29): offline gate PASS; three single runs; CBX-27 STAYS UNMET; limited-result CP2 pending
- **B2 desktop diagnostic (D7-9, one non-hooks boot) FAILED identically:** panic after 16.28 s, VFS-service-cell (task 5, inferred from log order) `OOM: cell heap exhausted size=4341072` then `tlb_shootdown.rs:389 Timeout{hart:1,epoch:8785}`. So the failure is in the 7.1 B2 desktop image as assembled, not the hooks. **B2 desktop comparator = null; no further attempt authorized.** Evidence `reports/s10-7-evidence/b2-desktop-diagnostic/`.
- **Offline gate (ONE invocation, 394 tests, 0 fail/error/skip):** 268 retained (one retained test excluded: the S10-4.17 pre-run preflight is false by design now: run token exists, 431,927,301 B trace and `/tmp/cellbox-s10-3d-resume` gone, census rewritten by S10-5; not mocked) + 35 memory + 91 new. Census differs from `preservation-after.json` in five canonical paths that match S10-5 `promotion.json` and 76 retained-candidate temp paths with byte-identical cache twins; `kernel/src/main.rs` unchanged; 43 frozen sources and 25 prior archives byte-identical. Iterations incl. three whose raw output was deleted are in `offline-iterations.json`. Boot argv risk disclosed (runs used the retained 256M q35 one-vCPU argv, smokes used 2048M/-smp 2).
- **Runs (one each, no rerun; evidence `reports/s10-7-evidence/measurement-runs/<set>/`):**
| Set | Class | Result |
|---|---|---|
| cand-headless (test-hooks kernel) | C | 9 memory snapshots ok/exact; STOP `timeout: new interrupt prompt` in `top_idle()->interrupt()`; no `complete` snapshot; masked by observer defect `TypeError: MemoryMixin._stop() missing 1 required positional argument: 'message'` |
| cand-desktop (test-hooks kernel) | C | 6 snapshots ok/exact; STOP `timeout waiting for runtime new GUI prompt` in `streams()->interrupt()->gui_fresh()` (10 s wait); same TypeError; missing `terminal-stream-cancelled`, `rg-cancelled`, `interactive-top-idle`, `complete` |
| b2-headless (hooks kernel, shared subset) | B | COMPLETE, 7 snapshots; B2 allocator peak not measured (no heapwatch); ls/cat/echo/ps/kill are shell builtins |
- **Actual measured values (guest-resident under QEMU/TCG, test-hooks measurement kernels, lower-bound sentinel depth, not the promoted kernel):**
  - Cellbox (candidate, both profiles): mapped-backed regions elf-load-0..4 = 2,043,904 / 581,632 / 163,840 / 7,344,128 / 8,192 B, user stack 65,536 B, kernel stack 65,536 B, heap arena 6,295,552 B; sum 16,568,320 B, distinct frames 10,264,576 B. Kernel stack depth 16,072 B constant. User stack depth 26,264 (initial-prompt) -> 35,352 (idle) -> 45,720 B from finite-stream on (to interactive-top-idle on headless). Allocator `used` at snapshot up to 229,432 B at interactive-top-idle (headless); allocator maxima per phase: finite 59,392 (headless)/67,464 (desktop), collected-cat 58,040/56,160, terminal-stream 55,056/53,144, rg-large 89,184 (headless), interactive-top 284,448 and top-idle 285,792 (headless).
  - B2 shell (headless): elf-load 696,320 / 57,344 / 20,480 / 1,052,672 / 8,192 B, stacks 65,536 B each, heap arena (HEAP_MEM) 1,052,672 B; sum 3,018,752 B, distinct frames 1,957,888 B; kernel stack depth 14,016 B; user stack 9,168 (prompt/idle) -> 15,248 (echo) -> 16,384 (ls) -> 32,096 B (cat, ps, complete).
- **Why CBX-27 still does not close (each unrun/unmatched, not inferred):** (1) cand-headless and cand-desktop did not complete the schedule (interrupt-prompt timeouts; the observer defect hid the final stop); full-workload peaks and the `complete` snapshots are absent; (2) the B2 comparator exists only for the headless shared subset (no allocator, no top/rg/finite-stream; desktop null), so there is no matched feature-equivalent before/after; (3) only the focus Cellbox was snapshotted (terminal, vfs, workers unmeasured; user-stack figure is a floor with one page excluded); (4) measurement kernels are hooks builds (timing not comparable; live physical read parity with fixtures only supported by these runs); (5) no RSS beyond "mapped-backed under eager SAS". **No rerun is authorized (D7-3/D7-5).** CBX-27 and Phase 10 criterion 4 stay unmet; S10-7 closes partial pending CP2. No commit beyond docs; nothing pushed.

## 8. S10-8 observer defect fix and ONE replacement run for the class-C sets — CP1 DRAFT (status AWAITING CP1; design only; nothing edited, built or run)

**Scope.** Only `cand-headless` and `cand-desktop` (class C in S10-7.4). `b2-headless` is class B COMPLETE and is NOT rerun; `b2-desktop` stays null (image defect, no repair authorized here). Fresh run tokens; S10-7's `RUN_CONSUMED-*` evidence stays untouched.

### Root causes (from retained S10-7.4 evidence, read-only)
1. **Observer defect (certain).** `MemoryMixin._stop(self, message)` in `observer-s10-7/s107_memory.py` overrides `threading.Thread._stop()` (the retained `Observer` is a `threading.Thread`, `observer.py:31`). CPython calls `_stop()` with no arguments on thread completion, hence `TypeError: MemoryMixin._stop() missing 1 required positional argument: 'message'` in both runs; it replaced the real observation status (`observation.json` says COMPLETE while `summary.json` says STOP). It is a name collision in new S10-7 code, not a retained-code fault. Fix: rename to a non-colliding name (e.g. `_wiring_stop`), add a regression test that a `MemoryHookObserver` thread starts, finishes and joins without error and that every wiring stop still raises `common.Stop`.
2. **Why both runs stopped (strongly supported, not proven causal).** Headless: after `interactive-top-idle` ended (end_ns 18324075902297) the observer kept servicing heap-watch stops at the same site (ordinal 9918, about one stop per 8.6 ms) until the original 10 s new-interrupt-prompt wait expired; total observer pause 85.17 s. Desktop: last native steps in `endless-terminal-stream`, GUI-prompt wait of exactly 10.0 s (start 18700590616373, end 18710600080476), pause 44.19 s. The retained S10-4.11 diagnosis already bounded the same effect (>=7.355 s of an original 10 s wait spent debugger-stopped). So the 10 s wall-clock recovery deadline measures the observer's throttling of the guest under TCG, not product recovery; I have NOT shown the product would have recovered in 10 unpaused seconds.

### Slices (strict order; stop on first unmet gate; no retry)
| Slice | Work | Gate |
|---|---|---|
| S10-8.0 | Offline rename fix + regression tests; static census of every other retained-method-name collision in `observer-s10-7` (all `def` names in mixins vs `threading.Thread`, `Observer`, `CoordinatedObserver`, `HookObserver`, `Machine` attributes) | collision census empty |
| S10-8.1 | Deadline treatment per D8-1 (below), implemented in a new sibling wrapper only, with tests: boundary behaviour, deadline never extended by more than the measured observer-pause, wall 900 s cap unchanged, pause accounting equals the retained `observer_pause_ns` method, no effect on any other timeout | tests + review of exact accounting |
| S10-8.2 | ONE clean offline gate (retained 268 + 35 + 91 + new, one invocation, 0 fail/error/skip, the one excluded stale preflight test disclosed again) + regenerated go/no-go with new tokens `RUN_CONSUMED-s108-cand-{headless,desktop}` | go/no-go all true |
| S10-8.3 | ONE run each, headless then desktop, original schedule/order, same measurement kernels/argv, no rerun | classes A-E pre-declared (A = every snapshot incl. `complete`, all exact flags true) |
| S10-8.4 | Report, records, CP2 request | docs only |

### Decisions requested
| ID | Question | Options | Recommended |
|---|---|---|---|
| **D8-1** | Recovery-wait deadlines (new interrupt prompt, GUI fresh prompt and the other original 10 s recovery waits) | **A** keep wall-clock 10 s unchanged and only fix the defect: lowest change, but likely reproduces the same class-C stop (both runs stopped by throttling); **B** budget = 10 s of unpaused guest time: wall deadline extended by exactly the observer pause accumulated inside that wait, hard-capped by the original 900 s and each phase's own maximum; deviation disclosed, product timing claims not made from these runs; **C** remove the heap-watch during recovery waits only (re-arm after prompt): changes coverage and the allocator events seen in those waits, weakens the full-coverage census CP2 relied on | **B** (a documented pause-credit is the only option that tests whether the workload completes; previous CPs forbade it unprompted, so it needs your explicit approval) |
| D8-2 | Run set | headless and desktop once each (recommended); or desktop only | both |
| D8-3 | Outcome rule | no further rerun for any outcome (recommended); class C again closes S10-7 partial | no further rerun |
| D8-4 | `b2-desktop` | stays null, no repair (recommended; image defect: VFS-cell heap OOM then TLB-shootdown panic, would need a separately approved image rebuild); or authorize one rebuild CP1 later | stays null |

### Honest ceiling even on success (class A)
Candidate full-schedule peaks for the focus Cellbox become measured (stacks exact lower-bound, mapped-backed, allocator peaks) on hooks kernels. CBX-27 still lacks a matched feature-equivalent comparator (B2 only for the headless shared subset, no allocator; desktop null), RSS beyond mapped-backed, other cells' stacks and timing on the promoted kernel. Under D8-4 = null the row can not be honestly called met; the expected outcome is a further, narrower partial record, not closure. Phase 10 criterion 1 also stays open (CBX-23/24/25).

**Preservation:** 546 protected paths (with the S10-5 deltas as already explained), 43 frozen sources, 25+ prior archives byte-exact; new sibling dir `runtime/observer-s10-8/` and `reports/s10-8-evidence/`; no guest writes, no product/candidate/canonical edit, no commit of images, no push without separate request.

- **Status 2026-10-29:** S10-8 is **proposed only**; CBX-27 and Phase 10 criteria 1 and 4 remain **UNMET**.

- **S10-8 CP1 APPROVED by the user 2026-10-29 as recommended: D8-1=B (pause-credit deadline), D8-2 both sets once, D8-3 no further rerun for any outcome, D8-4 b2-desktop stays null.** Execution order S10-8.0 -> 8.4; stop on the first unmet gate.

### S10-8 results (2026-10-29): defect fixed; cand-headless class E, cand-desktop class C; no rerun (D8-3); CBX-27 STAYS UNMET; S10-7/S10-8 close partial pending CP2
- **8.0:** `_stop` collision renamed `_wiring_stop` (`observer-s10-8/s108_memory.py`); real observer thread starts/finishes/joins cleanly, wiring stops still raise `common.Stop`, the S10-7 mixin reproduces the `TypeError` in the same test; collision census empty (only four intentional super-chained overrides). **8.1:** pause-credit deadline for the two original 10 s recovery waits only (headless new interrupt prompt; desktop new GUI prompt), 35 tests. **8.2:** ONE gate invocation, 479 tests, 0 fail/error/skip (268 retained + 35 + 91 + 85 new; the one stale retained preflight test excluded and disclosed); go/no-go all true with new tokens; 546-path census saved before/after and equal for both runs (470 present + 76 relocated).
- **cand-headless: class E STOP.** `Stop: 900s measurement window expired`; `observation blocker: RspTimeout: RSP receive timed out`. Never reached ready: boot stalled ~9.27 s after the observer connected while loading `/bin/xhci`; last RSP receive was a complete `$T05thread:01;#07` with no ack/parse event afterwards; no snapshots, no recovery waits. [INFERENCE] the retained non-atomic headless-client stop-packet receive race (S10-4.14 failure mode); not proven for this instance.
- **cand-desktop: class C STOP**, 352.9 s: completed terminal-launch, initial prompt, idle, finite-stream, collected-cat buffered/cancelled, terminal-stream live/cancelled (further than S10-7). Pause credits: collected-cat 1.665 s (effective 11.665 s, waited 7.33 s, satisfied), terminal-stream 5.278 s (15.278 s, 13.94 s, satisfied), rg-large 3.990 s (13.990 s, waited 14.22 s, **timeout**: ~10 s unpaused guest time without the fresh GUI prompt; an observation, not a product-defect claim). Missing `rg-cancelled`, `interactive-top-idle`, `complete`. Observer shutdown clean (`observer_error` null).
- **Desktop measured (guest-resident, QEMU/TCG, test-hooks kernel, instance 2 foreground-terminal; 7 snapshots all ok/exact):** same eight regions as S10-7 (sum 16,568,320 B, distinct frames 10,264,576 B); user-stack depth 35,352 B (prompt/idle) then 45,720 B from finite-stream; kernel stack 16,072 B; allocator maxima finite 67,488 B, collected-cat 56,168 B, terminal-stream 53,376 B, rg-large 102,392 B; autoload-background 25,272 B. No `complete` snapshot in either set.
- **Deviation to confirm at CP2:** the retained schedule has no per-phase maximum table; "each phase's own maximum" was implemented as 120 s (retained `command_s`) as a hard cap on one credited wait plus the 900 s window. Desktop was run after a class-E headless result because D8-2/D8-3 approved both sets once.
- **Rg-large interrupt and `top` interrupt remain unobserved to completion; no full-schedule candidate peak exists; no matched feature-equivalent comparator; b2-desktop null; RSS beyond mapped-backed absent. CBX-27 and Phase 10 criterion 4 stay UNMET. No rerun authorized; nothing pushed.** Evidence `reports/s10-8-evidence/`, code `runtime/observer-s10-8/`.
