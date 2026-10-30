# KSSH K8 CP1 DRAFT: QEMU evidence for CBX-24's publickey half at the DEV_POSTURE ceiling (status AWAITING CP1; documents only; nothing built, run, generated or edited)

Written 2026-10-30 at HEAD `fad076ee9` (`git status --short` empty when read). Everything cited as `file:line` was read in this session; `[INFERENCE]` marks anything not observed in a file or command output; `OPEN (decider)` marks a decision for the named owner. K8 depends on slices that do not exist yet (K3, K5, K6, K7, and K4b for T1); their designs are only as far as `k2-cp1.md`, `lock-adoption-draft.md`, `contract-delta-import-surface.md` and the K1 evidence say, so every K3/K6/K7-specific name below is `[INFERENCE]` until those CP1s land and K8 must be re-read against them before its own CP2.

## 1. Scope

**In:** one private QEMU/TCG evidence program that shows, on an image the Cellbox plan already knows how to build privately, that `cellbox ssh --auth publickey --identity KEY_ID` logs into a controlled OpenSSH server with an ed25519 key held by KMS, and that the refusals, secrecy and persistence properties claimed for T0/T1 hold or are honestly shown not to hold. Tier order: **T0 first (K4a, volatile)**, **T1 only if K4b exists (persistent, `DEV_POSTURE`)**.

**Out (section 16):** canonical promotion, any change to `scripts/assemble-x86-image.py`, `scripts/cellos_sign/**`, `kernel/src/embedded-x86_64/**` or the S10 protected census, Tier-3 guest endpoints (CBX-23/25), physical hardware, T2/K9, crash-durability, any `ProductionQualified` statement.

**Result K8 can produce:** a private evidence dossier plus an explicit statement of what it proves; "CBX-24 publickey half met at DEV_POSTURE ceiling" is a user disposition after CP2, never an automatic outcome (`plan.md` tiers note).

## 2. Entry gates (all must be recorded true before any VM run)

| # | Gate | Source of truth |
|---|---|---|
| G-1 | K3 done: `CALLER_FLAG_KMS_SSH_SIGN` kernel-attested for `/bin/cellbox-ssh` only, the `cellbox -> /bin/cellbox-ssh` launch edge carries it, `sign-policy.py` row (v8) exists, K3's own host + lane evidence passed | K3 CP2 dossier (pending) |
| G-2 | K5 done: KMS imports, parses (D-K4 own parser), unlocks per login (D-K5), signs, self-verifies, limiter, zeroization host tests; **KDF cost measured on the QEMU target** at rounds 16 and 64 (required by `k2-cp1.md` §3.5); the K1 lock diff (`bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`) applied and `cargo build --locked --offline -p service-kms --target x86_64-unknown-none -Z build-std=core,alloc` works with the user's default `CARGO_HOME` (the K1 fetch went to scratch `/tmp/kssh-k1`, `k1-cp1-draft.md` network disclosure) | K5 CP2 dossier (pending) |
| G-3 | K6 done: `KmsClient` SSH ops, `AgentSign`, `--identity`, one no-echo passphrase lease, `publickey_unavailable` lifted, documented order of (key probe, unlock, sign) and the stderr/exit-code vocabulary the judge must match | K6 CP2 dossier (pending) |
| G-4 | K7 done: the admin import path chosen at K7 CP1 (O-2) exists and is invocable from the lane driver, including delete and status | K7 CP2 dossier (pending) |
| G-5 | T1 runs only: K4b done (record v2, x86 dev provider, dev root, production exclusion); D-K2 label stays "synced, not crash-proof" | K4b CP2 dossier (pending) |
| G-6 | `scripts/cellbox-ceiling-diff.py` (L5 runs it, `qemu-phase09-ssh-test.sh:92`) passes on the K3/K6 tree, i.e. the approved ceiling delta covers the new right | run at K8 preflight |
| G-7 | Lock handoffs of K3/K5/K6/K7/K4b are approved and recorded; K8's own handoff (section 4) approved | controlling plan |
| G-8 | Working tree clean at run time except K8's own new/edited files; HEAD and a dirty digest recorded (assembler `provenance()` form, `assemble-x86-image.py:353`) | preflight |
| G-9 | Docker image `cellbox-p09-sshd:<sha256 of RECIPE>` (OpenSSH_9.9p2, `phase09_ssh_fixture.sh:36-44`; version string retained in `s10-3b-evidence/rerun/lanes/default-p09-nofaults/fixture.txt`) exists in the local Docker store. K8 builds **no** image: if absent the runner stops with status 3 (not a run); rebuilding needs the network and its own authorization. **Not checked:** whether the image is present now | preflight |
| G-10 | QEMU 10.2.0 (`~/.cache/cellos/qemu-10.2.0/bin`, present) first in `PATH`; version and binary SHA-256 recorded; TCG is the qualifying accelerator (`qemu-phase10-canonical-test.sh` README: KVM runs are non-qualifying) | preflight |
| G-11 | Free disk and the S10 census baseline recorded before the run (section 12) | preflight |

## 3. File closure with current SHA-256 (HEAD `fad076ee9`)

### 3.1 Existing files K8 reads and **must leave byte-identical** (frozen for the run)

| Path | SHA-256 | Why it is frozen |
|---|---|---|
| `scripts/assemble-x86-image.py` | `b74cc99787ab69dde04dac5c517acb6f771016043c23841bc7b40ab0b3bec7ed` | pinned recipe of the four signing routes (`signing.py:70`); also refuses any path outside its manifest (3.1 note) |
| `scripts/cellos_sign/signing.py` | `ba97129308e60c50d59ff669831ab47d344b3ccbf44e8f17fbf052c9666c9511` | G2a/G2b/G2d (`:116-131`, `:148-157`, `:220-251`) |
| `scripts/test_assemble_x86_image.py` / `scripts/test_cellos_sign.py` | `d78b4eeb2a2c5fc065662c0444352fcff4d1c094e53757778ac4f3634a09c4c5` / `a08c603b969d9a183f097605c902443c8f6fd2f04fbd663b71cb6ef3e5672183` | guard tests |
| `scripts/x86/make-iso-ci.sh` / `scripts/x86/check_default_kernel.py` | `8f5bef774ad048cd456b3e3f0c3a2a0181ac8c81405d4a202de2554b9988593b` / `150f40a8e52ce05859e8eeb6dfda2e405bd93b0d0338898ebdf287a4524f6f6a` | G3 (conditional on the default kernel path, `make-iso-ci.sh:22-34`) |
| `scripts/qemu-phase09-ssh-test.sh` | `30c031a1f08d47fc5118800baeddb047bf197f4a7a6e39c0c6cd99fb624889b2` | L5/L5f runner: **forked, not edited** |
| `scripts/phase09_ssh_fixture.sh` | `5a9afb76fc45a450471ec4e34b7bec4a38dfb3d0ef1f3a09d8d7b139ca4c5199` | referenced by `qemu-phase10-canonical-test.sh` and `test_phase10_canonical_drive.py:23`: forked, not edited |
| `scripts/phase09_trust_disk.py` | `10901b916c948efd62b632e2144e1a3ab519ce8487d324f87e42c60390e6e5e1` | reused as is (P1 FAT, P5 CellosFS `/srv` with the host-trust store) |
| `scripts/phase10_candidate_import.py` | `9e4a8e7cfcaebf44f791364cf608e445816b8f4f14c7f03d75f7b3bcec8b0f9c` | optional payload-match step (D8-4) |
| `scripts/qemu-phase10-canonical-test.sh` | `fd3c295fda81c7c5879e435a043c9556324aab6a8f321338e2da55b2801ab801` | work-dir alias guards, evidence layout (model) |
| `scripts/phase04_gui_drive.py` / `scripts/phase10_canonical_drive.py` | `bfef6d8e6426bbddcb2d5e59d9f00dced631bef995af7c62d3366486d32027bc` / `d4fcb022dc340f6d79451fa02ae3b38e0ca90f809f0c943ba48abfbeb835fad9` | pixel oracle (optional GUI run RG) |
| `scripts/sign-policy.py` | `70a6fb27bdff390a4a42e3eaeebe11fd48aaa619488950d512da97c6ed308598` (pre-K3; K3 edits it) | imported as a module to build the lane policy; K8 does not edit |
| `scripts/generate-boot-config.py` / `tools/mkfat32.py` / `tools/mkfat32_inplace.py` | `8ffc3177151f0fc8e3366d97698445d782b1db40e4c2bc91e0c4bf485e6a9d0e` / `b82c284a714ed2f546783085da035328d351b37e712624924b27eb5826500565` / `7e133650a7b767d8c597a856c1c60af8e0f590185b78a38e2c7ed7c7d3468982` | image and config writers |
| `config/services.toml` | `188d4d1966a91ad0bb4fb0336548a27969f86f349bd2b05041704ed8d5deecdb` | source of the `kms` row (`:88-92`) copied into the private config |
| `kernel/build.rs`, `kernel/Cargo.toml`, `kernel/src/main.rs` | `c46adb3053a91e3513f525017f4cb69601a87c882f2e8195951c09a0518cc22b`, `b71f1ba41c72b36b6295da3c10aef73479ccfc5ec9b2a53b40079cfa80669f27`, `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` | `phase02_profile` refuses the canonical embedded dir (`build.rs:128-148`); `main.rs` is in the census |
| `kernel/src/embedded-x86_64/{kernel_fs.img,init,manifest.json}` | `3848cefb25f0e7c4abc67feccf3d8886292e25da60a88b4633552651292479ec`, `c0bd14580fda0d697a2edd66b19168107fa6877a190bc06c1a8a482b467b23f3`, `af55e60245832b348e1e20b7689d35d84fe6747b7b359e886ebe3a4a81ae4e8d` | promoted canonical bytes: **never touched** |
| `libs/api/src/abi/caller_identity.rs` | `924a0325566282747d8b8afd3023ef5a7e24ec7f3b57fe6cb3aefc9033ddac8f` (pre-K3) | trailer is overwritten by the kernel after the payload copy (`:15-17`; `kernel/src/task/syscall.rs:3351-3358`): basis of the forged-trailer scenario |
| `cells/services/kms/src/{main,dispatch,auth}.rs`, `libs/ostd/src/clients/kms.rs`, `cells/services/kms/Cargo.toml` | `3cce19d7…10`, `2c19b290…06b`, `174690fb…ebe9`, `b9fdd3af…1286`, `3888e6bf…d167` (pre-K5/K6; K5/K6 edit them, K8 does not) | `KmsClient::call` blocks in `sys_recv(self.tid)` without timeout (`kms.rs:162`, O-9): a dropped reply hangs the caller; K8's deadlines catch it |
| `Cargo.lock` | `02d35d446b90fe569fab50d721a634258d6bb733ea789c45ddc8534ee8ca555a` (pre-K5) | K8 edits **no** lock and adds **no** dependency (the driver uses only existing `ostd`/`api`) |

**Assembler finding (why a KMS image cannot use the pinned route).** `check_inputs` requires `set(destinations) == profile.image_paths()` (`assemble-x86-image.py:196-200`), `FORBIDDEN_PATHS`/the manifest list contain no `/bin/kms` (`:75`, `HEADLESS_CELLS` `:108-132`), `--test-inject` is refusal-only (`:541-558`), and the dev-key routes list only the existing binaries (`signing.py:60-94`, no `service-kms`). A `--seed-file` run also hits `check_inputs`. So no flag of the pinned recipe produces an image with `/bin/kms`; making it do so is a recipe edit that changes the pin, the route table, the guard tests and needs its own S10-5-style promotion CP, which D10-3 = A forbids as a lift (`tier3-and-guards.md` guard table). K8 therefore uses the lane/fixture route only (section 5).

### 3.2 Files K8 would create (new; none exists yet; hashes are of nothing)

| Path | Purpose |
|---|---|
| `scripts/qemu-kssh-test.sh` | runner forked from `qemu-phase09-ssh-test.sh` (work dir `/tmp/cellbox-kssh-*`, per-set argument `--set` taking one of rt0, rt0f, rt1, rg) |
| `scripts/kssh_fixture.sh` | OpenSSH fixture forked from `phase09_ssh_fixture.sh`: same pinned `RECIPE`/image tag, adds `authorized_keys` and the third listener (5.7) |
| `scripts/kssh_keys.py` | run-time TEST key generation into `$W/keys` via the host `ssh-keygen`, canary derivation, key blob to FAT P1 via `mcopy` |
| `scripts/kssh_judge.py` + `scripts/test_kssh_judge.py` | evaluators (serial markers, sshd log, scanner with controls, preservation); host tests with fixtures and planted canaries |
| `cells/tests/phase04-terminal-driver/src/kssh.rs` | driver scenarios (feature `kssh`) |
| `.agents/261030-1000-kms-ssh-key-custody/reports/k8-evidence/**` | text-only evidence (sections 8, 12) |
| `.agents/261030-1000-kms-ssh-key-custody/k8-cp1-approval.md`, `k8-cp2-dossier.md` | approval record and dossier at the corresponding steps |

### 3.3 Existing files K8 would edit (pre-edit SHA-256)

| Path | Edit | Pre-edit SHA-256 |
|---|---|---|
| `cells/tests/phase04-terminal-driver/Cargo.toml` | add feature `kssh = []` beside `phase09-ssh` (a feature adds no package, so `Cargo.lock` is unaffected) | `5744e1f50cc0b50f00dda15a46ad63dd3afe0f197c0e337f3c170a11ad8c5a9b` |
| `cells/tests/phase04-terminal-driver/src/main.rs` | `#[cfg(feature = "kssh")] mod kssh;` and the dispatch beside `phase09-ssh` (`main.rs:19,40-41,343-348`) | `095c8e3e5670ca26571e756edd7a05a7714c33aba4cf5237f372849d8aff7329` |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | none (read-only reuse of `Term`, `login`, `wait_network` helpers if made `pub(crate)`; if that needs a visibility edit it is listed in the CP2 delta) | `f189362150a431ef03d13edcbf2bdeef2def8332513a36befd636b6f704024df` |
| lane-only fault build (D8-7), if approved: `cells/tools/cellbox/**`/`libs/vissh/**` feature `kssh-faults` | owned by K6/Cellbox single integrating owner; list fixed at K6 CP1 | post-K6 |

No path under `kernel/**`, `cells/services/**`, `libs/types/**`, `libs/api/**`, `scripts/cellos_sign/**`, `scripts/assemble-x86-image.py`, `build/`, `target/`, `kernel/src/embedded-*` is edited by K8.

## 4. Locked relations and the lock handoff packet (form of `lock-handoff-k2-cp2.md`)

K8 is evidence work. It edits no CELL-CONTRACT-0024 locked surface; it **builds and runs** locked trees (read-only use) and it relies on the handoffs of K3 (kernel permission, row `260603-1803-phase-h-vfs-hardening` `locks.md:118`, controlling P05), K5/K4b (row `260825-1726-kms-silo-production-root` `:145`), K2 (Law 1, done). The packet below is therefore short and exists to make that explicit and to record the Cellbox-owned test-file closure.

1. **Slice identity.** KSSH K8, QEMU evidence for CBX-24 publickey at DEV_POSTURE; serves Cellbox Phase 09 S9-5 and CBX-24, **not** an Intel C2C outcome (same statement as `lock-handoff-k2-cp2.md` §1).
2. **Contract delta vs ADR-0024 principles.** None: no capability, handle, identity concept, delegation, revocation or substrate change. The lane uses the K3 attested flag and K7's admin class exactly as shipped.
3. **File closure.** Section 3.2 (new) and 3.3 (edit). The driver/lane scripts are Cellbox-plan artifacts (Phase 04/09 slices); no locks row names `cells/tests/**` or `scripts/qemu-*` `[INFERENCE: grep of `cell-contract-0024-locks.md` for these paths found nothing]`.
4. **Locked rows / overlapping plans.** No new row. Re-check at K8 start: `261025-cellosfs-crash-safety` (planned, no lock row, D-K2), `260826-1605-phase4-dev-reference-authority` (reciprocal notes already recorded by the K2 handoff C-1..C-3), Cellbox plan single integrating owner for the driver/fault-build edits.
5. **Retained gates.** Law 1: K8 changes no `libs/types` byte; every ABI exercise uses K2's frozen frames. Protected identity/non-rollback time/state: not applicable (T0/T1 `DEV_POSTURE`, never `ProductionQualified`); K8 shows the rollback gap, it does not close it. Physical/production/remote gates untouched.
6. **Scheduling change (text only).** Record "KSSH K8" as a bounded evidence slice with ceiling "private fixture image, QEMU/TCG, loopback Docker OpenSSH_9.9p2, DEV_POSTURE"; nothing is narrowed or released.
7. **Execution boundary.** Single integrating owner for the new files; no parallel edit of the driver crate; builds only into `$W` (`/tmp/cellbox-kssh-*`); no commit of images, disks, ISOs, keys; no push without a separate request.
8. **Decisions requested.** Section 18 (D8-1..D8-11).

## 5. Design A: the private fixture image route

### 5.1 How the existing private images were assembled and signed (observed)

| Route | Used by | Mechanics (file:line) | Fit for K8 |
|---|---|---|---|
| **Candidate** | S10-3 candidate, S10-5 promotion | `assemble-x86-image.py --profile headless` or `desktop` with `--out-dir /tmp/...` signs with the dev key through route `x86-candidate-<profile>`; the kernel is built outside with `EMBEDDED_OVERRIDE` plus `kernel_features` from the manifest, ISO by `make-iso-ci.sh` (`s10-3c-evidence/build-candidate.sh:1-39`, assembler docstring `:1-35`) | **No**: exact path set (3.1 finding) |
| **Lane / fixture (L5, L5f, phase02-04)** | `qemu-phase09-ssh-test.sh` | throw-away seed from `os.urandom(32)` (`:94-110`); cells built into a private `CARGO_TARGET_DIR` (`:116-137`); signing `CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 scripts/cellos-sign --seed-hex SEED --sign ...` (`:171`; allowed by `signing.py:116-131,148-157`); image by `tools/mkfat32.py` with explicit (host file, image path) pairs plus a private `--config-dir` (`:175-231`); kernel built with `phase02-terminal-profile,phase04-driver-profile`, `CELLOS_PHASE02_CELL_PUBLIC_KEY=$KEYS/cell.pub`, `EMBEDDED_OVERRIDE=$W/embedded` (`:71,238-243`); ISO via `X86_KERNEL=... make-iso-ci.sh` (`:245`; G3 is skipped for an explicit `X86_KERNEL`, `make-iso-ci.sh:29`); QEMU one `-netdev user,id=net0` without `restrict`, no forwards, `-no-reboot`, NVMe fixture disk (`:261-279`) | **Yes** |
| **Import mode (S10-3.3)** | L5 `--candidate` | shared cells copied from the candidate, re-signed with the lane key, signed-payload hash compared (`phase10_candidate_import.py`, `qemu-phase09-ssh-test.sh:20-27`) | optional (D8-4); the retained candidate directory is among the 76 census paths that no longer exist (`census-before.json`, `missing`), so any import source would be the promoted canonical bytes read from a copy `[INFERENCE: not checked that this satisfies `candidate.json` schema]` |

Guards stay exactly as they are: G2a (name guard with the private-fixture allowance, route table untouched), G2b (quarantined hashes, unconditional, re-checked after key load: `signing.py:220-229`, `:275`), G2d (`guard_prod_key`, `:116-131`: non-dev key outside CI needs the override variable, as L5 does), G3 (not applicable to an explicit `X86_KERNEL`, and `check_default_kernel.py` is never pointed at a K8 kernel), G4 (`kernel/build.rs:128-148` refuses the canonical embedded dir for a phase02 profile), G5 (alias guards: the runner repeats `qemu-phase10-canonical-test.sh:84-87` checks). `service-kms` and the admin cell are not in `PRIVATE_FIXTURE_NAMES` (`signing.py:37`), so only `cellbox` needs the fixture allowance, as in L5. K8 changes no guard and adds no route.

### 5.2 Image delta against the L5 lane image

L5 image paths (`qemu-phase09-ssh-test.sh:138-148` `CELL_BINARIES`/`CELL_IMAGE_PATHS`, `:175-181` `FAT_ARGS`): `/bin/cellbox`, `/bin/cellbox-ssh` (same file, D79-9), `/bin/vfs`, `/bin/config`, `/bin/input`, `/bin/platform`, `/bin/nvme`, `/bin/e1000`, `/bin/net`, `/bin/echo`, `/bin/phase04-terminal-driver`, `/p09/ssh.conf`, plus `init` and the generated `/etc/cellos/*.toml`. `/bin/shutdown` is **not** in this list (it is one of `WORKERS`, `assemble-x86-image.py:104`, built by `app-sys-tools`, which the lane already builds).

| Added by K8 | Why | Notes |
|---|---|---|
| `/bin/kms` (package `service-kms`, built from the K5 tree, T0 or T1 variant) | there is no KMS in the promoted image (`lock-adoption-draft.md` K0 resolution 3); service id 13 | KMS needs VFS for `boot_probe_store` and, at T1, for `/srv/cellos/kms/` (P5 CellosFS of the fixture disk) |
| admin import cell per K7 CP1 (O-2): (a) `supervisor` (`/bin/supervisor`, service id 11, template feature `supervisor`: `generate-boot-config.py:57-58`) with a supervisor-authorised import op; or (b) a dedicated `/bin/kssh-admin` `[INFERENCE name]` with its own launch row and `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7` (`k2-cp1.md` §2.1); or (c) offline provisioning (T1 only, not usable at T0) | the import is not a CLI command (D-K3) | exactly one of (a)/(b) is built; K8 builds what K7 ships |
| `/kssh/kssh.conf` (lane config, like `/p09/ssh.conf`) | `PORT_MAIN PORT_REKEY PORT_UNAUTH FP KEY_ID PASS WRONG_PASS PASS_L KEY_ID_L` for the driver | holds TEST secrets; lives only in the private ISO (never archived, section 8.5) |
| the three TEST blobs on the fixture disk's P1 FAT (`/mnt/sd/kssh/*`) placed by `mcopy -i disk@@1048576` (P1 starts at LBA 2048 x 512 = 1 MiB, `phase09_trust_disk.py:30`) | the admin path reads the blob from a data volume; the passphrase never goes through the admin path (D-K5) | VFS read access for the admin cell is a K7 detail |

**Not added: `/bin/net-broker`.** It is selected only with feature `c2c-broker` (`generate-boot-config.py:59-60`) and KMS admits it only for the relay lifecycle (`auth.rs:84`); the SSH classes are the K3 flag and K7's admin class. KMS's per-message `sys_lookup_service(NET_BROKER)` returns "none" when it is absent (`main.rs:30-37`) `[INFERENCE: the snapshot type accepts that; K5 must keep it so, otherwise K8 adds net-broker and records it]`. Net-broker is therefore a K5 correctness condition, not an image requirement.

**Services config.** The lane builds its `services.toml` from the template (`--config-features input`) and adds a `kms` row copied verbatim from `config/services.toml:88-92` (`service_id = 13`, `restart = "always"`), because the template selects `/bin/kms` only for the `ui` feature (`generate-boot-config.py:49-50`) and the headless lane has none. The same Python step that already rewrites `autoload.toml` (`qemu-phase09-ssh-test.sh:183-231`) is reused: the driver row gets `after = ["vfs","input","net","kms"]` (+ the admin cell), the shell is not autoloaded, nothing starts `/bin/sshd`. `/bin/shutdown` is added to the `FAT_ARGS` pairs (5.2 list).

**Kernel (fixture, not promoted).** Built from the K3 tree: features `phase02-terminal-profile,phase04-driver-profile` plus (D8-2) `policy-required`, `CELLOS_PHASE02_CELL_PUBLIC_KEY` = the run's throw-away public key, `EMBEDDED_OVERRIDE=$W/embedded`, private `CARGO_TARGET_DIR=$W/kernel-target`. The existing lane rows are reused unchanged: init -> driver, driver -> `/bin/cellbox` (`SHELL_EDGE`) and the product edge `cellbox -> /bin/cellbox-ssh` (`profiles.rs:63-79`, K3 adds the right there). `/bin/kms` is already a legacy-ceiling boot row (`boot_ceiling.rs:195`, `profiles.rs:27`). **K8 asks for no new kernel row.** The foreign callers are the driver itself (no flag) and K7's admin path. If K3 or K7 turn out to need a lane-only row, it belongs to their handoff and K8 waits (OPEN, decider K3/K7).

**Policy (OPEN D8-2).** The L5 lane has no `/POLICY.BIN` and the kernel is not `policy-required`; whether a policy-less lane applies the K3 right through the launch ceiling alone is `[INFERENCE: not read: kernel/src/policy.rs]`. The production fleet kernel is `policy-required` (`assemble-x86-image.py:64-69`). Recommended: the lane policy is generated by importing `scripts/sign-policy.py` read-only, taking its production rows (including K3's v8 `/bin/cellbox-ssh` row and K7's rows) and appending lane-only rows for `/bin/phase04-terminal-driver` and `/bin/kms` (which has no row today: the row list `sign-policy.py:115-200` lacks it; how an unlisted path is treated is `[INFERENCE: not read]`), signed with the dev policy key (`dev-policy-key`, `kernel/Cargo.toml:70`), written to `/POLICY.BIN` of the private image only. Preflight control: build the lane with `policy-required` and the L5 `--no-faults` driver suite first (smoke B0); if that fails for reasons unrelated to KSSH, fall back to the policy-less lane and record "policy path not covered" as a limit.

### 5.3 Build procedure (private, ordered, stops on first failure)

All under `W=/tmp/cellbox-kssh-XXXX` (mode 700, owned by the uid, not aliasing `kernel/src`, `target`, `build`; same checks as `qemu-phase10-canonical-test.sh:84-105`):

1. Preflight (section 13.2). 2. `scripts/cellbox-ceiling-diff.py`. 3. Generate the throw-away cell seed + public key (as `qemu-phase09-ssh-test.sh:94-110`). 4. `CARGO_TARGET_DIR=$W/cells-target cargo build --release --locked --offline --target x86_64-unknown-none -Z build-std=core,alloc` for `service-vfs service-config service-input service-platform driver-nvme driver-e1000 service-net app-sys-tools service-kms` (+ K7 admin cell), `app-cellbox` (product features; fault image: `--features kssh-faults`, D8-7), `app-phase04-terminal-driver --features kssh`, `app-init --features input`, with `RUSTFLAGS="-C relocation-model=pic"` and the `CC_/CFLAGS_/BINDGEN_` toolchain variables exactly as the L5 script exports them. 5. Sign copies with the private seed (the L5 call), then write the policy blob (D8-2). 6. `tools/mkfat32.py` with explicit path pairs and the private config; `tools/inspect_fat.py` read-back checks that every expected path exists and that **`/bin/kms` and no quarantined hash** are as expected (G2b hashes imported from `signing.py`, never copied). 7. Kernel build (features above), ISO by `make-iso-ci.sh` with `X86_KERNEL`/`X86_ISO_ROOT` in `$W`. 8. Fixture disk: `phase09_trust_disk.py` (P1 boot config copy, P5 `/srv` with the host-trust store for the three listeners) then the `mcopy` of the TEST blobs. 9. `provenance.sha256` of every cell, kernel, ISO, `kernel_fs.img`, the disk; `HEAD`, dirty digest, toolchain, QEMU.

### 5.4 Image variants (one ISO each; none is a promotion candidate)

| Variant | Content | Used by |
|---|---|---|
| I-T0 | K5 KMS (K4a volatile), product cellbox | RT0, RG |
| I-F | I-T0 with the lane-only `kssh-faults` cellbox (D8-7) | RT0f (labelled lane-only bytes, like L5f: never matched-image evidence) |
| I-T1 | K4b KMS (persistent dev provider), product cellbox | RT1 |

### 5.5 What a payload match can and cannot say

The kernel (K3 delta), `service-kms`, the admin cell, `cellbox` (K6 delta) and `init` (if changed) differ from the promoted bytes by construction. Unchanged shared cells (`vfs`, `config`, `input`, `platform`, `nvme`, `e1000`, `net`, `echo`) can optionally be compared by signed-payload hash with the promoted image's copies (D8-4; same `_signed_payload` rule as `phase10_candidate_import.py`) `[INFERENCE: equality needs deterministic rebuilds from unchanged sources; K5's lock change might alter dependency resolution only for packages that depend on the new crates]`. Either result is recorded; neither upgrades the claim beyond "private fixture build".

### 5.6 Key provisioning (run time only)

`scripts/kssh_keys.py` runs on the host with `umask 077` and writes only under `$W/keys` (mode 700, `/tmp`, never under the repository; the runner refuses if `$W` resolves inside the repo): ed25519 key A (`ssh-keygen -t ed25519 -a 16 -N "$PASS_A" -C kssh-k8-A`), key L for the limiter (`-a 4`, own passphrase), key B (authorized on the third listener only; its private half is generated and then deleted immediately, only the public line is kept), malformed variants for import refusals (`ssh-keygen -Z aes128-ctr` and `-Z chacha20-poly1305@openssh.com` copies of an ed25519 key `[INFERENCE: ssh-keygen -Z exists in the host OpenSSH 9.6p1; the nonce/format of its output is not read]`), plus a plain (`-N ''`) key P for the headless-pipeline scenario S15. Passphrases come from `/dev/urandom` (printable, 24 chars) with a distinct WRONG value. Nothing here is committed: the archive step (8.5) copies only an allowlist and scans the destination. `ssh-keygen -p -N '' -f copy` on a copy yields the unencrypted container from which the host derives the canary seed (8.2); no `bcrypt` module is installed on the host (`import bcrypt` fails), so no custom KDF code is written.

### 5.7 OpenSSH fixture

`scripts/kssh_fixture.sh` reuses the pinned recipe and image tag of `phase09_ssh_fixture.sh:36-44` (OpenSSH_9.9p2, OpenSSL 3.3.7 recorded in S10-3b). Differences, all in config/mounts, none in the image: (i) `authorized_keys` for user `tester` installed by the container entrypoint (`install -m 600 -o tester`, because a read-only bind mount keeps the host uid and `StrictModes` would refuse it `[INFERENCE]`); (ii) three listeners, all offering only the Cellbox profile (curve25519-sha256, ssh-ed25519, chacha20-poly1305@openssh.com): port 22 `PubkeyAuthentication yes` and **`PasswordAuthentication yes`** with `authorized_keys` = A and L (so a fallback to password after a publickey failure would be visible and possible: "no switching methods" is only meaningful if the alternative exists); port 2222 as L5 (`RekeyLimit 64K`) with A; port 2223 `authorized_keys` = B only; (iii) `LogLevel DEBUG1` instead of `VERBOSE` if the host control run shows the needed lines only there; (iv) published on `127.0.0.1` random ports only, reached from the guest as `10.0.2.2` (S9 rule D79-1); the fixture log is saved and the private key/password files are removed on `stop`, exactly as the original. The accepted/refused sshd line formats (`Accepted publickey for tester from ... ED25519 SHA256:...`, `Postponed publickey`, `Failed publickey`) are `[INFERENCE from OpenSSH behaviour]`; **host control run C-host** (host `ssh`, a key that is authorized, one that is not, no QEMU) fixes the exact strings used by the judge before any guest run, and a mismatch is a harness fix, not a product finding.

## 6. Design B: guest driver and observation protocol

The driver is the existing `phase04-terminal-driver` with a new feature `kssh` (precedent `phase09-ssh`, `ssh.rs:1-20`): it drives the real `cellbox sh` through `FrontendHost`, types commands and the passphrase with `type_keys`/`press` after the stderr prompt appears (`ssh.rs` `login`, `wait_stderr`), judges `$?`, exact output bytes and stable stderr fragments, and prints `P04 PASS|FAIL kssh-<id>` and `P04 INFO kssh-obs <id> key=value` lines. It also acts as the **foreign caller** (no SSH flag) by sending raw KMS frames with `ostd::syscall` (header layout and payloads from `k2-cp1.md` §1.3 and the K2 golden vectors; no new crate dependency, so `Cargo.lock` is unaffected).

The host judge (section 11) correlates four independent sources: the serial markers, the saved `docker logs` of the fixture, host-computed values (fingerprints, canaries), and the run metadata. The guest never decides a verdict alone: sshd acceptance, fingerprint equality and absence of canaries are host facts.

Guest-side secrecy: the driver scans every transcript it captured (`Term.out`, `Term.err`) for the passphrase and WRONG value and prints only a pass/fail marker (never the value), as `ssh.rs` already does for the password.

## 7. Scenarios

Run sets: **RT0** (I-T0, boots: A, then B for volatility), **RT0f** (I-F), **RT1** (I-T1, up to five boots), **RG** (optional desktop GUI). "Class" is the evidence that decides the scenario. `FP` = fingerprint of key A computed on the host with `ssh-keygen -lf`.

| ID | Set | Scenario | Pass criteria (all required) | CBX-24 element |
|---|---|---|---|---|
| S00 | all | **Lane parity control**: L5 `--no-faults` core (shell prompt, trust store, first-trust, `ssh --auth password` login, wrong password once) on the K8 image | the L5 markers pass with the same judge rules (`qemu-phase09-ssh-test.sh:385-440`); proves the KMS/policy additions did not break the lane | baseline |
| S01 | RT0 RT1 | **Import** of blob A through the K7 path; status | commit succeeds; reported public key and fingerprint equal host `FP`; `custody = PassphraseWrapped`, `posture = DevPostureVolatile` (T0) or `DevPosturePersistent` (T1), `key_count = 1`, `store_revision`; admin transcript contains no blob/passphrase bytes | custody, protects secret input |
| S01b | RT0 | **Import refusals**: aes128-ctr and chacha blobs -> `SshKeyFormatRejected`, answered fast (no KDF; timing recorded against S03's KDF time); tampered last chunk -> `SshImportIntegrity`; second import while A exists -> `SshStoreFull`; wrong order/handle cases are K5 host tests, not repeated | each typed error observed, store unchanged (revision unchanged), subsequent S02 still works | invalid input honestly refused |
| S02 | RT0 RT1 | **Login with the KMS key**: `ssh --user tester --auth publickey --identity KEY_ID -p PORT_MAIN 10.0.2.2 --command 'echo kssh-ok-NONCE'`, passphrase typed once at the no-echo prompt | `$?` = 0 and exact stdout; sshd log has exactly one `Accepted publickey for tester ... ED25519 SHA256:` whose fingerprint equals `FP`, no `Accepted password`, no `Failed password`, no second connection from the guest for this attempt; remote ledger one `kssh-login` line; stderr shows the prompt once and never echoes the passphrase | real publickey auth, no plaintext fallback, secret protection |
| S03 | RT0 RT1 | **Wrong passphrase**: same command, WRONG typed | non-zero `$?` with the K6-defined failure fragment; **no** second prompt, **no** `password:` prompt, sshd log: no `Accepted`, no `Failed password` for this connection (no method switch, no retry); the next correct S02' login succeeds; K5 failure counter +1 (observed through S08) | invalid auth, no fallback |
| S04a | RT0 | **Unknown KEY_ID** (`--identity NOPE0`) | refused with the K6 `SshKeyNotFound` surface; no passphrase prompt; no password fallback; whether sshd saw a connection is recorded and judged against K6's documented order | invalid auth |
| S04b | RT0 | **Key not authorized by the server** (port 2223, authorizes B only) | auth failure; sshd line pattern for an unauthorized key (`Failed publickey` without a preceding `Postponed publickey`, from the C-host control); no signature produced (the server never returned PK_OK, so no `AgentSign`); no password fallback; prompt behaviour (whether the passphrase was asked at all) recorded vs K6's order | unknown key |
| S05 | RT0f | **Signature bound to the session** (lane-only `kssh-faults`): (i) session-id byte flipped before the KMS request; (ii) username changed before the KMS request | KMS returns a valid signature for the *wrong* input; sshd sees `Postponed publickey` then `Failed publickey` for `FP`, no `Accepted`; client auth failure; then a normal login in the same boot succeeds (state not poisoned). Without D8-7, S05 is reduced to the K5 host vector (`k2-cp1.md` §4.5 row 31) and the server-side refusal is **not evidenced** | invalid auth |
| S06 | RT0 | **Forged / foreign caller refused** (driver cell, no flag): (a) ops 21, 22 -> `PermissionDenied`; ops 15, 20 as non-admin -> `PermissionDenied`; op 19 -> `PermissionDenied`; (b) **forged trailer**: a 128-byte valid `SignSshUserauth` frame padded to the largest message `sys_send` accepts (target `IPC_BUF_SIZE` = 4096, `ipc.rs:23`) with the last 32 bytes shaped as a trailer carrying the SSH flag; the kernel overwrites the tail after the payload copy (`syscall.rs:3351-3358`), so KMS sees the driver's real identity; (c) frame with nonzero header `flags`/reserved and an unknown opcode 25 -> **no reply** (`dispatch.rs:163`), observed with a bounded `sys_recv_timeout` (the blocking `KmsClient::call` is not used for these) | (a) typed `PermissionDenied`; (b) `PermissionDenied`, never a signature, floors unchanged (a following S02 works); the accepted send length is recorded (if the trailer region is unreachable the check degrades to "kernel-written trailer not forgeable at this length" and is labelled so); (c) timeout, no state change | invalid auth / admission |
| S07 | all | **No key bytes in serial/logs/screens** | section 8 | secret protection |
| S08 | RT0 RT1 | **Unlock limiter** (order matters): after S02/S03, admin delete A (wrong public key -> `SshKeyNotFound`, correct -> ok) then import L (`-a 4`); five consecutive wrong passphrases (each `SshUnlockFailed`, no distinguishing detail between wrong passphrase / checkint / key mismatch); sixth attempt **with the correct passphrase** -> `SshUnlockLocked`; admin delete + re-import of L -> correct login succeeds | the sixth answer is `SshUnlockLocked` although the passphrase is right; its latency is a small fraction of a KDF-carrying failure (recorded, labelled as timing evidence for "no KDF work", not a proof); N and the reset rule equal K5's documented values (`k2-cp1.md` §3.5: N = 5, reset by restart or key delete); T1: whether the counter survives a reboot is recorded; if it resets, the report states that power-cycling bypasses the limiter (a limit, not a failure) | invalid auth, bounds |
| S09 | RT0 | **Admin lifecycle**: duplicate public key -> `SshKeyExists`; delete wrong-key -> `SshKeyNotFound`, key intact | typed errors, state unchanged | invalid input |
| S10 | RT0 | **Trust before any signature**: publickey to `host:PORT` (changed host key in the trust store), to `gateway` (unknown), and first-trust answered `no` | refused at the host-key stage as in L5; no passphrase prompt; sshd shows no `Postponed`/`Accepted`/`Failed publickey` for those connections; KMS cannot attest "not asked" (stated limit, `k2-cp1.md` §2.1) | changed/untrusted host identity |
| S11 | RT0 (optional) | **Rekey after publickey login** (port 2222, `RekeyLimit 64K`, 1 MiB through the L5 `p9 bulk` helper) | session survives, `ssh-rekeys >= 1` as L5 counts it, passphrase not requested again, one `Accepted publickey` | rekey/bounds |
| S12 | RT1 | **Reboot persistence** (identity L, `-a 4`, the cheap-KDF key; D8-8): boot 1 import + S02 + clean `/bin/shutdown` -> snapshot S1; boot 2 (copy of S1, same ISO, **no import**) status then S02 | boot 2 status: key present, same `FP`, same `store_revision`, posture `DevPosturePersistent`; login succeeds with the passphrase; QEMU exited by guest shutdown, not killed | T1 persistence |
| S12c | RT1 | **Unclean stop (informational)**: SIGKILL QEMU on the first `import-committed` marker, then boot | whatever is observed (key present / absent / `SshKeyUnavailable`) is recorded; **never a pass/fail input and never a durability claim** (D-K2, "synced, not crash-proof") | none |
| S13 | RT1 | **Rollback shown as unprotected**: boot 4 (copy of S2, after the key was deleted in boot 3) shows the key absent; boot 5 runs on the restored S1 copy | restoring S1 makes the deleted key available again (login works) and `posture` still reads `DevPosturePersistent`; no rollback indication. If K4b detects it, that is recorded and the posture label is still `DEV_POSTURE`. The row passes when the observed behaviour is documented either way; it fails only if status or any output claims rollback protection or `ProductionQualified` | honest limit |
| S14 | RT1 | **At-rest scan of the disk image** after each shutdown | the raw seed (32 B), the 64-byte private section, the passphrase and WRONG passphrase do not occur in any canary form in the disk image; presence or absence of the verbatim wrapped blob (`openssh-key-v1` magic) is recorded as K4b-dependent. Claim limit: a **plain** key is protected only by the co-resident dev root, i.e. not against a holder of disk + image (`kms-prerequisites.md` §5) | T1 limits |
| S15 | RT0 (optional, K6-dependent) | **Headless pipeline stage with a plain key P**: `ssh --auth publickey ... --command ...` as a stage piped into `cat` works without a prompt; the same with wrapped A is refused (no terminal) | as K6 documents; reopens S9-5 pipeline exec, not a CBX-24 row | S9-5 |
| S16 | RT0 | **T0 is volatile**: clean shutdown, boot B with the same disk, no import | status: key absent; S02 refused with the typed error; no key bytes on disk (S14 applies to the P1/P5 volumes too) | honest T0 limit |
| G1 | RG (optional) | **GUI passphrase lease** on the desktop image with the pixel oracle: type the command, observe the prompt, type the passphrase | the exact-screen oracle (`phase04_gui_drive.py` `match_full`/`expect_screen`) matches the expected rendering of the visible terminal (prompt, no echo, no extra glyph) at each step and after login; the screen exact-match is the "no key bytes on screen" evidence for the visible window | secret protection |

Order inside a boot is fixed (S00, S01/S01b/S09, S10, S06, S04, S02, S03, S02', S11, S08, then shutdown/S16) so that consumed limiter budget and the one-identity rule (LR-1: one store, one identity; S08 deletes A and imports L) never confound earlier scenarios.

## 8. Secret scan method (S07, S14)

**8.1 What is a secret.** Per run, the host holds: passphrases PASS_A, PASS_L, WRONG (and P's none); the 32-byte seed and the 64-byte expanded private section of A, L (derived from the unencrypted copy of the container: parse `openssh-key-v1` header, skip cipher/KDF fields, read the private section, `checkint` pair, key type, public key, `seed||pub`); the encrypted blob file text (`-----BEGIN OPENSSH PRIVATE KEY-----`, its base64 body, and the binary `openssh-key-v1\0` magic); the cell signing seed of the lane (`seed.hex`, as L5 `no-key-material-in-log`, `qemu-phase09-ssh-test.sh:522`); the fixture host private key and password (as L5, `:519`). Public values (public key, fingerprint, signature, session ids) are not secrets and are expected to appear.

**8.2 Forms.** For every secret byte string `S`: raw, lowercase and uppercase hex, base64 std and URL-safe each at the three alignment offsets (encode `X || S` for `|X| = 0,1,2` and drop the characters that depend on `X`), decimal-byte-list (`[1, 2, ...]` and space-separated) as `Debug` output would print, and for passphrases additionally the hex of the typed key events. Strings shorter than 8 bytes are not used (false-positive control); the 24-character passphrases and 32/64-byte keys qualify.

**8.3 Targets.** `serial.raw.log` and `serial.log` of every boot, the saved `sshd.log` (docker logs), QEMU stdout/stderr, the driver's transcripts (guest-side, passphrases only), `verdict.json` and every other evidence file the judge writes (so the evaluator itself cannot leak), the admin/import tool output, the T1/T0 disk images after each shutdown (S14/S16: seed, private section, passphrases; the ISO is excluded because `/kssh/kssh.conf` carries the passphrases by design), and the final evidence archive directory before it is copied into the repository (8.5). GUI run: every judged frame is compared by the exact-screen oracle; additionally each canary string with `len >= 8` is rendered with the oracle's glyph renderer and searched in the frames (`phase04_gui_drive.py` `render`), and the search is validated by the control below.

**8.4 Controls (the scan must not be vacuous).** The judge ships a self-test run before and after the real scan: (i) plant each canary form into a copy of `serial.raw.log` and require detection of every form; (ii) require that expected public strings (`FP`, `Accepted publickey`, the `[kms]` start line) **are** found in the scanned files (proves the right files were read); (iii) for the GUI search, draw a canary into a copy of one frame and require detection. Failures of a control are class E. A canary hit in a real target is class D (product finding) unless it is a control artefact, and is final.

**8.5 Evidence hygiene.** The repository archive receives only an allowlist: serial logs, `sshd.log`, `verdict.json`, hashes, public keys, fingerprints, QEMU argv, judged frames (GUI). It never receives `$W/keys`, the ISO, kernel_fs.img, disk images, `kssh.conf` or `seed.hex`. After copying, the judge scans the archive directory with the same canary set (`archive-scan.json`) and `git status` is checked for any `id_*`, `*.pem`, `*.img`, `*.iso`. Keys, disks and ISOs under `$W` are deleted at the end (like the fixture `stop`, `phase09_ssh_fixture.sh:127-129`) after the archive scan passes.

**8.6 Not covered by the scan:** guest RAM and CPU state (the seed lives on the KMS stack during a sign and zeroization is a K5 host-test claim, not a QEMU claim; an optional QMP `pmemsave` residue scan is deferred, D8-6, and absence there would not prove zeroization); bcrypt/AES-CTR derived key bytes (the host has no `bcrypt_pbkdf` to derive them); timing channels.

## 9. T1 procedures (RT1)

Disks are raw files under `$W/disks/`; QEMU runs one at a time on one file. Snapshots are plain `cp --reflink=auto` copies taken **only after QEMU has exited** (clean shutdown), so a snapshot is a quiescent image, never a live copy. RT1 uses identity L (`-a 4`) throughout so the limiter and reboot cases stay affordable under TCG. Boot list: **1** (blank `/srv` store, import L, S02, shutdown) -> snapshot S1; **2** (from S1: S12 status + S02 persistence, then the S08 sequence: five wrong passphrases, correct passphrase refused `SshUnlockLocked`, shutdown) -> snapshot S1b; **3** (from S1b: status, then a correct login: locked or not, i.e. whether the failure counter persisted, is recorded; then admin delete, shutdown) -> snapshot S2; **4** (from S2: status empty, delete persisted); **5** (from S1: key reappears and logs in, S13); **6** optional, informational (S12c, SIGKILL on a copy of the blank disk). Every boot uses the same ISO and the same `-drive file=...,format=raw` NVMe arguments as L5. The host-trust store sits on the same `/srv`; boot 2 re-runs one S10 trust check (host key still `KnownExact`) to show the KMS store did not disturb it.

Clean shutdown path: the existing `/bin/shutdown` worker (`app-sys-tools`; added to the lane image, see 5.2) and `-no-reboot`, so the guest's power-off ends the QEMU process `[INFERENCE: the worker powers the machine off and QEMU exits under -no-reboot; to be confirmed in smoke B0, otherwise the runner issues a QMP `system_powerdown`]`. Whether VFS flushes the KMS record before power-off is D-K2's "synced, not crash-proof" territory: S12 succeeds only if the record is on the disk after the guest's clean shutdown; a failure there is a class D finding about K4b, not a harness defect.

## 10. CBX-24 mapping

Row text (`acceptance.md:32`): "SSH rejects changed/untrusted host identities per explicit trust policy and invalid auth; uses real entropy, no plaintext fallback, protects secret input/logs and handles rekey/bounds/EOF/disconnect honestly." Current state: partial; "publickey/KMS custody deferred".

| Row element | Publickey evidence | Scenarios |
|---|---|---|
| changed/untrusted host identity | publickey attempts stop at the host-key stage; no prompt, no sign request visible at the server | S10 |
| invalid auth | wrong passphrase, unknown identity, unauthorized key, wrong-session signature, foreign/forged caller, limiter | S03, S04a/b, S05, S06, S08, S09 |
| real entropy | unchanged from L5: `-cpu qemu64,+rdrand,+rdseed` under TCG, judge `no-weak-entropy` (`:521`); ed25519 signing is deterministic (no RNG in KMS, `k2-cp1.md` §1.7); TCG's RDRAND is host-provided, not a production-entropy claim | S00/S02 |
| no plaintext fallback | server offers password, client never switches after a publickey failure; no unencrypted key path for a wrapped key | S02, S03, S04 |
| protects secret input/logs | no-echo passphrase lease, single prompt; canary scans of serial/logs/transcripts/screens | S02, S07, S14, G1 |
| rekey/bounds/EOF/disconnect | rekey after publickey login; import bounds (64 KiB, chunking) are K5 host tests; EOF/disconnect behaviour is unchanged client code evidenced by L5/L5f | S11 (optional), S00 |
| (publickey-specific, not in the row text) custody | one identity per store, wrapped custody, volatility (T0), persistence/rollback limits (T1) | S01, S12-S14, S16 |

A met-at-ceiling statement can only be made for the elements whose scenarios finish class A (or B with the missing scenario named) in the T0 set, and, for the T1 sentence, in RT1. The row text does not name publickey; the user decides the wording and D10-2/D10-4 consistency (D8-9).

## 11. Harness changes

| Item | Change | Reuse |
|---|---|---|
| runner | new `scripts/qemu-kssh-test.sh`: fork of `qemu-phase09-ssh-test.sh` with the build list, image delta (5.2), three-listener fixture, multi-boot orchestration for RT1, per-boot serial socket and pump (`:258-300`), `P04 INFO kssh-*` reaction hooks only if S11/S12c need them (the S9-6 `p09-hook` pattern, `:307-330`) | work-dir/alias guards from the Phase 10 runner; QEMU argv rules (one user netdev, no `restrict`, no forwards, asserted before start, `:274-278`) |
| fixture | `kssh_fixture.sh` (5.7) | pinned recipe/image |
| keys/disk | `kssh_keys.py`, `phase09_trust_disk.py` unchanged + `mcopy` | |
| judge | `kssh_judge.py`: marker table per scenario (as the `S93`/`S96` lists and `chk()` rules, `:385-440`), sshd log classifier (strings fixed by C-host), canary scanner and controls (section 8), preservation checks (section 12), `verdict.json` | judge structure of `qemu-phase09-ssh-test.sh:385-545` |
| driver | feature `kssh`, `kssh.rs` | `Term`, `login`, `wait_stderr`, `read_conf` patterns (`ssh.rs`) |
| pixel/serial oracles | serial markers as L5; GUI: `phase04_gui_drive.py` oracle imported unchanged for RG | no edit to either script |
| host tests | `scripts/test_kssh_judge.py`: log fixtures for every sshd pattern, scanner with every form and planted canaries (positive and negative), class precedence, allowlist archive | in the offline gate H0 |

Host control runs, not VM boots of the Cellos image: **C-host** (section 5.7) and the scanner self-test. Both are part of H0 and need only Docker, `ssh`, `ssh-keygen`.

## 12. Preservation rules

1. **Protected census (S10).** 546 paths: 470 present, 76 temp paths that no longer exist (`census-before.json` of the S10-9 runs, `paths: 546`, `present`/`missing`; sha256 of the file `31ee2a93…0701` for cand-headless). Composition seen in this session: 52 files under `target/x86_64-unknown-none/release`, 5 under `kernel/src/embedded-x86_64`, many `build/*.iso|qcow2|g1-*` fixtures, `kernel/src/main.rs` (`a9636fee…`). A read-only comparison of the 470 present entries against the disk at HEAD `fad076ee9` (run in this session) found **0 missing and 0 changed**, so K2 CP2's builds did not disturb it. K8 hashes the same path set before and after every run set (hashing takes minutes: the comparison took ~68 s with the disks idle and exceeded 300 s once under load) and requires equality. Therefore **no K3/K5/K6/K7 or K8 build may write into `<repo>/target/x86_64-unknown-none/release`**: all K8 cargo runs use `CARGO_TARGET_DIR=$W/...`, and the K3/K5/K6/K7 slices must be told the same for their bare-metal builds (stated here as a precondition on G-1..G-4 evidence, not as a change to them).
2. **Canonical bytes.** `kernel/src/embedded-x86_64/**`, `kernel/src/main.rs`, the promoted ISO/qcow2 under `build/` and the promoted `cellos-kernel` are never read for writing, never used as an out-dir, `EMBEDDED_OVERRIDE`, `CARGO_TARGET_DIR` or QEMU disk. The runner refuses a `$W` that aliases them (as `qemu-phase10-canonical-test.sh:84-87`) and the build asserts the lane kernel's embedded dir differs from the canonical one (`kernel/build.rs:128-148`).
3. **Frozen sources.** Before the first run: `frozen-sources.sha256` for the 3.1 table, the K3/K5/K6/K7/K4b closure files at their landed hashes, and K8's own new files; any change after the go/no-go means a new gate (S10-9 pattern). Existing S10 archives (`reports/s10-*-evidence`) are not modified; K8 writes only to its own `reports/k8-evidence/`.
4. **No lift of any guard** (G2a/G2b/G2d/G3/G4/G5, non-Intel and cloud-image refusals); no new signing route; `cellos-sign` is called exactly as L5 does.
5. **No network** beyond loopback and no new Docker image (G-9); no hostfwd/guestfwd into the guest, no `restrict` change (the fixture listens on `127.0.0.1` only); no inbound sshd in Cellos (the assembler's `FORBIDDEN_PATHS` stays irrelevant: no `/bin/sshd`).
6. **Secrets.** Section 5.6 and 8.5. No key, seed, passphrase, image, disk or ISO is committed or staged; no commit or push by K8 without a separate request.
7. **Token and cleanup.** One `RUN_CONSUMED-k8-<set>` token per run set, claimed with `O_EXCL` before QEMU is started (S10-4.17 practice); a stray QEMU or Docker container after the run is cleaned and recorded.

## 13. Run budget, classes A-E

### 13.1 Budget (recommended, D8-3)

| Step | Count | Notes |
|---|---|---|
| H0 offline gate: judge tests, scanner controls, C-host control, `cellbox-ceiling-diff`, host builds | one clean invocation, zero failures/errors/skips; offline fixes allowed, each iteration recorded | before any VM |
| B0 boot smoke (READY + KMS start line + S00 parity) | one per image variant (I-T0, I-F, I-T1) | no evidence value, like the S10-7 D7-8 smokes; a failed smoke ends that variant until a new gate |
| Evidence runs | **one RT0, one RT0f, one RT1, optionally one RG**; **no rerun** for any class | class E before the first S02 sign epoch caused by host infrastructure may be re-attempted **once** only with a recorded decision (S10-3c status-3 rule: pre-QEMU preparation failures are not runs) |
| KDF calibration | measured inside B0/RT0 (rounds 16, 4, optionally 64); not a pass criterion | K5 already owns the formal measurement (G-2) |

### 13.2 Preflight (all must be true; stop before QEMU otherwise)

Clean tree and recorded HEAD/dirty digest; slice commits K3-K7(+K4b) present; frozen sources match; census before saved; Docker image present; QEMU 10.2.0 and ports free; disk space; `$W` rules; token claimed; H0 passed; C-host strings fixed.

### 13.3 Classes (K8, modelled on S10-4.17 section 6, `phase-10-s0-proposal.md:2662-2677`)

"First sign epoch" = the first S02 login reaching its server-side verdict.

| Class | Condition | Consequence |
|---|---|---|
| **A SUCCESS_COMPLETE** | every scenario of the set executed in the fixed order, every pass criterion met (host oracles included), scanner controls pass and no canary found, census equal, archive scan clean | evidence for the set recorded at the stated ceiling |
| **B SUCCESS_CORE_ONLY** | S00, S01, S02 pass with the sshd fingerprint equal to `FP` and the scan clean up to that point, then any later STOP | core proven; each unrun scenario listed as unproven |
| **C OBSERVABILITY_INCOMPLETE** | healthy host but an oracle cannot attribute (sshd wording not matching the C-host strings, missing log, deadline expiry without cause, screen oracle mismatch not attributable) | recorded as is; no inference of success or product failure |
| **D OBSERVED_NEGATIVE_RESULT** | oracle integrity proven and the product contradicts the criterion (canary found, wrong-session signature accepted, foreign/forged caller served, wrong or locked passphrase accepted, publickey login falling back to password, status claiming rollback protection) | a real finding, final; **any D on a secrecy or admission row is stop-ship for the slice that owns it** |
| **E HOST_FAILURE** | QEMU/Docker/serial/harness exception, scanner control failure, preservation drift, cleanup failure, unattributed stop | host defect recorded; not a verdict on the product |

Precedence if a failure occurs before the first sign epoch passes: E, then C, then D. After that the class is A or B by progress and any later E/C reason is a tail STOP, not a downgrade. No fix-and-rerun loop for any class; a further attempt needs its own CP1. "Unprotected rollback" (S13) and "limiter resets on reboot" are expected limits and are never class D.

### 13.4 Effort and timing

Development M, about 8-10 working days: runner/fixture/judge and tests 4-5, driver scenarios 2, image/smoke debugging 1-2, runs and dossier 1-2. Run time is `[INFERENCE]` and dominated by TCG: the S10-3 estimate for L5 + L5f was about 0.5 h (`phase-10-s0-proposal.md:1037`); RT0 adds a KDF-bearing failure sequence (S03, five S08 failures, successes) whose cost is the unmeasured TCG bcrypt time (host 4.7 ms/round natively, `k1-cp1-draft.md`), so plan for 0.5-2 h per RT0, a similar RT1 with up to five boots, and a GUI run on the S10-9 desktop scale if RG is approved.

## 14. What the evidence honestly proves, and does not

**Proves (at the ceiling "private fixture image, fixture kernel with the K3 delta, QEMU/TCG, loopback Docker OpenSSH_9.9p2, `DEV_POSTURE`"):**
- end-to-end ed25519 publickey authentication by a key whose private half is held by KMS, accepted by a real OpenSSH server for the key's host-computed fingerprint, with no password fallback available to hide a failure;
- the refusals that are observable on the wire or the IPC: wrong passphrase, unknown identity, unauthorized key, (with D8-7) wrong-session and wrong-user signatures, flagless caller, forged trailer at the tested length, malformed/duplicate imports, limiter lockout beating a correct passphrase;
- absence of the enumerated secret forms from serial, logs, transcripts, the (GUI) visible window and, at T1, the disk at rest, with a scanner whose positive controls pass;
- T1: a clean shutdown and reboot keep the key; a disk revert resurrects a deleted key, i.e. rollback is not protected and the status says `DEV_POSTURE`.

**Does not prove:** any production claim, `ProductionQualified`, T2; confidentiality against a holder of disk + image at T1 (dev root co-resident; for a plain key not even the passphrase protects it); rollback resistance, monotonic time, crash/power-loss durability (S12c is informational); zeroization or RAM residue (host tests and a deferred optional scan only); that KMS enforces host-trust-before-signature (it cannot; ordering is the client's, evidenced only by the server's view); protection against a compromised `cellbox-ssh` (an admitted caller can request signatures for sessions it chooses); that the executed bytes are the promoted/canonical bytes (kernel, `kms`, `cellbox` differ by construction; the run is not a promotion and not a candidate); behaviour on a physical Intel machine, VMX/KVM or a Tier-3 guest endpoint (CBX-23 guest half and CBX-25 unchanged, `acceptance.md`); KDF time on real hardware; parser robustness beyond the exercised blobs (K5 differential tests and fuzzing own that); cross-version KMS/client compatibility (matched builds only, O-9); more than one concurrent client; the `Accepted`/`Failed` wording beyond OpenSSH_9.9p2.

## 15. Risks

| Risk | Mitigation / disclosure |
|---|---|
| TCG KDF time makes S03/S08 long or trips driver deadlines (the single-threaded KMS loop also stalls other KMS clients) | calibrate in B0; limiter key at `-a 4`; per-scenario deadlines in the driver, not global; a timeout is class C, not D |
| `KmsClient::call` blocks forever on a dropped reply (O-9) | S06(c) uses bounded `sys_recv_timeout`; a hang in S02-S04 is stopped by the runner's boot window (class C/E) |
| sshd log wording differs from the inferred strings | C-host control fixes strings before any guest run; mismatch is a harness fix in H0 |
| K3/K6/K7 names, order of probe/unlock/sign, admin class unknown | section 2 re-read at each CP2; scenarios S03/S04 are written against "K6's documented order" |
| a lane-only `kssh-faults` cellbox changes ceilings or leaks to products | feature off by default, `cellbox-ceiling-diff` run on both builds, L5f-style "lane-only bytes" label, never promoted (D8-7) |
| the lane policy path differs from production (D8-2) | smoke control; limit recorded if policy-less |
| census drift from other slices' builds writing `<repo>/target/x86_64-unknown-none/release` | section 12.1 precondition; before/after census; class E on drift |
| a secret reaches the repository through the archive | allowlist copy, archive scan, `git status` deny-list, `$W` outside the repo |
| Docker image absent, or WSL/Docker port publication changes | G-9 stop with status 3; the fixture reads the published port each start as the original does |
| scope creep into pipeline exec / non-UTF-8 stdin | optional S15 only, and only if K6 reopened it |
| first-ever boot of `/bin/kms` in an x86 image exposes KMS or boot-order defects unrelated to SSH | B0 smoke isolates them from evidence runs |

## 16. Out of scope

Canonical promotion and any recipe/route/guard change (a later separate CP, S10-5 style, if the user ever wants `/bin/kms` in the promoted image); T2/K9; CellosFS crash safety (X); Tier-3 guest endpoint; physical or KVM/VMX runs; fuzzing; RAM residue scanning (deferred D8-6); RSA/ECDSA/agent/keyboard-interactive; a CLI import command (D-K3); multi-identity stores (LR-1); changing `command-contracts.md`/`command-catalog.md`; commits of evidence images.

## 17. Evidence layout (new directory only)

`reports/k8-evidence/{preflight, h0-gate, c-host, smoke-b0, rt0, rt0f, rt1, rg, census-before.json, census-after.json, frozen-sources.sha256, archive-scan.json, summary.json, evidence.sha256}`; each run dir holds `serial*.log`, `sshd.log`, `verdict.json`, `qemu-argv.txt`, `provenance.sha256`, `scan.json`, `controls.json`, `classification.json`. Nothing outside it is written.

## 18. Decisions requested

| ID | Question | Options | Recommended |
|---|---|---|---|
| D8-1 | Image route | **A** private fixture/lane route (this draft); **B** extend the assembler profile + route table + promotion CP (changes pinned recipe and guards; D10-3 = A lift) | **A** |
| D8-2 | Policy coverage | **A** `policy-required` kernel + lane `POLICY.BIN` (production rows + lane rows) if the smoke passes, else fall back; **B** policy-less lane (L5 shape), limit recorded | **A** with fallback to B |
| D8-3 | Run budget | **A** one evidence run per set, no rerun, one infra-only re-attempt before the first sign epoch; **B** allow offline-fixed reruns like the S10-3 lanes | **A** |
| D8-4 | Shared-cell payload match against the promoted image | yes (optional, informational) / no | yes, informational |
| D8-5 | GUI run RG (exact-screen oracle) | include one desktop run / headless only | include one, after RT0 is A or B |
| D8-6 | QMP RAM residue scan after sign | include / defer | defer (no claim either way) |
| D8-7 | Lane-only `kssh-faults` cellbox for S05 (Cellbox owner) | approve / skip (S05 becomes host-vector only, server-side refusal unevidenced) | approve |
| D8-8 | TEST key parameters | ed25519, rounds 16 (A), 4 (limiter L), passphrases 24 random printable chars; optional one-off calibration at 64 | as stated |
| D8-9 | Disposition rule | the CBX-24 sentence "publickey half met at DEV_POSTURE ceiling" is made only by an explicit user disposition at K8 CP2, naming T0 and/or T1, consistent with D10-2/D10-4 | as stated |
| D8-10 | Ownership of the driver/lane-script edits | Cellbox single integrating owner under a bounded file list (section 3.3) | as stated |
| D8-11 | Docker image missing at preflight | stop (status 3) / authorize one network build of the pinned recipe | stop |
| D8-12 | T1 scope | RT1 only after K4b CP2; if K4b slips, K8 closes at T0 and says so | as stated |

## 19. Not checked

No build, test, VM, Docker, `ssh-keygen`, key generation or network action was run for this draft. Not read: `kernel/src/policy.rs`, `cap.rs`, `tcb.rs`, `syscall.rs` beyond the trailer function (`:3351-3375`), `boot_ceiling/selftest.rs`, `tools/cellos-fs-provision`, whether VFS flushes on shutdown, `sys_send` maximum message length and the exact `/bin/shutdown` power-off behaviour, `service-kms` `boot_probe_store` under a lane without `/srv`, whether `policy-required` combines with the phase02/phase04 lane features, how an unlisted path is treated by a policy blob, the Docker store contents, OpenSSH 9.9p2 log wording, `ssh-keygen -Z` output, `mcopy -i img@@offset` against the fixture disk, the `candidate.json` schema against the promoted manifest, the phase10 runner's `Machine` class reuse, VFS read rights for an admin cell, and any K3/K4b/K5/K6/K7 document (none exists yet). The census comparison (470 present paths, 0 missing, 0 changed) is the only command executed for this draft besides `sha256sum` and reads.
