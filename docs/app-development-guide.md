# Cellos App Development Guide

> How to choose a development model and write applications for Cellos.
> For syscall reference, see [api-reference.md](api-reference.md);
> for kernel internals, see [system-architecture.md](system-architecture.md).

**Version**: v0.4.0 | **Last updated**: 2026-08-19

---

## What is a Cell App?

A Cellos application runs as a **Cell**. For native apps, Cellos chooses an
execution **tier** first, then a runtime profile and SDK modules inside that
tier. Use `tier` only for execution/isolation classes; use `runtime profile` for
`rust-no-std`, future `rust-std`, POSIX/FFI, and Lua; use `SDK module` for
developer APIs.

---

## Execution Tiers

| Tier | Canonical name | Runtime profiles | Isolation | Current status | When to use |
|------|----------------|------------------|-----------|----------------|-------------|
| **Tier 1** | Trusted SAS Cell | `rust-no-std` shipped; pure-Rust `rust-std` in-tree; `ffi-posix` and `lua` trusted profiles | Shared SAS + LBI; fleet posture depends on signing/admission | Current native app path | Trusted first-party/platform cells, drivers, services, UI, embedded/robot apps. |
| **Tier 2** | Native Domain Cell | Same native Cell shape as Tier 1; `cpp-freestanding` (planned) | Private MMU domain; copied/domain-explicit IPC | Paged-domain mechanism is shipped and routed by artifact class (unsigned, `FFI`, `UNTRUSTED`) on RV64/AArch64/x86_64, and the on-path admission control landed ([ADR-0019](decisions/0019-tier2-admission-control-on-path.md)): one policy gate at the publication point, explicit per-profile posture, device/DMA authority refused. Physical containment, DMA quarantine and production approvals remain open, and no capability is qualified in the ledger yet | Containment target for untrusted/FFI native code in development profiles; fleet profiles deny domain-class artifacts. Treat deployment as gated by the acceptance ledger, and never silently assign such code to Tier 1. |
| **Tier 3** | VM Guest | `linux-guest` | Hypervisor / Stage-2 | ARM64 guest path exists; broader platform work tracked separately | Legacy Linux/POSIX stacks, fork-heavy apps and supported untrusted guest workloads under target-specific qualification. |

Legacy names: `Tier 1b` now means the Tier 1 `ffi-posix` or `lua` runtime
profile. `Tier 3b` now means the Tier 3 `linux-guest` profile. Guide filenames
keep the old names for link compatibility.

## SDK Modules

The SDK is one family, not a numbered set of tiers:

| SDK area | Examples | Applies to | Current maturity |
|---|---|---|---|
| Foundation | manifest, syscall ABI, lifecycle entrypoint | Tier 1 and future Tier 2 native Cells | Shipped for current native Cells |
| Runtime profiles | `rust-no-std`, `rust-std`, `ffi-posix`, `lua`, `cpp-freestanding` (planned) | Profile-specific setup | `rust-no-std`, in-tree `rust-std` (pure-Rust PAL), trusted `ffi-posix`, and Lua exist; `cpp-freestanding` is planned (ADR-0018 §2.3) |
| Service clients | VFS, net, IPC, service discovery | Tier 1 and future Tier 2 native Cells | Available in the native SDK; coverage remains service-specific |
| UI/graphics | ViUI, signal API, surfaces, desktop environment | Native UI Cells | ViUI and Desktop environment (`desktop`) exist |
| Middleware/helpers | AppContext, wrappers, RAII handles | Native app ergonomics | Available incrementally; not a separate SDK tier |
| Tooling | signing, manifest checks, image/build helpers | Build and release | Development tooling exists; fleet production key/admission provisioning is not complete |
| Guest integration | VirtIO/proxy contracts | Tier 3 VM guests | ARM64 path exists; strict guest verification is KVM/hardware-gated |

---
Tier 1 branches below require trusted code. Tier 2 routing exists and is exercised — a signed
`FFI`/`UNTRUSTED` cell or an unsigned cell runs in a private domain, asserted by
`tests/integration/tests/tier2_fault_isolation.rs`, `aarch64-boot.rs`, and `x86_64-boot.rs` —
and the admission control that gates it is on the path since
[ADR-0019](decisions/0019-tier2-admission-control-on-path.md) landed: one policy evaluated at
the publication point, explicit posture per build profile, and device/DMA authority refused
because a domain root cannot map it. A denial is final — no task, no domain, no SAS fallback.
Development profiles enable admission; a fleet-secure profile leaves it disabled, so a
domain-class artifact is denied rather than admitted anywhere. Qualification claims (`PASS`) are
still gated by the acceptance ledger, so treat Tier 2 deployment as gated by evidence, never by
this paragraph.


## Decision Tree: Which Tier?

```
┌─ "I have existing C/C++/Zig code"
│  └─ Use Tier 1 ffi-posix profile (legacy: Tier 1b) if the code is trusted.
│     C++ uses the `cpp-freestanding` subset profile (planned, ADR-0018 §2.3).
│     Untrusted C/C++ belongs in Tier 2 once its admission control lands (ADR-0019).
│
├─ "I want to write Rust"
│  ├─ "Need VFS, network, or IPC?"
│  │  └─ Use Tier 1 + service-client SDK modules
│  ├─ "Building a UI or dashboard?"
│  │  └─ Use Tier 1 + ViUI
│  ├─ "Handling cryptographic keys?"
│  │  └─ Use a purpose-specific KMS client; the Silo backend is not a public API.
│  └─ "Just syscalls and linked libraries?"
│     └─ Use Tier 1 rust-no-std profile
│
├─ "I want quick scripting / dynamic code"
│  └─ Use Tier 1 lua profile (legacy: Tier 1b Lua)
│
├─ "I need to view documents (HTML, PDF, Markdown, text, images)"
│  └─ Use Ocel (Tier 2, cells/apps/ocel). See ADR-0017.
│
├─ "I need a full web browser (Gmail, YouTube, web apps)"
│  └─ Use Tier 3 Chrome via hypervisor. See ADR-0017.
│
├─ "I need untrusted native code without a VM"
│  └─ Tier 2 is the target: routing and the on-path admission control are live in development
│     profiles (ADR-0019). Claims remain ledger-gated, so check the acceptance matrix first.
│
└─ "I have a legacy Linux binary / fork() is essential"
   └─ Use Tier 3 linux-guest profile (legacy: Tier 3b)
```

---

## Guides by Tier

- **[Tier 1 Rust (Bare)](guides/tier1-rust-bare.md)** — Minimal entry point, syscall allowlists, manifest declaration.
- **[Tier 1 Rust + SDK modules](guides/tier1-rust-sdk.md)** — AppContext, VFS/network clients, service discovery.
- **[Tier 1 Rust + ViUI](guides/viui-guide.md)** — Signal API, .vi DSL, compositor surfaces (see `system-architecture.md` §6).
- **[Development Silo provider](guides/tier1-silo.md)** — KMS-mediated `DEV_REFERENCE` AArch64 QEMU evidence only; no public Silo API or production/hardware custody claim.
- **[Tier 1 FFI/POSIX profile](guides/tier1b-c-zig.md)** — legacy `Tier 1b` guide: POSIX shim vs mlibc.
- **[Tier 1 Lua profile](guides/tier1b-lua.md)** — legacy `Tier 1b` guide: interpreter cell, VFS bindings, restricted stdlib.
- **[Tier 3 Linux guest profile](guides/tier3b-linux-vm.md)** — legacy `Tier 3b` guide: full kernel in hypervisor.

---

## SAS Laws Apply to All Cells

All Cells (regardless of tier) must respect the **8 Coding Laws** in [code-standards.md](code-standards.md):

| Law | Rule |
|-----|------|
| **Law 2** | Owned buffers (`Box<[u8]>`) across async; never `&mut [u8]`. |
| **Law 4** | Cells forbid `unsafe` (no exceptions for `#[no_mangle] main` in app_entry!). |
| **Law 5** | No `mod.rs` files — use `foo.rs` parallel to `foo/`. |
| **Law 8** | Implement `Drop` for all resources; no process cleanup. |

---

## Build & Run

```bash
# In a Cell directory (e.g., cells/apps/hello-cell):
cargo build --release --target riscv64gc-unknown-none-elf

# Run on QEMU:
./run.ps1   # Uses scripts/run-qemu-riscv64.sh internally
```

For multi-arch builds (ARM64, x86), see [getting-started.md](getting-started.md) § Build.

---

## Examples

- **Tier 1 bare**: `cells/demos/hello-cell/src/main.rs`
- **Tier 1 + SDK service clients**: `cells/demos/sdk-demo/src/main.rs`
- **Tier 1 + ViUI**: `cells/apps/robot-dashboard/src/main.rs`
- **Development Silo evidence**: `cells/tests/silo-test/src/main.rs` (KMS-mediated, `DEV_REFERENCE`, AArch64 QEMU only)
- **Tier 1 ffi-posix profile (mlibc)**: `cells/tests/mlibc-smoke/src/main.rs`
- **Tier 1 ffi-posix profile (POSIX shim)**: `cells/tests/posix-shim-test/src/main.rs`
- **Tier 1 lua profile**: `cells/runtimes/lua/src/main.rs`
- **Native C2C CPU 3D renderer**: `cells/apps/c2c-render/` and `libs/render-core/`; see below.

### Native C2C CPU 3D renderer

`c2c-render` is a CLI coordinator. Its separately spawned `c2c-render-worker`
is a trusted Tier-1 Cell; `c2c-render-domain-worker` is a signed `UNTRUSTED`
Tier-2 Cell with a private page-table root. The shared `no_std` render kernel
renders a deterministic
Cornell-style triangle scene with BVH acceleration, area lighting, diffuse and
metal materials, dielectric refraction, multiple bounces and Russian roulette.
No HTTP server, browser, GPU, global worker service registration or custom
network transport is required.

Build the isolated signed x86 image and run the real coordinator/worker smoke:

```bash
bash scripts/build-x86_64-c2c-render-ci.sh
python3 scripts/x86/run-c2c-render-smoke.py --cpus 1
python3 scripts/x86/run-c2c-render-smoke.py --cpus 2 --workers 4 --exercise-expiry --accel kvm \
  --log target/c2c-render-smp-shared.log
python3 scripts/x86/run-c2c-render-smoke.py --cpus 2 --mode compare-tier2 --workers 4 \
  --exercise-expiry --accel kvm --log target/c2c-render-smp-tier2.log

The image is `build/vicell-x86-c2c-render.iso`; artifacts use their own
`target/x86-c2c-render*` directories, not the production embedded image.
The smoke retains serial evidence in `target/c2c-render-smoke.log`.
In its Cellos shell:

```text
c2c-render compare --width 32 --height 24 --samples 2 --depth 4 --tile-size 8 --output /tmp/c2c-render.ppm
```

Modes are `baseline`, `copy`, `shared`, `compare`, `tier2`, and `compare-tier2`.
Baseline calls the same render kernel directly. Copy returns RGB tiles from the
trusted worker in typed IPC replies. Tier2 uses the same copied protocol with
the private-root domain worker; it never allocates or shares an output grant,
and admission failure never falls back to a trusted worker. Shared
grants a dedicated reusable output buffer to each worker; RGB pixels do not cross the IPC
copy path. Metadata still uses copied IPC, and tile-to-image assembly still
copies rows locally: this is **zero-copy pixel transfer from each worker to its
coordinator**, not an end-to-end zero-copy renderer or a generic C2C fastpath.
Compare requires identical complete image bytes and actual ray/sample totals
across all three modes, then saves a checked binary RGB PPM.

`--workers N` selects a bounded pool of 1–4 separately spawned Cells (default 1).
The effective count is `min(N, tile_count)`; baseline spawns none. Each slot owns
one accepted tile operation and, in shared mode, its own grant. The coordinator
fills all free slots, scans every operation's completion, then refills available
workers instead of waiting for the first worker to finish. Copied RGB replies
are borrowed from reusable per-slot IPC buffers through validation and assembly,
without allocating another pixel vector per tile. No accepted tile is retried.

```text
c2c-render compare --workers 4 --width 32 --height 24 --samples 2 --depth 4 --tile-size 8 --output /tmp/c2c-render.ppm
```

`compare-tier2` requires exact baseline/domain-worker image and ray/sample
equality. During setup it sends a valid one-pixel tile to every domain worker
with a sentinel shared-grant handle and requires `InvalidOutput` before copied rendering.
The domain worker rejects every Shared output before grant access, has no
grant syscall in its allowlist, and its copied-only runtime instantiation
eliminates the shared-grant branch. The smoke requires the kernel's exact
`Tier 2 Paged Domain (CR3 isolation)` admission marker, not an app-reported tier.
This works on the qualified x86 **development profile**, without `test-hooks`.
Fleet admission remains disabled; this consumer does not change that policy.

```text
c2c-render compare-tier2 --width 32 --height 24 --samples 2 --depth 4 --tile-size 8 --output /tmp/c2c-render.ppm
```

Each pixel owns its complete sample budget and a seed derived independently of
tile ID or worker order. The scene fingerprint covers geometry, materials,
camera, light parameters and an integrator version. The scene/BVH is built
once per Cell, not once per task. The coordinator never accesses a shared
buffer while its worker may write. On abnormal completion, all owned operations
are cancelled and drained, and **every** worker must cross the kernel's
exit/quiescence barrier before **any** shared output buffer is reclaimed.
Partial startup, validation and shutdown failures use the same pool ownership
cleanup; incomplete cleanup retains ownership and parks the coordinator.

Native limits: RGB image at most 4 MiB, at most 65,536 tiles, tile edge 1–18
(copied RGB payload at most 1,024 bytes), and the core's bounded dimensions,
sample and bounce counts. Defaults are 256×192, 64 samples/pixel, depth 8,
seed 1, tile edge 16, one worker, and `/tmp/c2c-render.ppm`. Output needs a writable VFS
backend and sufficient caller quota; errors do not claim a saved image.
Only the coordinator declares VFS mutation and lifecycle authority. Its
reviewed launch edges can create only these two capability-free worker artifacts.

CSV reports the actual monotonic timer frequency, scene setup, render wall/setup/
compute durations, exact serialized render request/reply bytes, copied pixel
payload bytes, actual rays and samples, RGB hash, and nearest-rank tile latency
p50/p95/p99. `render_wire_bytes` means encoded local request/reply bytes,
not kernel operation metadata, scene startup/shutdown or shared-refusal probe
messages, or Ethernet/TLS
wire traffic. Render wall time includes allocation, worker setup and shutdown;
hashing, percentile sorting and PPM save are outside it. Save time is separate.
Clock conversion uses the running kernel's reported frequency, never an
assumed clock rate or cross-node timestamp subtraction.

CSV additionally reports `workers_requested`, `workers_started`, and
`peak_outstanding`. The peak counts successfully submitted operations whose
results have not yet been consumed; it may include retained terminal replies.
Per-worker records include the actual TID and completed tile count, and those
counts must sum to the image's tile count. Tile latency is submission through
coordinator-observed/validated reply, not the worker's completion timestamp.
`compute_ns` sums worker-measured elapsed render durations, which can include
preemption; it is not CPU time and need not be less than render wall time.

The x86 backend supports **two-CPU multi-core** (`MAX_HARTS=2`) via Limine v8
multiprocessor protocol, per-CPU GS `CpuLocal`, per-CPU GDT/TSS, directed legacy
LAPIC IPIs, and remote all-PCID/global TLB invalidation acknowledgements.
The smoke runner supports `--cpus 1|2` (default 1) and `--accel tcg|kvm` (default `tcg`).
CSV and stdout report `processor_ids` for the overall run and for each worker
(semicolon-separated physical APIC IDs, e.g. `0;1`). These are hardware render
instruction endpoint observations (via unprivileged CPUID leaf 0xb/1), not all-core
CPU time or simultaneous-overlap proofs. On 2 physical cores under KVM, a 4-worker
128×96 8-sample Tier-2 run completes in ~2.62s vs ~4.70s baseline (~1.8x speedup)
with exact pixel hash equality.

Accepted RPCs currently expire after 3,000 scheduler ticks (30 seconds).
A tile exceeding that lifetime fails as indeterminate; the coordinator quiesces
the entire pool before releasing any shared output. It does not retry accepted work
or report a partial render as successful. Smaller tiles reduce per-call compute
time without changing the image's sample budget.
The runner's optional `--exercise-expiry` deliberately exceeds this lifetime,
then requires bounded failure, one ForceExit victim per pool member, and a
successful fresh pool render without reboot. `--workers 4` submits one heavy
tile to each of four Cells; recovery also exercises all four workers.
`--mode compare-tier2` performs the same exercise with admitted private-root Cells.

For a larger native correctness run, increase the runner's workload and timeout:

```bash
python3 scripts/x86/run-c2c-render-smoke.py --width 128 --height 96 \
  --samples 8 --depth 8 --tile-size 8 --timeout 300 \
  --log target/c2c-render-larger.log
python3 scripts/x86/run-c2c-render-smoke.py --mode compare-tier2 --width 128 --height 96 \
  --samples 8 --depth 8 --tile-size 8 --workers 4 --timeout 300 \
  --log target/c2c-render-pool-larger.log
```

A host executable exercises the same kernel with a larger workload and verifies
full-frame versus partitioned rendering:

```bash
cargo run --release -p render-core --example direct \
  --target x86_64-unknown-linux-gnu -- \
  --width 640 --height 480 --samples 64 --depth 12 --seed 1 \
  --output /tmp/c2c-render-host.ppm --csv /tmp/c2c-render-host.csv \
  --compare --tile-size 31
```

The host example is not a native C2C performance result. QEMU timings are
software evidence, not physical Intel performance or speedup claims. Compare
hardware workloads only with identical scene, sample budget, build profile
and stated CPU resources; retain repeated runs and variability.
Remote/LAN/Internet and Tier-3 guest execution are not implemented by this
consumer. `RemoteEndpoint::call` still returns `NotSupported`; its protected
relay authority build-entry gates (protected persistence, authenticated time,
pending-key binding and hostile endpoint evidence) remain unmet. The current
x86 HAL guest operations also return `NotSupported`, and the C2C guest bridge
queue/export/provenance contract is not ratified. An available host `/dev/kvm`
does not provide a Cellos guest runtime or a guest-to-native C2C bridge.
This app does not bypass those gates or route RPC through HTTP/TCP.


---

## Next Steps

1. Pick your tier from the **Decision Tree** above.
2. Read the corresponding **Guide**.
3. Copy a canonical example from the list above.
4. Adapt for your use case.
5. See [api-reference.md](api-reference.md) for syscall details.

---

## FAQ

**Q: Can I use the Rust standard library?**
A: Not yet in native Cells. Today use `ostd` and `rust-no-std`; G4 tracks a
future pure-Rust `std` profile. For trusted C/POSIX interop, use the Tier 1
`ffi-posix` profile.

**Q: Do I need to write unsafe code?**
A: No. Cells forbid `unsafe` at the crate root (Law 4). Only syscall entry points (`app_entry!` generated code) use it, and it is isolated.

**Q: How do I talk to other Cells?**
A: Use IPC. See [api-reference.md](api-reference.md) § IPC for syscalls (`sys_send`, `sys_recv`); Tier 1 SDK service clients provide ergonomic wrappers.

**Q: Can I spawn other Cells?**
A: Only `/bin/*` Cells with `spawn = true` in the manifest. See Phase 30 (project-roadmap.md).

**Q: What about real-time performance?**
A: Tier 1 is native (~1 μs syscall latency on QEMU). Use `sys_heartbeat()` for watchdog-style deadlines.
