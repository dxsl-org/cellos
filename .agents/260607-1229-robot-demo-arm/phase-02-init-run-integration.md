# Phase 02 — Init auto-start + ARM disk image + QEMU MQTT host-forward

**Status:** ⬜ Todo  
**Priority:** High — demo cannot run without this  
**Blocked by:** Phase 01 (robot-demo aarch64 binary must exist before disk image is built)

---

## Context Links

- Init supervisor: [cells/apps/init/src/main.rs](../../cells/apps/init/src/main.rs)
- QEMU run script: [run-arm-virt.ps1](../../run-arm-virt.ps1)
- Existing embedded ARM: [kernel/src/embedded-aarch64/](../../kernel/src/embedded-aarch64/)
- Build.rs arch detection: [kernel/build.rs](../../kernel/build.rs) — checks `src/embedded-{arch}` first
- Disk builder (RISC-V reference): [scripts/format-disk.ps1](../../scripts/format-disk.ps1)

---

## Overview

Phase 01 produced a robot-demo binary that does real MQTT. This phase wires it into the full boot sequence:

1. **Init code**: Add `/bin/robot-demo` as a `Temporary` supervised cell (spawns once, never restarted on clean exit).
2. **ARM disk image**: Create `scripts/format-disk-arm.ps1` that builds an `disk_arm_virt.img` containing aarch64 builds of all cells init tries to load.
3. **QEMU networking**: Add `-netdev user,...` with MQTT port-forward to `run-arm-virt.ps1`.

**Scope boundary:**
- ✅ Init: NSVC 6 → 7; add `/bin/robot-demo` with `Policy::Temporary`
- ✅ Create `scripts/format-disk-arm.ps1` — build ARM disk (analogous to format-disk.ps1)
- ✅ Update `run-arm-virt.ps1` — add VirtIO-NIC + SLIRP with port 1883 forward
- ✅ Update `kernel/src/embedded-aarch64/` rebuild instructions
- ❌ Out of scope: multi-board support, composite disk (riscv+arm), compositor, Lua on ARM

---

## Key Insights

1. **Init array is fixed-size**: init.rs uses `const NSVC: usize = 6`. Changing to 7 requires touching paths, tids, svc_ids, policy, restart_count, and window_start arrays. All are stack-allocated fixed-size — safe to extend.

2. **`Policy::Temporary` is already defined** in init.rs with `#[allow(dead_code)]`. Adding robot-demo is the first real use of it. When the demo exits cleanly (reason=0), init logs "service exited cleanly — policy says no restart" and moves on. No crash-storm risk.

3. **Arch-specific embedded**: `kernel/build.rs` prefers `kernel/src/embedded-{arch}` over `kernel/src/embedded`. The `aarch64` dir already has `init` and `kernel_fs.img`. After init changes, rebuild init for aarch64 and copy to `kernel/src/embedded-aarch64/init`.

4. **ARM disk vs embedded**: The kernel embeds only `init`. All other cells (`vfs, config, input, net, compositor, shell, robot-demo, ...`) come from the FAT32 disk image. `format-disk-arm.ps1` must build them for `aarch64-unknown-none-softfloat`.

5. **VirtIO NIC on ARM virt**: QEMU `virt` machine supports `virtio-net-device` with `-netdev user` (SLIRP). Add:
   ```
   -netdev user,id=net0,hostfwd=tcp::11883-:1883
   -device virtio-net-device,netdev=net0
   ```
   Port 11883 on host → port 1883 in guest (avoids needing root on Linux hosts). The QEMU gateway for the guest is always `10.0.2.2`.

6. **DHCP timing**: The SLIRP DHCP server responds fast (< 100ms), but the net service needs a few poll cycles. Init yields once between each service spawn — this gives net time before robot-demo starts. If needed, robot-demo's `sys_lookup_service` retry in its main loop handles the race.

7. **Disk layout**: format-disk-arm.ps1 only needs to differ from format-disk.ps1 in: `$Target = "aarch64-unknown-none-softfloat"`, `$OutFile = "disk_arm_virt.img"`, and the cells list (add robot-demo, net, periph-test if it exists).

---

## Architecture

```
Boot sequence (after this phase):

QEMU ARM virt
  │
  ├─ kernel (aarch64-pic kernel, embedded: init + kernel_fs.img)
  │    └─ spawns init
  │
  ├─ init (embedded aarch64)
  │    ├─ spawns /bin/vfs       (Permanent)
  │    ├─ spawns /bin/config    (Permanent)
  │    ├─ spawns /bin/input     (Permanent, may fail silently)
  │    ├─ spawns /bin/net       (Permanent)
  │    ├─ spawns /bin/compositor(Permanent, may fail silently)
  │    ├─ spawns /bin/shell     (Transient)
  │    └─ spawns /bin/robot-demo (Temporary ← NEW)
  │
  └─ robot-demo
       ├─ GPIO sensor loop (PL061 @ 0x0903_0000)
       └─ MQTT publish → net service → VirtIO NIC
                          → SLIRP → host:11883 → MQTT broker
```

---

## Related Code Files

**Modify:**
- `cells/apps/init/src/main.rs` — NSVC 6→7, add robot-demo Temporary entry
- `run-arm-virt.ps1` — add netdev + virtio-net-device lines

**Create:**
- `scripts/format-disk-arm.ps1` — ARM disk image builder
- `scripts/update-embedded-arm.ps1` — rebuild aarch64 init → embedded-aarch64/

---

## Implementation Steps

### Step 1 — Modify init to spawn robot-demo

In `cells/apps/init/src/main.rs`:

```rust
// Change:
const NSVC: usize = 6;
// To:
const NSVC: usize = 7;
```

Add `/bin/robot-demo` as the 7th entry in `paths`:
```rust
let paths: [&str; NSVC] = [
    "/bin/vfs", "/bin/config", "/bin/input", "/bin/net",
    "/bin/compositor", "/bin/shell",
    "/bin/robot-demo",  // ← NEW: G1 sensor→actuator→MQTT demo
];
```

Add `None` to `svc_ids` (robot-demo is not a registered service):
```rust
let svc_ids: [Option<u16>; NSVC] = [
    Some(service::VFS), Some(service::CONFIG), Some(service::INPUT),
    Some(service::NET), Some(service::COMPOSITOR),
    None, // shell
    None, // robot-demo
];
```

Add `Policy::Temporary` to `policy` (run once, no restart):
```rust
let policy: [Policy; NSVC] = [
    Policy::Permanent, // vfs
    Policy::Permanent, // config
    Policy::Permanent, // input
    Policy::Permanent, // net
    Policy::Permanent, // compositor
    Policy::Transient, // shell
    Policy::Temporary, // robot-demo: run once, never restart
];
```

`restart_count` and `window_start` are `[0; NSVC]` so they auto-expand.

After change, run:
```
cargo check -p app-init
```

### Step 2 — Rebuild init for aarch64 and update embedded

Build init for aarch64:
```
cargo build --release -p app-init --target aarch64-unknown-none-softfloat
```

Copy to embedded-aarch64:
```powershell
Copy-Item target\aarch64-unknown-none-softfloat\release\app-init `
    kernel\src\embedded-aarch64\init -Force
```

Note: `kernel_fs.img` in `embedded-aarch64/` is the VFS bootstrap image. If it's stale, rebuild with:
```powershell
# (Use mkfat16.py or format-disk flow for a minimal FAT16 with /bin/vfs, /bin/config, /bin/shell)
# Out of scope for this phase — use existing kernel_fs.img until a dedicated ARM bootstrap tool exists.
```

### Step 3 — Create `scripts/format-disk-arm.ps1`

Create a PowerShell script that:
1. Builds all cells for `aarch64-unknown-none-softfloat`
2. Creates `disk_arm_virt.img` (64 MiB, FAT32) 
3. Populates `/bin/` with aarch64 binaries

Cells to include:
```
app-init → init, app-shell → shell, service-vfs → vfs, service-config → config,
service-input → input, service-net → net, service-compositor → compositor,
app-robot-demo → robot-demo, app-hello → hello, app-echo → echo, app-cat → cat, app-ls → ls
```

Script skeleton (adapt from `format-disk.ps1`):
```powershell
param(
    [string]$OutFile = "disk_arm_virt.img",
    [int]$SizeMiB   = 64,
    [string]$Target  = "aarch64-unknown-none-softfloat",
    [string]$Profile = "release"
)
# Build all cells for aarch64
cargo build --release --target $Target `
    -p app-init -p app-shell -p service-vfs -p service-config `
    -p service-input -p service-net -p service-compositor `
    -p app-robot-demo -p app-hello -p app-echo -p app-cat -p app-ls
# Create image (mtools required)
$BinDir = "target\$Target\$Profile"
# ... same mpartition/mformat/mcopy flow as format-disk.ps1
```

Update the error message in `run-arm-virt.ps1` to reference the new script:
```powershell
Write-Host "Build it with: .\scripts\format-disk-arm.ps1"
```

### Step 4 — Add networking to `run-arm-virt.ps1`

In `run-arm-virt.ps1`, add two lines to the QEMU invocation after the `virtio-blk` device:
```powershell
    -netdev user,id=net0,hostfwd=tcp::11883-:1883 `
    -device virtio-net-device,netdev=net0 `
```

Also update the header comments to mention network:
```
#   VirtIO NIC  SLIRP user-mode networking
#               MQTT forwarded: host:11883 → guest:1883
```

Host MQTT broker instructions (add to header comment):
```
# MQTT broker: mosquitto -p 11883 (or any broker on host port 11883)
# Monitor:     mosquitto_sub -p 11883 -t 'vios/#'
```

### Step 5 — Rebuild aarch64 kernel and verify

```powershell
$env:RUSTFLAGS = "-C relocation-model=pic"
cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat
$env:RUSTFLAGS = $null
```

### Step 6 — End-to-end run

```powershell
.\scripts\format-disk-arm.ps1  # build disk_arm_virt.img
.\run-arm-virt.ps1              # boot and observe serial output
```

Expected serial log sequence:
```
[boot] ARM64 kernel starting...
Init: Starting ViCell Orchestrator...
Init: services spawned.
Init: service registry verified.
[robot-demo] Starting GPIO sensor-actuator loop on ARM...
[robot-demo] tick 0: sensor=HIGH actuator=HIGH
...
[robot-demo] tick 9: sensor=LOW actuator=LOW
[robot-demo] connecting to MQTT broker 10.0.2.2:1883...
[robot-demo] MQTT CONNACK received
[robot-demo] MQTT published: vios/robot
[robot-demo] done.
Init: service exited cleanly — policy says no restart.
```

---

## Todo List

- [ ] init.rs: NSVC 6→7
- [ ] init.rs: add `/bin/robot-demo` path, svc_ids None, Policy::Temporary
- [ ] `cargo check -p app-init` passes
- [ ] Build init aarch64 + copy to `kernel/src/embedded-aarch64/init`
- [ ] Create `scripts/format-disk-arm.ps1`
- [ ] Run `format-disk-arm.ps1` → `disk_arm_virt.img` created
- [ ] `run-arm-virt.ps1`: add netdev + virtio-net-device lines
- [ ] Update error message in run-arm-virt.ps1 to reference new script
- [ ] Rebuild aarch64 kernel
- [ ] `.\run-arm-virt.ps1` boots and shows robot-demo serial output

---

## Success Criteria

- `cargo check -p app-init` compiles with NSVC=7 and robot-demo entry
- `disk_arm_virt.img` contains aarch64 `/bin/robot-demo`
- Serial output shows robot-demo tick logs and "MQTT published" (or "broker unreachable" if no MQTT broker — both are valid)
- Init does not restart robot-demo after it exits
- Shell still spawns and is interactive

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `mtools` not installed on build host | Medium | Provide WSL fallback instructions in script |
| `aarch64` build of some cell fails | Low | Build incrementally; other cells exist from prior ARM work |
| kernel_fs.img in embedded-aarch64 stale/missing VFS | Medium | Existing img was placed during ARM HAL work; vfs is a Permanent spawn — if init fails to start it, log appears. This is pre-existing state. |
| MQTT CONNACK timeout (DHCP not yet acquired) | Medium | robot-demo gracefully logs "broker unreachable" and exits cleanly (Phase 01 design) |
| virtio-net-device not recognized by kernel | Low | ARM64 HAL includes VirtIO NIC driver; net service uses it on riscv64 already |

---

## Security Considerations

- SLIRP user-mode networking: guest cannot initiate connections TO host (only outbound from guest) — safe default
- Port 11883 on host is localhost-bound by default in QEMU SLIRP
- No secrets in QEMU invocation or scripts
