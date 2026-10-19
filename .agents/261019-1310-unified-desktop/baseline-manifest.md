# Unified Desktop — Source and Image Baseline Manifest

**Date:** 2026-10-19  
**Plan:** `.agents/261019-1310-unified-desktop`  
**Phase:** 01 — Source baseline, exact contracts and executable image path

---

## 1. Source Revision and Working Tree

- **Base Commit Hash:** `4994602b55a43f3fd4262f36dc3dfaa7b1a2ff44`
- **Branch/Ref:** HEAD
- **Git State:** 26 modified tracked files, 1 staged new file (`tests/integration/tests/desktop-x86.rs`), untracked development artifacts present.
- **Preservation Contract:** User-owned working changes are retained untouched; no reset, revert, or overwrite.

### 1.1 Tracked Dirty Files (SHA-256 Hashes)

| Status | File Path | Full SHA-256 Hash |
|---|---|---|
| M | `.agents/TODO.md` | `b87295ed85db1b920bfd755c32e92cff825c9b736b005187e594d4d1e21b7119` |
| M | `.agents/failure-history.jsonl` | `f2769c13b85552c03df0b30bf08892d193d2cb2f82ba4415eb634fe7ba918664` |
| M | `.agents/plan-portfolio.md` | `689de85043c52162590fc92576fc93b6883e4453509d2e1b12b509ef14ae257b` |
| M | `CHANGELOG.md` | `e7d5ce3e9fab0af2441d830b5336bf9b8b7045b84294bce430ae16cbffec5535` |
| M | `Cargo.lock` | `877c81e9bc555760882e997f7bbff403160a2fc504f26033bbd57ce6c0e5a6fc` |
| M | `Cargo.toml` | `2b64dbb598b90e1ea7208d13253b802a8e80629b3ae3df67ee604e38c9fe29bf` |
| M | `build/x86-iso-root/boot/kernel.elf` | `ec85cb7ab06e6af33eeb6dbe5b5003c2fa6277c858b292353a152fbefce48721` |
| M | `cells/services/compositor/src/input_handler.rs` | `b35c81c5e584c9913197b1029c78fe20703c6218f673891d4e2db5d6ae68e82a` |
| M | `cells/services/net/src/dns.rs` | `a752a1fd4841af693bb44fe0c5e315357db5cba7ff06a77d4697ecb08f4316d9` |
| M | `cells/services/net/src/dns/tests.rs` | `1c97f4f8c2fc76cbdd5b33a595f7d242699e19d750fbfe3fbcf45a16d5661a3d` |
| M | `cells/tools/shell/src/async_utils.rs` | `528099419e23a6a1a1ffaa163ba345b13b4259b3cb6f23594e9f5e1ad3cb83e5` |
| M | `cells/tools/shell/src/executor.rs` | `833d0400e27e0f6534282e705bf1bc277c413b6aa8a8e3cb27cfdfc58ceba411` |
| M | `hal/arch/x86/src/x86_64.rs` | `9cff5497464acdba1c3426e85501fb0cfd58673a0058b75791d14e0ae5ec3791` |
| M | `hal/arch/x86/src/x86_64/paging.rs` | `d640c328ec97e8b4e70570b72049c313ef0104620f3a61ba932ca4d8a5c378e9` |
| M | `hal/arch/x86/src/x86_64/vmx.rs` | `f4a0301c6ba4710cd39ea2f57a94cb2ad63cb8dc667a481617495b45ca2c8524` |
| M | `hal/arch/x86/src/x86_64/world_switch.rs` | `65e09cab92d57b309192465ce2db5878440bc443bb2fb6921b0337c73ffaa364` |
| M | `kernel/src/embedded-x86_64/kernel_fs.img` | `bf99e48d17cab00473872c54fe1a7d6e67e3aeb09b1f09bb68cf5b2d716e25dc` |
| M | `kernel/src/hypervisor/svm_registry.rs` | `e887e6acfa4e50dca0bbff58c1fe2814ea331ffbf058e1c31317ba85698b671a` |
| M | `kernel/src/loader/boot_ceiling.rs` | `a4cb6ad1fa63ad072714c1aa57947702f3cf9f1d07c086be7fb9f0f97022d4f2` |
| M | `kernel/src/loader/launch_profile/profiles.rs` | `538d0cc84dbcc5951d4546ee5eb75225916ee1ad451cfbcba51e605d3b6efdf4` |
| M | `kernel/src/loader/launch_profile/targets.rs` | `4c3265aea3b2cf2339679f224b5ba47e929a75d5a7d97e897914a87383636605` |
| M | `kernel/src/main.rs` | `a9636fee1f3b63b82aa83570d517cbe32eb7a1a2e8790ee6ee5e7fc2bcf81cb3` |
| M | `kernel/src/task/drivers/ps2_keyboard.rs` | `6d9d661cb81ef6da6a1200234199c0944062cb17a5e8f49a37bbba0b777a82ba` |
| M | `scripts/unsafe-allowlist.toml` | `6b0e4779938a009ce4a065842848cba47e45e54d3ca3e5f2cfc3a44d03da047a` |
| M | `tests/integration/Cargo.toml` | `9367bccbe510514fbb1a0ef9bac13a36d04ea773648146cf17b83cd33a52bdbd` |
| M | `tests/integration/src/lib.rs` | `c74f07f103e68d9d6fd486b7df52b02c4d97bc9a5855fa4fc01b2b69dfd0cec5` |
| A | `tests/integration/tests/desktop-x86.rs` | `2efdeafb52330ed49821bdc3ac5e6ae598b0b523539b7b170913350d5c106449` |

### 1.2 Untracked User Artifacts Complete Inventory

| Path | Size (Bytes) | Full SHA-256 Hash | Role / Description |
|---|---|---|---|
| `docs/decisions/0027-haiku-tabs-and-spotlight-hud-desktop.md` | 17,412 | `63c31910266b5076104535f41f250790346727a349a9a5f40b6206273562e0bd` | Approved ADR-0027 architecture decision record |
| `hal/arch/x86/src/x86_64/vmcs.rs` | 16,266 | `5a2185cc15321f7be22a04151d68fe19075c490f32c0f9e34accb8bc50225384` | x86 VMX VMCS encoding substrate |
| `hal/arch/x86/src/x86_64/vmx_vcpu.rs` | 16,860 | `e907e88c2345481cbea07e753c12280ab6efc21bb67abd1eeca78a18aadc1169` | x86 VMX vCPU execution loop |
| `run-desktop.sh` | 1,347 | `a93a641e54988dbc8990c8795aefe9f89300c9071b709d29e16e0837824b54fe` | Local interactive QEMU launcher with KVM |
| `scripts/build-chrome-app.sh` | 2,468 | `bac87ec8d1b2964770011e12956dd9dcb036c09422836d8ffea7ad08854bf1f8` | Tier 3 Chrome ext4 payload builder |
| `scripts/build-linux-base-image.sh` | 4,298 | `6a84b2c6d2e9f9aa0d4196b0f00511b68d0f86fb0f5905be83e01b561481bcf0` | Minimal guest Linux rootfs builder |
| `apps/chrome/chrome.ext4` | 367,001,600 | `5f7f197d0ac8e42759c5738dfecdfe36a3a9f360cee71c892353d4eb2714adcc` | Chrome ext4 image |
| `apps/chrome/chrome.lua` | 866 | `fb9fe802c918f15cb76b8da4cc72086d85f92f9bb84829325dab0120258a9a41` | Chrome startup script |
| `apps/chrome/chrome.toml` | 787 | `fb8211d0004ecb5ab7a97eb90199222f8878e0add3c17cfae670e4026796045e` | Chrome manifest |
| `cells/apps/chrome/Cargo.toml` | 362 | `1e48086472cf5fdac178c024d814d68f3a0cd874aa46434610ac09bef14ec1ec` | Chrome cell manifest |
| `cells/apps/chrome/build.rs` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` | Chrome cell build script |
| `cells/apps/chrome/src/main.rs` | 1,395 | `76be143b6939737038591b7ef46d36f4444b6765c6cccb500bcd17a6b153d4aa` | Chrome cell entry |
| `cells/apps/vm-runner/Cargo.toml` | 364 | `0e05a3a1bdb1c285ba3faaf990b2a1ec58561b9b4cefcea521c778585f31ae85` | VM runner cell manifest |
| `cells/apps/vm-runner/build.rs` | 52 | `309c7f94ddf64d2c2d1fce2f9472d674869c6feb2218ebcadbf4bab5626d5f90` | VM runner cell build script |
| `cells/apps/vm-runner/src/main.rs` | 3,750 | `dad12baf485fea0ed4c1e20a006c8d88a3323dfff277f36bed75ba25e57b22fd` | VM runner cell entry |
| `build/chrome-app.ext4` | 367,001,600 | `5f7f197d0ac8e42759c5738dfecdfe36a3a9f360cee71c892353d4eb2714adcc` | Admitted single-app Tier 3 payload filesystem |
| `build/linux-base.ext4` | 536,870,912 | `b1b02e0bbba383221b64f2d65dbadeba1d4f20d42db75d9c232e4348ac778ea0` | Minimal guest Linux rootfs image for Tier 3 VMM |
| `build/screen-after.png` | 96,803 | `21d40db809bab4b59ea85fe6a770e552967a73056a370811cdaf6e1d7be2c226` | Disposable framebuffer capture |
| `build/screen-after.ppm` | 3,072,016 | `3d4cc6dbf0901eb06a86fa0d7c83a9ad95f6bd1c18be61e48a7877d4b18fbb22` | Disposable framebuffer capture |
| `build/screen-before.ppm` | 3,072,016 | `b626307dd19bfdc7acd0a371e1cb7420885bf4b47660b16c9b85152e09b97847` | Disposable framebuffer capture |
| `build/screen-chrome-run-final.png` | 125,311 | `45d18e2909e17cb6ed4a32b9782049c303be7d508a98352264bea3647501d345` | Disposable framebuffer capture |
| `build/screen-chrome-run-final.ppm` | 3,072,016 | `43e7d052d037a1525ec512b8418e89f4b8ac747ac43a35bd60f4b558d3f5074e` | Disposable framebuffer capture |
| `build/screen-chrome-run.png` | 118,726 | `7bcf532aaa5bc03173def5d4db3d57eee8d4a34fea49e667047ba21ce1b97505` | Disposable framebuffer capture |
| `build/screen-chrome-run.ppm` | 3,072,016 | `7e9b86eaa71cd95f7cd3f66ad3974270a31436afebcbadac63a3d34a1ba06b3d` | Disposable framebuffer capture |
| `build/screen-chrome-success.png` | 125,117 | `837928dd95c63ba3d939abcf5dab8c71cdef5d64f0834ac8d74fe5f523d7937d` | Disposable framebuffer capture |
| `build/screen-chrome-success.ppm` | 3,072,016 | `001c0e8544d47e5dab810dfc7bdec3e7103eb9bd746bda0cc0614ffa618052fd` | Disposable framebuffer capture |
| `build/screen-chrome.png` | 96,166 | `6127c7c7e3b0fd5a9742fefc57ad81ff2586a0c645d4317020100628e3d9221c` | Disposable framebuffer capture |
| `build/screen-chrome.ppm` | 3,072,016 | `b6c43d01551282acc80319d3e346d3926858f47cd5f86da007442f66d83ac1b1` | Disposable framebuffer capture |
| `build/screen-chrome2.png` | 119,132 | `997b6dfc906e49262923b531073fb13b218191e6fec5961f72783ce535587def` | Disposable framebuffer capture |
| `build/screen-chrome2.ppm` | 3,072,016 | `2011cad34af11bb017290d28bc2c3bfbc7f75cc00954e5012821950541e8c641` | Disposable framebuffer capture |
| `build/screen-chrome3.png` | 118,338 | `f194f112603c112360e1aa7cf344c1d85ea06a7b705eaa339de99617286263b0` | Disposable framebuffer capture |
| `build/screen-chrome3.ppm` | 3,072,016 | `c5eaca3a6455bd4d93da01d5cc39f626bf1091dc913921ec3ce29cd4531b0f94` | Disposable framebuffer capture |
| `build/screen-live.png` | 46,736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` | Disposable framebuffer capture |
| `build/screen-live.ppm` | 3,072,016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` | Disposable framebuffer capture |
| `build/screen-ocel-exit.png` | 117,633 | `1951b4e3132cf064c95b4782737bd54a84c17af82efd6b077427c50030a1d13b` | Disposable framebuffer capture |
| `build/screen-ocel-exit.ppm` | 3,072,016 | `a15763fcc56a3665d736b16c4a2931b8dfc0cf30166c2c3c274410ad62d0cb10` | Disposable framebuffer capture |
| `build/screen-ocel-running.png` | 46,736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` | Disposable framebuffer capture |
| `build/screen-ocel-running.ppm` | 3,072,016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` | Disposable framebuffer capture |
| `build/screen-ocel.png` | 46,736 | `2a7e8f13c14b5293a6b16465e1d9ac4776e3352b6adc318f15003afd96e2fc16` | Disposable framebuffer capture |
| `build/screen-ocel.ppm` | 3,072,016 | `bab9d082408d9a1164d50f53139ec041a9c0b8c853e9410e31d63bc6afb6dbb4` | Disposable framebuffer capture |
| `build/screen-test.png` | 78,175 | `9ed3049e3bb4ced1f397fd4dca3fa5edb40ce0bb6db43871279e8dafe3a5fc46` | Disposable framebuffer capture |
| `build/screen-test.ppm` | 3,072,016 | `5cf53f8ce2c81f2e6b7115a3aef9d43ddad7601e15fba45198feb8b94ffa7005` | Disposable framebuffer capture |

---

## 2. Compiler, Platform and Toolchain State

- **Rust Compiler:** `rustc 1.97.0-nightly (f53b654a8 2026-04-30)`
- **Host Target:** `x86_64-unknown-linux-gnu`
- **Kernel/Cell Target:** `x86_64-unknown-none`
- **LLVM Version:** `22.1.4`
- **QEMU Emulator:** `8.2.2 (Debian 1:8.2.2+ds-0ubuntu1.17)`
- **Kernel Boot Target:** Limine v8.7.0 dual BIOS/UEFI protocol, x86_64 ELF
- **Hardware Virtualization:** Intel VT-x (`/dev/kvm` read/write verified; VMX root operation active)
- **Framebuffer Driver:** SeaBIOS/Limine linear framebuffer (1280x800x32 bpp, physical base `0xfd000000`)

---

## 3. Disk Image and Executable Runner Inventory

| Artifact | Size (Bytes) | 1MB Head SHA-256 | Role / Description |
|---|---|---|---|
| `build/cellos-x86_64-desktop.qcow2` | 12,582,912 | `ae3a64905fe34ab0` | Primary development desktop disk (QCOW2, VirtIO) |
| `build/cellos-x86_64.iso` | 19,279,872 | `571b40e0e65ccc2f` | Hybrid BIOS+UEFI bootable release ISO |
| `build/chrome-app.ext4` | 367,001,600 | `0095d09e7780f1e6` | Admitted single-app Tier 3 payload filesystem |
| `build/linux-base.ext4` | 536,870,912 | `d9ed4aad9b60c758` | Minimal guest Linux rootfs image for Tier 3 VMM |
| `kernel/src/embedded-x86_64/kernel_fs.img` | 12,582,912 | `bf99e48d17cab004` | VIFS1 embedded initramfs/FAT image |

### 3.1 Materialized x86 Desktop Integration Runner (`tests/integration/tests/desktop-x86.rs`)

Phase 01 materializes the planned runner contract:
```sh
cargo test --manifest-path tests/integration/Cargo.toml \
  --target x86_64-unknown-linux-gnu --test desktop-x86 -- --nocapture
```
The runner verifies:
1. Validates that `CELLOS_DESKTOP_ISO` or `build/cellos-x86_64.iso` exists; panics if absent.
2. Spawns `qemu-system-x86_64` under Limine BIOS with serial bridge.
3. Awaits Limine boot and `[Cellos] kernel boot` marker within 60s timeout.
4. Asserts that display compositor/fb-console service publishes.
5. Asserts that interactive prompt `Cellos > ` appears.
6. Injects `help` command over serial and verifies prompt return without kernel panics or cell faults.

---

## 4. Observed Pre-Existing Baseline Smoke Behaviors

Booting `build/cellos-x86_64-desktop.qcow2` in QEMU with KVM acceleration produces the following documented baseline observations:

1. **Hardware & Virt:**
   - 1GiB paging supported, PCID enabled (`CR4.PCIDE=1, CR3=0x59000`).
   - VMX preconditions met (`FEATURE_CONTROL=0x100005`, `VMX_BASIC_revid=0x11e57ed0`), VMX root operation active.
   - ACPI HPET + LAPIC initialized; 16550 serial port 0 active at `0x3f8` (IRQ 4).
   - PCIe ECAM scan discovered 7 devices (Host bridge, VGA, e1000, VirtIO block, ISA bridge, ICH9 AHCI, SMBus).

2. **Kernel Self-Tests:**
   - `THREAD-CAP`: PASS
   - `GRANT-RECLAIM`: PASS
   - `THREAD-QUOTA`: PASS
   - `NET-RX-RESERVATION`: PASS
   - `IPC-PENDING`: PASS
   - `cap-file`: PASS
   - `IPC-GUARDRAILS`: PASS
   - `P-TRUST`: PASS
   - `boot-ceiling`: PASS
   - `mmio-allowlist`: PASS
   - `MANIFEST-V2`: PASS
   - `COMPLETION-QUEUE`: **FAIL** (`[selftest] COMPLETION-QUEUE: FAIL — an older waiter erased its replacement`) — **retained pre-existing failure**.

3. **Services & Desktop Entry:**
   - Tier 2 admission: ENABLED (`browser engine profile, QuickJS only`).
   - Platform, Init (tid 2), e1000 (tid 6), serial (tid 8), input (tid 10), net (tid 11, DHCP acquired 10.0.2.15), compositor (tid 12, software blending + software cursor), fb-console, sshd (port 22 active), and shell (tid 15) all launch successfully.
   - Interactive shell reaches `=== Cellos shell ready — type 'help' for commands ===` and `Cellos > ` prompt within 12 seconds.
   - Known pre-existing warnings: FAT32 `/mnt/sd` mount failed, CellosFS open failed (volume unavailable).

---

## 5. Verification Matrix: TCG, KVM, and VMware Procedures

| Target Matrix | Command / Runner | Executed Observation / Exit Status | Status |
|---|---|---|---|
| **QEMU KVM (Local Headless Smoke)** | `timeout 15 qemu-system-x86_64 -enable-kvm -cpu host -machine q35 -m 2048M -drive file=build/cellos-x86_64-desktop.qcow2,format=qcow2,if=virtio -display none -serial stdio` | Limine boot, VMX root operation, compositor/fb-console/sshd, `Cellos > ` prompt in 12s | **VERIFIED (Exit 0)** |
| **QEMU KVM (Integration Runner)** | `CELLOS_DESKTOP_ACCEL=kvm cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 -- --nocapture` | `test desktop_x86_boots_to_shell_and_compositor ... ok` (QMP pointer routing + screendump diff + QMP keyboard injection verified; 1 passed in 13.79s) | **VERIFIED (Exit 0)** |
| **QEMU TCG (Integration Runner)** | `CELLOS_DESKTOP_ISO=build/cellos-x86_64.iso CELLOS_DESKTOP_ACCEL=tcg cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 -- --nocapture` | `test desktop_x86_boots_to_shell_and_compositor ... ok` (QMP pointer routing + screendump diff + QMP keyboard injection verified; 1 passed in 19.45s) | **VERIFIED (Exit 0)** |
| **VMware Workstation / ESXi** | Attach `build/cellos-x86_64.iso` as IDE/SATA CD-ROM; GuestOS `other-64`, BIOS firmware, e1000 NIC, Serial COM1 pipe | Limine BIOS bootloader menu, kernel boot banner, serial shell prompt | Procedure pinned; hardware execution external-gated |

---

## 6. Scope and Governance Reconciliation

- **`docs/roadmap/current-focus.md`:** Records the architectural direction of C2C Anywhere on Intel x86-64 and a documentation-only freeze under ADR-0024. The user's explicit approval of ADR-0027 and `.agents/261019-1310-unified-desktop` authorizes this unified desktop implementation without unlocking unrelated locked legacy lanes (such as G2-J persistent crash format or distributed remote clusters).
- **Public Symbol Governance:** Edits to public interfaces in `libs/api` and `libs/ostd` are governed by Law-1 CP1 (Design approval) and CP2 (Implementation approval), tracked per delta in `abi-delta-ledger.md`.
