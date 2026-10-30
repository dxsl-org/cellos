//! Tier-3 guest endpoint lane (`261110-0900-tier3-guest-endpoint`, S10-11): the `ssh` checks of
//! the parent module run against OpenSSH inside the nested Linux guest, reached through the
//! one loopback `hostfwd` of `scripts/qemu-tier3-ssh-guest-x86.sh` at `10.0.2.2:PORT`.
//! Built only with `--features guest`.
//!
//! Readiness (T3-C2): the guest's sshd comes up long after the shell does (nested TCG, no
//! virtio-rng), so the lane waits for the route before any scenario. The probe is the
//! client's own changed-key refusal: `host` has a deliberately wrong stored key for
//! `host:PORT`, so the client connects, reads the server's host key, and refuses before any
//! credential with a message that names the key it was shown. The route is ready when that
//! message carries exactly the fingerprint the lane recorded from the guest's host key
//! (`/p09/ssh.conf`). No password is ever typed here.

use super::*;

/// Total wait for the guest's sshd (the T3-B window: 60 minutes).
const READY_MS: u64 = 3_600_000;
/// Pause between probes.
const RETRY_MS: u64 = 10_000;

/// Wait until a client connection through `10.0.2.2:PORT` shows the guest's host key.
pub(crate) fn wait_ready(a: &mut Term, f: &Fixture) -> bool {
    let end = now_ms() + READY_MS;
    let mut attempts: u32 = 0;
    while now_ms() < end {
        attempts += 1;
        let probe = attempt(a, &format!("ssh --user tester --auth password -p {} host", f.port));
        if let Ok((st, err)) = &probe {
            if st == "st=255" && err.contains("HOST KEY CHANGED") && err.contains(&f.fp) {
                println(&format!("P04 INFO ssh-guest-ready attempts={attempts}"));
                return true;
            }
        }
        if attempts % 6 == 1 {
            println(&format!("P04 INFO ssh-guest-wait attempts={attempts}"));
        }
        sleep_ms(RETRY_MS);
    }
    println(&format!("P04 INFO ssh-guest-ready never attempts={attempts}"));
    false
}
