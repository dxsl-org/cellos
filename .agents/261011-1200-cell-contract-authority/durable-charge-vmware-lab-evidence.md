# VMware durable charge lab — implementation and evidence

## Scope and actual implementation

The [authorized lab plan](durable-charge-vmware-lab-plan.md) and [extended user grant](durable-charge-vmware-lab-authorization.json) are implemented in `scripts/durable_charge_lab/`. This is a real Linux guest runtime with separate persistent SQLite issuer/Authority/destination stores and Ed25519-signed role/purpose/scope-bound commands/results. The destination owns finite persistent BLOB/slot resources. Its signed closure is evidence only for that admitted resource owner, not native task/DMA/state/external-effect quiescence.

- `auth.py`: strict bounded canonical JSON, complete public role roster, Ed25519, exclusive durable mode0600 key provisioning and fail-closed reopen.
- `runtime.py`: atomic expected-revision/state/resource/key/result/journal SQLite commits, whole-vector conservative accounting, fixed-home-only credits, non-abortable fencing, custody transfer, bootstrap reconciliation/activation, Authority succession, Closing and one-credit release. A namespace-wide interprocess lock serializes the actual lab gates across owner databases; it is not native distributed fencing.
- `scenarios.py`: source-defined normal/refusal/concurrency/corruption/capacity witnesses and before/after-commit fault points. Recovery compares original committed state, request/result identities and resource contents with a separately fsynced pre-cut witness, then finishes original keys.
- `vmware_lab.py`: pinned offline Ubuntu guest, uniquely owned boot/data VMDK, NoCloud payload and serial-driven real hard-stop/restart. Exact bootstrap/source/config/ISO/serial/stopped-disk hashes and own-VM cleanup bind results. Failed rows remain failure evidence.

The private lab carrier is canonical JSON+Ed25519 and the store schema is SQLite v1. This is **not an implementation of the DCO1 packed codec** or native Cellos G3. No existing production runner, VFS/kernel/resource gate, public ABI or protected authority path is changed. The old source census remains its historical denominator; new private lab sources have their own exact verification pins, not an inferred complete production consumer census.

## Guest/backend and failure model

Official Ubuntu24.04 minimal image SHA256 `2f119f50bf29fe40e73b69ae2a736e8210d8146796f036e0324315b6b3a777df`, from the Canonical release URL recorded in the profile. Observed guest: Linux6.8.0-146-generic, Python3.12.3, SQLite3.45.1, cryptography41.0.7, OpenSSL3.0.13. VMware Workstation25.0.0 build24995812; one vCPU/1024MiB, no network/shared folders. Dedicated1GiB ext4 data volume, SQLite DELETE/FULL with foreign keys and finite page limit. The final profile uses IDE/ata_piix, not the initial planned lsilogic/SCSI.

The first lsilogic boot failed to expose its root block device and panicked; evidence is retained. Only owned preboot disks were converted to IDE, and provisioning now selects IDE directly. The first successful guest smoke generated [actual public lab assignments](durable-charge-vmware-lab-public-assignment.json); its private keys remain only inside the lab data VMDK. This initial public roster is one scenario/namespace, not a production trust root or the identity of every later scenario.

An earlier matrix completed seven normal witnesses and ten full hard-stop/recovery pairs, then the host command deadline interrupted it during later work. Its incomplete run is not a matrix PASS. The remaining own VM was explicitly hard-stopped and absence confirmed. Repeated offline network-online waiting and unsynced first-boot service installation were corrected by persisting root-volume provisioning before scenario execution and masking only the irrelevant offline wait-online service. Final verification uses a fresh namespace; no initialized store was reset to make it pass.

This ordinary virtual-volume crash/restart evidence does not establish host-power-loss durability or independent anti-rollback. Restoring/copying the whole VMDK also restores identities/state; no external floor claims otherwise. All signing roles share one guest/root trust domain, without role process isolation or protected hardware custody.

## Independent review and corrections

Authentication review found no defect. Controller review identified and resolved stale bootstrap execution, missing stopped-disk/config/payload provenance, omitted failed matrix rows, manifest-generation source race and per-boot versus matrix source-pin mismatch. Runtime review identified and resolved cross-store admission/fence races, stale/wrong-kind destination authorization, absent Closing, premature B4 activation, issuer recovery bypass, reused adoption IDs, unverified replay and unexercised finish-capacity boundary. Follow-up audit-gate, aggregate-adoption-cap and commit-count findings were also fixed. Focused independent re-reviews reported no remaining original/follow-up findings; this is technical review, not native/production checkpoint admission.

## Host verification

Final host receipt: `build/durable-charge-vmware-lab/host-final-verification.json`. It pins the current sources and records actual commands/results. Authentication has13passing tests; byte compilation passes. All eight manifest-defined normal scenarios pass against fresh host on-disk SQLite, including reserved-tail, inherited Unknown, serializer race, replay corruption and aggregate adoption-capacity boundaries. Host runs are logical regression evidence, not guest persistence qualification.

## Remaining admission boundary

The lab supplies real development identities, a concrete ordinary backend, executable producer/verifier behavior and measurable durable state-machine paths. It does not supply native Cellos issuer/genesis/publication/resource/floor assignments or exact DCO1 external proof bytes. Physical storage/IOMMU/device qualification, independent protected floors, deployment workload/profile/caller closure, G0–G2 prerequisites and complete exact-digest CP1/CP2 remain separate. Experimental caps and observed lab samples are not production budgets. Public receipts may be retained; VMDKs/private development keys must not be published as public evidence.


## Final actual VMware matrix

Fresh final namespace: `/mnt/c/Temp/cellos-vmware/durable-charge-lab-lzdy5d2z`. The complete source-defined matrix finished with **eight normal scenarios and thirteen real host hard-stop/recovery pairs across34guest boots**. Each prepare observed its exact cut then hard-stopped the VM; each recovery reopened the same persistent stores, verified exact retained original state/keys/resource content, then completed forward. All34runs confirmed own-VM cleanup; final `vmrun list` reports0running VMs.

Final matrix: `/mnt/c/Temp/cellos-vmware/durable-charge-lab-lzdy5d2z/matrix-3f0c8d7b7361431ebd33306e9287d96b.json`. Public copies are `build/durable-charge-vmware-lab-matrix.json` and `build/durable-charge-vmware-lab-verification.json`. The latter independently verifies current source pins, every retained serial digest, final stopped disk/config hashes, scenario counts and recovered original-key assertions. [Final actual assignments](durable-charge-vmware-lab-final-assignment.json) retain real per-scenario public rosters and backend/volume identity; earlier smoke assignments remain historical and are not substituted for final runs.

The sequential guest witness performs25commits in456911140ns with31864KiB observed process maximum RSS. Final database sizes are issuer65536bytes, Authority86016bytes and destination65536bytes; destination observed resource high-water is140BLOB bytes/10lab slots. These are one finite lab workload's observed values, not latency distributions, allocation attribution, upper bounds or production profile recommendations. Its Authority charge ends at(0,0), issuer nonspendable provenance remains(100,10), and the destination retains the closed-key row with zero resource allocation. Reserved-tail and adoption-capacity scenarios additionally exercise their actual source-defined cutoff/finish and aggregate-history boundaries.