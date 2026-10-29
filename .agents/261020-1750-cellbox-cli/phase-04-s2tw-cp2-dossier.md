# S2-T/S2-W CP2 dossier — edge-bounded telemetry (X/M) and VFS-mutation (W) rights

**Status: CP2 APPROVED by the user on 2026-10-22 (“duyệt bốn CP2”) for dossier SHA-256 `1ea820149ad84989c506328000907af3d9e36b70d112f6e7f6780b72a090a04d` (digest before this status amendment).** CP2 would authorize the S2-T/W source as the S2-T/W implementation baseline **only**: the hunks attributed to S2-T/W below, at the exact file hashes listed. Quarantine, canonical packaging, release signing (including any release-signed policy v6 blob) and rollout stay **BLOCKED**.

Prepared 2026-10-22 as S4b of the [S4 retirement proposal](phase-04-s4-retirement-proposal.md), from:
- the current source bytes;
- read-only `git diff` against `HEAD` (`3471028bc`);
- the saved logs in `reports/s4-final-evidence/`.

The only new execution was `sha256sum`/`stat` and read-only git/grep. No build, test or QEMU run was performed for this dossier.

## Authority

- [S2-T/S2-W annex](phase-04-s2tw-annex.md): text approved ("duyệt D-TW1..D-TW3"), with these decisions:
  - D-TW1: preserve image behavior and deny only the sandbox prefixes.
  - D-TW2: no restricted shell row now.
  - D-TW3: gate `GetProcs` with X.
- **CP1 APPROVED** ("duyệt CP1") for annex SHA-256 `57869333b8e6a1dc53209e2c2d9c0d21de3608fdddc284ef8f683281f6aead6d`. Entry check E1 is resolved in annex §10.
- Results and deviations are in annex §11. This dossier adds no scope.
- **Depends on S2-N:** S2-T/W was implemented on top of S2-N in the same kernel and policy files. See the [S2-N CP2 dossier](phase-04-s2n-cp2-dossier.md). Policy v6 extends v5, and the S2-N row constants take `..LEGACY` as their base.

## Delta summary

The kernel adds three kernel-only `CapSet` rights: `observe_tasks` (X), `observe_memory` (M) and `vfs_mutate` (W).

**How a Cell gets them**
- The install path requests them (`with_path_observe_mutate_rights`). Every path requests all three except `/apps/`, `/data/bin/` and `/home/`.
- The request is bounded by the reviewed launch/boot ceiling, then by policy v6. A v1–v5 entry decodes the three rights as `true`.
- Every pre-existing reviewed target, launch-profile and boot row ORs in `CapSet::LEGACY_OBSERVE_MUTATE`. The sandbox default arm stays `EMPTY`.

**How they are enforced**
- `check_allowlist` additionally requires X for `GetProcs`/`GetProcs2` and M for `MemInfo`. This applies to permit-all tasks too; only tid 0 is exempt.
- `attested_identity_of` attests `CALLER_FLAG_VFS_MUTATE` only when allowlist bit 63 **and** the sender task's `vfs_mutate` are both set.
- The caller-ELF route gate and the ocel-quickjs gates in `domain_admission` ignore the three rights (`without_observe_mutate`).

**Audit and fixtures**
- The policy narrowing audit gains bits 13–15.
- A private fixture row `/bin/s2tw-probe` (`LEGACY`) exists under the kernel feature `phase04-driver-profile` only.
- New probe crate: `cells/tests/s2tw-probe`.

**Unchanged:** syscall numbers, allowlist semantics, the `libs/api` syscall ABI, the VFS service, ostd and cellbox source. Grep confirms that no S2-T/W symbol appears in them.

## Evidence

Line numbers refer to `reports/s4-final-evidence/`. The run exercised the **combined** tree: S2-N + S2-T/W + S4a + every earlier delta.

| Check | Saved log lines | Result |
|---|---|---|
| Fixture feature compiled | `phase04-terminal.log:335` (`Building private kernel (phase02-terminal-profile,phase03-filter-profile,phase04-driver-profile)`), `:40` `Compiling app-s2tw-probe` | — |
| Probe signed and packaged | `phase04-terminal.log:511` `"s2tw-probe": "4da1bb9d…"` | The runner's FAT check for both `/bin/s2tw-probe` and `/data/bin/s2tw-probe` (runner 399-413, `set -euo pipefail`) passed before `:335` |
| Sandbox (`/data/bin/s2tw-probe`, EMPTY ceiling) denied 4/4 | `phase04-terminal.log:589-592` `PASS marker:sandbox-denied-{getprocs,getprocs2,meminfo,vfs-write}`; `phase04-serial.log:1573-1576` | Kernel gate observed directly at `serial:1569-1571`: `syscall GetProcs` / `GetProcs2` / `MemInfo denied for tid 40: edge withholds the S2-T observation right`; probe loaded `:1568`, task 40 exits 0 at `:1572` |
| No file written by the sandbox | `phase04-terminal.log:593` `PASS marker:sandbox-no-file`; `serial:1577` `cat: /tmp/s2tw-probe-sb: No such file or directory`, `:1578` | The check now also requires a non-zero `cat` status (driver 885-899; S4a `cat` fix) |
| Control (`/bin/s2tw-probe`, `LEGACY` row) admitted 4/4 | `phase04-terminal.log:594-597` `PASS marker:control-allowed-{getprocs,getprocs2,meminfo,vfs-write}`; `serial:1586-1589` | No denial WARN between the probe load (`serial:1584`) and its exit (`:1585`, task 41 code 0) |
| Control file created | `phase04-terminal.log:598` `PASS marker:control-file-created`; `serial:1590` | — |
| Probe output stays off the UART | `phase04-terminal.log:868` `PASS s2tw-probe-not-on-uart` | — |
| Existing users of X/M/W still work | e.g. `phase04-terminal.log:529` `builtin-free`, `:531` `builtin-ps-records`, `:534-535` `file-create-ls`/`file-cat-roundtrip`, `:541` `external-free`, `:600` `ps-reflects-new-shell` | Exercised indirectly: these pass with the rights on cellbox/sys-tools edges |
| S2-N / S3 checks unchanged | `phase04-terminal.log:580-588`, and the S3 checks | — |
| Whole runner | `phase04-terminal.log:870-874`, `:878` `PASS: 353 Phase04 S3-output runtime checks (87 driver markers)` | 354 `PASS` lines (353 checks + summary); 87 `PASS marker:` lines and 87 `P04 PASS` serial lines |
| Phase03 regression | `phase03.log:397-456` (60 `PASS`), `:457` completion | 60/60 |
| Phase02 regression | `phase02.log:407` `passes: 48`, `:409` completion | The log prints no exit status; "exit 0" is recorded in the [S4a Results](phase-04-s4-retirement-proposal.md#results--s4a-2026-10-22) |
| On-image kernel self-tests (evidence kernel boot) | `phase04-serial.log:96-97` `THREAD-CAP: PASS`, `:114-115` `IPC-PENDING: PASS`, `:123` `policy verify+parse self-test PASS`, `:127-128` `boot-ceiling: PASS` | These suites contain the S2-T/W additions:<br>• X/W inheritance (`thread_cap_selftest.rs:127-130`)<br>• case 4, W withheld means no `VFS_MUTATE` attestation (`ipc_pending_selftest.rs:844-888`, run via line 907)<br>• `v6_parse_cases` (`policy.rs` 441)<br>• boot-row X/M/W, sandbox and lan9514 checks (`selftest.rs` 152-195)<br>The same boot logs the historical `COMPLETION-QUEUE: FAIL` (`serial:108-110`). It is unrelated to S2-T/W and already disclosed in earlier CP2s |
| Kernel host tests, four-target build, signer | **No raw log saved.** Annex §11 records only: 258/258 with default features and with `phase04-driver-profile`, four-target build, signer runs | Not re-run here |
| First S2-T/W image run | **No raw log saved.** Annex §11 records that it failed only `sandbox-no-file`, because of the `cat` status defect | Superseded: S4a fixed `cat` and made the check status-based; the 353 run above passed it |

`phase04-terminal.log:10` `G-S1a FAIL` is the informational Terminal `SetTimer` delta approved earlier. It is not S2-T/W.

**Evidence ↔ current source.**
- The run's `cargo_lock_sha256` (`terminal.log:486`) equals the current `Cargo.lock`.
- The runner pins `launch_profile/{mod,profiles,targets,tests}.rs` (runner 196-202). The pinned values equal the current hashes below, and the run proceeded past the pin check (`terminal.log:12`).
- No other file has a per-file hash in the evidence. Saying the run used exactly these bytes for those files is an `[INFERENCE]`: their mtimes precede the log files' mtimes (18:29), but those are copy times.

## Proposed implementation inventory — current disk SHA-256

**Exclusive to S2-T/W** (new files, or every changed hunk is S2-T/W):

| Path | State | Bytes | SHA-256 |
|---|---|---:|---|
| `cells/tests/s2tw-probe/Cargo.toml` | new | 587 | `f34d8e634b3c54c3ee7b15c2fa0ea62d66be06467a3bff4ef43a012f8937ab1c` |
| `cells/tests/s2tw-probe/build.rs` | new | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` |
| `cells/tests/s2tw-probe/src/main.rs` | new | 7370 | `ec26947495e0cebe6b382caec93c7753faa80e2a887a245cab1667f0624803e7` |
| `kernel/src/loader/domain_admission.rs` | modified: `without_observe_mutate` at 211-225 (browser-engine policy and `artifact_eligible`), test 633-638 (deviation 2) | 26771 | `3f104aed832cb7021c6f256ca5347614299b718544c72be48b1c46513be97839` |
| `kernel/src/task/thread_cap_selftest.rs` | modified: inheritance of X/W (127-130) | 21218 | `3dff9e230eeee9a95b7f6b5b987af880543e82e7a434224aab29ef635522a5e8` |
| `kernel/src/task/ipc_pending_selftest.rs` | modified: case 4, attestation withheld without `vfs_mutate` (801-888) | 34923 | `abc3a4ca4ff58b379f618df79f0d18b0b0a6edf5c3a0c128ae00d89ed21439d1` |

**Shared files.** Each is hashed once, as the whole file, and the hash covers every delta's hunks. Approving this dossier covers only the S2-T/W hunks named here.

| Path | Bytes | SHA-256 | S2-T/W hunks (current lines) | Other deltas in the same file |
|---|---:|---|---|---|
| `kernel/src/task/cap.rs` | 43493 | `8676f4330363056d7fed88e3723b66d6079e6173e44dc01b29fb76b318598cd2` | fields 250-260; the last three lines of each field-list block (`EMPTY` 281-283, `ALL` 328-330, task snapshot 351-353, `from_manifest` 408-411, `intersect` 585-587, `apply_to` 628-630, `from_task` 653-655); `LEGACY_OBSERVE_MUTATE` 286-295; `with_path_caps` tail 472; `with_path_observe_mutate_rights` 500-517; `without_observe_mutate` 553-565; tests 1026-1139 | **S2-N** 236-249, the first four lines of each block, 471, 475-498, `net_caller_flags` 591-608, tests 842-1025; S1 rename (22) |
| `kernel/src/task/tcb.rs` | 40658 | `65effec593a0c39870e27075e9afd16dd3681127c04c90d69e011419322c5b7b` | doc + fields 378-385, defaults 661-663 | **S2-N** 370-377, 657-660; Phase02 `child_context`/`child_stdio`; S1 rename |
| `kernel/src/task/syscall.rs` | 417105 | `79cd3ec36c8aacf81ecd7c5238689e518f27dfe6e40ebcbb2322c9e0048e0286` | `attested_identity_of` 2891 → `vfs_mutate_caller_flag` 2909-2920; `check_allowlist` doc 9508-9511 and gate 9583-9590; `observe_right_held` 9594-9614; tests ≈9753-9906 (`with_scheduler_task_caps`, `telemetry_syscalls_need_the_observe_rights_beyond_the_allowlist_bit`, `vfs_mutate_attestation_needs_the_right_and_the_declaration`) | **S2-N** 2892-2898; S2c raw shutdown; Phase02/S3 `CellSessionV1`, user-output preflight, launch-edge hunks; S1 rename |
| `kernel/src/policy.rs` | 50520 | `b34b9af9f8f88f0cddd2e3c532b59e4ac7036d1b3b0f7869e3a576fed78e0196` | `VERSION_V6` 47-49, `CAP_BYTES_V6` 57, `cap_bytes_for` 74, v6 decode 350-358 (v1–v5 → `true`), entry fields 377-379, `v6_parse_cases` 736-786, narrowing bits 13-15 (943-945), tests `v6_entry_round_trips_every_observe_mutate_right`, `pre_v6_permit_leaves_observe_mutate_unnarrowed`, `malformed_observe_mutate_byte_rejects_the_policy`, `signer_v6_output_decodes_with_kernel_rule` | **S2-N** v5 constants/decode/tests, bits 9-12, `NetRightsGranted`; the shared stride comment and audit format string; S1 rename |
| `kernel/src/audit.rs` | 11233 | `d6fa7e0a1d16adeace6c9c802aac4e5543bdc40938d6093f81109a720be17c09` | doc bits 13-15 (83) | **S2-N** bits 9-12, `NetRightsGranted = 27` |
| `kernel/src/loader/launch_profile/mod.rs` | 4459 | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` | caller-ELF gate comment and `without_observe_mutate()` (105-109; deviation 1) | Phase02/03/04 fixture-profile dispatch, `cellbox` caller name |
| `kernel/src/loader/launch_profile/targets.rs` | 5469 | `62a67cbbe89ae7ff2d61e4a9e74257e7a9657314973c99f4146eb1b020e2ab24` | `LEGACY` (7-9) and every `..LEGACY`/`=> LEGACY` row, including the base of the S2-N `NET_*` constants; `/bin/s2tw-probe` row under `phase04-driver-profile` (118-123); sandbox comment (D-TW1) | **S2-N** `NET_*` constants, net rows, removal of curl/ocel from the EMPTY list |
| `kernel/src/loader/launch_profile/profiles.rs` | 13851 | `3140c61b0765100bdf14eea8b8455e294c069abbffd71aa86885367377f0b0ca` | `LEGACY` (12-14) and 21 `LEGACY` row uses (65-336), including bench/capacity-probe at 301-302 | **S2-N** llm-gateway (90); Phase02/03/04 fixture profiles, terminal edge, S2c shutdown row, S1 rename. Some rows are new versus `HEAD` (e.g. terminal 149, S2c shutdown 69, fixture rows): their `LEGACY` value cannot be split from the row's introduction by git diff, so attribution there is by symbol |
| `kernel/src/loader/launch_profile/tests.rs` | 33844 | `a0f6aa7bbb7937f237301395de8398b31140ecfc16bb5d8b767ec51410ecee86` | `LEGACY` (6-8); `EMPTY`→`LEGACY` expectations; 620-1000 (`with_legacy`, `every_pre_annex_target_row_keeps_observe_mutate`, `every_pre_annex_boot_row_keeps_observe_mutate`, `s2tw_probe_fixture_row_is_feature_gated`, `elf_route_ignores_only_the_legacy_rights`) | **S2-N** 51-144, httpd list, llm-gateway; S2c/S3/Phase04 driver tests; S1 rename |
| `kernel/src/loader/boot_ceiling.rs` | 13369 | `cebbb73af1bb1525f73de3510173b0f846a4ba27a3eae63fa4a70d2bbdae22d0` | `LEGACY` (45-48), init X/M/W (83-87), every `..LEGACY`/`=> LEGACY` row (e.g. `/bin/cellbox` → `LEGACY`), sandbox comment 197-198, `log_refusal` 270-278 | **S2-N** net rows and refusal lines; S1 `/bin/shell`→`/bin/cellbox` |
| `kernel/src/loader/boot_ceiling/selftest.rs` | 10599 | `c1b9796daf7318deb4b17467fc1b938ba03a45190cbd8c860728e1813f69f31b` | 39, lan9514 152-160, boot-row X/M/W and sandbox loop 165-195, capability-free edge 254 | **S2-N** 197-246; S1 rename |
| `kernel/src/loader/elf_tests.rs` | 37045 | `3868a16e8d6d1e9d78fd4859b18ddf3cd2e055021437a252b6bcc7ff55fac823` | `/mem/` label expects `LEGACY_OBSERVE_MUTATE` (229-234) | S1 rename (212) |
| `scripts/sign-policy.py` | 24555 | `1ebeefada57020464f2cf87feb2858b178694d26d07bbb0ff4755257979f2c4b` | `VERSION = 6`, `CAP_BYTES` 6→18, `OBSERVE_MUTATE` (106), `ROW_FIELDS`, `observe_mutate_request`/`full_row` (186-210), v6 encode, independent v6 decode/domain checks, round-trip padding | **S2-N** net tuples and columns, v5 decode, E1 rule; S1 `/bin/cellbox` row |
| `cells/tests/phase04-terminal-driver/src/main.rs` | 53581 | `0139d5cc3404d092d848ba756d73c680bbeeaa0eeea59c912625cf7166d8fe61` | call `sandbox_rights(&mut a)` (330); `S2TW_RIGHTS`, `ProbeRun`, `run_s2tw_probe`, `s2tw_result`, `sandbox_rights` (837-918) | **S2-N** 329, 777-835; S3/S2-K/S2c driver; the S4a status-based `sandbox-no-file` condition (896) |
| `scripts/qemu-phase04-terminal-test.sh` | 42941 | `d4cdaadaffe48949d3122ca9ab81863b3732b8bce7f7de4d2dab382a0ee884a6` | header 43-48; feature note 74-79; `-p app-s2tw-probe`/packaging (241-258, shared lists); `/data/bin/s2tw-probe` FAT entry 267; acceptance-limit text 310-314; FAT check 399-413; provenance name list 439; expected markers 554-557; `s2tw-probe-not-on-uart` 689-691; digest pins 189-202 (S2-N + S2-T/W rows) | **S2-N** 35-41, 306-309, 377-381, 459, 467-482, 551-553, 671-688, 692-696; S3/S2-K runner |
| `Cargo.toml` | 8829 | `ec5800c0fdfb6612a5b5b290b48399c2643d55b4d9670ad213b07a853941e48b` | workspace member `cells/tests/s2tw-probe` (212) | S1 cellbox rename; Phase02/03/04 fixture members; `libs/terminal`, `libs/tui-cellos` |
| `Cargo.lock` | 76721 | `e58e780a681c093925eaf8c1b3dcd882e5d2fb4ac8de5ce634a3d5c60f1f18f6` | `app-s2tw-probe` entry (≈461) | Every other workspace delta; equals the evidence-run `cargo_lock_sha256` |

### Context: not S2-T/W hunks, listed for completeness

- **`kernel/src/main.rs`** (`a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64`, 80532 B):
  - Its mtime (17:26) falls inside the S2-T/W edit window.
  - Its diff against `HEAD` is only an x86 VMX boot-path change (`under_hypervisor` guard removed). That is not S2-T/W content and is not described in any S2/S4 result read for this dossier.
  - **Excluded and unattributed**, but it is compiled into the evidence kernel.
- **VFS consumer `cells/services/vfs/src/caller.rs:81`** reads `CALLER_FLAG_VFS_MUTATE` and is unchanged by this delta. Its earlier state is covered by the Phase02 CP2.

### Relationship to earlier approved hashes

These files changed since their earlier CP2 approval, through several later deltas including this one. No earlier approval extends to the current bytes.

| File | Approved in | Approved hash | Current hash |
|---|---|---|---|
| `launch_profile/mod.rs` | Phase02 CP2 | `53e9e41a…` | `b8db3b80…` |
| `tcb.rs` | Phase02 CP2 | `9def966e…` | `65effec5…` |
| `syscall.rs` | Phase02 CP2 | `ea8443a6…` | `79cd3ec3…` |
| `targets.rs` | Phase02 CP2 | `8892a225…` | `62a67cbb…` |
| `profiles.rs` | S3-output CP2 | `cd51eb0a…` | `3140c61b…` |
| `tests.rs` | S3-output CP2 | `a60748c8…` | `a0f6aa7b…` |
| driver | S3-output CP2 | `ec167edc…` | `0139d5cc…` |
| runner | S3-output CP2 | `bcce7aad…` | `d4cdaada…` |
| `Cargo.toml` | Phase02 CP2 | `31d831c4…` | `ec5800c0…` |

## Caller census (read-only grep, 2026-10-22)

- **`CapSet::{observe_tasks, observe_memory, vfs_mutate}` and `LEGACY_OBSERVE_MUTATE`** (kernel-only):
  - Written by `with_path_observe_mutate_rights`.
  - Bounded by `targets.rs`, `profiles.rs`, `boot_ceiling.rs`, then narrowed by policy v6.
  - Copied to tasks by `apply_to`/`from_task`; threads copy them (deviation 4).
  - **Read by:** `observe_right_held` ← `check_allowlist` (X/M); `vfs_mutate_caller_flag` ← `attested_identity_of` (W); `log_refusal`; policy `apply` audit.
- **`without_observe_mutate`**, read by:
  - `launch_profile/mod.rs:109` (caller-ELF gate);
  - `domain_admission.rs:217,225` (browser-engine policy, ocel-quickjs eligibility).
- **Cells declaring or using `GetProcs`/`GetProcs2`/`MemInfo`** (text grep; includes declarations). All of these keep the rights through the path request plus their `LEGACY` row; only a launch from a sandbox prefix loses them:
  - cellbox (`cmd_sys.rs`, `commands.rs`, `main.rs`, `top.rs`)
  - sys-tools `ps`/`free`/`kill`
  - init (`activation.rs`, `boot.rs`, `main.rs`)
  - supervisor; sshd (`main.rs`, `session.rs`); compositor `input_handler.rs`
  - hypha `tools/sys`
  - bench/capacity-probe/footprint tests; tier2-grant tests
  - phase02-session tests; phase03-filter; the phase04 driver; s2tw-probe
  - `libs/ostd` (`runtime.rs`, `syscall.rs` wrappers and implicit `spawn` grant); `libs/text-engine/src/top.rs`
- **`CALLER_FLAG_VFS_MUTATE` consumer:** VFS `caller.rs` (`may_mutate`) and the VFS tests. Unchanged; it now receives the flag only from Cells holding W.
- **Behavior change by design:** an ELF under `/apps/`, `/data/bin/` or `/home/` that reads tasks or memory, or writes `/data` `/tmp` `/mnt/sd` `/srv`, is now refused (annex §7).

## Known limitations / not exercised (annex §11 and the logs)

1. **Runtime coverage.**
   - Not run: GUI 6x103, KVM, the kernel AP lane, riscv64/aarch64 runtime.
   - Only `/data/bin/` was booted. `/apps/` and `/home/` are host-tested only.
2. **What is observed versus host-tested.**
   - The syscall ABI returns one generic error, so the image distinguishes a telemetry refusal from other failures only against the `/bin` control run. The kernel WARN lines (`serial:1569-1571`) are the direct gate observation.
   - The VFS refusal is observed as `Err(ERR_DENIED)` to `WriteAt` through the delegated cwd handle (runner 310-314).
   - The kernel CapSet values themselves are host-test claims; their logs were not saved.
3. **Coarse rights.** A granted Cell still sees every Cell's rows and may write any open prefix. Scoped D04 telemetry and D03 conditional save remain in Phases 06/08.
4. **Name-keyed launch edges (pre-existing, not closed).** Edges are keyed on the caller's task *name*, so a sandbox binary named `cellbox` would borrow cellbox's edges (annex §11).
5. **Row census risk (annex §7).** A row missing `LEGACY` silently loses ps/free/writes. Host tests `every_pre_annex_*` cover the target and boot tables, but their results were not saved as raw logs.
6. **Permit-all and thread inheritance.** The X/M gate also applies to permit-all (`u64::MAX`) tasks (deviation 3). Thread inheritance of X/W is asserted only by the THREAD-CAP self-test, which passed on the evidence boot (`serial:96-97`). No image check targets a threaded caller of these rights.
7. **`cat` read-error status (S4a).** S4a records that `cat` still exits 0 when a read error occurs midway. This is outside this delta.

## Deviations (annex §11, cross-checked against source)

1. The caller-ELF route's "non-empty edge is refused" check ignores the three rights (`launch_profile/mod.rs:105-109`).
2. `domain_admission.rs` ignores them in the ocel-quickjs empty-profile checks (217, 225). This file is outside annex §5.
3. The X/M gate applies to permit-all tasks; only tid 0 is exempt (`observe_right_held`, `syscall.rs:9599-9614`).
4. Attestation uses the sender task's copy of W (`syscall.rs:2891`). By contrast, S2-N net flags use the Cell root.
5. The probe writes through its cwd directory handle because external children are path-sealed. Telemetry denial is judged against the `/bin` control run.
6. *(Superseded by S4a.)* `sandbox-no-file` originally judged absence by diagnostic plus listing, because `cat` of a missing file exited 0. S4a fixed `cat` (exit 1) and the check is now status-based (driver 885-899). It passed in the 353 run.

## What CP2 would authorize

Approving CP2 would make the S2-T/W hunks identified above, at the listed whole-file hashes, the S2-T/W baseline. That includes the private `/bin/s2tw-probe` fixture row and the probe crate. Later work could rely on X/M/W being edge-bounded and on the sandbox prefixes being denied.

CP2 would **not**:
- approve the S2-N, S2-K, S2c, S3, Phase02/03, S4a or S1 hunks in the shared files, or the unattributed `kernel/src/main.rs` VMX change;
- lift quarantine, or authorize canonical packaging, release signing of policy v6 blobs, or rollout;
- deliver D03/D04;
- close criterion 3 on its own (that needs all four S4b CP2s; S4 proposal §5).

## Options

A. **Approve this CP2 baseline** at the exact hashes and evidence scope above.

C. **Not yet**: retain the implementation and evidence, withhold CP2.

No approval has been recorded.

## Rollout and rollback

**Rollout stays BLOCKED** under the [containment](reports/rollout-containment.md) release conditions; nothing here changes them.

**Rollback steps:**
1. Revert the S2-T/W hunks and new files, including the probe crate, its workspace and `Cargo.lock` entries, the driver `sandbox_rights`, and the runner S2-T/W parts.
2. Restore `EMPTY` where `LEGACY` replaced it.
3. Re-pin the runner's launch-profile digests.
4. Cold-boot a matched previous kernel, image and policy. A v6 blob is not accepted by a kernel whose `cap_bytes_for` stops at v5.

**Constraints on rollback:**
- S2-T/W can be reverted without S2-N, but only by hunk-level edits. The shared files interleave both deltas, `NET_*` must get back an `EMPTY`-based form, and the signer must return to emitting v5. No S2-N-only snapshot of those files exists on disk.
- Reverting S2-N requires reverting S2-T/W first or together.

No persisted state is involved.
