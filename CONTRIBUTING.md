# Contributing to ViOS

Welcome! ViOS is a `no_std` Rust OS with a Cellular Single Address Space architecture.

## Quick Start

### Prerequisites
- Rust nightly (pinned in `rust-toolchain.toml`)
- QEMU with RISC-V support: `qemu-system-riscv64`
- RISC-V cross-compiler: `riscv-none-elf-gcc` (xpack release)
- Python 3.10+ (for disk image tooling)

### Build
```bash
cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
```

### Run
```powershell
./run.ps1
```
or
```bash
bash scripts/run-aarch64.sh  # AArch64
bash scripts/run-x86-64.sh  # x86_64
```

## Code Standards

1. **Law 4 (unsafe):** Every `unsafe` block requires `// SAFETY:` explaining the invariant.
2. **Law 5 (module style):** Use `foo.rs` + `foo/` — never `mod.rs`.
3. **Law 6 (naming):** Public traits get the `Vi` prefix (`ViFileSystem`, `ViDriver`).
4. **YAGNI / KISS / DRY** — see `docs/code-standards.md`.

## Submitting a PR

1. Fork and create a feature branch.
2. Write code; run `cargo check --workspace` (must be zero warnings).
3. Add a test or document why one isn't needed.
4. Submit PR — the CI template will guide you through the checklist.

## Where to Start

Look for issues labelled [`good-first-issue`](../../issues?q=label%3Agood-first-issue).

## Questions

Open a Discussion or join the project chat (see README for links).
