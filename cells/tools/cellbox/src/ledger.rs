// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Command disposition ledger (Phase 04 slice S2-lite).
//!
//! One final disposition per shell builtin, sys-tools ELF and net-tools ELF. The ledger is
//! plain data: it changes no behavior and grants no right. Host tests diff it against
//! [`crate::executor::BUILTINS`], the `[[bin]]` lists of `sys-tools` / `net-tools` and the
//! catalog table in
//! `.agents/261020-1750-cellbox-cli/command-catalog.md`, so a command can neither be
//! silently lost nor silently joined to the suite.
//!
//! A command name is identified by `(Surface, name)`: the `cat` shell builtin and the
//! `/bin/cat` ELF are two different artifacts with different ceilings and therefore carry
//! their own disposition. Within one surface a name appears exactly once.

/// Where a command lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A shell builtin of `cellbox sh` (`executor::BUILTINS`).
    Builtin,
    /// A binary of the `app-sys-tools` crate (`/bin/<name>`).
    SysTool,
    /// A binary of the `app-net-tools` crate (`/bin/<name>`).
    NetTool,
}

/// Final disposition of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Runs inside the cellbox ceiling; every syscall it needs is already in the shell ceiling.
    InSuite,
    /// A shell builtin whose privileged effect the kernel or launch edge was **observed to
    /// deny** for this caller on the private image. Only valid with a named runner check
    /// proving it ([`DENIAL_CHECKS`]).
    InSuiteEffectDenied,
    /// A shell builtin that performs **no effect at all**: it prints a refusal, issues no
    /// syscall and no IPC, and exits non-zero (S2c-min). The effect needs a lifecycle owner
    /// the suite shell does not hold. Only valid with a named runner check proving the
    /// refusal and an unchanged process set ([`REFUSAL_CHECKS`]).
    InSuiteRefused,
    /// A separate ELF with its own launch edge and ceiling; never part of the suite ELF.
    RetainedWorker,
}

/// One ledger row.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub surface: Surface,
    pub name: &'static str,
    pub disposition: Disposition,
}

const fn e(surface: Surface, name: &'static str, disposition: Disposition) -> Entry {
    Entry {
        surface,
        name,
        disposition,
    }
}

use Disposition::{InSuite, InSuiteEffectDenied, InSuiteRefused, RetainedWorker};
use Surface::{Builtin, NetTool, SysTool};

/// The ledger. Builtins first (alphabetical), then sys-tools, then net-tools.
pub const LEDGER: &[Entry] = &[
    e(Builtin, "alias", InSuite),
    e(Builtin, "awk", InSuite),
    e(Builtin, "bg", InSuite),
    e(Builtin, "blktest", InSuiteEffectDenied),
    e(Builtin, "break", InSuite),
    e(Builtin, "cat", InSuite),
    e(Builtin, "cd", InSuite),
    e(Builtin, "clear", InSuite),
    e(Builtin, "continue", InSuite),
    e(Builtin, "cp", InSuite),
    e(Builtin, "date", InSuite),
    e(Builtin, "edit", InSuite),
    e(Builtin, "ssh", InSuite),
    e(Builtin, "completion", InSuite),
    e(Builtin, "echo", InSuite),
    e(Builtin, "env", InSuite),
    e(Builtin, "exec", InSuite),
    e(Builtin, "exit", InSuite),
    e(Builtin, "export", InSuite),
    e(Builtin, "fg", InSuite),
    e(Builtin, "find", InSuite),
    e(Builtin, "free", InSuite),
    e(Builtin, "grep", InSuite),
    e(Builtin, "head", InSuite),
    e(Builtin, "help", InSuite),
    e(Builtin, "history", InSuite),
    e(Builtin, "hv", InSuite),
    e(Builtin, "ifconfig", InSuite),
    e(Builtin, "ip", InSuite),
    e(Builtin, "jobs", InSuite),
    e(Builtin, "kill", InSuiteRefused),
    e(Builtin, "ls", InSuite),
    e(Builtin, "mkdir", InSuite),
    e(Builtin, "mv", InSuite),
    e(Builtin, "ps", InSuite),
    e(Builtin, "pwd", InSuite),
    e(Builtin, "read", InSuite),
    e(Builtin, "rg", InSuite),
    e(Builtin, "rm", InSuite),
    e(Builtin, "rmdir", InSuite),
    e(Builtin, "sed", InSuite),
    e(Builtin, "shutdown", InSuiteEffectDenied),
    e(Builtin, "sleep", InSuite),
    e(Builtin, "snapshot", InSuiteRefused),
    e(Builtin, "sort", InSuite),
    e(Builtin, "source", InSuite),
    e(Builtin, "tail", InSuite),
    e(Builtin, "tee", InSuite),
    e(Builtin, "test", InSuite),
    e(Builtin, "top", InSuite),
    e(Builtin, "touch", InSuite),
    e(Builtin, "unalias", InSuite),
    e(Builtin, "uniq", InSuite),
    e(Builtin, "unset", InSuite),
    e(Builtin, "uname", InSuite),
    e(Builtin, "uptime", InSuite),
    e(Builtin, "vappend", InSuite),
    e(Builtin, "vcat", InSuite),
    e(Builtin, "vwrite", InSuite),
    e(Builtin, "wc", InSuite),
    // sys-tools (`cells/tools/sys-tools/Cargo.toml` [[bin]]): every one stays a separate ELF.
    e(SysTool, "ls", RetainedWorker),
    e(SysTool, "cat", RetainedWorker),
    e(SysTool, "echo", RetainedWorker),
    e(SysTool, "ps", RetainedWorker),
    e(SysTool, "kill", RetainedWorker),
    e(SysTool, "env", RetainedWorker),
    e(SysTool, "uname", RetainedWorker),
    e(SysTool, "date", RetainedWorker),
    e(SysTool, "free", RetainedWorker),
    e(SysTool, "shutdown", RetainedWorker),
    e(SysTool, "hotswap", RetainedWorker),
    // net-tools (`cells/tools/net-tools/Cargo.toml` [[bin]]).
    e(NetTool, "ping", RetainedWorker),
    e(NetTool, "curl", RetainedWorker),
    e(NetTool, "nc", RetainedWorker),
    e(NetTool, "wget", RetainedWorker),
    e(NetTool, "httpd", RetainedWorker),
    e(NetTool, "mqtt", RetainedWorker),
];

/// Every `InSuiteEffectDenied` builtin and the runner check (`chk("<name>"` in
/// `scripts/qemu-phase03-pipeline-test.sh`) that observes the denial on the private image.
/// A builtin may only carry the class when a check here proves it.
pub const DENIAL_CHECKS: &[(&str, &str)] = &[
    ("blktest", "s2-blktest-raw-block-denied"),
    ("shutdown", "s2-builtin-shutdown-denied-shell-survives"),
];

/// Every `InSuiteRefused` builtin and the runner check that observes the refusal (no effect,
/// no IPC, process set unchanged) on the private image.
pub const REFUSAL_CHECKS: &[(&str, &str)] = &[
    ("kill", "s2-kill-builtin-refused-no-ipc"),
    ("snapshot", "s2-snapshot-builtin-refused-no-ipc"),
];

/// Disposition of `name` on `surface`, or `None` when the ledger has no such entry.
pub fn disposition(surface: Surface, name: &str) -> Option<Disposition> {
    LEDGER
        .iter()
        .find(|entry| entry.surface == surface && entry.name == name)
        .map(|entry| entry.disposition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;
    use alloc::string::String;
    use alloc::vec::Vec;

    const SYS_TOOLS_MANIFEST: &str = include_str!("../../sys-tools/Cargo.toml");
    const NET_TOOLS_MANIFEST: &str = include_str!("../../net-tools/Cargo.toml");
    const CATALOG: &str = include_str!("../../../../.agents/261020-1750-cellbox-cli/command-catalog.md");

    /// `name = "…"` of every `[[bin]]` table in a Cargo manifest.
    fn bin_names(manifest: &str) -> Vec<&str> {
        let mut names = Vec::new();
        let mut in_bin = false;
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('[') {
                in_bin = line == "[[bin]]";
            } else if in_bin {
                if let Some(rest) = line.strip_prefix("name") {
                    let value = rest.trim_start().strip_prefix('=').expect("name = ").trim();
                    names.push(value.trim_matches('"'));
                    in_bin = false;
                }
            }
        }
        names
    }

    fn names_of(surface: Surface) -> Vec<&'static str> {
        LEDGER
            .iter()
            .filter(|entry| entry.surface == surface)
            .map(|entry| entry.name)
            .collect()
    }

    fn as_set<'a>(names: impl IntoIterator<Item = &'a str>) -> BTreeSet<&'a str> {
        names.into_iter().collect()
    }

    #[test]
    fn no_surface_name_pair_appears_twice() {
        let mut seen = BTreeSet::new();
        for entry in LEDGER {
            let key = (entry.surface as u8, entry.name);
            assert!(seen.insert(key), "{:?} {} listed twice", entry.surface, entry.name);
        }
    }

    #[test]
    fn counts_match_the_s2_gate() {
        // 57 at the S2 gate, plus `edit` (Phase 08 S8-4), `ssh` (Phase 09 S9-2) and
        // `completion` (Phase 09 S9-6, the `cellbox.ssh.completion` record).
        assert_eq!(names_of(Builtin).len(), 60);
        assert_eq!(names_of(SysTool).len(), 11);
        assert_eq!(names_of(NetTool).len(), 6);
    }

    #[cfg(not(feature = "shell_test"))]
    #[test]
    fn builtin_entries_equal_the_executor_builtins() {
        let executor = crate::executor::BUILTINS;
        assert_eq!(executor.len(), as_set(executor.iter().copied()).len(), "BUILTINS has a duplicate");
        assert_eq!(as_set(names_of(Builtin)), as_set(executor.iter().copied()));
    }

    #[test]
    fn tool_entries_equal_the_cargo_bin_lists() {
        let sys = bin_names(SYS_TOOLS_MANIFEST);
        let net = bin_names(NET_TOOLS_MANIFEST);
        assert_eq!(sys.len(), 11, "sys-tools [[bin]] count");
        assert_eq!(net.len(), 6, "net-tools [[bin]] count");
        assert_eq!(as_set(names_of(SysTool)), as_set(sys));
        assert_eq!(as_set(names_of(NetTool)), as_set(net));
    }

    #[test]
    fn every_separate_elf_is_a_retained_worker_and_never_in_the_suite() {
        for entry in LEDGER.iter().filter(|e| e.surface != Builtin) {
            assert_eq!(entry.disposition, RetainedWorker, "{:?} {}", entry.surface, entry.name);
        }
        for entry in LEDGER.iter().filter(|e| e.surface == Builtin) {
            assert_ne!(entry.disposition, RetainedWorker, "builtin {}", entry.name);
        }
    }

    #[test]
    fn effect_denied_builtins_are_exactly_the_proven_ones() {
        let denied: BTreeSet<_> = LEDGER
            .iter()
            .filter(|e| e.disposition == InSuiteEffectDenied)
            .map(|e| (e.surface as u8, e.name))
            .collect();
        let expected: BTreeSet<_> = DENIAL_CHECKS.iter().map(|(name, _)| (Builtin as u8, *name)).collect();
        assert_eq!(denied, expected, "EffectDenied entries and DENIAL_CHECKS must be the same set");
    }

    #[test]
    fn every_effect_denied_entry_names_a_runner_check_that_exists() {
        let runner = include_str!("../../../../scripts/qemu-phase03-pipeline-test.sh");
        for (name, check) in DENIAL_CHECKS {
            assert_eq!(disposition(Builtin, name), Some(InSuiteEffectDenied), "{name}");
            let needle = alloc::format!("chk(\"{check}\"");
            assert_eq!(runner.matches(needle.as_str()).count(), 1, "runner must define `{check}` exactly once");
        }
    }

    #[test]
    fn refused_builtins_are_exactly_the_proven_ones() {
        let refused: BTreeSet<_> = LEDGER
            .iter()
            .filter(|e| e.disposition == InSuiteRefused)
            .map(|e| (e.surface as u8, e.name))
            .collect();
        let expected: BTreeSet<_> = REFUSAL_CHECKS.iter().map(|(name, _)| (Builtin as u8, *name)).collect();
        assert_eq!(refused, expected, "Refused entries and REFUSAL_CHECKS must be the same set");
        let names: BTreeSet<_> = refused.iter().map(|(_, name)| *name).collect();
        assert_eq!(names, BTreeSet::from(["kill", "snapshot"]));
    }

    #[test]
    fn every_refused_entry_names_a_runner_check_that_exists() {
        let runner = include_str!("../../../../scripts/qemu-phase03-pipeline-test.sh");
        for (name, check) in REFUSAL_CHECKS {
            assert_eq!(disposition(Builtin, name), Some(InSuiteRefused), "{name}");
            let needle = alloc::format!("chk(\"{check}\"");
            assert_eq!(runner.matches(needle.as_str()).count(), 1, "runner must define `{check}` exactly once");
        }
    }

    fn surface_label(surface: Surface) -> &'static str {
        match surface {
            Surface::Builtin => "builtin",
            Surface::SysTool => "sys-tool",
            Surface::NetTool => "net-tool",
        }
    }

    fn disposition_label(d: Disposition) -> &'static str {
        match d {
            InSuite => "InSuite",
            InSuiteEffectDenied => "InSuiteEffectDenied",
            InSuiteRefused => "InSuiteRefused",
            RetainedWorker => "RetainedWorker",
        }
    }

    /// The catalog table, exactly as `command-catalog.md` must carry it.
    fn catalog_block() -> String {
        let mut out = String::from("<!-- ledger:begin -->\n| Surface | Name | Disposition |\n|---|---|---|\n");
        for entry in LEDGER {
            out.push_str("| ");
            out.push_str(surface_label(entry.surface));
            out.push_str(" | `");
            out.push_str(entry.name);
            out.push_str("` | ");
            out.push_str(disposition_label(entry.disposition));
            out.push_str(" |\n");
        }
        out.push_str("<!-- ledger:end -->");
        out
    }

    #[test]
    fn catalog_table_is_generated_from_the_ledger() {
        let begin = CATALOG.find("<!-- ledger:begin -->").expect("catalog ledger begin marker");
        let end = CATALOG.find("<!-- ledger:end -->").expect("catalog ledger end marker") + "<!-- ledger:end -->".len();
        let expected = catalog_block();
        assert!(
            &CATALOG[begin..end] == expected.as_str(),
            "catalog ledger table is stale; replace the marked block with:\n{expected}"
        );
    }
}
