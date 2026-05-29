# ViOS Security Model

**Version:** v0.2.1-dev | **Updated:** 2026-05-29

## Design Philosophy

ViOS uses a **Cellular Single Address Space (SAS)** model with
Language-Based Isolation (LBI) via Rust's type system.  Traditional OS
security relies on hardware MMU separation between processes; ViOS instead
relies on:

1. **Rust ownership + borrow checker** — prevents spatial/temporal memory bugs
2. **Capability tokens (`CapId`)** — unforgeable, kernel-managed access rights
3. **`#![forbid(unsafe_code)]` on Cells** — enforced by `cargo-geiger` in CI
4. **VFS access through capabilities** — no direct file-descriptor integers

## STRIDE Threat Model

### Spoofing
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell forges another Cell's CellId in IPC | Kernel verifies sender ID from TCB on every message; user cannot inject arbitrary sender values | ✅ Mitigated |
| Cell constructs a valid CapId by guessing | CapIds are kernel-assigned opaque u64 values; 64-bit ID space makes guessing infeasible | ✅ Mitigated |
| Malformed ELF binary spawns as a different Cell | ELF header validated before execution; Cell registry assigns IDs monotonically | ✅ Mitigated |

### Tampering
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell writes to another Cell's memory | SAS + Rust ownership; no `unsafe` in cells/ | ✅ Mitigated |
| Cell modifies a revoked capability | Kernel removes CapId from table on Close; subsequent ops return PermissionDenied | ✅ Mitigated |
| Attacker modifies disk image to inject malicious ELF | ELF is checksummed at load time (TODO: Phase 12 adds SHA256 verification) | 🔶 Partial |

### Repudiation
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell claims it did not send an IPC message | Sender ID in TCB is set by kernel on message delivery; cannot be forged | ✅ Mitigated |
| Audit log missing | No audit log yet; planned for v1.x | ❌ Deferred |

### Information Disclosure
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Kernel leaks pointers to user-mode | Kernel zeroes new frames before mapping; TrapFrame zeroes scratch regs on EL0 return | ✅ Mitigated |
| Spectre / Meltdown side-channel | Known limitation; not mitigated in v1.0.  See `known_limitations` below | ⚠️ Known |
| File content readable without capability | VFS returns data only to cells holding a valid `CapId` with READ permission | ✅ Mitigated |

### Denial of Service
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell allocates unbounded memory | Frame allocator has a hard cap (total usable RAM); OOM kills the cell | ✅ Mitigated |
| Cell floods IPC queue | Message queue is bounded; sender blocks when full (future Phase 20) | 🔶 Partial |
| Lua/MicroPython script infinite loop | Cell exit triggered by kernel timeout (future scheduler enhancement) | ❌ Deferred |

### Elevation of Privilege
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell executes privileged instruction (e.g., `wfi`) | Cells run in EL0/Ring3; trap dispatched to kernel | ✅ Mitigated |
| `#[allow(unsafe_code)]` in a Cell | `cargo-geiger` CI gate fails if any Cell contains `unsafe`; zero-tolerance policy | ✅ Mitigated |
| Malformed syscall arguments overflow kernel buffers | All syscall arg lengths validated via `validate_user_buf` before dereference | ✅ Mitigated |

## Known Limitations

1. **Spectre v1/v2:** The SAS model means kernel and all Cells share a
   virtual address space.  Spectre-class microarchitectural leakage is
   inherent.  Mitigation (retpoline, IBRS, CSR flushing) is deferred to
   Phase 12 hardening.

2. **No KASLR:** Kernel is loaded at a fixed address by Limine.  Planned
   for v1.x.

3. **Trusted Cells:** All installed Cells are fully trusted.  There is no
   sandbox for untrusted Cells in v1.0.  See Phase 23 for community
   submission review gates.

4. **No audit log:** Cell actions are not persistently logged.

## Defense in Depth

| Layer | Mechanism |
|-------|-----------|
| Language | `#![forbid(unsafe_code)]` on all Cell crates |
| Compile-time | Rust ownership, borrow checker, lifetimes |
| Kernel | Capability table, syscall argument validation, frame zeroing |
| CI | `cargo-geiger`, `cargo-audit`, `cargo-deny` on every PR |
| Fuzzing | Weekly libFuzzer harnesses on ELF parser + VFS path validator |

## Security Contacts

For vulnerability reports, open a GitHub Issue with label `security`.
Critical issues (RCE, privilege escalation): email directly (see SECURITY.md).
