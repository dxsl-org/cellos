# VMware durable charge lab — authorized execution scope

## Authority and boundary

The user agreed to the proposed VMware lab: provision lab identities and a dedicated persistent disk, implement the durable end-to-end paths, run fault witnesses and measure actual lab costs. This extends the earlier diagnostic-only VMware scope. It does not activate native Cellos G3, release its locks, approve complete production CP1/CP2 packages or assert physical power-loss/independent anti-rollback protection.

Decision: an isolated Linux guest runtime under `scripts/durable_charge_lab/`, using real guest SQLite transactions on a dedicated ext4 virtual disk and real Ed25519-signed lab owner facts. No dictionary/in-memory store is accepted as durable execution evidence. Lab identity keys are generated inside the guest, private files mode0600 outside the repository; only public assignments/pins enter evidence. All roles share one guest/root trust domain, explicitly not independent protected authorities. Host tools control only a uniquely created lab VM.

The lab carrier is a separately named canonical JSON+Ed25519 evidence envelope and SQLite state/journal schema. It exercises the approved charge/ownership semantics; it is not a DCO1 codec implementation, native kernel publication/resource proof, production issuer or qualification of DCO1 r1/kinds48–58. Those deployment and byte-family boundaries remain explicit. No existing production parser/gate is weakened or bypassed.

## Concrete guest/backend selection

- Official Ubuntu24.04 minimal amd64 cloud image, exact source URL and SHA256 retained with evidence; Python3/sqlite3/cryptography and OpenSSL from its manifest.
- VMware Workstation25, BIOS, one vCPU,1024MiB; no network, shared folders or user disks.
- Dedicated boot VMDK plus uniquely created ext4 data VMDK labelled CELLOS_DCO_LAB; no formatting discovered disks. Cloud-init NoCloud ISO and boot-time systemd service read scenario/payload from the attached CD.
- SQLite rollback journal, synchronous FULL, foreign keys, BEGIN IMMEDIATE and expected revision; key-result/index/balance/journal publication is one transaction. Database opening never silently recreates missing initialized stores. Genesis is an explicit initialize-only command. Actual guest SQLite/version, mount and device information are reported.
- Ordinary virtual-volume crash/restart model only. No volume snapshot rollback resistance or host-power-loss durability claim. Guest hard-stop is a real VM reset but cannot prove lower physical cache persistence.

## Interfaces and ownership

`auth.py`: deterministic canonical encoding, guest Ed25519 provisioning, public role roster and purpose/scope-bound signed fact issue/verify; strict finite decode bounds. `runtime.py`: separate durable issuer, Authority and destination stores, keyed operations, charge accounting, custody/fence transitions and proof-dependent release. `scenarios.py`: actual end-to-end assertions, restart checks and resource observations. `vmware_lab.py`: unique Windows-visible workspace, pinned guest conversion, boot/data VMDK, NoCloud payload, serial-driven fault injection, exact provenance/results and verified own-VM cleanup. These are private lab interfaces, not exported Cellos ABI.

## Acceptance criteria

- LAB-01: Only a complete lab assignment/public-pin roster admits signed facts; changed scope/purpose/content, wrong role/pin, stale writer and changed same-key request reject before effect.
- LAB-02: Reserve commits before destination effect. An uncertain reply is resolved through the same durable key/result, never a second debit/publication.
- LAB-03: Whole-vector mixed resize preserves conservative charge, never nets shrink against growth; partial shrink credit and proven-no-application cancellation each credit once under original keys. Unknown remains charged.
- LAB-04: Bootstrap prepare/ready is nonspendable; FenceStarted excludes abort and closes old mutation at its serializer; assignment/acceptance/settlement preserve debit origin and transfer sole effective custody at acceptance. Settlement never credits. Same-ledger succession rejects stale writer/effect admission.
- LAB-05: Destination durable key closure plus complete lab resource outcome/quiescence is required for final fixed-home release; replay/lost reply returns the original result and cannot credit twice. Missing/corrupt/wrong-ledger stores or proof omissions close mutation, not create empty genesis.
- LAB-06: Host actually hard-stops/restarts the guest at selected before-commit, after-commit-before-reply and ownership/fence boundaries; recovery asserts exact retained state and original transaction identities. Marker observation alone is not a passing recovery witness.
- LAB-07: Evidence includes actual public roster, image/payload/schema/config/disk/serial hashes, guest backend versions/SQLite settings, observed state/charge/outcomes and file/work/time high-water within explicit lab-only finite caps. No secret keys are logged. No selected cap or sample becomes a production profile.
- LAB-08: Every created VM is stopped and its absence confirmed; failure is retained as failure. No native Cellos source, production admission or existing user VM/storage is modified.

## Verification and failure posture

Use contract-focused host regression checks for deterministic edge cases, then the actual VMware guest path and hard-stop/recovery matrix. Independent review checks authentication/credit/fence/crash boundaries. Before any lab effect, rejection changes code/config only; after durable effects, recover the same lab identities/stores forward or retire the entire disposable lab namespace. Never reset a damaged store to make a scenario pass. Evidence and encrypted/private key material are not interchangeable; only public evidence is copied back.
