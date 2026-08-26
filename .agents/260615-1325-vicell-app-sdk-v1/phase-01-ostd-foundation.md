# Phase 01 — ostd Foundation: hashbrown + embedded-io

**Status**: 📋 Planned  
**Priority**: P1 — blocker for the rest  
**Estimate**: ~4 hours  
**Parallel**: standalone (no dependencies)

## Context Links

- Codebase: [libs/ostd/Cargo.toml](../../libs/ostd/Cargo.toml) · [libs/ostd/src/io.rs](../../libs/ostd/src/io.rs) · [libs/ostd/src/fs.rs](../../libs/ostd/src/fs.rs) · [libs/ostd/src/prelude.rs](../../libs/ostd/src/prelude.rs)
- Roadmap: "embedded-io traits for ostd" + "HashMap in ostd prelude" (both **~quick win** items in §D)

## Overview

ostd today has no `HashMap` and no standard I/O traits. This blocks:
1. Any no_std crate that uses `embedded_io::Read/Write` (codec libraries, regex engines, etc.)
2. App-layer code that needs an associative container faster than `BTreeMap`

This phase adds both in < 200 LOC total.

## Key Insights

- ostd's `prelude.rs` already exports `Vec`, `String`, `Box` — HashMap is a natural addition
- `hashbrown 0.14` is already a transitive dep (fontdue pulls it in with `hashbrown` feature); adding it directly just pins the version
- `embedded-io 0.6` is the stable version used by most no_std crates (embedded-hal ecosystem)
- `File` already has `read(&mut self, buf: &mut [u8]) -> ViResult<usize>` — the impl is a thin wrapper
- `Stdin::read_line()` works over `&[u8]` — need to add a byte-level `read()` that returns characters

## Requirements

- `hashbrown::HashMap` re-exported as `ostd::collections::HashMap`
- `embedded_io::Read` implemented for `ostd::fs::File`
- `embedded_io::Write` implemented for `ostd::fs::File` (returns `ErrorKind::Unsupported` for read-only)
- `embedded_io::Read` implemented for `ostd::io::Stdin`
- `ostd::prelude` exports `HashMap`
- No breaking changes to existing public API

## Architecture

```
libs/ostd/Cargo.toml
  + hashbrown = { version = "0.14", default-features = false, features = ["alloc"] }
  + embedded-io = { version = "0.6", default-features = false }

libs/ostd/src/collections.rs  (new)
  pub use hashbrown::HashMap;

libs/ostd/src/io.rs  (extend)
  impl embedded_io::ErrorType for File { type Error = ViError; }
  impl embedded_io::Read for File { fn read(&mut self, buf) -> Result<usize, ViError> }
  impl embedded_io::ErrorType for Stdin { type Error = ViError; }
  impl embedded_io::Read for Stdin { fn read(&mut self, buf) -> Result<usize, ViError> }

libs/ostd/src/prelude.rs  (extend)
  + pub use crate::collections::HashMap;
  + pub use embedded_io::{Read as IoRead, Write as IoWrite};

libs/ostd/src/lib.rs  (extend)
  + pub mod collections;
```

## Implementation Steps

1. Add `hashbrown` and `embedded-io` to `libs/ostd/Cargo.toml`
2. Create `libs/ostd/src/collections.rs` — re-export `hashbrown::HashMap` (and `HashSet`)
3. In `libs/ostd/src/io.rs`:
   - Add `impl embedded_io::ErrorType for Stdin`
   - Add `impl embedded_io::Read for Stdin` — reads up to `buf.len()` bytes from stdin (single-byte read loop or line buffer drain)
4. In `libs/ostd/src/fs.rs`:
   - Add `impl embedded_io::ErrorType for File`
   - Add `impl embedded_io::Read for File` — delegates to existing `File::read()`
   - Add `impl embedded_io::Write for File` — returns `Err(ViError::NotSupported)` (read-only for now)
5. In `libs/ostd/src/prelude.rs` — add `HashMap` and `IoRead`/`IoWrite` to prelude
6. In `libs/ostd/src/lib.rs` — add `pub mod collections`
7. `cargo check` — verify zero errors

## Todo List

- [ ] Add hashbrown + embedded-io to Cargo.toml
- [ ] Create collections.rs (HashMap + HashSet)
- [ ] Impl embedded_io::Read for Stdin in io.rs
- [ ] Impl embedded_io::Read + Write for File in fs.rs
- [ ] Extend prelude.rs
- [ ] Add pub mod collections to lib.rs
- [ ] cargo check passes

## Success Criteria

- `use ostd::collections::HashMap;` works in any cell without adding deps
- `use ostd::prelude::*;` pulls in `HashMap`, `IoRead`, `IoWrite`
- An external no_std crate that bounds on `embedded_io::Read` accepts `ostd::fs::File`
- `cargo check` clean

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| hashbrown version conflict with fontdue's transitive dep | Pin `hashbrown = "0.14"` explicitly; workspace dedup resolves |
| embedded-io 0.6 `ErrorType` associated type mismatch | ViError implements `Debug` (required bound); verify once |
| Stdin read semantics (line-buffered vs byte) | Implement byte-level read draining internal line buffer; document line-buffered limitation |

## Security Considerations

None — purely additive, no new syscalls or trust boundaries.
