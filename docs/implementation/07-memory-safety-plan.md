# Phase 12: Memory Safety & Lease System Implementation Plan

## 1. Objective
Replace the current Unsafe IPC Pointer implementation (`ipc_borrow_read` / `ipc_borrow_write` using raw pointers) with a secure **Lease System**. This ensures tasks can only access memory they have been explicitly granted permission to access by the owner.

## 2. Architecture: The Lease System

### 2.1 Concepts
- **Owner**: The task that owns the memory (e.g., the Driver creating a buffer).
- **Borrower**: The task borrowing the memory (e.g., the Window Manager reading the buffer).
- **Lease**: A capability token granted by the Owner to the Borrower.
- **Lease ID**: A unique handle used by the Borrower to reference the memory.

### 2.2 Lease Structure
Already defined in `kernel/src/process/task.rs`:
```rust
pub struct Lease {
    pub id: usize,    // Logic Lease ID (Handle)
    pub ptr: usize,   // Physical/Virtual Address in Owner's Space
    pub len: usize,   // Length
    pub attributes: LeaseAttributes, // READ | WRITE
}
```

### 2.3 Workflow
1.  **Lending**: Task A (Owner) calls `ipc_lend(ptr, len, target_pid, permissions)`.
    - Kernel verifies `ptr` + `len` is within Task A's valid memory (Stack/Heap).
    - Kernel generates a `LeaseID`.
    - Kernel adds `Lease` to Task A's "Lent Table" (optional, for revocation) and Task B's "Borrowed Table".
    - Returns `LeaseID` to Task A, which sends it to Task B via IPC message.
2.  **Borrowing**: Task B (Borrower) calls `ipc_borrow_read(lease_id, offset, len, dest_ptr)`.
    - Kernel looks up `lease_id` in Task B's lease table.
    - Kernel validates `offset + len <= lease.len`.
    - Kernel validates `READ` permission.
    - Kernel performs copy from `lease.ptr + offset` to `dest_ptr`.
3.  **Revocation**: Task A calls `ipc_revoke(lease_id)`.
    - Kernel removes Lease from Task B. Future access fails.

## 3. Implementation Steps

### Step 3.1: Enhance Task Struct
- Add `leases: BTreeMap<usize, Lease>` to `Task`.
- Add `next_lease_id` counter.

### Step 3.2: Implement `ipc_lend` Syscall (Kernel Side)
- Create `kernel::process::ipc_lend(lender_id, target_id, ptr, len, flags) -> Result<usize, Error>`.
- Validate memory range (basic check: non-null, user-space range).
- Create `Lease` object.
- Push to target task's lease map.

### Step 3.3: Refactor `ipc_borrow_read` / `ipc_borrow_write`
- **Current**: `ipc_borrow_read(caller_id, lender_id, src_ptr, dst_ptr, len)` -> Unsafe!
- **New**: `ipc_borrow_read(caller_id, lease_id, offset, dst_ptr, len)`.
- **Logic**:
    - `let lease = task.leases.get(lease_id)?;`
    - `if !lease.attributes.contains(READ) { return Err(PermissionDenied); }`
    - `if offset + len > lease.len { return Err(OutOfBounds); }`
    - `let src_addr = lease.ptr + offset;`
    - `unsafe { copy(src_addr, dst_ptr, len); }`

### Step 3.4: Update Userspace (`ostd`)
- Update `ostd::ipc` to expose `lend` and updated `borrow` functions.
- Update `vios-hello` example to use the new API.

## 4. Verification
- Create a test where Task A lends a buffer to Task B.
- Task B reads it successfully.
- Task B tries to read *past* the end -> Fails.
- Task B tries to write to a Read-Only lease -> Fails.
