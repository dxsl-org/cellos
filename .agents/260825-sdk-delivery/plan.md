# SDK Delivery Plan

## Goal
Complete roadmap-aligned SDK behavior without changing frozen ABI or claiming G2 qualification.

| Phase | Status | Scope | Dependencies |
|---|---|---|---|
| 01 | implemented; target build passed | K1 gate and LAN beacon correctness | None |
| 02 | server verified; Cellos client blocked | Mutual-TLS relay with protected certificate identity | production KMS/Silo identity lifecycle |
| 03 | verified | Native SDK/ViUI contract tests and guide correction | None |
| 04 | verified | VFS grant-backed writes and SDK client result | None |
| 05 | verified | Damage-clipped desktop compositor composition | None |
| 06 | partial | Targeted integration verification and docs sync | 01–05 |

## Decisions
- Preserve `api::abi` and existing `NetRequest`/`NetResponse` ordering and payloads.
- K1 failure must leave beacon, Noise, and relay unavailable.
- Existing UDP receive data includes a six-byte source prefix; broker must strip it rather than alter shared IPC.
- Compositor window policy is a compatibility boundary; no desktop-shell features enter this plan.
- ViUI reactive-v2 remains canonical for generated nodes; legacy Elm stays separate and experimental.
- VFS grant writes must remain owner-bound, re-authorized, quota-checked, and commit-before-acknowledge.

## Blocker
The chosen mutual-TLS relay needs a production protected P-256 signing root, an attested service-net-to-Silo/KMS authorization path, client certificate issuance/provisioning, and a new TLS client-auth profile. Current KMS intentionally fails with `SecureRootRequired`, Silo has no authenticated signing caller path, and the broker NodeId is ephemeral X25519.

## Evidence
- `docs/project-roadmap.md:68-85,148-150`
- `docs/specs/23-native-sdk-contract.md:1-35,213-236`
- `.agents/260825-sdk-delivery/phase-01-broker.md`
- `.agents/260825-sdk-delivery/phase-03-native-sdk-viui.md`
- `.agents/260825-sdk-delivery/phase-04-vfs-grant-write.md`

## Verification
- `cargo test -p viui --target x86_64-unknown-linux-gnu --lib signal::tests`: 5 passed.
- `cargo test -p viui --target x86_64-unknown-linux-gnu --lib app_runner::tests`: 3 passed.
- `cargo test -p viui --target x86_64-unknown-linux-gnu --lib surface_renderer::tests`: 1 passed.
- `cargo build -Z build-std=core,alloc -p service-net-broker --target riscv64gc-unknown-none-elf`: passed.
- `cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p service-vfs -p app-vfs-test --features service-vfs/test-hooks`: passed.
- `cargo test --target x86_64-unknown-linux-gnu -p service-compositor framebuffer::tests`: 3 passed.
- `pwsh ./gen_disk.ps1`, `bash scripts/build-test-hooks-ci.sh`, then `cargo test --manifest-path tests/integration/Cargo.toml --test vfs-quota --target x86_64-unknown-linux-gnu -- --nocapture`: passed (2 tests); guest evidence covers committed count, exact acknowledgement, invalid-offset/short-grant preservation, authorization-before-grant, and quota refusal.
- `python3 -m unittest discover -s tools/relay-server -p '*_test.py' -v`: 23 passed, including mounted-manifest validation and repeated source replacement during blocked destination drain; security re-review found no remaining High/Medium server issue.
- Broker raw relay cutover: obsolete `RelayClient`, `CLIENT_REGISTER`, relay runtime state/polling, and fallback callsites are removed. Direct Noise remains; exhausting direct addresses returns `ViError::NotSupported`. The RV64 broker target build passes and security review found no High/Medium issue.
