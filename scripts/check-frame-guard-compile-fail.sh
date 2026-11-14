#!/usr/bin/env bash
# Compile-fail witness for the frame-allocator guard (Phase 16, census §7.3).
#
# `cellos-kernel` is a bin crate, so it has no doctests: the "a shared TLB flush
# cannot be issued while the frame allocator is held" rule is proved here with
# rustc on a model of the guard API.
#
#   1. The real signatures in kernel/src are checked against the model, so the
#      model cannot drift from the code it stands for.
#   2. The correct order (lock, release, flush) compiles.
#   3. A flush while an `AllocGuard` lives fails with E0499 (the guard borrows
#      the only cap mutably).
#   4. A `with` closure that captures the cap fails with E0277 (the closure
#      must be `Send`; the cap is not).
#
# Exit 0 when all hold, 1 otherwise. Needs only `rustc`.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail() {
    printf 'frame-guard witness: %s\n' "$1" >&2
    exit 1
}

frame=kernel/src/memory/frame.rs
shootdown=kernel/src/memory/tlb_shootdown.rs

# --- 1. the model matches the code -------------------------------------------
require() {
    grep -qF -- "$2" "$1" || fail "$1 no longer has: $2"
}
require "$frame" 'static FRAME_ALLOCATOR: crate::sync::Spinlock<Option<FrameAllocator>>'
if grep -qE '^pub[^ ]* static FRAME_ALLOCATOR' "$frame"; then
    fail "FRAME_ALLOCATOR must stay private to $frame"
fi
require "$frame" '_not_send: core::marker::PhantomData<*mut ()>,'
require "$frame" 'pub fn take() -> Option<FlushCap> {'
require "$frame" "_cap: core::marker::PhantomData<&'c mut FlushCap>,"
require "$frame" "pub fn lock(cap: &mut FlushCap) -> AllocGuard<'_> {"
require "$frame" 'pub fn with<R>(f: impl FnOnce(&mut FrameAllocator) -> R + Send) -> Option<R> {'
if grep -nE 'FRAME_ALLOCATOR' kernel/src -r --include='*.rs' \
    | grep -vE '^kernel/src/memory/frame\.rs:' \
    | grep -vE ':[0-9]+:\s*(//|\*)' \
    | grep -vE 'rank::FRAME_ALLOCATOR|^kernel/src/sync/rank\.rs:'; then
    fail 'FRAME_ALLOCATOR is named outside memory/frame.rs (use frame::lock / frame::with)'
fi
if [[ -f "$shootdown" ]] && grep -q 'cap: &mut FlushCap' "$shootdown"; then
    for fn in flush_range flush_page flush_ranges; do
        if grep -qE "pub fn $fn\(" "$shootdown"; then
            grep -qE "pub fn $fn\(\s*cap: &mut FlushCap" "$shootdown" \
                || fail "tlb_shootdown::$fn must take the FlushCap first"
        fi
    done
fi

# --- 2-4. the model ------------------------------------------------------------
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/model.rs" <<'EOF'
// Model of kernel/src/memory/frame.rs's guard API and a capped shared flush.
#![allow(dead_code)]
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard};

pub struct FrameAllocator { used: usize }
static FRAME_ALLOCATOR: Mutex<Option<FrameAllocator>> = Mutex::new(None);

#[must_use]
pub struct FlushCap { _not_send: PhantomData<*mut ()> }
impl FlushCap {
    pub fn take() -> Option<FlushCap> { Some(FlushCap { _not_send: PhantomData }) }
}

pub struct AllocGuard<'c> {
    guard: MutexGuard<'static, Option<FrameAllocator>>,
    _cap: PhantomData<&'c mut FlushCap>,
}
impl std::ops::Deref for AllocGuard<'_> {
    type Target = Option<FrameAllocator>;
    fn deref(&self) -> &Self::Target { &self.guard }
}
impl std::ops::DerefMut for AllocGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

pub fn lock(cap: &mut FlushCap) -> AllocGuard<'_> {
    let _ = cap;
    AllocGuard { guard: FRAME_ALLOCATOR.lock().unwrap(), _cap: PhantomData }
}

pub fn with<R>(f: impl FnOnce(&mut FrameAllocator) -> R + Send) -> Option<R> {
    FRAME_ALLOCATOR.lock().unwrap().as_mut().map(f)
}

pub enum FlushOutcome { Confirmed }
pub fn flush_range(cap: &mut FlushCap, start: usize, size: usize) -> FlushOutcome {
    let _ = (cap, start, size);
    FlushOutcome::Confirmed
}
EOF

case_file() {
    local name=$1
    cp "$work/model.rs" "$work/$name.rs"
    cat >> "$work/$name.rs"
}

compile() {
    rustc --edition 2021 --crate-type lib --emit=metadata \
        -o "$work/$1.rmeta" "$work/$1.rs" 2> "$work/$1.err"
}

case_file correct_order <<'EOF'
pub fn release(mut cap: FlushCap) {
    {
        let mut guard = lock(&mut cap);
        if let Some(allocator) = guard.as_mut() { allocator.used -= 1; }
    }
    let _ = flush_range(&mut cap, 0x1000, 0x1000);
    let _ = with(|allocator| allocator.used += 1);
    let _ = flush_range(&mut cap, 0x1000, 0x1000);
}
EOF
compile correct_order || { cat "$work/correct_order.err" >&2; fail 'the correct order does not compile'; }

case_file flush_under_guard <<'EOF'
pub fn release(mut cap: FlushCap) {
    let mut guard = lock(&mut cap);
    let _ = flush_range(&mut cap, 0x1000, 0x1000);
    if let Some(allocator) = guard.as_mut() { allocator.used -= 1; }
}
EOF
if compile flush_under_guard; then
    fail 'a flush while the AllocGuard lives compiled'
fi
grep -q 'E0499' "$work/flush_under_guard.err" \
    || { cat "$work/flush_under_guard.err" >&2; fail 'flush under the guard failed for another reason than E0499'; }

case_file flush_in_closure <<'EOF'
pub fn release(mut cap: FlushCap) {
    let _ = with(|allocator| {
        allocator.used -= 1;
        flush_range(&mut cap, 0x1000, 0x1000)
    });
}
EOF
if compile flush_in_closure; then
    fail 'a flush inside a frame::with closure compiled'
fi
grep -q 'E0277' "$work/flush_in_closure.err" \
    || { cat "$work/flush_in_closure.err" >&2; fail 'flush in the closure failed for another reason than E0277'; }

echo 'frame-guard witness: correct order compiles; flush under the guard (E0499) and in a with-closure (E0277) do not'
