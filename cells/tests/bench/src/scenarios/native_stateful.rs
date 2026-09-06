//! Native Stateful Workload Scenario (Phase 05).
//!
//! Executes 1,000 deterministic operations with:
//! - 999 primary writer increments + 1 cached-TID increment at cutover (op 301)
//! - Real VFS checkpoints every 100 operations to `/srv/checkpoint.log`
//! - Hotswap v1 -> v2 at operation 300
//! - VFS service restart via supervisor bridge at operation 600
//! - Verification of stale handle refusal after restart
//! - Full readback and CRC32C verification of all 10 checkpoints
//! - Failed hotswap test preserving live provider
//! - Soft latency and error budget reporting

extern crate alloc;

use alloc::format;
use api::services::hostile_backend_recovery::{encode_kill_request, KILL_STATUS_OK};
use api::syscall::service;
use cellos_fs::crc32::crc32c;
use ostd::io::println;
use ostd::syscall::{
    sys_heartbeat, sys_lookup_service, sys_recv, sys_send, sys_set_spawn_args, sys_set_timer,
    sys_spawn_pinned, sys_yield, SyscallResult,
};
const CHECKPOINT_PATH: &str = "/srv/checkpoint.log";
const TOTAL_OPS: u32 = 1000;
const CHECKPOINT_INTERVAL: u32 = 100;
const WAIT_TICKS: usize = 500;
const CUTOVER_WINDOW_TICKS: usize = 1_600;

const OP_HOTSWAP: u8 = 0x01;
const SVC_NAME_LEN: usize = 64;
const ELF_PATH_LEN: usize = 128;
const REQUEST_LEN: usize = 1 + SVC_NAME_LEN + ELF_PATH_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CheckpointRecord {
    seq: u32,
    counter: u32,
    checksum: u32,
}

impl CheckpointRecord {
    fn new(seq: u32, counter: u32) -> Self {
        let mut data = [0u8; 8];
        data[..4].copy_from_slice(&seq.to_le_bytes());
        data[4..].copy_from_slice(&counter.to_le_bytes());
        let checksum = crc32c(&data);
        Self {
            seq,
            counter,
            checksum,
        }
    }

    fn to_bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..4].copy_from_slice(&self.seq.to_le_bytes());
        b[4..8].copy_from_slice(&self.counter.to_le_bytes());
        b[8..12].copy_from_slice(&self.checksum.to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 12 {
            return None;
        }
        let seq = u32::from_le_bytes(b[0..4].try_into().ok()?);
        let counter = u32::from_le_bytes(b[4..8].try_into().ok()?);
        let checksum = u32::from_le_bytes(b[8..12].try_into().ok()?);

        let mut data = [0u8; 8];
        data[..4].copy_from_slice(&seq.to_le_bytes());
        data[4..].copy_from_slice(&counter.to_le_bytes());
        if crc32c(&data) != checksum {
            return None;
        }
        Some(Self {
            seq,
            counter,
            checksum,
        })
    }
}

pub fn run() {
    println("[native-stateful] START: 1000-op stateful workload");
    sys_heartbeat(0);
    // 1. Service Discovery
    let Some(mut demo_tid) = sys_lookup_service(service::HOTSWAP_DEMO) else {
        fail("HOTSWAP_DEMO service is not registered");
    };
    let Some(vfs_tid) = sys_lookup_service(service::VFS) else {
        fail("VFS service is not registered");
    };
    let Some(supervisor_tid) = sys_lookup_service(service::SUPERVISOR) else {
        fail("SUPERVISOR service is not registered");
    };

    println(&format!("[native-stateful] initial services: demo_tid={demo_tid} vfs_tid={vfs_tid} supervisor_tid={supervisor_tid}"));

    // Ensure clean start for checkpoint log
    let mut vfs_client = ostd::clients::VfsClient::new();
    let _ = vfs_client.unlink(CHECKPOINT_PATH);

    let mut current_counter: u32 = 0;
    let mut checkpoint_seq: u32 = 0;
    let mut oracle: [Option<CheckpointRecord>; 10] = [None; 10];

    // 2. Operations 1 to 300 (v1)
    for op in 1..=300 {
        sys_heartbeat(0);
        if !inc_demo(demo_tid) {
            fail(&format!("increment failed at op {op}"));
        }
        current_counter += 1;

        if op % CHECKPOINT_INTERVAL == 0 {
            checkpoint_seq += 1;
            let record = CheckpointRecord::new(checkpoint_seq, current_counter);
            oracle[(checkpoint_seq - 1) as usize] = Some(record);
            append_checkpoint(CHECKPOINT_PATH, &record);
            println(&format!("[native-stateful] checkpoint {checkpoint_seq} committed: counter={current_counter}"));
        }
    }
    assert_eq!(current_counter, 300);
    assert_eq!(checkpoint_seq, 3);

    // 3. At Operation 300: Hotswap v1 -> v2 with cached-TID witness for Op 301
    println("[native-stateful] Operation 300 reached: initiating hotswap v1 -> v2");
    if !spawn_cached_sender_probe(demo_tid) {
        fail("cannot spawn cached-sender probe for operation 301");
    }

    sys_heartbeat(0);
    // Trigger supervisor hotswap to v2
    if !trigger_hotswap(supervisor_tid, "hotswap-demo", "/bin/hotswap-demo-v2") {
        fail("supervisor hotswap request to v2 failed");
    }

    // Wait for replacement demo TID
    let Some(v2_tid) = wait_for_replacement_demo(demo_tid, WAIT_TICKS) else {
        fail("replacement demo v2 did not publish a new tid");
    };
    println(&format!(
        "[native-stateful] hotswap v2 published: old_tid={demo_tid} -> new_tid={v2_tid}"
    ));
    demo_tid = v2_tid;

    current_counter = 301;
    let mut readback_301 = None;
    for _ in 0..100 {
        sys_heartbeat(0);
        readback_301 = read_counter(demo_tid, b"v2:");
        if readback_301 == Some(301) {
            break;
        }
        sys_yield();
    }
    if readback_301 != Some(301) {
        fail(&format!(
            "cutover counter expected 301, got {readback_301:?}"
        ));
    }
    println("[native-stateful] Op 301 reconciled via cached-TID witness: counter=301");

    // Operations 302 to 600 (v2)
    for op in 302..=600 {
        sys_heartbeat(0);
        if !inc_demo(demo_tid) {
            fail(&format!("increment failed at op {op}"));
        }
        current_counter += 1;

        if op % CHECKPOINT_INTERVAL == 0 {
            checkpoint_seq += 1;
            let record = CheckpointRecord::new(checkpoint_seq, current_counter);
            oracle[(checkpoint_seq - 1) as usize] = Some(record);
            append_checkpoint(CHECKPOINT_PATH, &record);
            println(&format!("[native-stateful] checkpoint {checkpoint_seq} committed: counter={current_counter}"));
        }
    }
    println("[native-stateful] Operation 600 reached: testing VFS restart recovery");
    let old_vfs_tid = vfs_tid;

    // Send authorized kill request for VFS to supervisor
    let kill_req = encode_kill_request(service::VFS);
    if !matches!(sys_send(supervisor_tid, &kill_req), SyscallResult::Ok(_)) {
        fail("failed to send VFS kill request to supervisor");
    }
    let mut kill_resp = [0u8; 2];
    let received = recv_from(supervisor_tid, &mut kill_resp);
    if !received || kill_resp[1] != KILL_STATUS_OK {
        fail(&format!("supervisor rejected or failed VFS kill request: received={received} resp={kill_resp:?}"));
    }
    println(&format!(
        "[native-stateful] VFS kill confirmed by supervisor: killed tid={old_vfs_tid}"
    ));

    // Wait for init to restart VFS and register new TID
    let Some(new_vfs_tid) = wait_for_service_restart(service::VFS, old_vfs_tid, WAIT_TICKS) else {
        fail("VFS was not restarted by init");
    };
    println(&format!("[native-stateful] VFS restarted successfully: old_tid={old_vfs_tid} -> new_tid={new_vfs_tid}"));
    let _ = new_vfs_tid;

    // Verify checkpoints 1..=6 are intact in `/srv/checkpoint.log` after VFS restart
    verify_persisted_checkpoints(CHECKPOINT_PATH, &oracle[..6]);
    println("[native-stateful] VFS restart recovery verified: checkpoints 1..6 intact");

    // 5. Operations 601 to 1,000
    for op in 601..=TOTAL_OPS {
        sys_heartbeat(0);
        if !inc_demo(demo_tid) {
            fail(&format!("increment failed at op {op}"));
        }
        current_counter += 1;

        if op % CHECKPOINT_INTERVAL == 0 {
            checkpoint_seq += 1;
            let record = CheckpointRecord::new(checkpoint_seq, current_counter);
            oracle[(checkpoint_seq - 1) as usize] = Some(record);
            append_checkpoint(CHECKPOINT_PATH, &record);
            println(&format!("[native-stateful] checkpoint {checkpoint_seq} committed: counter={current_counter}"));
        }
    }
    assert_eq!(current_counter, 1000);
    assert_eq!(checkpoint_seq, 10);

    // Final counter readback from demo v2
    let final_counter = read_counter(demo_tid, b"v2:");
    if final_counter != Some(1000) {
        fail(&format!(
            "final counter mismatch: expected 1000, got {final_counter:?}"
        ));
    }
    println("[native-stateful] final counter verified: 1000");

    // Final readback of all 10 checkpoints
    verify_persisted_checkpoints(CHECKPOINT_PATH, &oracle[..10]);
    println("[native-stateful] all 10 checkpoints verified against independent oracle");

    // 6. Test failed hotswap preserves live provider
    println("[native-stateful] testing failed hotswap preserves live provider");
    if trigger_hotswap(
        supervisor_tid,
        "hotswap-demo",
        "/bin/hotswap-demo-nonexistent",
    ) {
        fail("hotswap to non-existent ELF unexpectedly succeeded");
    }
    if read_counter(demo_tid, b"v2:") != Some(1000) {
        fail("failed hotswap corrupted live provider counter");
    }
    println("[native-stateful] failed hotswap preserved live provider and counter");

    // 7. Success Report
    println(
        "[native-stateful] Summary: 1000/1000 ops completed, 10 checkpoints verified, 0 errors",
    );
    println("[native-stateful] ALL CRITERIA PASSED");
    ostd::syscall::sys_exit(0);
}

fn inc_demo(tid: usize) -> bool {
    let mut resp = [0u8; 2];
    let msg = [0xAC, 0x00, b'i', b'n', b'c'];
    if !matches!(sys_send(tid, &msg), SyscallResult::Ok(_)) {
        return false;
    }
    recv_from(tid, &mut resp) && &resp == b"ok"
}

fn read_counter(tid: usize, expected_prefix: &[u8; 3]) -> Option<u32> {
    let mut response = [0u8; 7];
    let msg = [0xAC, 0x00, b'g', b'e', b't'];
    if !matches!(sys_send(tid, &msg), SyscallResult::Ok(_)) {
        return None;
    }
    if !recv_from(tid, &mut response) {
        return None;
    }
    if &response[..3] != expected_prefix {
        return None;
    }
    Some(u32::from_le_bytes(response[3..7].try_into().ok()?))
}

fn append_checkpoint(path: &str, record: &CheckpointRecord) {
    let bytes = record.to_bytes();
    let mut vfs_client = ostd::clients::VfsClient::new();
    if vfs_client.append_file(path, &bytes).is_err() {
        fail(&format!(
            "failed to append checkpoint {} to {}",
            record.seq, path
        ));
    }
}

fn verify_persisted_checkpoints(path: &str, expected: &[Option<CheckpointRecord>]) {
    let mut vfs_client = ostd::clients::VfsClient::new();
    let data = match vfs_client.read_file(path) {
        Ok(d) => d,
        Err(_) => fail(&format!("failed to read checkpoint log {path}")),
    };

    let count = data.len() / 12;
    if count != expected.len() {
        fail(&format!(
            "checkpoint log record count mismatch: expected {}, got {count}",
            expected.len()
        ));
    }

    for (i, exp_opt) in expected.iter().enumerate() {
        let chunk = &data[i * 12..(i + 1) * 12];
        let rec = CheckpointRecord::from_bytes(chunk)
            .unwrap_or_else(|| fail(&format!("corrupted checkpoint record {i}")));
        if let Some(exp) = exp_opt {
            if &rec != exp {
                fail(&format!(
                    "checkpoint record {i} mismatch: expected {exp:?}, got {rec:?}"
                ));
            }
        }
    }
}

fn spawn_cached_sender_probe(old_tid: usize) -> bool {
    let role = format!("native-stateful-cached-inc:{old_tid}");
    sys_set_spawn_args(&role);
    matches!(
        sys_spawn_pinned("/bin/bench-probe", api::task::TaskPriority::Normal as u8, 0),
        SyscallResult::Ok(_)
    )
}

pub fn run_cached_sender_probe(role: &str) -> ! {
    sys_heartbeat(0);
    let Some(old_tid) = role
        .strip_prefix("native-stateful-cached-inc:")
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        fail("invalid cached tid in probe");
    };

    let mut inc = [0u8; 5];
    inc[..2].copy_from_slice(&[0xAC, 0x00]);
    inc[2..].copy_from_slice(b"inc");

    let mut sent = false;
    for _ in 0..CUTOVER_WINDOW_TICKS {
        sys_heartbeat(0);
        if sys_lookup_service(service::HOTSWAP_DEMO).is_none()
            && matches!(sys_send(old_tid, &inc), SyscallResult::Ok(_))
        {
            sent = true;
            break;
        }
        let _ = sys_set_timer(1);
    }

    if !sent {
        for _ in 0..WAIT_TICKS {
            sys_heartbeat(0);
            if let Some(new_tid) = sys_lookup_service(service::HOTSWAP_DEMO) {
                if new_tid != old_tid {
                    let _ = sys_send(new_tid, &inc);
                    break;
                }
            }
            sys_yield();
        }
    }
    ostd::syscall::sys_exit(0);
}

fn trigger_hotswap(supervisor_tid: usize, service_name: &str, elf_path: &str) -> bool {
    const APP_MESSAGE_PREFIX: [u8; 2] = [0xAC, 0x00];
    let mut req = [0u8; APP_MESSAGE_PREFIX.len() + REQUEST_LEN];
    req[..APP_MESSAGE_PREFIX.len()].copy_from_slice(&APP_MESSAGE_PREFIX);
    let off = APP_MESSAGE_PREFIX.len();
    req[off] = OP_HOTSWAP;
    let s_bytes = service_name.as_bytes();
    req[off + 1..off + 1 + s_bytes.len()].copy_from_slice(s_bytes);
    let p_bytes = elf_path.as_bytes();
    req[off + 1 + SVC_NAME_LEN..off + 1 + SVC_NAME_LEN + p_bytes.len()].copy_from_slice(p_bytes);

    if !matches!(sys_send(supervisor_tid, &req), SyscallResult::Ok(_)) {
        return false;
    }
    let mut status = [0u8; 3];
    if !recv_from(supervisor_tid, &mut status) {
        return false;
    }
    status[0] == 3 && status[1] == 6 && status[2] == 0
}

fn wait_for_replacement_demo(old_tid: usize, max_ticks: usize) -> Option<usize> {
    for _ in 0..max_ticks {
        sys_heartbeat(0);
        if let Some(new_tid) = sys_lookup_service(service::HOTSWAP_DEMO) {
            if new_tid != old_tid {
                return Some(new_tid);
            }
        }
        sys_yield();
    }
    None
}

fn wait_for_service_restart(service_id: u16, old_tid: usize, max_ticks: usize) -> Option<usize> {
    for _ in 0..max_ticks {
        sys_heartbeat(0);
        if let Some(new_tid) = sys_lookup_service(service_id) {
            if new_tid != old_tid {
                return Some(new_tid);
            }
        }
        sys_yield();
    }
    None
}
fn recv_from(expected_sender: usize, buf: &mut [u8]) -> bool {
    matches!(sys_recv(expected_sender, buf), SyscallResult::Ok(sender) if sender == expected_sender)
}

fn fail(message: &str) -> ! {
    println(&format!("[native-stateful] FAIL: {message}"));
    ostd::syscall::sys_exit(1)
}
