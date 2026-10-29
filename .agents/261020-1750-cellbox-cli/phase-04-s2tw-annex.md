# Phase 04 S2-T / S2-W annex — edge-bounded telemetry and VFS-mutation rights

Status: **TEXT APPROVED by the user on 2026-10-22 (“duyệt D-TW1..D-TW3”: preserve image behavior and deny sandbox prefixes; no restricted shell row now; gate `GetProcs` with X). Entry check E1 resolved (§10). FROZEN; **CP1 APPROVED by the user on 2026-10-22 (“duyệt CP1”) for SHA-256 `57869333b8e6a1dc53209e2c2d9c0d21de3608fdddc284ef8f683281f6aead6d`** (digest before this status amendment).** No source, ABI, kernel, policy, launch-profile or image change has been made.

## 1. Why this is not the full D03/D04 yet
The plan's full contracts are D04 typed scoped telemetry and D03 conditional save ([abi-vfs-telemetry.md](abi-vfs-telemetry.md) §2–§5). Their own CP1 prerequisites are not satisfiable today:
- **D04** needs a validated scheduler-tick → monotonic-time mapping (§5 "CPU percentage unavailable" until the owner supplies it). x86 `GetTime` returns ns while `TIMER_HZ` assumes 10 MHz ([OOM proposal](phase-04-oom-local-sink-proposal.md) "Not in this proposal" 1). It also needs a task-incarnation producer and a retained Cell CPU aggregate. None exists.
- **D03** needs a backend that preserves the old bytes on failed replace. Native `engine.rs` replace is in-place and FAT is remove/create (`abi-vfs-telemetry.md` §1); "a backend incapable of preservation … is not admitted for editor save".

Drafting those as CP1 now would mean inventing evidence. They stay in Phase 06 (Cell top) and Phase 08 (editor save), where their consumers are built. This annex closes the part of criterion 3 that is closable now: **who may hold the rights at all**.

## 2. Problem (facts)
- The syscall allowlist is **self-declared by the ELF** (`__ViCell_syscalls`, `kernel/src/loader/governed_spawn.rs:160`). It is not bounded by any launch ceiling, boot ceiling or policy: only `CapSet` goes through `requested ∩ ceiling ∩ policy` (`:122-150`).
- So any Cell, including the sandbox prefixes `/apps/`, `/data/bin/` and `/home/` (EMPTY ceiling, `targets.rs` default arm), can declare:
  - `GetProcs` (bit 14) / `GetProcs2` (bit 55): every Cell's task rows, unfiltered (`kernel/src/task/syscall.rs:6983-7042`);
  - `MemInfo` (bit 56): the global frame allocator snapshot;
  - `VfsMutate` (bit 63): kernel attests `CALLER_FLAG_VFS_MUTATE` (`syscall.rs:2886-2905`) and VFS admits create/write/unlink/rename on every policy-open prefix (`/data /tmp /mnt/sd /srv`).
- An ELF with no section runs permit-all (`u64::MAX`) for syscalls but never gets the VFS flag (it requires an explicit list).
- The cellbox shell legitimately needs all four (ps/top/free; file builtins and redirects). The gap is the **restricted edges** in the catalog matrix (`command-catalog.md` §4.2: "restricted cellbox invocation / untrusted app … no W/X/M"), which today cannot be restricted.

## 3. Model
Three new kernel-only `CapSet` fields, following the S2-N pattern (path request ∩ ceiling ∩ policy, no manifest bit):

| Right | `CapSet` field | Effect when false |
|---|---|---|
| X (task observation) | `observe_tasks` | `GetProcs`, `GetProcs2` refused with `PermissionDenied` at dispatch, even if the allowlist bit is set |
| M (system memory) | `observe_memory` | `MemInfo` refused the same way |
| W (VFS mutation) | `vfs_mutate` | kernel omits `CALLER_FLAG_VFS_MUTATE` from the attested trailer, even if allowlist bit 63 is declared; VFS then refuses every classified mutation (`cells/services/vfs/src/dispatch.rs:43-65`) |

The gate is an extra check, not a rewrite of the allowlist. `check_allowlist` (`syscall.rs:9502`) additionally requires the CapSet field for exactly these three syscalls, and `attested_identity_of` ANDs bit 63 with `vfs_mutate`. Allowlist bits, `u64::MAX` permit-all semantics, ostd implicit grants, syscall numbers, VFS wire and service code are all unchanged.

**Behavior-preserving defaults.** Unlike S2-N, these rights are relied on by most of the image today, so failing closed by census would break it.
- `with_path_caps` requests all three for every path **except** the sandbox prefixes (`/apps/`, `/data/bin/`, `/home/`).
- Every existing reviewed target row and boot-ceiling row that is `CapSet::EMPTY` today gains a named constant `CapSet::LEGACY_OBSERVE_MUTATE` (the three fields true) ORed in. That preserves current behavior row by row.
- The sandbox default arm stays EMPTY: **untrusted apps lose X/M/W**.
- Restricted shells can be added later as rows without the constant.

Threads inherit through the existing CapSet copy. `Spawner::User` children stay bounded by the parent's CapSet (`governed_spawn.rs:137-145`).

## 4. Policy
v6 = version byte `6`, `CAP_BYTES_V6 = 18`: v5's 15 bytes plus `observe_tasks, observe_memory, vfs_mutate` as literal `0|1`. v1–v5 decode the three fields as **true** (the same compatibility reasoning as S2-N §3.3: these were ambient, and `false` would strip every legacy Permit). `scripts/sign-policy.py` gets v6 rows, encoder and independent decoder. The narrowing audit gains three bits.

## 5. Closure
- `kernel/src/task/cap.rs` (fields, `EMPTY`/`ALL`, `from_manifest` false, `intersect`, `LEGACY_OBSERVE_MUTATE`, `with_path_caps` sandbox rule)
- `kernel/src/task/tcb.rs` (task storage, as for the net fields)
- `kernel/src/task/syscall.rs` (`check_allowlist` gate; `attested_identity_of` bit-63 AND)
- `kernel/src/loader/launch_profile/{targets,profiles}.rs` and `kernel/src/loader/boot_ceiling.rs` (rows ∪ constant; sandbox stays EMPTY)
- `kernel/src/policy.rs` (v6), `kernel/src/audit.rs`, `scripts/sign-policy.py`
- Kernel/self-tests that enumerate CapSet fields, and the launch-profile digest pins in `scripts/qemu-phase04-terminal-test.sh`
- Fixture: a sandbox probe (§6)

**Untouched:** `libs/api` syscall ABI, the VFS service, ostd, and cellbox source.

## 6. Verification
- **Host (kernel):**
  - `intersect`/`from_manifest`;
  - `with_path_caps` grants the three for `/bin/x` and denies them for each sandbox prefix;
  - every pre-existing row keeps the three (row-by-row test over the target table);
  - v6 round trip; a v5 Permit leaves the three unnarrowed; malformed bytes reject;
  - `check_allowlist` refuses GetProcs/GetProcs2/MemInfo for a task whose bit is declared but whose cap is false;
  - the attestation flag is absent when `vfs_mutate` is false.
- **Private image:**
  - A probe ELF declaring `GetProcs, GetProcs2, MemInfo, VfsMutate, Send, Recv, LookupService` is launched at a sandbox path, e.g. `/data/bin/s2tw-probe`, on the existing caller-ELF route with its EMPTY ceiling. It prints `S2TW getprocs|getprocs2|meminfo|vfs-write denied|other:<x>`, and the runner requires `denied` for all four and that `/tmp/s2tw-probe` does not exist afterwards.
  - The same binary at a `/bin` fixture path gets `other` for all four (positive control: the gate is the path/edge, not the binary).
  - Cellbox `ps/top/free`, file builtins and redirects keep working (existing S3/P03/P02 checks).
  - **Entry check E1:** confirm the driver/shell can place and launch an ELF under `/data/bin/` on the private image. If it cannot, use a private fixture row whose ceiling omits the three rights, recorded as a restricted edge.
- **Regression:** S3 terminal (342), Phase03 (60), Phase02, kernel AP lane, signer test.

## 7. Risks
- **Row census errors:** a row missing the constant silently loses `ps`/`free`/writes for that program. Mitigation: the row-by-row host test compares the post-change table against the pre-change list of non-sandbox rows.
- **Sandbox apps that write `/data` today stop working.** This is intentional (catalog §4.2), and visible as a VFS denial.
- **Coarse rights:** a granted Cell still sees every Cell's rows and may write any open prefix. Scoped observation (D04) and destination-bound save (D03) stay in Phases 06/08.
- **Policy v6 is a second signed-format bump in one series.** Release signing remains blocked by rollout containment.

## 8. Decisions requested (recommendations first)
- **D-TW1 Default:** *preserve image behavior and deny only the sandbox prefixes* (recommended), or fail closed by census as in S2-N. The census option is much larger: every ELF declaring the four bits needs a reviewed row.
- **D-TW2 Restricted shell edge now:** *no* (recommended; no consumer exists yet), or add a private read-only `cellbox` row (no X/M/W) to demonstrate the catalog's "restricted cellbox invocation".
- **D-TW3 `GetProcs` (bit 14), granted implicitly to `spawn` users by ostd:** *gate it with X like `GetProcs2`* (recommended; it leaks the same cross-Cell names), or leave it ambient.

## 9. Rollback
Revert the §5 files; cold-boot a matched previous image and policy. No persisted state. Rollout, S4 and CP2 remain blocked.

## 10. Entry check (2026-10-22, read-only)
- **E1:** feasible on the existing route. Cellbox launches externals with `sys_spawn_from_path_raw` first (`cells/tools/cellbox/src/executor.rs:2092-2096`). The kernel path route resolves VIFS1, and the reviewed-target default arm admits `/apps/`, `/data/bin/` and `/home/` with an EMPTY ceiling (`kernel/src/loader/launch_profile/targets.rs:123-127`). The probe is therefore packaged in the private VIFS1 at both `/data/bin/s2tw-probe` (sandbox) and `/bin/s2tw-probe`. The `/bin` copy needs one private fixture target row with `LEGACY_OBSERVE_MUTATE`, under the kernel feature `phase04-driver-profile`, never default. No new route.

## 11. Results (2026-10-22)
- **Implemented** per §3–§5:
  - `CapSet` fields `observe_tasks`, `observe_memory`, `vfs_mutate` and `LEGACY_OBSERVE_MUTATE`.
  - `with_path_observe_mutate_rights`: sandbox prefixes get none.
  - `check_allowlist` X/M gate; bit 63 attested only together with `vfs_mutate`.
  - Every pre-existing target, launch-profile and boot row keeps the three rights.
  - Policy v6 (18 bytes; v1–v5 decode the three as true), `sign-policy.py` v6, audit bits 13–15.
  - Private fixture row `/bin/s2tw-probe` under `phase04-driver-profile`, and the probe crate `cells/tests/s2tw-probe`.
- **Deviations:**
  1. The caller-ELF route's "non-empty edge is refused" check ignores the three rights; otherwise every formerly empty edge would lose that route, contradicting D-TW1.
  2. `domain_admission.rs` (not in §5) likewise ignores them for the ocel-quickjs empty-profile checks.
  3. The X/M gate also applies to permit-all (`u64::MAX`) tasks; only tid 0 is exempt. This closes the "no allowlist section" bypass.
  4. Attestation uses the sender task's copy of the right; threads copy it.
  5. The probe writes through its cwd directory handle, because external children are path-sealed by VFS. Telemetry denial is reported as any error, compared against the `/bin` control run.
  6. The driver's `sandbox-no-file` check judges absence by the `No such file or directory` diagnostic and the `/tmp` listing, because `cat` of a missing file exits 0. That is a pre-existing cellbox defect (`commands.rs` cmd_cat "No such file" branch returns `Ok`), recorded and not fixed here.
- **Verified:**
  - Kernel host tests 258/258, with default features and with `phase04-driver-profile`; four-target build; signer runs.
  - Private terminal runner exit 0, **353 checks / 87 driver markers**:
    - `sandbox-denied-{getprocs,getprocs2,meminfo,vfs-write}`, `sandbox-no-file`;
    - `control-allowed-{getprocs,getprocs2,meminfo,vfs-write}`, `control-file-created`;
    - all S2-K/S2-N/S3 checks unchanged.
  - Phase03 60/60, Phase02 exit 0.
- **First run failed only `sandbox-no-file`**, because of the cat status defect above; the file was in fact absent (listing and diagnostic).
- **Not exercised:** GUI 6x103, KVM, the kernel AP lane, a `/apps/` or `/home/` launch (only `/data/bin/` was booted; the other two are host-tested).
- **Known remaining gap (reported by the implementer):** launch edges are keyed on the caller's task *name*; a sandbox binary named `cellbox` would borrow cellbox edges. This predates the slice and is not closed by it.
- **Criterion 3:** the X/M/W rows are now proven for restricted (sandbox) callers. With S2-K and S2-N, every catalog right named in criterion 3 has an image denial. Scoped D04/D03 remain in Phases 06/08.
