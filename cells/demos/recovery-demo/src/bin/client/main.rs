//! `recovery-client`: the Phase 09B evidence driver. One shell command per
//! group, each printing `RECOVERY-CLIENT <group>: PASS` or a FAIL line:
//!   `recovery-client core`           E1–E7, E10, E6, E12, E11 (needs fault-inject cells)
//!   `recovery-client foreign <id>`   E4 second principal
//!   `recovery-client lease`          E8 (needs the `recovery-fixture` VFS)
//!   `recovery-client export`         E9 (terminal: the provider stays down)

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

#[path = "../common.rs"]
mod common;
mod cycles;
mod driver;
mod extra;
mod faults;

use alloc::format;
use ostd::io::println;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Log,
    Exit,
    Send,
    Recv,
    RecvTimeout,
    SetTimer,
    LookupService,
    MemInfo,
    StateRestore
];

ostd::cell_main!(cell_main);

fn cell_main() {
    let args = ostd::args();
    let Some(group) = args.first().map(|s| s.as_str()) else {
        driver::fail("no scenario group argument")
    };
    let mut client = driver::Client::connect();
    match group {
        "core" => {
            faults::commit_faults(&mut client);
            faults::conflicts(&mut client);
            faults::fenced_writer(&mut client);
            let mut remote = u32::from(faults::whole_cell(&mut client) != 0);
            faults::abandoned_lock(&mut client);
            cycles::restore_refusals(&mut client);
            remote += cycles::ten_cycles(&mut client);
            cycles::identities(&client);
            println(&format!(
                "[recovery-client] remote-hart retirement waits: {remote} of 11 spin crashes"
            ));
        }
        "foreign" => {
            let id = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1002);
            extra::foreign(&mut client, id);
        }
        "lease" => extra::missing_ack(&mut client),
        "export" => extra::untracked_export(&mut client),
        other => driver::fail(&format!("unknown group {other}")),
    }
    println(&format!("RECOVERY-CLIENT {group}: PASS"));
}
