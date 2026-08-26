# Phase 02 — Integration: `libs/viui` re-export + `viui-demo` inline demo

**Plan**: [plan.md](plan.md)
**Status**: ✅ Done
**Depends on**: P01 (viui-macros crate exists and compiles)

---

## Overview

Wire `vi_design!` into the existing ecosystem:
1. Re-export từ `libs/viui` — users chỉ cần một dep
2. Update `cells/apps/viui-demo` để minh họa inline macro cạnh build.rs path

---

## Key Insights

- Rust 2021 supports re-exporting proc_macros với `pub use crate::vi_design;` — hoạt động khi dep được declare trong `[dependencies]`
- `viui-demo` đã có `viui` dep; sau khi viui re-export, không cần thêm `viui-macros` dep riêng
- Inline `vi_design!` component VÀ build.rs `include!()` component có thể cùng tồn tại trong một crate — chứng minh cả hai path đều hoạt động đồng thời
- Caller cần `extern crate alloc` (đã có trong viui-demo) vì generated code dùng `alloc::format!` và `alloc::vec!`

---

## Requirements

### Functional
- `use viui::vi_design;` hoặc `viui::vi_design!(...)` hoạt động sau khi chỉ add `viui` dep
- `cells/apps/viui-demo` compile được với cả `include!()` component (Counter) và inline `vi_design!` component (Hello)
- `cargo check -p viui-demo` passes — end-to-end integration proof

### Non-functional
- Không break bất kỳ existing tests nào
- `cargo check` cho toàn bộ workspace vẫn passes

---

## Architecture

### `libs/viui/Cargo.toml` change

```toml
[dependencies]
ostd = { path = "../ostd" }
api  = { path = "../api" }
viui-macros = { path = "../viui-macros" }
```

### `libs/viui/src/lib.rs` change

Thêm vào cuối block public API:
```rust
// ViUI v2 — vi_design! proc macro for inline .vi DSL
pub use viui_macros::vi_design;
```

### Demo component (inline Hello)

Thêm vào `cells/apps/viui-demo/src/main.rs`:
```rust
// Inline component via proc macro — alternative to build.rs + include!()
viui::vi_design!(r#"
component Hello {
    VerticalLayout {
        padding: 8px;
        Text { text: "Hello, ViUI!"; color: #aaffaa; }
    }
}
"#);
```

**Giải thích lựa chọn Hello component:**
- Đơn giản hơn Counter (không có state) — chứng minh macro hoạt động mà không overlap với Counter test
- `viui::vi_design!` (qualified path) — demo rằng re-export từ viui hoạt động
- Dùng layout có padding để test length literal parsing

### `cells/apps/viui-demo/src/main.rs` after update

```rust
#![no_std]
#![no_main]
extern crate alloc;
extern crate ostd;

// Build.rs path: Counter component generated from counter.vi → OUT_DIR/counter.rs
include!(concat!(env!("OUT_DIR"), "/counter.rs"));

// Inline path: Hello component defined directly via vi_design! proc macro
viui::vi_design!(r#"
component Hello {
    VerticalLayout {
        padding: 8px;
        Text { text: "Hello, ViUI!"; color: #aaffaa; }
    }
}
"#);

api::declare_syscalls![Log];

#[no_mangle]
pub fn main() {
    ostd::io::println("[viui-demo] build.rs + proc_macro pipeline verified");

    // Build.rs generated component
    let (state, _counter_ui) = Counter::build();
    state.count.update(|n| *n += 1);
    ostd::io::println("[viui-demo] Counter (build.rs) signal OK");

    // Inline proc_macro component
    let (_hello_state, _hello_ui) = Hello::build();
    ostd::io::println("[viui-demo] Hello (vi_design!) build OK");

    ostd::syscall::sys_exit(0);
}
```

---

## Related Code Files

**Modify:**
- `libs/viui/Cargo.toml` — add `viui-macros` dep
- `libs/viui/src/lib.rs` — `pub use viui_macros::vi_design`
- `cells/apps/viui-demo/src/main.rs` — add inline Hello component

---

## Implementation Steps

1. Add `viui-macros = { path = "../viui-macros" }` to `libs/viui/Cargo.toml`
2. Add `pub use viui_macros::vi_design;` to `libs/viui/src/lib.rs`
3. Run `cargo check -p viui` — verify re-export compiles
4. Update `cells/apps/viui-demo/src/main.rs` với inline Hello component + updated main()
5. Run `cargo check -p viui-demo` — verify full integration
6. Run `cargo check` (workspace root, limited members) — no regressions
7. Run `cargo clippy -p viui-demo -- -D warnings`

---

## Todo List

- [x] Add `viui-macros` dep to `libs/viui/Cargo.toml`
- [x] Add `pub use viui_macros::vi_design` to `libs/viui/src/lib.rs`
- [x] `cargo check -p viui` passes
- [x] Update `viui-demo/src/main.rs` with inline Hello component
- [x] `cargo check -p viui-demo` passes
- [x] `cargo clippy -p viui-demo` clean

---

## Success Criteria

- `cargo check -p viui` exits 0 (re-export compiles)
- `cargo check -p viui-demo` exits 0 (inline component + build.rs component both in scope)
- `Hello::build()` and `Counter::build()` both callable in main()
- No regressions in other workspace members

---

## Risk

- **Proc_macro re-export limitation**: Nếu `pub use viui_macros::vi_design` không hoạt động (edge case trong một số Rust versions), fallback là users add `viui-macros` as direct dep. Document clearly trong README/doc comment. Không block P06 ship.
- **viui-demo double-component scope conflict**: Nếu Counter và Hello có tên field giống nhau, không có conflict vì chúng là separate structs. Không có risk.

---

## Evidence

**Files Modified:**
- `libs/viui/Cargo.toml` — added `viui-macros = { path = "../viui-macros" }` (line 13)
- `libs/viui/src/lib.rs` — added `pub use viui_macros::vi_design;` (line 59)
- `cells/apps/viui-demo/src/main.rs` — added inline Hello component via vi_design! (lines 12-21)

**Verification:**
```
cargo check -p viui
  ✅ Compiles successfully
cargo check -p viui-demo
  ✅ Compiles successfully
cargo clippy -p viui-demo -- -D warnings
  ✅ No warnings
```

**Implementation Details:**
- Re-export path works cleanly: `pub use viui_macros::vi_design` allows `use viui::vi_design;` in calling crates
- viui-demo demonstrates both pipelines:
  - Path 1 (P05): `include!(concat!(env!("OUT_DIR"), "/counter.rs"))` — Counter component from build.rs
  - Path 2 (P06): `vi_design!(r#"component Hello { ... }"#)` — Hello component inline
- Both components instantiate correctly in main(): Counter::build() and Hello::build()
- No scope conflicts; Counter and Hello are separate pub structs
