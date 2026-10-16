# Private VMware durable charge lab

This is an executable Linux/SQLite/Ed25519 development lab, **not** a native Cellos G3 implementation, DCO1 codec, protected authority, independent anti-rollback floor or physical-power-loss qualification. It exercises charge/ownership semantics against actual guest disk transactions and a finite SQLite BLOB/slot resource owner. All roles share the guest/root trust domain. The namespace-wide process lock is a centralized lab gate, not proof of distributed kernel fencing.

## Inputs and provisioning

Requires WSL access to Windows VMware Workstation `vmrun.exe`, `wslpath`, Python3 with `cryptography`, `qemu-img`, `xorriso`, `mkfs.ext4` and `blkid`. The offline guest is the official Ubuntu24.04 minimal amd64 cloud image pinned in `vmware_lab.py`; obtain it from the recorded Canonical URL. Provision refuses any other image digest rather than following a changing latest image.

```sh
python3 scripts/durable_charge_lab/vmware_lab.py provision \
  --image build/durable-charge-vmware-lab/ubuntu-24.04-minimal-cloudimg-amd64.img
```

The printed unique directory under `/mnt/c/Temp/cellos-vmware/` owns its boot VMDK,1GiB preformatted ext4 data VMDK and profile. No discovered/user disk is formatted. VMware exposes the disks through IDE: the pinned image's built-in `ata_piix` boots this tuple; the initial lsilogic attempt lacked a usable root device and its failed evidence is preserved. Networking/shared folders are disabled. Runtime dependencies are already present in the guest image; no guest package install or network access is needed.

## Execution

```sh
python3 scripts/durable_charge_lab/scenarios.py --list-json
python3 scripts/durable_charge_lab/vmware_lab.py matrix --lab /mnt/c/Temp/cellos-vmware/<printed-lab>
```

Use a freshly provisioned namespace for a full matrix. Individual commands support `run --scenario NAME --phase run`, or a manifest-defined crash `--phase prepare --expect-cut LABEL` followed by `--phase recover` using the same lab disks. Do not rerun a completed normal scenario as a fresh initialization or erase databases to obtain a PASS. Failed evidence remains retained; a changed implementation uses a separately provisioned namespace.

The first boot runs NoCloud provisioning; later boots execute a stable launcher reading the current readonly seed payload. Actual launcher/service/bootstrap hashes, copied source hashes, ISO/VMX/profile/ownership and stopped-disk hashes bind evidence. Every run hard-stops only its owned VM and verifies its absence. A cut marker alone is not a recovery PASS: the next boot must compare exact committed state, retained request/result identities and resource content with its fsynced cut witness, then finish original operations forward.

## State and identity boundaries

`auth.py` exclusively provisions distinct Ed25519 role keys inside each scenario's data directory, fsyncs files/directories and publishes the complete public roster last. Private keys stay mode0600 beneath mode0700 identities; they are never returned in results. Missing/incomplete/mismatched keysets refuse without regeneration. The lab VMDK contains these private development identities and must not be published as a public evidence artifact; export only public JSON receipts, hashes and serial logs.

`runtime.py` uses separate explicitly initialized issuer, Authority and destination SQLite databases, `BEGIN IMMEDIATE`, rollback journal DELETE, synchronous FULL, checked expected revisions and atomic state/resource/key/result/journal publication. The destination resource is a persistent BLOB/slot row; it is not a native task, DMA grant or external effect. Fixed-home Authority alone credits. Issuer retains nonspendable bootstrap provenance after B4 even when settlement/reply is lost. Missing/corrupt/wrong-identity stores refuse instead of creating a fresh ledger.

`scenarios.py` contains source-defined assertions for mixed resize, credits/cancellation, custody/fencing, reconciliation/closure, capacity and corrupted evidence, and exposes actual hard-stop fault boundaries. Source-defined caps are experimental lab policy, not measured production capacity. Retained results are never evicted to make room; fresh work must preserve finish capacity.

## Evidence ceiling

SQLite FULL commits surviving guest hard-stop/restart are observations under the selected virtual-volume failure model. They do not prove host-power-loss persistence, dishonest-controller behavior or resistance to restoring the whole VMDK. No floor is claimed outside that rollback domain. Real native publication/resource/state/effect producers, exact DCO1 proof bytes, physical storage/IOMMU and G0–G2/CP1/CP2 remain separate gates.

Before effect, rejection changes code/config only. After durable effects, retain identities and original operation keys for forward recovery; alternatively retire the entire disposable lab namespace without treating its old credit/keys as transferable production authority. The controller never automatically deletes a failed namespace.
