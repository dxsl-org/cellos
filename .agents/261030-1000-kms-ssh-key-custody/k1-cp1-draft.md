# KSSH K1 CP1 DRAFT: dependency admission and D-K4 (status AWAITING CP1; documents only; nothing applied)

Evidence: [reports/k1-dependency-evidence.md](reports/k1-dependency-evidence.md) (+ `.json`, `k1-proposed-cargo-lock.diff`, `k1-network-hosts.txt`, probe sources/logs). Gathered 2026-10-30 in `/tmp/kssh-k1` under the user's network authorization scoped to read-only evidence.

## Network disclosure (deviation to acknowledge)
Authorized hosts used: `index.crates.io`, `static.crates.io`, `crates.io` (4 API GETs), `github.com/RustSec/advisory-db` (through cargo-deny). **One unauthorized host was contacted:** `github.com/rafalh/rust-fatfs`, fetched read-only into the scratch `CARGO_HOME` when `cargo update --workspace --dry-run` resolved the workspace's existing `fatfs` git dependency (`kernel/Cargo.toml:18`, `cells/services/vfs/Cargo.toml:18`). Not repeated; the repo and its lock were untouched. No inbound connection, no rustup network use.

## Findings that decide the design
- Already in the lock: `ssh-key 0.7.0-rc.11` (only `alloc` active; used by sunset, vissh, cellbox), `aes 0.9.3`, `ctr 0.10.1`, `ed25519-dalek 3.0.0` (+2.2.0), `sha2 0.10.9` and `0.11.1`, `zeroize`, `cipher`, `byteorder`.
- `bcrypt-pbkdf 0.11.0` (MIT OR Apache-2.0, MSRV 1.85, `#![no_std]`, not yanked) is the only lock-compatible version. The lock diff adds exactly **three packages**: `bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`; no existing lock line moves. Its `zeroize` feature does not enable `blowfish/zeroize`, so the manifest adds a direct `blowfish` dependency with that feature.
- `ssh-key`'s `encryption` feature pulls `bcrypt-pbkdf`, `aes`, `aes-gcm`, `chacha20poly1305`, `rand_core`; AES-256-CTR-only is **not** achievable by features. `ssh-key` accepts any bcrypt rounds (fixture with `rounds = 0x7fffffff` parses) and any non-empty salt, so the contract bounds (rounds 1..64, salt 16..64 B) must be a check written before `decrypt()`. `ssh-key` has no final 0.7.0 (newest is the rc).
- All three candidate variants build for `x86_64-unknown-none` (synthetic probes, x86 `.text`): own minimal 77,427 B; hybrid 129,923 B; `ssh-key`+`encryption` 169,107 B.
- Host check: the public `ssh-key` fixture `id_ed25519.aes256-ctr.enc` decrypts and signs with the own parser, signature equals `ssh-key`'s; chacha and aes128-ctr fixtures are rejected before any KDF work. Host KDF cost 4.7 ms/round, 301 ms at 64 rounds; Cellos-target cost not measured.
- Baseline `cargo deny --all-features` already fails on the repo lock (exit 7: 7 advisories, 2 license rejections, 4 wildcards), unrelated to the candidates. Own parser and hybrid add **0** new diagnostics; `ssh-key`+`encryption` adds 5 duplicate-version warnings. RustSec (db `ee1a49a2`) has no advisory for any candidate crate or locked version.

## Proposal (decision D-K4)
**Own minimal OpenSSH-v1 parser with a structural pre-KDF allowlist** (rank 1), built on `bcrypt-pbkdf` + `blowfish` (zeroize) + `aes` + `ctr` + `zeroize`; hybrid second; `ssh-key`+`encryption` third. Reasons: makes AES-256-CTR-only and the rounds/salt bounds structural, keeps chacha20-poly1305/AES-GCM/`rand_core`/the pre-release crate out of the protected service, lets KMS zeroize derived keys, 91,680 B smaller. Risk: ~100 lines of untrusted-input parsing inside a locked service; mitigations required at K5: a differential test against `ssh-key` as a **dev-dependency only**, byte fixtures, and fuzzing of the parser (not run yet). KMS stack budget for the 4,168-byte Blowfish state is unchecked.

## What K1 would change, and when
Adding a dependency to `Cargo.lock` needs a manifest that uses it. The only consumer is KMS (`cells/services/kms/Cargo.toml`, CELL-CONTRACT-0024 locked scope per the lock-adoption packet). Therefore **K1 admits the dependencies (decision, license/advisory record and the pre-reviewed 3-package lock diff) but applies the lock and manifest edit only together with K5**, under that slice's lock handoff. Nothing is applied now.

## Acceptance for K1 (when applied)
1. Lock diff equals `reports/k1-proposed-cargo-lock.diff` (three packages, no other line changes), checksums recorded.
2. `cargo deny check` shows **no new diagnostics** versus the pre-change baseline (not "deny green": baseline is red).
3. Licenses of the three new crates are within the repo's allowed set (MIT OR Apache-2.0).
4. Build for `x86_64-unknown-none` with `-Z build-std=core,alloc` succeeds; `.text` growth recorded.
5. No `ssh-key` `encryption` feature, no chacha20-poly1305/AES-GCM in KMS.

## Not authorized by this draft
Editing `Cargo.toml`/`Cargo.lock`/`deny.toml`, vendoring, any KMS source, further network use, fuzzing runs, target/QEMU runs, key generation.

## Decision requested
D-K4 = own minimal parser with allowlist (recommended) / hybrid / `ssh-key`+`encryption`; acknowledgement of the unauthorized `rust-fatfs` fetch; confirmation that applying the lock waits for K5.
