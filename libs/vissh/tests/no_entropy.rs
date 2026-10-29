//! Without a working entropy source the client never sends KEXINIT. This is a separate
//! test binary because the source is process-global.
mod common;

use common::*;
use vissh::sunset::{Client, Runner};

fn failing(_: &mut [u8]) -> bool {
    false
}

fn kexinit_sent(cli: &mut Runner<'static, Client>) -> (bool, bool) {
    let mut failed = false;
    let mut out = Vec::new();
    for _ in 0..8 {
        if cli.progress().is_err() {
            failed = true;
            break;
        }
        out.extend(drain(cli));
    }
    out.extend(drain(cli));
    let (_, packets) = after_ident(&out);
    (failed, !packets.is_empty())
}

#[test]
fn failed_or_missing_entropy_stops_before_kexinit() {
    // Nothing installed yet.
    let mut cli: Runner<'static, Client> = Runner::new_client_owned();
    let (failed, sent) = kexinit_sent(&mut cli);
    assert!(failed && !sent, "no source: failed={failed} sent={sent}");

    // A source that fails, and stays the only one.
    assert!(vissh::install_entropy(failing));
    assert!(!vissh::install_entropy(os_entropy));
    let mut cli: Runner<'static, Client> = Runner::new_client_owned();
    let (failed, sent) = kexinit_sent(&mut cli);
    assert!(failed && !sent, "failing source: failed={failed} sent={sent}");
}
