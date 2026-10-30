# KSSH K7 CP2 dossier (admin cell `/bin/kms-ssh-admin`, edge, signer row)

Implemented by `K7Impl` from `k7-cp1.md` over `k7-cp1-draft.md`, approved in `k7-cp1-approval.md`; closure extended by Main with `kernel/src/policy.rs` (one test, answer "A"). Base HEAD `943829c59` at start (tree clean). Main's commit `d752aae10` swept my work-in-progress files into history while I was working; I committed and pushed nothing. `CHANGELOG.md` untouched. No network, key generation, QEMU/VM/image, signing. All cargo runs: `CARGO_TARGET_DIR=/tmp/k7-target`, `--offline --locked`, host target `x86_64-unknown-linux-gnu`.

## 1. Baselines (before any edit; logs `/tmp/k7/base-*.log`) and after

| Command | Before | After |
|---|---|---|
| `-p cellos-kernel` | 292 | **293** (+1: `kms_ssh_admin_edge_is_shell_only_path_only_admin`) |
| same `--features phase04-driver-profile` | 292 | **293** |
| `-p api` | lib 154, contract 2, doc 4 ignored | same |
| `-p ostd` | 84 + 7 + 19 (2 ignored) = 110 | same |
| `-p service-kms` (default) | 65 | 65 |
| `-p service-kms --features ssh-dev-custody` | 122 + 1 ignored | 122 + 1 ignored |
| `-p app-kms-ssh-admin` (new) | n/a | **71** (debug and `--release`) |
| `sign-policy.py --out` | 2129 B, sha256 `386524a0487b329550061a6d1f4377c5e083917a8ab997fecccd27c5e5932ce1` (deterministic: two runs equal) | 2169 B, `4ee6f63ea8211b6c05c70d2fe8d18114e556692412e81ca596a836ebcb56c579` (+1 row); round-trip and P-TRUST gates pass; kernel test `signer_v8_output_decodes_with_kernel_rule` passes |
| phase04 `authority` pin recompute (`/tmp/k7/pins.py`, 5 pins) | all True | all True after re-pin of `profiles.rs`, `tests.rs`; `bash -n` OK |

Start-of-slice overlap grep (kernel, scripts, cells, libs, docs, `.agents`, root files; the repo-wide grep timed out twice at 300 s, so `libs`/`docs`/`cells` coverage is partial): `kms-ssh-admin` occurs only in K3's kernel/script paths (`selftest.rs`, `launch_profile/tests.rs`, `policy.rs`, `cap.rs`, `sign-policy.py`) and in the plan documents of this program and `261011-1200-cell-contract-authority/plan.md`. No other owner found.

## 2. Hashes (SHA-256)

| Path | Before | After |
|---|---|---|
| `Cargo.lock` | `1d35b7fa1a4ed189e45a07f3f25db907401bfd03d93a6f6ef4fb61b3292d12f6` | `5181251ccc712f27f71db9861b1b4feab0a8fec910e43716a7b744a4f8c81130` |
| `Cargo.toml` | `3c30c55d79ca8ce93781f38cc1739e0fdfdb2e557e2383fa1fcecd7f615153e9` | `a325c8b2d9b5badafaac9f80bb56a34430e4b77245782b6f9288296a9abfcd20` |
| `kernel/src/loader/launch_profile/profiles.rs` | `202745bad8a395770dc6e3d1b7c2d6a1874ef1543f8e1b75cabc6540331c6b38` | `78058780bad2549c67ab7487b2a1e741d52580cbbdd27d1fccef132a69aa2930` |
| `kernel/src/loader/launch_profile/tests.rs` | `f0463db78b787af0e3e7a534058e9a84db358c3de5cd5e6c9e45906c1187a190` | `b41874eb9299487aa5d857617e41027f05f638fd39dc86b6bc4ce19c7a33bc3d` |
| `kernel/src/policy.rs` | `c53c87c557799890f087c64bca818e24ee97fdf2c0538671905371395e690130` | `c16896d908e426faf0fcc73febed18dde81c097b6cd07a2eb4fb1a17b3ae558e` |
| `scripts/sign-policy.py` | `ba7a0d5f89a1bd30983914340c75adf25ca10fdc5cbb810c9d09613c0137214e` | `7e2c2cc7cb3fbd30d36d40525128fdc7f917dadf65daf7df6106ddee24b10203` |
| `scripts/qemu-phase04-terminal-test.sh` | `a51c7a4966b0a560a0140660e07e294960dd9ca9efcd6bfbfe1905612fc42af3` | `776e5897ad6b69a8f834e649a1f9366af5058c20f25f5a0cb52e415f893ab36a` |

Byte-identical (before = after): `launch_profile/targets.rs` `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f`, `launch_profile/mod.rs` `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c`, `boot_ceiling.rs` `3776caa3a0085c057ff0c9cc675024429e132e4ce44998652c503eae9b6e23a6`, `boot_ceiling/selftest.rs` `93521291b4b029a6d7faf5cb3f2c6ffbb34041e7dee81f13c2eb819917a5477f`, `task/cap.rs` `7d7bae8cc71ae176f046a7560602256cbd00a22585d16b2fbad255cc26a5a651`, `CHANGELOG.md` `8a02cdc945ee316696f832ee52e85c8a66884c3574ddc7adccbe695ee5826f8d`.

New crate `cells/tools/kms-ssh-admin/`: `Cargo.toml` `0ac4dc99…`, `build.rs` `309c7f94…`, `src/{armor,console,delete,import,kms,main,report,secret,status}.rs` (1,349 lines) and `src/tests.rs` + `src/tests/{armor,class_guard,console,delete,driver,mapping,output,real}.rs` (2,530 lines). Full per-file digests: `sha256sum` of the tree.

## 3. Lock diff proof

`diff /tmp/k7/Cargo.lock.before Cargo.lock` is exactly one added `[[package]]` stanza `app-kms-ssh-admin 0.1.0` (deps `api`, `base64ct`, `cell-build`, `ostd`, `service-kms`, `terminal-sdk`, `types`), 13 lines. No external package line changed; `base64ct 1.8.3` was already locked. Generated with `cargo metadata --offline` (no network); all later runs used `--locked`.

## 4. Item -> code -> test

| Item | Code | Test |
|---|---|---|
| Launch edge: shell, Path route only, `/bin/kms-ssh-admin`, ceiling LEGACY + `kms_ssh_admin`, never sign; `targets.rs`/`mod.rs` untouched | `profiles.rs`: `KMS_SSH_ADMIN_EDGE`, one `cellbox_profile` arm | `launch_profile::tests::kms_ssh_admin_edge_is_shell_only_path_only_admin` (ceiling literal, label, no network/Q/sign, grant = admin flag only, ELF/Mem/Pinned refused, 8 other callers x 4 routes x 3 states refused, 8 near-miss/sandbox spellings never carry the right); `kms_ssh_sign_is_held_only_by_the_ssh_edge` extended: admin iff (cellbox, Path, exact path), never both on one ceiling/request, sweep floor `admin_edges >= 4` |
| Signer v8 row | `sign-policy.py` DEV_POLICY row `("/bin/kms-ssh-admin", 0,0,0,0,0,0, 0,0,0)` (request helper already existed from K3; columns default to the kernel request: X/M/W + admin) | `policy::tests::signer_v8_output_decodes_with_kernel_rule`: every row's rights equal `with_path_caps`; admin holders are exactly `["/bin/kms-ssh-admin"]`; no row has both rights |
| Phase04 pins | `qemu-phase04-terminal-test.sh`: `profiles.rs`, `tests.rs` digests + comment | `/tmp/k7/pins.py` all True; `bash -n` |
| Manifest all false; allowlist = terminal-session child set (phase02-session-child set minus `SpawnFromPath`) | `main.rs` | `class_guard::the_allowlist_is_the_terminal_session_child_set` (exact list, no GetRandom/spawn/net/VFS/Read/Write/Open, manifest has no `true`) |
| Echo-off lease reader: typed text, paste (bracketed or not), Backspace, Escape/Ctrl-C/EOF cancel, ends at the END line, cap 98,304, rest of a running paste dropped on stop (it would otherwise reach the shell as commands) | `console.rs::read_block`, `Tty` (`set_mode(2,0)` lease) | `tests/console.rs` (10 tests: typed lines, paste in 1/many/split-in-marker events, unterminated last paste line, CRLF, cancel keys, backspace incl. line reopening, overflow + drain, cancel inside paste, junk after END) |
| Confirmation word reader (typed only, paste ignored, bounded) | `console.rs::read_word` | `tests/console.rs` (2 tests) |
| Strict armor decode with `base64ct` (O-K7-5) | `armor.rs::decode`: exactly one block, exact markers, optional empty lines outside, LF/CRLF only, body = non-empty base64 lines, canonical padding and trailing bits, decoded 1..=65,536, refused before any frame | `tests/armor.rs` (13 tests): widths 4/64/70/76/one line, LF/CRLF, sizes 1/96/97/65,534/65,535/65,536 accepted and 65,537/65,538 refused, text cap +/-1, empty, missing/extra/mismatched/space/case markers, two blocks, text before/after, 10 bad body lines, 11 bad paddings + non-canonical bits (`QR==`, `QUJ=`), CR only, empty body, oversized body, `Secret` wipe |
| Chunk driver over the K6 admin methods: Begin check (handle, length, canonical count, capacity), chunks with ack checks, Commit, Abort on every failure after Begin, none after a timeout | `import.rs::drive`, `kms.rs` trait + `impl Kms for KmsClient` (ops 15-20 only) | `tests/driver.rs` (11): K2 4.2 vectors N = 1, 96, 97, 250, 290, 65,535, 65,536 (count, last length, order, reassembly, digest), 5/6/685 round trips, 40 random sizes re-chunk identically, local refusals send nothing, abort after each failing step, timeout aborts nothing, 8 reply deviations -> mismatch + abort (zero handle: no abort possible) |
| import/delete/status with exact text | `import.rs`, `delete.rs`, `status.rs`, `report.rs` | `tests/output.rs` (7), `tests/delete.rs` (6), `driver.rs::the_import_command_*` |
| Delete: status first, public key from the reply, typed `delete` + Enter, Escape/other word/paste cancel, no op 20 without it | `delete.rs` | 7 refusal variants send exactly one frame (status); lost session -> 125 |
| Zeroized buffers | `secret.rs` (fixed capacity, volatile wipe of the whole allocation on drop; text buffer dropped before the KMS calls; decoded blob dropped at end of run; text events scrubbed) | `armor.rs::a_secret_buffer_is_wiped_*` (inspects the allocation after `wipe`/`pop`/`truncate`) |
| Error mapping table, one fixed message per code, exit codes 0/1/2/124/125/130 | `report.rs::kms_failure` | `tests/mapping.rs` (7): the 12 draft rows verbatim, every `KmsErrorCode` value 0..=200 (single ASCII line, exit 1, deterministic), transport/protocol rows, timeout 124, grammar and usage (exit 2, never connects) |
| No terminal -> refusal, 125 | `main.rs::entry` (no control grant or `from_own_context` failure -> message on stderr grant if any) | bare-metal build only (no host seam); text/code constants pinned in `mapping.rs::the_no_terminal_refusal_text_is_fixed` |
| No ops 21-24 | `Kms` trait has six methods; no source names a sign/unlock method, payload or `KmsOpcode` | `tests/class_guard.rs` (4): banned identifiers over all 9 source files, trait method list, allowlist/manifest, no log/entropy/VFS/net names |
| No key byte in output | `report.rs` formats only name, KMS status fields, blob length, SHA-256 prefix | `output.rs::no_key_byte_appears_in_any_output` (24 random blobs x 5 endings; base64 body lines, hex windows, base64 windows of raw bytes absent from stdout and stderr) |
| Integration: admin driver against the real `service-kms` (`ssh-dev-custody`) | dev-dependencies `service-kms`, `types` | `tests/real.rs` (11): plain import + status leaf/named + fingerprint equals independent SHA-256 of the wire public key; wrapped import; maximal 65,5xx-byte blob = 685 frames stored; frame layouts decode as K2; second import refused (same name: `SshKeyExists`, other: `SshStoreFull`), delete then re-import; delete without word sends only op 19; refused blob -> Commit refused then Abort, store empty, retry works; callers with no flag / sign flag / both flags -> "not an admin caller", status prompt never shown; no opcode outside 15..=20 in any run; silent service -> 124, nothing sent after |

## 5. Choices where the text was open

1. **Bridge, not `KmsClient`, in the integration test.** The K6 test seam (`KmsClient::scripted`, the `script` field) is `#[cfg(test)]` inside `ostd`, so another crate cannot reach it and `KmsClient::connect` needs the kernel. The test bridge encodes the K2 payloads with `types`, builds the 128-byte frames, calls `KmsService::handle` with a synthetic attested caller and applies `call_bounded`'s envelope checks. It exercises the cell's driver and the real service; it does **not** run `KmsClient`'s own methods against the service (K6's ostd tests still cover those against a script). Closing that gap needs an `ostd` change (outside the closure).
2. **`types` dev-dependency** (local path, no new package) in addition to the approved `service-kms` one: frames and payload codecs live there and ostd does not re-export `KmsRequestV1`.
3. **Armor refusal exit code = 2** (the draft lists 1 for KMS refusals, 2 for usage; local input refusal is neither). Oversized text, wrong armor, bad base64: exit 2, fixed message.
4. **Empty lines outside the block are accepted** ("nothing else outside the block" read as no non-line-break bytes); any other byte, spaces included, is refused.
5. **Argv is the argument list** (`argv[0]` is `status|import|delete`), as the shell's `spawn_external` passes `args` without the program name.
6. **Drain of a running paste** on cancel/overflow/after END, bounded at 512 KiB, so leftover pasted key text never reaches the shell (not in the draft; same intent as its secret hygiene).
7. **Output text** (the draft names elements, not strings): `kms-ssh-admin: imported key 'NAME'` then `  custody:`, `  fingerprint: SHA256:..`, `  posture:`, `  blob: N bytes, sha256 prefix <16 hex>` and for plain `  WARNING: unencrypted key held by the key service (development posture)`; leaf status prints `readiness: <KmsCapabilityReadiness Debug name>` (ostd does not re-export that enum), `keys: N`, posture. Prompts on stderr.
8. **Confirmation word** is read under the session's current mode (no echo lease, nothing echoed by the cell); whether the owner echoes typed text is not verified here.
9. **Own `Secret`, `ostd::session::{zeroize, sha256}`** instead of the `zeroize`/`sha2` crates: the only external dependency is `base64ct`.
10. **`boot_ceiling.rs` / `selftest.rs` byte-identical**: no boot row for the admin path is needed (it is not boot-started); the K3 self-test loops already assert no boot row and neither right after request ∩ boot ceiling. `cap.rs` byte-identical: its helper already requests the right for the exact path.

## 6. Deviations from the text

- `kernel/src/policy.rs` edited (Main's closure extension): one assertion replaced by "admin holders == [`/bin/kms-ssh-admin`]". No production code change.
- `cap.rs` still carries K3's comment "no launch edge or boot row carries `kms_ssh_admin` yet" (line ~577), now stale; left because the closure says byte-identical. Suggest a one-line comment fix with the next `cap.rs` edit.
- The K3 `profiles.rs` comment at `cellbox_profile` and the `sign-policy.py` header were updated only where K7 changes the statement.

## 7. Builds and lint (private target dir)

| Check | Result |
|---|---|
| `cargo build --release -Z build-std=core,alloc -p app-kms-ssh-admin --target x86_64-unknown-none` with `CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic"` | OK, 0 warnings; text/data/bss 91,303 / 1,272 / 1,048,625 |
| same, `riscv64gc-unknown-none-elf` | OK, 0 warnings; 72,521 / 1,736 / 1,048,800 |
| `cargo clippy --no-deps -p app-kms-ssh-admin -- -D warnings`, riscv64 build-std and host `--tests` | clean (the CI form with deps is red at baseline in `api`/`vissh`, K3/K6 dossiers; not re-measured) |
| kernel clippy | not run (red at baseline per K3 dossier) |
| `rustfmt` | the new crate was formatted with `rustfmt` (module tree of `main.rs`); no other file formatted |

bss of about 1 MiB is ostd's default heap arena (same as K5's note); the cell's live heap peak is about 250 KB (98,304 text + 87,384 compact base64 + 65,538 blob) during decode.

## 8. Not covered / deferred

- Bare-metal-only code has no host test: `Tty` (lease, event wait loop, writes), `entry` (control-grant check, no-terminal refusal), `cell_main`. Lane evidence is K8: `kms-ssh-admin import lane` with paste on serial/GUI, burst throughput (R-4), echo of the confirmation word, `/bin/kms-ssh-admin` via the shell edge actually receiving bit 7, non-admin callers refused, T0 restart empties the store, serial-log leak check.
- How the shell resolves `kms-ssh-admin` typed at its prompt to `/bin/kms-ssh-admin` (reading A: no Cellbox change). `spawn_external` builds `/bin/<prog>` and tries the kernel path route first, so the edge is reachable; not exercised.
- Image packaging, `POLICY.BIN` promotion (v7 stays), CHANGELOG, docs, lane markers (K8).
- Prompt has no inactivity timeout (draft names none); Escape cancels.

## 9. Things the user may want to decide

1. Accept the bridge-based integration test (choice 1) or authorise an `ostd` change exposing the client's test seam so the real `KmsClient` runs against `KmsService`.
2. Armor/input refusal exit code 2 (choice 3) versus 1.
3. [INFERENCE, not run] `cargo test --workspace` now enables `service-kms/ssh-dev-custody` for the whole test unification (dev-dependency, resolver 2): K5's default-feature test count (65) is unaffected for `-p service-kms`, but a workspace-wide test run builds KMS with the feature, and `CELLOS_PRODUCTION=1` there would trip K5's build.rs guard. CI test recipes that run `--workspace` under production env need a look.
4. Stale `cap.rs` comment (section 6).
