# Phase 04 — DSL build.rs Integration

**Status:** Planned  
**Wave:** G1.1 (parallel với P01, P02, P03)  
**Priority:** High  
**Estimate:** 2 ngày  
**Depends on:** Không (vi-compiler đã có, độc lập)

---

## Context Links

- vi-compiler: `tools/vi-compiler/src/` (lib.rs, codegen.rs, parser.rs)
- vi-compiler Cargo.toml: `tools/vi-compiler/Cargo.toml`
- Robot dashboard: `cells/apps/robot-dashboard/` (target consumer demo)
- Build script docs: Cargo book §Build Scripts

---

## Overview

Hiện tại để dùng `.vi` DSL file, developer phải:
1. Chạy `vi-compiler` CLI manually: `vi-compiler input.vi -o output.rs`
2. Copy output vào src/
3. Maintain cả hai files

Đây là DX tệ. Phase này thêm **`vi-build` crate** — một build helper library dùng trong `build.rs` của app cells. App chỉ cần:

```rust
// cells/apps/my-app/build.rs
fn main() {
    vi_build::compile_vi_dir("src/ui/");
}
```

Tất cả `.vi` files trong `src/ui/` được compile thành `$OUT_DIR/vi_generated/` và app `include!` macro kết quả.

---

## Key Insights

- `build.rs` chạy trong **host environment** (Windows/Linux), không phải target RISC-V/ARM. vi-compiler phải là host tool.
- `vi-compiler` đã là crate library (có `lib.rs`). `vi-build` chỉ cần wrap các function có sẵn + file I/O.
- Cargo build cache: dùng `cargo:rerun-if-changed=<path>` cho mỗi .vi file → chỉ recompile khi file thay đổi.
- Generated files vào `OUT_DIR` (env var set bởi Cargo). App `include!` từ `env!("OUT_DIR")`.
- `vi-build` không cần depend vào `viui` — chỉ cần `vi-compiler` (host tool).
- Error reporting: nếu .vi parse/codegen fail → `panic!` trong build.rs với clear message + file:line.

---

## Architecture

### New crate: `tools/vi-build`

```
tools/vi-build/
├── Cargo.toml  — name="vi-build", lib crate
└── src/
    └── lib.rs  — public API
```

**Cargo.toml:**
```toml
[package]
name = "vi-build"
version = "0.1.0"
edition = "2021"

[dependencies]
vi-compiler = { path = "../vi-compiler" }
```

**lib.rs API:**
```rust
/// Compile tất cả .vi files trong `input_dir` vào `$OUT_DIR/vi_generated/`.
/// Tự động emit `cargo:rerun-if-changed` cho mỗi file.
///
/// Panics nếu parse hoặc codegen fail — Cargo sẽ hiển thị lỗi rõ ràng.
pub fn compile_vi_dir(input_dir: &str) {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let gen_dir = std::path::Path::new(&out_dir).join("vi_generated");
    std::fs::create_dir_all(&gen_dir).unwrap();

    for entry in std::fs::read_dir(input_dir).expect("cannot read input_dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("vi") { continue; }

        println!("cargo:rerun-if-changed={}", path.display());
        
        let source = std::fs::read_to_string(&path).expect("read .vi file failed");
        let rust_code = vi_compiler::compile(&source).unwrap_or_else(|e| {
            panic!("vi-compiler error in {:?}: {}", path, e);
        });
        
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let out_path = gen_dir.join(format!("{}.rs", stem));
        std::fs::write(out_path, rust_code).unwrap();
    }
}

/// Compile một file .vi cụ thể.
pub fn compile_vi_file(input_path: &str) {
    // ... tương tự, nhưng single file
}
```

### App integration pattern

Mỗi app cell muốn dùng DSL:

**Bước 1:** Thêm `vi-build` vào build-dependencies trong Cargo.toml:
```toml
[build-dependencies]
vi-build = { path = "../../../../tools/vi-build" }
```

**Bước 2:** Tạo `build.rs`:
```rust
fn main() {
    vi_build::compile_vi_dir("src/ui/");
}
```

**Bước 3:** Trong `main.rs` (hoặc lib.rs), include generated code:
```rust
mod vi_generated {
    include!(concat!(env!("OUT_DIR"), "/vi_generated/dashboard.rs"));
}
use vi_generated::Dashboard;
```

### vi-compiler `compile()` public function

Hiện tại `vi-compiler/src/lib.rs` có thể đã expose functions. Cần đảm bảo:
```rust
// tools/vi-compiler/src/lib.rs
pub fn compile(source: &str) -> Result<String, CompileError> {
    let tokens = lexer::lex(source)?;
    let ast    = parser::parse(tokens)?;
    let rust   = codegen::generate(ast)?;
    Ok(rust)
}

#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub line:    Option<usize>,
    pub col:     Option<usize>,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let (Some(l), Some(c)) = (self.line, self.col) {
            write!(f, "{}:{}: {}", l, c, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}
```

### Demo: robot-dashboard dùng build.rs

Sau P04, robot-dashboard có thể có `src/ui/dashboard.vi` được compile tự động. Nhưng migration này optional — không bắt buộc trong P04 scope. Đủ để tạo `cells/apps/vi-dsl-demo/` as proof-of-concept.

---

## Related Code Files

### Tạo mới
- `tools/vi-build/Cargo.toml`
- `tools/vi-build/src/lib.rs`
- `cells/apps/vi-dsl-demo/` — demo app dùng build.rs + .vi file (optional, hoặc update viui-demo)
- `cells/apps/vi-dsl-demo/build.rs`
- `cells/apps/vi-dsl-demo/src/ui/counter.vi`

### Sửa
- `tools/vi-compiler/src/lib.rs` — đảm bảo `pub fn compile(source: &str) -> Result<String, CompileError>` exposed
- `tools/vi-compiler/Cargo.toml` — thêm vào workspace nếu chưa có, ensure `lib` target
- Root `Cargo.toml` (workspace) — thêm `tools/vi-build` vào members

---

## Implementation Steps

1. **vi-compiler/src/lib.rs** — verify/add `pub fn compile(source: &str) -> Result<String, CompileError>`
2. **CompileError** — ensure proper Display, line/col info từ parser/lexer spans
3. **tools/vi-build/Cargo.toml** — new crate
4. **tools/vi-build/src/lib.rs** — `compile_vi_dir()` + `compile_vi_file()`
5. **Root Cargo.toml** — thêm `tools/vi-build` vào workspace members
6. **Demo app** — `cells/apps/vi-dsl-demo/`: build.rs, counter.vi, main.rs với include!
7. **Test** — build demo app: `cargo build -p vi-dsl-demo` thành công

---

## Todo List

- [ ] Verify vi-compiler/src/lib.rs có `pub fn compile()` với proper error type
- [ ] Tạo `tools/vi-build/Cargo.toml`
- [ ] Implement `tools/vi-build/src/lib.rs`: compile_vi_dir, compile_vi_file
- [ ] Update root Cargo.toml workspace members
- [ ] Tạo `cells/apps/vi-dsl-demo/` với build.rs + counter.vi
- [ ] Test: `cargo build -p vi-dsl-demo` success
- [ ] Verify: thay đổi counter.vi → incremental rebuild (cargo:rerun-if-changed works)
- [ ] Document pattern trong vi-build/src/lib.rs doc comments

---

## Success Criteria

- `cargo build -p vi-dsl-demo` compile vi-dsl-demo app sử dụng build.rs + .vi file
- Thay đổi `counter.vi` → Cargo tự rebuild mà không cần manual step
- Không thay đổi `counter.vi` → build cache hit (no recompile)
- Parse error trong .vi file → Cargo build thất bại với error message rõ ràng (file + line number)
- `cargo check -p vi-build` pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| OUT_DIR path có spaces (Windows) | Low | Đã handle bởi Cargo, path strings đủ |
| vi-compiler không có clean public API | Medium | Refactor lib.rs ngay bước 1 |
| Generated code conflict tên module | Low | Namespace trong `mod vi_generated {}` block |
| Incremental rebuild broken | Low | `cargo:rerun-if-changed` chuẩn, tested |

---

## Security Considerations

build.rs chạy trên host machine với file system access. `vi-compiler` chỉ đọc `.vi` files và write `.rs` files. Không có network, không có shell execution. An toàn.

---

## Next Steps

Sau P04: P07 (DSL advanced bindings) có thể build trên foundation này. Apps trong workspace dễ migrate sang DSL-first workflow.
