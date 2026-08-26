# Phase 05 — Virtual ListView + Performance

**Status:** Planned  
**Wave:** G2.1 (parallel với P06, sau Wave 1)  
**Priority:** High  
**Estimate:** 3 ngày  
**Depends on:** ListView baseline (đã có), không cần P01-P04

---

## Context Links

- Current ListView: `libs/viui/src/node_widgets/list_view.rs` (320 lines, non-virtual)
- Signal: `libs/viui/src/signal.rs`
- Node trait: `libs/viui/src/node.rs`
- ViApp dirty rect: `libs/viui/src/app_runner.rs`

---

## Overview

ListView hiện tại render **tất cả** items — chỉ clip khi scroll. 200 items với phức tạp widget mỗi item = layout + paint cho 200 widgets mỗi frame. Không scale lên server dashboard với 10k+ log entries.

Virtual ListView: chỉ instantiate + layout + paint N items visible trong viewport (plus small buffer). Khi scroll, recycle widget slots. O(visible_count) thay vì O(total_count).

**Performance target:** 10k items, < 2ms paint time trên QEMU.

---

## Key Insights

- **Fixed item height là prerequisite cho efficient virtual list.** Variable-height virtual list cần prefix-sum array của heights (phức tạp). G1: require fixed `item_height: f32`.
- **Row recycling pool:** số lượng widget slot cố định = `ceil(viewport_height / item_height) + 2` (buffer). Khi scroll 1 item, top slot được reused cho bottom item.
- **Data provider trait** thay vì `Vec<Box<dyn ViNode>>`: builder function `Fn(idx: usize) -> Box<dyn ViNode>` được gọi khi slot cần rebind.
- **Scrollbar** vẫn dùng `total_height = item_count * item_height`. Không thay đổi scroll API.
- **Signal<usize> item_count** cho dynamic lists (items thêm/xóa real-time).
- Không backward compatible hoàn toàn: ListView cũ dùng `Vec<Box<dyn ViNode>>`. Giải pháp: giữ cả hai — `StaticListView` (cũ, rename) và `ListView` (new virtual). Apps hiện tại dùng StaticListView, migrate dần.

---

## Architecture

### ListDataProvider trait

```rust
// libs/viui/src/node_widgets/list_view.rs (hoặc list_data.rs)

pub trait ListDataProvider: 'static {
    fn item_count(&self) -> usize;
    fn build_item(&self, index: usize) -> Box<dyn ViNode>;
    fn item_height(&self) -> f32;  // fixed height required
}

// Concrete impl cho simple string lists:
pub struct StringListProvider {
    items: Signal<Vec<String>>,
    row_builder: Box<dyn Fn(&str, usize) -> Box<dyn ViNode>>,
}

// Concrete impl cho Vec of any T:
pub struct VecProvider<T: Clone + 'static> {
    items: Signal<Vec<T>>,
    height: f32,
    builder: Box<dyn Fn(&T, usize) -> Box<dyn ViNode>>,
}
```

### Virtual ListView

```rust
pub struct ListView {
    provider:      Box<dyn ListDataProvider>,
    scroll_y:      f32,               // current scroll position (pixels)
    scroll_target: f32,               // for fling decay
    slot_count:    usize,             // ceil(viewport/item_height) + 2
    slots:         Vec<ListSlot>,     // recycled widget instances
    selected:      Signal<Option<usize>>,
    bounds:        Cell<Rect>,
    dirty:         Rc<Cell<bool>>,
    subs:          Vec<SubscriptionHandle>,
}

struct ListSlot {
    widget:    Box<dyn ViNode>,
    bound_idx: Option<usize>,   // which data index this slot is currently showing
}
```

**Layout:**
1. Compute `first_visible_idx = floor(scroll_y / item_height)`
2. `last_visible_idx = first_visible_idx + slot_count`
3. For each slot i (0..slot_count): bind to data idx `first_visible_idx + i`
4. If `slot.bound_idx != target_idx`: call `provider.build_item(target_idx)` → replace slot widget

**Optimization:** không rebuild slot widget nếu `bound_idx == target_idx` (scroll didn't change which items are visible).

**Fling decay:** giữ nguyên từ implementation hiện tại — `scroll_velocity` decays theo `friction` constant.

**Selection:** khi click → compute `clicked_idx = first_visible_idx + slot_offset` → update Signal<Option<usize>>.

### Migration path

1. Rename current `list_view.rs` struct → `StaticListView` (hoặc giữ tên, tạo alias)
2. New `ListView` dùng `ListDataProvider`
3. `pub use` cả hai từ `node_widgets.rs`
4. vi-compiler codegen: `"ListView"` → new virtual ListView (breaking nếu app dùng old API)

---

## Related Code Files

### Sửa
- `libs/viui/src/node_widgets/list_view.rs` — refactor toàn bộ: ListDataProvider trait, VecProvider, virtual ListView
- `libs/viui/src/node_widgets.rs` — update pub use (StaticListView nếu cần backward compat)
- `cells/apps/robot-dashboard/src/main.rs` — migrate sang VecProvider<LogEntry>

### Tạo mới (optional)
- `libs/viui/src/node_widgets/static_list_view.rs` — old implementation nếu rename

---

## Implementation Steps

1. **ListDataProvider trait** — define trong list_view.rs header
2. **VecProvider<T>** — impl ListDataProvider với Signal<Vec<T>> + builder Fn
3. **ListSlot struct** — widget slot với bound_idx tracking
4. **ListView virtual rendering** — layout (slot binding), paint (slot-offset transform), scroll
5. **Fling decay** — port từ implementation hiện tại
6. **Selection** — click → compute idx, update Signal
7. **Scrollbar** — dùng `provider.item_count() * item_height` cho total height
8. **Migrate robot-dashboard** — thay Vec<Box<dyn ViNode>> bằng VecProvider<LogEntry>
9. **Benchmark** — test với 10k items: build + scroll 10 frames, measure paint time

---

## Todo List

- [ ] Define ListDataProvider trait
- [ ] Implement VecProvider<T>: ListDataProvider impl
- [ ] ListSlot struct
- [ ] Virtual ListView struct + ViNode impl (layout, paint, event)
- [ ] Port fling decay từ old impl
- [ ] Port scrollbar rendering
- [ ] Update node_widgets.rs pub use
- [ ] Migrate robot-dashboard sang VecProvider
- [ ] Manual benchmark: 10k items, check paint time < 2ms (hoặc note QEMU overhead)
- [ ] `cargo check -p viui` pass

---

## Success Criteria

- ListView với 10k items không lag scroll (< 2ms paint per frame trên QEMU TCG)
- Slot count = `ceil(viewport/item_height) + 2` — đúng số widget được instantiate
- Scroll xuống 5000 items → item 5000 hiển thị đúng (correct idx binding)
- Selection click đúng idx
- Fling decay hoạt động (touch up → coast + decelerate)
- Robot-dashboard log list vẫn hoạt động sau migration
- `cargo check` pass

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Variable height items không support | High | Document clearly: fixed height required, offer StaticListView cho variable |
| Slot rebind tạo jank khi scroll nhanh | Medium | Increase buffer: slot_count = viewport + 4 buffer rows |
| build_item gọi nhiều lần cùng idx (bounce scroll) | Low | Cache last slot.bound_idx, skip rebuild if same |
| Robot-dashboard migration break layout | Low | VecProvider giữ nguyên item_height và row builder pattern |

---

## Security Considerations

build_item index bounds: guard `if index >= provider.item_count() { return empty_widget }`. Không crash trên invalid scroll positions.

---

## Next Steps

Sau P05: ListView scale lên server log viewer với 100k+ entries. Kết hợp với P03 Charts cho full data visualization stack.
