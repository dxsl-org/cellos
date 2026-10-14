# 2026-10-12 — Cell contract red-team completion

## What happened
Completed the 2026-10-11 review package for `.agents/261011-1200-cell-contract-authority`. Four completed independent lenses yielded 14 raw findings, eight unique gaps and five accepted Major groups; all remain plan-level, not runtime findings.

## Decisions
- Maintainer selected independent fixed-home Budget Authority enforcement; controller owns policy only. New claims require live Authority participation, trading partition availability for finite aggregate enforcement.
- Issuer-precommitted keyed genesis resolves control bootstrap; immutable maxima bound mutable allocations.
- Receiver-start challenges reject delayed unseen sequences. Old ingress generations freeze both scale directions until fenced reconciliation; demand/T is homogeneous-domain only.
- Destination Ready provenance and immutable origin-local single-target sealing prevent controller/router confused-deputy gaps.
- Spec 25 v1.3 has 50 pending criteria; Spec 26 v1.1 supplies detail. Ten phases and 38 locks unchanged.

## Lessons
- Destination-local key/replay checks cannot enforce aggregate budget or prevent two-target origin proof reuse alone.
- Provider failures interrupted later edit/wake jobs, not completed initial lens reviews. Parent completed missing edits; no post-amendment independent pass claimed.

## Next steps
- Run `hc-plan validate` interview against the controlling plan.
- P01 non-activating contract/ABI inventory before implementation; exact approval and protected/hardware/production gates remain mandatory.
