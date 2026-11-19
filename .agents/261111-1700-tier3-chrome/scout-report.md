# Scout report — real Chrome (Tier 3 Linux guest) on the x86 desktop

Date: 2026-11-11. Sources: two read-only scouts plus direct reads. `[INFERENCE]` = not run.

## What the user sees today
- `cells/apps/chrome/src/main.rs` (907 lines) is a fake browser. It draws its own tabs, URL bar
  and buttons; `navigate()` (l.168) writes "Loaded: <url>" text and never fetches anything.
  Its status bar says "Standalone Kiosk (VMM offline)" (l.106), because no hypervisor runs.
- `scripts/assemble-x86-image.py` `DESKTOP_CELLS` (l.141-149) has no `/bin/hypervisor`.
  The hypervisor row lives in `config/autoload.toml:16-20` and is only picked by the
  `tier3-autostart`/`hv-autostart` features (`scripts/generate-boot-config.py:69-70`).
- The play/proof QEMU runs `-nic none`, so no app can reach the web.

## Hypervisor cell (x86), `cells/services/hypervisor/src`
| Piece | Status | Evidence |
|---|---|---|
| PVH boot of `/vmlinux` + `/initrd.gz` from `kernel_fs.img` | works (SVM, TCG) | `boot_x86.rs`, `boot_x86_profile.rs:13-14`; lane `qemu-hypervisor-smoke-x86.sh:186` |
| Kernel VMX backend (VMXON, VMCS, EPT, INVEPT) | exists, never run by a lane | `kernel/src/cpu_features.rs:85-111`, `hal/arch/x86/src/x86_64/vmx_vcpu.rs`, `kernel/src/hypervisor/svm_registry.rs:172,264`; every lane uses `-cpu qemu64,+svm` |
| Guest RAM | 128 / 256 (alpine-wide) / 512 MiB (ubuntu) | `boot_x86_profile.rs:5-10` |
| vCPUs | 1 | single `vmm::create_vcpu`; MADT has one LAPIC |
| virtio-blk | one disk, `/mnt/sd/guest_disk.img` or volatile | `run_loop_x86.rs:55`, `persistent_disk.rs:124,137` |
| virtio-net | L2 bridge to the net service, no NAT | `net_backend.rs:1-4`; host DHCP/ARP compiled out by `service-net/hypervisor-bridge` (`net/src/main.rs:62-73`) |
| virtio-gpu | device model + scanout Grant -> own compositor surface, role `Background` | `virtio_gpu/scanout.rs:31-89`; Background is outside window management (`compositor state.rs:142-145`, `configure.rs:21`) |
| virtio-input | device model; fed from `ostd::input::poll_events` regardless of focus; keysym used as Linux keycode untranslated | `run_loop_x86.rs` `forward_input_events` |
| Guest sees GPU/input | **no** | cmdline declares only slots 0,1 (blk, net): `boot_x86_profile.rs:17-21`; DSDT empty |
| GPU/input IRQs on x86 | **wrong** | `inject_irq(19/20)` used directly as vectors (CPU exceptions); blk/net go via the 8259 as IRQ 5/6 (`x86-irq-dispatch.rs`) |
| `vmm_session` LAUNCH | stub: records client, boots nothing, ignores size | `run_loop_x86.rs:347-376` |
| RESIZE | missing (`_ => {}`) | same |
| TERMINATE | clears session only; guest and surface stay | `run_loop_x86.rs:410` |
| `cells/apps/vm-runner` | prints image paths, never uses them | `vm-runner/src/main.rs:58-62` |

## Guest images
- `scripts/fetch-alpine-x86.sh`: pinned Alpine 3.21.7 `vmlinux` (39 MB) + `initramfs-virt` (9 MB). Works.
  The initramfs has `virtio-gpu.ko`/`virtio_input.ko`; drm/kms deps presence `[INFERENCE]`.
- `scripts/build-linux-base-image.sh` (cage + mesa + `/opt/app` from `/dev/vdb`) and
  `scripts/build-chrome-app.sh` (Docker `apk add chromium`, 350M ext4): unpinned, orphaned;
  nothing consumes them; Chromium's library deps are not in the base `[INFERENCE]`.
- Storage limits: `/mnt/sd` P1 is 256 MiB (`tools/write-mbr.py:28`); `kernel_fs.img` is
  embedded in the kernel ELF (`kernel/src/dev/ramdisk.rs:10`), unsuitable for 0.5-1 GB.

## Host machine
- Intel i7-14700, `kvm_intel nested=Y`: nested VMX under `-accel kvm -cpu host` is possible.

## Decisions on record
- ADR-0017 §2.2: Tier 3 Chromium on Alpine, `--disable-gpu`, scanout via virtio-gpu -> compositor,
  thin launcher Cell. Parked by ADR-0022 ("Phase 4 is not started", §5a).
- ADR-0025 §3.2: the VMM stays a confined workload sandbox, not a general hypervisor.
- `docs/guides/tier3b-linux-vm.md:29`: "Intel VT-x guest execution is not implemented" (no evidence).
