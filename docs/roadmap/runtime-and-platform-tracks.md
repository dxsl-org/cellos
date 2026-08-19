# Runtime and Platform Tracks

**Last updated**: 2026-08-19

This page groups the runtime and higher-level platform overlays that sit next
to the physical hardware tracks.

## Active Native Runtime

- Lua 5.4 is the active native scripting runtime.
- It is the only current native scripting runtime that should be documented as
  supported in-tree.

## Historical Runtime

- MicroPython is historical roadmap text only.
- Do not describe it as a current workspace member, supported runtime, or
  shipping Python path.
- Python workloads belong in the Tier 3 Linux VM path.

## Other Runtime-Adjacent Paths

- The WASM host cell is a tool/runtime-adjacent path for `.wasm` workloads.
- Native Tier 1 remains the Rust-first path; use the platform boundary instead
  of drifting POSIX assumptions into native cells.

## Platform Overlays

- G4 is the planned pure-Rust `std` overlay for Tier 1.
- G5 is the later virtualization-platform overlay.
- Untrusted Linux/POSIX application compatibility stays in Tier 3 VM lanes.
