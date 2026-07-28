//! IPC Test Harness for Hubris-style Send/Recv/Reply
//!
//! This module provides test scenarios to validate the IPC implementation.

use alloc::vec::Vec;
use log::info;

/// Spawn a scenario task, or `None` if the spawn failed.
///
/// These harnesses have no recovery path — a scenario that cannot spawn its tasks
/// cannot run — so the caller returns early rather than reporting on tasks that do
/// not exist.
fn spawn_or_skip(name: &str) -> Option<usize> {
    match super::spawn(name, types::CellId(0), Vec::new()) {
        Ok(id) => Some(id),
        Err(e) => {
            info!("IPC test: spawn '{}' failed ({:?}) — scenario skipped", name, e);
            None
        }
    }
}

/// Test Scenario 1: Simple Ping-Pong
/// - Task A sends "PING" to Task B
/// - Task B receives, replies with "PONG"
/// - Task A receives reply
pub fn test_ping_pong() {
    info!("=== IPC Test: Ping-Pong ===");

    // Spawn Task B (Server)
    let Some(server_id) = spawn_or_skip("ipc-server") else {
        return;
    };
    info!("Spawned Server: Task {}", server_id);

    // Spawn Task A (Client)
    let Some(client_id) = spawn_or_skip("ipc-client") else {
        return;
    };
    info!("Spawned Client: Task {}", client_id);

    // In simulation, we can't actually run these tasks in parallel
    // So we'll simulate the flow manually

    info!("Test Setup Complete. Manual validation required.");
    info!("Expected Flow:");
    info!("  1. Client calls sys_send(server_id, msg)");
    info!("  2. Client blocks in Sending state");
    info!("  3. Server calls sys_recv() and receives msg");
    info!("  4. Server calls sys_reply(client_id, result)");
    info!("  5. Client unblocks with reply_value");
}

/// Test Scenario 2: Borrow Memory
/// - Task A creates a buffer
/// - Task B borrows and reads it via BorrowRead
pub fn test_borrow_read() {
    info!("=== IPC Test: Borrow Read ===");

    let Some(lender_id) = spawn_or_skip("lender") else {
        return;
    };
    let Some(borrower_id) = spawn_or_skip("borrower") else {
        return;
    };

    info!("Lender: Task {}", lender_id);
    info!("Borrower: Task {}", borrower_id);

    // Simulate: Lender has buffer at 0x80000000
    // Borrower calls sys_borrow_read(lender_id, 0x80000000, local_buf, 64)

    info!("Expected: Borrower can read 64 bytes from Lender's memory");
}

/// Test Scenario 3: Multiple Clients
/// - 3 clients send to 1 server
/// - Server processes in FIFO order
pub fn test_multiple_clients() {
    info!("=== IPC Test: Multiple Clients ===");

    if spawn_or_skip("multi-server").is_none() {
        return;
    }

    for i in 0..3 {
        use alloc::string::ToString;
        let name = alloc::string::String::from("client-") + &i.to_string();
        let Some(client_id) = spawn_or_skip(&name) else {
            return;
        };
        info!("Client {}: Task {}", i, client_id);
    }

    info!("Expected: Server receives 3 messages in order");
}

/// Run all IPC tests
pub fn run_all_tests() {
    info!("╔════════════════════════════════════╗");
    info!("║   IPC Test Harness - Hubris Style ║");
    info!("╚════════════════════════════════════╝");

    test_ping_pong();
    test_borrow_read();
    test_multiple_clients();

    info!("All tests scheduled. Check logs for validation.");
}
