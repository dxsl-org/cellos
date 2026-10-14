# 2026-10-12 — Producer/close protocol

## What happened
Specified private x86 producer admission/slot reservation and final freeze/drain, integrated trace/P01/overview. No implementation/code/ABI change or commit.

## Decisions
- Native STOP+active atomic gate; one strongCAS per admission and slot reservation. Contention invalidates observation, never spins or changes underlying operation.
- Record-ready Release/Acquire and balanced guard Release; counter-only observers also enter gate. Admission isn't mutual exclusion or auxiliary memory safety.
- Safe-point STOP cutoff/Acquire drain; no collector wait under subsystem locks. Reserved-but-unready/faulted writer means incomplete, no forced guard reset.
- Late selected work invalidates bounded capture; host must consume through final validity sampling/termination, not accept an earlier final line.
- Actual Rust/weak-memory/owner coverage remains an implementation-review/runtime gate.

## Verification
Finite strong-atomic model:20774interleavings across normal/one-slot/counter-only/fault-after-reservation cases. Modeled overflow/fault cases have0complete final states. Active count/unique slots/ready-after-write/STOP/stale-admission invariants held. Not kernel execution, success rate or hardware timing proof.
30docs/321local links/0missingpaths; acyclic10pendingphases/38reciprocal locks/adoptionrows/50ACowners;3graph negative controls;3174source hashes unchanged.

## Next steps
Review actual source-bound guard/auxiliary access/coverage before instrumentation implementation. Resource measurements, numeric profiles and exact checkpoints remain pending.
