# Witness cells, launch edge, and image staging

## Requirements

- A two-binary witness package: `backend-supervisor` (declares the tree, kills its own children,
  asserts what the library did) and `backend-worker` (capability-free actor answering typed calls).
- The supervisor must be spawnable at runtime with `SpawnCap` intact, and its *only* child edge must
  be the exact worker path.
- Image staging must put both cells where the kernel's own authorization rules allow them to be
  launched.
- The witness must be drivable by `tests/integration/tests/actor-supervisor.rs` and
  `scripts/qemu-actor-supervisor.sh` (build, boot, assert, publish evidence).

## Result

### Cells

- `cells/tests/backend/{Cargo.toml,build.rs}` + `src/lib.rs` (`WorkerMsg`/`WorkerReply`, the worker
  actor, and the scripted supervisor) + `src/bin/{backend-supervisor,backend-worker}.rs`.
  Registered as workspace member `cells/tests/backend`.
- The supervisor's script is deterministic and self-driving: ping → kill w0 → measure the restart →
  six kills on w1 → assert give-up with the survivors still answering → a second two-child tree under
  `one_for_all` → `ACTOR-SUPERVISOR: PASS`. Any failed assertion prints `ACTOR-SUPERVISOR: FAIL` and
  exits non-zero, and a 6 000-tick watchdog keeps a stuck step from hanging the runner.
- Manifests: the supervisor declares `spawn = true` (the source of `SpawnCap`); the worker declares
  no capability. Both declare the syscalls they use (the worker needs none beyond IPC).

### Launch edge (kernel)

- `kernel/src/loader/launch_profile/profiles.rs`: `backend_supervisor_profile` — exact
  `/bin/backend-worker` on `Path|Elf`, `child_ceiling = EMPTY`, `requires_lifecycle_authority = true`.
- `kernel/src/loader/launch_profile/mod.rs`: dispatcher arm `"backend-supervisor" if caller.has_spawn`.
- `kernel/src/loader/launch_profile/targets.rs`: `/bin/backend-supervisor` carries
  `CapSet { spawn: true }` on the reviewed user-target table (the shell edge must not strip
  `SpawnCap`), `/bin/backend-worker` is `CapSet::EMPTY`.
- `kernel/src/loader/launch_profile/tests.rs`: positive/negative pins for the new edge, plus a pin
  that the shell reaches the supervisor with `spawn` intact. **These pins are documentary** — see the
  plan's findings: the kernel crate cannot link a test harness, so the edge is enforced end to end by
  the QEMU witness, whose runner fails on any `DENY launch edge` line.

### Where the cells have to live (found the hard way)

The first witness run failed at the launch:

```text
[ WARN] [loader] DENY launch edge: caller=19 name=shell route=Elf target=/bin/backend-supervisor spawn_cap=false supervisor_cap=false
[ WARN] [loader] SpawnFromPath refused: caller=19 path=/bin/backend-supervisor error=NotFound
USER: shell: command not found: backend-supervisor
```

Two kernel rules decide this, and both are deliberate:

1. `launch_profile::authorize` refuses a non-empty `child_ceiling` on the **`SpawnFromElf`** route
   unless the caller holds lifecycle authority, so the shell cannot launch an authority-bearing
   binary from caller-read bytes.
2. The kernel loader's path route reads **VIFS1**; post-boot QEMU has no kernel block driver, so a
   disk-only copy is `NotFound`.

Therefore an authority-bearing cell must be staged in VIFS1 — the same placement the hotswap demos
use. `gen_disk.ps1` now builds and signs both cells, stages `/bin/backend-supervisor` into VIFS1
(§3a `kfs_args`), and stages both into the P2 table + FAT cell-store. The worker stays out of VIFS1:
it is capability-free, so the supervisor reaches it through VFS + `SpawnFromElf` in the disk
cell-store. `scripts/qemu-actor-supervisor.sh` boots the canonical kernel and `disk_v3.img`, checks
VIFS1 for the supervisor before booting, and rebuilds the kernel so the checked image is the booted
one.

### Witness plumbing

- `scripts/qemu-actor-supervisor.sh` — preflight (VIFS1 contains `/bin/backend-supervisor`), kernel
  build, boot with `-snapshot` (the canonical disk stays pristine and runs cannot collide on QEMU's
  write lock), `ACTOR-SUPERVISOR: PASS` wait, marker requirements (three workers ran, typed call,
  exit reason, kill, measured latency, six storm kills, give-up, survivors, `one_for_all`), rejection
  of `[backend] FAIL`/`ACTOR-SUPERVISOR: FAIL`/`[fault] Cell`, a failure on any `DENY launch edge`
  naming the *worker* (the shell's own Elf-route denial for the supervisor is expected and logged as
  INFO, because VIFS1 is what resolves it), `--harts 1|2`, and `EVIDENCE_DIR` capture of
  `<name>-harts<N>-qemu.{log,txt}`.
- `tests/integration/tests/actor-supervisor.rs` + its `[[test]]` registration — a thin driver of that
  runner (the `native-domain-qemu.rs` pattern), `#[ignore]`d because it rebuilds a cross-compiled
  guest; run with `cargo test --test actor-supervisor -- --ignored --test-threads=1`.

## Risk assessment

- Rollback: remove the package and its workspace member, the profile function and dispatcher arm, the
  two `targets.rs` rows and pins, the runner, the test registration, and the `gen_disk.ps1` rows.
- The launch edge is exact and lifecycle-gated, so a compromised child cannot escalate: the worker
  gets `CapSet::EMPTY` and the supervisor cannot spawn anything but `/bin/backend-worker`.
- `gen_disk.ps1` changes are the canonical-image path; they are exercised by the boot-suite job that
  already runs that script, and the QEMU witness fails loudly (not silently) if the staging is absent.
