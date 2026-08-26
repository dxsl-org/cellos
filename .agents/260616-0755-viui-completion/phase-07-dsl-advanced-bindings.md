# Phase 07 — DSL Advanced Bindings

**Status:** Planned  
**Wave:** G2.2 (sau P04 DSL build.rs)  
**Priority:** Medium  
**Estimate:** 3 ngày  
**Depends on:** P04 (DSL build.rs — cần để test E2E pipeline)

---

## Context Links

- Parser: `tools/vi-compiler/src/parser.rs`
- AST: `tools/vi-compiler/src/ast.rs`
- Codegen: `tools/vi-compiler/src/codegen.rs`
- Lexer: `tools/vi-compiler/src/lexer.rs`
- Old plan: `.agents/260608-1451-viui-next-phases/phase-10-dsl-reactive-bindings.md`
- Tests: `tools/vi-compiler/tests/{parser_tests.rs, codegen_tests.rs}`

---

## Overview

DSL hiện tại hỗ trợ:
- `property: expr` — one-way binding (expr → property value)
- `on_click: { ... }` — callback body

Còn thiếu:
1. **`@=` two-way binding** — `input_value @= self.name` (Signal đọc VÀ ghi cả hai chiều)
2. **`#=` computed property** — `display_text #= format!("{:.0}%", self.value * 100.0)` (property tự động recompute khi dependencies thay đổi)
3. **`@import` component reuse** — `import { Button } from "base.vi"` (component library)

G1 chỉ cần `@=` và `#=`. `@import` là G2+ nếu thời gian cho phép.

---

## Key Insights

- **`@=` hai chiều:** Trong codegen, thay vì `self.field.set(expr)` một chiều, cần:
  1. Init widget với `Signal::clone(self.field)`
  2. Đăng ký subscription: khi widget's internal signal thay đổi → update `self.field`
  3. Đăng ký subscription: khi `self.field` thay đổi → update widget's internal signal
  → Ví dụ: TextEdit với `@=` binding: khi user type → internal text signal → write back to `self.name`. Khi `self.name` set từ ngoài → update TextEdit display.
- **`#=` computed:** Syntax `text #= "Battery: \{self.battery * 100.0}%"` → codegen emit `Computed::new(...)` subscribed to `self.battery`.
- **Lexer changes:** cần 2 new token types: `TwoWayBind` (@=) và `ComputedBind` (#=).
- **AST changes:** `Binding` enum → `Binding::OneWay(Expr)` | `Binding::TwoWay(Expr)` | `Binding::Computed(Expr)`.
- **Codegen changes:** `emit_binding()` match trên binding type, emit appropriate Rust code.
- **@import:** phức tạp hơn — cần file resolution, component registry. Defer nếu 3 ngày không đủ.

---

## Architecture

### Lexer additions

```rust
// tools/vi-compiler/src/token.rs
pub enum TokenKind {
    // ... existing ...
    TwoWayBind,    // @=
    ComputedBind,  // #=
    At,            // @ (cho @import nếu implement)
}
```

Lexer: khi thấy `@` và `=` liền tiếp → emit TwoWayBind. Khi thấy `#` và `=` → ComputedBind.

### AST additions

```rust
// tools/vi-compiler/src/ast.rs

pub enum BindingMode {
    OneWay,    // property: expr  (hiện tại)
    TwoWay,    // property @= expr
    Computed,  // property #= expr
}

pub struct Binding {
    pub property: String,
    pub mode:     BindingMode,
    pub expr:     Expr,
}

// Component properties (existing) nhận thêm:
pub struct PropDecl {
    pub name:         String,
    pub ty:           PropType,
    pub default:      Option<Expr>,
    pub is_two_way:   bool,  // property được export để parent bind @=
}
```

### Codegen additions

#### `@=` Two-Way Binding

Input DSL:
```vi
component Login {
    property inout name: string = "";
    
    TextEdit {
        text @= self.name
    }
}
```

Generated Rust:
```rust
pub struct Login {
    pub name: Signal<String>,
}

impl Login {
    pub fn build(&self) -> Box<dyn ViNode> {
        // Two-way: TextEdit internal signal synced bidirectionally with self.name
        let name_clone = self.name.clone();
        let text_edit = TextEdit::new(self.name.clone())
            .on_change(move |new_val| {
                name_clone.set(new_val);  // TextEdit → self.name
            });
        // self.name changes → TextEdit updates automatically (Signal subscription in TextEdit)
        Box::new(text_edit)
    }
}
```

#### `#=` Computed Property

Input DSL:
```vi
component BatteryDisplay {
    property battery: float = 1.0;
    
    // Computed: auto-recompute when self.battery changes
    property display_text: string;
    display_text #= format!("Battery: {:.0}%", self.battery * 100.0)
    
    Label {
        text: self.display_text
    }
}
```

Generated Rust:
```rust
pub struct BatteryDisplay {
    pub battery: Signal<f32>,
    display_text: Signal<String>,
    _computed_subs: Vec<SubscriptionHandle>,
}

impl BatteryDisplay {
    pub fn new() -> Self {
        let battery = Signal::new(1.0f32);
        let display_text = Signal::new(String::new());
        
        // Computed binding
        let dt = display_text.clone();
        let bat = battery.clone();
        let sub = battery.subscribe(move |v| {
            dt.set(format!("Battery: {:.0}%", v * 100.0));
        });
        // Initial compute
        display_text.set(format!("Battery: {:.0}%", *battery.get() * 100.0));
        
        Self { battery, display_text, _computed_subs: vec![sub] }
    }
    
    pub fn build(&self) -> Box<dyn ViNode> {
        Box::new(Label::new(self.display_text.clone()))
    }
}
```

### @import (optional, implement nếu còn thời gian)

```vi
import { PrimaryButton } from "components/buttons.vi"
import { SensorRow } from "components/sensor.vi"

component Dashboard {
    Column {
        SensorRow { label: "CPU"; value: self.cpu }
        PrimaryButton { label: "Stop"; on_click: { self.stop() } }
    }
}
```

Codegen challenge: cần resolve file path, parse + codegen imported file, merge structs/impls. Complex — **only implement if P01-P06 done with time remaining**.

---

## Related Code Files

### Sửa
- `tools/vi-compiler/src/token.rs` — thêm TwoWayBind, ComputedBind token kinds
- `tools/vi-compiler/src/lexer.rs` — recognize `@=` và `#=` sequences
- `tools/vi-compiler/src/ast.rs` — Binding struct với BindingMode, PropDecl.is_two_way
- `tools/vi-compiler/src/parser.rs` — parse_binding: detect TwoWayBind/ComputedBind tokens
- `tools/vi-compiler/src/codegen.rs` — emit_binding: branch trên BindingMode, emit_two_way_bind, emit_computed_bind
- `tools/vi-compiler/tests/parser_tests.rs` — test @= và #= parsing
- `tools/vi-compiler/tests/codegen_tests.rs` — test @= và #= codegen output

---

## Implementation Steps

1. **token.rs** — TwoWayBind, ComputedBind variants
2. **lexer.rs** — scan `@=` (not just `@`) → TwoWayBind, `#=` → ComputedBind
3. **ast.rs** — BindingMode enum, update Binding struct
4. **parser.rs** — parse_binding: sau property name, check TwoWayBind/ComputedBind/Colon
5. **codegen.rs** — emit_two_way_bind: emit on_change closure + comment; emit_computed_bind: emit subscribe + initial compute
6. **Parser tests** — 2 tests: two_way_parse, computed_parse
7. **Codegen tests** — 2 tests: two_way_codegen (assert on_change closure present), computed_codegen (assert subscribe present)
8. **@import** — chỉ nếu còn thời gian: file resolver, import AST node, merged codegen

---

## Todo List

- [ ] Thêm TwoWayBind, ComputedBind vào token.rs
- [ ] Update lexer.rs: nhận dạng @= và #=
- [ ] Update ast.rs: BindingMode, update Binding struct
- [ ] Update parser.rs: parse_binding với mode detection
- [ ] Update codegen.rs: emit_two_way_bind (on_change closure)
- [ ] Update codegen.rs: emit_computed_bind (subscribe + initial compute)
- [ ] Viết parser tests (2)
- [ ] Viết codegen tests (2)
- [ ] Optional: @import implementation
- [ ] `cargo test -p vi-compiler` — tất cả tests pass (không có regression)

---

## Success Criteria

- DSL `text @= self.name` generates bidirectional binding code (has both `Signal::clone` sharing AND `on_change` callback)
- DSL `display_text #= expr` generates `subscribe()` call + initial compute
- Tất cả existing 53 tests vẫn pass (không regression)
- 4 new tests pass (2 parser + 2 codegen)
- E2E: vi-dsl-demo app sử dụng @= và #= compile thành công via build.rs

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Lexer ambiguity: `@` standalone vs `@=` | Low | Peek next char, multi-char scan |
| Computed binding circular dependency | Medium | Document: computed cannot depend on itself. No cycle detection in G1 |
| @import file resolution cross-platform | Medium | Đây là lý do @import là optional |
| Two-way binding memory: circular Arc | Low | Không có Arc ở đây — Signal dùng Rc, same thread, không có cycle issue |

---

## Security Considerations

Compiler chạy host-side (build.rs). Input là .vi source files từ developer. Không có runtime parsing, không có user-supplied input vào compiler. Safe.

---

## Next Steps

Sau P07: DSL đủ mạnh để viết production UI components. Kết hợp với P04 build.rs cho full DX pipeline. Nền tảng cho @import và component library G2+.
