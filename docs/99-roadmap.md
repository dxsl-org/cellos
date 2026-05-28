# ViOS Development Roadmap

This document outlines the strategic plan for stabilizing and expanding ViOS.

## 🟢 Phase 1: Core Stability (Current Priority)
**Goal**: Get the kernel to boot reliably into user-space `init` process without hangs.

- [x] **Build Verification**
  - [x] Run clean build `cargo build --release`
  - [x] Ensure 0 warnings in `kernel` and `api`.
- [ ] **Fix Boot Hang (`init_kernel_paging`)**
  - [ ] Verify `intrinsics.rs` implementation (memset, memcpy).
  - [ ] Debug `paging.rs` identity mapping.
- [ ] **User-space Execution**
  - [ ] Verify ELFLoader correctly maps segments.
  - [ ] Test context switch to Ring 3 (User Role).
  - [ ] Achieve "Hello from Userspace" output.

## 🟡 Phase 2: Input & Shell Interaction
**Goal**: Enable interactive shell usage.

- [ ] **Keyboard Driver**
  - [ ] Fix sticky keys/buffering issues.
  - [ ] Ensure interrupt-driven input works reliably.
- [ ] **Shell Application**
  - [ ] specific fix for "Enter" key handling.
  - [ ] Implement command history buffer.

## 🔵 Phase 3: Runtimes & Applications
**Goal**: Run dynamic applications (Lua).

- [x] **Lua Runtime Port**
  - [x] Complete `cells/runtimes/lua` structure.
  - [ ] Link Lua C sources with `cc` crate.
  - [ ] Implement Rust bindings for Lua 5.4.
- [ ] **VFS Enhancements**
  - [ ] Stabilize FAT32 driver.
  - [ ] Implement `FileHandle` passing between cells.

## ⚪ Phase 4: Optimization & Cleanup
**Goal**: Professionalize the codebase.

- [x] **Refactoring**
  - [x] Remove unused code in `hal` and `kernel`.
  - [x] Enforce "No mod.rs" rule strictly.
- [ ] **Testing**
  - [ ] Add integration tests for Allocator.
  - [ ] Add KUnit tests for Scheduler.
