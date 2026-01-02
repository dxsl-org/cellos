# ViOS Kernel Prelude Policy

**Date**: 2026-01-01  
**Status**: Active  
**Version**: 1.0

## Overview

ViOS kernel uses a **minimal prelude module** to reduce boilerplate imports of fundamental types while maintaining the explicitness required for kernel development.

## Rationale

### Why We Use a Prelude

1. **Industry Standard**: Production kernels (Redox OS, Tock OS) use this pattern
2. **Fundamental Types**: `Option` and `Result` are as fundamental as built-in types
3. **Maintainability**: Easier to evolve codebase with 50+ modules
4. **Developer Productivity**: Focus on kernel logic, not import boilerplate
5. **Consistency**: Mirrors `std::prelude::v1` pattern from Rust standard library

### Why We Keep It Minimal

1. **Security Auditing**: Easy to verify what's universally available
2. **Debugging**: Clear dependency tracking for critical kernel code
3. **Binary Size**: No risk of pulling in unused code
4. **Explicitness**: Domain-specific types remain explicitly imported
5. **Principle of Least Privilege**: Only truly universal types in prelude

## Policy Rules

### ✅ ALLOWED in Prelude

**ONLY** these fundamental types from `core`:

```rust
pub use core::option::Option::{self, Some, None};
pub use core::result::Result::{self, Ok, Err};
```

**Rationale**: These types appear in virtually every kernel module and are part of Rust's core language semantics.

### ❌ FORBIDDEN in Prelude

**Everything else**, including but not limited to:

- ❌ Collections: `Vec`, `String`, `BTreeMap`, `VecDeque`
- ❌ Smart pointers: `Box`, `Rc`, `Arc`
- ❌ Utilities: `fmt::Write`, `mem::*`, `ptr::*`
- ❌ Traits: `From`, `Into`, `Iterator`, etc.
- ❌ Domain types: Anything from `alloc`, `kernel`, `drivers`
- ❌ Wildcard re-exports: `pub use module::*;`

**Rationale**: These should be explicitly imported to maintain clarity about dependencies.

## Usage Guidelines

### For Module Authors

**DO** use the prelude in every kernel module:

```rust
// At the top of every .rs file in kernel/src/
use crate::prelude::*;
```

**DO** explicitly import other types:

```rust
use alloc::vec::Vec;
use alloc::string::String;
use crate::sync::Spinlock;
```

**DON'T** assume types are available without checking:

```rust
// ❌ BAD: Assuming Vec is in prelude
fn process(data: Vec<u8>) { }

// ✅ GOOD: Explicit import
use alloc::vec::Vec;
fn process(data: Vec<u8>) { }
```

### For Prelude Maintainers

**Before adding to prelude**, ask:

1. ✅ Is this type used in >90% of kernel modules?
2. ✅ Is this a fundamental language type (like Option/Result)?
3. ✅ Would removing this require 50+ import statements?
4. ✅ Is this in `std::prelude::v1`?

If **ALL** answers are YES, propose addition via team review.

**Process for modification**:

1. Open discussion in team meeting
2. Document rationale in this file
3. Get consensus from 2+ senior developers
4. Update prelude with clear comments
5. Update this policy document

## Comparison with Other Kernels

### Redox OS
```rust
pub mod prelude {
    pub use core::prelude::v1::*;
    pub use alloc::prelude::v1::*;
}
```
**Note**: More permissive than ViOS policy

### Tock OS
```rust
// No formal prelude, but common pattern:
use core::option::Option::{self, Some, None};
use core::result::Result::{self, Ok, Err};
```
**Note**: Similar to ViOS approach

### Linux Kernel (Rust for Linux)
```rust
pub mod prelude {
    pub use core::option::Option::{self, Some, None};
    pub use core::result::Result::{self, Ok, Err};
    // + kernel-specific macros
}
```
**Note**: Closest to ViOS policy

## Benefits Observed

### Development Speed
- ✅ Reduced boilerplate: ~2 lines saved per file × 50 files = 100 lines
- ✅ Faster module creation: No need to remember basic imports
- ✅ Easier refactoring: Change once in prelude vs 50 files

### Code Quality
- ✅ Consistency: All modules use same fundamental types
- ✅ Readability: Less import noise at top of files
- ✅ Maintainability: Clear separation of fundamental vs domain imports

### Security & Safety
- ✅ Auditable: Single file to review for universal imports
- ✅ Explicit: Domain types still require explicit imports
- ✅ Traceable: Easy to grep for specific type usage

## Review Schedule

This policy should be reviewed:
- ✅ Every 6 months
- ✅ When adding new fundamental types to kernel
- ✅ After major Rust version updates
- ✅ When team consensus changes

## Version History

### v1.0 (2026-01-01)
- Initial policy established
- Prelude limited to Option and Result only
- Documented rationale and guidelines

---

**Approved by**: ViOS Kernel Team  
**Next Review**: 2026-07-01
