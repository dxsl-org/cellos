# Security Audit — Capability-Stripping Gap & `SpawnReplacement` Contract

**Scope:** the cap-ceiling gap in the Supervisory Cell hotswap path (plan Phase 00) + threat model for the `SpawnReplacement` fix.
**Verdict:** the shipped supervisor's cap-stripping is a **LATENT correctness bug** (authority *reduction*, benign direction, not reachable by any shipped caller today). The **security risk lives entirely in the fix**: the plan's stated invariant `requested ∩ frozen_ceiling` is **incomplete** and, as written, leaves a **DMA-anywhere escalation** open. Ranked findings below.
**Method:** all claims grep-verified against the tree at HEAD (2026-07-12). No code changed.

---

## 1. Every spawn-with-caps path — what CapSet the child actually receives today

Grant pipeline for all non-Root spawns (`loader.rs:239-326`):
`requested = manifest∩ (or legacy_path_caps)` → `after_spawner = requested ∩ ceiling` → `granted = policy::apply(path, after_spawner)` → `granted.apply_to(task)` → **then unconditional path-based caps at `loader.rs:301-324`** (PcieDriverCap, PlatformCap, SupervisorCap, VFS cell-store region).

| Path | Entry | `Spawner` | Ceiling source | CapSet child receives today |
|------|-------|-----------|----------------|------------------------------|
| **init** | `main.rs:535` `spawn_from_mem` + direct TCB write `main.rs:549-558` | none (bypasses `spawn_gated`) | n/a | `CapSet::ALL` + `SupervisorCap` + `is_critical=true`, written directly. Manifest never read. **Exemption = it never goes through `spawn_gated`**; the only ambient-authority injection point in the system. Correct. |
| **loader `spawn_from_path`** (init's children: vfs/net/shell/platform/drivers) | `syscall.rs:1719` | `User(caller=init)` | `CapSet::of_task(init)` = `ALL` | `manifest ∩ ALL = manifest`, then policy. Privileged services get their real caps because **init** is the spawner. Correct. |
| **`sys_spawn_from_elf` (238)** | `syscall.rs:1761-1828` | `User(caller)` | `CapSet::of_task(caller)` | `manifest ∩ caller` then policy. Monotonic downgrade. Correct. Note the `/bin/` privilege gate (`loader.rs:156`) + a lying `path` can only LOSE privilege. |
| **`sys_spawn_pinned`** | `syscall.rs:1830-1852` | `User(caller)` | caller caps | same as above. |
| **kernel `hotswap()` (400)** | `hotswap.rs:320`; ceiling read at `328-334` | `Ceiling(CapSet::of_task(old))` | **frozen cell's live CapSet** | `manifest ∩ old_caps` then policy. **CapSet-correct** — but see Finding C1: path-based caps (`loader.rs:301-324`) are re-granted **ceiling-blind**. |
| **supervisor `hotswap()` → `sys_spawn_from_path` (`supervisor/src/hotswap.rs:167`)** | via `sys_spawn_from_path`→`User(supervisor)` | `CapSet::of_task(supervisor)` = **spawn only** (`SupervisorCap` is not in CapSet) | **`manifest ∩ {spawn}`** — block_io/network/mmio/hypervisor all dropped | **THE GAP.** A hotswapped vfs/net/compositor comes up with **no block_io / no NetworkCap**. `policy::apply` trusted-core recovery (`policy.rs:281`) does NOT rescue it — recovery only *keeps* `after_spawner`, which already lost the cap at the intersect step. Also `granted.block_io==false` ⇒ the VFS fast-IPC handler is never re-pointed (`loader.rs:330`). Replacement is dead. |
| **`SpawnReplacement` (planned, Phase 00)** | new | `Ceiling(frozen_record)` | frozen cell's recorded CapSet | *intended* `manifest ∩ frozen`. **Incomplete — see §2/C1.** |
| **pkg install (`260712-1000`)** | writes ELF to `/bin`, then normal `sys_spawn_from_path`/`sys_spawn_from_elf` | `User(caller)` | caller caps | install does **not** grant caps; the spawn-gate does, unchanged. A tampered/unsigned ELF lands harmlessly and **cannot spawn** (`signing.rs`, fail-closed under `signing-required`). No new spawn-with-caps path — **but it makes `/bin` writable**, which changes the `SpawnReplacement` threat model (see C1 exploit). |

**Reachability today:** the `hotswap` CLI still calls `sys_hotswap` (kernel 400) — `sys-tools/src/bin/hotswap.rs:73`. **No shipped cell sends `OP_HOTSWAP` to `service::SUPERVISOR`.** The supervisor's crippled `sys_spawn_from_path` path is therefore **dead code** until the Phase 02 cutover. Confirmed: only init *registers* SUPERVISOR (`init/src/main.rs:85`); nothing invokes it.

---

## 2. Threat model for `SpawnReplacement`

Proposed invariant (phase-00): `replacement_caps = new_manifest_request ∩ frozen_original_caps`, SupervisorCap-gated, fail-closed on missing frozen record.

### C1 — CRITICAL: the frozen-ceiling does NOT bound path-based caps → DMA-anywhere escalation
`CapSet` (`cap.rs:119-126`) contains **only** `block_io, network, spawn, hypervisor, mmio_devices, block_regions`. It does **not** carry `pcie_driver_cap`, `platform_cap`, or `supervisor_cap`. So `CapSet::of_task(frozen)` cannot capture them, `intersect` cannot bound them, and `spawn_gated` **re-grants them by path match, unconditionally, after the ceiling step** (`loader.rs:301-324`):

```
if path == "/bin/nvme"|"/bin/e1000"|"/bin/virtio-net"|"/bin/block"|"/bin/input"|"/bin/virtio-gpu"
                                        { task.pcie_driver_cap = Some(PcieDriverCap::new()); }
if path == "/bin/supervisor"           { task.supervisor_cap  = Some(SupervisorCap::new());  }
if path == "/bin/vfs"                  { task.block_regions  |= 0b1000; }  // cell-store
```

**Exploit (compromised/malicious supervisor, dev-mode default):**
1. Freeze *any* non-critical cell — even one whose `CapSet` is `EMPTY` (a demo). Frozen ceiling = EMPTY.
2. `SpawnReplacement(frozen_tid, "/bin/nvme")`. Cap intersection yields EMPTY (correct)…
3. …but `loader.rs:301` matches `"/bin/nvme"` and grants **`PcieDriverCap`** regardless of the EMPTY ceiling.
4. Replacement now claims a PCIe BAR + authorizes DMA (`GrantDma`) → **DMA writes anywhere in the single address space, bypassing LBI entirely.** Total memory/kernel compromise.

Same shape yields **`SupervisorCap`** via `"/bin/supervisor"` (self-perpetuating lifecycle authority, escapes the self-freeze guard by seeding a *second* supervisor) and the VFS cell-store region via `"/bin/vfs"`. Under the pkg-distribution plan `/bin` becomes writable, so the attacker can also stage arbitrary bytes at these privileged paths.

This hole **already exists latently in kernel `hotswap()`** (400) — it too calls `spawn_from_path(new_path, Ceiling(..))` and re-runs the path grants — but is SpawnCap-gated (shell/init). `SpawnReplacement` widens the reachable actor set to SupervisorCap holders and is the point at which it must be closed.

**Required contract fix — the ceiling must bind the ELF *identity*, not just the CapSet.** Two acceptable shapes:
- **(preferred) Path-identity binding:** record the frozen cell's **original spawn path** at freeze. `SpawnReplacement` uses the *recorded original path* — not the caller-supplied path — for the `/bin/` privilege gate **and** the path-based cap grants; the caller-supplied argument only selects the ELF bytes. Result: a `/bin/net` replacement can only ever receive `/bin/net`'s path-caps (none), never `/bin/nvme`'s. Closes C1 and ELF-substitution-for-caps in one stroke.
- **(alternative) Extend `CapSet` to carry the path-based caps** as booleans, capture them in `of_task`, intersect them, and gate the `loader.rs:301-324` grants on "frozen original held it." More invasive (touches `CapSet` shape) and must not regress the init grant.

Either way the invariant must read: **replacement authority ⊆ frozen original's *total* authority (CapSet ∪ path-caps ∪ path-region), across every grant channel** — not just the `CapSet` subset.

### C2 — HIGH: stale ceiling record = J-Kernel stale-authority retention
Phase-00 stores `swap_ceiling: BTreeMap<tid, CapSet>` recorded at freeze. Tids are **reused** after `exit_task`. If a record outlives its swap (supervisor crash between freeze and replacement; missed clear on an error path), and the tid is later reissued to an *unrelated* cell that then gets frozen, a stale high-cap ceiling could be applied to it. This is exactly the J-Kernel *stale-authority retention* the kernel-cap rule exists to prevent (CLAUDE.md §Kernel Boundary Law).

Mitigations, all required:
- Clear the record on **both** terminal points — `KillCell`/`exit_task_internal` **and** `ResumeCell`/`unfreeze_task` (phase-00 step 5).
- **Reject freeze if a live record already exists for that tid** (one swap per cell; phase-00 risk note) — prevents record overwrite/accumulation.
- Guarantee **tid is not reissued until after the record clear** completes on the exit path (verify ordering in `exit_task_internal` vs tid allocator).
- **Stronger, preferred:** avoid the stored map where possible — read the ceiling **live** from the frozen cell's TCB at replacement time (as kernel `hotswap.rs:328` already does). The frozen cell is *frozen, not dead*, so `of_task` is valid. Store a record **only** to survive the old-cell-exits-mid-swap race (`hotswap.rs:382-387`), and treat a present-but-cell-alive mismatch as fail-closed.

### C3 — HIGH: ELF substitution within the ceiling (accepted risk, must be documented + signing-gated)
Even with C1 closed, `SpawnReplacement` lets the supervisor run a **different binary** under the victim's authority envelope (needed for legitimate v1→v2 live upgrade). Containment rests on three gates, in order:
1. **Authority bound** — replacement ⊆ frozen original's total authority (C1 fix). Cannot manufacture *new* authority.
2. **Signature gate** (`loader.rs:114-136`) — under `signing-required`, only a validly-signed ELF loads (fail-closed). **This is the code-identity control.** In dev-mode (default, unsigned permitted) a compromised supervisor CAN inject arbitrary code, still authority-bounded. **Production MUST run `signing-required`** — state this as a precondition of the SpawnReplacement security argument.
3. **`/bin/` path gate** (`loader.rs:156`) — privileged manifest only under `/bin/`.

Residual (acceptable): a supervisor with `signing-required` can substitute only *signed* code, bounded to the frozen cell's authority. No new authority, no unsigned code.

### C4 — MED: repeated-hotswap accumulation
Freeze A (caps X) → replace with B → freeze B → replace… Each `SpawnReplacement` is bounded by **one** frozen record. As long as (a) the ceiling is recomputed per-swap from the currently-frozen target and (b) records are never unioned across cells, authority **cannot grow** across swaps. Verify the implementation never merges two records and never carries a ceiling forward past a terminal.

### C5 — MED: manifest-over-request rejected, not clamped
If the replacement manifest requests **more** than the frozen ceiling, the correct behavior is `intersect` (silent clamp) — that is fail-safe (child gets less). But a request that *exceeds* the ceiling on a **path-based** cap is the C1 hole; ensure the C1 fix clamps those channels too, not just the `CapSet` intersect.

---

## 3. Where the frozen CapSet (+ path identity) must live

**In the kernel, alongside the `FROZEN` set (`hotswap.rs:68`).** Consistent and required:
- The `FROZEN` set already lives in the kernel and **survives a supervisor crash** — the supervisor holds no swap state. If the supervisor dies mid-swap, the frozen target stays frozen and **init** (holds `SupervisorCap`, `main.rs:554`) can resume/kill it. The ceiling record must share this lifetime and location so init's recovery path also sees/cleans it.
- Cap authority is kernel-only by law (`docs/specs/15-kernel-boundary.md` §1.2; §1.3 "LBI prevents forgery, not revocation/stale-authority"). The ceiling — the token that authorizes inheriting a privileged cap — is itself capability authority and **cannot** be asserted by the supervisor. Storing it in the kernel is the only compliant home.
- **Requirement:** `ResumeCell` (414 → `unfreeze_task`) must clear the record (init's orphan-recovery uses ResumeCell). Verify this closes the crash-mid-swap leak.

---

## 4. `is_critical` interaction

- **init cannot be captured.** `FreezeCell` rejects `is_critical` (`syscall.rs:2446-2452`) and `KillCell` likewise (`2489`). init is the only `is_critical=true` cell (`main.rs:557`). So `CapSet::ALL` + init's `SupervisorCap` can **never** enter a swap record → `SpawnReplacement` can never wield init's authority. Provided `SpawnReplacement` **requires a live frozen record** (fail-closed on absent — phase-00 step 4), and init can never be frozen, `SpawnReplacement(init, …)` fails closed automatically. **Make this explicit** rather than relying on the transitive argument.
- **The supervisor is NOT `is_critical`.** It is spawned by init via `spawn_from_path` and never marked critical (only init is). It is protected instead by two narrower guards: `FreezeCell`/`KillCell` reject `target_tid == caller_id` (`2442`, `2477`) → the supervisor **cannot freeze/kill/replace itself**; and only init + the supervisor hold `SupervisorCap`. So: **supervisor-of-supervisor = init.** If the supervisor crashes, init restarts it (never-die); frozen targets survive in the kernel. No infinite regress — the tree terminates at the kernel (which respawns init or panic-reboots; plan Open-Q4 flags verifying which).
- **Watch:** because the supervisor is not `is_critical`, a *second* `SupervisorCap` holder could freeze it. Today only init holds a second copy. **The C1 `"/bin/supervisor"` path-grant escalation would let an attacker mint a second SupervisorCap holder** and then freeze/replace the real supervisor — another reason C1 is critical.

---

## 5. Least-authority for the Supervisory Cell

The whole point: **lifecycle authority ≠ resource authority.** The supervisor must orchestrate freeze/resume/kill/spawn-replacement on *other* cells **without holding those cells' resource caps** (so it can never impersonate vfs/net or DMA like a driver).

The shipped design already achieves this and must be preserved:
- `SupervisorCap` is a distinct ZST (`cap.rs:53-64`), **not** a `CapSet` field, **not** delegable via `intersect` — a supervisor cannot forge it into a child.
- The supervisor holds **`SpawnCap` + `SupervisorCap` only** (`supervisor/src/main.rs:89-92` + path-grant `loader.rs:315`). It has **no** block_io/network/mmio.
- `SpawnReplacement` gives it the *authority to trigger* a privileged replacement **without** the *resource caps* — because the kernel supplies the ceiling **from the frozen cell**, never from the supervisor. This is the correct capability-attenuation model (Fuchsia/Genode).

**`SupervisorCap` semantics to ratify:** "authority to invoke `FreezeCell`/`ResumeCell`/`KillCell`/`SpawnReplacement` on any **non-`is_critical`** cell — and nothing else." It grants **no** ability to read/write those cells' resources, send as them, or hold their caps. `SpawnReplacement` is the one primitive that *bridges* lifecycle→resource authority, and it does so **only** by the kernel copying the frozen target's own (already-held) authority to its replacement — never by minting authority the target lacked. Keep `SpawnCap` on the supervisor **only** because `sys_register_service` (`syscall.rs:1527`) requires it for the commit step; consider narrowing service re-registration to `SupervisorCap` so the supervisor need not hold `SpawnCap` at all (removes its ability to spawn arbitrary new cells — tighter least-authority; follow-up).

---

## 6. Fail-closed analysis — every error path must DENY

| Condition | Required outcome | Where enforced / to add |
|-----------|------------------|--------------------------|
| `SpawnReplacement` with no live frozen record for `old_tid` | `PermissionDenied` (never ambient spawn) | phase-00 step 4 — **mandatory** (the record IS the authorization) |
| Frozen record present but frozen cell's live caps < recorded (downgrade race) | use the **lesser**; never the recorded | add: `min(recorded, of_task(frozen))` |
| Caller lacks `SupervisorCap` | `PermissionDenied` | `caller_has_supervisor` gate (mirror `2438`) |
| Target `is_critical` (init) | cannot be frozen ⇒ no record ⇒ fail-closed | transitive; assert explicitly |
| Replacement ELF signature invalid / absent under `signing-required` | `PermissionDenied` (fail-closed) | `loader.rs:117-136` (already) |
| Replacement path resolves to a privileged `/bin/` cap the frozen original lacked (C1) | **deny the extra cap** — clamp via path-identity binding | **NEW — C1 fix, currently OPEN** |
| Replacement manifest requests > ceiling (CapSet) | silent `intersect` clamp (fail-safe) | `intersect` (already) |
| Old cell exits between freeze and replacement | record valid (dead cell's caps = safe upper bound); or fail-closed if identity can't be confirmed | tolerate per `hotswap.rs:382`, but prefer identity re-check |
| Stash/ready timeout, IPC failure mid-swap | old cell stays frozen (no split-brain); record retained for init recovery, cleared on eventual resume/kill | `hotswap.rs` rollback semantics + C2 clears |
| `PlatformCap` re-grant via `/bin/platform` replacement | singleton latch already returns `None` ⇒ whole spawn `PermissionDenied` (fail-closed) | `cap.rs:105`; note this is the *only* path-cap already fail-closed |

---

## Ranked findings

- **[CRITICAL] C1** — `loader.rs:301-324` grants `PcieDriverCap`/`SupervisorCap`/cell-store region by **path match, ceiling-blind**; `CapSet` (`cap.rs:119`) omits these caps so `intersect` can't bound them. `SpawnReplacement(any_frozen, "/bin/nvme")` ⇒ PcieDriverCap ⇒ **DMA-anywhere, LBI bypass**. Fix = bind replacement to the frozen original's **path identity** (use recorded original path for the privilege/path-grant logic), not just its CapSet.
- **[HIGH] C2** — stored `swap_ceiling` map is J-Kernel stale-authority retention under tid reuse. Clear on kill **and** resume; reject freeze if a record exists; prefer live-read from the frozen TCB.
- **[HIGH] C3** — ELF substitution within the ceiling is inherent; containment depends on `signing-required` in production. Document as a precondition.
- **[MED] C4** — verify no cross-cell ceiling union across repeated swaps.
- **[MED] C5** — ensure the over-request clamp covers path-based cap channels, not only the `CapSet` intersect.
- **[POSITIVE]** init-exemption (`spawn_from_mem` + direct TCB write, never through `spawn_gated`) is the single, auditable ambient-authority injection point. `SupervisorCap` as a non-`CapSet`, non-delegable ZST is the correct lifecycle≠resource separation. `is_critical` + self-target guards give a clean supervisor-of-supervisor termination at init/kernel. Fail-closed-on-missing-record (phase-00) is the right authorization model.
