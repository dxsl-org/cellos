# Phase 01 — H-ext detection + HypervisorCap ZST

**Status:** ✅ Done  
**Priority:** High — unblocks Phase 02  
**Blocked by:** nothing  
**Law 1:** ❌ Not touched — no `libs/api/` or `libs/types/` changes

---

## Context Links

- Cap pattern: [kernel/src/task/cap.rs](../../kernel/src/task/cap.rs)
- TCB: [kernel/src/task/tcb.rs](../../kernel/src/task/tcb.rs) (cap fields ~148–157)
- Kernel entry: [kernel/src/main.rs](../../kernel/src/main.rs) (`kmain` — dtb at line 42–43)
- Kernel Cargo: [kernel/Cargo.toml](../../kernel/Cargo.toml)

---

## Overview

**3 changes, no Law 1:**

1. Add `fdt = "0.1"` to `kernel/Cargo.toml` — the only `no_std`/no-alloc DTB parser we need.
2. New `kernel/src/cpu_features.rs` — detect H-extension via DTB `riscv,isa` property; store in `static HAS_H_EXT`.
3. `kernel/src/task/cap.rs` — add `HypervisorCap(())` ZST.
4. `kernel/src/task/tcb.rs` — add `hypervisor_cap: Option<HypervisorCap>` field after `spawn_cap`.
5. `kernel/src/main.rs` — call `cpu_features::detect(dtb)` early in `kmain`, add module declaration.

---

## Key Insights

1. **Why DTB, not misa**: The kernel boots in S-mode (OpenSBI handles M-mode). Attempting `csrr t0, misa` from S-mode → illegal instruction trap. The DTB property `riscv,isa` (e.g. `"rv64imafdcbsh"`) is the authoritative source and accessible safely.

2. **`fdt` crate**: `fdt = "0.1"` (by repnop) is `no_std` + no-alloc, parses borrowed FDT in-place. Used by Asterinas (see `.references/Asterinas/ostd/src/arch/riscv/cpu/extension.rs`).

3. **ISA string 'h' check**: DTB may use the old single-char form (`riscv,isa = "rv64imafdcbsh"`) or the new extension-list form (`riscv,isa-extensions = "h smstateen ..."`). Check both:
   - Old form: scan for 'h' in chars after "rv64" prefix (stop at multi-char extension zone — 'h' MUST be lowercase)
   - New form: presence of "h" as a standalone word in the space-separated list
   - Gate on `#[cfg(target_arch = "riscv64")]` — aarch64/x86 always return false

4. **AtomicBool**: Write-once at boot (single hart writes before any Cells spawn). `Relaxed` ordering is sufficient — the atomic write happens-before any Cell spawn since init is launched after `cpu_features::detect()`.

5. **HypervisorCap ZST**: Identical pattern to `SpawnCap`. `Option<HypervisorCap>` is 1 byte (Rust niche optimization).

6. **TCB default**: `tcb.rs` initializes `block_io_cap: None` etc. Adding `hypervisor_cap: None` to the same initializer block is the only TCB change.

---

## Architecture

```
kmain(hartid, dtb)                  // main.rs
  │
  ├── cpu_features::detect(dtb)     // riscv64 only; no-op on other arches
  │     ├── fdt::Fdt::from_ptr(dtb) // unsafe: SAFETY: dtb from OpenSBI, magic-checked
  │     ├── for cpu in fdt.cpus():
  │     │     check "riscv,isa-extensions" for "h" token
  │     │     OR "riscv,isa" for 'h' character
  │     └── HAS_H_EXT.store(result, Relaxed)
  │
  └── (later) loader::spawn_from_path()
        └── tcb.hypervisor_cap = None  ← Phase 01 default; Phase 02 adds grant logic
```

---

## Related Code Files

**Modify:**
- `kernel/Cargo.toml` — add `fdt = "0.1"`
- `kernel/src/main.rs` — add `mod cpu_features;` + call `detect(dtb)` (remove `let _dtb = dtb;`)
- `kernel/src/task/cap.rs` — add `HypervisorCap`
- `kernel/src/task/tcb.rs` — add `hypervisor_cap` field + init to None

**Create:**
- `kernel/src/cpu_features.rs` — new module (~50 LOC)

---

## Implementation Steps

### Step 1 — Add `fdt` dependency

In `kernel/Cargo.toml`, add under `[dependencies]`:
```toml
fdt = "0.1"
```

### Step 2 — Create `kernel/src/cpu_features.rs`

```rust
//! CPU feature detection — parsed from the firmware-provided device tree.

use core::sync::atomic::{AtomicBool, Ordering};

static HAS_H_EXT: AtomicBool = AtomicBool::new(false);

/// Probe the device tree for CPU feature flags.  Call once at kernel boot
/// before any Cell is spawned.  No-op on non-riscv64 targets.
pub(crate) fn detect(dtb: usize) {
    #[cfg(target_arch = "riscv64")]
    detect_riscv(dtb);
    #[cfg(not(target_arch = "riscv64"))]
    let _ = dtb;
}

#[cfg(target_arch = "riscv64")]
fn detect_riscv(dtb: usize) {
    if dtb == 0 { return; }
    // SAFETY: dtb is a valid FDT pointer provided by OpenSBI firmware.
    // We verify the magic number inside fdt::Fdt::from_ptr before reading further.
    let fdt = match unsafe { fdt::Fdt::from_ptr(dtb as *const u8) } {
        Ok(f) => f,
        Err(_) => return,
    };
    for cpu in fdt.cpus() {
        let found = cpu.property("riscv,isa-extensions")
            .and_then(|p| p.as_str())
            .map(|s| s.split_whitespace().any(|ext| ext == "h"))
            .or_else(|| {
                cpu.property("riscv,isa")
                    .and_then(|p| p.as_str())
                    .map(|s| isa_string_has_h(s))
            })
            .unwrap_or(false);
        if found {
            HAS_H_EXT.store(true, Ordering::Relaxed);
            return;
        }
    }
}

/// Check for 'h' in the single-letter extension zone of an ISA string.
/// E.g. "rv64imafdch" → true; "rv64imafdc" → false.
/// The multi-char zone (lowercase words after all single-letter extensions)
/// does not encode 'h' — H-extension is always a single letter here.
#[cfg(target_arch = "riscv64")]
fn isa_string_has_h(isa: &str) -> bool {
    // Strip "rv32"/"rv64"/"rv128" prefix
    let body = isa.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c.is_ascii_digit())
        .to_ascii_lowercase(); // conservative: only lowercase 'h' counts
    // Actually, split from char 4 (after "rv64" or "rv32")
    let after_prefix = if isa.len() >= 4 { &isa[4..] } else { return false; };
    // Single-letter extensions are lowercase ASCII letters.
    // Stop at first digit or '_' (multi-char extension separator).
    after_prefix.chars()
        .take_while(|c| c.is_ascii_lowercase())
        .any(|c| c == 'h')
}

/// Returns true if the H-extension (hypervisor) is present on this hart.
/// Always false on non-riscv64 targets.
pub(crate) fn has_h_ext() -> bool {
    HAS_H_EXT.load(Ordering::Relaxed)
}
```

Note the `body` variable computed but unused — remove it. The final implementation uses `after_prefix` directly.

### Step 3 — Wire into `kmain`

In `kernel/src/main.rs`:

```rust
// After existing mod declarations, add:
mod cpu_features;
```

In `kmain`:
```rust
// Replace:
let _hartid = hartid;
let _dtb = dtb;
// With:
let _hartid = hartid;
cpu_features::detect(dtb);  // must run before any Cell spawns
```

Log the result:
```rust
if cpu_features::has_h_ext() {
    puts("[cpu] H-extension detected\n");
} else {
    puts("[cpu] H-extension: not present\n");
}
```

### Step 4 — Add HypervisorCap to `cap.rs`

Append to `kernel/src/task/cap.rs`:
```rust
/// Permits use of RISC-V H-extension CSRs (hstatus, hgatp, etc.).
/// Granted only when the firmware reports H-ext present AND the ELF
/// manifest declares `hypervisor = true`.
#[derive(Copy, Clone, Debug)]
pub struct HypervisorCap(());

impl HypervisorCap {
    pub(crate) fn new() -> Self { Self(()) }
}
```

### Step 5 — Add field to TCB

In `kernel/src/task/tcb.rs`, after `spawn_cap`:
```rust
/// H-extension hypervisor access.  Granted only to VMM cells on rv64 with H-ext.
pub hypervisor_cap: Option<super::cap::HypervisorCap>,
```

Find the TCB constructor/default init and add `hypervisor_cap: None`.

### Step 6 — Compile check

```
cargo check -p vicell-kernel
cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat
```

Both must pass cleanly. The `#[cfg(target_arch = "riscv64")]` guard ensures aarch64 builds compile without the fdt path.

---

## Todo List

- [x] `kernel/Cargo.toml`: add `fdt = "0.1"`
- [x] Create `kernel/src/cpu_features.rs`
- [x] `kernel/src/main.rs`: `mod cpu_features` + `cpu_features::detect(dtb)` + log line
- [x] `kernel/src/task/cap.rs`: add `HypervisorCap`
- [x] `kernel/src/task/tcb.rs`: add `hypervisor_cap: Option<HypervisorCap>` + init to None
- [x] `cargo check -p vicell-kernel` (riscv64)
- [x] `cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat`

---

## Success Criteria

- `cargo check` clean on both riscv64 and aarch64 targets
- `HAS_H_EXT` is false on default QEMU (`-cpu rv64`, no H-ext) — confirmed by boot log "[cpu] H-extension: not present"
- `HAS_H_EXT` is true on QEMU with `-cpu rv64,h=true` or `-cpu max` — confirmed by "[cpu] H-extension detected"
- `HypervisorCap` defined in cap.rs; `tcb.hypervisor_cap: None` for all existing cells

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `fdt` crate doesn't compile in kernel `no_std` env | Low | Crate is designed for bare-metal; confirm `default-features = false` if needed |
| DTB is 0 on some boot paths (direct `-kernel` without OpenSBI) | Medium | Guarded: `if dtb == 0 { return; }` |
| aarch64 build fails (fdt pull riscv-specific code) | Low | `#[cfg(target_arch = "riscv64")]` gates all fdt usage |
| ISA string parsing misses 'h' in multi-char extension zone | Low | Both `riscv,isa-extensions` and `riscv,isa` formats handled |

---

## Security Considerations

- `fdt::Fdt::from_ptr` is unsafe; SAFETY comment documents firmware-provided pointer
- DTB magic number verified inside `fdt` before any slice access
- `HAS_H_EXT` is write-once (boot) + read-many (spawn time); AtomicBool prevents data races

---

## Estimated LOC

| File | Lines |
|------|-------|
| cpu_features.rs (new) | ~55 |
| cap.rs additions | ~8 |
| tcb.rs additions | ~4 |
| main.rs additions | ~6 |
| Cargo.toml | 1 |
| **Total** | **~74** |
