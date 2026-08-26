# Dossier 3c — virtqueue parser fuzz + memory-backend refactor

**Cho:** P06 · **Nguồn:** research fuzz (a5fed…) 2026-07-12 · **Trạng thái:** ready-to-code · **KHẢ THI XÁC NHẬN** (tiền lệ rust-vmm)

## Kết luận chốt — tính khả thi refactor ĐÃ được chứng minh
rust-vmm/`vm-virtio` fuzz **chính xác class code này** hôm nay: `fuzz/fuzz_targets/virtio_queue.rs` deserialize bytes → `VirtioQueueInput` (structure-aware qua bincode/serde) → dựng `GuestMemoryMmap` → viết desc chain qua `MockSplitQueue::create` → gọi **chính** `virtio_queue::Queue` production. ⇒ F3 refactor không phải nhảy vào bóng tối; có bản đồ.

## Thiết kế refactor (giải F3)
1. **Tách parser thành crate `#![no_std]` riêng**, KHÔNG định nghĩa `#[panic_handler]`/`#[global_allocator]` (chúng ở cell binary). Đây là điều kiện DUY NH ẤT để crate std (fuzz harness) depend không xung đột `panic_impl`.
2. **Trait `GuestMem` tối thiểu** (mô phỏng *shape* vm-memory, KHÔNG depend vm-memory — nó std-only, không có no_std feature):
   ```
   trait GuestMem { fn read(&self, gpa: GuestAddr, buf: &mut [u8]) -> Result<(), E>;
                    fn write(&mut self, gpa: GuestAddr, buf: &[u8]) -> Result<(), E>; }
   ```
   `GuestAddr` = newtype u64 (như `VAddr`/`PAddr`). KHÔNG copy region-crossing của vm-memory (YAGNI — guest mem là 1 vùng phẳng).
3. **Generic `<M: GuestMem>` — KHÔNG `&dyn`.** Production đường nóng monomorphize zero-cost (đúng lựa chọn rust-vmm: `fn is_valid<M: GuestMemory>`). vm-memory `GuestMemory` còn *không dyn-compatible* (trả `impl Iterator`). Trait 2-method của ta thì dyn được nhưng vô ích — thêm 1 indirection/desc read, không lợi.
4. **Một parser, hai impl, gated bằng trait boundary — KHÔNG `#[cfg(fuzzing)]` trong parser:**
   - Production cell: `SyscallGuestMem` (gọi `crate::vmm::read_guest_memory`).
   - Fuzz host: `VecGuestMem` (Vec phẳng). Mock đặt sau `test-utils` feature như convention rust-vmm.
   - ⇒ Byte + control-flow **y hệt** giữa production và fuzz (đóng đúng lo ngại F3 "divergent copy").

## Công cụ
- **cargo-fuzz (libFuzzer) = primary** — coverage-guided, tìm đúng class "chain thù địch" (flags méo, `next` ngoài ring, self-loop) mà Strategy viết tay khó nghĩ ra. Harness crate là std binary depend crate parser no_std — không cần điều chỉnh no_std trong harness. Cần nightly + LLVM sanitizer.
- **proptest = lưới correctness pre-fuzz** chạy trong `cargo test` trên **bản host (std)** của parser (round-trip length, loop-termination). KHÔNG fight proptest no_std mode (yếu: mất persistence/fork, cần nightly, seed cố định) — chạy trên host build là đủ.
- **Toolchain: WSL2** (repo đã dùng cho mlibc/Tier-1b) — bớt 1 toolchain so với ASan Windows-native (ít battle-tested).
- (Ghi chú) rust-vmm còn dùng **Kani** model-check song song fuzz — angle formal bổ sung, cân nhắc sau.

## Fuzz targets (từ threat-model P02)
- desc chain vòng lặp / self-referential; `next`/`cur` ngoài biên → **clamp `cur < q_size`** (A1); `len` tràn; `avail_idx` nhảy → **cap delta ≤ q_size** (M2); `buf.writable` mismatch chiều device (assert thiếu, `virtqueue.rs:69`).
- Property (proptest): used-ring length accounting đúng; luôn terminate; không panic với input bất kỳ.
- Robustness Mn3: `blk_read` phải xử lý return của `write_guest_memory` (hiện bỏ, `:96`); `blk_write` không phụ thuộc sentinel `usize::MAX` giòn (`:111`).

## Ready-to-code checklist (P06)
- [ ] Tách crate parser `no_std` (không panic_handler/allocator); verify `cargo build` cho cả no_std target lẫn làm dep của std test-bin.
- [ ] Định nghĩa `GuestMem` + `GuestAddr`; refactor `process_notify` + caller (blk/net/console) sang `<M: GuestMem>`.
- [ ] `SyscallGuestMem` (production) + `VecGuestMem` (test-utils feature).
- [ ] fuzz crate riêng (workspace escape như rust-vmm `[workspace] members=[]`), depend path + `libfuzzer-sys`.
- [ ] Targets ở trên; clamp + assert + Mn3 fixes.
- [ ] Cap IRQ queue depth + avail delta (C1) — kernel `registry.rs:398`.
- [ ] Giữ smoke no_std-binary phụ để bắt regression API core-only mà fuzz std không thấy.

## Rủi ro / mở
- `arbitrary` crate thay bincode/serde nếu derive khó cho layout 16-byte desc — follow-up.
- Verify empirical trên cây Cellos (chưa có `GuestMem`/parser tách) — làm ở bước 1.
