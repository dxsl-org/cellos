# Phase 02 — Manifest `hypervisor` Flag + Loader Grant

**Status:** ✅ Done  
**Priority:** Medium — depends on Phase 01  
**Blocked by:** Phase 01 complete  
**⚠️ LAW 1: `libs/api/` CHANGE — REQUIRES 2× USER CONFIRMATION BEFORE IMPLEMENTATION**

---

## Context Links

- Manifest macro: [libs/api/src/manifest.rs](../../libs/api/src/manifest.rs)
- Loader grant logic: [kernel/src/loader.rs](../../kernel/src/loader.rs) (spawn-time cap grant ~147–196)
- Cap definition (Phase 01): [kernel/src/task/cap.rs](../../kernel/src/task/cap.rs)
- TCB (Phase 01): [kernel/src/task/tcb.rs](../../kernel/src/task/tcb.rs)
- cpu_features (Phase 01): `kernel/src/cpu_features.rs` (new)

---

## ⚠️ Law 1 Gate

This phase modifies `libs/api/src/manifest.rs` — the stable ABI between kernel and Cells.

**Required before any implementation:**
1. First confirmation: user acknowledges this touches a Law 1 file
2. Second confirmation: user approves the specific change (backward-compat 6-param arm)

Do NOT implement until BOTH confirmations are received.

---

## Overview

Two changes — both backward-compatible:

1. **`libs/api/src/manifest.rs`** — add `hypervisor: bool` to `CellManifest` struct; add `has_hypervisor()` accessor; add a new 6-param `declare_manifest!` arm (`hypervisor = …`). All existing 3-param and 5-param call sites are unchanged (new param defaults to `false`).

2. **`kernel/src/loader.rs`** — at spawn time, check `manifest.has_hypervisor()` AND `cpu_features::has_h_ext()`; if both true, grant `HypervisorCap::new()` to the new TCB. This mirrors the existing `block_io`, `network`, `spawn` grant pattern exactly.

---

## Key Insights

1. **Backward compatibility**: Existing `declare_manifest!(block_io = false, network = false, spawn = true)` (3-param) and `declare_manifest!(block_io = false, network = true, spawn = false, gpio = true, uart = false)` (5-param) are not affected — the new arm adds a 6th param `hypervisor = …`.  The `CellManifest` struct must default `hypervisor: false` for old ELFs that lack the field. Since it's embedded in ELF data, the struct must be `#[repr(C)]` and the new field must be added at the END — any older ELF not setting it will have zero bytes there, which Rust reads as `false`. This is sound as long as the kernel zero-initializes the excess bytes when the ELF section is shorter than `sizeof(CellManifest)`.

2. **ELF section size**: The current 5-param form sets `CellManifest { block_io, network, spawn, gpio, uart }` — 5 bytes if each is `bool`. Adding `hypervisor: bool` appends 1 byte. The kernel reads the section via `get_section("__ViCell_manifest")` — it must accept either 5 or 6 bytes. This is the key backward-compat invariant.

3. **Grant gate**: `loader.rs` gate must be: `manifest.has_hypervisor() && cpu_features::has_h_ext()`. Granting H-ext to a cell on a machine without H-ext would let it attempt H-extension CSR accesses that would trap. The cpu_features guard is mandatory.

4. **Security invariant**: Only the loader grants caps at spawn time. Cells cannot request `HypervisorCap` dynamically. This follows the same pattern as `BlockIoCap` / `NetworkCap`.

---

## Architecture

```
declare_manifest!(block_io=false, network=false, spawn=false, gpio=false, uart=false,
                  hypervisor=true)        // new 6-param arm in libs/api/src/manifest.rs
    │
    └── embeds CellManifest { ..., hypervisor: true } in __ViCell_manifest ELF section

kernel/src/loader.rs:spawn_from_path()
    ├── parse __ViCell_manifest from ELF
    ├── existing: if manifest.block_io()  → tcb.block_io_cap  = Some(BlockIoCap::new())
    ├── existing: if manifest.network()   → tcb.network_cap   = Some(NetworkCap::new())
    ├── existing: if manifest.spawn()     → tcb.spawn_cap     = Some(SpawnCap::new())
    └── NEW:      if manifest.has_hypervisor() && cpu_features::has_h_ext()
                    → tcb.hypervisor_cap = Some(HypervisorCap::new())
```

---

## Related Code Files

**Modify (`libs/api/` — ⚠️ Law 1):**
- `libs/api/src/manifest.rs` — add `hypervisor: bool` field + `has_hypervisor()` + 6-param `declare_manifest!` arm

**Modify (kernel — safe):**
- `kernel/src/loader.rs` — add HypervisorCap grant after the existing spawn_cap grant block

---

## Implementation Steps

### Step 1 — Read current manifest.rs

Read `libs/api/src/manifest.rs` in full to understand current struct layout and macro arms before editing.

### Step 2 — Add `hypervisor` to `CellManifest`

In `CellManifest` struct, append the new field at the end (preserves backward-compat byte layout):
```rust
#[repr(C)]
pub struct CellManifest {
    pub block_io:    bool,
    pub network:     bool,
    pub spawn:       bool,
    pub gpio:        bool,
    pub uart:        bool,
    pub hypervisor:  bool,   // ← new; zero in old ELFs = false
}
```

Add accessor:
```rust
pub fn has_hypervisor(&self) -> bool { self.hypervisor }
```

### Step 3 — Add 6-param `declare_manifest!` arm

In the `declare_manifest!` macro, add a new arm before the 5-param arm (more-specific matches must come first in Rust macros):
```rust
($block_io:tt, $network:tt, $spawn:tt, $gpio:tt, $uart:tt, hypervisor = $hv:tt) => {
    // 6-param form
    #[link_section = "__ViCell_manifest"]
    #[used]
    static _MANIFEST: $crate::manifest::CellManifest = $crate::manifest::CellManifest {
        block_io:   $block_io,
        network:    $network,
        spawn:      $spawn,
        gpio:       $gpio,
        uart:       $uart,
        hypervisor: $hv,
    };
};
```

The existing 3-param and 5-param arms keep their exact form; they produce a `CellManifest` with `hypervisor: false` (since it's missing, the struct literal would fail — so the existing 5-param arm must also be updated to add `hypervisor: false`):
```rust
// 5-param arm — add hypervisor: false to the struct literal
($block_io:tt, $network:tt, $spawn:tt, $gpio:tt, $uart:tt) => {
    // ... same as before ...
    static _MANIFEST: ... = $crate::manifest::CellManifest {
        ..., hypervisor: false,  // ← add
    };
};
// 3-param arm — same
```

### Step 4 — Update loader.rs grant block

Read `kernel/src/loader.rs` first to find the exact location of the existing cap grant block. Then append:
```rust
// Grant HypervisorCap only when both ELF manifest and CPU support H-extension.
if manifest.has_hypervisor() && crate::cpu_features::has_h_ext() {
    tcb.hypervisor_cap = Some(crate::task::cap::HypervisorCap::new());
}
```

### Step 5 — Compile check

```
cargo check -p vicell-api
cargo check -p vicell-kernel
cargo check --workspace
```

All must pass cleanly.

---

## Todo List

- [x] ⚠️ Get first user confirmation (Law 1 acknowledgment)
- [x] ⚠️ Get second user confirmation (specific change approval)
- [x] Read `libs/api/src/manifest.rs` in full before editing
- [x] `CellManifest`: add `hypervisor: bool` at end of struct
- [x] Add `has_hypervisor()` accessor
- [x] Add 6-param `declare_manifest!` arm
- [x] Update existing 5-param arm to include `hypervisor: false`
- [x] Update existing 3-param arm to include `hypervisor: false`
- [x] Read `kernel/src/loader.rs` cap grant block before editing
- [x] `loader.rs`: add HypervisorCap grant after spawn_cap grant
- [x] `cargo check -p vicell-api`
- [x] `cargo check -p vicell-kernel`
- [x] `cargo check --workspace`

---

## Success Criteria

- `cargo check --workspace` clean
- All existing `declare_manifest!(block_io = …, …)` call sites compile unchanged
- A new `declare_manifest!(…, hypervisor = true)` compiles
- `loader.rs` grants `HypervisorCap` only when manifest flag AND cpu_features agree
- Old ELFs (5 bytes in `__ViCell_manifest`) continue to work — kernel reads `hypervisor` as `false`

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Struct layout change breaks old ELFs with 5-byte manifest section | Medium | Kernel must accept ≤6-byte manifest; pad missing bytes with 0. Verify `get_section` read path. |
| Macro arm order conflict (5-param and 6-param match the same tokens) | Low | Rust macros match most-specific first; order the 6-param arm before the 5-param arm |
| `hypervisor: false` omission in existing 3- and 5-param arms causes compile error | High | Step 3 explicitly updates all arms |
| Granting HypervisorCap on non-riscv64 (e.g. ARM build) | Low | cpu_features::has_h_ext() always returns false on non-rv64 |

---

## Security Considerations

- Only the loader grants `HypervisorCap` — no dynamic acquisition path
- Double gate: manifest flag AND cpu_features — neither alone is sufficient
- H-extension CSR access without `HypervisorCap` will trap (kernel enforces)
- Law 1 process ensures peer review before ABI surface expands
