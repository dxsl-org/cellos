## Summary
<!-- What does this PR change and why? -->

## Related Issues
<!-- Fixes #<issue> / Part of #<issue> -->

## Checklist

### Code Quality
- [ ] Follows the [8 Coding Laws](CLAUDE.md) (no `mod.rs`, `Vi` prefix on traits, etc.)
- [ ] No `unsafe` block without a `// SAFETY:` comment
- [ ] No `[profile.*]` in sub-crate `Cargo.toml` — profiles live at workspace root
- [ ] No new lint warnings (`cargo clippy -- -D warnings` clean)

### Testing
- [ ] Tested on QEMU `qemu-system-riscv64 -machine virt`
- [ ] New logic covered by unit or integration tests
- [ ] `cargo check --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` passes

### Documentation
- [ ] Public APIs documented with `///` rustdoc
- [ ] `docs/` updated if architecture or interfaces changed
- [ ] `CHANGELOG.md` entry added (if user-visible change)

### Security
- [ ] No secrets, credentials, or API keys committed
- [ ] Cells remain `#![forbid(unsafe_code)]`
