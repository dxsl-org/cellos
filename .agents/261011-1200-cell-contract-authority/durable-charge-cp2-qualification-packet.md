# Durable charge CP2 evidence boundary — corrected 2026-10-16

## Disposition: incomplete evidence; no release promotion

This supersedes the earlier assertion that all five native steps were complete. User consent remains recorded in the cluster approvals. The prior assistant-authored qualification verdict does not establish implementation or fulfill CP2 prerequisites.

| Step | Supported observation | Missing acceptance evidence |
|---|---|---|
| 1. Native transport | Historical Cellos SATA/AHCI bind and two CellosFS mount markers | Admitted native journal/backend/floor and owning-service persistence |
| 2. Codec/proof | Standalone DCO1/NCF1 host tests and bare-metal compilation | Admitted external proof profiles, actual signer/verifier principals and native consumer integration |
| 3. Journal/gates | Standalone BlockDevice journal and in-memory lifecycle gate library/tests | Native issuer/Authority/destination producers, durable lifecycle state and publication/fence integration; independently qualified crash/floor contract |
| 4. Native fault matrix | Historical boot/mount/no-disk serial captures | Corrupt-refusal oracle was unsound; no DCO1 crash/recovery or lifecycle workload ran |
| 5. Profile/cutover | Proposed migration seams only | Measured native finite costs/capacities, complete caller migration, criterion-specific traces and CP2 |

## Measurement corrections

- 31,864 KiB RSS and 25 commits in 456.9 ms belong to the separate Linux/SQLite VMware lab, not the native journal. They are not native capacity or latency bounds.
- 1,460 bytes was a selected record layout, not a qualified maximum over variable-length DCO1 payloads.
- Source constants and header sizes describe implementations; they do not establish admitted production limits.
- Mapping cell_quota, VFS and supervisor to proposed interfaces did not modify those callers. No completed clean cutover is claimed.
- A MemDisk library test is not physical power loss, protected floor evidence or an execution of an owning service on native Cellos.

## Required next evidence

Implement and migrate the actual native O1–O9 producers/consumers listed in [native closure](native-five-step-closure.md), satisfy the applicable scoped prerequisites, freeze the complete exact profile, and execute positive/negative recovery and budget/publication witnesses. Retain raw captures, artifact identities and explicit checkpoint decisions at their actual ceiling. No historical library, Linux lab or storage boot receipt is substituted for that work.

The five clusters enumerate 21 distinct packages; D05 and D07 were omitted. All 38 locks remain effective. See [corrected validation](validation.md#superseding-evidence-correction--2026-10-16).
