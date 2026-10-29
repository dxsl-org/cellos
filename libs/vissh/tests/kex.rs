//! Negotiation and authentication ordering of the narrowed Sunset client.
mod common;

use common::*;
use vissh::sunset::{
    CliEvent, Client, Event, KeyType, PubKey, Runner, ServEvent, Server, SignKey,
};

fn setup() {
    vissh::install_entropy(os_entropy);
}

#[derive(Default, Debug)]
struct Seen {
    hostkey: Option<[u8; 32]>,
    password_asked_before_hostkey: bool,
    authenticated: bool,
    password_offered: Option<String>,
}

/// Run a client against a Sunset server until the client authenticates, stalls or
/// fails. `accept_host` decides the host-key check; `password` is the client's reply.
fn run(accept_host: bool, password: &str) -> (Seen, Result<(), vissh::sunset::Error>) {
    setup();
    let key = SignKey::generate(KeyType::Ed25519, None).unwrap();
    let mut cli: Runner<'static, Client> = Runner::new_client_owned();
    let mut srv: Runner<'static, Server> = Runner::new_server_owned();
    let mut seen = Seen::default();
    let (mut c2s, mut s2c) = (Wire::default(), Wire::default());
    let result = (|| {
        for _ in 0..500 {
            match cli.progress()? {
                Event::Cli(CliEvent::Hostkey(h)) => {
                    if let PubKey::Ed25519(k) = h.hostkey()? {
                        seen.hostkey = Some(k.key.0);
                    }
                    if accept_host { h.accept()? } else { h.reject()? }
                }
                Event::Cli(CliEvent::Username(u)) => u.username("tester")?,
                Event::Cli(CliEvent::Password(p)) => {
                    if seen.hostkey.is_none() {
                        seen.password_asked_before_hostkey = true;
                    }
                    if seen.password_offered.is_some() {
                        p.skip()?;
                    } else {
                        seen.password_offered = Some(password.into());
                        p.password(password)?;
                    }
                }
                Event::Cli(CliEvent::Authenticated) => {
                    seen.authenticated = true;
                    return Ok(());
                }
                Event::Cli(CliEvent::Pubkey(p)) => p.skip()?,
                Event::Cli(CliEvent::Defunct) => return Ok(()),
                _ => {}
            }
            match srv.progress()? {
                Event::Serv(ServEvent::Hostkeys(h)) => h.hostkeys(&[&key])?,
                Event::Serv(ServEvent::FirstAuth(a)) => a.reject()?,
                Event::Serv(ServEvent::PasswordAuth(a)) => {
                    if a.password()? == "right" { a.allow()? } else { a.reject()? }
                }
                _ => {}
            }
            c2s.take(&mut cli);
            s2c.take(&mut srv);
            c2s.give(&mut srv)?;
            s2c.give(&mut cli)?;
        }
        Ok(())
    })();
    let expect = match key.pubkey() {
        PubKey::Ed25519(k) => k.key.0,
        #[allow(unreachable_patterns)]
        _ => unreachable!(),
    };
    if let Some(got) = seen.hostkey {
        assert_eq!(got, expect, "client saw a different host key");
    }
    (seen, result)
}

#[test]
fn client_offers_only_the_cellbox_profile() {
    setup();
    let mut cli: Runner<'static, Client> = Runner::new_client_owned();
    let mut out = Vec::new();
    for _ in 0..8 {
        let _ = cli.progress().unwrap();
        out.extend(drain(&mut cli));
    }
    let (ident, packets) = after_ident(&out);
    assert!(ident.starts_with(b"SSH-2.0-"));
    let lists = kexinit_lists(packets);
    let kex: Vec<&str> = lists[0].iter().map(String::as_str).collect();
    assert_eq!(
        kex,
        ["curve25519-sha256", "ext-info-c", "kex-strict-c-v00@openssh.com", "kexguess2@matt.ucc.asn.au"]
    );
    assert_eq!(lists[1], ["ssh-ed25519"]);
    assert_eq!(lists[2], ["chacha20-poly1305@openssh.com"]);
    assert_eq!(lists[3], ["chacha20-poly1305@openssh.com"]);
    assert_eq!(lists[6], ["none"]);
    assert_eq!(lists[7], ["none"]);
}

#[test]
fn right_password_authenticates_after_the_host_key_check() {
    let (seen, result) = run(true, "right");
    result.unwrap();
    assert!(seen.hostkey.is_some());
    assert!(!seen.password_asked_before_hostkey);
    assert!(seen.authenticated);
}

#[test]
fn wrong_password_never_authenticates() {
    let (seen, _) = run(true, "wrong");
    assert!(!seen.authenticated);
    assert_eq!(seen.password_offered.as_deref(), Some("wrong"));
}

#[test]
fn rejected_host_key_ends_before_any_password_is_requested() {
    let (seen, result) = run(false, "right");
    assert!(seen.hostkey.is_some());
    assert!(seen.password_offered.is_none());
    assert!(!seen.authenticated);
    assert!(result.is_err() || !seen.authenticated);
}

/// Feed a scripted server identification and KEXINIT; return the client's result.
fn against_server_kexinit(payload: Vec<u8>) -> Result<(), vissh::sunset::Error> {
    setup();
    let mut cli: Runner<'static, Client> = Runner::new_client_owned();
    let mut input = b"SSH-2.0-OpenSSH_9.9\r\n".to_vec();
    input.extend(packet(&payload));
    let mut fed = 0;
    for _ in 0..50 {
        if let Event::Cli(CliEvent::Defunct) = cli.progress()? {
            return Ok(());
        }
        drain(&mut cli);
        if fed < input.len() {
            fed += cli.input(&input[fed..])?;
        }
    }
    Ok(())
}

#[test]
fn server_without_the_profile_algorithms_is_refused() {
    let r = against_server_kexinit(kexinit(
        "diffie-hellman-group14-sha1,curve25519-sha256@libssh.org,kex-strict-s-v00@openssh.com",
        "ssh-ed25519",
        "aes128-ctr,aes256-ctr",
        "hmac-sha2-256",
    ));
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn server_without_strict_kex_is_refused() {
    let r = against_server_kexinit(kexinit(
        "curve25519-sha256",
        "ssh-ed25519",
        "chacha20-poly1305@openssh.com",
        "hmac-sha2-256",
    ));
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn server_with_the_profile_is_accepted_through_kexinit() {
    let r = against_server_kexinit(kexinit(
        "curve25519-sha256,kex-strict-s-v00@openssh.com",
        "ssh-ed25519",
        "chacha20-poly1305@openssh.com",
        "hmac-sha2-256",
    ));
    assert!(r.is_ok(), "{r:?}");
}
