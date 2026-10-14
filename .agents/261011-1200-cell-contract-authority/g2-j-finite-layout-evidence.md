# G2-J finite-layout evidence — proposed r2

2026-10-14. Companion to [preservation design](g2-j-preservation-design.md). Source geometry and a conditional set model, not runtime implementation, media qualification, CP1/CP2 or a production profile.

## Exercised certificate
Current review correction: r1 cut rows were only emitted, not validated, and omitted bootstrap/torn one-root and reservation aliases. Do not treat that historical receipt as corrected transition proof. [Reviewed v2 certificate](../../build/g2-j-reviewed-finite-layout-certificate.json) runs14bootstrap/two-root transitions and11negative controls, including absent/valid lost replies, early promotion/resume/reclaim, unreserved successor, outstanding reservation alias and duplicate assignments. Geometry is unchanged. [Review](g2-j-proposal-review.md) and [limited grant](g2-j-cow-fault-authorization.md) distinguish this model from actual CoW runtime evidence.

Historical r1 command exited0 (geometry/set checks only):

```sh
python3 scripts/verify-g2-j-finite-layout.py --out build/g2-j-finite-layout-certificate.json
```

[Receipt](../../build/g2-j-finite-layout-certificate.json) pins actual format/engine/allocator/inode/disk bytes. Output uses exclusive creation; a subsequent run must name a new output. Initial run refused an unsupported constant expression; the parser was corrected to match the actual `8+1+1+MAX_NAME_LEN` expression, not the misleading source comment. No Rust build or runtime fault run was performed.

| Source-derived or selected-session quantity | Bound | Evidence class |
|---|---:|---|
| Block / inode / inline bytes | 4096 / 512 / 428 | Actual encoded source constants, not Rust struct size |
| Directory entry / directory capacity | 56 bytes / 7 entries | Actual expression and integer division |
| Indexed inode numbers | 256 | 32 table pointers × 8 records |
| Session directories, including volume root and `cellos` parent | 11 | Explicit selected namespace |
| Immutable record files | 12 | tx4×2 + cp2×2 |
| Total created inode numbers | 23 | Corrected parent-inclusive bound |
| Inode-table blocks per complete session root | 3 | ceil(23/8), fresh exclusive volume only |
| External record-data blocks per root | 6 | Six436-byte records exceed428 inline;184-byte references remain inline |
| Two recoverable roots + full candidate + reserved blocks | 30 blocks | Conservative disjoint3×(3+6)+2superblocks+1bitmap |
| Four reachability/reservation bitsets for selected64MiB volume | 8192 bytes | 4×ceil(16384/8), storage arithmetic only |
| One block buffer / encoded-record buffer | 4096 / 436 bytes | Proposed reusable scratch buffers, not total stack/heap |

The previous21-inode statement excluded the volume root and `cellos` parent outside the nine-directory journal subtree. It is not a volume-wide fit certificate. Existing unrelated inodes or a nonfresh `next_inode_num` invalidate the23-number premise. `create_node` monotonically consumes numbers; unlink does not reset it. A bounded occupancy therefore cannot certify unbounded slot churn. Selected session uses slots ONCE, has no GC/recreation and refuses before further dispatch.233 unused inode numbers are not permission for extra transactions or implicit reset.

The30-block certificate bounds the selected filesystem graph only, with no other volume writer, no old unrelated reachable blocks, no extra data/dirty metadata, exact one-block maximum external records and complete graph validation. Physical allocation alignment/driver queues, VFS objects, authority records and journal producer memory are excluded. All prior roots and the candidate are charged together; sharing can lower observed use but is not subtracted from this upper bound.

## Conditional model evidence

The corrected executed v2 model validates14publication transitions across fresh one-root bootstrap and later two-root operation, including a torn target leaving only active. Candidate allocations must be reserved, disjoint from valid roots/prior reservations and unique in allocation events. Eleven negative controls reject omission, reuse, alias, capacity/charge failure, premature promotion/resume and reclamation. Original r1 six-row/five-control output remains historical and is not transition proof.

This model does NOT execute CellosFS writes, enumerate sector permutations, verify canonical codecs or prove graph traversal correctness. Lost-FS2 reply covers both absent and valid successors, without mutation resume or reclamation. Actual fault recovery is read-only then discard under the limited grant; no floor/charge recovery is implemented. Model acceptance is conditional on preserving these sets; current engine does not yet do so.

## Missing certificates, explicit proof inputs

| Certificate | Exact remaining proof input / owner |
|---|---|
| Compiled finite scratch | Fixed traversal stack23 inode identities, visited/conflict metadata, two root descriptors, checked dirty table scratch, buffers/bitsets, locks and diagnostics with actual compiler layout and peak call-stack; filesystem owner. Do not add4096/436 to8192 and call it total memory. |
| Complete allowed-state graph | Source-pinned validator and CoW publication; prove every create/write/checkpoint/failure path fits3 tables/6 data per root and no inaccessible inode/table survives outside declared roots; actual sector/barrier cut evidence. |
| Unknown/lifecycle storage | Exact same-T reservation/result/owner-transfer/retirement codec and bounded producer; actual owner-death/restart witness, no refunded ambiguous charge; P05/P06/P08-C. |
| Global heap1024 / physical history8192 | Full background/prearm/scratch/realloc/failure producer graph and lifetime births including reuse.30 live filesystem blocks cannot establish nonrecycled history fit. MP01 remains open. |
| Retained ranges2048 / obligations128 | Complete resource transitions, interval fragmentation and owner/ref/deferred lifetimes; repeated publication and failure paths. Filesystem root bitsets do not replace kernel obligation/range proofs. |
| Event-complete resource qualification | Disjoint per-owner highwater, coherent settled observations, full memory topology/residual interpretation; G1 CPU accounting does not close MP02/MP03. |
| Production journal profile | Authenticated full checkpoint fits canonical body or separately approved larger encoding; arbitrary Authority state does not fit256 by assertion. Real producer costs, finite retention/compaction, admitted issuer and parent/destination vector needed. |
| Backend/floor | Actual device/cache/driver/flush/nonregression tuple and authenticated fence/floor; separate hardware/rollback evidence. SHA/CRC and superblock sequence cannot provide these. |

## Proposed next verification after separate authorization

Compile actual fixed data structures and report sizes/stack, then run real engine fault injection across all eight sectors of every4KiB write, both filesystem barriers and both journal barriers, with fresh read-only recovery and old/new bytes compared. Exercise capacity refusal BEFORE dispatch, retained Unknown/owner loss, both roots' reachability and safe later reuse. Keep every failed case. Actual device qualification follows separately. Do not flip readiness flags, discard old matrices or select numeric production budgets from this certificate.
