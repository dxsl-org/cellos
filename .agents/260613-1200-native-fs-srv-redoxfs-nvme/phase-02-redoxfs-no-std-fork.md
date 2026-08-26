# Phase 02 — RedoxFS no_std Fork

**Status**: Planned
**Priority**: High — unblocks Phase 03
**Parallel with**: Phase 01

---

## Context Links

- ADR: `docs/specs/09b-vfs-native-fs-adr.md` (chose RedoxFS, explains rationale)
- Existing C-FFI crate pattern: `cells/services/vfs/src/lfs_disk.rs` (littlefs2 adapter — mirror this)
- RedoxFS source: `github.com/redox-os/redoxfs` — pin to commit `HEAD` of tag `0.9.0`

---

## Overview

Vendor-fork RedoxFS 0.9.0 into `third_party/redoxfs/`, patch the single blocker (`libc` is
an unconditional Cargo dep despite being used only in FUSE/std-gated code), and verify the
no_std core compiles in the ViCell workspace.

No ViCell-specific logic goes here. This phase produces only a compilable no_std crate.

---

## Requirements

- `cargo check -p redoxfs --no-default-features --target riscv64gc-unknown-none-elf` passes
- No changes to the RedoxFS library API (Disk trait, FileSystem struct) — we adapt to it
- Fork is vendored locally; all crypto/compression deps use `default-features = false`
- Pin to a specific commit hash in `Cargo.toml` comment for future upgrade guidance

---

## The One Hard Blocker

`Cargo.toml` line:
```toml
libc = "0.2"   # unconditional — appears in std-gated FUSE modules only
```

Fix (fork diff):
```toml
# Before
libc = "0.2"

# After
libc = { version = "0.2", optional = true }

# Under [features]
std = [..., "dep:libc"]
```

Verify `grep -r "libc::" src/` — expected results: `src/mount/`, `src/unmount/`, `src/archive/`
(all already `#[cfg(feature = "std")]`). If `libc::` appears in non-std-gated code, it must be
replaced with `core`/`alloc` equivalents or removed.

---

## All Cargo Dependencies — no_std Audit

| Dep | Version | no_std | Action |
|-----|---------|--------|--------|
| `aes` | 0.8 | ✅ native | `default-features = false` |
| `argon2` | 0.4 | ✅ native | `default-features = false` |
| `lz4_flex` | 0.11 | ✅ block format | `default-features = false` (disables `frame` feature) |
| `seahash` | 4.1 | ✅ (inferred) | verify `cargo tree --no-default-features` |
| `xts-mode` | 0.5 | ✅ native | `default-features = false` |
| `bitflags` | 2 | ✅ | `default-features = false` |
| `uuid` | 1.4 | ✅ | `default-features = false`; disable `v4` (needs getrandom) |
| `endian-num` | 0.1 | ✅ | no action |
| `libc` | 0.2 | ❌ blocker | make optional (see above) |
| `libredox` | any | ❌ Redox-OS only | already `cfg(target_os = "redox")` — confirm |

---

## Related Code Files

| File | Action |
|------|--------|
| `third_party/redoxfs/` | Create — vendored fork |
| `third_party/redoxfs/Cargo.toml` | Patch `libc` optional; all deps `default-features = false` |
| `third_party/redoxfs/src/` | Copy verbatim (no functional changes) |
| `Cargo.toml` (workspace) | Add `redoxfs = { path = "third_party/redoxfs", default-features = false }` |
| `third_party/README.md` | Create — note upstream source, commit hash, patch summary |

---

## Implementation Steps

1. **Vendor the crate**:
   ```powershell
   mkdir third_party
   git clone https://github.com/redox-os/redoxfs third_party/redoxfs
   cd third_party/redoxfs
   git checkout <commit-hash>   # pin to 0.9.0 release commit
   rm -rf .git                  # not a submodule — full vendor
   ```

2. **Patch `Cargo.toml`**:
   - `libc` → optional (see above)
   - All other deps: add `default-features = false`
   - `uuid`: add `features = []` (no v4 generation; RedoxFS uses UUID for volume ID,
     but the `Filesystem::create()` call that generates it is `std`-gated anyway)
   - Add `[features] std = ["dep:libc", "uuid/v4", ...]` and set as non-default

3. **Grep audit**: `grep -rn "std::" third_party/redoxfs/src/ | grep -v "#\[cfg(feature"` —
   every `std::` hit outside a `#[cfg(feature = "std")]` block must be replaced with
   `core::` / `alloc::` equivalent.

4. **Add to workspace**:
   ```toml
   # Cargo.toml [workspace.members]
   "third_party/redoxfs"

   # Or as path dep consumed by service-vfs:
   redoxfs = { path = "third_party/redoxfs", default-features = false }
   ```

5. **Verify**:
   ```powershell
   cargo check -p redoxfs `
       --no-default-features `
       --target riscv64gc-unknown-none-elf `
       -Z build-std=core,alloc
   ```

6. **Write `third_party/README.md`** — upstream URL, commit hash pinned, patch description.

---

## Todo

- [ ] Clone and vendor `redoxfs` source into `third_party/redoxfs/`
- [ ] Patch `Cargo.toml`: `libc` optional, all deps `default-features = false`
- [ ] Run `grep -rn "std::" src/` audit; fix any hits outside `#[cfg(feature = "std")]`
- [ ] Confirm `libredox` is `cfg(target_os = "redox")`-only — remove or gate if not
- [ ] Add to workspace and run `cargo check` clean for riscv64 no_std
- [ ] Write `third_party/README.md` with upstream commit hash

---

## Success Criteria

- `cargo check -p redoxfs --no-default-features --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` exits 0
- `cargo tree -p redoxfs --no-default-features` shows no `std` dependencies
- `grep -c "libc::" third_party/redoxfs/src/**/*.rs` = 0 (no libc usage in non-std path)
- `third_party/README.md` records the upstream commit hash

---

## Risk

**Undocumented `std::` uses in non-gated modules**: the `grep` audit may reveal subtle `std::io`
or `std::collections` usage in core modules (e.g. `BTreeMap` — which IS in `alloc::collections`).
These are straightforward substitutions (`std::collections::BTreeMap` → `alloc::collections::BTreeMap`).

**`uuid` crate v4 generation**: if `Filesystem::create()` is called at runtime without a pre-seeded
UUID, it panics in no_std (no entropy source). Mitigation: pass a caller-supplied UUID when creating
a new RedoxFS volume — the ADR specifies format the image offline with `redoxfs-mkfs` (Linux tool)
and mount it read-write from ViCell; we never call `Filesystem::create()` from the kernel.
