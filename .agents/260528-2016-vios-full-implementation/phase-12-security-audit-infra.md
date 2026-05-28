# Phase 12 — Security Audit Infrastructure

**Effort:** 80h | **Priority:** P1 | **Status:** pending | **Blockers:** Phase 02

## Overview

Establish security tooling: continuous advisory scanning, `unsafe`-regression gates on cells, STRIDE threat model documentation, host-side libFuzzer harnesses for parsers, and Kani formal-verification harnesses for critical invariants (lease lifetime, capability revocation, memory quota). Ships a defensible security posture before community PRs open in Phase 23.

## Context Links

- `docs/code-standards.md` — Law 4 (unsafe management), Law 7 (trait objects)
- `docs/02-memory.md` — capabilities, leases, quotas
- Phase 02 already runs `cargo-deny` + `cargo-audit` — this phase extends with geiger, fuzzing, Kani
- Phase 07 introduced capabilities — invariants checked here

## Key Insights

- `cargo-geiger` counts unsafe expressions per crate. The discipline: cells/ must stay at zero; hal/ + kernel/ may use unsafe with `// SAFETY:` annotations. A CI check "fail if cells/ contains any unsafe" enforces Law 4.
- Fuzz harnesses run on the **host** with libFuzzer (`cargo fuzz`), targeting parsers that consume external bytes (ELF, VFS paths, VirtIO descriptors). Cannot fuzz the kernel itself directly; fuzz isolated parsing logic that's also pulled into a `std` library shim.
- Kani is a bounded model checker for Rust. It proves properties for bounded loops/inputs. Cost: harnesses are slow (minutes per property). Use sparingly on critical invariants.
- STRIDE: Spoofing, Tampering, Repudiation, Information disclosure, DoS, Elevation of Privilege. Document each per subsystem; SAS is honest about Spectre-class side-channels as a known limitation.

## Requirements

**Functional**
- Weekly CI runs: cargo-audit, cargo-deny, cargo-geiger, all 3 fuzz harnesses (corpus replay), Kani harnesses
- Per-PR CI: cargo-geiger regression gate (fail if cells/ unsafe count > 0)
- `docs/security-model.md` documents threat model + known limitations
- 3 fuzz harnesses with seed corpora committed
- 3 Kani harnesses for critical invariants

**Non-functional**
- Weekly security workflow wall-time < 30 min
- Per-PR gate < 2 min added to existing CI
- No false-positive churn (annotate exceptions in `deny.toml` with reason + expiry)

## Architecture

```
Per-PR gate (.github/workflows/ci.yml):
   ├── cargo deny check        ← already in Phase 02
   ├── cargo audit --deny      ← already in Phase 02
   └── geiger regression       ← NEW; fail if cells/ unsafe > 0

Weekly (.github/workflows/security.yml):
   ├── cargo audit (full)
   ├── cargo deny (licenses, bans, advisories)
   ├── cargo geiger (full report, upload JSON artifact)
   ├── cargo fuzz run elf_parser     -- -max_total_time=300
   ├── cargo fuzz run vfs_path       -- -max_total_time=300
   ├── cargo fuzz run virtio_desc    -- -max_total_time=300
   └── cargo kani --harness lease_lifetime
                  --harness cap_revocation
                  --harness memory_quota_bound
```

## Related Code Files

**Create:**
- `docs/security-model.md` — STRIDE breakdown, SAS limitations, defense-in-depth
- `docs/known-issues.md` — list of accepted-risk items (Spectre, etc.)
- `fuzz/Cargo.toml` — cargo-fuzz package
- `fuzz/fuzz_targets/elf_parser.rs` — fuzz ELF byte input → parser
- `fuzz/fuzz_targets/vfs_path.rs` — fuzz path string → canonicalize/resolve
- `fuzz/fuzz_targets/virtio_desc.rs` — fuzz VirtIO descriptor ring bytes → driver state machine
- `fuzz/corpus/elf_parser/*` — seed corpus (small valid ELFs)
- `fuzz/corpus/vfs_path/*` — seed paths (normal, edge, malicious)
- `fuzz/corpus/virtio_desc/*` — recorded happy-path descriptor sequences
- `verification/Cargo.toml` — Kani harness crate
- `verification/src/lib.rs` — `#[cfg(kani)]` proofs
- `verification/src/lease_lifetime.rs` — proof: lease expiry strictly enforced
- `verification/src/cap_revocation.rs` — proof: revoked cap never resolvable
- `verification/src/memory_quota.rs` — proof: per-cell quota never exceeded
- `scripts/run-geiger-gate.sh` — extracts cells/ unsafe count, exits nonzero if > 0
- `.github/SECURITY.md` — vulnerability disclosure policy

**Modify:**
- `.github/workflows/security.yml` — extend with geiger, fuzz, Kani jobs
- `.github/workflows/ci.yml` — add geiger gate per-PR
- `deny.toml` — add geiger threshold note, document any allow exceptions
- `CONTRIBUTING.md` (created in Phase 19) — link to security policy
- `kernel/src/loader/elf.rs` — extract pure parsing into a sub-module reachable from host-target fuzz harness

## Implementation Steps

1. **Write STRIDE doc `docs/security-model.md`**:
   - For each subsystem (kernel, HAL, VFS, IPC, capability registry, network, input, compositor): table of S/T/R/I/D/E threats + mitigations
   - Section "Known limitations": Spectre-class side-channels in SAS; no MMU-level isolation between cells; documented as informed acceptance for v1.0
2. **Refactor ELF parser** so it's host-compilable:
   - Move parse-only logic in `kernel/src/loader/elf.rs` into `kernel/src/loader/elf_parse.rs` with no kernel dependencies
   - Add `[features] fuzz = []` to `kernel/Cargo.toml`; gate any kernel-only code with `#[cfg(not(feature = "fuzz"))]`
3. **Set up cargo-fuzz package**:
   - `cargo install cargo-fuzz`
   - `cd fuzz && cargo fuzz init` creates layout
   - Add to workspace exclusion list (fuzz uses `std`, not `no_std`)
4. **Fuzz target `elf_parser.rs`**:
   ```rust
   #![no_main]
   use libfuzzer_sys::fuzz_target;
   fuzz_target!(|data: &[u8]| {
       let _ = kernel::loader::elf_parse::parse(data);
   });
   ```
5. **Fuzz target `vfs_path.rs`**:
   ```rust
   fuzz_target!(|s: &str| {
       let _ = libs_api::vfs_path::canonicalize(s);
   });
   ```
6. **Fuzz target `virtio_desc.rs`**:
   ```rust
   fuzz_target!(|data: &[u8]| {
       let _ = kernel::task::drivers::virtio_blk::parse_used_ring(data);
   });
   ```
7. **Build seed corpora**:
   - For ELF: include a minimal valid riscv64 ELF (~200 bytes), a malformed truncated header, a header with bogus phdr count
   - For paths: `/`, `/bin/`, `/../../etc`, embedded NULs, very long names, Unicode
   - For VirtIO: dumps captured from Phase 04 trace logs
8. **Run fuzz locally**:
   - `cargo fuzz run elf_parser -- -max_total_time=60`
   - Fix any panics / crashes reported (record as bugs first, then fix)
   - Promote crash inputs into corpus
9. **Set up Kani**:
   - Create `verification/` crate at workspace root, excluded from default workspace members
   - Use `kani-verifier` install instructions; pin a version in `verification/README.md`
10. **Kani harness `lease_lifetime.rs`**:
    ```rust
    #[kani::proof]
    fn lease_never_resolves_after_expiry() {
        let now: u64 = kani::any();
        let expires_at: u64 = kani::any();
        kani::assume(expires_at <= now);
        let lease = Lease { expires_at, /* … */ };
        assert!(lease.is_valid(now).is_err());
    }
    ```
11. **Kani harness `cap_revocation.rs`** + **`memory_quota.rs`** with similar structure: model a small bounded state machine, assert the invariant always holds.
12. **Write `scripts/run-geiger-gate.sh`**:
    ```bash
    #!/usr/bin/env bash
    set -euo pipefail
    cargo geiger --workspace --output-format json > geiger.json
    UNSAFE_IN_CELLS=$(jq '[.packages[] | select(.package.id | contains("cells/")) | .unsafety.used.expressions] | add // 0' geiger.json)
    if [ "$UNSAFE_IN_CELLS" -gt 0 ]; then
        echo "REGRESSION: $UNSAFE_IN_CELLS unsafe expressions in cells/"; jq '...' geiger.json; exit 1
    fi
    echo "OK: zero unsafe in cells/"
    ```
13. **Extend CI** to call geiger gate per-PR, full security workflow weekly.
14. **Write `.github/SECURITY.md`**:
    - How to report a vulnerability (private email or GitHub security advisory)
    - Disclosure timeline (90 days standard)
    - Supported versions
    - PGP key (optional for v1.0)

## Todo List

- [ ] Write `docs/security-model.md` (STRIDE per subsystem)
- [ ] Write `docs/known-issues.md` (Spectre, etc.)
- [ ] Refactor `elf.rs` to extract `elf_parse.rs` (host-compilable)
- [ ] Set up `fuzz/` package with cargo-fuzz
- [ ] Write 3 fuzz harnesses (elf_parser, vfs_path, virtio_desc)
- [ ] Build seed corpora (≥5 inputs each)
- [ ] Run each fuzz locally for 60s, fix any panics
- [ ] Set up `verification/` Kani crate
- [ ] Write 3 Kani harnesses (lease, cap, quota)
- [ ] Write `scripts/run-geiger-gate.sh`
- [ ] Extend `.github/workflows/ci.yml` with geiger gate
- [ ] Extend `.github/workflows/security.yml` with fuzz + Kani jobs
- [ ] Write `.github/SECURITY.md`
- [ ] Link security policy from README + CONTRIBUTING

## Success Criteria

- Weekly security CI passes; 0 HIGH/CRITICAL advisories outstanding
- Per-PR CI fails if any unsafe is introduced in cells/
- 3 fuzz harnesses build, run, accept seed corpus
- 3 Kani harnesses pass (or document failure as accepted with bug filed)
- `docs/security-model.md` covers all major subsystems
- Vulnerability disclosure path documented

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `unsafe` becomes unavoidable in a cell (e.g., wasm runtime FFI) | Med | Med | Carve a documented exception in geiger script with annotation `// SAFETY:` and bug-tracking the path out |
| Kani harnesses take longer than weekly CI window | Low | Low | Run on bigger runner (or self-host); if minutes is too long, downgrade to monthly |
| Fuzz finds crashes faster than we can fix | High | Low | Triage queue with severity; ship known-non-exploitable as documented bugs |
| ELF parser refactor breaks loader | Med | Med | Phase 11 test suite catches it; bisect via per-commit CI run |
| Threat model doc becomes outdated as features land | Cert | Low | Each new phase adds a "Security Considerations" section (already in template); roll up into security-model.md at v1.0 prep |

## Security Considerations

- Phase 12 is fundamentally meta-security — it sets up the apparatus, not new features
- Vulnerability disclosure timeline: 90 days standard; document in `.github/SECURITY.md`
- Be honest about what we DON'T defend against in v1.0: Spectre, Rowhammer, malicious hardware

## Rollback

Security tooling additions are independently reversible. CI gate can be set to non-blocking (informational) if a fix is in flight. STRIDE doc stays even on rollback (pure documentation).

## Next Steps

Phase 19 (docs automation) publishes the security policy to GitHub Pages. Phase 23 (community) references SECURITY.md in CONTRIBUTING. Every subsequent phase adds at least one fuzz/Kani test if it touches parsing or invariants.
