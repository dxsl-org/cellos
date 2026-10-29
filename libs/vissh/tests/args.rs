//! `cellbox ssh` argv contract (§4.1) and the host-key fingerprint format.
use vissh::args::{parse, ArgError, Auth, Host};
use vissh::client::HostKey;

fn p(s: &str) -> Result<vissh::args::Args, ArgError> {
    parse(s.split(' ').filter(|a| !a.is_empty()))
}

#[test]
fn password_shell_and_publickey_exec_forms() {
    let a = p("--user bob --auth password -p 2222 --connect-timeout 5 Host.Example.").unwrap();
    assert_eq!((a.user.as_str(), a.port, a.connect_timeout, a.timeout), ("bob", 2222, 5, None));
    assert_eq!(a.auth, Auth::Password);
    assert_eq!(a.host, Host::Name("host.example".into()));
    assert_eq!(a.command, None);

    let mut argv = vec!["--auth", "publickey", "--identity", "k_1", "--user", "u", "10.0.2.2", "--command"];
    argv.push("printf x; exit 7");
    let a = parse(argv).unwrap();
    assert_eq!(a.auth, Auth::PublicKey { key_id: "k_1".into() });
    assert_eq!(a.host, Host::V4([10, 0, 2, 2].into()));
    assert_eq!(a.port, 22);
    assert_eq!(a.command.as_deref(), Some("printf x; exit 7"));
}

#[test]
fn every_unsupported_or_malformed_form_is_refused() {
    for (argv, want) in [
        ("--auth password h", ArgError::Missing("--user")),
        ("--user u h", ArgError::Missing("--auth")),
        ("--user u --user v --auth password h", ArgError::Repeated("--user")),
        ("--user u --auth publickey h", ArgError::Missing("--identity")),
        ("--user u --auth password --identity k h", ArgError::Invalid("--identity with --auth password")),
        ("--user u --auth password -i key h", ArgError::Unsupported("-i".into())),
        ("--user u --auth password -L 1:a:2 h", ArgError::Unsupported("-L".into())),
        ("--user u --auth password -o X=y h", ArgError::Unsupported("-o".into())),
        ("--user u --auth password h ls", ArgError::Extra("ls".into())),
        ("--user u --auth password h --command a b", ArgError::Extra("b".into())),
        ("--user u --auth password -p 0 h", ArgError::Invalid("port")),
        ("--user u --auth password -p 65536 h", ArgError::Invalid("port")),
        ("--user u --auth password --connect-timeout 301 h", ArgError::Invalid("connect timeout")),
        ("--user u --auth password --timeout 0 h", ArgError::Invalid("timeout")),
        ("--user u --auth password h -p 22", ArgError::Extra("-p".into())),
        ("--user u --auth agent h", ArgError::Invalid("--auth (password|publickey)")),
        ("--user b@d --auth password h", ArgError::Invalid("user")),
        ("--user u --auth password u@h", ArgError::Invalid("host")),
        ("--user u --auth password *.example", ArgError::Invalid("host")),
        ("--user u --auth password ssh://h", ArgError::Invalid("host")),
        ("--user u --auth password 300.1.1.1", ArgError::Invalid("host")),
        ("--user u --auth password -bad-.example", ArgError::Unsupported("-bad-.example".into())),
        ("--user u --auth password", ArgError::Missing("HOST")),
    ] {
        assert_eq!(p(argv), Err(want), "{argv}");
    }
    assert_eq!(parse(["--user", "u", "--auth", "password", "h", "--command", ""]), Err(ArgError::Invalid("command")));
    let long = "x".repeat(4097);
    assert_eq!(parse(["--user", "u", "--auth", "password", "h", "--command", &long]), Err(ArgError::Invalid("command")));
    assert_eq!(p("--user u --auth password -- -x").unwrap_err(), ArgError::Invalid("host"));
    assert_eq!(p("--user u --auth password -- x").unwrap().host, Host::Name("x".into()));
}

#[test]
fn ipv6_literal_is_canonical() {
    assert_eq!(p("--user u --auth password 0:0::1").unwrap().host.to_string(), "::1");
}

#[test]
fn fingerprint_matches_ssh_keygen() {
    // `ssh-keygen -lf` on a freshly generated ed25519 key:
    // SHA256:1SeiAa5jRYvg3LY3ZGtG2OvkfIs5JvHq7+zXQqBNq1k
    let hex = "72c5be44c71b734b67e09a2af9a66a3a2bf704aa2e8b140655dfcb806167e809";
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    let k = HostKey { algorithm: "ssh-ed25519", key };
    assert_eq!(k.fingerprint(), "SHA256:1SeiAa5jRYvg3LY3ZGtG2OvkfIs5JvHq7+zXQqBNq1k");
}
