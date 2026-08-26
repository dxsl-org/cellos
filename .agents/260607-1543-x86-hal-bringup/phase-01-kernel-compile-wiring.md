# Phase 01 — Kernel x86_64 Compile Wiring

**Status:** TODO  
**Priority:** Critical (blocks all other phases)  
**Estimated effort:** Small (4 targeted edits)

---

## Context Links

- `kernel/Cargo.toml` — missing x86_64 dep block
- `kernel/linker-x86-64.ld` — missing Limine request sections
- `kernel/src/boot/limine.rs` — `BASE_REVISION` wrong struct shape; missing delimiters
- `hal/arch/x86/src/lib.rs` — already gates x86_64 correctly (no changes needed)

---

## Overview

The kernel cannot currently compile for `x86_64-unknown-none` because:
1. No `[target.'cfg(target_arch = "x86_64")'.dependencies]` block in `kernel/Cargo.toml`
2. `BASE_REVISION` in `limine.rs` uses a 5-word `LimineBaseRevision` struct — protocol requires `[u64; 3]`
3. No `.requests`, `.requests_start_marker`, `.requests_end_marker` output sections in linker script
4. No delimiter statics in `limine.rs` (Limine rev 2+ requirement)

---

## Requirements

- `cargo check -p vicell-kernel --target x86_64-unknown-none` passes (no errors)
- Limine rev 3 BASE_REVISION wired correctly
- Linker script has all required Limine sections

---

## Architecture

### BASE_REVISION — correct wire format (v8.x rev 3)

```rust
// NOT a struct — the protocol says this is exactly [u64; 3]:
//   [0] = 0xf9562b2d5c95a6c8   (Limine base-revision identifier magic 0)
//   [1] = 0x6a7b384944536bdc   (Limine base-revision identifier magic 1)
//   [2] = 3                     (revision number we support)
#[used]
#[link_section = ".requests"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8,
    0x6a7b384944536bdc,
    3,
];
```

### Delimiter statics (required by rev 2+)

```rust
#[used]
#[link_section = ".requests_start_marker"]
static REQUESTS_START_MARKER: [u64; 2] = [0xf6b8f4b39de7716f, 0xfaa4f786d5a15bc4];

#[used]
#[link_section = ".requests_end_marker"]
static REQUESTS_END_MARKER: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];
```

### Linker script sections to add (between .text and .rodata)

```ld
.requests_start_marker : { KEEP(*(.requests_start_marker)) }
.requests : { KEEP(*(.requests)) }
.requests_end_marker : { KEEP(*(.requests_end_marker)) }
```

---

## Related Code Files

| Action | File |
|--------|------|
| Modify | `kernel/Cargo.toml` |
| Modify | `kernel/linker-x86-64.ld` |
| Modify | `kernel/src/boot/limine.rs` |

---

## Implementation Steps

1. **`kernel/Cargo.toml`** — add x86_64 dependency block after the aarch64 block:
   ```toml
   [target.'cfg(target_arch = "x86_64")'.dependencies]
   hal = { path = "../hal/core", package = "hal-core", default-features = false, features = ["x86_64"] }
   ```

2. **`kernel/linker-x86-64.ld`** — insert after `.text` section, before `.rodata`:
   ```ld
   .requests_start_marker : { KEEP(*(.requests_start_marker)) }
   .requests : { KEEP(*(.requests)) }
   .requests_end_marker : { KEEP(*(.requests_end_marker)) }
   ```

3. **`kernel/src/boot/limine.rs`** — three changes:
   a. Remove `LimineBaseRevision` struct + `BASE_REVISION` static entirely  
   b. Add `LIMINE_BASE_REVISION: [u64; 3]` (as above)  
   c. Add `REQUESTS_START_MARKER` + `REQUESTS_END_MARKER` delimiter statics

4. **Verify**: `cargo check -p vicell-kernel --target x86_64-unknown-none`

---

## Success Criteria

- `cargo check --target x86_64-unknown-none -p vicell-kernel` exits 0 (may have warnings)
- `LimineBaseRevision` struct is gone; flat `[u64; 3]` is the only BASE_REVISION form
- Linker script defines all three `.requests*` sections

---

## Risk Assessment

- **LOW** — pure wiring; no logic. Only risk is linker section ordering (`.text.boot` must come first).
- `BASE_REVISION` struct removal: check that nothing else in `boot/` references `LimineBaseRevision` type — if so, remove that reference too.

---

## Security Considerations

- None — this is build infrastructure only.
