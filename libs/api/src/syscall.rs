// SPDX-License-Identifier: MPL-2.0

/// System Call Identifiers (The Contract)
///
/// These IDs must match between Kernel and User (libs/ostd).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum ViSyscall {
    // === IPC (0-9) ===
    Send = 0,
    Recv = 1,
    Call = 2,
    Reply = 3,
    
    // === Process Management (10-49) ===
    Exit = 60,  // Linux compat usually, but we define our own space
    Spawn = 5,
    Exec = 6,
    Yield = 104, // Linux sched_yield is 24, but we use 104 in current code
    
    // === Logging (50-59) ===
    Log = 11,   // Current implementation uses 11
    
    // === Filesystem (100-199) ===
    Open = 101,
    Read = 102,
    Close = 103,
    ReadDir = 105,
    Write = 109,
    
    // === Unknown ===
    Unknown = 9999,
}

impl From<usize> for ViSyscall {
    fn from(id: usize) -> Self {
        match id {
            0 => ViSyscall::Send,
            1 => ViSyscall::Recv,
            2 => ViSyscall::Call,
            3 => ViSyscall::Reply,
            60 => ViSyscall::Exit,
            5 => ViSyscall::Spawn,
            6 => ViSyscall::Exec,
            104 => ViSyscall::Yield,
            11 => ViSyscall::Log,
            101 => ViSyscall::Open,
            102 => ViSyscall::Read,
            103 => ViSyscall::Close,
            105 => ViSyscall::ReadDir,
            109 => ViSyscall::Write,
            _ => ViSyscall::Unknown,
        }
    }
}
