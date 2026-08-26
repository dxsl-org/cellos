---
title: "Dossier 6 — Tier 3b VMM hardening verdicts (backing isolation, P05 scope, C1 IRQ cap)"
description: "Adjudicates the three open architecture decisions the 260712-0952 red-team surfaced: per-VM backing isolation (M1/A2), P05 Debian/glibc scope balloon (F1/F2), and the LIVE C1 IRQ-queue DoS. Feeds the existing plan; does not replace it. Analysis-only."
status: verdicts-final (feed into .agents/260712-0952-tier3b-vm-hardening-compat)
window: mythos-analysis-only (expires 2026-07-14)
created: 2026-07-12
---

# Dossier 6 — Tier 3b hardening verdicts

The `260712-0952` plan is already red-teamed and phase-shaped (P01-P03 = docs/threat
this window; P04-P08 = code after). Three decisions were left open. Verdicts below,
grounded in agent-verified code at HEAD.

## Verified baseline (so the verdicts are grounded, not assumed)

- **Guest RAM access is already bounds-checked kernel-side.** `write_guest_memory`/
  `read_guest_memory` (`registry.rs:301-376`) validate the GPA via `checked_sub`/
  `checked_add` and clamp the end to `guest_pages * PAGE_SIZE`. No guest can read/
  write outside its window today. Good — the memory-window isolation the plan asserts
  is real.
- **Backing store is a single shared 16 MiB `Vec`, zero-filled, volatile.**
  `virtio_blk.rs:15-33` allocates one `Vec<u8>` of `DISK_SIZE`, not per-VM, not disk-
  loaded, lost on cell restart. `blk_read`/`blk_write` (`:90-116`) index it by
  `sector * SECTOR_SIZE` with an in-bounds check against the Vec length.
- **C1 is LIVE.** `registry.rs:398` `inject_irq` does `q.push_back(intid)` with **no
  depth cap**; the two descriptor-parser gaps (`cur` unclamped vs `q_size`,
  `avail_idx` delta unbounded) are unfixed (`05-application.md:277-280`).

## Verdict 1 — Backing isolation (M1/A2): per-VM image-file backing MANDATORY the moment backing becomes shared-or-persistent

Today's single volatile Vec with one VM is **not yet** exploitable — there's no
second VM to escape into and nothing persists. But the red-team's `sector → offset`
escape becomes real the instant P04/P05 does either of two things: (a) supports a
second concurrent VM against the same backing, or (b) persists backing to a
disk/cell-store region. The failure mode: VM-A writes sector S; if the backing is a
shared cell-store region, S maps to a byte offset that another VM — or the host
cell-store itself — also addresses → cross-VM/host corruption, and the kernel's
guest-RAM bounds-check does **nothing** here because this is the *block* path, not
the *memory* path.

**Verdict:** P04's writable/persistent backing MUST be a **per-VM image file (or
per-VM partition), never a shared cell-store region**. Each VM gets its own backing
object; the VMM addresses it by (vm_id, sector) with the sector bound-checked
against *that* VM's image length (the check already exists per-Vec at `:94/:107` —
the fix is *one backing object per VM*, not sharing one). This matches the plan's
own M1/A2 lean; this dossier confirms it as non-negotiable and states the exact
trigger (shared **or** persistent) so P04 can't accidentally ship a shared-store
shortcut "just for now."

## Verdict 2 — P05 scope (F1/F2): freeze the graduation deliverable at Alpine/musl; Debian/glibc is a SEPARATE opt-in phase

The red-team found P05 balloons M→XL because Debian glibc needs root-on-blk, a real
disk image, a RAM bump, and an init system — a different beast from the Alpine boot
that already works. Letting "Debian support" ride inside the hardening phase couples
a compatibility stretch-goal to the security deliverable, so neither ships cleanly.

**Verdict:** **split.** The hardening phase's compatibility deliverable is
**Alpine/musl parity on the hardened VMM** (the existing, working path, now running
under bounded/isolated backing). **Debian/glibc root-on-blk becomes its own opt-in
follow-up phase** with its own effort budget, gated behind the hardening phases being
green. This keeps the security work (backing isolation + C1 + bounds) on a fixed
scope and lets the glibc stretch move at its own pace. The user chose "glibc guest +
writable virtio-blk" as a *goal* (memory: tier3b plan) — that goal is preserved, just
sequenced as a distinct phase rather than smuggled into hardening.

## Verdict 3 — C1 IRQ queue DoS: bounded ring + IRQ coalescing (the resource-exhaustion principle's first instance)

`inject_irq`'s uncapped `push_back` lets a guest mask its IRQ line and spam
`QueueNotify` → the kernel-side pending-IRQ queue grows without bound → kernel-heap
OOM → **SAS collapse** (one guest kills the whole system — the exact opposite of
never-die). This is the single must-fix-now item in the Tier 3b set and it is LIVE.

The fix is mechanical once the semantics are decided, and the semantics are clean:
**pending interrupts are collapsible.** A given INTID either is or isn't pending —
delivering it once vs queuing it 10,000 times is semantically identical to the
guest. So:

**Verdict:** replace the unbounded `VecDeque::push_back` with a **bounded pending-IRQ
representation that coalesces duplicates** — either (a) a fixed-size ring with
drop-on-full, or preferably (b) a **bitset/small set of pending INTIDs** (one bit per
supported interrupt) so duplicate `inject_irq(intid)` is idempotent and the structure
is O(max_intids) regardless of guest behavior. Option (b) is both smaller and
strictly correct (no legitimate IRQ is ever dropped; only redundant re-injections
collapse). Pair it with the two descriptor-parser bounds (`cur < q_size`;
`avail_idx - last_avail` capped at `q_size`) — those are straight bounds checks.

**Architecture note this instantiates:** spec `05-application.md:284` states the
principle "a guest must not exhaust kernel queues." C1 is its **first concrete
instance**, and the fix pattern — *bound every guest-triggered kernel queue by a
ceiling, coalesce where the semantics allow* — should be written into the threat
model (P02) as the rule, with the IRQ set as example, so the descriptor-ring and any
future guest-driven kernel queue inherit the same discipline. That elevation from
"fix this one queue" to "here is the invariant for all guest-triggered kernel state"
is the P02 deliverable; the IRQ/descriptor fixes are its P06 implementation.

## Dependency on P-TRUST (dossier 1)

The plan notes P02's resource-exhaustion cap "requires kernel changes" and depends on
P-TRUST. Confirmed compatible: P-TRUST is a spawn-gate/cap change and does not touch
the VMM IRQ path, so the C1 fix can proceed independently — the only shared thread is
that both enforce "a cell/guest cannot exceed a kernel-enforced ceiling." No ordering
constraint between C1 and P-TRUST; they are parallel-safe.

## Summary for the plan

| Open decision | Verdict | Phase | Tag |
|---------------|---------|-------|-----|
| Per-VM vs shared backing (M1/A2) | per-VM image/partition, mandatory once shared-or-persistent | P04 | [ARCHITECTURE-DECISION → resolved] |
| P05 Debian/glibc scope (F1/F2) | freeze P05 at Alpine/musl; Debian = separate opt-in phase | P05 split | [ARCHITECTURE-DECISION → resolved] |
| C1 IRQ DoS + descriptor bounds | bounded coalescing set (bitset) + `cur`/`avail_idx` bounds; write the exhaustion invariant into P02 | P02 (invariant) → P06 (impl) | [was LIVE → fix specified] |

All three feed the existing `260712-0952` plan. No new plan needed — this dossier is
the decision record its P01-P03 (this window's docs/threat work) should cite.
