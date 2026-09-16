---
phase: 4
title: "Unsupported Families & Boundary Shims"
status: pending
priority: P2
effort: "1d"
dependencies: [1, 2]
tier: medium
---

# Phase 04: Unsupported Families & Boundary Shims

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview
Implements clean, fail-closed shims for unsupported API families in Tier 1 Cells, preventing ambient authority escalation while ensuring compliance with Rust `std` trait contracts.

## Requirements
- Functional:
  - Filesystem (`fs.rs`): Operations return `io::ErrorKind::Unsupported` or `PermissionDenied`. No ambient cwd/temp/root paths.
  - Environment (`env.rs`): Environment variable lookups and mutations return `io::ErrorKind::Unsupported`. Define explicit `std::env::consts::OS = "cellos"` (`PAL-035`).
  - Network (`net.rs`): Socket creation and DNS resolution return `io::ErrorKind::Unsupported`.
  - Process (`process.rs`, `pipe.rs`): Child process spawning returns `io::ErrorKind::Unsupported`.
  - Thread Concurrency: Thread creation returns `io::ErrorKind::Unsupported`.
  - Personality: Supply aborting C-unwind personality symbol `rust_eh_personality` returning abort without unwinder linkage.
- Non-functional:
  - No synthetic success (e.g. returning empty files or fake empty environments).
  - Every unsupported function fails with an explicit identifiable error code.

## Architecture
```text
std::fs::File::open("...")   ──► io::Error::from(io::ErrorKind::Unsupported)
std::env::var("FOO")         ──► io::Error::from(io::ErrorKind::Unsupported)
std::net::TcpStream::connect ──► io::Error::from(io::ErrorKind::Unsupported)
std::process::Command::spawn ──► io::Error::from(io::ErrorKind::Unsupported)
```

## Assumptions
- **Claim:** Rust `std` allows platforms to implement `io::ErrorKind::Unsupported` across networking, filesystem, and processes.
  **Confidence:** high
  **How to verify:** `sys/pal/unsupported/` in upstream Rust standard library uses this exact pattern.

## Related Files
- Modify: `patches/rust-std-cellos.patch`
- Create in patch: `library/std/src/sys/pal/cellos/fs.rs`
- Create in patch: `library/std/src/sys/pal/cellos/env.rs`
- Create in patch: `library/std/src/sys/pal/cellos/net.rs`
- Create in patch: `library/std/src/sys/pal/cellos/process.rs`
- Create in patch: `library/std/src/sys/pal/cellos/pipe.rs`

## Implementation Steps
1. Implement `fs.rs`:
   - Stub `File`, `OpenOptions`, `DirBuilder`, `ReadDir` returning `io::ErrorKind::Unsupported`.
2. Implement `env.rs`:
   - Stub `env::var`, `env::set_var`, `env::remove_var` returning `io::ErrorKind::Unsupported`.
   - Add `pub const OS: &str = "cellos";` and `pub const FAMILY: &str = "";`.
3. Implement `net.rs`:
   - Stub `TcpStream`, `TcpListener`, `UdpSocket`, `LookupHost` returning `io::ErrorKind::Unsupported`.
4. Implement `process.rs` and `pipe.rs`:
   - Stub `Process`, `Command` returning `io::ErrorKind::Unsupported`.
5. Implement `personality.rs`:
   - Provide minimal aborting personality stub for compiler linking.

## Success Criteria
- [ ] Attempts to call `std::fs::read` fail cleanly with `ErrorKind::Unsupported` without panicking.
- [ ] `std::env::consts::OS` evaluates to `"cellos"`.
- [ ] Binary links successfully with `panic=abort` without unresolved personality or unwind symbols.

## Security Considerations
Fail-closed behavior guarantees that untrusted or unreviewed crates using standard library APIs cannot bypass CellOS capability gates.

## Risk Notes
Crates depending on `std::env::var` for configuration must handle `Err` gracefully or use CellOS argument injection.

## Deviation Log
None.
