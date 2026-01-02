# 08. Reference Integration Strategy ("The Great Theft")

To accelerate ViOS development and ensure robustness, we explicitly adopt proven patterns from existing high-assurance operating systems. This document serves as the "Source of Truth" for which components are adapted (stolen) from where.

## 1. Core Architecture: Hubris (Oxide Computer)

We align our Kernel ABI and IPC model with [Hubris](https://github.com/oxidecomputer/hubris).
Hubris is chosen for its focus on reliability and eliminating "dynamic" undefined behavior at the system level.

### Adopted Patterns:
-   **Synchronous IPC**: `Send` / `Recv` / `Reply` blocking semantics.
    -   *Why*: Simplifies reasoning about state. No message queues filling up and causing OOM.
-   **Task Identity**: `TaskId` includes a **generation count**.
    -   *Why*: Prevents "ABA problems" where a task restarts and peers confusingly talk to the new instance thinking it's the old one.
-   **Notification Masks**: Simple 32-bit signal masks for interrupts/events.
-   **Zero-Copy Leases**: `BorrowRead` / `BorrowWrite` (similar to Tock's Allow).
    -   *Why*: Efficient transfer of large buffers (Network/Display) without memcpy in kernel.

### Divergences:
-   **Dynamic Loading**: ViOS supports loading "Cells" at runtime (Theseus style). Hubris is static. We wrap Hubris-style tasks in dynamic headers.

## 2. Driver & HAL Layer: Embassy (Rust Embedded)

We adopt [Embassy](https://github.com/embassy-rs/embassy) as the standard for hardware interaction within Cells.

### Adopted Patterns:
-   **Async/Await Drivers**: Drivers inside Cells should use `async` Rust.
    -   *Why*: Rust's state machines compile down to very efficient code, handling interrupts and state waiting naturally.
-   **Hardware Traits**: Use `embedded-hal` and `embedded-hal-async`.
    -   *Why*: Allows swapping `vios-driver-gpio` with a mock for testing, or a specific hardware implementation (STM32, nRF, VirtIO) without rewriting logic.
-   **Channels**: Adapt `embassy-sync::channel` for safe intra-cell communication.

## 3. Filesystem & Resources: Redox OS

We adopt the "Everything is a URL" scheme from [Redox](https://gitlab.redox-os.org/redox-os/redox).

### Adopted Patterns:
-   **Schemes**: `file:`, `tcp:`, `display:`, `input:`.
-   **Resource Handles**: Opening a URL returns a file descriptor (ID).
-   *Implementation*: Our `vios-vfs` module already starts this, but we will strictly define schemes to match standard URLs.

## 4. Integration Roadmap

### Step 1: Kernel ABI Calibration (The Hubris Standard)
-   [ ] Rename/Refactor `Syscall` enum to match Hubris `Sysnum`.
-   [ ] Implement `TaskId` with Generation counting.
-   [ ] Implement `sys_borrow_read` / `sys_borrow_write` leases.

### Step 2: Userspace Runtime (The Embassy Standard)
-   [ ] Create `ostd::async`: A lightweight async runtime for Cells.
-   [ ] Port `Waker` logic from `embassy-executor` to `ostd`.
-   [ ] Implement `ostd::channel` using `embassy-sync` logic.

### Step 3: Library "Copy-Paste"
-   [ ] Copy `zerocopy` usage for safe ABI parsing (already in dependencies).
-   [ ] Copy `volatile-register` patterns for MMIO.

## 5. Directory Map

| ViOS Component | Reference Source | Path in `.reference` |
| :--- | :--- | :--- |
| `kernel/abi` | Hubris ABI | `hubris/sys/abi` |
| `kernel/sched` | Hubris Scheduler | `hubris/sys/kerncore` |
| `libs/ostd/sync` | Embassy Sync | `embassy/embassy-sync` |
| `libs/ostd/task` | Embassy Executor | `embassy/embassy-executor` |
