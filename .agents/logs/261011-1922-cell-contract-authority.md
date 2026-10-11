# 2026-10-11 — Cell contract and authority cutover

## What happened
Created ADR-0024, Spec 25 with 29 observable criteria, and a 10-phase implementation plan. Inventoried 161 legacy plans and applied 38 scoped execution locks while preserving historical progress/evidence. No runtime code or ABI changed.

## Decisions
- Cell is principal/incarnation; threads share CSpace and aggregate budgets, not TLS or receive context.
- Kernel Cell is exceptional root substrate, not a transferable root login or normally restartable actor.
- Typed object capabilities plus immutable admission ceiling/explicit receive policy; retain issuer-owned VFS/NET semantics.
- Recovery needs final committed watermark and writer fence; remote create uses durable intent/keyed publication/reconciliation.
- Remote mutable authority fails closed under partition; relative timeout does not prove origin absolute expiry.
- POSIX optional; native TLS/futex/justified stream mechanism remains kernel-neutral.

## Lessons
- Shared completion ring currently has one waiter; task CPU ticks are not aggregate Cell enforcement.
- Four specialist reviewer jobs failed at provider; direct adversarial review completed, no independent PASS claimed.
- Document smoke passed: 161 inventory rows, 38 reciprocal locks, 10-phase acyclic DAG, 29 AC owners, 201 links and 3 rejecting negative controls.

## Next steps
Use `.agents/261011-1200-cell-contract-authority/plan.md` P01 only for exact ABI/migration/adoption inventory. Keep old plans locked until explicit scoped handoff. Law-1/protected relay/Intel physical/VMX/production gates remain separate; no implementation claim follows from docs.
