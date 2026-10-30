# Tier-3 guest endpoint on the SVM/TCG VMM: read-only survey

Status: survey for the CP1 draft in [cp1-draft.md](cp1-draft.md). Documents only.

Basis: HEAD `12f0b9d19`, working tree clean when read. Tools used: `read`, `grep`, `sha256sum`, `git log/show/stat`, `ls`, `command -v`, and one Python parse of the cached initramfs (read-only). **Not done:** no source edit, build, QEMU, Docker, network, key generation or commit. Anything not observed in a file or a command output is marked `[INFERENCE]`.

The ceiling the user chose (2026-10-30, probe go/no-go item 5 with items 3, 4, 6): **guest endpoint on the existing SVM/TCG VMM, not Intel VMX qualification, password credential only, route from a Cellos cell.** Probe: [`reports/s10-10-evidence/probe/tier3-and-guards.md`](../261020-1750-cellbox-cli/reports/s10-10-evidence/probe/tier3-and-guards.md) (sha256 `417b324b4ee7608ac43236be35b861a7d8f2e123fe7f11ae992594810ed27e35`).

## 0. Verdict table

| Sub-item | Verdict | One-line reason |
|---|---|---|
| Guest VMM on SVM/TCG exists | **Exists, unverified at HEAD** | Code, scripts, CI jobs and a Sep-29 retained serial log exist; nothing in the repo shows a boot-to-shell at HEAD, and four relevant commits landed after the last retained run (§1.4). |
| Guest image with sshd | **No image; recipe is reachable** | Cached Alpine kernel/initramfs match their pins; the initramfs already holds musl, libcrypto 3 and zlib but **no sshd**. One authorized fetch of Alpine apks is the only missing input (§2). |
| Route Cellos cell → guest sshd | **Feasible only through QEMU `hostfwd` looped via host loopback; unproven** | The Net service has no internal path between its own stack and the guest port (§3.2). The loop path needs no source edit but rests on libslirp behaviour nobody has exercised (§3.3). |
| Password credential | **GO** | Client supports it; no KMS needed (§3.4). |
| Acceptance at the reduced ceiling | **Reachable on paper; rows need explicit ceiling wording** | §4. |
| Locks / Law-1 / protected paths | **No locked or protected path needs an edit** if the route is the hostfwd loop and the run reveals no VMM defect (§5). |
| Overall | **Conditional GO** to a CP1 whose first slice is a boot/route experiment with hard stop rules | §6 lists what could turn it into NO-GO. |

## 1. What exists for a Tier-3 guest on the SVM/TCG path

### 1.1 Components

| Piece | File:lines | Role (read) |
|---|---|---|
| Kernel VM store (SVM) | `kernel/src/hypervisor.rs:8-18`, `kernel/src/hypervisor/svm_registry.rs` (wraps SVM and, since `15830f4fd`, VMX vCPUs) | x86 VM registry; SVM is the path under QEMU `qemu64` TCG. Kernel log `x86 virt: root operation active (SVM)` at `kernel/src/main.rs:635`. |
| Hypervisor cell, x86 personality | `cells/services/hypervisor/src/boot_x86.rs`, `run_loop_x86.rs:60-305`, `loader_image_x86.rs`, `boot_x86_profile.rs` | `create_vm` → map 128 MiB → parse `/vmlinux` PVH note → stream `/initrd.gz` → `[hv-x86] vCPU ready — entering run loop` (`boot_x86.rs:175`) → exit loop. |
| Device model | `uart_16550.rs`, `pic_8259.rs`, `pit_8253.rs`, `cmos_rtc.rs`, `virtio_mmio.rs`, `virtio_blk.rs`, `virtio_net.rs`, `virtio_console.rs`, `virtio_gpu*`, `virtio_input.rs` | 16550 on COM1 `0x3F8`; virtio-mmio block `0xd0000000`/IRQ5 and net `0xd0000200`/IRQ6 (`.agents/260827-1004-hardware-independent-roadmap/phase-10-tier3-x86-virtio-parity.md:68`); PIT, PIC, CMOS RTC. No virtio-rng device (see §6 R-5). |
| Guest console | `uart_16550.rs:48` | Every TX line is printed on the host console with the prefix `[guest-uart] ` (so guest markers are visible and attributable on the host serial log). RX comes from the kernel UART ring through the cell's `Read` syscall (`main.rs` syscall list; `uart_16550.rs:1-11`). |
| Guest NIC backend | `cells/services/hypervisor/src/net_backend.rs`, `virtio_net.rs:13-40` | Guest TX frame → `NetRequest::L2Send` to the Net cell; RX polled with `NetRequest::L2Recv`. `GUEST_MAC = 52:54:00:AA:BB:CC` (`virtio_net.rs:13`), distinct from the QEMU e1000 default. Modern 12-byte `virtio_net_hdr_v1` stripped/prepended. |
| Net-side bridge | `cells/services/net/src/handlers.rs:200-233`, `interface.rs:333-345,376,576-609` | `L2Send` learns the guest MAC from the frame source and queues the frame to the NIC driver; `route_rx` queues a received frame to the guest only if dst MAC = learned guest MAC (or broadcast, which also goes to smoltcp), otherwise to smoltcp. Admission since `2f0bc0659` by kernel-attested flag `CALLER_FLAG_NET_L2` (`rights.rs:16,42,60`). |
| On-demand launch | `cells/tools/cellbox/src/executor.rs:2740-2784` (`hv`) | Shell `hv` spawns `/bin/hypervisor` by Path edge. |
| Boot-time launch | `config/autoload.toml:16-20` (`hypervisor`, `service_id = 14`, `restart = "always"`); `scripts/generate-boot-config.py:63-64` | The row is emitted only when the boot-config features include `tier3-autostart` or `hv-autostart`. `cells/tools/init/src/main.rs:12-18,142` makes `tier3-autostart` a compile guard; the plan, not code, decides what starts. |

### 1.2 Launch edges, policy rows, authority

- **Launch edge exists, no new row needed.** Init edge: `/bin/hypervisor` is in the reviewed list at `kernel/src/loader/launch_profile/profiles.rs:26`. Shell edge: `targets.rs:94-99` grants exactly `hypervisor + net_l2`; test `launch_profile/tests.rs:51-63` pins it. Both are in the default kernel, not behind fixture features.
- **Policy row exists.** `scripts/sign-policy.py:139` `("/bin/hypervisor", 0,0,0,1, … *NET_L2)`; `NET_L2` = `(0,0,0,1)` (`:116`). The ssh client's row is `("/bin/cellbox-ssh", … *NET_CLIENT)` (`:145`), `NET_CLIENT` = resolve + connect (`:113`). A lane that bakes no `/POLICY.BIN` takes the kernel's `Absent` (dev-permissive) branch (`scripts/make-hypervisor-fs-x86.sh:121-123`).
- **Consequence:** `[INFERENCE]` no kernel, launch-profile, policy or Law-1 file needs editing to run a guest next to `cellbox-ssh`; the survey found no row that is missing.

### 1.3 How a guest is launched and what it gets

- Guest RAM 128 MiB (`boot_x86_profile.rs:6-11`); PVH entry from the `vmlinux` ELF note; initramfs `/initrd.gz` placed at a 2 MiB-aligned GPA (`boot_x86.rs:17,76+`). Both files are read through `sys_open_cap` from the **kernel's embedded FAT image** (`boot_x86.rs:35-52`), so they must be inside `kernel_fs.img`.
- Default cmdline (`boot_x86_profile.rs:19-20`): `console=ttyS0 … rdinit=/bin/sh … virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 -- -i`. If the image carries a file `/virtio-e2e`, the cmdline instead selects `rdinit=/bin/virtio-e2e-init` (`:30-37`). That selector is how every existing in-guest workload gets its own init (E2E, hostile, nginx, python gates).
- The hypervisor image must not depend on a VFS-backed disk unless wanted: the default is a persistent `/mnt/sd/guest_disk.img` that **fails closed** if it cannot be opened; `service-hypervisor/volatile-disk` selects the no-drive policy (`scripts/lib-make-hypervisor-fs.sh` header text; `make-hypervisor-fs-x86.sh:39-41`; CI sets `HV_VOLATILE_DISK=1`).
- Headless is fine for the GPU/input personalities: `scanout.rs:27-29,86-88` return early when the compositor TID is 0. `[INFERENCE]` unverified in a headless image.

### 1.4 Does it work under QEMU-TCG on this machine?

| Evidence | What it shows | Date / limits |
|---|---|---|
| `build/tier3-hostile-current/qemu.log` (local, gitignored) | SVM root active (`:55`), `[hv-x86] hypervisor cell starting (SVM PVH)`, `guest rdinit: /bin/virtio-e2e-init`, `vCPU ready — entering run loop`, virtio block and net probes, `[net-bridge] first e1000 TX/RX` | 2026-09-29. Not rerun at HEAD. |
| Phase 10 parity plan (`phase-10-tier3-x86-virtio-parity.md:98-131`) | strict boot to `/bin/sh`; 27 hostile scenarios and two-boot persistence passed on pinned QEMU-TCG 10.2.0; shared net backend with distinct nested MAC | "regression-only at the QEMU ceiling"; ARM64 blocked; no physical claim. Plan status `completed`, but its parent plan is in L32 (§5). |
| CI `qemu-x86-hypervisor-boot` (`ci.yml:1036-1117`) | builds with `make-hypervisor-fs-x86.sh`, `-cpu qemu64,+pdpe1gb,+svm`, 1G, `BOOT_WINDOW=900`, asserts the `/ #` prompt | The recipe needs a signed `cellbox`; G2a refuses it (probe §(d)). `[INFERENCE]` this job cannot currently pass; not run. Also jobs `qemu-x86-tier3-python`, `-virtio-e2e`, `-hostile` (`:1143-1414`). |
| Pinned QEMU | `~/.cache/cellos/qemu-10.2.0/bin/qemu-system-x86_64` present (sha256 `169fb4ea…1241fe`, 82,896,680 B); `/usr/bin/qemu-system-x86_64` is 8.2.2, which `qemu-hypervisor-smoke-x86.sh:73-76` rejects (the smoke requires exactly 10.2.0) | Host: 27 CPUs, 39 GiB RAM, `/dev/kvm` exists but SVM-under-KVM is Intel-incompatible (`qemu-hypervisor-smoke-x86.sh:24-27`); TCG only. |
| Open fact | `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted` in the Oct-12 pinned log (`.agents/261011-1200-cell-contract-authority/plan.md:224`, `measurement-readiness.md:78`) | A kernel smoke under `test-hooks`, not the VMM boot lane. Retained as an unexplained fact; `[INFERENCE]` not decisive for the VMM boot. |

**Changes after the last retained guest run (2026-09-29; `build/vicell-x86-hv-ondemand.iso` 2026-10-03):** `c01f3375d` (10-08, L2 RPC queued, RX gated on posted buffers, byte-wise virtio config reads), `c88008d8f` (10-09, kernel-owned RPC cutover), `2f0bc0659` (10-29, net admission by `CALLER_FLAG_NET_L2`, `rights.rs` +143 lines, runtime-admission tests +275), `15830f4fd` (10-29, VMX backend; `kernel/src/main.rs` rewrote the VMX branch, SVM branch lines `:625-640` unchanged in shape). **Therefore the guest NIC path and the VMM itself have no evidence at HEAD.** The first deliverable of any CP1 must be a boot proof, not an SSH proof.

### 1.5 Is `/bin/hypervisor` in any image?

- Canonical x86 recipe `scripts/assemble-x86-image.py` has no `hypervisor` or `vm-runner` cell (probe a3; hash of the recipe is pinned by G2a at `b74cc997…`). `[INFERENCE]` the promoted desktop image lacks `/bin/hypervisor`; **not** checked by listing the FAT image. The candidate import lane (`scripts/phase10_candidate_import.py`) therefore cannot import it: it would be a lane-only binary, built locally.
- Existing hypervisor image recipe: `scripts/make-hypervisor-fs-x86.sh` (fetch → build cells PIE → sign → mkfat32 with `/vmlinux`, `/initrd.gz`, `/POLICY.BIN`); signing via `lib-sign-cells.sh` is stopped by G2a (dev key + `cellbox`). A private-fixture lane (`CELLBOX_PRIVATE_FIXTURE_SIGNING=1`, explicit random seed, fixture kernel with `CELLOS_PHASE02_CELL_PUBLIC_KEY`) is the existing, accepted way around this (`scripts/qemu-phase09-ssh-test.sh:329-331,448-451`); the guards stay closed.
- The kernel embeds the whole image (`EMBEDDED_OVERRIDE`): existing hypervisor images are ~60 MiB (`kernel/src/embedded-hv-x86/kernel_fs.img`, 62,914,560 B, 2026-10-03, gitignored).

### 1.6 Phase 10 tier3-x86-virtio-parity plan (what it settles and what it does not)

Settles: one virtio-mmio map (block `0xd0000000`/IRQ5, net `0xd0000200`/IRQ6), shared net backend, distinct nested MAC, 128 MiB Alpine PVH guest, QEMU-TCG 10.2.0 only, hostile corpus, persistence (`:68-77,98-131`). Does not settle: any inbound path to the guest (every retained network evidence is guest → outside: DHCP/`apk`/HTTPS in the nginx/python gates), a guest sshd, virtio-rng, or HEAD behaviour. The nginx gate (`tests/guests/x86-nginx/guest-init.sh`) is the closest recipe: static `10.0.2.15/24`, gateway `10.0.2.2`, `apk add` from the CDN over the nested NIC, markers `NGINX_IN_VM_*`.

## 2. Guest image

### 2.1 Existing recipes (none builds sshd)

| Recipe | Output | Network at build/run | sshd? |
|---|---|---|---|
| `scripts/fetch-alpine-x86.sh` | `.alpine-cache-x86/{vmlinuz-virt,initramfs-virt,vmlinux}` | Build: CDN `dl-cdn.alpinelinux.org` for the two artifacts (skipped when the cached sha256 matches the pin, `:29-45` elided, `[INFERENCE]` from "re-runs work offline" comment `:49-50`); `extract-vmlinux` pinned `:57-58` | no |
| `scripts/prepare-x86-app-initramfs.sh` + `tools/repack-initramfs.py` | repacked initramfs with `/bin/virtio-e2e-init` from a fixture script, optional module pruning | none | no |
| `scripts/prepare-tier3-hostile-initramfs-x86.sh` | adds a probe binary and `bin/sh` | none | no |
| `scripts/build-linux-base-image.sh` | Alpine 3.21 ext4 rootfs (cage, mesa…), no kernel | CDN | no (`debugfs` finds no sshd/dropbear in `build/linux-base.ext4`, probe a4) |
| `scripts/build-ubuntu-wide-guest-x86.sh` | 512 MiB Ubuntu cloud rootfs on `/dev/vda` | cloud-image download | `[INFERENCE]` ships openssh-server, nothing enables or keys it; no artifact built |

### 2.2 Local cache (read; sha256 computed)

| Path | sha256 | Matches pin? |
|---|---|---|
| `.alpine-cache-x86/vmlinuz-virt` | `26bf81ada3e8fc30fd4d81805fe6c8c60be5c7fb18a43563c707e49117e624ca` | yes (`fetch-alpine-x86.sh:24`) |
| `.alpine-cache-x86/initramfs-virt` | `e2562e019a506f9bdac24d06953823106a2ab29da50eea01185d005a3ca4acdf` | yes (`:25`); byte-equal to `initramfs-virt.orig` |
| `.alpine-cache-x86/vmlinux` | `2d62d53b39ed16f5d24a89a39abebcf946d9a0f9482734baf3da51e867b1ec07` | derived by `extract-vmlinux`; no pin in the script |
| `scripts/.extract-vmlinux` | `97cfeeeb51de17f4b5928c5442b56e5581314ddef3cedf2523be2049d79394af` | yes (`:58`) |

`.alpine-cache-x86/` is gitignored (`.gitignore:12`). Alpine release 3.21.7 (`fetch-alpine-x86.sh:18`; `[INFERENCE]` kernel 6.12.81-0-virt from the module path inside the initramfs, which lists `lib/modules/6.12.81-0-virt`). So **kernel and initramfs need no download**.

### 2.3 What the cached initramfs contains (parsed read-only: 286 entries, 27,521,744 B unpacked)

- Present: `bin/busybox` (808,712 B; applets include `adduser`, `chpasswd`, `ip`, `ifconfig`, `udhcpc`, `timeout`, `stty`, `setsid`, `nc`, `dd`, `wget`, `modprobe`, `mdev`), `sbin/apk` + `usr/lib/libapk.so.2.14.0`, five Alpine release keys under `etc/apk/keys/`, CA bundle, `lib/ld-musl-x86_64.so.1`, `usr/lib/libcrypto.so.3` (4,498,768 B), `usr/lib/libssl.so.3`, `usr/lib/libz.so.1.3.2`, `virtio_net.ko`.
- Absent: any `ssh*`, `dropbear*`, `etc/ssh`, `/var/empty`, `sshd` user, `login`-independent password database beyond busybox `/etc/passwd` (115 B).
- Consequence: an OpenSSH server needs only the OpenSSH binaries and config on top of libraries that are already there; `apk` can verify signatures against the keys that ship in the initramfs.

### 2.4 Options for getting an sshd in

| # | Option | What must be fetched / authorization | Pin strength | Verdict |
|---|---|---|---|---|
| A | **OpenSSH from the Alpine v3.21 `main` repository, staged as `.apk` files in the initramfs, installed offline by `apk add --no-network --repository /apks`** (packages `openssh-server`, `openssh-server-common`, `openssh-keygen`, plus whatever `apk` resolves for `libcrypto3`/`zlib`/`musl` at the same 3.21.x level; `openssh-sftp-server` not needed) | One authorized read-only fetch from `dl-cdn.alpinelinux.org` of `APKINDEX.tar.gz` and about 4–6 `.apk` files, a few MiB. The exact file names, versions and sha256 are **unknown until fetched** `[INFERENCE]`. | sha256 per file recorded at fetch time and pinned in the recipe; `APKINDEX` verified by `apk` against the Alpine keys already inside the initramfs; versions match the retained Docker fixture's family (OpenSSH from Alpine) | **Recommended.** Matches the retained interop target; offline at lane time; every input hash-pinned. |
| B | Dropbear (`dropbear` + `zlib` + `musl` apks) | same kind of fetch, 1–2 files | same | Lighter in a 128 MiB guest, but Sunset (the client) has only ever been interoperated with OpenSSH (`phase09-ssh-client.md` S9-3, K8 `OpenSSH_9.9p2`); `dropbearconvert`/key-format step needed `[INFERENCE]`. Fallback only. |
| C | Runtime `apk add openssh-server` in the guest over the nested NIC (nginx-gate style) | Internet at **lane run time**, DNS+TLS from inside the nested guest; guest clock set from build time | weakest: repository state at run time, no recorded hashes, 181–411 s TLS in observed python-gate runs (`docs/project-changelog.md:936`, `[INFERENCE]` same family of timing) | Rejected: not hash-pinned, makes the lane flaky, adds a second network dependency. |
| D | Docker export of the retained fixture image `cellbox-p09-sshd:<recipe-sha>` (Alpine `alpine@sha256:ce647…d3507` + `openssh-server`) | Docker daemon (not authorized in this assignment); the image's presence in the local store was **not queried** | strong (digest) | Not recommended: libs would be a different Alpine point release than the initramfs' libcrypto/musl; needs Docker; ABI mismatch risk. |
| E | Ubuntu cloud-image rootfs profile (`HV_GUEST_PROFILE=ubuntu`) | ~hundreds of MiB download, 512 MiB guest, systemd | weak | Rejected: size, time under nested TCG, nothing configures sshd. |
| F | Build OpenSSH/Dropbear from source | source tarball fetch + a musl cross toolchain (none present: no `musl-gcc` checked; `[INFERENCE]`) | strong if pinned | Rejected: new toolchain admission. |

Host side already present: `ssh-keygen` (`/usr/bin/ssh-keygen`), `openssl`, `python3` 3.x with `cryptography` 41.0.7, `xorriso`, `mcopy`, `objcopy`, `cargo`. No `mkpasswd`; the guest sets the password with busybox `chpasswd`. No sshd on the host (`command -v sshd` empty) and none is needed.

### 2.5 What the guest init must do (derived from the nginx gate and the fixture)

`[INFERENCE]` for every item until run: mount proc/sys/dev, `modprobe virtio_net`, wait for CRNG (`head -c 32 /dev/random` with a timeout, as the nginx gate does, because there is no virtio-rng; see §6 R-5), bring `eth0` up with a **static `10.0.2.16/24`, gateway `10.0.2.2`** (not `.15`, which the outer Net cell's DHCP lease takes), create user `tester` and set a per-run password with `chpasswd`, install the pinned apks offline, drop the pre-generated ed25519 host key (mode 600), write an `sshd_config` that mirrors the fixture (`phase09_ssh_fixture.sh:85-98`: ed25519 only, `curve25519-sha256`, `chacha20-poly1305@openssh.com`, password on, root login off, no forwarding, `PerSourcePenalties no`), run `sshd -D -e` so its log lines come out on the console as `[guest-uart]` lines, and print markers. A small `p9` helper like the fixture's (ledger line per execution) provides the no-replay witness.

Memory: 128 MiB guest, ~27.5 MiB unpacked base initramfs plus apks; the app gates prune unused kernel modules (`prepare-x86-app-initramfs.sh` `--prune-modules-to`) for the same reason (python needed 256 MiB `alpine-wide`; sshd is much smaller). `[INFERENCE]` fits.

## 3. Route from a Cellos cell to the guest sshd

### 3.1 Actual topology (no ambiguity)

```
host Linux
 └─ QEMU 10.2.0 (TCG, q35) with ONE user-mode (libslirp) netdev + e1000 NIC
     └─ outer Cellos kernel  (SVM root)
         ├─ e1000 driver cell ── L2 frames ── Net cell  (smoltcp stack, DHCP lease 10.0.2.15 `[INFERENCE]`)
         │                                   ├─ cellbox-ssh  (NET_CLIENT: resolve+connect only)
         │                                   └─ L2 bridge ── hypervisor cell ── virtio-mmio net 0xd0000200
         └─ hypervisor cell ── SVM vCPU ── nested Alpine guest (static 10.0.2.16, sshd :22)
```

The guest NIC has no wire of its own: its frames go through the Net cell onto the one e1000 and out to slirp, and come back the same way. The existing Phase 09 lane has **one** netdev "without restrict and with no hostfwd/guestfwd: … nothing reaches the guest" (`qemu-phase09-ssh-test.sh:5-7`; the script aborts if any argv element contains `fwd`, `:521-522`).

### 3.2 Why a cell cannot reach the guest inside Cellos

Read: `route_rx` (`interface.rs:576-609`) sends a frame to the guest queue only when it **arrived from the NIC**; `handlers.rs:200-209` + `interface.rs:376-396` sends guest TX frames to the NIC driver. There is no code that delivers a guest TX frame to the smoltcp stack or a smoltcp TX frame to the guest queue. A cellbox connect to `10.0.2.16` would make smoltcp ARP/SYN out through the NIC; the frame leaves to slirp, and `[INFERENCE]` libslirp does not forward frames between two addresses on its own subnet (it terminates TCP for the gateway/host/DNS and for forwarded ports), so nothing returns. Hence the hairpin must be provided either outside (QEMU) or inside (Net-cell source).

### 3.3 Options

| # | Route | Source edits | Feasibility | Risks |
|---|---|---|---|---|
| R1 | **QEMU `hostfwd=tcp:127.0.0.1:P-10.0.2.16:22` on the single user netdev; cellbox connects to `10.0.2.2:P`** (slirp maps `10.0.2.2` to host loopback, where QEMU itself listens on `P`; the forwarded connection is then initiated by slirp toward `10.0.2.16` through the NIC, ARP answered by the nested guest via the Net-cell bridge) | none in the repo; one new QEMU argv in a **new** lane script | Plausible. QEMU documents `hostfwd=[tcp]:[hostaddr]:hostport-[guestaddr]:guestport`. The ARP broadcast reaches both smoltcp and the guest (`route_rx` broadcast arm), unicast replies go to the learned guest MAC. | (a) `[INFERENCE]` libslirp accepting a loopback connection to its own listener and then ARPing a non-DHCP-leased guest address: untested here. (b) The static guest IP must not collide with the outer lease. (c) Host-side port must be loopback-only; hostfwd listens on `127.0.0.1` only when asked. (d) The existing "no forwards" invariant belongs to the old lane; the new lane needs its own, narrower invariant (exactly one hostfwd, `127.0.0.1`, one guest address). (e) Connection path is host-mediated, so the evidence says "route via QEMU user networking with one loopback forward", not "Cellos routed it". |
| R2 | Net-cell hairpin: deliver guest TX frames whose dst IP/MAC belongs to the Cellos stack to smoltcp and vice versa | edit `cells/services/net/src/{interface,handlers}.rs` (+ tests) | Technically straightforward, honest route | Touches the Net service (adjacent to L37/L32 device-authority scope; authority-relevant; protected under the Cellbox plan rules). Not allowed without its own CP1, Law-1 review and owner handoff. |
| R3 | Second QEMU netdev/NIC (e.g. `-netdev socket`) for the guest | Net cell has one NIC; drivers for two | No | Needs multi-NIC support and a net-cell change. |
| R4 | Host `ssh` to `127.0.0.1:P` | none | Works as a host-side **probe** only | Proves host→guest only; **not** a Cellos cell client, so it cannot satisfy CBX-23/25. Useful as the decisive first experiment (§3.5). |

Honest feasibility: **R1 is the only route that fits the chosen ceiling without touching a locked path, and it is unproven**. If R1 fails on the decisive experiment, the SVM/TCG guest endpoint is **infeasible without R2**, i.e. without a Net-cell source change that this program cannot make by itself. The CP1 therefore makes the route experiment the first gate and treats a failure as a stop with a no-go record.

### 3.4 Credentials and trust

- Password auth is supported (D79-6, `phase-09-ssh-client.md`); `--auth publickey` was later enabled at DEV_POSTURE by KSSH, but the user chose password only for this endpoint. No KMS needed.
- Host trust: the lane's trust-store builder `scripts/phase09_trust_disk.py DISK CONFIG_DIR HOST_KEY_PUB PORT [REKEY_PORT]` writes the real key for `10.0.2.2:PORT`, a wrong key for `host:PORT`, nothing for `gateway`. It is reusable unchanged with the guest's public key and the hostfwd ports. The driver reads `/p09/ssh.conf` = `PORT SHA256:FP PASSWORD [REKEY_PORT]` and hard-codes `10.0.2.2` in every command (`cells/tests/phase04-terminal-driver/src/ssh.rs:164-176,437`), so **a hostfwd route is the one that reuses the existing scenarios without changing their target**.
- The guest host key and the password are generated per run on the host (`ssh-keygen -t ed25519`, `head -c 18 /dev/urandom | base64`), embedded in the private initramfs under `/tmp`, scanned for in every retained log, and removed after capture, as the K8 mode (`qemu-phase09-ssh-test.sh:503,659-662`, `--extra-secret-file`) and the check `no-key-material-in-log` (`:815`) already do for the lane's secrets.

### 3.5 Decisive experiment, performance, and what slows it down

1. Boot-only: guest reaches `TIER3_SSHD_LISTENING` on the host serial log; host-side `ssh-keyscan -p P 127.0.0.1` returns the pinned fingerprint (R4: proves VMM + NIC bridge + sshd + hostfwd listener, no Cellos client).
2. Cell-side: the Phase 04 driver connects `cellbox ssh … 10.0.2.2 -p P` and the client prints the same fingerprint (R1 proper).
- Nested TCG cost `[INFERENCE]`: CI allows 900 s just for the guest to reach `/ #`; the python gate's TLS took 181–411 s; existing Phase 09 cases use 120 s per case and a 3600 s lane window for five boots on one level of TCG (`qemu-phase09-ssh-test.sh:113-115`). A guest-side OpenSSH handshake (ed25519 + curve25519) plus a Sunset handshake on the same TCG thread will be several times slower per case; budgets must be raised (Section 6 R-6).

## 4. Acceptance text and evidence each row needs

### 4.1 Row text (acceptance.md at HEAD)

- **CBX-23:** "SSH authenticates to controlled other-host and Tier-3 guest servers, supports PTY shell/resize and raw non-PTY stdin/stdout/stderr/remote exit status without JSON conversion; producer traffic stops on consumer close." Other-host half **met** (L5, canonical GUI SSH). Guest half **blocked**.
- **CBX-25:** "VM endpoint route/server/credentials are explicitly proven; uncertain dispatched remote work is not replayed; existing unsafe inbound sshd is not started or exposed as a test prerequisite." No-replay (lane-only L5f) and no-inbound (canonical) **PASS**; VM route/server/credentials **blocked**.
- Phase 09 criterion 4 (already checked) reads "VM route/server readiness is actual evidence; no Intel VMX, C2C authority, inbound server or production readiness is inferred" and its text names only "QEMU user netdev to a loopback-only fixture". `phase-09-ssh-client.md` Verification: "An unavailable guest route blocks that scenario explicitly; it cannot be substituted by host localhost or a printed guest label." Therefore R1 must be justified as a route **into the guest** (the endpoint process runs inside the nested VM; hostfwd is only the transport), and the host-side probe (R4) must **not** be presented as the proof.
- D79-2 (user, 2026-10-25): "does not substitute host localhost or a labelled container for the guest class". The new endpoint is a real VMM guest, so D79-2 is satisfied by construction; the ceiling reduction is VMX → SVM/TCG only.

### 4.2 Evidence per sub-claim (what the lane must produce)

| Claim | Evidence (new lane, at the reduced ceiling) |
|---|---|
| Guest is a Tier-3 VM guest on the SVM VMM | Host serial: `x86 virt: root operation active (SVM)`, `[hv-x86] vCPU ready`, `[guest-uart]` lines from a Linux guest with the pinned kernel; **sshd started by the guest init** (`Server listening on 0.0.0.0 port 22` as a `[guest-uart]` line), plus the pinned image hashes |
| Route proven | host-side: exact QEMU argv with one `hostfwd` bound to `127.0.0.1`; cell-side: driver marker `ssh-guest-route` with the fingerprint read by the Sunset client equal to the pinned key; sshd log line `Connection from 10.0.2.2 port N` (source seen by the guest is slirp's gateway) |
| Server proven | guest `sshd -V`/version line (as `fixture/sshd-version`), config hash, `Accepted password for tester` lines |
| Credentials proven | driver markers for correct password (`ssh-exec-bytes` etc.), wrong password rejected on the guest (`ssh-wrong-password`), changed host key rejected (`ssh-changed-host-key`), no secret in any log (`seed/password/host-key not in serial` check as `qemu-phase09-ssh-test.sh:815`) |
| CBX-23 PTY/exec | `ssh-pty-shell`, `ssh-pty-resize`, `ssh-pty-escape-disconnect`, `ssh-exec-bytes`, `ssh-exec-stdin-eof`, `ssh-exec-signal` (existing scenarios, `ssh.rs:278-348`), run against the guest |
| "producer traffic stops on consumer close" | a guest-side producer (`yes`) cancelled from the client; the guest ledger/process check shows it ended (new guest scenario; the existing `ssh-cancel-exec`/`ssh-backpressure-bytes` need Docker hooks and a host sink) |
| No replay on the guest | self-inflicted drop after dispatch (guest helper kills its own session process after writing one ledger line); client records `unknown`/exit 255 (`completion` record); a second login reads the ledger: exactly one line |
| No unsafe inbound sshd | FAT layout has no `sshd`; boot config has no `sshd` service row; no outer listener on 22; **the only sshd is inside the nested guest** (`cellos` `/bin/sshd` 197,048 B stays out; D10-7 A) |

### 4.3 Evidence-class rules that bind this lane (S10-3.3 of `phase-10-s0-proposal.md`)

- Fixture kernels (`phase02-terminal-profile`, `phase04-driver-profile`) "count **only** for the behaviour of payload-matched **cells**" and "never count for kernel-level claims". A cell run on a private fixture kernel is the existing L5 precedent. The hypervisor cell, the driver and `app-init` with a hypervisor autoload row are **lane-only bytes** (not in the candidate), like L5f's `p09-faults` cellbox: so the evidence class is "lane-only guest bytes + payload-matched shared cells (where imported)", never "matched-image evidence".
- "A CBX row that cites a lane must name the lane's fixture features."
- The user's earlier practice for limited results: "one run each", no fix-and-rerun without a new CP1, results recorded as at-ceiling dispositions (DS6-1 B, D10-1 C, S10-9 CP2).
- CBX-24 is a different row: after KSSH, its publickey half is met at DEV_POSTURE T0/T1 (`acceptance.md` CBX-24 tail); the row stays `partial` overall. **Closing Phase 10 criterion 1 therefore depends on a separate CBX-24 disposition**, not only on CBX-23/25. This survey does not assume it.

## 5. Locks, rows, Law-1, protected paths, overlapping plans

### 5.1 CELL-CONTRACT-0024 relations (`.agents/cell-contract-0024-locks.md`; numbering verified by counting the frontmatter list)

| Relation | Plan | Locked scope (verbatim intent, `:137,:139,:144,:151,:158,:162,:163`) | Touched by this program? |
|---|---|---|---|
| L11 | `260711-1917-tier3b-x86-vtx` | "Intel VMX/EPT P09 and unfinished guest-lifecycle/authority integration; AMD remains parked" | **No**: no VMX path, no `svm_registry.rs`, no `vmx*.rs`, no `kernel/src/main.rs`, no `cells/services/hypervisor/**` edit. The program consumes the SVM VMM as built. |
| L13 | `260712-0952-tier3b-vm-hardening-compat` | "unfinished shared guest backing, device and lifecycle authority" | **No** edit; but `virtio-rng` (the entropy gap, §6 R-5) and writable storage belong here: adding them is out of scope. |
| L18 | `260722-2330-tier3b-finish-g5-lite` | "unfinished guest clone, VMID, reset, snapshot and shared device authority" | No. |
| L25 | `260821-0642-app-tiers-completion` | admission/runtime slices | No. |
| L32 | `260827-1004-hardware-independent-roadmap` | "unfinished shared security, guest-lifecycle, C2C … slices"; its Phase 10 virtio parity is **completed** | Consumes completed output as evidence only (`locks.md:307`: "consume completed predecessors only as evidence"). |
| L36 | `260927-1100-c2c-anywhere-tier-aware` | guest bridge, AC-24 | No: no c2c broker, no guest bridge. |
| L37 | `261004-1957-x86-pc-lane` | device-authority changes affecting Cell contracts | No, unless route R2 (Net cell) were chosen. |
| (not in list) | `260724-1632-tier3b-virtio-gpu` | partial; "PARKED … no new lock" (`locks.md:258`) | No. |

Unfreeze procedure `locks.md:300-310` (six steps) applies to **executing** a locked plan's unfinished scope. `[INFERENCE]` a new lane that boots the already-built SVM VMM and adds only new test files and a private image does not execute any of those plans' scope and needs no unfreeze; the CP1 asks the owner to record that reading explicitly (§Locks in the CP1). If the run exposes a defect that needs a source fix under `cells/services/{hypervisor,net}/**` or `kernel/src/hypervisor/**`, that is **a different slice** needing its own CP1, owner handoff (step 3) and file-bounded exception.

### 5.2 Law-1 and protected material

- Law-1 (ABI and authority deltas need two digest-bound confirmations): **no ABI, syscall, capability, launch-edge or policy change** is proposed. Launch edges and policy rows for `/bin/hypervisor` and `/bin/cellbox-ssh` already exist (§1.2). No `libs/api`, `libs/types`, `kernel/**`, `hal/**` file is edited.
- Protected paths the program must leave byte-identical (names from `phase-10-qualification.md:224,258` and the probe §(d)): canonical `kernel/src/embedded-x86_64`, default `target/x86_64-unknown-none/release/cellos-kernel`, the **user's VMX source** (`hal/arch/x86/src/x86_64/vmx*.rs`, `vmcs.rs`, `world_switch.rs`, `kernel/src/hypervisor/svm_registry.rs`, `kernel/src/main.rs`), the G1–G5 guards (`scripts/cellos_sign/signing.py`, `check_default_kernel.py`, `make-iso-ci.sh`, `kernel/build.rs`, lane alias guards), `cells/services/net/**` and `cells/services/hypervisor/**`, `build/`.
- Guard interaction: G2a refuses `cellbox`/`app-cellbox`/`terminal` dev-key signing; the lane uses the existing private-fixture allowance with a random seed (precedent: `qemu-phase09-ssh-test.sh:329-331`). G2b (quarantined hashes) is unconditional and unaffected. G3 gates only the **default** kernel path; explicit `X86_KERNEL` is unchecked (`make-iso-ci.sh`, probe §(d)).
- Rows named in the assignment: L11, L13, L18 (above); adjacent L25, L32, L36, L37 are listed because the guest-bridge/device-authority wording could attract them.

### 5.3 Overlapping plans

| Plan | Status (HEAD, per probe + frontmatter) | Overlap |
|---|---|---|
| `260711-1917-tier3b-x86-vtx` | pending/blocked, L11 | P09 Intel VMX; AMD parked. P03 SVM done (M1 PASS under TCG 2026-07-23). Our target is the SVM path. |
| `260712-0952-tier3b-vm-hardening-compat` | pending/blocked, L13 | P04–P08 (writable storage, glibc rootfs, fuzz, x86 SVM continuation) not coded. Its dossier notes "no rng stalls crng init" (`reports/dossier-glibc-guest.md:20-23`; sshd host-key wait). |
| `260722-2330-tier3b-finish-g5-lite` | pending/blocked, L18 | clone/VMID/reset/snapshot; not touched. |
| `260724-1632-tier3b-virtio-gpu` | partial; parked, no lock | ARM64 first; x86 phase gated; not touched. |
| `260927-1100-c2c-anywhere-tier-aware` | pending, L36 | guest bridge. Not touched. |
| `260827-1004-hardware-independent-roadmap` P06/P10 | completed phases (virtio parity 27 scenarios) | evidence only. |
| `261030-1000-kms-ssh-key-custody` (KSSH) | program complete at DEV_POSTURE (T0/T1) | Precedent for lane scripts (`kssh_boots.py`), private image routing, one-run-per-set rule, and a possible later publickey-to-guest extension; not part of this CP1. |

## 6. Risks, unknowns, what was not checked

| # | Item | Why it matters | How the CP1 handles it |
|---|---|---|---|
| R-1 | VMM boot at HEAD never run since 2026-09-29/10-03; four relevant commits since (§1.4) | The whole program is moot if the guest does not boot | T3-B first; stop rule |
| R-2 | Hostfwd-to-non-leased guest address via loopback loop (libslirp behaviour) | Only available route | T3-C decisive experiment, with fallback = no-go record (R2 needs another slice) |
| R-3 | Guest IP/DHCP: outer Net cell leases `10.0.2.15` (`[INFERENCE]`); nginx gate hard-codes the guest to `.15` | Duplicate address would break ARP | Static `.16` for the guest |
| R-4 | Hypervisor reads UART RX (`Read`, fd 0) while the lane's Phase 04 driver expects no serial input | Possible stolen bytes if the lane writes to serial | Lane never writes to the serial socket; checked by a judge rule |
| R-5 | No virtio-rng: crng init stalls seconds to minutes (`dossier-glibc-guest.md:20-23`, nginx gate `head -c 32 /dev/random` wait, 240 s) | `sshd` blocks in `getrandom` until ready | Guest init waits with a timeout and prints a marker; no hypervisor edit (L13) |
| R-6 | Nested TCG speed | Per-case deadlines (120 s) and lane window (3600 s) from the single-level lane are too small `[INFERENCE]` | New per-case/lane budgets measured in T3-B, set in the CP1 as ceilings |
| R-7 | Headless image: hypervisor GPU/input personalities and the `/virtio-e2e` selector were only exercised in images with `ui` | A headless private image might trip an unforeseen path | T3-B runs exactly that image |
| R-8 | Phase 04 driver, hypervisor and Net service all in one 1 GiB outer VM with a 128 MiB contiguous guest carve (ARM CI notes carve fragmentation, `ci.yml:733-735`) | create_vm failure | Outer `-m 1G` like the CI lane; kernel log reason is printed |
| R-9 | `X86-VMM-SMOKE: FAIL … Preempted` open fact | Unknown relation | Recorded; not a gate (test-hooks only) |
| R-10 | OpenSSH `sshd-session` split and `kill -KILL $PPID` semantics for the no-replay case on the pinned version | Scenario design | Verified in T3-B/D offline iteration, recorded |
| R-11 | Documented guest docs stale (`docs/guides/tier3b-linux-vm.md:5,434` say Intel VMX not implemented) | Reader confusion only | Not edited here |

**Not checked (explicit):** the exact `apk` names/versions/hashes (needs the authorized fetch); that `docker` has the fixture image; whether libslirp in the pinned QEMU build permits the hostfwd loop; `libs`/`kernel` behaviour of `sys_open_cap` for a ~40 MiB `vmlinux` plus an initramfs inside a lane-sized FAT image; the contents of `kernel/src/embedded-hv-x86/kernel_fs.img` (not listed); whether the Oct-29 Net rights change admits the hypervisor's `L2Send` at runtime (the unit tests exist; no run); `cells/services/hypervisor` tests; anything not at HEAD `12f0b9d19`.
