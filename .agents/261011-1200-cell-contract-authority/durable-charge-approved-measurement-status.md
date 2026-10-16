# Durable charge approved choices and VMware evidence

## Effective user decision

The user explicitly approved the presented contracts/choices and instructed that missing measurements use VMware. [Exact scoped approval](durable-charge-user-approval.json) pins the five reviewed documents and preserves full C03 r1/C05 r2 predecessor bytes with their SHA-256 values. Older candidate headings saying “unapproved” are historical preparation status; this decision approves their presented contract and structural choices. It does not assert nonexistent deployment facts.

The governed preparation integration is now C03 r2 (fixed-home-only credit, no-credit issuer settlement and B4 custody) and C05 r3 (global prospective charge/debit separate from independently required local growth admission). Their actual bytes/revisions are pinned in the preparation manifest. This closes those semantic-choice conflicts, not their complete exact-package CP1/CP2 requirements. No 23-package blanket approval, phase completion or lock release is recorded.

## VMware tuple and production-runner refusal

Observed VMware Workstation 25.0.0 build24995812; vmrun1.17.0.24995812, Windows executable reached from WSL. The existing runner creates a disposable BIOS/HPET, one-vCPU/256MiB VM, IDE CD-ROM and COM1 file; networking and persistent disks are disabled. Only its own VM is hard-stopped and its absence verified.

The first run used `build/vicell-x86.iso` SHA256 `06f1317e96e143126596c8e8ee42885d2cf6b9114a59d9d1e5486346c32024e8`. It **FAILED** the production runner's existing assertion because the image reports `Tier 2 admission: ENABLED (development profile)`. Cleanup was confirmed. Evidence: `/mnt/c/Temp/cellos-vmware/run-20261015T064047Z-z5xux70z/result.json` and adjacent serial/VMware logs. The assertion was not weakened and the failed invocation was not rerun to manufacture a PASS. This is an image/lane mismatch, not evidence of production admission-disabled compatibility.

## Pinned P01 control observation

A separate disposable diagnostic harness ran the existing immutable `parked-footprint:0` image for the full150-second observation window and decoded its retained serial with `p01_trace.validate_stream`. It did not forge QEMU metadata, call the QEMU-specific capture validator or change any permanent source/parser. The harness and exact capture hashes are retained in the build evidence receipt.

- Build manifest SHA256: `d8a1225beccdc682947a12aebd9dd44396ed0277e36b64603848e27d9bc3f917`.
- Control ISO SHA256: `f3e732bcd1287c661eca4f910ebe247fb3b29a1e984485348482cca08826d188`.
- Capture: `/mnt/c/Temp/cellos-vmware/durable-charge-footprint-hjmlbd_l/46d7fdd4-a187-475e-9b38-39ef6a9771f8/`.
- Stream structurally complete; **fully_settled=false**; VM cleanup confirmed.
- Blockers include heap ownership capacity/coverage, physical semantic/topology coverage, pending/retained/live component, cleanup/Unknown accounting and raw heap/quota/frame residuals.

The retained same-build QEMU control receipt already reports the identical blocker set: `build/p01-budget-stage-ownership-0-captures/ed6cfef7-618e-4c57-8d61-35f0a081aae4/evidence.json`. This comparison uses existing evidence; it is not a new QEMU run and does not attribute every residual to VMware or classify it as a leak. No tolerance or control subtraction was invented to mark settlement successful.

## Remaining closure boundary

VMware workload observations can measure this existing root-only fixture. They cannot measure absent durable Authority/controller/router/journal/resize/adoption implementations. The VM has no persistent test disk, so it supplies no D1/D2, cache/barrier, crash-preserved debit, floor, custody-transfer or one-credit runtime evidence. It also cannot provision public pins/role grants or prove independent protected anti-rollback/physical power-loss persistence.

## Bounded workload and refusal observations

Two additional fresh150-second VMware captures used the same pinned build, separate count8 and denied scenario ISOs. All three diagnostic streams are structurally complete and all VM cleanup checks succeeded. These are one observation per selected scenario, not the five-scenario/three-replica QEMU qualification matrix or a VMware production capacity profile.

| Scenario | Actual workload result | Accounting disposition |
|---|---|---|
| 0 | Lifecycle complete;0admitted/0exited | Fully settled false |
| 8 | Lifecycle complete;8admitted/8exited | Fully settled false;8Unknown-retained obligations remain in final metric |
| denied | DENIED_CONTROL;1requested,0admitted;public Err(Unknown) | Decoder accepts the intentional refusal control; not a normal settled-resource workload |

At count8 live-stage-minus-pre, the observed global deltas are2440used frames,70528heap-used bytes,1179648stack bytes and9011200segment bytes. These metric planes are not added together as a total or divided by8 to claim attributed per-Cell costs. Final-minus-pre retains1728heap-used bytes,8Unknown-retained obligations and-56used frames; this is not a zero-residual or leak-free qualification. No control subtraction or tolerance changes the verdict. The denied control reports no admitted subject or heap/frame delta at its post-refusal stage; its public error remains Err(Unknown), not an invented precise production refusal code.

Count8 capture: `/mnt/c/Temp/cellos-vmware/durable-charge-footprint-6pzohf49/b1019445-a548-4eb5-beca-d1b7954b15ae/`. Denied capture: `/mnt/c/Temp/cellos-vmware/durable-charge-footprint-6pzohf49/f2f08ee7-ea5e-4416-928d-83aae3c35939/`. Exact serial/result/ISO pins, metric deltas and qualification blockers are in `build/durable-charge-approved-vmware-verification.json`. The executed diagnostic harness is retained as build evidence, not installed as a permanent capture backend.

No finite production budget is selected from the observed samples or integer capacities. Actual profile selection remains blocked on attributable complete lifetimes, independent high-water/work/retention bounds and the real target service paths. Actual principal/suite/proof/backend/floor assignments still require the public, owner-authorized deployment packet in the proof acceptance contract; approval of their required semantics does not fill missing values.
