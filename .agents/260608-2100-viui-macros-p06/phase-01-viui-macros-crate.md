# Phase 01 — `libs/viui-macros/` proc_macro crate

**Plan**: [plan.md](plan.md)
**Status**: ✅ Done
**Priority**: P1 — foundation for P02

---

## Overview

Tạo `libs/viui-macros/` — một proc_macro crate chứa macro `vi_design!`. Macro nhận raw string literal chứa `.vi` DSL, chạy qua vi-compiler pipeline, và inject generated Rust code trực tiếp vào caller's crate.

---

## Key Insights

- `proc-macro = true` crates luôn được compile cho HOST target bởi Cargo — không cần `[workspace]` standalone như vi-compiler/viui-build
- `vi_compiler::compile_str()` + `CodeGen::new().generate()` là public API — gọi trực tiếp
- Không cần `syn`/`quote` deps — chỉ cần extract string literal từ TokenStream và parse output ngược lại; cả hai đều dùng stdlib string ops
- Generated code KHÔNG có `extern crate alloc` hay `#![...]` inner attrs (đã fix trong P04) — safe để inject inline

---

## Requirements

### Functional
- `vi_design!(r#"component Foo { ... }"#)` → `pub struct Foo { ... }` + `impl Foo { pub fn build() -> ... }` inline trong caller scope
- Raw string literals (`r"..."`, `r#"..."#`, `r##"..."##`) — primary support
- Regular string literals (`"..."`) — secondary support (basic unescape)
- Invalid DSL → `panic!` tại compile time với message rõ ràng
- Generated code identical với output của P05 `CodeGen::generate()`

### Non-functional
- Thêm `libs/viui-macros` vào root `Cargo.toml` members
- Zero new external crate deps (chỉ vi-compiler path dep + proc_macro stdlib)

---

## Architecture

### Cargo.toml

```toml
[package]
name = "viui-macros"
version = "0.2.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
vi-compiler = { path = "../../tools/vi-compiler" }
```

**Note**: `proc-macro = true` — Cargo tự động compile crate này cho HOST. Không cần `.cargo/config.toml` override. Path dep `../../tools/vi-compiler` hoạt động vì vi-compiler là standalone workspace nhưng vẫn build-able qua path.

### `src/lib.rs` structure

```rust
use proc_macro::TokenStream;

#[proc_macro]
pub fn vi_design(input: TokenStream) -> TokenStream {
    let src = extract_string_literal(input);
    let vi_file = vi_compiler::compile_str(&src)
        .unwrap_or_else(|e| panic!("vi_design!: parse error: {}", e));
    let rust_src = vi_compiler::codegen::CodeGen::new().generate(&vi_file);
    rust_src.parse()
        .unwrap_or_else(|e| panic!("vi_design!: codegen produced invalid Rust: {}", e))
}

fn extract_string_literal(input: TokenStream) -> String {
    // Collect tokens, expect single Literal
    // Call to_string() → get source text (r#"..."# or "...")
    // Strip outer delimiters → return inner .vi source
}
```

### `extract_string_literal` logic

```
input.to_string() gives the full literal source text, e.g.:
  r#"component Foo { }"#    → inner = "component Foo { }"
  r##"text with # inside"## → inner = "text with # inside"
  "hello world"             → inner = "hello world" (with escape handling)
```

Implementation (no deps):
```rust
fn extract_string_literal(input: TokenStream) -> String {
    let s = input.to_string();
    let s = s.trim();
    // Raw strings: r"..." r#"..."# r##"..."##
    if s.starts_with('r') {
        let after_r = &s[1..];
        let hash_count = after_r.chars().take_while(|c| *c == '#').count();
        let hashes = &"#".repeat(hash_count);
        let open = format!("r{}\"", hashes);
        let close = format!("\"{}", hashes);
        if s.starts_with(&open) && s.ends_with(&close) {
            let inner_start = open.len();
            let inner_end = s.len() - close.len();
            return s[inner_start..inner_end].to_owned();
        }
    }
    // Regular string: "..."
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return inner
            .replace("\\n", "\n").replace("\\t", "\t")
            .replace("\\r", "\r").replace("\\\\", "\\")
            .replace("\\\"", "\"");
    }
    panic!("vi_design! expects a string literal (use r#\"...\"# for DSL content)");
}
```

---

## Related Code Files

**Create:**
- `libs/viui-macros/Cargo.toml`
- `libs/viui-macros/src/lib.rs`

**Modify:**
- `Cargo.toml` (root) — add `"libs/viui-macros"` to `members`

---

## Implementation Steps

1. Create `libs/viui-macros/` directory
2. Write `libs/viui-macros/Cargo.toml` (proc-macro = true, vi-compiler path dep)
3. Write `libs/viui-macros/src/lib.rs` (extract_string_literal helper + vi_design! macro)
4. Add `"libs/viui-macros"` to root `Cargo.toml` `members` list
5. Run `cargo check -p viui-macros` — verify crate structure compiles
6. Run `cargo clippy -p viui-macros -- -D warnings` — clippy clean

---

## Todo List

- [x] Create `libs/viui-macros/Cargo.toml`
- [x] Implement `extract_string_literal()` + `vi_design!` in `src/lib.rs`
- [x] Add to root workspace `members`
- [x] `cargo check -p viui-macros` passes
- [x] `cargo clippy -p viui-macros` clean

---

## Success Criteria

- `cargo check -p viui-macros` exits 0
- `cargo clippy -p viui-macros -- -D warnings` exits 0
- No new external crate deps in workspace `Cargo.lock` (only proc_macro stdlib + vi-compiler already excluded)

---

## Risk

- **`vi-compiler` path dep resolution**: vi-compiler is `exclude`d from the main workspace but still accessible via path dep. Cargo resolves path deps independently — this works. If Cargo complains, add `vi-compiler` as an additional `exclude` entry (it already is).

---

## Evidence

**Files Created:**
- `libs/viui-macros/Cargo.toml` — proc-macro = true, vi-compiler path dep
- `libs/viui-macros/src/lib.rs` — vi_design! macro + extract_string_literal()

**Files Modified:**
- `Cargo.toml` (root) — added "libs/viui-macros" to members (line 17)

**Verification:**
```
cargo check -p viui-macros
  ✅ Compiles successfully (no errors)
cargo clippy -p viui-macros -- -D warnings
  ✅ No warnings
```

**Implementation Details:**
- `extract_string_literal()` handles raw strings (r"...", r#"..."#, r##"..."##) via strip_prefix + hash count
- `parse_literal_text()` also handles regular strings with basic unescape (\\n, \\t, \\r, \\\\, \\")
- `vi_design!` invokes vi_compiler::compile_str() → CodeGen::new().generate() → TokenStream::parse()
- Panic on parse errors with clear messages
- Zero external deps (only vi-compiler path dep + stdlib proc_macro)
