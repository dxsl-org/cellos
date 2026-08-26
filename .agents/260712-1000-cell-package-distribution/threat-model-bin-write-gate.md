# Threat Model — Unlocking the Read-Only `/bin` Write Gate (P01)

> Red-team analysis of the security-sensitive primitive that Phase 01 unlocks: a gated write into
> the FAT cell-store that surfaces at `/bin`. Scope = the write gate + its interaction with the kernel
> spawn-gate. **No code.** Covers both the G1 first-party trust model (plan's baseline) and the G2
> delta when that assumption is dropped.
>
> Evidence base: `kernel/src/loader.rs:104-269`, `kernel/src/signing.rs:61-115`,
> `scripts/sign-cell.py:69-150`, `cells/services/vfs/src/{backend_bin_overlay.rs,access.rs,backend_fat.rs}`,
> `kernel/src/task/syscall.rs:1761-1828`, `gen_disk.ps1:337-431`.

---

## 1. The invariant being unlocked

**Today (`/bin` read-only):** three VFS-cell enforcement points refuse every mutating op on `/bin`
(`backend_bin_overlay.rs:63-68` returns `false`; `access.rs:33` `allow_write_all:false`;
`manager.rs:54` `writable=false`). Consequence: **the only bytes that ever reach a spawnable `/bin`
location are placed there offline by `gen_disk.ps1`** — a trusted, air-gapped toolchain step. At
runtime the set of privilege-capable executables is frozen.

**The spawn-security property that rests on it.** `loader.rs:156` denies spawn of any cell whose
path is not under `/bin/` *and* whose manifest declares privilege (`CellSpawnDenied`). So "capability
= must live in `/bin`." Combined with read-only `/bin`, the invariant is: *the set of
privilege-bearing binaries is fixed at image-build time and cannot grow at runtime.* Every deeper
gate (Ed25519 sig `loader.rs:114-127`, manifest∩policy `loader.rs:245-269`) has historically been
enforced against a static, offline-curated population.

**After the gate opens for `pkg install`, what must remain true.** The property weakens from "fixed
population" to a *conditional*: **nothing reaches a spawnable `/bin` location without the exact bytes
that later spawn having passed Ed25519 verify + manifest/policy intersection.** Phase 01 does not add
that guarantee at write time — it *relies* on the kernel re-applying it at spawn time
(`loader.rs:104` `spawn_gated` runs on every spawn against the bytes as they are then). The unlock is
therefore safe **iff** two sub-invariants hold that the read-only rule used to give for free:

- **I-1 (content authenticity):** bytes that spawn are fleet-signed. — Re-verified at spawn. Holds.
- **I-2 (role/identity binding):** the binary occupying name `N` is the *legitimate* binary for role
  `N` (esp. bootstrap/trusted-core). — **NOT a spawn-gate property** (see §2, A2). The read-only rule
  used to guarantee this implicitly; after the unlock it is guaranteed *only* by the VFS-cell
  denylist. This is the load-bearing shift the plan under-states.

---

## 2. Attack enumeration (ranked by severity)

Legend: **CONFIRMED-open** = exploitable against a naive P01 · **CLOSED** = an existing mechanism
already stops it · **NEEDS-DESIGN** = not exploitable under G1 assumptions but requires a decision
before G2 / before the assumption is relaxed.

### A1 — Caller-name identity spoofing on the write gate — **CONFIRMED-open (G2) / accepted-risk (G1)** — severity CRITICAL (G2), MED (G1)
The plan's gate option 2 authorizes writers by **cell name** (`ProcessInfo.name` via `sys_get_procs`),
allowing only `pkg`/`shell` (phase-01 §gate 2). Two problems:

1. **Writing `/bin` needs no capability token** — it is a VFS IPC op, not a kernel-gated syscall. The
   VFS-cell name check is the *entire* boundary. There is no `WriteCellStore` cap in the manifest
   (`manifest.rs`), so the kernel offers VFS no capability basis to authorize on; VFS must invent one
   from name.
2. **A cell's name is its spawn-path basename** (`loader.rs:170` `name = path.rsplit('/').next()`),
   and for `SpawnFromElf` the path is **caller-supplied and advisory** (`syscall.rs:1787`,
   `loader.rs:100-103`). A `SpawnCap` holder can spawn a child from path `/tmp/shell` → the child's
   name is `shell`, it gets the user cap-ceiling (non-`/bin` → no privilege, `loader.rs:156`), **but
   its name still reads `shell`** to `sys_get_procs`. That child then passes the name-based write
   gate and can write `/bin` freely.

Under **G1 first-party** the only `SpawnCap` holders are `init`/`shell`/`supervisor` (all trusted),
so name-spoofing is out of scope *by assumption* — accepted risk, MED. The moment a third-party or
compromised `SpawnCap`-bearing cell exists (G2), the gate is forgeable → CRITICAL. The plan's phrase
"stable across restart, not forgeable by a Rust cell under G1" is only true because of the trust
assumption, not because the mechanism resists forgery. **Hardening (G2):** authorize by the singleton
shell's *tid* resolved from the service registry, or by a kernel-issued write capability — not by name.

### A2 — Role/identity confusion: any fleet-signed cell can masquerade as any `/bin` name — **CONFIRMED-open** — severity CRITICAL
`spawn_gated` verifies "*is this signed by the fleet key* AND *are its caps ⊆ policy(path)*"
(`loader.rs:114-269`). It **never checks that the binary is the correct one for its filename.** The
signature authenticates the *publisher*, not the *role*. Therefore, if the VFS denylist is bypassed or
incomplete, overwriting `/bin/net` or `/bin/supervisor` with *a different but validly-fleet-signed
cell* produces a spawnable replacement the kernel cannot distinguish from the real one.

Crucially, of the 8 protected-core names {`vfs,shell,net,init,supervisor,config,block,platform`}, only
6 are baked into the VIFS1 ramdisk (`gen_disk.ps1:348-366`: init, shell, vfs, config, platform, block)
and thus read-shadowed (`backend_bin_overlay.rs:35` reads VIFS1 first → a cell-store entry of the same
name is dead). **`net` and `supervisor` are NOT in VIFS1** — they are served live from the cell-store.
For those two, the *only* thing preventing a signed-cell masquerade is the VFS denylist; the spawn-gate
provides zero backstop. This is the concrete proof that the denylist is **not** redundant with the
spawn-gate (contra the plan's "a hostile write is harmless" framing). **Hardening:** the manifest
should carry a role/name field that the sig covers and the loader cross-checks against `path`.

### A3 — Rollback / downgrade of a revoked-but-signed version — **CONFIRMED-open** — severity HIGH
The spawn-gate has no version awareness and no revocation list. A `.prev` copy (P03), or any retained
older signed ELF, re-verifies successfully forever (its fleet signature never expires). `pkg rollback`
(phase-03) explicitly re-installs `.prev` and re-subjects it to the gate — which passes. So a version
pulled for a security bug can be resurrected at will. Re-verify does **not** close this; only fleet-key
rotation does (phase-03 §Security notes this narrow escape hatch, but rotation invalidates *all* cells,
so it is not a per-cell revocation mechanism). **Hardening:** signed monotonic version counter in the
manifest + a kernel min-version floor, or a revocation set in `/POLICY.BIN`.

### A4 — Unsigned ELF metadata: entry-point / segment-permission / load-address tampering — **CONFIRMED-open** — severity HIGH
The signed payload is **PT_LOAD segment *content* (by offset) + `__ViCell_manifest` bytes only**
(`signing.rs:76-112`, `sign-cell.py:69-150`). The ELF header, program headers, and section headers are
**not** signed (module doc `signing.rs:5-13`; note that the doc comment at `signing.rs:59-60` wrongly
claims "covers the ELF header" — that comment contradicts the implementation and should be fixed).
Because verify and the loader both read segments through the *same* (tampered) program headers, an
attacker can alter metadata that content-hashing does not bind, without invalidating the signature:

- **`e_entry`** — redirect execution to any offset inside the signed code (skip a bounds/permission
  check, enter mid-function). Sig still valid.
- **`p_flags`** — flip a data segment to executable or code to writable (W^X / NX defeat) on the exact
  privileged cells that matter. Sig still valid.
- **`p_vaddr`** — relocate a segment's map address. Sig still valid.

This does **not** grant capabilities (the manifest *is* signed; caps stay `manifest ∩ policy`), so it
is HIGH not CRITICAL — but it lets an attacker hijack the control flow / defeat the exploit mitigations
of a legitimately-signed privileged cell. The read-only `/bin` rule previously made this a
build-time-only concern; the write gate turns it into a runtime attack. **This is the single gap most
likely to be wrongly assumed "closed by spawn-time re-verify."** **Hardening:** extend the signed
payload to cover `e_entry`, per-segment `p_flags`/`p_vaddr`/`p_memsz` (a canonical phdr digest), or
enforce W^X + entry-in-first-exec-segment in the loader independent of the header.

### A5 — Signature over a different manifest / ELF-without-manifest — **CLOSED** — severity (would be CRITICAL)
The manifest bytes are appended to the signed payload (`signing.rs:107-112`) and both verify and the
cap-grant path read `__ViCell_manifest` via the same `get_section` (`loader.rs:147`). A manifest
swapped after signing changes the hashed payload → verify fails. A cell with no manifest signs an empty
manifest slice and gets no privilege (`loader.rs:149`, legacy path caps only). Manifest authenticity is
bound to content. Closed.

### A6 — TOCTOU: install-verify vs the file swapped before spawn reads it — **CLOSED** — severity (would be HIGH)
Install-time checks are advisory (plan §Trust model). The kernel **re-reads the ELF fresh and
re-verifies on every spawn** (`loader.rs:71-90` reads bytes → `spawn_gated` verifies the same in-memory
buffer it then loads from — `syscall.rs:1804` for the grant path). There is no persisted "verified"
bit to desync from the bytes. A file swapped between install and spawn is simply re-verified at spawn.
The verify-buffer and load-buffer are one and the same allocation, so there is no verify-then-reread
window either. Closed. *(Residual, not a race: the swap could substitute another A2/A3/A4 payload —
covered there, not here.)*

### A7 — Path traversal / name games on the FAT cell-store — **CLOSED (traversal) / NEEDS-DESIGN (FAT name-folding)** — severity MED
`FatBackend` rejects any path component equal to `..` on write, unlink, and recursive-delete
(`backend_fat.rs:255,281-283,302-304`); `/bin/../evil` strips to `/../evil` → rejected. Null bytes fail
the UTF-8 decode at the syscall boundary (`syscall.rs:1789`). Traversal/`.`/`..`/null → closed.
**Open sub-case (NEEDS-DESIGN):** the denylist (phase-01 §gate 1) compares *names* but FAT is
case-folding / 8.3-aware. Confirm the denylist match is applied to the *canonical* stored name and is
**case-insensitive** — otherwise `/bin/VFS`, `/bin/Net`, or an 8.3 alias (`SUPERV~1`) may bypass a
case-sensitive Rust `==` check while FAT resolves it to the protected name on read. Also confirm long-
name vs 8.3 collision cannot produce a stored entry that reads back as a protected name. Decide the
canonicalization contract in P01 review.

### A8 — Bootstrap-shadow install (dead-code write) — **CLOSED (safety) / MED (UX/DoS)** — severity LOW-MED
Installing `/bin/vfs` (or any VIFS1 name) writes a cell-store entry that is never read (VIFS1-first,
`backend_bin_overlay.rs:35`) → inert. Safe. But a naive `pkg` that reports "installed" misleads the
user and wastes cell-store space (a mini-DoS vector, see A9). Denylist + a `pkg` shadow warning
(plan §Cross-cutting) cover it. The safety property holds regardless.

### A9 — Resource exhaustion (cell-store fill / quota) — **NEEDS-DESIGN** — severity MED
The cell-store is a fixed 32 MB window (`disk.rs:48-54`). `.prev` doubles per-cell footprint (P03).
Repeated installs / upgrades with no eviction can fill it; a full cell-store may also break a legitimate
upgrade mid-write (torn `foo`, recoverable only via a `.prev` that itself may have been the thing being
overwritten). Phase-01 §gate 3 mandates a remaining-space check — ensure it is **pre-write** (reject
before any destructive step) and accounts for the `.prev` copy. Under G1 (trusted installer) this is a
robustness concern; under G2 (third-party) it is a denial-of-service primitive. Confirm the quota check
exists and is ordered before backup/overwrite.

### A10 — `.prev` rollback race / crash-consistency — **CLOSED (safety) / LOW** — severity LOW
The upgrade order is backup-then-overwrite with full-file writes (phase-03 §Data Flow): a crash between
steps leaves `foo` intact + a `.prev`; a crash during the overwrite leaves a torn `foo` that fails the
spawn-gate (fail-closed) and is recoverable from `.prev`. No step produces a spawnable-but-unverified
binary. The one residual is A3 (a `.prev` is a signed old version that re-verifies) — a *design* gap,
not a race. A concurrent spawn during overwrite reads whatever bytes are on disk at read time and
re-verifies them — a torn read fails closed. Crash-consistency of the write gate itself: closed.

### A11 — Capability-review bypass (under-declare at install, over-request at spawn) — **CLOSED** — severity (would be HIGH)
Install-time cap display (P02) reads the *same* signed manifest the kernel reads at spawn. A package
cannot show the user one manifest and spawn with another (A5). The granted set is
`requested ∩ spawner-ceiling ∩ policy(path)` (`loader.rs:245-269`) — monotonic downgrade only; a cell
can never gain a cap by asking. The only "bypass" is cosmetic (a `pkg info` that parses the manifest
differently than the kernel) — a UX bug, not an escalation. The intersection genuinely prevents gain.
Closed. *(Watch: A2/A4 let a signed cell mis-behave within its already-granted caps — that is control-
flow hijack, not cap-gain.)*

### A12 — Key confusion / trust-anchor reuse — **NEEDS-DESIGN** — severity MED
The cell-signing anchor is a single fleet key (`signing.rs:29-33`; dev key gated behind
`dev-signing-key`, prod key a fail-closed zero placeholder until provisioned). Confirm this key is
**distinct** from `/POLICY.BIN`'s signing key (the P5 operator-policy anchor) — reuse would let a
policy-signing capability forge cell signatures or vice-versa. One anchor for *all* cells also means no
per-publisher trust (fine for G1 first-party; the core of the G2 delta, §4). Confirm anchor separation
and document that G1 = one fleet key, all-or-nothing trust.

---

## 3. The safe gate contract (verify-before-place, ordered)

The install path MUST enforce this order. **Bold** steps are the ones whose omission is unsafe; the
rest are advisory UX.

1. **(kernel, unchangeable) Spawn-time backstop.** Every spawn re-reads the bytes and applies
   Ed25519 verify + `/bin`-privilege check + `manifest ∩ ceiling ∩ policy` (`loader.rs:104-269`). This
   is the *only* enforcement that cannot be bypassed by a compromised VFS/shell, because it runs in the
   kernel against the actual bytes at actual spawn time. It makes **content/signature** install-time
   failures non-catastrophic — but see §3 caveat.
2. **(VFS cell, install-time) Writer authorization** — gate the `/bin` write on a *non-forgeable*
   caller identity (tid-from-registry, not name — see A1). Fail-closed if unresolved.
3. **(VFS cell, install-time) Protected-core + canonicalized denylist** — refuse write/unlink of any
   name that canonicalizes (case-fold, 8.3-alias-resolved) to a protected-core name (A2, A7). This is
   **not** redundant with the spawn-gate; it is the *sole* protection for `net`/`supervisor` (A2).
4. **(VFS cell, install-time) Pre-write quota/size check** — reject before any destructive step (A9).
5. **(shell, install-time, advisory) Structural sanity + cap review** — ELF magic, `__ViCell_sig` +
   `__ViCell_manifest` present, render manifest caps to the user (P02). Advisory: makes failure early
   and legible; never a substitute for step 1.
6. **(shell, install-time, advisory-optional) Userspace Ed25519 pre-verify** — reject a bad sig before
   writing (defense-in-depth, plan open-question). Recommended once A4 is addressed, since it lets the
   installer also validate phdr metadata the kernel currently ignores.

**Kernel vs VFS split & why the kernel is the true backstop.** Enforcement that must survive a
compromised installer lives in the kernel (step 1). UX/policy that only needs to hold under the current
trust model lives in the VFS cell / shell (steps 2-6). The kernel step is the backstop **for the
content-authenticity invariant I-1 only.** It is **NOT** a backstop for I-2 (role binding, A2), for
anti-rollback (A3), or for phdr integrity (A4). Those three are guarded *exclusively* at install-time
(denylist) or not at all — so the claim "a hostile write is harmless" is true for forged content and
false for signed-cell masquerade, downgrade, and metadata tampering.

---

## 4. G1 → G2 delta

**The precise G1 assumption being banked:** *every cell that holds `SpawnCap` and every cell that can
issue VFS writes is first-party and non-malicious, and there is exactly one fleet signing key trusted
absolutely.* Under this, A1 (name-spoof) and A2's exploitation path collapse (no adversary to spoof),
and single-key trust is acceptable because all publishers are "us."

**What breaks when the assumption is dropped (third-party packages):**

- **A1 becomes live** — name-based writer auth is forgeable by any `SpawnCap` cell. **Add:** a
  kernel-issued, non-forgeable install capability (`InstallCellStoreCap`) checked at the VFS write, or
  a hardware/attested installer identity. Name-matching must be retired.
- **A2 becomes live for `net`/`supervisor` and any future cell-store core cell** — a third-party cell
  signed by *a* trusted key can masquerade. **Add:** signed role/name field in the manifest, loader
  cross-check `path`↔manifest-role; and **per-package / per-publisher trust anchors** instead of one
  fleet key (A12), so a third-party publisher's key cannot sign a core-role cell.
- **A3/A4 become higher-value** — a hostile publisher will deliberately ship downgrade + phdr-tampered
  variants. **Add:** signed monotonic version + revocation set in `/POLICY.BIN`; extend the signed
  payload to cover phdr metadata (or loader-enforced W^X + entry validation).

**Roadmap cross-reference.** These land naturally on the confidential-computing / hardware-RoT track
already scoped: Layer-2 hardware security (`CLAUDE.md` §Security Model — ARM64 MTE/PAC, x86 PKU/CET,
RISC-V PMP) provides the attested-installer and per-domain isolation primitives; the Silo
(`docs/guides/tier1-silo.md`) provides a kernel-independent key store for per-publisher anchors. The
G2 install-capability should be minted through the same capability machinery as `PcieDriverCap` /
`SupervisorCap` (`loader.rs:301-307`) — a path-gated, kernel-held token, not a name string.

---

## 5. Fail-closed matrix

Every check → its failure action. All failures MUST deny **and** emit an audit event (existing
discriminants: `CellSignatureFailed=22`, `CellSignatureVerified=21`, `CellSpawnDenied`, `CellSpawn` —
`loader.rs:119/125/162/189`). New install-time checks need audit events too (allocate fresh IDs; per
memory, sig events already occupy up to 22 — pick the next free, do not collide).

| # | Check | Layer | On failure | Audit |
|---|-------|-------|-----------|-------|
| C1 | Writer authorized (tid/cap, not name) | VFS install | deny write, no bytes touched | new `CellStoreWriteDenied` |
| C2 | Name ∉ canonicalized protected-core denylist | VFS install | deny write/unlink | new `CellStoreWriteDenied` |
| C3 | Pre-write quota / remaining space | VFS install | deny before any destructive step | new `CellStoreQuotaDenied` |
| C4 | ELF magic + `__ViCell_sig` + `__ViCell_manifest` present | shell advisory | reject early, no write | (shell log) |
| C5 | Userspace Ed25519 pre-verify (opt) | shell advisory | reject early, no write | (shell log) |
| C6 | **Ed25519 sig valid (fleet key)** | **kernel spawn** | **deny spawn, no task created** | `CellSignatureFailed=22` |
| C7 | Signature present (if `signing-required`) | kernel spawn | deny spawn | `CellSignatureFailed=22` |
| C8 | Non-`/bin` cell declares no privilege | kernel spawn | deny spawn | `CellSpawnDenied` |
| C9 | `granted = requested ∩ ceiling ∩ policy` | kernel spawn | grant only the intersection (monotonic) | `CellSpawn` (tid) |
| C10 | Unknown spawner ceiling | kernel spawn | `CapSet::EMPTY` (fail-safe, `loader.rs:258`) | — |
| C11 | Prod build, unprovisioned key | kernel spawn | zero-key placeholder fails every verify (`signing.rs:33`) | `CellSignatureFailed=22` |
| — | Torn/partial write (power loss) | disk | spawn-gate rejects corrupt image (C6); `.prev` recovery | `CellSignatureFailed=22` |
| — | phdr metadata tampering (A4) | — | **NO current fail-closed** — gap; add C5 phdr check or loader W^X | — |
| — | Downgrade to revoked signed version (A3) | — | **NO current fail-closed** — gap; add version floor / revocation | — |
| — | Signed-cell role masquerade (A2, net/supervisor) | VFS install (C2 only) | denylist is sole guard; **no kernel backstop** | `CellStoreWriteDenied` (if C2 catches) |

---

## Summary of open items (rank)

1. **A2 — signed-cell role masquerade (CRITICAL, CONFIRMED).** Denylist is the *only* guard and is not
   redundant with the spawn-gate for `net`/`supervisor`.
2. **A1 — name-based writer auth forgeable (CRITICAL@G2 / MED@G1, CONFIRMED).** Retire name-matching
   for tid/capability.
3. **A4 — unsigned phdr/entry/flags (HIGH, CONFIRMED).** The gap most likely mistaken for
   "closed by re-verify."
4. **A3 — no anti-rollback / revocation (HIGH, CONFIRMED).**
5. A7 FAT name-folding, A9 quota ordering, A12 anchor separation (MED, NEEDS-DESIGN).
6. Closed by existing mechanism: A5, A6, A10, A11 (and A8 for safety).
