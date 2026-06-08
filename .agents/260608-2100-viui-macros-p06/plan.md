# ViUI v2 P06 — vi_design! Proc Macro

**Plan ID**: 260608-2100-viui-macros-p06
**Stage**: G2
**Priority**: P1 — enables inline `.vi` DSL in Rust source without a separate file
**Created**: 2026-06-08
**Status**: ✅ **COMPLETE** — 2026-06-08
**Depends on**: P04 (vi-compiler codegen), P05 (viui-build — proves CodeGen output is valid Rust)

---

## Mục tiêu

Cho phép viết `.vi` DSL inline trong Rust source code, không cần file riêng hay build.rs:

```rust
// cells/apps/my-app/src/main.rs
vi_design!(r#"
component Counter {
    in-out property <int> count: 0;
    VerticalLayout {
        Text { text: "Count: \{count}"; color: #ffffff; }
        Button { text: "Increment"; clicked => { count += 1; } }
    }
}
"#);

// Counter struct is now in scope — same as what P05 generated via include!()
let (state, ui) = Counter::build();
```

---

## Scope

### In scope
- `libs/viui-macros/` — proc_macro crate exporting `vi_design!`
- `vi_design!(r"...")` input: raw string literals (primary); regular string literals (secondary)
- Add `libs/viui-macros` to workspace members
- Re-export `vi_design!` from `libs/viui` so users only need one dep
- Update `cells/apps/viui-demo` to demonstrate inline macro alongside existing build.rs path
- Unit tests for string-literal parsing helper

### Out of scope (explicitly deferred)
- **Hot-reload** — needs file watcher daemon; G2 polish
- **`#[vi_component]` derive macro** — different use case; separate P
- **LSP / IDE syntax highlighting for inline DSL** — tooling concern
- **Rich span-precise error messages** — basic `panic!` is acceptable for P06
- **Recursive glob / multi-file** — already handled by viui-build (P05)

---

## Architecture

### Why proc_macro works here

`viui-macros` is a proc_macro crate — Cargo ALWAYS compiles proc_macro crates for HOST target, regardless of workspace default target (`riscv64gc-unknown-none-elf`). No standalone workspace needed; `libs/viui-macros` can live in the main workspace.

```
libs/viui-macros/ (proc_macro = true)
    depends on tools/vi-compiler (path dep — std, host-compiled as build tool)
```

Cargo's dependency resolution handles the HOST compilation automatically because `proc-macro = true` crates are always compiled for the host toolchain.

### Macro pipeline

```
vi_design!(r#"component Counter { ... }"#)
    ↓
proc_macro TokenStream → extract string literal text
    ↓
vi_compiler::compile_str(src) → ViFile AST
    ↓
vi_compiler::codegen::CodeGen::new().generate(&file) → Rust source string
    ↓
rust_src.parse::<proc_macro::TokenStream>() → inline code in caller's crate
```

The generated code is identical to what `viui-build` writes to `$OUT_DIR` in P05. The only difference: proc_macro injects it directly instead of via `include!()`.

### String literal input

```rust
vi_design!(r#"..."#)  // preferred — raw string, no backslash escaping issues
vi_design!("...")     // supported — regular string, escape sequences handled
```

Implementation: iterate `input` TokenStream, extract single `Literal` token, call `to_string()` to get source text (`r#"..."#` or `"..."`), strip outer delimiters to get the `.vi` source.

### Re-export path

```toml
# libs/viui/Cargo.toml
[dependencies]
viui-macros = { path = "../viui-macros" }
```

```rust
// libs/viui/src/lib.rs
pub use viui_macros::vi_design;
```

Users add only `viui` as a dep; `vi_design!` is available through it. If Rust's proc_macro re-export has edge cases, fallback is documenting direct `viui-macros` dep.

---

## Phase Table

| Phase | File | Nội dung | Status |
|-------|------|----------|--------|
| P01 | [phase-01-viui-macros-crate.md](phase-01-viui-macros-crate.md) | `libs/viui-macros/` proc_macro crate + `vi_design!` impl + workspace entry | ✅ Done |
| P02 | [phase-02-integration.md](phase-02-integration.md) | `libs/viui` re-export + `viui-demo` inline demo + `cargo check` verify | ✅ Done |

P02 depends on P01 — all phases complete.

---

## Files Created/Modified

```
libs/viui-macros/
├── Cargo.toml          (NEW — proc-macro = true, vi-compiler path dep)
└── src/
    └── lib.rs          (NEW — vi_design! macro implementation)

libs/viui/
└── Cargo.toml          (MODIFY — add viui-macros dep)
    src/lib.rs          (MODIFY — pub use viui_macros::vi_design)

cells/apps/viui-demo/
├── Cargo.toml          (MODIFY — add viui-macros dep for inline demo)
└── src/main.rs         (MODIFY — add vi_design! inline component alongside include!())

Cargo.toml              (MODIFY — add "libs/viui-macros" to members)
```

---

## Success Criteria

- [ ] `cargo check --manifest-path libs/viui-macros/Cargo.toml` passes (if checked standalone)
- [ ] `cargo check -p viui` passes (viui re-exports vi_design!)
- [ ] `cargo check -p viui-demo` passes (inline vi_design! component in scope)
- [ ] `vi_design!(r#"component Foo { }"#)` expands to `pub struct Foo { ... } impl Foo { pub fn build() ... }`
- [ ] Invalid DSL input → `panic!` with message (not silent failure)

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Proc_macro re-export from viui doesn't work cleanly | Low | Fallback: document direct `viui-macros` dep; re-export is optional ergonomic improvement |
| `vi-compiler` path dep from inside workspace causes resolver conflict | Low | vi-compiler has `[workspace]` so it's standalone; path dep from workspace member is standard Cargo |
| Generated code has `alloc::format!` / `alloc::vec!` — caller needs `extern crate alloc` | Known | viui-demo already has it; document requirement in macro doc comment |
| `proc_macro::TokenStream::from_str` rejects generated code | Low | P05 already validated generated code with `cargo check -p viui-demo`; same codegen path |
