# Phase 06 — FlexBox v2: Full Layout Engine

**Status:** Planned  
**Wave:** G2.1 (parallel với P05, sau Wave 1)  
**Priority:** Medium  
**Estimate:** 3-4 ngày  
**Depends on:** FlexBox widget baseline (đã có)

---

## Context Links

- Current FlexBox: `libs/viui/src/node_widgets/flex_box.rs` (520+ lines)
- Old plan phase 07: `.agents/260608-1451-viui-next-phases/phase-07-layout-v2.md`
- Node trait: `libs/viui/src/node.rs`
- Column/Row: `libs/viui/src/node_widgets/column.rs`, `row.rs`

---

## Overview

FlexBox widget đã tồn tại với `flex_grow`, `justify_content`, `align_items`. Nhưng chưa có:
- **flex-wrap** — sang hàng mới khi overflow
- **align-content** — alignment của multiple lines khi wrap
- **gap** — khoảng cách cố định giữa items
- **justify-content: space-evenly / space-around** — chỉ có space-between
- **align-items: stretch** — children kéo dài theo cross axis
- **flex-shrink** — co lại khi thiếu space
- **min/max constraints** trên FlexChild level

Phase này implement full flexbox spec subset đủ cho G2 responsive layouts.

---

## Key Insights

- Flexbox layout algorithm có 3 bước chính: **measure** (intrinsic size) → **distribute** (flex-grow/shrink) → **place** (justify/align). Hiện tại bước 3 incomplete.
- `LayoutCx` proposal từ old plan (breaking change): có thể thêm `min_width/min_height` vào ViNode::layout() mà không phải breaking change — thêm default params hoặc separate struct.
- **flex-wrap** là feature phức tạp nhất: cần "line boxes" — group items vào dòng, sau đó align từng dòng.
- **gap** đơn giản: thêm `gap_main: f32, gap_cross: f32` field. Áp dụng khi đặt items.
- **G2 target apps:** server dashboard với responsive columns (3 panels cạnh nhau), form layouts với inputs aligned.
- Test approach: unit tests với fixed bounds → assert child positions pixel-perfect.

---

## Architecture

### Enum additions

```rust
// Thêm vào justify_content
pub enum Justify {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,     // NEW
    SpaceEvenly,     // NEW
}

// Thêm vào align_items
pub enum AlignItems {
    Start,
    End,
    Center,
    Stretch,         // NEW — children stretch to cross-axis size of line
    Baseline,        // NEW (optional, complex — defer nếu cần)
}

// NEW
pub enum AlignContent {
    Start, End, Center, Stretch, SpaceBetween, SpaceAround, SpaceEvenly,
}
```

### FlexBox struct additions

```rust
pub struct FlexBox {
    // Existing
    children:      Vec<FlexChild>,
    direction:     FlexDirection,
    justify:       Justify,
    align_items:   AlignItems,
    // NEW
    wrap:          FlexWrap,       // NoWrap | Wrap | WrapReverse
    align_content: AlignContent,   // alignment of multiple lines
    gap_main:      f32,            // gap along main axis
    gap_cross:     f32,            // gap along cross axis
}

pub struct FlexChild {
    node:       Box<dyn ViNode>,
    flex_grow:  f32,    // existing
    flex_shrink: f32,   // NEW (default 1.0)
    min_size:   Option<f32>,  // NEW — min along main axis
    max_size:   Option<f32>,  // NEW — max along main axis
    align_self: Option<AlignItems>,  // NEW — override parent align_items
}
```

### Layout algorithm (full spec)

```
layout(available: Rect) -> LayoutView:

1. MEASURE phase:
   For each child:
     base_size = child.layout(available).main_size  // intrinsic
     hypothetical_size = clamp(base_size, min_size, max_size)
   
2. LINE BREAKING (nếu wrap enabled):
   Greedy: accumulate items into line, break when total + gap > main_axis_available
   Result: Vec<Vec<&FlexChild>> (line boxes)

3. DISTRIBUTE flex (per line):
   free_space = main_axis - sum(hypothetical_sizes) - sum(gaps)
   If free_space > 0: distribute among items with flex_grow > 0
     grow_unit = free_space / total_grow_factor
     each item gets: hypothetical + grow_unit * flex_grow
   If free_space < 0: shrink among items with flex_shrink > 0
     shrink proportionally to flex_shrink * base_size

4. PLACE items (per line):
   Compute item positions from justify_content:
     Start: pos = 0, advance by (size + gap)
     SpaceBetween: gaps = free_space / (n-1)
     SpaceAround: gaps = free_space / n, half-gap at edges
     SpaceEvenly: gaps = free_space / (n+1), full-gap at edges
   
5. ALIGN cross-axis (per item):
   effective_align = item.align_self.unwrap_or(self.align_items)
   Stretch: set cross_size = line cross size
   Center/Start/End: offset item position in cross axis

6. PLACE lines (wrap only):
   align_content: analogous to justify_content but for lines in cross axis
```

### Builder API additions

```rust
impl FlexBox {
    pub fn wrap(mut self) -> Self                          // enable wrap
    pub fn gap(mut self, gap: f32) -> Self                // set both gaps
    pub fn gap_cross(mut self, gap: f32) -> Self
    pub fn align_content(mut self, ac: AlignContent) -> Self
}

impl FlexChild {
    pub fn shrink(mut self, factor: f32) -> Self
    pub fn min_size(mut self, size: f32) -> Self
    pub fn max_size(mut self, size: f32) -> Self
    pub fn align_self(mut self, a: AlignItems) -> Self
}
```

---

## Related Code Files

### Sửa
- `libs/viui/src/node_widgets/flex_box.rs` — major refactor: thêm enums, update struct, rewrite layout algorithm
- `tools/vi-compiler/src/codegen.rs` — thêm `wrap`, `gap`, `align_content` builder calls
- `tools/vi-compiler/tests/codegen_tests.rs` — update flexbox_codegen test

### Không sửa
- `libs/viui/src/node.rs` — không cần LayoutCx change (defer)
- Existing widgets — FlexBox layout chỉ trong flex_box.rs

---

## Implementation Steps

1. **Enum additions** — Justify::SpaceAround/Evenly, AlignItems::Stretch, AlignContent, FlexWrap
2. **FlexChild updates** — flex_shrink, min/max_size, align_self fields + builder methods
3. **FlexBox struct updates** — wrap, align_content, gap_main, gap_cross + builder methods
4. **Line breaking algorithm** — `compute_lines(&self, children: &[FlexChild], main_avail: f32) -> Vec<LineBox>`
5. **Flex distribute** — `distribute_flex(line: &mut LineBox, free_space: f32)`
6. **Place items** — `place_items_in_line(line: &LineBox, justify: Justify) -> Vec<f32>` (positions)
7. **Cross-axis align** — per-item cross position based on align_items/align_self
8. **align_content** — multi-line placement (only active when wrap)
9. **Codegen update** — new builder calls
10. **Unit tests** — 8+ layout scenarios: wrap, gap, space-evenly, stretch, mixed flex-grow

---

## Todo List

- [ ] Thêm enum variants: Justify (SpaceAround/Evenly), AlignItems (Stretch), AlignContent, FlexWrap
- [ ] Update FlexChild: flex_shrink, min/max_size, align_self + builders
- [ ] Update FlexBox: wrap, align_content, gap + builders
- [ ] Implement line-breaking algorithm (compute_lines)
- [ ] Implement flex distribution (grow + shrink)
- [ ] Implement justify placement (all variants)
- [ ] Implement cross-axis align (Stretch + Center/Start/End)
- [ ] Implement align_content (multi-line)
- [ ] Update codegen.rs cho new builder calls
- [ ] Viết 8+ layout unit tests
- [ ] `cargo check -p viui` pass, không regression trên existing robot-dashboard

---

## Success Criteria

- FlexBox với `wrap()` tự sang hàng mới khi overflow
- `gap(8.0)` đặt 8px giữa tất cả items (main + cross)
- `Justify::SpaceEvenly` chia đều khoảng cách bao gồm cả edges
- `AlignItems::Stretch` — children không có fixed cross-size kéo dài hết cross axis của line
- `flex_shrink(2.0)` → item co lại nhanh hơn item với `flex_shrink(1.0)` khi overflow
- Tất cả 8+ unit tests pass với pixel-perfect position assertions (± 0.5px)
- Robot-dashboard không có regression sau refactor

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Layout algorithm có rounding errors | Medium | Use f32 arithmetic consistently, allow ±0.5px in tests |
| Wrap + flex_grow tương tác phức tạp | Medium | Implement per-line (not global), spec là chuẩn |
| Breaking change với existing FlexBox users | Low | Chỉ robot-dashboard dùng, và chỉ basic API |
| Performance regression với wrap | Low | wrap có extra work nhưng chỉ khi `wrap=true` |

---

## Security Considerations

Layout chỉ liên quan đến f32 arithmetic. Không có memory unsafety, không có user input.

---

## Next Steps

Sau P06: FlexBox đủ mạnh cho server dashboard với multi-column responsive layout. G2 desktop apps dùng được.
