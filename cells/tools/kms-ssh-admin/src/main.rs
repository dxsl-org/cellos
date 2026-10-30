//! `/bin/kms-ssh-admin` (KSSH K7): the admin surface for the single KMS-custodied SSH
//! identity. Dedicated cell, launched only by the shell's reviewed path edge, holding the
//! kernel-attested `CALLER_FLAG_KMS_SSH_ADMIN` and never the sign right.
//!
//! ```text
//! kms-ssh-admin status [NAME]   leaf status, or one key's facts           (op 19)
//! kms-ssh-admin import NAME     armored key on the echo-off lease         (ops 15, 16*, 17; 18)
//! kms-ssh-admin delete NAME     typed confirmation, then                  (ops 19, 20)
//! ```
//!
//! Exit codes: 0 ok, 1 refused by the key service (or unreachable), 2 usage or key text that
//! is not one armored OpenSSH private key, 124 key service timeout, 125 no terminal or
//! session lost, 130 cancelled. DEV_POSTURE: anyone at the foreground session is the admin.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(not(test), forbid(unsafe_code))]

extern crate alloc;
extern crate ostd;

// No network, no spawn, no block I/O, no peripherals, no hypervisor. The right this cell
// holds comes from the kernel launch edge, never from a manifest bit.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// The terminal-session child set (the one `phase02-session-child` runs on, without its
// launch syscall): IPC to the session owner and to the key service, logging, the clock and
// the one-tick timer. Deliberately absent: GetRandom, every net and spawn call, VFS and
// grants. `Exit` and `Yield` are always permitted by the allowlist layer.
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Exit,
    Yield,
    Heartbeat,
    LookupService,
    GetTime,
    SetTimer,
    MemInfo,
];

mod armor;
mod console;
mod delete;
mod import;
mod kms;
mod report;
mod secret;
mod status;
#[cfg(test)]
mod tests;

use console::Console;
use kms::Kms;
use ostd::clients::kms::{validate_ssh_key_name, KmsClientError};
use report::EXIT_USAGE;

#[derive(Debug, PartialEq, Eq)]
pub enum Command<'a> {
    Status(Option<&'a str>),
    Import(&'a str),
    Delete(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub enum UsageError {
    Grammar,
    Name,
}

impl UsageError {
    pub const fn message(&self) -> &'static str {
        match self {
            Self::Grammar => report::USAGE,
            Self::Name => "kms-ssh-admin: invalid key name (1 to 64 characters from A-Z a-z 0-9 _ -)\n",
        }
    }
}

/// The whole grammar. The name is checked here with the same validator KMS uses, so a name
/// KMS would refuse never leaves the cell. Nothing here reads key material.
pub fn parse<'a>(args: &[&'a str]) -> Result<Command<'a>, UsageError> {
    let name = |text: &'a str| {
        if validate_ssh_key_name(text.as_bytes()) {
            Ok(text)
        } else {
            Err(UsageError::Name)
        }
    };
    match args {
        ["status"] => Ok(Command::Status(None)),
        ["status", text] => Ok(Command::Status(Some(name(text)?))),
        ["import", text] => Ok(Command::Import(name(text)?)),
        ["delete", text] => Ok(Command::Delete(name(text)?)),
        _ => Err(UsageError::Grammar),
    }
}

/// One run: parse, connect to the key service, execute. `connect` is only called after a
/// valid command line, so a usage error never depends on the key service.
pub fn run<K: Kms>(
    args: &[&str],
    console: &mut dyn Console,
    connect: impl FnOnce() -> Result<K, KmsClientError>,
) -> i32 {
    let command = match parse(args) {
        Ok(command) => command,
        Err(error) => {
            console.err(error.message());
            return EXIT_USAGE;
        }
    };
    let kms = match connect() {
        Ok(kms) => kms,
        Err(error) => {
            let (code, message) = report::kms_failure(error);
            console.err(&message);
            return code;
        }
    };
    match command {
        Command::Status(name) => status::run(name, console, &kms),
        Command::Import(name) => import::run(name, console, &kms),
        Command::Delete(name) => delete::run(name, console, &kms),
    }
}

#[cfg(not(test))]
ostd::cell_main!(cell_main);

#[cfg(not(test))]
fn cell_main() {
    let code = match ostd::child_context::ChildContext::read_own() {
        Ok(context) => entry(&context),
        Err(_) => report::EXIT_NO_TERMINAL,
    };
    ostd::syscall::sys_exit(code as u32 as usize)
}

/// The foreground-child entry. Without a terminal session (a pipeline stage has no control
/// grant; a bare spawn has no context session) there is nobody to read the key from or to
/// confirm a delete with: refuse with 125.
#[cfg(not(test))]
fn entry(context: &ostd::child_context::ChildContext) -> i32 {
    use alloc::string::String;
    use alloc::vec::Vec;
    use ostd::child_context::GrantSlot;
    use terminal_sdk::session::TerminalSession;

    let has_control = context.grants().any(|grant| grant.slot == GrantSlot::Control);
    let session = if has_control { TerminalSession::from_own_context(context).ok() } else { None };
    let Some(session) = session else {
        console::refuse_without_terminal(context);
        return report::EXIT_NO_TERMINAL;
    };
    let args: Vec<String> = context.argv().map(String::from).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut tty = console::Tty::new(session);
    let code = run(&words, &mut tty, ostd::clients::kms::KmsClient::connect);
    if tty.lost() && code == report::EXIT_OK {
        report::EXIT_NO_TERMINAL
    } else {
        code
    }
}
