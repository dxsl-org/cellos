# T3 CP1 DRAFT: Tier-3 guest endpoint at the SVM/TCG ceiling (CBX-23 guest half, CBX-25) (status AWAITING CP1; documents only; nothing edited, built, fetched or run)

Survey and evidence: [survey.md](survey.md) (HEAD `12f0b9d19`, tree clean). Probe it builds on: `261020-1750-cellbox-cli/reports/s10-10-evidence/probe/tier3-and-guards.md` (sha256 `417b324b4ee7608ac43236be35b861a7d8f2e123fe7f11ae992594810ed27e35`), section "Go / no-go", item 5 with items 3, 4 and 6.

Requested by the user 2026-10-30: open the Cellbox Tier-3 guest endpoint at a **reduced ceiling**. This draft proposes the work as slice **S10-11** of Phase 10 (follows S10-10 in `phase-10-s0-proposal.md`) with sub-slices T3-A..T3-E.

## 1. Ceiling statement (to be recorded verbatim in acceptance rows and the dossier)

> **Ceiling reduction (user disposition 2026-10-30, S10-11; same kind as DS6-1 B and D10-1 C).** The CBX-23 guest half and the CBX-25 VM route/server/credentials proof are made, if at all, against a Linux guest running OpenSSH on the **existing AMD SVM VMM** under **QEMU-TCG 10.2.0** (`qemu64,+svm`), reached from a Cellos cell by **one loopback-only QEMU `hostfwd`**, with a **password** credential, in a **private lane-only image**. This is **not** Intel VMX/EPT Tier-3 and **not** the P09 outcome; the plans `260711-1917-tier3b-x86-vtx`, `260712-0952-tier3b-vm-hardening-compat` and `260722-2330-tier3b-finish-g5-lite` and relations L11/L13/L18 are unchanged. Not claimed: KVM, physical x86, production, remote or C2C use, a guest bridge or guest-lifecycle authority, virtio-rng, publickey to the guest, a matched-candidate image (the hypervisor cell, the fixture kernel and the guest are lane-only bytes), any inbound Cellos sshd, any guarantee about HEAD beyond the one retained run.

## 2. Honest go / no-go per sub-item

| Sub-item | Call | Reason (survey section) |
|---|---|---|
| T3-A guest image recipe (OpenSSH in the pinned Alpine PVH initramfs) | **GO, conditional on authorization A-NET (§8)** | Kernel and initramfs are cached and match their pins; libcrypto 3, zlib, musl, `apk` and the Alpine keys are already inside the initramfs; only the OpenSSH apks must be fetched once (survey §2). |
| T3-B VMM boot proof under TCG at HEAD | **CONDITIONAL GO as an experiment; outcome unknown** | Last retained boot 2026-09-29; four relevant commits since, including net admission by `CALLER_FLAG_NET_L2` and the kernel `main.rs` VMX rewrite (survey §1.4). It may fail for reasons outside this program; if so the program stops (§5 STOP rules) and the rows stay as they are. |
| T3-C route cell → guest | **CONDITIONAL GO via R1 only (hostfwd loop through host loopback); NO-GO for R2 (Net-cell hairpin) inside this CP1** | No in-Cellos path exists (survey §3.2). R1 needs no source edit but is unproven libslirp behaviour. If R1 fails, the SVM/TCG endpoint is infeasible without a Net-service source change, which belongs to a separate, locked-adjacent slice. |
| T3-D cellbox lane scenarios (password; no replay; no inbound sshd) | **GO after T3-B and T3-C pass** | Existing `ssh_checks` scenarios already target `10.0.2.2:PORT` and read `/p09/ssh.conf`; with R1 they run unchanged against the guest (survey §3.4). Three small, new guest scenarios plus a readiness wait are needed. |
| T3-E dossier and dispositions | **GO after T3-D** | Documentation only, after a CP2. |
| Credentials | **GO for password** | Client supports it; per-run host key and password; no KMS (survey §3.4). |
| Intel VMX, KVM, publickey-to-guest, Net hairpin, virtio-rng | **NO-GO here** | Out of scope or locked (survey §5). |
| Locks | **No handoff required; owner acknowledgement requested (§9)** | No file under L11/L13/L18/L25/L32/L36/L37 scope is edited (survey §5). |

Overall: a **conditional GO** to run T3-A, then a bounded boot/route experiment (T3-B and T3-C together), then T3-D only if that experiment passes. A failed experiment ends in a recorded no-go with the exact evidence, not in a weaker substitute (no host localhost, no labelled container, no Docker fixture presented as the guest: D79-2 holds).

## 3. Slice decomposition

Order is strict. Each slice has its own gate; a failed gate stops the program (§5).

### T3-A: guest image recipe (new files only; one authorized fetch)

| Item | Detail |
|---|---|
| Inputs, already present | `.alpine-cache-x86/vmlinux`, `vmlinuz-virt`, `initramfs-virt` (pinned sha256 in survey §2.2). Not modified; outputs go to the work dir. |
| One fetch (A-NET) | From `dl-cdn.alpinelinux.org` (Alpine v3.21 `main`, x86_64, same point release family as `ALPINE_VERSION=3.21.7`): `APKINDEX.tar.gz` and the `.apk` files `apk` resolves for `openssh-server`, `openssh-server-common`, `openssh-keygen` (and `libcrypto3`/`zlib`/`musl` only if the resolution requires versions the initramfs lacks). GET only, no other host, no credentials, into `$W/apk-fetch`. Expected 4–8 files, under 20 MiB `[INFERENCE]`. |
| Pinning | The authorized fetch **produces** the pins: sha256 of every file and of `APKINDEX.tar.gz`, recorded in `tests/guests/x86-sshd/apk-pins.sha256` and in the T3-A evidence. The recipe refuses any later run whose files differ. `apk` verifies the index signature with the Alpine keys already in the initramfs. |
| Guest init | `tests/guests/x86-sshd/guest-init.sh`: mount proc/sys/dev/devpts, `modprobe virtio_net`, wait for crng with a timeout (no virtio-rng), `eth0` static `10.0.2.16/24` gateway `10.0.2.2`, user `tester` + `chpasswd`, offline `apk add --no-network --repository /apks`, per-run host key, `sshd_config` mirroring the fixture profile (`phase09_ssh_fixture.sh:85-98`: ed25519 only, `curve25519-sha256`, `chacha20-poly1305@openssh.com`, password yes, root login no, no forwarding, `PerSourcePenalties no`), `sshd -D -e`, helper `p9`, markers `TIER3_SSHD_*`. |
| Repack | `tools/repack-initramfs.py` (unchanged) with `--add` for init, apks, key, config, helper, and `--prune-modules-to` like `prepare-x86-app-initramfs.sh`. The `/virtio-e2e` selector file in the FAT image picks `rdinit=/bin/virtio-e2e-init`; the init is added under that name, as the nginx gate does. |
| Gate A (offline) | `bash -n` on both new scripts; `python3 -m py_compile` on the judge; host test file passes; `sha256sum -c apk-pins.sha256`; a read-only audit of the produced cpio (names, modes, absence of `sshd` host key in the repo, presence of the pins) recorded as JSON; protected-path digest unchanged. |

### T3-B: VMM boot proof under TCG, then T3-C route (one boot, two gates)

Both gates are judged on the same private image, which is the full-run image: the driver (built with feature `guest`, so the driver edits of §6.2 land at the start of T3-B) first runs the readiness wait (the cell-side probe for T3-C). In `--stage boot` the lane script kills QEMU as soon as B1, B2, C1 and C2 are all satisfied, so no scenario beyond readiness runs in that stage.

| Gate | Pass criterion (all must hold, from the serial log and host probes) |
|---|---|
| B1 VMM | `x86 virt: root operation active (SVM)`; `[hv-x86] hypervisor cell starting (SVM PVH)`; `[hv-x86] vCPU ready — entering run loop`; no `KERNEL PANIC`, no `[fault] Cell`, no `[hv-x86] …fail/error/unexpected/guest triple-fault`. |
| B2 guest | `[guest-uart]` line with the guest kernel version (6.12.81-0-virt `[INFERENCE]`), `TIER3_SSHD_CRNG_READY`, `TIER3_SSHD_NET_UP`, `TIER3_SSHD_LISTENING` and a `Server listening on 0.0.0.0 port 22.` line from `sshd -e`. |
| C1 host-side route (R4, a probe only, **not** claim evidence) | lane script runs `ssh-keyscan -t ed25519 -p P 127.0.0.1` against the hostfwd listener; the key's SHA-256 fingerprint equals the pre-generated host key's. |
| C2 cell-side route (R1) | driver marker `P04 INFO ssh-guest-ready attempts=N` from a `cellbox ssh` attempt to `10.0.2.2 -p P` whose output names exactly the guest host-key fingerprint (the changed-key refusal path prints it before any credential); the guest's `sshd -e` log shows `Connection from 10.0.2.2 port N`. |

### T3-D: cellbox scenarios against the guest (full run)

| Item | Detail |
|---|---|
| Reused unchanged | the existing `ssh_checks` scenarios (`ssh.rs:164-352`): `ssh-conf`, `ssh-refusals`, `ssh-changed-host-key`, `ssh-first-trust-no`, `-paste-refused`, `-yes`, `-known-after`, `ssh-wrong-password`, `ssh-exec-bytes`, `ssh-exec-stdin-eof`, `ssh-exec-signal`, `ssh-pty-shell`, `ssh-pty-resize`, `ssh-pty-escape-disconnect`, `ssh-shell-alive`, `shell-long-line`. They already run against `10.0.2.2` and the port in `/p09/ssh.conf`. |
| New, guest only (`ssh/guest.rs`) | `ssh-guest-ready` (readiness wait, T3-C2); `ssh-guest-cancel-producer` (a guest `yes` started with a pid file, cancelled from the client, a second login proves the process is gone: "producer traffic stops on consumer close"); `ssh-guest-fault-may-have-run` (guest helper writes one ledger line then kills its own session process; the client reports exit 255 with an `unknown` outcome via the `completion` record; no automatic retry); `ssh-guest-no-replay-ledger` (second login reads `/tmp/p09-ledger`: exactly one `p09x-selfkill` line). |
| Not run (need Docker hooks or a host sink) | the S9-6 group (`ssh-fault-*`, `ssh-rekey-*`, `ssh-backpressure-bytes`, `ssh-timeout-*`, `ssh-cancel-exec`, `ssh-fault-restores`); their other-host evidence stays as recorded (L5f). |
| Judge | new `scripts/tier3_ssh_guest_judge.py` (host-testable, like `kssh_judge.py`): required markers each exactly once, zero `FAIL`, guest markers, QEMU argv invariant (below), ISO/kernel/initramfs hashes, no secret in any log, no `sshd` in the FAT layout or boot config, fingerprint equality, fixture kernel features recorded. |
| QEMU argv invariant (new, narrower than the old "no forwards" rule) | one `-netdev user,id=net0,hostfwd=tcp:127.0.0.1:P-10.0.2.16:22`; no `restrict`; no other `fwd`; one `e1000`; `-m 1G`; `-accel tcg`; `-cpu qemu64,+pdpe1gb,+svm,+rdrand,+rdseed`; no `-enable-kvm`; QEMU is the pinned 10.2.0 binary (`~/.cache/cellos/qemu-10.2.0/bin/qemu-system-x86_64`, sha256 `169fb4eaaa79f7ad1f3a2d1a1f3534a0807354eabdedaf6a51b6f4b1412241fe`). |
| Gate D | judge exit 0 on one complete run. |

### T3-E: dossier and dispositions (documents only, after CP2)

`cp2-dossier.md` in the plan directory with exact observed outcomes, deviations, hashes; then the doc edits in §6 (existing files). Nothing is claimed that the judge did not pass.

## 4. Route design (R1) and the decisive experiment

```
cellbox-ssh (Cellos cell)  --TCP-->  10.0.2.2:P  (slirp alias of host loopback)
   └─> QEMU's own hostfwd listener 127.0.0.1:P  --slirp-->  10.0.2.16:22
        └─> e1000 -> Net cell L2 bridge -> hypervisor virtio-net -> nested guest sshd
```

- Guest address is **static `10.0.2.16`**, not the outer lease `10.0.2.15` (`[INFERENCE]` the outer Net cell takes `.15` by DHCP) and not a DHCP client.
- Listener: loopback only, one port, chosen by the lane script from a free port (no fixed port), removed with QEMU.
- The experiment is B2 → C1 → C2 in that order. **If C1 passes and C2 fails**, the VMM, the NIC bridge and sshd are proven but a Cellos cell cannot reach the guest through R1: record "no-go for the cell route; Net-cell hairpin (R2) required" and stop. If C1 fails, record where (guest not listening, ARP not answered, hostfwd refused) and stop.

## 5. Run budget and STOP rules

| Rule | Value |
|---|---|
| QEMU runs | T3-B/C: at most **2** boots (one planned, one after an offline fix of the guest init or lane script recorded as an iteration). T3-D: at most **2** full runs (one planned, one only for a host/environment error, not a marker FAIL). No fix-and-rerun after a marker FAIL without a new CP1 (same rule as S10-4.17 and K8). |
| Windows | Set after the first successful T3-B timing: window = 3 × (time to `TIER3_SSHD_LISTENING`) + per-case budgets; **hard cap 4 h** per run. If T3-B itself does not reach `TIER3_SSHD_LISTENING` within 60 min, it counts as a failed gate. Per-case driver deadline for the guest build raised in `ssh.rs` (`SSH_MS` under `feature = "guest"`), value recorded from T3-B. |
| STOP-1 | no `vCPU ready`, a kernel panic, a cell fault, `create_vm failed`, or a guest triple fault: stop, record "VMM at HEAD does not boot a guest in this image" with the kernel-log reason; rows unchanged. |
| STOP-2 | guest boots but never reaches `TIER3_SSHD_LISTENING` within the window: stop after the one allowed offline iteration. |
| STOP-3 | C1 fails: stop (route or guest NIC path). |
| STOP-4 | C1 passes, C2 fails: stop (R1 does not deliver a cell route). |
| STOP-5 | any secret (seed, password, host private key) found in any log/artifact: stop, scrub, do not archive. |
| STOP-6 | any protected-path digest changes, or `Cargo.lock` changes: stop and report. |
| No substitution | A STOP never leads to a Docker fixture, host-localhost server or printed label standing in for the guest. |

## 6. Closure

All hashes are SHA-256 at HEAD `12f0b9d19`, taken 2026-10-30 with a clean tree. Anything else under `git status` at execution time is a stop.

### 6.1 New files (no hash yet)

| Path | Slice | Purpose |
|---|---|---|
| `.agents/261110-0900-tier3-guest-endpoint/plan.md` | CP1 approval | status/gate frontmatter and index (documents) |
| `.agents/261110-0900-tier3-guest-endpoint/cp1-approval.md` | CP1 approval | digest-bound record of the user's decisions |
| `.agents/261110-0900-tier3-guest-endpoint/cp2-dossier.md` | T3-E | results and deviations |
| `.agents/261110-0900-tier3-guest-endpoint/reports/t3-{a,bc,d}-evidence/**` | A,B/C,D | small files only: sanitized serial log, qemu argv, verdict JSON, fixture config, pins, image/ISO/initramfs hashes, protected-path digests |
| `tests/guests/x86-sshd/guest-init.sh` | T3-A | guest init (section 3) |
| `tests/guests/x86-sshd/apk-pins.sha256` | T3-A | produced by the authorized fetch |
| `scripts/fetch-tier3-sshd-apks-x86.sh` | T3-A | one-time fetch into `$W/apk-fetch`; refuses non-listed hosts; writes pins; never runs in lanes |
| `scripts/prepare-tier3-sshd-initramfs-x86.sh` | T3-A | verify cache pins and apk pins, repack, write only under `$W`; refuses to overwrite `.alpine-cache-x86` |
| `scripts/qemu-tier3-ssh-guest-x86.sh` | T3-B/C/D | private lane (modes `--stage boot` and full); modelled on `qemu-phase09-ssh-test.sh`, **not** a modification of it |
| `scripts/tier3_ssh_guest_judge.py` | T3-B/C/D | independent verdict |
| `scripts/test_tier3_ssh_guest_judge.py` | T3-A..D | host tests of the judge (marker accounting, argv invariant, secret scan, fingerprint rule, no-sshd rule) |
| `cells/tests/phase04-terminal-driver/src/ssh/guest.rs` | T3-B (readiness), T3-D (scenarios) | guest-only readiness wait and scenarios |

### 6.2 Existing files to edit (hash is the pre-edit digest)

| Path | SHA-256 | Edit | When |
|---|---|---|---|
| `cells/tests/phase04-terminal-driver/Cargo.toml` | `8040b18eb7191cd9306a460eeae68d8d042f6744e5f0b4a3d4fd1f8d56eebc90` | add feature `guest = ["phase09-ssh"]` (no dependency, so `Cargo.lock` is unchanged) | T3-B (before the first boot) |
| `cells/tests/phase04-terminal-driver/src/ssh.rs` | `efca2f77d4d914abb9ab10fc919c493ae399957c28380b3e04b7b2bf2793ed32` | (1) after `:22` add `#[cfg(feature = "guest")] pub(crate) mod guest;`; (2) `:24` `SSH_MS` raised under `feature = "guest"`; (3) after the `let Some(f) = fixture else { … };` block (`:183-185`) a `#[cfg(feature = "guest")]` readiness call; (4) at `:354` `s96_checks(a, &f)` guarded `#[cfg(not(feature = "guest"))]` and `#[cfg(feature = "guest")] guest::guest_checks(a, &f);`. Default, `phase09-ssh` and `kssh` builds compile the same code as before. | T3-B (readiness) and T3-D (scenarios) |
| `.agents/261020-1750-cellbox-cli/acceptance.md` | `226888b53924802130af096cf96f3baa87a2a61e7b9b6fc34518bc7d59699d67` | CBX-23 and CBX-25 rows (§7) | T3-E, after CP2 |
| `.agents/261020-1750-cellbox-cli/phase-09-ssh-client.md` | `fce05ad752ad2903141695331ee80e06c46ca09c755714880401a30bdfa7368e` | deviation-log entry and criterion 1/4 notes with the ceiling statement | T3-E |
| `.agents/261020-1750-cellbox-cli/phase-10-qualification.md` | `33c2d1ee84faa46904026092c7b066f8a19ed1adf39d28168a77d655bdc8a774` | criterion 1 note (what is met at which ceiling; criterion stays unchecked unless decided in D-T3-8) | T3-E |
| `.agents/261020-1750-cellbox-cli/phase-10-s0-proposal.md` | `01f2d442eb27d3a208928212cb8c011c7d6d7a5f44f2f32d6cb2d7df8a6e82e3` | new §11 "S10-11" with CP1 record, results, CP2 record | CP1 approval and T3-E |
| `.agents/261020-1750-cellbox-cli/plan.md` | `d90d4abb61d6098ab1a3ad1b61726bc7e8d8c783ee62ae52ecdb9a7dfb31f8d4` | `executionGate` and phase 10 row wording | T3-E |
| `.agents/261020-1750-cellbox-cli/verification.md` | `77147ebd4fb5c9a9b4c18d5c039be68d02aa13ac884a3b686c16d3d158551c98` | line 5 status sentence only | T3-E |
| `.agents/261020-1750-cellbox-cli/reports/rollout-containment.md` | `f92f0d96d340551988c7ff011e03a55698838d40fa82fa89f798068f9de17836` | "Not claimed" paragraph (`:78`): add the SVM/TCG guest-endpoint ceiling | T3-E |
| `.agents/plan-portfolio.md` | `baa97c210d3320d3c40ecfeea0316face1d592eb870ccbc0c31a9e90fa801f1b` | one entry for the program and its status | CP1 approval and T3-E |
| `docs/cellbox-cli.md` | `28ac3c3ab9834e04c207a53cdcd2ce9ad94e600da9e741b490616c16a6e3057f` | line 10 SSH bullet | T3-E |
| `CHANGELOG.md` | `8866d0db4b1044c9c5567418ac9319d5941d7ac03d3d9c134c12270b76bdba03` | one entry (CP2) | T3-E |

Not edited on purpose: `command-catalog.md` (`f06f38ce7447758f7b49e58d6e98dc8309b583515bf1ffd3608e665011a5fd0f`, digest-bound and user-acknowledged at S10-5 CP2; its "excluding Tier-3 guest" wording is superseded by the acceptance row, see D-T3-10), `contract-deltas.md` (`e068d89d98afbc679b0d3561deef2f3344291ae923a7299a12c734fbcc7e768f`: no contract delta), `.agents/cell-contract-0024-locks.md` (`258e920e384a0c939657b318cf737ffc7d22bb4c4b12d1b2a736025512311499`: no relation narrowed), the three Tier-3 plan directories and the controller plan.

### 6.3 Existing files reused unchanged (digest pins; any change is a STOP)

Driver and lane templates: `cells/tests/phase04-terminal-driver/src/main.rs` `b661277daadeadd43904302a31bd9546028b6717bb2bbecd03f03574bdf16ca3`; `…/src/ssh/kssh.rs` `7474e8b9761c62b24c0499050b7d2e0bb1d5cc6e0bfc37bd9ad09ef6e3288a50`; `scripts/qemu-phase09-ssh-test.sh` `620b735d404037b79c883fde5272e0f1c3cd11c4432ac786003f031ccc071dc2`; `scripts/phase09_trust_disk.py` `10901b916c948efd62b632e2144e1a3ab519ce8487d324f87e42c60390e6e5e1`; `scripts/phase09_ssh_fixture.sh` `07c6747139a41edc686c96c1d3f2763b5092dcc5eece8ec724970526e5eb8b50` (reference only; Docker unused); `scripts/kssh_boots.py` `651ebadb019a3dc31f634cbff2c2efa6cf0fb9eea544545a4b71781b2a6b6388`; `scripts/kssh_judge.py` `8f2e06e0eb506040d306cb45c7204178eaeffb287b1994890e8a4d8b2d0dd732`.

Image and guest tooling: `tools/repack-initramfs.py` `dd9cd07b51afc93b1891537e04cde10c6fca936676a7ce35e1f03d21a4ff6ff1`; `tools/mkfat32.py` `b82c284a714ed2f546783085da035328d351b37e712624924b27eb5826500565`; `tools/inspect_fat.py` `a41dfad334ac7f459fa71f010aebea9b82e43eb0b5a3e0e8daf533044d745031`; `scripts/prepare-x86-app-initramfs.sh` `bc9b2bdd9859d54ea306abcc070f913d40cfa638eb5fcac1babb20f18684df36`; `scripts/fetch-alpine-x86.sh` `4e2ec9769047f494f13d4c3433de8cc5a12599def5f8ea3180c05d0f9b3726e6`; `scripts/make-hypervisor-fs-x86.sh` `a0c12d03ae3d993c54938e7b3e0c396f5f6e9909ff572cc106a379fb06ea565b`; `scripts/lib-make-hypervisor-fs.sh` `98c28db9cd248c96fe473ab6eb2ad1ad1e5a2334463197e03fc045180bf5f527`; `scripts/qemu-hypervisor-smoke-x86.sh` `b9470e52903cbb04a90646377edd0a91c1d009d75c89a1703a064c30efe81044`; `scripts/qemu-x86-nginx-gate.sh` `2f595e325a37bf3b681562667bd4b62f0ac6d26a5622183ed50ccdbb14b1962b`; `tests/guests/x86-nginx/guest-init.sh` `a62776074fa8f6a319f4fb0f925262b2571b93d9d4f8e5628ac04c6e9fa4322d`; `scripts/generate-boot-config.py` `8ffc3177151f0fc8e3366d97698445d782b1db40e4c2bc91e0c4bf485e6a9d0e`; `config/autoload.toml` `85661e78ac6020cb60dec9bf32c6db503ba3c2fca5265c3913276b7e8885985f`; `scripts/sign-policy.py` `7e2c2cc7cb3fbd30d36d40525128fdc7f917dadf65daf7df6106ddee24b10203`; `scripts/x86/make-iso-ci.sh` `8f5bef774ad048cd456b3e3f0c3a2a0181ac8c81405d4a202de2554b9988593b`; `scripts/x86/check_default_kernel.py` `150f40a8e52ce05859e8eeb6dfda2e405bd93b0d0338898ebdf287a4524f6f6a`.

Guards (unchanged, G1–G5, D10-3 A): `scripts/cellos_sign/signing.py` `ba97129308e60c50d59ff669831ab47d344b3ccbf44e8f17fbf052c9666c9511`; `scripts/cellos-sign` `005bb64c653e7328151c2275690f6dd48a89a05eb61876ec3499b4f5bbddd7f0`; `scripts/assemble-x86-image.py` `b74cc99787ab69dde04dac5c517acb6f771016043c23841bc7b40ab0b3bec7ed`; `scripts/package-desktop-x86.py` `d80517a8517f4e7689828ec89c663d88c0c7a51b29711621fd2b7b0350c4f4cd`; `kernel/build.rs` `c46adb3053a91e3513f525017f4cb69601a87c882f2e8195951c09a0518cc22b`.

Protected, never touched (locked or user-owned): `kernel/src/main.rs` `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64`; `kernel/src/hypervisor/svm_registry.rs` `e887e6acfa4e50dc049db9124866a9bee7f0938686725d31a44f89ccf323a44c`; `hal/arch/x86/src/x86_64/vmx.rs` `f4a0301c6ba4710c90c752b562c4b8d8878709e031fbd134c51e44fcc7b92b2b`; `kernel/Cargo.toml` `b71f1ba41c72b36b6295da3c10aef73479ccfc5ec9b2a53b40079cfa80669f27`; `kernel/src/loader/launch_profile/targets.rs` `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f`; `…/profiles.rs` `78058780bad2549c67ab7487b2a1e741d52580cbbdd27d1fccef132a69aa2930`; `cells/services/hypervisor/src/boot_x86.rs` `0c89aae7748fa7fa464b75686258ae9ca3fd3a94ef1debee15e44f73bbe284dd`; `…/run_loop_x86.rs` `5328b35d70d05096e9557b06b517e8b26357a3e2ef5631739b20403da1d29351`; `…/boot_x86_profile.rs` `a13fe3694dbed741f6c9eb8643bf61b19d5ab0f340188bc772b359de3661714a`; `…/net_backend.rs` `fcee93dec0d901238d0dabcb2569ec44cea5ddb7c33934b3b833d4e0b908665a`; `…/virtio_net.rs` `0be30929d5cdbbc9c437e8e188085406adf647a6461374ca44aef11fc7aa6071`; `…/uart_16550.rs` `ca7d2546e101bc99c78626b87a2a75ddfd829dda9dafd8d0bf7f0056ece4b04b`; `cells/services/hypervisor/Cargo.toml` `d6a1f36c6f69f0b5f13ed2588591a69ce4ef26dcd94489ac68b32669008553be`; `cells/services/net/src/interface.rs` `08d0a7a8742248396380c1aeed045cc5580294bb3ceba4fa508750f995233684`; `…/handlers.rs` `835d09b2bdacc60822b148240d8556f22fa6e55caf5648d3088f9d9bad02f280`; `cells/tools/init/src/main.rs` `b28083e0b399c8dba8719152f2eb64bf3e6eb6512c0450d00fb1e0d94732e6b0`; `cells/tools/cellbox/Cargo.toml` `9a130f44e4fc35b3b222b5dbbc2e3f80d24605965ed2862128f6001b93a878fd`; `Cargo.toml` `a325c8b2d9b5badafaac9f80bb56a34430e4b77245782b6f9288296a9abfcd20`; `Cargo.lock` `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130`; `.gitignore` `de60a4152bd81c0414804bff489acba769d1a5688324059cabf78f616fb3131d`. Also untouched: `kernel/src/embedded-x86_64/**`, default `target/x86_64-unknown-none/release/cellos-kernel`, `build/**`, `.alpine-cache-x86/**`.

Protected-path manifest: before T3-A starts and after every run, the executing agent records the tree digest `git ls-files --cached --others --exclude-standard DIR | LC_ALL=C sort | xargs sha256sum | sha256sum` for `kernel`, `hal`, `libs`, `cells/services`, `cells/tools`, `config`, `scripts/cellos_sign`, `scripts/x86`, `tools`, and compares. Only the closure above may differ.

## 7. Proposed acceptance wording (recorded at T3-E, only for outcomes the judge passed)

- **CBX-23** (append to the row; status word changes only per D-T3-8): "**S10-11 (date, user CP2, ceiling reduction):** the guest half is met at the SVM/TCG ceiling: password SSH from a Cellos cell (`cellbox-ssh`) to OpenSSH `<version>` inside a Linux guest on the AMD SVM VMM, QEMU-TCG 10.2.0, route = one loopback-only `hostfwd` to the static guest address, lane `qemu-tier3-ssh-guest-x86.sh` (fixture kernel features `phase02-terminal-profile,phase04-driver-profile`, driver feature `guest`, lane-only hypervisor/driver bytes), PTY, resize, exec bytes/stderr/status, stdin EOF, signal, producer stops on consumer close all PASS (`reports/t3-d-evidence/`). Not Intel VMX/EPT, not KVM, not matched-candidate bytes, not production; the S9-3 pipeline-exec and exact non-UTF-8 stdin limitation is unchanged."
- **CBX-25** (status becomes `met at stated ceiling` only if D-T3-8 = A): "VM endpoint route/server/credentials proven at the same ceiling (route, guest fingerprint equal to the pinned key, sshd version and log lines, password accepted, wrong password and changed key rejected on the guest); uncertain dispatched work on the guest is reported `unknown` and not replayed (one ledger line); no inbound Cellos sshd in the image (FAT layout and boot config), the only sshd is inside the nested guest."
- Phase 10 criterion 1 (`phase-10-qualification.md:31`): add the list of which of CBX-23/24/25 are met at which ceiling. **Whether it is checked is not decided here**: it also depends on the CBX-24 overall status (still `partial` after the KSSH T0/T1 dispositions) and the retained S9-3 CBX-23 limitation (D-T3-8).

## 8. What runs, is generated, or is written outside the repository

| Item | Where | Notes |
|---|---|---|
| Work directory | `/tmp/cellbox-tier3-<id>` (owned, not a symlink; same check as the Phase 09 lane) | everything below lives here and is removed after evidence capture |
| Network, **authorization A-NET** | `dl-cdn.alpinelinux.org`, HTTPS GET of `APKINDEX.tar.gz` and the OpenSSH `.apk` files only, once (T3-A), into `$W/apk-fetch` | No other host, no crates, no registry, no Docker pull, no inbound connection, no rustup. Cargo builds `--locked --offline`; if a build needs the network it is a STOP. |
| Host listener | QEMU's hostfwd on `127.0.0.1:P` (one port, random free port) | loopback only; exists only while QEMU runs; QEMU is killed at the end and on any STOP |
| Key material (A-KEY) | per-run ed25519 guest host key (`ssh-keygen`, mode 600), password (`head -c 18 /dev/urandom \| base64`), cell-signing seed (`os.urandom`, as `qemu-phase09-ssh-test.sh:221-226`) | `$W/keys`; embedded in the private initramfs/image; the judge scans every log and archive for them; deleted after capture |
| Processes | `qemu-system-x86_64` 10.2.0 (TCG), `ssh-keyscan`, `cargo`, `objcopy`, `xorriso`, `mcopy`, `python3` | no Docker, no KVM, no other QEMU running (precondition checked) |
| Disk | cargo target dirs for cells and kernel under `$W`, ISO, trust-fixture disk, initramfs | several GiB; `/tmp` has 692 GiB free |
| Repository writes | only §6.1 and §6.2; `.alpine-cache-x86/` read-only; nothing under `build/` or `kernel/src/embedded-*` | no commit, no push |

## 9. Locks, Law-1, protected paths: statement and handoff text

- **Law-1:** no ABI, syscall, capability, launch edge or policy row is added or changed; the init/shell launch edges and policy rows for `/bin/hypervisor` and `/bin/cellbox-ssh` exist (survey §1.2). No Law-1 confirmation is requested.
- **Locked paths touched:** none (no file under `kernel/**`, `hal/**`, `libs/**`, `cells/services/{hypervisor,net,vfs,kms}/**`, `cells/tools/{init,cellbox}/**`). The edited driver files belong to the Cellbox private test cell.
- **Owner acknowledgement requested at CP1 (text for `cp1-approval.md`):** "The accountable owner (the user) acknowledges that S10-11 boots and exercises the already-built AMD SVM Tier-3 VMM under QEMU-TCG, edits no file under the scope of relations L11 (`260711-1917-tier3b-x86-vtx`), L13 (`260712-0952-tier3b-vm-hardening-compat`), L18 (`260722-2330-tier3b-finish-g5-lite`), L25, L32, L36 or L37, consumes completed outputs of `260827-1004-hardware-independent-roadmap` only as evidence, does not execute, adopt or narrow any locked plan, and does not change Intel VMX/EPT P09 status. The reading that a new private lane consuming the built SVM VMM needs no six-step unfreeze (`cell-contract-0024-locks.md` procedure, steps 1-6) is accepted for this slice only."
- **Contingent handoff (not requested now; template only).** If a STOP reveals a defect whose repair needs a source edit under `cells/services/hypervisor/**` or `kernel/src/hypervisor/**` (L11/L13/L18) or `cells/services/net/**` (adjacent L37/L32), it becomes a separate slice with its own CP1, named file-owner handoff and file-bounded exception following steps 3-5 of the unfreeze procedure, recording: "Slice: `<name>`. Files: `<exact list>`. Owner adoption: the accountable owner adopts the slice into the controlling plan `261011-1200-cell-contract-authority`. Remaining locked scope: all other scope of L11/L13/L18 unchanged. Acceptance ceiling: SVM/TCG regression-only." No such handoff is part of this CP1.

## 10. Decision table

| ID | Question | Options | Recommended |
|---|---|---|---|
| **D-T3-1** | Accept the ceiling statement of §1 as the recorded reduction for CBX-23 guest half and CBX-25 | **A** accept; **B** edit wording; **C** do not proceed | **A** |
| **D-T3-2** | Where does sshd come from | **A** Alpine v3.21 OpenSSH apks fetched once, sha256-pinned, installed offline; **B** Dropbear from the same repository; **C** `apk add` at guest run time over the nested NIC; **D** Docker export of the fixture image | **A** (retained interop target; offline at lane time; every input hashed) |
| **D-T3-3** | Network authorization A-NET (§8) | **A** one HTTPS GET session to `dl-cdn.alpinelinux.org` for the apk set, scratch only; **B** no network (blocks T3-A and the whole program) | **A** |
| **D-T3-4** | Route | **A** R1 hostfwd loop with the STOP-3/4 rules; **B** R2 Net-cell hairpin (new slice, locked-adjacent, not authorized here); **C** do not proceed | **A** |
| **D-T3-5** | Evidence class | **A** fresh lane-only build at HEAD, no candidate import (L5f-like), no changes to `phase10_candidate_import.py`; **B** extend the importer so `cellbox`/`net`/… are payload-matched to candidate `4fae8b20…` (edits `phase10_candidate_import.py` and its tests) | **A** (honest class "lane-only"; B is a larger change for little claim) |
| **D-T3-6** | Scenario scope | **A** core set of §3 (reused CBX-23 scenarios plus four guest scenarios); **B** also the peer-rekey case with a second hostfwd and second sshd; **C** also KMS publickey to the guest | **A** (the user chose password only; rekey is CBX-24 territory) |
| **D-T3-7** | Run budget and caps of §5 (2+2 runs, 4 h cap, no fix-and-rerun after a marker FAIL) | **A** accept; **B** one run per stage; **C** more | **A** |
| **D-T3-8** | Row dispositions at T3-E | **A** CBX-25 `met at stated ceiling`; CBX-23 guest half met at ceiling but row stays `partial` until the retained S9-3 pipeline-exec/non-UTF-8-stdin limitation is dispositioned; Phase 10 criterion 1 stays open pending CBX-24; **B** additionally disposition that S9-3 limitation as retained-at-ceiling so CBX-23 is `met at stated ceiling`; criterion 1 still depends on CBX-24; **C** record evidence only, no status words | **A** (no hidden promise; the user can add B at CP2) |
| **D-T3-9** | Lock reading and the acknowledgement text of §9 | **A** accept; **B** require a formal slice adoption into the controller plan first | **A** |
| **D-T3-10** | `command-catalog.md` "excluding Tier-3 guest" wording | **A** leave unchanged (digest-bound, acknowledged at S10-5 CP2) and let the acceptance row supersede it; **B** edit it and request a new digest acknowledgement | **A** |
| **D-T3-11** | On STOP | **A** record a no-go dossier, leave rows as they are, close the plan; **B** keep the plan open for R2 | **A** |

## 11. Not authorized by this draft

Any edit, build, fetch, QEMU run or key generation before CP1 approval; Intel VMX/KVM runs; edits to any file in §6.3 or any protected path; a new signing route or guard change; a Net-cell, hypervisor, kernel or `libs` change; virtio-rng; reopening `/bin/sshd` or the quarantined bytes; a Docker fixture, host localhost server or labelled container standing in for the guest; publickey to the guest; pushing or committing; status words beyond §7.

## 12. Known unknowns that can end the program (restated)

The VMM may not boot at HEAD (STOP-1). libslirp may not forward the loop (STOP-3/4). Nested TCG may be too slow for per-case deadlines even after raising them (recorded as a failed gate, not silently stretched past the 4 h cap). The guest's entropy stall (no virtio-rng) may exceed the wait. The exact OpenSSH apk set and its hashes are unknown until A-NET. None of these is hidden or worked around by a substitute.
