# CP1: Phase 03 slice 3, benchmark counters and Tier-2 echo peer

Status: APPROVED by the user 2026-11-01 (A and B). The user chose the direction the same day: "Feature bench-counters + in serial" and "Echo peer Tier-2".

Source facts: agent://Cp3Scout2 (read-only survey, file:line citations).

## A. `bench-counters` kernel feature

**Off by default.** No production, canonical or test-hooks image enables it. Without the feature none of this code is compiled.

### Counters

Global `AtomicU64`s with Relaxed ordering, cfg-gated, in a new `kernel/src/bench_counters.rs`:

| Counter | Site |
|---|---|
| `syscalls` | `ViCell_syscall_dispatch` (`task/syscall.rs:9930`). All architectures meet here. |
| `domain_syscall_cr3` | Same dispatch point, when the saved user CR3 ≠ `kernel_cr3()`. This counts the two SYSCALL-path CR3 writes that the HAL root counter misses (x86 `syscall.rs:314-325`, 359-367). Not counted on other architectures. |
| `root_programmings` | Kernel side of `SwitchPlan::root_switch` at each `observe_switch_activation` (`task/domain_switch.rs:135,149,154`). |
| `context_switches` | `note_task_dispatch` (`task.rs:1303`), plus the matching point on the non-native-domains path. |
| `ipc_bytes_copied` | The IPC copy sites only: `ipc_send` copy-in, `ipc_recv` / `Recv` copy-out, async submit/terminal/take. Not the generic `TaskCopyView`, which would over-count. |
| `heap_allocs`, `heap_alloc_bytes` | The `QuotaAlloc::alloc_impl` success branch (`memory/heap.rs`). Already IRQ-masked. |

### Read path

Raw opcode **504 `BenchCounters`** (`a0` = user buffer, `a1` = length):
- It lives **outside** `ViSyscall`, like 500–503, which `syscall.rs:9747` documents as the way to avoid a Law-1 enum change. There is a `map_syscall` arm and a `known_raw` entry, both under `cfg(feature = "bench-counters")`.
- Gating: the MemInfo allowlist bit 56 plus the `observe_memory` right, the same as `MemInfo`.
- It copies a fixed versioned `[u64; 8]` record (version, then the counters) to the caller.
- Without the feature, 504 is unknown and denied, exactly as today.

### Serial output

`bench` reads the counters before and after each scenario and prints one JSON line per scenario:

```json
{"name":"<scenario>.counters","tier":"sas|domain","path":"copy|ring|...","syscalls":N,"domain_syscall_cr3":N,"root_programmings":N,"context_switches":N,"ipc_bytes_copied":N,"heap_allocs":N,"heap_alloc_bytes":N}
```

When the opcode is refused, every value is `"unknown"`, never zero.

### Tooling

- The collector records the kernel feature list in the capture.
- Captures with and without `bench-counters` are separate compatibility groups, because the counters cost time.
- Counter records are informational. They are never budget-gated by themselves, but a before/after diff shows added copies, allocations or CR3 writes separately per tier.
- The x86 lifecycle builder gains `X86_KERNEL_FEATURES` (default empty). Its existing `S22-X86-DOMAIN-LIVE` refusal stays, so `test-hooks` can never ride along.

## B. Tier-2 echo peer

- **Binary.** New `[[bin]] bench-peer-t2` in `cells/tests/bench` that reuses the existing echo loop. Its manifest is `tier = PROTECTION_CLASS_UNTRUSTED`, with minimal `declare_syscalls![Send, Recv, Log, Exit, Yield]`. The template is `c2c-render-domain-worker`.
- **Launch edge.** One new row in `launch_profile/profiles.rs` `pinned_profile`: `("bench", "/bin/bench-peer-t2") => LEGACY`, route `Pinned`, no lifecycle authority, the same shape as the existing `bench-probe` row. Domain admission accepts `LEGACY` (`unenforceable_authority` ignores observe/mutate).
- **Policy.** A `sign-policy.py` parity row `("/bin/bench-peer-t2", 0,0,0,0,0,0,0,0,0)`. This changes the policy digest; the digest pinned in KSSH K8 CP1 is already stale (noted in the P1 commit).
- **Packaging.** Add the binary to `WORKLOAD_CELLS` in `scripts/build-x86_64-c2c-lifecycle-ci.sh`, which packs, signs and checks it. RV64 `gen-disk-ci.sh` rows are optional and not in scope.
- **Scenario.** New `ipc_send_recv_domain`: the same 64-byte request and 1-byte reply as `ipc_send_recv`, with the Tier-2 peer. It is labeled `tier=domain, path=copy` and has its own metric name, so SAS and domain numbers are never merged.
  - If the peer cannot be spawned or admitted, the scenario emits a `bench_event:invalid` record with that stage; it does not silently fall back to a SAS peer.
  - It is added to the x86 profile contract only. RV64 keeps its contract.

## Observable deltas (Law 1)

- `bench-counters` builds only: a new raw opcode 504. Production builds see no change.
- All builds: a new launch edge `bench` → `/bin/bench-peer-t2` (Pinned, LEGACY), so the `bench` cell may now start a Tier-2 domain cell.
- The bench JSON gains new record names. The collector contract is updated in the same change.

## Evidence plan

- Host tests:
  - the counter record layout;
  - opcode 504 is unknown without the feature (a `map_syscall` test);
  - a launch-profile test for the new edge;
  - collector tests: counter records are parsed, a feature mismatch is incompatible, and "unknown" is preserved.
- x86 QEMU:
  - TCG plus ≥3 KVM captures with `bench-counters`;
  - `ipc_send_recv_domain` runs with `[domain] admitted cell 'bench-peer-t2' … (CR3 isolation)`;
  - the counter deltas show `domain_syscall_cr3 > 0` for the domain scenario and `= 0` for the SAS scenarios.
- No-feature check: the default x86 and RV64 kernels are bit-for-bit independent of the feature. `bench` reports counters as "unknown" there.
- Regression lanes: x86 `x86_64-boot`, `ahci-x86` and `nvme-x86`; RV64 `tier2-fault-isolation`; kernel host tests.

Rollback: revert the commit; the feature is off by default.
