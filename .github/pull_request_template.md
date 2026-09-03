## Summary
<!-- What does this PR change and why? -->

## Related Issues
<!-- Fixes #<issue> / Part of #<issue> -->

## Checklist

### Code Quality
- [ ] Follows the [8 Coding Laws](https://github.com/dxsl-org/cellos/blob/main/docs/code-standards.md#the-8-coding-laws-non-negotiable) (no `mod.rs`, `Vi` prefix on traits, etc.)
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

## Independent Promotion (Only When Required)
<!-- Delete this section when the PR makes no independently ratified, external, or production claim. -->

- Binary question: `Should Cellos promote the bound claim?`
- Bound claim:
- Bound proposal:
- Bound commit/tree:
- Bound evidence URLs:
- Accountable maintainer:
- Required member:

The required repository member must be distinct from the accountable maintainer
and answer `DECISION: YES` or `DECISION: NO` in this PR. Silence, reactions,
AI/CI output, email, and chat do not count. A material change to any bound input
requires a new decision.
