# Plan — Cell Permission Model G1: P2 (delegation) + P5 (operator policy)

**Created:** 2026-06-21
**Branch base:** `feat/cell-security-p1-p3` (P1+P3 already shipped here)
**Design ref:** [docs/research/research-cell-security-permissions.md](../../docs/research/research-cell-security-permissions.md) §2.4–2.5
**Roadmap:** [project-roadmap.md](../../docs/project-roadmap.md) §G.2 (P2, P5)

## Goal

Complete the two remaining **G1** per-Cell permission items so capabilities obey the four
capability-OS invariants (no ambient authority · explicit delegation · **monotonic downgrade** ·
revocable), with **operator policy** as the headless-robot "consent" mechanism. Kernel + boot-path
work; every phase gated by a QEMU boot smoke (`ViCell >`, no faults).

## Governing design decision (resolves the init-as-root problem)

`init` becomes the **root authority**: granted `CapSet::ALL`, like seL4's initial task holds the root
CNode. Then **intersection applies uniformly** to every *user-cell-initiated* spawn (incl. init→vfs,
init→net) and boot does NOT break, because init holds every cap its children request. Monotonic
downgrade holds from init down: no cell hands a child a cap it lacks.

> **⚠️ CORRECTED by red-team (C1): the grant must be a DIRECT TCB write in `main.rs`, NOT via the
> manifest.** init is spawned by `task::spawn_from_mem` ([main.rs:455](../../kernel/src/main.rs#L455))
> + a manual `t.spawn_cap = Some(..)` ([main.rs:462](../../kernel/src/main.rs#L462)) — it NEVER calls
> `spawn_from_path`, so its `__ViCell_manifest` is never read. Expanding init's manifest is a no-op
> (and, if init were ever routed through the loader with block_io, it would hijack the VFS fast-IPC
> handler at loader.rs:185). So: grant `CapSet::ALL` in the main.rs init grant block; do NOT expand
> init's manifest; do NOT route init through `spawn_from_path`.
>
> **Escalation-oracle bound (C1/security):** init holding ALL caps is only safe if init's spawn
> targets are **compile-time constants** (no data-derived paths). This is an enforced invariant, not
> an assumption — the plan must verify init never spawns a path derived from mutable VIFS1/config.

## Every cap-granting / Task-creating path (the "choke point" is plural — red-team C2)

The plan's intersection must cover ALL of these, not just the syscall path:

| Path | Today | Required handling |
|------|-------|-------------------|
| `SpawnFromPath` **syscall** → `spawn_from_path` | grants from manifest | `spawner = Some(caller_id)` → `∩ spawner ∩ policy` |
| **boot** init via `spawn_from_mem` + manual grant (main.rs:455) | grants `spawn_cap` only | replace with `CapSet::ALL` direct write (C1) |
| **HotSwap** → `spawn_from_path` (hotswap.rs:100, kernel-internal, no caller) | grants full manifest | pass the **replaced cell's CapSet as ceiling**; subject to policy; NEVER the `None` root-exempt branch (C2) |
| **snapshot/warm-boot restore** (snapshot.rs:256) | memcpys Task caps verbatim | **invalidate snapshot when policy blob changes** (extend `kernel_hash()`); else old caps resurrect, breaking "revoke = reboot" (C2/M1) |
| `SpawnFromMem` **syscall** (syscall.rs:1716) | grants nothing | confirm it stays cap-less; if it ever grants, route through `effective_caps` |
| `task.rs::spawn_from_file` | dead stub | ignore (confirmed stub) |

## Phases

| # | Phase | Tier | Depends | Status |
|---|-------|------|---------|--------|
| 01 | [Spawn-time capability intersection (P2)](phase-01-spawn-cap-intersection.md) | thinking | — | ✅ done (riscv64+aarch64) |
| 02 | [In-kernel Ed25519 verify (P5a)](phase-02-ed25519-verify.md) | thinking | — | ✅ done — spike resolved: **signed viable** (ed25519-compact) |
| 03 | [Signed policy load + verify at boot (P5b)](phase-03-policy-load-verify.md) | thinking | 02 | ✅ logic done (machinery+absent+signed/invalid verified); on-disk bake deferred (needs FAT16 insert tool) |
| 04 | [Policy intersection + fail-closed semantics (P5c)](phase-04-policy-intersection.md) | medium | 01, 03 | ✅ done — `∩ policy` + recovery hatch; narrowing self-test green both arches |

**Parallelism (CORRECTED by red-team M-parallel):** Phase 02 adds a crypto dep to `kernel/Cargo.toml`
+ `Cargo.lock` (shared with Phase 01's kernel build). Per `project-release-build-broken-at-head`, dep
additions are exactly what has broken the PIC kernel build. So **do NOT run 01 ∥ 02 truly parallel**:
land the **Phase 02 dep spike FIRST** (or on its own branch), confirm the PIC build is clean, then
rebase/continue Phase 01 — so a P02 dep-graph change cannot silently break P01's already-passed boot
gate. Phase 03 needs 02; Phase 04 needs 01 + 03.

**Sequencing rationale:** P2 first (no crypto risk, unblocks the intersection plumbing Phase 04
reuses). P5 split so the risky crypto decision (Phase 02 spike) is isolated and can fail fast.

## Key cross-phase artifacts

- **`CapSet`** — a plain-data snapshot of a Task's capabilities (introduced Phase 01,
  reused by Phase 04). Single source of truth for "what caps does X hold".
- **Intersection point** — one function `effective_caps(manifest, spawner, policy)` that all spawn
  paths call. Phase 01 builds `manifest ∩ spawner`; Phase 04 adds `∩ policy`.

## Hard constraints (apply to every phase)

- **Law 1:** keep `CellManifest` at `u8` (all 8 bits used). P2 needs no manifest change. P5 stores
  policy **out-of-band in VIFS1**, not in the manifest. Do not expand to u16.
- **Law 4:** cells stay `#![forbid(unsafe_code)]`; kernel `unsafe` only for hardware + documented.
- **Build:** `$env:RUSTFLAGS="-C relocation-model=pic"; cargo build --release -p vicell-kernel`
  (riscv64) + `--target aarch64-unknown-none-softfloat`.
- **Verify (mandatory per phase):** `./run-tests.ps1 boot boots_to_shell_prompt` green +
  no `panic`/`scause`/`PermissionDenied` in boot log. A phase is NOT done until it boots.

## Out of scope (deferred) — decisions, not omissions

- Runtime hot-revocation of a *running* cell's caps (`CapHandle` + `sys_cap_revoke`) — G1 revocation
  is "push new policy + reboot" (with snapshot-invalidate on policy change). Hot-revoke = later §G.2.
- Interactive consent-broker Cell (G2 HMI only).
- Parameterized `__ViCell_cap_args` beyond P1 (device-scoped MMIO).
- **`syscall_allowlist` is NOT delegated/policy-constrained in G1 (red-team M6 — explicit decision).**
  It is already enforced per-cell from each cell's own `__ViCell_syscalls` ELF section; folding it
  into `CapSet` delegation is deferred to G2. Documented so it is a decision, not a silent gap.

## Rollback realism (red-team — entangled base)

Each phase's "rollback" target is **the prior phase's commit**, NOT `git revert` of a single hunk —
the branch base (savepoint `0983dc58`) bundles unrelated WIP, so per-hunk reverts won't separate
cleanly. Therefore: **commit each phase atomically** with a green `cargo build` (both arches) + boot
smoke; the phase commit IS the rollback unit. Phase 01 threads a `spawner` param through ~5 sites
(loader, syscall, hotswap, init grant, lua/other callers) — that whole change is one commit.

## Red Team Review (2026-06-21)

4 hostile-lens reviewers (Security Adversary · Assumption Destroyer · Failure Mode Analyst ·
Dependency Trap Hunter), each code-grounded. Verdict: 3× BLOCKED, 1× PASS_WITH_RISK → all findings
ACCEPTED and folded into the phases above/below.

**Critical (all 4 converged):**
- **C1** init grant is via `spawn_from_mem`+manual TCB write, not the manifest → decision (a) was a
  no-op that would boot-brick. → Phase 01 grants `CapSet::ALL` directly in main.rs; decision (a) removed.
- **C2** HotSwap + snapshot-restore bypass the intersection → escalation/laundering + "revoke=reboot"
  broken. → Phase 01 handles hotswap ceiling; Phase 03/04 invalidate snapshot on policy change.
- **C3** fail-OPEN defaults (absent/NoEntry → permit) = signature-bypass-by-deletion + no headless
  recovery on fail-closed mis-fire. → Phase 03/04 fail-CLOSED for P5 builds; dev-permissive behind a
  release-impossible cfg feature; add a maintenance-mode recovery hatch + minimal trusted core.

**Major (accepted):** Ed25519 fallback fork + `verify_strict` + round-trip test (P02); VIFS1 path
uppercase/8.3 → `/POLICY.BIN` (P03); bake blob into ALL embedded images incl test-hooks + assert
`PolicyLoaded` (P03); verify-then-parse + panic-free parser (P03); non-reentrant Spinlock — snapshot
spawner caps in a dropped guard, `policy::lookup` outside any SCHEDULER guard (P01/P04); delete
lazy-load option (b) (P03); policy CapSet domain validation + explicit audit discriminants 16–19
(P03/P04); dev key behind explicit feature (not `debug_assertions`) + CI pubkey check (P02/P03).

**Validated by red-team (no change needed):** no policy↔VIFS1↔vfs circular dep (VIFS1 is
kernel-embedded, mounted before init — eager load is correct); `caller_id` is hart-local, unforgeable
by a cell; `task.rs::spawn_from_file` is a dead stub; `CapSet` shape consistent P01↔P04;
`manifest::from_bytes` hardening pattern reused for the policy parser.

**Consistency:** OK.

## Validation Log (2026-06-21)

Critical-questions interview (verification pass skipped — Red Team section already carries
code-grounded evidence; no `[UNVERIFIED]` markers remain). All 4 decisions = recommended:

1. **init caps → `CapSet::ALL` + compile-time spawn-path bound** (not minimal-closure). Boot-safe;
   escalation-oracle bounded by the Phase 01 invariant (init spawns only compile-time-constant paths).
2. **Crypto fork → if Ed25519 breaks PIC, ship policy UNSIGNED in G1, defer Ed25519 to G2.** Policy
   model is not blocked on crypto; integrity rests on the kernel-embedded VIFS1 image (documented G1
   limitation). Phase 02 decision fork stands.
3. **G1 posture → dev-permissive default** (`absent ⇒ permit`); the fail-closed + recovery-hatch
   machinery is built now behind cfg so flipping to secure for a real fleet is one flag.
   **Invalid signature/parse ALWAYS fail-closed regardless of posture.** → Phase 03/04: G1 ships with
   `dev-permissive` ON; `policy_required`/fleet-secure is the flag, not the default.
4. **Sequencing → cook Phase 01 (P2) FIRST as a standalone commit**; run the Phase 02 crypto spike
   separately; decide P5 (Phases 03–04) only after the spike resolves. Reduces risk; each step verifies.

**Consistency:** OK.

## Cook handoff (per Validation decision 4)

**Now — Phase 01 only (P2, no crypto):**
```
/hc-cook .agents/260621-0830-cell-perms-p2-p5/plan.md --phase 01
```
(or implement Phase 01 directly — it is self-contained, kernel-only, boot-verifiable.)

**Then — Phase 02 spike** (crypto feasibility under PIC) → resolves the unsigned-vs-signed fork.

**Then — decide + cook Phases 03–04** (P5) based on the spike outcome.

