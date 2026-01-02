# IPC Zero-Copy Architecture Design (Phase 19)

## Overview
Current ViOS IPC uses **Single-Copy** mechanism (`memcpy` from Sender to Receiver). While efficient for small messages, it incurs CPU overhead for large data transfers (e.g., framebuffer, file streams).
This document proposes a **Zero-Copy** architecture using **Grant Tables** and **Move Semantics**.

## Hybrid Strategy
To balance latency and throughput, we define two transport paths based on message size:

### 1. Fast Path (Registers)
- **Threshold**: Messages < 128 Bytes (2 Cache Lines).
- **Mechanism**:
  - Use CPU registers (a0-a7) and a small dedicated `FastIPC` buffer in Process Control Block (PCB).
  - No memory mapping or complex logic.
  - **Goal**: Ultra-low latency for signals, events, and small command packets.

### 2. Slow Path (Zero-Copy)
- **Threshold**: Messages >= 128 Bytes.
- **Mechanism**: **Grant Table** (inspired by seL4).
- **Logic**:
  1. **Grant**: Sender grants access to a specific memory page (Physical Frame) to Receiver.
  2. **Map**: Kernel maps the physical frame into Receiver's virtual address space (or just returns Physical Address in SAS model).
  3. **Move Semantics**:
     - To ensure safety, ownership is **Tranferred** (Moved).
     - Sender **loses access** to the page (kernel unmaps/invalidates it for Sender).
     - Receiver gains full ownership.
     - *Alternative*: **Share Semantics** (Time-limited Lease) for read-only scenarios (like `ipc_lend`).

## Implementation Roadmap
1. **Grant Table Structure**:
   - Add `GrantTable` to `Task` struct.
   - Entries: `(TargetTaskID, PhysAddr, Len, Permissions)`.
2. **IPC Syscall Update**:
   - `sys_ipc_send` detects size.
   - If large, creates a Grant Entry and passes `GrantIndex` to Receiver.
3. **Receiver Logic**:
   - `sys_ipc_recv` receives `GrantIndex`.
   - Receiver calls `sys_map_grant(index)` to access data.

## Safety Considerations
- **Life Cycle**: Who frees the memory?
  - With Move Semantics, Receiver creates a new connection to the Frame. Receiver drops it when done.
- **Revocation**: Sender can force-revoke access? (Necessary for killing hung processes).

## References
- seL4 Capability System.
- Fuchsia Zircon Channel/VMO.
