# Phase P07 — Elm Facade (iced-compatible API)

**Step**: 3 (Windows — entry point)  
**Priority**: P2  
**Status**: 📋 Planned  
**Effort est.**: 3-4 ngày  
**Depends on**: P04, P05

---

## Context Links

- [docs/specs/14-viui.md](../../docs/specs/14-viui.md) §5 Elm Facade

---

## Overview

Hoàn chỉnh Elm facade với iced-compatible builder pattern: `text()`, `button()`, `column![]`, `row![]`, `Element<Msg>` chaining. Phase này làm cho developer quen iced viết ViUI Elm apps mà không cần học API mới.

---

## Architecture

```rust
// Free functions — iced-compatible
pub fn text<Msg>(content: impl Into<String>) -> Element<Msg>;
pub fn button<Msg>(content: impl Into<Element<Msg>>) -> Button<Msg>;
pub fn column<Msg>(children: Vec<Element<Msg>>) -> Element<Msg>;
pub fn row<Msg>(children: Vec<Element<Msg>>) -> Element<Msg>;
pub fn checkbox<Msg>(checked: bool, label: &str) -> Element<Msg>;
pub fn scrollable<Msg>(content: Element<Msg>) -> Element<Msg>;
pub fn image<Msg>(pixels: &[u8], w: u32, h: u32) -> Element<Msg>;
pub fn space<Msg>(w: f32, h: f32) -> Element<Msg>;

// Builder chaining
impl<Msg> ButtonBuilder<Msg> {
    pub fn on_press(self, msg: Msg) -> Element<Msg>;
    pub fn padding(self, px: f32) -> Self;
    pub fn style(self, style: ButtonStyle) -> Self;
}

// Macro — iced-compatible syntax
#[macro_export]
macro_rules! column { ($($e:expr),* $(,)?) => { ... } }
#[macro_export]
macro_rules! row { ($($e:expr),* $(,)?) => { ... } }
```

### Element<Msg> type erasure
```rust
pub struct Element<Msg> {
    inner: Box<dyn ErasedWidget<Msg>>,
}
trait ErasedWidget<Msg> {
    fn layout(&self, cx: &LayoutCx, constraints: Constraints) -> Size;
    fn paint(&self, cx: &mut PaintCx);
    fn event(&mut self, cx: &mut EventCx, e: &Event) -> Option<Msg>;
}
```

`on_press(msg)` lưu `Msg` vào `ButtonWidget.on_press: Option<Msg>` và trả về khi click.

---

## Implementation Steps

1. `Element<Msg>` + `ErasedWidget<Msg>` trait
2. Free functions: `text`, `button`, `column`, `row`, `checkbox`, `scrollable`, `image`, `space`
3. `ButtonBuilder<Msg>` với `.on_press()`, `.padding()`, `.style()`
4. `column![]` + `row![]` macros
5. Update `run_app` runner dùng `Element<Msg>` → dispatch Msg → `app.update(msg)`
6. `cargo check -p viui`

---

## Todo

- [ ] ErasedWidget<Msg> trait
- [ ] Element<Msg> wrapper
- [ ] text() free function
- [ ] button() + ButtonBuilder + on_press
- [ ] column() + row()
- [ ] checkbox()
- [ ] scrollable()
- [ ] image()
- [ ] space()
- [ ] column![] + row![] macros
- [ ] run_app Elm event loop
- [ ] cargo check clean

---

## Success Criteria

```rust
// Iced-style app compile + run:
#[derive(Debug, Clone)]
enum Message { Increment, Decrement }

struct Counter { value: i32 }

impl ViApp for Counter {
    type Message = Message;
    fn view(&self) -> Element<Message> {
        column![
            button("Increment").on_press(Message::Increment),
            text(format!("Value: {}", self.value)),
            button("Decrement").on_press(Message::Decrement),
        ]
    }
    fn update(&mut self, msg: Message) {
        match msg {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }
}
```

Compile clean, render đúng, button click update counter.

---

## Next Steps

→ P08: Multi-window + Window Chrome (Step 3 completion)
