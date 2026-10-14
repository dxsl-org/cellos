# P01 — Two-CPU measurement launch readiness

## Decision
Keep normal shell authority unchanged. Dedicated measurement image should reuse the init-owned opt-in fixture pattern; exact launcher/child edges, feature scope and signing ceilings require review before code. No implementation, policy widening, lock release or commit.

## Observed
Same source-rebuilt ISO booted q35/TCG/qemu64+pdpe1gb/256M/two CPUs, no network. Existing topology/handoff/shootdown/SAS/pin-dying boot markers present. Shell exec /bin/bench async-lifecycle denied by launch edge with spawn_cap=false and supervisor_cap=false; no scenario START/PASS. Existing smoke script uses a bench command absent from current shell sources. Do not repeat denied launch as a test or substitute another authority grant.

Allocator before/after rejected launch: total245680/used25752/free219928 KiB, arithmetic consistent. This is not workload reclamation or per-CPU marginal cost. Known VMM failure appeared incidentally; no overall PASS.

## Evidence
Plan directory: .agents/261011-1200-cell-contract-authority
- evidence-2026-10-12-service-x86-smp.log
- service-x86-smp-provenance.json
- measurement-readiness.md and plan.md updated
- Verification:25documents/291local links/no missing paths;10pending phases/38blocked plans. Exact ISO digest and recorded boot markers match evidence.

## Next
Detail/review dedicated opt-in launcher contracts within P01, then implement only after authorization; bounded async scenario first, distinct accounting workloads for marginal/high-water/CPU/SMP. No contract qualification inferred from existing boot markers.
