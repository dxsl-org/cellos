# P01 — Measurement launcher contract

Status: design decision for baseline preparation; runtime acceptance pending. No new public ABI, init feature, production activation, shell authority or phase/lock release. Supersedes the proposed new init fixture launcher in the two-CPU readiness record: source review found an existing configured route. Keep refused-launch evidence unchanged.

## Decision and source evidence

Use a measurement-only `/etc/cellos/autoload.toml` through the existing init configuration loader, not a new compile-time launcher. `cells/tools/init/Cargo.toml:40-43` explicitly makes ordinary boot selection configuration-owned.

- `libs/boot-config/src/lib.rs`: version-1 CellSpec has bounded structured args, dependency validation and restart policy; encoded argv maximum512bytes.
- `cells/tools/init/src/configuration.rs`: reads and validates services/autoload together before configured launch.
- `cells/tools/init/src/service_table.rs:53-78`: stages full argv before each ELF/raw-path attempt, frees ELF grants, and does not path-fallback on OOM. Existing internal fallback is not application request retry.
- `kernel/src/loader/launch_profile/profiles.rs:49-53` plus `targets.rs`: init autoload may launch reviewed `/bin/bench` with its exact application ceiling, not init's authority union.
- `profiles.rs:196-214`: bench may launch `/bin/bench-probe` by Pinned route with CapSet::EMPTY; `mod.rs` also requires bench spawn authority. Policy row alone does not grant the probe spawn authority past the edge ceiling.
- `cells/tests/bench/src/scenarios/async_lifecycle.rs`: eight outstanding calls, four death-leg attempts,60settle yields,200wait ticks per round,40rounds; probes pinned to CPU0.

These are source-backed existing mechanisms, not evidence that the new config has booted. If admission/configuration rejects the image, report NOT_RUN and investigate that exact gate; never broaden authority to obtain a result.

## Measurement image selection

This proposed configuration is a private image input, not installed into normal images:

```toml
version = 1

[[cells]]
name = "bench"
path = "/bin/bench"
args = ["async-lifecycle"]
enabled = true
required = true
restart = "never"
registration = "init"
```

No service_id: bench is a workload, not a globally registered service. Do not encode measurement success as service publication/readiness. No explicit `after` is needed for this scenario's direct probe IPC; retain existing service-stage validation and VFS bootstrap. Reject duplicate bench entries/paths across services and autoload. Do not add shell→bench, bench→bench, init→probe or arbitrary user-supplied path edges. Package the real signed bench and probe at those exact paths with current manifest/policy ceilings.

Build inputs: preserve selected-service source set and dev signing; add generated config bytes/digests and verify FAT readback. Rebuild kernel to embed the changed ramdisk; assemble isolated new ISO. Do not mutate or relabel `build/p01-service-x86.iso`. Trace config→ramdisk→kernel→ISO and actual runtime profile. No production claim follows from a dev-key image.

## Bounded execution and acceptance

One scenario per image/run; automatic restart disabled. Capture raw serial from boot to termination. Host watchdog150seconds is an observation bound, not a guest scheduling guarantee or CPU quota. Timeout yields INCOMPLETE; never count it as a passing sample. Capture result before killing the VM; preserve command, QEMU version/CPU mode/online CPU count, artifact hashes, elapsed time and termination reason. Changing watchdog/profile invalidates like-for-like latency comparison.

Required lifecycle evidence:
1. Exactly one START and one final PASS, no FAIL, plus workload exit identified as bench generation rather than any clean init child exit.
2. BOUNDED completed=8/lost=0/wrong_seq=0 and BOUNDED-RELEASED ok=true. Preserve provider/reverse reply records; a generic PASS alone is insufficient.
3. TRY-SEND parked_refused=false/busy_refused=true and provider's own busy/second-receive evidence; do not treat an Ok sentinel as delivery.
4. DEATH records attempted/submitted/refused outcomes: terminal equals outstanding admitted calls, peer_gone equals terminal, unterminal=0/lost=0 and at least one admitted call. Four attempts do not imply four submissions: record refusals, do not rewrite expected behavior.
5. Timer frequency nonzero for converted latency; retain raw ticks. Eight samples' maximum labeled p99 by the existing scenario is not a production p99 estimate. Unknown frequency invalidates seconds-based metrics, not observed terminal accounting.

The source scenario is not a cancellation test, shared-CSpace multithread test, arbitrary worker-fault containment test or cross-CPU throughput benchmark. Providers are pinnedCPU0; two online CPUs alone cannot certify multi-CPU workload execution. Never promote this baseline to AC-01..AC-50 acceptance.

## Measurement matrix and remaining instrumentation

| Question | Baseline configuration/scenario | Required evidence | What it cannot establish |
|---|---|---|---|
| Launch legality and bounded completion | async-lifecycle; first oneCPU, then twoCPU | Exact launch chain, leg counters, terminal records | New contract correctness, cluster behavior |
| Resident incremental/retained Cell cost | Separate fixed-count parked workload; count/cleanup mechanism must be designed before run | Same image, pre/live/post attributed frames/metadata, exact live generations and termination | Delta from unrelated boot images |
| Peak operations/queue footprint | Separate bounded submission workload with known peer state | Event-boundary peak, operation/completion counts, overflow outcome and settled residual | Peak inferred from two shell snapshots |
| Aggregate CPU budget across CPUs | Future exact approved quota implementation and workload that executes on distinct CPUs | Sum charged execution, replenishment/burst, overshoot tolerance, runnable/throttled transitions | Wall-clock latency on TCG or topology markers |
| Application-wide admission and autoscaling | P08 Authority/controller/router plus deterministic admitted placement scenarios | Durable reserve/live/unknown charge, nested shares, demand coverage, resize/drain and failure traces | Launcher's restart behavior as autoscaling |

No unbounded capacity sweep. Fixed count, sample cadence, peak instrumentation, settling criterion and attribution tolerance must be recorded before each resource run; remain undecided until measured/design-owned inputs exist. Poll-only sampling is a sampled maximum, not event-complete high-water. A probe absent from process listings is insufficient to prove all deferred/quarantined resources reclaimed. Reclamation requires settled resources and generation/operation accounting; unknown/quarantine stays charged according to the eventual approved contract.

## Ownership, dependencies and rollback

P01 owner maintains private config/build manifest/capture validation and profile-budgets evidence. Existing init/configuration and loader owners review actual composed launch ceilings. P02/P03 own future lifecycle/metadata accounting; P08 owns distributed budget/placement/controller/router evidence. No new phase, public inspection syscall, permanent diagnostic ABI or old-plan unlock is authorized by this document.

First runtime step is packaging/configuring existing mechanism, not implementing a new launcher. If required counters are unavailable through current interfaces, record the gap and design narrowly scoped development instrumentation separately; do not fake the values or make a public ABI without checkpoint1.

Rollback: stop measurement VM and discard isolated image/config; normal image/source/policy remain unchanged. Captured evidence cannot be undone and must remain labeled if a subsequent design supersedes it. Do not delete denied-launch logs.

## Verification observed for this planning update

Initial host-test command inherited bare-metal target and failed E0463 (missing test crate); corrected explicit host target, without source edits:
`cargo test -p cellos-boot-config --lib --locked --offline --target x86_64-unknown-linux-gnu` —18passed. This exercises existing parser/argv/dependency boundaries, not a boot of the proposed config or the async workload. Runtime gate remains pending.

Subsequent [configured runtime evidence](measurement-readiness.md#configured-bounded-lifecycle-runtime--2026-10-12) now observes real init-owned launch and all existing scenario legs on oneCPU/twoCPU. It supersedes the earlier unrun-config status for that isolated baseline, not the strict evidence requirements: bench TID/incarnation attribution is inferred rather than explicitly published; resource counters/settled reclamation and aggregate CPU remain unproven. Existing scenario PASS is not full launcher-contract qualification or P01 completion.

## Private parked implementation commands

The separate private lane is implemented by `scripts/x86/p01_capture.py` and
`p01_trace.py`; the existing async evidence above is not substituted for it.
Runtime/build acceptance remains pending until the integration owner executes it.
No architecture phase, lock, production profile or budget is approved here.

```sh
python3 scripts/x86/p01_capture.py build --out build/p01-private
python3 scripts/x86/p01_capture.py capture --build build/p01-private --out build/p01-captures
python3 scripts/x86/p01_capture.py parse --capture build/p01-captures/<capture-UUID>
python3 -m unittest discover -s scripts/x86 -p test_p01_trace.py
```

Build/capture output directories must not already exist. The builder uses locked,
offline Cargo, separate target/embedded/ISO roots, the existing development signing
tools and unchanged policy, signed init/VFS/bench/probe, FAT byte readback, an empty
services table and real init-owned `args = ["parked-footprint:N"]`. Each selected
count0/1/4/8 and the separate denied control is built once and booted three times
on oneCPU. The96byte private manifest binds signed init/bench/probe digests; it is
not boot attestation. `/bin/vfs` Pinned from bench is the intentional denied edge
under existing authority, not a missing-file substitute.

Dependencies: installed repository Rust nightly + rust-src and cached locked
Cargo dependencies, host C compiler and libclang/bindgen support, Python3 and
`cryptography`, GNU `objcopy`, `mtype` from mtools, `xorriso`, repository Limine
BIOS/UEFI files, Git (source-input inventory), and `qemu-system-x86_64` with TCG.
The wrapper downloads nothing. Build logs, input/artifact/config hashes, full
commands/environment, QEMU binary/version and raw serial remain in new immutable
output trees, including rejected evidence and build failures.

Every boot has a host-assigned UUID; serial consumption continues through the
150second watchdog and actual VM termination, not an early final line. Guest
readiness/release/exit uses bounded exact-operation IPC plus exact child exit
watches, with no replacement/retry launch or kill-based success. Driver lifecycle
completion does not certify child resource settlement. Decoder acceptance
separates structurally complete capture from fully settled accounting and rejects
missing/reserved/duplicate/overflow/faulted/late evidence; absent or unknown
metrics never become zero. Synthetic decoder tests are not kernel evidence.

Observer-only private fault images can be requested independently, for example:

```sh
python3 scripts/x86/p01_capture.py build --out build/p01-full-control --counts 0 --fault full
python3 scripts/x86/p01_capture.py capture --build build/p01-full-control --out build/p01-full-captures --counts 0
```

`--fault` also accepts `contention`, `unready`, and `late`, each gated by the
private kernel trace build; it never changes runtime authority or success.
Malformed/missing/retained/duplicate sidecars are isolated decoder negative
inputs, not fabricated boot outcomes. TwoCPU capture is refused unless
`--cpus 2 --one-cpu-evidence build/p01-captures/matrix.json` references the same
build's twelve independently re-parsed fully settled oneCPU captures. Even then,
this is attribution/concurrency evidence, never distinct-CPU throughput.
