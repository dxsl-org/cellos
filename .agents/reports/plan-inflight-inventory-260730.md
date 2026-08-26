# In-Flight Plan Inventory — `.agents/` (2026-07-30)

Analysis only. No code or existing file was modified.

Scope: 79 entries under `.agents/`, of which 76 are plan directories (excluding `reports/`, `hv-logs/`).
Three are analysis/research artifacts with no `plan.md` and no executable phases:
`0-chip-cellos-proposal/` (investment proposal), `260623-remote-cell-ipc-research/` (research report),
`260706-0952-system-analysis-g1-g3/` (system analysis dossier). They are excluded from the table below.

Method: frontmatter `status:` + plan.md phase-table Status cells + phase-file status headers, batch-grepped.
Where the header and the phase table disagreed the phase table won.

## Classification counts

| Class | Count |
|---|---|
| COMPLETE (closed, excluded from table) | 20 |
| IN-PROGRESS (work demonstrably started) | 9 |
| OPEN (planned, never started) | 44 |
| SUSPECT-STALE (claims done; reality differs) | 3 |

### Judged COMPLETE and excluded from the table

ViUI v2 pipeline P01–P07 and the perf series: `260608-1500-viui-core-signal`, `260608-1600-viui-widgets-p02`,
`260608-1700-vi-compiler-p03`, `260608-1800-vi-compiler-p04-codegen`, `260608-1900-viui-build-p05`,
`260608-2100-viui-macros-p06`, `260608-2300-viui-gpu-p07`, `260608-2355-viui-perf-p08`,
`260608-2359-viui-perf-p09`, `260609-0000-viui-perf-p10a`, `260609-0030-viui-perf-p10b`,
`260609-0100-viui-perf-p10c` — every phase-table row reads `✅ Done` / `✅ Complete`.

Also: `260607-1420-h-ext-hypervisor-cap` (2/2 phases done), `260621-0830-cell-perms-p2-p5` (4/4 `✅ done`,
on-disk bake explicitly deferred), `260621-1823-g14-tls-server-auth` (`✅ Done (P00–P03 complete; P04 deferred)`),
`260606-1503-rt-and-service-registry` (`✅ DONE 2026-06-06, commit 5cda48d8`),
`260712-0901-suite-green-3arch` (`status: done (2026-07-13)`),
`260712-1903-thread-cellid-quota-fix` (`status: done (kernel-side)` + Closure Note),
`260726-full-utility-suite` (`status: complete`),
`260712-1836-mythos-g123-analysis` (`status: analysis-complete` — dossier, not an implementation plan).

---

## (a) Every non-COMPLETE plan

Status key: **ACTIVE** = the one plan currently being executed · **IN-PROG** = phases merged, more open ·
**OPEN** = planned, nothing started · **SUSPECT** = claims completion, contradicted by code/git ·
**PARKED** = explicitly shelved by its own text.

| Plan | Status | Goal (one line) | Blocking dependency |
|---|---|---|---|
| `260727-2101-midori-lessons-cellos` | **ACTIVE** | Midori lessons → Cellos: async, no-root, lightweight | 01/03/05 merged; 02/04/06/07/08 open; 09/10/11 newly added. Conflicts with `260712-1000` phase-01 (see (b)) |
| `260624-cell-to-cell-anywhere` | **SUSPECT** | Cell-to-cell IPC across local/LAN/internet (v2, post red-team) | Claims "G1 COMPLETE"; net-broker forwarder is a stub — see (c) |
| `260707-1726-g2-loader-redesign` | IN-PROG | G2 loader: ramdisk boot + virtio-blk driver cell | Phases 01–06 done 2026-07-07; P07 deferred (spike-first); **P08 in-progress** |
| `260607-1543-x86-hal-bringup` | IN-PROG | x86_64 HAL full bring-up | Header `Status: In Progress`; P00 blocks ALL, then 01→02→03→04→05→06 chain |
| `260607-1854-x86-32-aarch32-hal` | IN-PROG | x86_32 + AArch32 HAL, nano profile | `Trạng thái: In Progress`; 4 phases, none marked done |
| `260615-1458-gpio-mmio-el0-fix` | IN-PROG | GPIO MMIO EL0 permission fix, finish peripheral suite | `Status: In Progress`, single phase |
| `260608-1451-viui-next-phases` | IN-PROG | ViUI G1 completion + G2 roadmap | `Status: Active`; 3 of 11 phase files marked done |
| `260616-0755-viui-completion` | IN-PROG | ViUI 70% → production-ready (waves W1–W3) | `Status: Active`; 1 of 7 phases done; overlaps `260608-1451` and `260609-0601` |
| `260609-0601-viui-g2` | IN-PROG | ViUI G2: reactive DSL, flexbox, virtual list, a11y, GPU | `Status: Active`; Wave 1 = P01–P04, Wave 2 = P05 |
| `260724-1632-tier3b-virtio-gpu` | IN-PROG | Tier 3b VirtIO-GPU backend | `status: partial`. **P02 blocked on new prerequisite P00** (hv-arm-gui image); P05 needs 03+04 |
| `260712-0900-spi-peripheral` | IN-PROG | SPI mode 0–3 + software loopback | `status: pending` but 5 ✅ in body; R1 risk: host unit tests blocked by bare-metal default target |
| `260623-1500-tls-server-accept` | **PARKED** | TLS server-side accept (rustls 0.23 dual-stack) | Self-declared: `PARKED — G2, và là PHƯƠNG ÁN DỰ PHÒNG (không phải default G2)` |
| `260712-1001-shell-on-screen` | OPEN (partly dead) | Terminal-emulator cell / shell on screen | Tier A declared **DEAD** — superseded by shipped `cells/apps/fb-console/`; P02 depends on P01 |
| `260712-0800-supervisory-cell-migration` | OPEN | Supervisory cell migration | **Phase 00 depends on P-TRUST (`260712-1100`) landing first**; 00/01/03/04 all touch `kernel/src/task/syscall.rs` |
| `260712-1100-loader-trust-repair` | OPEN | P-TRUST — loader spawn trust-model repair | `status: ready`; nothing started. Gates `260712-0800` |
| `260712-1000-cell-package-distribution` | OPEN | Cell package distribution | `status: pending`; phase-01 writable cell store conflicts with active midori plan (see (b)) |
| `260712-1900-manifest-v2` | OPEN | Manifest v2 | `status: pending`, 4 phases |
| `260712-1901-cap-revocation` | OPEN | Runtime capability revocation completion | `status: pending`, 6 phases |
| `260712-1902-dice-attestation-identity` | OPEN | DICE/RIoT attestation + KMS cell + K2/K3 node identity | `status: pending`, 7 phases with a Depends-on column |
| `260712-0952-tier3b-vm-hardening-compat` | OPEN | Tier 3b Linux VM hardening + compatibility | `status: pending`, 8 phases |
| `260711-1917-tier3b-x86-vtx` | OPEN | Tier 3b x86_64 hardware-virt VMM (boots Alpine) | `status: pending`; VT-x backend + APIC phase depends on P03 + P06 |
| `260722-2330-tier3b-finish-g5-lite` | OPEN | Tier 3b finish + G5 Lite foundations | `status: pending`, 11 phases |
| `260722-0917-g4-full-std-tier1` | OPEN | G4 — full Rust std for Tier 1 apps | `status: pending`; P3 depends on new P2.6 (net readiness engine) |
| `260624-0630-kernel-boundary-cleanup` | OPEN | Kernel boundary Law cleanup — driver cell migration | `status: pending`; P05/P07/P08 all touch `block.rs`, serialize |
| `260613-1200-native-fs-srv-redoxfs-nvme` | OPEN | Native FS `/srv` — RedoxFS + NVMe | P03 blocked on P01+P02; P06 blocked on P05 |
| `260613-1500-rknn-inference-g2a` | OPEN | RKNN inference cell — G2 Level A | `Status: 📋 PLANNED`, 7 phases, has a Depends-On column |
| `260615-1325-vicell-app-sdk-v1` | OPEN | ViCell App SDK v1 | `Status: 📋 Planned`; 04 after 02+03, 05 after 04 |
| `260621-1433-hypha-ai-agent` | OPEN | Hypha — ViCell's first real application | 5 phases, no status markers reached |
| `260621-1823-ostd-http-json` | OPEN | `ostd::http` + `ostd::json` via `libs/http-core` | 3 phases; red-team MINOR: two phases both touch `lib.rs`/`Cargo.toml` → guaranteed merge conflict |
| `260611-0636-net-tools-lookup-service` | OPEN | net-tools → LookupService migration | `status: pending`, single phase |
| `260607-1200-phase-27-protocol-hardening` | OPEN | Phase 27 protocol hardening | `Status: 📋 Planned`; depends on Phase 27 Typed IPC + Syscall Filter (marked complete ✅) |
| `260607-1229-robot-demo-arm` | OPEN | Reference robot demo: sensor → actuator → MQTT on QEMU ARM | 2 phases, no status markers |
| `260607-1600-mmc-subsystem` | OPEN | MMC subsystem | 5 phases; P05 depends on P04 complete |
| `260607-1854-compositor-grant-surfaces` | OPEN | Compositor grant-based surface redesign | `Status: Ready for implementation`; 03/04 depend on 02, 05 on 04 |
| `260607-2038-viui-toolkit` | OPEN | ViUI toolkit | P01–P07 grouped into Steps 1–3; superseded in practice by the ViUI v2 series |
| `260608-1227-viui-embedded-robot-readiness` | OPEN | ViUI embedded/robot readiness | `Status: Planned`, 5 phases |
| `260605-2107-full-reliability-track` | OPEN | Full reliability track — ViCell "never-die" | `status: planned`, 7 phases with Depends-on column |
| `260606-repair-release-build` | OPEN (mostly resolved) | Repair release build + verification gate | `status: planned` but body says `build + boot RESOLVED (2026-06-06)` — see (c) |
| `260605-0958-phase24-perf-kaslr` | OPEN | Phase 24 — performance baseline + KASLR | `Status: 📋 PLANNED`; 19 ✅ are acceptance-criteria boxes, not progress |
| `260605-1406-phase28-wasm-cells-epmp` | OPEN (obsolete) | Phase 28 — Tier 2 WASM cells + RISC-V ePMP | ePMP full **blocked by M-mode architecture**; WASM was dropped from feature docs at commit 8607a16e — see (c) |
| `260605-1538-milestone-2-1-vfs-complete` | OPEN | Milestone 2.1 — complete VFS service | `Status: 📋 PLANNED`; strict 1→2→3→4→5 order |
| `260605-1822-milestone-3-shell-utilities` | OPEN | Milestone 3.1 enhanced shell + 3.2 standard utilities | `status: pending`, 7 phases; largely overtaken by `260726-full-utility-suite` (complete) |
| `260605-2102-milestone-3-4-micropython-enhancement` | OPEN | MicroPython runtime enhancement | `status: pending`; 01→02→03 chain |
| `260605-0738-x5-mqtt-client` | OPEN | Phase X-5 — MQTT client cell | `status: ready`, no external deps ("net IPC already functional") |
| `260604-2018-phase-x-remaining-work` | OPEN | Phase X — remaining ViCell work items | `status: pending`; **conflict zone: 02/03/04 all touch `executor.rs`**, must run sequentially |
| `260604-1512-phase-v-redirect-argv-fix` | OPEN | Phase V — redirect wiring + per-spawn ARGV fix | `status: pending`, no external deps |
| `260604-1023-phase-o-dynamic-httpd-while-loop` | OPEN | Phase O — dynamic httpd + shell `while` loop | `status: pending`, single phase |
| `260603-0717-phase-a-network-tcp-integration` | OPEN | Phase A — network TCP data path + integration tests | Both phases explicitly `Pending` in the table |
| `260603-1803-phase-h-vfs-hardening` | OPEN | Phase H — VFS capability + write hardening | `status: pending`; phases 2→3→4 must serialize to avoid edit conflicts |
| `260603-1922-phase-c-tcp-server-listen-accept` | OPEN | Phase C — TCP server LISTEN/ACCEPT + hostname stub | `status: pending`, 3 phases with Depends-on column |
| `260603-2101-phase-d-ipc-length-lua-tcp` | OPEN | Phase D — IPC buffer length fix + Lua TCP bindings | `status: pending`; Phase 02 functionally depends on Phase 01 |
| `260603-2158-phase-e-udp-dns` | OPEN | Phase E — UDP socket support + Lua DNS resolver | `status: pending`; Phase 02 depends on 01; DNS test depends on QEMU SLIRP |
| `260603-2324-phase-f-lua-scripts-vfs` | OPEN | Phase F — Lua script files + `vfs.*` bindings | `status: pending`; parallel execution would conflict in `main.rs` |
| `260528-2016-vicell-full-implementation` | **SUSPECT** | ViCell complete implementation roadmap (24 phases) | `status: pending` + post-audit note: 12/23 verified, 6 partial. Umbrella plan superseded by the per-phase plans above — see (c)/(d) |

---

## (b) Cross-plan conflicts

### B1 — `/bin` writability: active midori plan vs cell-package-distribution

Side 1 — `.agents/260727-2101-midori-lessons-cellos/plan.md:70-73`:

> **Cross-plan conflict**: `.agents/260712-1000-cell-package-distribution/phase-01-writable-cell-store.md`
> muốn mở `/bin/` writable (đụng đúng `access.rs:33` + `backend_bin_overlay.rs:63-68` mà phase 01/02 siết).
> Precedence đã chốt: **phase 02 làm rule per-cell trước**, pkg plan dùng rule per-cell cho `/bin/`,
> KHÔNG flip `allow_write_all` toàn prefix.

Side 2 — `.agents/260712-1000-cell-package-distribution/phase-01-writable-cell-store.md:11`:

> Unlock a **gated, capability-scoped write** into the FAT cell-store that surfaces read-only at `/bin`

Precedence is already decided in the midori plan's favour, but the pkg-dist phase file has **not** been
amended to record the constraint. Whoever picks up `260712-1000` will not see the ruling.

### B2 — grant-reap path: midori phase 07 vs capability revocation

`.agents/260727-2101-midori-lessons-cellos/plan.md:74-75`:

> **Cross-plan conflict**: phase 07 rewrite đường grant-reap, đụng
> `.agents/260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md`.

`260712-1901-cap-revocation` is `status: pending`, 6 phases, and carries no reciprocal note. Two plans
intend to rewrite the same reclaim path; only one records the collision.

Compounding hard blocker on the same phase — `.agents/260727-2101-midori-lessons-cellos/plan.md:76-77`:

> **Async Pinning Registry** (`docs/specs/03-runtime.md:22-24`, chưa hiện thực) là prerequisite
> cứng của phase 07, và phạm vi phải phủ cả completion queue, không chỉ DMA buffer.

Midori phase 07 is therefore blocked on an unimplemented spec *and* conflicts with a pending plan.

### B3 — supervisory migration gated on an unstarted plan

`.agents/260712-0800-supervisory-cell-migration/plan.md:195`:

> **Phase 00 depends on P-TRUST (.agents/260712-1100) landing first.**

`.agents/260712-1100-loader-trust-repair` is `status: ready` with zero phases started. A 5-phase plan is
parked behind a plan nobody has begun.

### B4 — four concurrently "Active" ViUI roadmaps

`260608-1451-viui-next-phases` (`Status: Active`, 11 phases), `260616-0755-viui-completion`
(`Status: Active`, 7 phases, waves W1–W3), `260609-0601-viui-g2` (`Status: Active`, 5 phases, Wave 1/2),
plus the older `260607-2038-viui-toolkit` (8 phases, no status). All four claim the ViUI G1→G2 surface and
all four number their phases P01–P0n. No plan declares precedence over the others.

### B5 — plan premise already superseded by shipped code

`.agents/260712-1001-shell-on-screen/plan.md:26`:

> Tier A = kernel fb_console keyboard relay | **DEAD.** Superseded by userspace `cells/apps/fb-console/`
> (340 LOC) … spawned by init (`init/src/main.rs:201`).

The plan documents its own supersession (`plan.md:35`: "Nothing new to build; Tier A folds into Tier B"),
yet the frontmatter is still `status: pending` across 3 phases.

### B6 — utility suite: shipped plan vs still-open predecessor

`260726-full-utility-suite` is `status: complete` and its `plan.md:87` says it "Supersedes only the
fidelity limits of completed plan…". Meanwhile `260605-1822-milestone-3-shell-utilities`
(`status: pending`, 7 phases, "Enhanced Shell + Standard Utilities") remains open over the same ground.

### B7 — VirtIO-GPU internal re-sequencing

`.agents/260724-1632-tier3b-virtio-gpu/plan.md:57` and `:145`: P02 (the "pixels appear" milestone)
**depends on a newly inserted prerequisite phase 00** (hv-arm-gui image + display sink), because the
HV-ARM image is headless. `status: partial` — the plan's own ordering changed after work began.

### Intra-plan file-ownership conflicts (serialization required; recorded for completeness)

| Plan | Contended file | Note |
|---|---|---|
| `260604-2018-phase-x-remaining-work` | `executor.rs` | `plan.md:43` "Conflict zone: 02, 03, 04 all touch `executor.rs`" |
| `260712-0800-supervisory-cell-migration` | `kernel/src/task/syscall.rs` | `plan.md:134` 00/01/03/04 all touch it |
| `260624-0630-kernel-boundary-cleanup` | `block.rs` | `plan.md:123` P05/P07/P08 all touch it |
| `260621-1823-ostd-http-json` | `lib.rs` / `Cargo.toml` | `plan.md:54` "a guaranteed merge conflict (red-team MINOR)" |
| `260603-1803-phase-h-vfs-hardening` | shared VFS paths | `plan.md:58` phases 3-after-2, 4-after-3 must serialize |
| `260603-2324-phase-f-lua-scripts-vfs` | `main.rs` | `plan.md:40` parallel run would clash |

---

## (c) False or suspect completion claims

### C1 — CONFIRMED FALSE: `260624-cell-to-cell-anywhere` G1 completion

Claim, `.agents/260624-cell-to-cell-anywhere/plan.md:132,175,238,274`:

> PHASE P00 — Remote-Call API Contract (GATE) `[G1, prerequisite]` ✅ COMPLETE
> PHASE P01 — CellNetId + Ticket + NodeId Binding `[G1]` ✅ COMPLETE
> PHASE P02 — STUN Reflexive Address `[G1]` ✅ COMPLETE
> PHASE P03 — DERP Relay Client `[G1]` ✅ COMPLETE

Reality — the request forwarder does not exist. `cells/services/net-broker/src/main.rs:150-155`:

```rust
fn dispatch(_buf: &[u8], _sender: usize) {
    // TODO P06: route RemoteServiceProxy calls via routing matrix.
    // TODO P08: handle lease request / renew / release.
    // TODO P09: handle enrollment / merge-split messages.
}
```

And `cells/services/net-broker/src/routing.rs:154-157` resolves every remote service to the local broker:

```rust
// Return our own TID as the proxy. Caller will route
// subsequent calls to us; we forward via Noise (P08).
out[0] = RESP_FOUND;
out[1..9].copy_from_slice(&(self.self_tid as u64).to_le_bytes());
```

A remote call terminates at `self_tid` and is then silently dropped by the empty `dispatch()`. The 43 `✅`
marks in this plan describe API-surface landings, not a working data path.

### C2 — SELF-ADMITTED: `260528-2016-vicell-full-implementation` completion was file-existence-based

`.agents/260528-2016-vicell-full-implementation/plan.md:22`:

> **⚠️ HONEST STATUS (post-audit)**: 12/23 phases fully verified-working; 6 phases partial (code exists,
> gaps in runtime verification or feature completion); v1.0-readiness: ~75% by functional tests,
> **100% by file existence**

`plan.md:26` names the partials: "Network (DHCP unconfirmed), Compositor (software path works; GPU hangs),
Shell (I/O echo unverified), Runtimes (Lua/Python build but bare-metal execution unproven)". Same failure
shape as C1 — files land, the path is never exercised. Treat `plan.md:22` as the audit template for any
other `✅` in the tree.

### C3 — STALE IN REVERSE: `260606-repair-release-build`

Frontmatter `plan.md:5` says `status: planned`. Body `plan.md:30` says
`## Status: build + boot RESOLVED (2026-06-06)`. Work landed; the machine-readable status never moved.
Low risk, but it inflates the open-plan count.

### C4 — DEAD PLAN rather than false claim: `260605-1406-phase28-wasm-cells-epmp`

Both halves are gone or unreachable:
- WASM: no WASM crate exists anywhere in the tree (`cells/*wasm*`, `libs/*wasm*` → no matches), and commit
  `8607a16e` is literally *"docs: drop WASM from the feature docs, ratify trust tiers and hardware
  isolation specs"*.
- ePMP: `plan.md:27` — "**ePMP (full)** is blocked by M-mode architecture".

The plan is unstarted (`Status: 📋 PLANNED`) and its premise was retired by decision, not by work.

### C5 — MIXED SIGNAL: `260712-0900-spi-peripheral`

Frontmatter is `status: pending`, the body carries 5 `✅` marks, and one is a literal `status: complete`
string inside a snippet. `plan.md:71` also flags "R1 (High) — host unit testing may be blocked by the
bare-metal default target". Classified IN-PROG above; the owner should reconcile the frontmatter.

### C6 — CORRECTION to the briefing: `260712-1903-thread-cellid-quota-fix` is NOT stale

The briefing stated this plan "still says pending". It does not, as of 2026-07-27:
- `plan.md:4` — `status: done (kernel-side) — VFS-side tracked elsewhere, see Closure Note`
- `plan.md:15` — `## Closure Note (2026-07-27)`
- Kernel fix present at `kernel/src/task/syscall.rs:1416-1422`: `Syscall::Spawn` inherits the parent cell's
  `CellId` "so its allocations charge the parent's quota, not the unlimited CellId(0) slot".
- The active plan already recorded the closure: `260727-2101-midori-lessons-cellos/plan.md:66-69` strikes
  the dependency through and notes "Plan sibling đã được đóng (D3)".

### Completion claims that VERIFY TRUE (spot-checked, no action needed)

| Plan | Claim | Evidence |
|---|---|---|
| `260726-full-utility-suite` | `status: complete` | commit `26a0584e` "feat(shell): full utility suite — grep/sed/mini-AWK/top on a host-testable text engine" |
| `260606-1503-rt-and-service-registry` | `✅ DONE 2026-06-06, commit 5cda48d8` | commit `5cda48d8` "feat(kernel): stable service-ID registry — clients reconnect across respawn". Caveat: its second slice reads "observability slice DONE … commit pending" |
| `260621-1823-g14-tls-server-auth` | `✅ Done (P00–P03; P04 deferred)` | `cells/services/net/src/tls/{roots,provider,clock,rng}.rs` + `libs/ostd/src/tls.rs` all present |

---

## (d) Recommended consolidation — options for the architect

Options, not instructions. Each line is one decision to accept, reject, or defer.

### Close as done (frontmatter fix only)

1. `260606-repair-release-build` — the body already says RESOLVED (`plan.md:30`); only the frontmatter lags.
2. `260712-1001-shell-on-screen` Tier A — the plan itself calls it DEAD and superseded by the shipped
   `fb-console` cell; close the plan, or shrink it to the Tier B terminal only.

### Close as retired by decision

3. `260605-1406-phase28-wasm-cells-epmp` — WASM was removed from the feature docs (`8607a16e`) and ePMP is
   arch-blocked; leaving it open implies a commitment that no longer exists.
4. `260623-1500-tls-server-accept` — already self-declared PARKED as a fallback; a formal close stops it
   reading as in-flight.
5. `260605-1822-milestone-3-shell-utilities` — superseded by the completed `260726-full-utility-suite`;
   close, or reduce to the residue the shipped suite does not cover.

### Reopen or downgrade a completion claim

6. `260624-cell-to-cell-anywhere` — flip the G1 header off "COMPLETE" and add a phase for the forwarder
   (`main.rs:150-155` `dispatch()`, `routing.rs:154-157` self-TID). The plan currently asserts a capability
   the code does not have, which is the most dangerous state in this inventory.
7. `260528-2016-vicell-full-implementation` — retire as an umbrella roadmap; its 24 phases are already
   re-planned as the standalone `260603-*`/`260604-*`/`260605-*` plans. Keep only its `plan.md:22` audit
   note, promoted into `docs/` as a verification-debt register.

### Merge

8. Fold `260608-1451-viui-next-phases` + `260616-0755-viui-completion` + `260609-0601-viui-g2` +
   `260607-2038-viui-toolkit` into one ViUI roadmap — four "Active" plans over one subsystem means none is
   authoritative and their phase numbering collides.
9. Fold `260712-1900-manifest-v2` + `260712-1902-dice-attestation-identity` +
   `260712-1000-cell-package-distribution` — all three gate on package identity/signing, all three are
   `pending` with no started phase.

### Sequence explicitly rather than leaving it implicit

10. `260712-1100-loader-trust-repair` → `260712-0800-supervisory-cell-migration`: promote `plan.md:195`'s
    dependency into `260712-1100`'s frontmatter so the gate is visible from the blocking side too.
11. `260712-1000-cell-package-distribution/phase-01` — copy the precedence ruling from
    `260727-2101-midori-lessons-cellos/plan.md:70-73` into that phase file (per-cell rule for `/bin/`,
    never `allow_write_all`). The decision exists in exactly one place today.
12. `260712-1901-cap-revocation/phase-02-selective-grant-reclaim.md` — add the reciprocal conflict note
    against midori phase 07, and record that midori 07 is itself blocked on the unimplemented Async Pinning
    Registry (`docs/specs/03-runtime.md:22-24`).

### Defer explicitly (keep, but mark not-now)

13. The `260603-*` / `260604-*` network and shell phase plans (A, C, D, E, F, H, O, V, X) — 9 plans, all
    `status: pending` since early June, all partly overtaken by later net/shell work. Marking them deferred
    with a one-line reason each stops them reading as active work.
14. `260605-0958-phase24-perf-kaslr`, `260605-1538-milestone-2-1-vfs-complete`,
    `260605-2102-milestone-3-4-micropython-enhancement`, `260605-2107-full-reliability-track`,
    `260607-1200-phase-27-protocol-hardening`, `260607-1229-robot-demo-arm`, `260607-1600-mmc-subsystem`,
    `260607-1854-compositor-grant-surfaces`, `260608-1227-viui-embedded-robot-readiness`,
    `260611-0636-net-tools-lookup-service`, `260613-1500-rknn-inference-g2a`,
    `260615-1325-vicell-app-sdk-v1`, `260621-1433-hypha-ai-agent` — 13 never-started plans with no
    dependency on current work. Deferring them shrinks the apparent in-flight set from ~53 to roughly a
    dozen.

### Keep active

15. `260727-2101-midori-lessons-cellos` is the single active plan. Its open phases 02/04/06/07/08 plus new
    09/10/11 should be the only ones drawing effort until B1 and B2 are written down on both sides.
