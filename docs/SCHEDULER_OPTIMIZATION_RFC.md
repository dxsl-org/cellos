# Scheduler Optimization Design (Phase 20)

## Overview
The current ViOS Scheduler is a simple **Round-Robin** (RR) system with a single FIFO queue. To support real-time requirements, multi-core hardware, and complex workloads, we propose upgrading to an **MLFQ (Multi-Level Feedback Queue)** architecture with SMP support.

## 1. Multi-Level Feedback Queue (MLFQ)
Replace the single `VecDeque` with multiple queues representing priority levels.
*   **Queues**: 4-5 Priority Levels (0=Highest to 4=Lowest).
*   **Rules**:
    1.  **Selection**: Always run the highest priority task found.
    2.  **Quantum**: Higher priority = Shorter Time Quantum (e.g., 10ms). Lower priority = Longer Quantum (e.g., 50ms).
    3.  **Feedback**:
        *   Task uses full quantum -> Demote priority (CPU-bound).
        *   Task yields/blocks before quantum -> Promote/Keep priority (IO-bound).
*   **Aging**: To prevent starvation, periodically promote all tasks to top priority.

## 2. SMP Support (Multicore)
*   **Per-CPU Scheduler**: Each Core (Hart) has its own Scheduler instance and "Local Ready Queue".
*   **Load Balancing**:
    *   **Work Stealing**: If a Cores runs out of tasks, it steals from another Core's queue.
    *   **Periodic Balancing**: Dedicated task to balance queue lengths.
*   **Affinity**: Allow pinning tasks to specific cores (`start_on_core(id)`).

## 3. Priority Inheritance
*   **Problem**: Priority Inversion (Low priority holds lock needed by High priority).
*   **Solution**: When High priority blocks on Mutex held by Low priority, temporarily boost Low priority to High.
*   **Implementation**: Update `Mutex`/`Futex` logic to track owners and boost priorities in `Scheduler`.

## 4. Signal Handling (POSIX-lite)
*   Mechanism for Inter-Process interruptions.
*   **Signals**: `SIGKILL`, `SIGTERM`, `SIGCHLD`, `SIGALRM`.
*   **Implementation**:
    *   Add `pending_signals` bitmap to `Task`.
    *   Check signals on return from Trap/Syscall.
    *   Register User-space Signal Handlers.

## 5. Optimized Timer & Idle
*   **Timer Heap**: Replace linear scan in `pick_next` with a Min-Heap (Binary Heap) for `Sleeping` tasks. O(1) peek, O(log N) insert.
*   **Idle Task**:
    *   Created per-core.
    *   Runs when no other task is ready.
    *   Executes `wfi` (Wait For Interrupt) to save power.

## Implementation Steps
1.  **Refactor**: Split `Scheduler` into `PerCoreScheduler` and `GlobalScheduler`.
2.  **Queues**: Implement `PriorityQueue` struct.
3.  **SMP**: Update `virtio_hal` and context switching for multicore.
4.  **Signals**: Add signal dispatch logic in `trap.rs`.
