# KSSH K1 prep: dependency evidence (read-only, 2026-10-30)

Scope: evidence only. Nothing in the repo was edited, no commit, no key generated, no socket opened, no VM. All work ran in `/tmp/kssh-k1` (own `CARGO_HOME`, own target dirs; repo manifests exported with `git archive HEAD` (HEAD `a3b9a7d10`, tree clean at start, `Cargo.lock` byte-identical to the repo's) into `/tmp/kssh-k1/repo-slice`). Toolchain: repo-pinned `nightly-2026-05-01` (`cargo 1.97.0-nightly (eb9b60f1f 2026-04-24)`), `cargo-deny 0.20.2`. `[INFERENCE]` marks anything not observed. Machine-readable twin: `k1-dependency-evidence.json`; hashes: `evidence.sha256`.

## 0. Bottom line

1. **`bcrypt-pbkdf 0.11.0` is the only version that fits the lock** (`ssh-key 0.7.0-rc.11` requires `^0.11`; 0.10.0 needs the old cipher 0.4 / sha2 0.10 stack). It is MIT OR Apache-2.0, MSRV 1.85, `#![no_std]`, not yanked. Resolving it in a scratch copy of the lock adds **exactly three packages**: `bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`. Every other crate (`byteorder 1.5.0`, `cipher 0.5.2`, `sha2 0.11.1`, `digest 0.11.3`, `zeroize 1.9.0`, `aes 0.9.3`, `ctr 0.10.1`, `ed25519-dalek 3.0.0`, `base64ct 1.8.3`) is already locked and, except `blowfish`/`pbkdf2`/`bcrypt-pbkdf`, already compiled by existing cells. No version in the lock moved (`cargo update --workspace` printed `Locking 3 packages`, only `Adding` lines).
2. **Three candidate shapes** all compile for `x86_64-unknown-none` (cell profile), `riscv64gc-unknown-none-elf` and `aarch64-unknown-none-softfloat` (hybrid and ssh-key variants; own-minimal on x86 only):

   | variant | text (x86, B) | new lock packages | new deny diagnostics |
   |---|---|---|---|
   | **own minimal** (bcrypt-pbkdf + aes + ctr + zeroize [+ base64ct, ed25519-dalek]) | 77,427 | 3 | 0 |
   | hybrid (`ssh-key` without `encryption` for the container + own allowlist/KDF/CTR) | 129,923 | 3 | 0 |
   | `ssh-key` + `encryption` | 169,107 | 3 (+2 edges in `ssh-cipher`, +2 in `ssh-key`) | 5 duplicate-version warnings |
3. **D-K4 recommendation: own minimal parser with an allowlist before the KDF** (details and risks §7). `ssh-key` cannot be made AES-256-CTR-only by features, accepts any bcrypt rounds/salt length at parse time, and is still a release candidate.
4. **The repo's `cargo deny --all-features check` already fails at baseline** (exit 7, unrelated to the candidates): 7 advisories, 2 license rejections, 4 wildcard errors (§5). K1 acceptance must therefore be "no new diagnostics", not "deny green".
5. **One network request exceeded the authorised host list**: `cargo update` resolving the repo's `git` dependency `fatfs` contacted `https://github.com/rafalh/rust-fatfs` (§9). Read-only git fetch into the scratch `CARGO_HOME`; disclosed, not repeated.

## 1. What the existing lock already covers

Source: `/home/dmin/cellos/Cargo.lock` (name-line numbers); manifests `third_party/sunset/Cargo.toml:13-36`, `libs/vissh/Cargo.toml:11-15`, `cells/services/kms/Cargo.toml:23-31`, `libs/ostd/Cargo.toml:8-24` (no crypto crates), `cells/tools/cellbox/Cargo.toml:37-41`.

| stack | in lock (line) | who pulls it (from `cargo tree --workspace --all-features -i`) |
|---|---|---|
| `ssh-key 0.7.0-rc.11` | 3729; deps ctutils, sha2 0.11.1, signature 3.0.0, ssh-cipher, ssh-encoding, zeroize | only `sunset` (optional feature `openssh-key`, `default-features=false`, `alloc`) ← `vissh` ← `app-cellbox`. **No `encryption`, no `ed25519`, no `rand_core`** feature active |
| `ssh-cipher 0.3.0` / `ssh-encoding 0.3.0` | 3700 / 3716 | via ssh-key. Effective ssh-cipher features: `default, encoding, zeroize` only (no `aes`/`chacha20poly1305`). The lock still lists `aes, aes-gcm, chacha20 0.10.2, des, poly1305 0.9.1` as ssh-cipher deps because the `zeroize` feature names them as weak deps (`aes?/zeroize`, ssh-cipher `Cargo.toml` `[features] zeroize`); they are lock-only, **not compiled** |
| `ed25519-dalek 3.0.0` (+ `ed25519 3.0.0`, `curve25519-dalek 5.0.0`, `sha2 0.11.1`, `signature 3.0.0`) | 1736 / 1709 / 1118 / 3568 / 3595 | `sunset` (`features zeroize, rand_core`) |
| `ed25519-dalek 2.2.0` (+ `curve25519-dalek 4.1.3`, `sha2 0.10.9`) | 1724 / 1102 / 3557 | `service-net` (optional, `cells/services/net/Cargo.toml:71`) |
| `ed25519-compact 2.3.1` | 1718 | `cellos-kernel`, `service-sshd` (also vf2/durable-charge crates) |
| `aes 0.9.3` / `ctr 0.10.1` | 78 / 1084 | `sunset` only (`aes`, `ctr` with `zeroize`). `aes 0.8.4` / `ctr 0.9.2` are for `aes-gcm 0.10.3` (TLS stack) |
| `sha2 0.10.9` | 3557 | KMS (`kms/Cargo.toml:29`), authority-protocol, net, net-broker, silo, sshd, vfs, p256, rsa, embedded-tls, clatter, ed25519-dalek 2.2.0 |
| `sha2 0.11.1` | 3568 | `vissh`, `app-cellbox`, `sunset`, `ssh-key`, `ed25519-dalek 3.0.0` |
| `cipher 0.5.2`, `digest 0.11.3`, `zeroize 1.9.0`, `hmac 0.13.0`, `base64ct 1.8.3`, `byteorder 1.5.0` | 913, 1357, 4644, 2278, 664, 720 | `byteorder` is compiled today via `heapless 0.8 → hash32` in `ostd` (every cell) |
| `p256 0.13.2`, `blake2`, `constant_time_eq` | 2742 | KMS today (no ed25519, no KDF, no AES in KMS: `kms/Cargo.toml:24-31`) |
| bcrypt/blowfish/pbkdf2 | **absent** (`grep -c` = 0 earlier; confirmed by diff) | |

Consequence for KMS: KMS would link `sha2 0.10.9` (p256) **and** `sha2 0.11.1` (bcrypt-pbkdf needs `sha2 ^0.11`; also `ed25519-dalek 3`). Both versions already coexist in the workspace (`bans.multiple-versions = "warn"`); only a size cost for KMS.

## 2. `bcrypt-pbkdf` and its transitive set

Index (`index.crates.io`) and crates.io API: latest = `0.11.0` (published 2026-04-22), `max_stable_version 0.11.0`, 19.0 M downloads, not yanked. Prior stable `0.10.0` (2023-03-05). 0.11 pre-releases (`pre.0/1`, `rc.0..rc.3`, `rc.6`) exist, none yanked; no reason to use them.

| crate | version | sha256 (verified against index AND the downloaded `.crate`) | license (crate metadata, `LICENSE-*` files shipped) | MSRV | yanked | in lock today |
|---|---|---|---|---|---|---|
| bcrypt-pbkdf | 0.11.0 | `144e573728da132683b9488acd528274c790e07fc06ff81ee29f9d8f8b1041e0` | MIT OR Apache-2.0 (`LICENSE-MIT` 36ce5478…, `LICENSE-APACHE` 3708458d…) | 1.85 | no | **new** |
| blowfish | 0.10.0 (2026-04-10) | `62ce3946557b35e71d1bbe07ec385073ce9eda05043f95de134eb578fcf1a298` | MIT OR Apache-2.0 | 1.85 | no | **new** |
| pbkdf2 | 0.13.0 (2026-04-21) | `112d82ceb8c5bf524d9af484d4e4970c9fd5a0cc15ba14ad93dccd28873b0629` | MIT OR Apache-2.0 | 1.85 | no | **new** |
| byteorder | 1.5.0 | `1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b` | Unlicense OR MIT (MIT is in `deny.toml` allow) | 1.60 | no | already (compiled by ostd) |
| cipher / sha2 / digest / zeroize | 0.5.2 / 0.11.1 / 0.11.3 / 1.9.0 | from lock; index checksums match; not yanked | MIT OR Apache-2.0 | 1.85 / 1.88 / 1.85 / 1.85 | no | already |

All 30 crates in `k1-dependency-evidence.json → locked_crate_checks` match their index checksum and none is yanked. Among the crypto crates listed in §1 the highest MSRV is `aes 0.9.3` = 1.89 (cargo metadata); the new crates (1.85) raise nothing; repo toolchain is 1.97-nightly.

Feature/shape facts (crate sources read, not inferred):
- `bcrypt-pbkdf 0.11.0` `Cargo.toml [features]`: `alloc = []`, `default = ["alloc"]`, `zeroize = ["dep:zeroize"]`; **no `std`** (removed, CHANGELOG 0.11.0). `src/lib.rs:7` `#![no_std]`; no `unsafe`, no `build.rs`. Deps: `blowfish ^0.10 [bcrypt]`, `pbkdf2 ^0.13` (`default-features=false`, so no `hmac`), `sha2 ^0.11` (no default), optional `zeroize`.
- `bcrypt_pbkdf` (alloc, `lib.rs:51-70`) keeps a 256 B stack buffer (or `Vec`) of derived output; **that buffer is not zeroized**. `bcrypt_pbkdf_with_memory` (`lib.rs:91-124`, always available, heapless) lets the caller own and zeroize the scratch.
- `zeroize` feature only zeroizes `Bhash.sha2_pass` on drop (`lib.rs:196-201`). It does **not** enable `blowfish/zeroize`; without it `blowfish`'s `Drop` is empty (`blowfish-0.10.0 src/lib.rs:210-218`), so the passphrase-derived 4,168-byte Blowfish state is left on the stack. A direct `blowfish = { features = ["zeroize"] }` dependency unifies the feature and closes that (included in the proposed manifest; adds no lock package).
- `pbkdf2 0.13.0`: `pbkdf2::<PRF>()` generic fn available without `hmac` (`src/lib.rs:162`); compiled in all probes.

## 3. `ssh-key 0.7.0-rc.11` `encryption` feature

Registry source `ssh-key-0.7.0-rc.11/Cargo.toml:83-89`: `encryption = ["dep:bcrypt-pbkdf", "alloc", "cipher/aes", "cipher/chacha20poly1305", "rand_core"]`; `bcrypt-pbkdf ^0.11 [alloc]` optional. `ssh-cipher 0.3.0` `[features]`: `aes = [dep:aead, dep:aes, dep:aes-gcm]`, `chacha20poly1305 = [dep:aead, dep:chacha20, dep:poly1305, dep:ctutils]`, `tdes = [dep:des]`.

Compiled additions for KMS (cargo tree, `-p service-kms --target x86_64-unknown-none`, baseline 64 crates → +30; `k1-evidence-logs/kms-tree-*.txt`): ssh-key, ssh-cipher, ssh-encoding, pem-rfc7468, base64ct, signature, bcrypt-pbkdf, blowfish, pbkdf2, **aead 0.6.1, aes-gcm 0.11.1, chacha20 0.10.2, ghash 0.6.0, polyval 0.7.3, poly1305 0.9.1, universal-hash 0.6.1, rand_core 0.10.1**, plus the cipher/digest/sha2 0.11 base.

**AES-256-CTR-only is not achievable by features.** `cipher/aes` alone drags in AES-GCM; `encryption` additionally enables ChaCha20-Poly1305; `Cipher::decrypt` (`ssh-cipher src/lib.rs:268-330`) dispatches on the cipher name taken from the file. The only ways to restrict:
- (a) call `PrivateKey::from_openssh` first (it parses container, cipher name and KDF options **without any KDF work**, `private.rs:269-`, `kdf.rs`), inspect `cipher()` / `kdf()` / `key_data().encrypted()` and refuse before `decrypt()`; verified on the public fixtures (hosttest). `Kdf::decode` has **no cap**: a fixture patched to `rounds = 0x7fffffff` parses fine and `kdf()` returns it; `decrypt()` would then run the KDF with it (≈4.7 ms/round measured ⇒ ~115 days; computed, not run). The allowlist is therefore caller discipline: one missed call path = unbounded KDF + any cipher.
- (b) an own parser (below) where unsupported ciphers/KDFs have no code path.

Own-parser crate set (probe `probe-ownmin`, built): `bcrypt-pbkdf` (+`blowfish`, `pbkdf2` transitively), `aes 0.9` + `ctr 0.10` (+ `cipher 0.5`), `zeroize 1`, plus what K5 needs anyway for signing, `ed25519-dalek 3.0`, and `base64ct 1` only if KMS receives PEM (an admin path that sends the decoded `openssh-key-v1` bytes needs no base64 in KMS; K2 decision). Parser size in the probe: ~100 lines (magic, 3 strings, kdf options `{salt,rounds}`, `nkeys==1`, pubkey blob, ciphertext with `len%16==0`, no trailing data; after CTR: checkints, `ssh-ed25519`, 32 B public, 64 B `seed||pub`, comment, padding `1,2,3…`; public from the outer blob = inner = derived from seed).

rc status (crates.io API / index): versions `0.7.0-rc.0` (2025-06-01) … `0.7.0-rc.11` (2026-06-29); **no `0.7.0` final**; `max_stable_version = 0.6.7` (2024-10-15, old stack: ssh-encoding 0.2 / ssh-cipher 0.2 / bcrypt-pbkdf 0.10 / sha2 0.10 / cipher 0.4: would add a second crypto stack, rejected). rc.11 is the newest as of 2026-10-30; no yank on any rc. Risks: pre-release API (12 rcs in 13 months), `^0.7.0-rc.11` could resolve to a future rc/final on `cargo update`; its optional deps (`dsa`, `rsa`, `p256 0.14`, `p384`, `p521`, `argon2 0.6-rc`) are rc as well but not enabled. The repo already accepts rc.11 for the vendored Sunset client (`third_party/sunset/Cargo.toml:36`), so rc-ness is a KMS-side new exposure, not a new workspace one. [INFERENCE: no stable-release date is known.]

## 4. no_std + alloc build for the Cellos target

Scratch probes (`k1-evidence-src/*`), lock seeded from the repo's `Cargo.lock` (every locked version preserved; only the three packages added). Command (per probe, own `CARGO_TARGET_DIR`):
`CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic" cargo +nightly-2026-05-01 build --release --target x86_64-unknown-none -Z build-std=core,alloc` (same invocation and cell RUSTFLAGS as `scripts/qemu-phase09-ssh-test.sh:118`/`:111`; root `[profile.release]` opt-level 2, lto, panic=abort copied). riscv64: `-C relocation-model=pic`; aarch64: `+bti,+paca,+pacg` (`.cargo/config.toml` template flags). No C code in the candidate crates, so the repo `CFLAGS` are not exercised. **All builds exit 0.**

Binary = `#![no_std] #![no_main]` `_start` that parses a fixed buffer, allowlists, runs bcrypt-pbkdf (`_with_memory`) + AES-256-CTR, checks the Ed25519 public key, and signs (hybrid/own) or `PrivateKey::decrypt` + sign (ssh-key); 256 KiB bump allocator (so every `bss` is ≈262,144 + a few KB).

| probe | x86_64 text / data / bss (B) | ELF B | riscv64 text | aarch64 text |
|---|---|---|---|---|
| base (stub) | 459 / 160 / 265,104 | 2,832 | – | – |
| own minimal | 77,427 / 272 / 265,792 | 87,136 | – | – |
| hybrid (ssh-key alloc + own KDF/CTR) | 129,923 / 376 / 262,344 | 151,248 | 117,628 | 116,228 |
| ssh-key `encryption` | 169,107 / 376 / 264,120 | 195,576 | 155,424 | 158,852 |

Deltas over the stub, x86 text: own **+76,968**, hybrid **+129,464**, ssh-key+encryption **+168,648**; ssh-key+encryption vs own minimal: **+91,680 B**. Binary-size figures are for a synthetic probe, not KMS; `bcrypt-pbkdf` alone ≈ 3.6 KB (`Bhash::finalize_into` 3,653 B, `llvm-nm --size-sort`) plus blowfish tables.

Functional check on the host (`hosttest`, std, same crates): the public `ssh-key` test fixture `tests/examples/id_ed25519.aes256-ctr.enc` (password `hunter42`, rounds 16, public `b33eaef3…ab62`; sha256 `b5fd6cdd…963d`) decrypts and signs with the own-minimal parser; the signature verifies against the fixture public key and is **identical** to `ssh-key(encryption).decrypt()` + dalek; wrong passphrase rejected; chacha20-poly1305 and aes128-ctr fixtures rejected before the KDF; a `rounds=0x7fffffff` blob rejected in 1.85 µs. (Fixtures are public vectors from the registry crate; no key was generated.)

## 5. License / advisory checks

`cargo deny 0.20.2 --locked --all-features check` with a byte-identical copy of the repo's `deny.toml`, run in `repo-slice` (baseline) and in three scratch workspaces whose `service-kms` manifest gets each variant (`k1-kms-manifest-*.diff`). Advisory DB: `https://github.com/RustSec/advisory-db` @ `ee1a49a2769a24a596afc1c20cb33748d2769586` (2026-10-29T15:05:38+01:00), fetched by cargo-deny.

| run | advisories err | bans err / warn | licenses err | sources warn | new diagnostics vs baseline |
|---|---|---|---|---|---|
| baseline (repo lock) | 7 | 4 / 31 | 2 | 1 | – |
| own parser | 7 | 4 / 31 | 2 | 1 | **0** |
| hybrid | 7 | 4 / 31 | 2 | 1 | **0** |
| ssh-key+encryption | 7 | 4 / **36** | 2 | 1 | 5 `duplicate` warnings (aead, aes-gcm, chacha20, ghash, polyval) |

Baseline failures (pre-existing, unrelated): RUSTSEC-2026-0387 `crypto-bigint 0.5.5`; RUSTSEC-2026-0049/-0098/-0099/-0104/-0382 `rustls-webpki 0.103.7`; RUSTSEC-2026-0381 `webpki-roots 1.0.3`; license rejections `ocel-pdf` (AGPL-3.0-or-later) and `webpki-roots`; wildcard deps in `app-c2c-render`, `cellos-boot-config`, `cellos-kernel`, `ocel`. `deny.toml` ignores only RUSTSEC-2026-0192 and RUSTSEC-2023-0071. [INFERENCE] the DB moved since the repo was last green (rustls-webpki/webpki-roots ids are 2026-dated); not investigated.

Advisory DB lookup by crate directory: **no advisory** for `bcrypt-pbkdf`, `blowfish`, `pbkdf2`, `byteorder`, `ssh-key`, `ssh-cipher`, `ssh-encoding`, `aes`, `ctr`, `cipher`, `zeroize`, `poly1305`, `ed25519`, `hmac`, `digest`, `base64ct`. Advisories for `sha2` (RUSTSEC-2021-0100, fixed ≥0.9.8), `ed25519-dalek` (RUSTSEC-2022-0093, patched ≥2), `curve25519-dalek` (RUSTSEC-2024-0344, patched ≥4.1.3), `chacha20` (RUSTSEC-2019-0029, patched ≥0.2.3), `aes-gcm` (RUSTSEC-2023-0096, patched ≥0.10.3) do not match the locked versions (0.10.9/0.11.1, 2.2.0/3.0.0, 4.1.3/5.0.0, 0.9.1/0.10.2, 0.10.3/0.11.1). The unrelated `bcrypt` crate (RUSTSEC-2026-0199) is a different package. SPDX against `deny.toml [licenses].allow` (MIT, Apache-2.0, …): bcrypt-pbkdf, blowfish, pbkdf2 = `MIT OR Apache-2.0`; byteorder `Unlicense OR MIT`; both satisfied by an allowed alternative, deny confirms (no new `rejected`).

## 6. Contract constraints (`command-contracts.md:142`) against the crate APIs

Observed with `bcrypt_pbkdf_with_memory` (hosttest, `k1-evidence-logs/hosttest-run.txt`):

| contract | crate behaviour | consequence |
|---|---|---|
| rounds 1..64 | `0 → Err(InvalidRounds)`; 1, 64, **65 and 0x7fffffff accepted** | KMS must enforce `1..=64` itself, before the KDF |
| salt 16..64 B | any non-empty length accepted (1, 15, 64, **65**); empty → `InvalidParamLen` | KMS must enforce `16..=64` |
| passphrase ≤1,024 B | 1,024 B accepted; **empty → `InvalidParamLen`** | map an empty passphrase to the refusal class; enforce the 1,024 cap in the protocol layer |
| output | `1..=1024`; 48 B for AES-256-CTR (32 key + 16 IV) accepted; memory buffer must be ≥ `ceil(48/32)*32 = 64` B | use `bcrypt_pbkdf_with_memory` with a `Zeroizing<[u8;64]>` |
| AES-256-CTR only | no such switch in `ssh-key`; trivially structural in an own parser (`ctr::Ctr128BE<aes::Aes256>`, `new_from_slices(32, 16)`) | see §3, §7 |

Time cost. Host (this WSL2 machine, release, 5 samples, 48-byte output): rounds=1 **4.70 ms**, 16 **75.25 ms**, 64 **301.26 ms** (median). Work = `2 · rounds · 129 · 521` Blowfish block encryptions (stride 2 × rounds PRF calls × 1 salted + 128 plain key expansions × 521 encryptions ≈ 8.6 M at 64 rounds ≈ 35 ns each). On the Cellos target: **[INFERENCE only]** same integer-only code, no SIMD/AES-NI use, so the same order of magnitude on bare metal; under QEMU TCG plausibly several times slower (seconds at 64 rounds). KMS is a single-threaded loop (`kms/src/main.rs:27-50` per probe report), so every import of a passphrase-protected key would stall relay/node-identity service for that time. Not measured on Cellos.

## 7. D-K4 recommendation

**Recommend: own minimal OpenSSH-v1 parser with an AES-256-CTR/bcrypt allowlist before the KDF**, using `bcrypt-pbkdf 0.11.0` (`default-features=false`, `zeroize`), `blowfish 0.10` (`zeroize`, `bcrypt`), `aes 0.9`, `ctr 0.10`, `zeroize 1`, `ed25519-dalek 3` and, only if KMS receives PEM, `base64ct`. Do not add `ssh-key` to KMS.

Reasons (evidence in §§2-6):
1. The contract demands rejection of everything but `bcrypt` + `aes256-ctr` with bounded rounds/salt **before expensive work**. With an own parser this is structural; with `ssh-key` it is a pre-`decrypt()` convention over a parser that accepts any rounds up to `u32::MAX` and any salt (shown with a patched fixture), while linking ChaCha20-Poly1305 and AES-GCM into the signer.
2. Smallest compiled/trusted set in the locked signer: no `ssh-key`/`ssh-cipher`/`ssh-encoding`/`pem-rfc7468`/`aead`/`aes-gcm`/`chacha20`/`poly1305`/`ghash`/`polyval`/`rand_core` (−91,680 B text vs ssh-key+encryption in the probe) and **no pre-release crate in KMS** (rc.11 is the last of 12 rcs; no final).
3. Memory hygiene is controllable: `bcrypt_pbkdf_with_memory` into a caller-zeroized buffer, `bcrypt-pbkdf/zeroize` + `blowfish/zeroize`; `ssh-key`'s `decrypt` calls the alloc variant whose stack buffer is never wiped and does not enable either feature.
4. Lock/deny impact is the minimum: 3 new packages, 0 new deny diagnostics (ssh-key+encryption adds 5 duplicate-version warnings and 4 extra lock edges).

Risks of the recommendation and how to hold them:
- ~100 lines of untrusted-input parsing (≤64 KiB) in a locked, Law-1-adjacent service. Mitigate: strict length checks before allocation, `nkeys==1`, no trailing data, reject all unknown names, a single error class for wrong passphrase vs. corrupt checkint, host fuzz/proptest, and a **differential oracle**: `ssh-key` with `encryption` as a **dev-dependency only** (host tests; same lock edges as the ssh-key variant but not compiled into the cell) over the public fixtures in `ssh-key/tests/examples` — I ran exactly this comparison once (§4).
- Own code must reproduce what `ssh-key` does implicitly: checkint equality, padding bytes `1,2,3…` (<16), outer-vs-inner public-key equality, and `public == derive(seed)` (without `ssh-key`'s `ed25519` feature even `ssh-key` does not validate the pair; the probe does it with dalek).
- Double SHA-2 in KMS (0.10.9 for p256, 0.11.1 for bcrypt-pbkdf/ed25519-dalek 3): size only.
- Passphrase-less (cipher `none`) keys need the same own parser branch (not exercised in the probe; fixture `id_ed25519` exists upstream).
- If the user prefers less custom code: the **hybrid** (ssh-key without `encryption` for the container, own allowlist/KDF/CTR) is the second choice (+52,496 B vs own minimal, still pre-release in KMS); `ssh-key`+`encryption` is the third (needs a mandatory pre-`decrypt()` allowlist helper and a reviewer to enforce it).

## 8. Proposed Cargo.lock diff (own-parser variant; computed in the scratch copy, never applied)

Produced by `cargo update --workspace` in `/tmp/kssh-k1/scen-a` after adding to `cells/services/kms/Cargo.toml [dependencies]` (`k1-kms-manifest-own-parser.diff`): `bcrypt-pbkdf 0.11 {zeroize}`, `blowfish 0.10 {zeroize,bcrypt}`, `aes 0.9 {zeroize}`, `ctr 0.10 {zeroize}`, `zeroize 1`, `base64ct 1 {alloc}`, `ed25519-dalek 3.0 {zeroize}`. File: `k1-proposed-cargo-lock.diff` (alternates: `k1-lock-diff-hybrid.diff`, `k1-lock-diff-sshkey-encryption.diff`).

```diff
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -667,6 +667,18 @@
 checksum = "2af50177e190e07a26ab74f8b1efbfe2ef87da2116221318cb1c2e82baf7de06"
 
 [[package]]
+name = "bcrypt-pbkdf"
+version = "0.11.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "144e573728da132683b9488acd528274c790e07fc06ff81ee29f9d8f8b1041e0"
+dependencies = [
+ "blowfish",
+ "pbkdf2",
+ "sha2 0.11.1",
+ "zeroize",
+]
+
+[[package]]
 name = "bitflags"
 version = "1.3.2"
 source = "registry+https://github.com/rust-lang/crates.io-index"
@@ -707,6 +719,16 @@
 ]
 
 [[package]]
+name = "blowfish"
+version = "0.10.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "62ce3946557b35e71d1bbe07ec385073ce9eda05043f95de134eb578fcf1a298"
+dependencies = [
+ "byteorder",
+ "cipher 0.5.2",
+]
+
+[[package]]
 name = "bstr"
 version = "1.13.1"
 source = "registry+https://github.com/rust-lang/crates.io-index"
@@ -2769,6 +2791,15 @@
 checksum = "2ee67f1008b1ba2321834326597b8e186293b049a023cdef258527550b9935b4"
 
 [[package]]
+name = "pbkdf2"
+version = "0.13.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "112d82ceb8c5bf524d9af484d4e4970c9fd5a0cc15ba14ad93dccd28873b0629"
+dependencies = [
+ "digest 0.11.3",
+]
+
+[[package]]
 name = "pem-rfc7468"
 version = "1.0.0"
 source = "registry+https://github.com/rust-lang/crates.io-index"
@@ -3422,14 +3453,21 @@
 name = "service-kms"
 version = "0.1.0"
 dependencies = [
+ "aes 0.9.3",
  "api",
+ "base64ct",
+ "bcrypt-pbkdf",
  "blake2",
+ "blowfish",
  "cell-build",
  "constant_time_eq",
+ "ctr 0.10.1",
+ "ed25519-dalek 3.0.0",
  "ostd",
  "p256",
  "sha2 0.10.9",
  "types",
+ "zeroize",
 ]
 
 [[package]]
```

The three added `[[package]]` blocks are independent of the variant; only the `service-kms` edge lines (and, for the ssh-key variant, `ssh-cipher` +`aead 0.6.1`, +`ctutils`; `ssh-key` +`bcrypt-pbkdf`, +`rand_core 0.10.1`) differ. Answer to "would the lock diff add only bcrypt-pbkdf + blowfish + cipher-family crates already present?": **yes, plus `pbkdf2 0.13.0` (new, depends only on `digest 0.11.3`, already locked); `byteorder`, `cipher`, `sha2 0.11.1`, `zeroize` are already present.**

## 9. Network requests (every host used)

| host | what | by |
|---|---|---|
| `index.crates.io` | sparse-index files (the 3 candidates, `ssh-key`, `ssh-cipher`, `ssh-encoding`, `byteorder`, 30 locked-crate checks; `config.json`; cargo resolution of the whole lock, ~250 index loads in the first `cargo update --dry-run`) | cargo, Python `urllib` (`k1-evidence-logs/network-requests-python.log`) |
| `static.crates.io` | `bcrypt-pbkdf-0.11.0.crate`, `blowfish-0.10.0.crate`, `pbkdf2-0.13.0.crate` (the only crates cargo downloaded; all others came from a local copy of `~/.cargo/registry/{cache,src}` into the scratch `CARGO_HOME`, no network) | cargo + Python |
| `crates.io` | API `GET /api/v1/crates/{bcrypt-pbkdf,blowfish,pbkdf2,ssh-key}` (4 requests, 1 s apart, identifying User-Agent) | Python |
| `github.com/RustSec/advisory-db` | advisory DB clone (authorised) | cargo-deny |
| **`github.com/rafalh/rust-fatfs`** | **UNAUTHORISED, unintended**: `cargo update --workspace --dry-run` printed `Updating git repository` for the `fatfs` git dependency (`kernel/Cargo.toml:18`, `cells/services/vfs/Cargo.toml:18`, locked at `c4b88477…`, `Cargo.lock:1855`). Read-only git fetch into `/tmp/kssh-k1/cargo-home/git`; later runs did not refetch. Outside the authorised host list; flagged here. | cargo |

No other host. No `rustup` network use (toolchain preinstalled). No inbound connection, no listening socket.

## 10. Not checked

- Execution on Cellos/QEMU (no VM allowed): KDF timing on the target is inference; the probes were compiled, not run (host results come from the std `hosttest`).
- `cargo audit` is not installed; the same RustSec DB was read through `cargo-deny` and by crate directory.
- Fuzzing, constant-time behaviour of Blowfish S-box lookups (data-dependent, inherent to bcrypt), side-channels of the soft AES backend, or `ed25519-dalek 3` vs `2.2.0` preference for KMS (K5 detail).
- Whether KMS can afford 4,168 B Blowfish state + 64 B buffers on its cell stack (not measured; `bcrypt_pbkdf` alloc variant uses a 256 B stack buffer).
- ssh-key rc.11's own test suite, upstream issue tracker, any cargo-vet/crev audits of the candidate crates; owners/maintainer history of the three new crates (only the crates.io metadata above).
- The unencrypted (`cipher none`) parse branch of the own parser was not built; passphrase UTF-8/normalisation behaviour of OpenSSH not compared.
- Why the baseline deny already fails (not investigated); the Cargo.lock hygiene line in `.github/workflows/security.yml` ("Cargo.lock is not committed") is stale versus `git ls-files`, observed only.
- `third_party/redoxfs`, `tools/*` excluded-workspace lockfiles (not part of the repo `Cargo.lock`).
- Aarch64/riscv64 builds exist for hybrid and ssh-key variants only (own-minimal on x86 only).

## 11. Reproduce

`/tmp/kssh-k1` (scratch, not part of the repo): `repo-slice/` (HEAD manifests), `scen-a|h|b/` (variant workspaces), `probes/*`, `hosttest/`, `cargo-home/`, `target-probes/`. Source of every probe and the host test: `k1-evidence-src/` (`Cargo.toml.txt` = `Cargo.toml`). Binaries' sha256 are in `k1-evidence-logs/probe-sizes.txt`.
