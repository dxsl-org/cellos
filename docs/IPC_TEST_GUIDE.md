# IPC Test Guide - Hubris Implementation

## Test Harness Created ✅

Đã tạo infrastructure để test IPC:

### 1. Test Module (`kernel/src/process/ipc_test.rs`)
- Test scenarios: Ping-Pong, Borrow, Multiple Clients
- Helper functions để spawn test tasks

### 2. Test Apps (`apps/vios-ipc-test/`)
- **IpcServer**: Receives messages, replies with 0xDEADBEEF
- **IpcClient**: Sends "PING", waits for reply

## How to Test (Manual Validation)

### Current Limitation
- Kernel simulation mode (`std` feature) có issue với `log` crate
- Bare metal mode chưa có proper task execution loop

### Recommended Testing Approach

#### Option 1: Unit Test (Future)
```rust
#[test]
fn test_ipc_send_recv() {
    // Initialize kernel
    kernel::init();
    
    // Spawn server
    let server_id = kernel::process::spawn("server", vec![]);
    
    // Spawn client  
    let client_id = kernel::process::spawn("client", vec![]);
    
    // Simulate: Client sends
    let msg = b"PING";
    kernel::process::ipc_send(client_id, server_id, msg.as_ptr() as usize, msg.len());
    
    // Verify: Client is in Sending state
    // Verify: Server receives message
    // Verify: Reply unblocks client
}
```

#### Option 2: QEMU Trace (Current)
```bash
# Build bare metal
cargo build --no-default-features -p kernel

# Run in QEMU with logging
qemu-system-riscv64 -machine virt -cpu rv64 -m 128M \
    -serial mon:stdio \
    -kernel target/riscv64gc-unknown-none-elf/debug/kernel \
    -d int,cpu_reset
```

**Expected Output:**
```
[INFO] Loader: === Starting IPC Tests ===
[INFO] Loader: IPC Server spawned as Task 4
[INFO] Loader: IPC Client spawned as Task 5
[INFO] IPC Client: Sending 'PING' to Task 4
[INFO] Syscall (Task 5): Dispatched Send { target: 4, ... }
[INFO] IPC: Task 5 -> Sending state (blocked)
[INFO] IPC Server: Waiting for message
[INFO] Syscall (Task 4): Dispatched Recv { ... }
[INFO] IPC: Rendezvous! Copying message...
[INFO] IPC Server: Received from Task 5
[INFO] IPC Server: Message: 'PING'
[INFO] IPC Server: Replying with 0xDEADBEEF
[INFO] Syscall (Task 4): Dispatched Reply { caller: 5, result: 0xDEADBEEF }
[INFO] IPC: Task 5 unblocked, reply_value = 0xDEADBEEF
[INFO] IPC Client: Received reply: 0xDEADBEEF
```

## Validation Checklist

### ✅ Structural Validation (Done)
- [x] Scheduler has task table
- [x] Task states include Sending/Recv
- [x] IPC functions implemented
- [x] Syscall dispatch connected
- [x] Kernel compiles

### ⏳ Behavioral Validation (Pending)
- [ ] Send blocks caller
- [ ] Recv blocks receiver
- [ ] Rendezvous copies data correctly
- [ ] Reply unblocks sender
- [ ] reply_value propagates
- [ ] Multiple clients handled in order

## Known Issues

### Issue 1: Simulation Mode Build Error
**Problem**: `log` crate fails to compile with `std` feature
**Workaround**: Use `no_std` mode only
**Fix**: Update `log` dependency or use different logger

### Issue 2: No Scheduler Loop
**Problem**: Loader executes tasks sequentially, no true concurrency
**Solution**: Implement proper scheduler loop:
```rust
loop {
    scheduler.schedule();
    // Context switch
    // Handle timer interrupts
}
```

### Issue 3: Context Switching Not Implemented
**Problem**: Tasks don't actually switch contexts
**Impact**: IPC blocking won't truly block in current simulation
**Fix**: Implement `arch/trap.rs` with real context save/restore

## Next Steps

1. **Fix std build**: Resolve `log` crate issue
2. **Add scheduler loop**: Implement continuous scheduling
3. **Add context switching**: Real task switching in bare metal
4. **Create integration test**: Automated IPC flow validation
5. **Add tracing**: Detailed IPC event logging

## Manual Test Procedure

1. Build kernel: `cargo build --no-default-features -p kernel`
2. Check logs for IPC test execution
3. Verify task state transitions in debug output
4. Confirm no panics or errors
5. Validate message content and reply values

## Success Criteria

✅ **Minimum**: Kernel compiles, IPC functions callable
⏳ **Target**: Client sends, server receives, reply returns
🎯 **Goal**: Multiple concurrent IPC operations work correctly
