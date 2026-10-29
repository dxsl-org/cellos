# Cellbox disposable verification fixtures — candidate

Status: prepared instructions only; no target execution, image admission, signing approval, server startup or CP1/CP2 is recorded here. Maintainer selects the exact profile and accountable owners before execution. This completes fixture design, not target feasibility or acceptance.

## Admission and provenance

For each selected slice record source commit plus dirty-byte manifest, toolchain, dependency lock/checksums/features, target and codegen, kernel hash, separately embedded init hash, every signed Cell hash, policy hash, accepted verification key identity and assembled image hash. Preserve build logs and the signed-image verification outcome. Existing signature sections and equal payload bytes in [baseline.md](baseline.md) are not verification of this chain. Missing provenance blocks signed comparisons, not read-only preparation.

Use one private run directory, private writable copies of every attached data disk, and one owned VM PID. Never reuse a user's running VM, kill image users, overwrite canonical images or package stale artifacts after build failure. Image construction/signing requires separate scoped approval; current scripts that warn and continue are not an admissible fail-closed builder (`scripts/build-x86_64-cells.ps1:119-175`).

## Existing runner and its limits

The currently existing desktop command, **not executed for this preparation**, is:

```sh
CELLOS_DESKTOP_ISO=/absolute/private/run/admitted.iso CELLOS_DESKTOP_ACCEL=tcg cargo test --offline --locked --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test desktop-x86 -- --nocapture --test-threads=1
```

The ISO path must already exist and have the admitted identity. `tcg` is emulator evidence only; select `kvm` explicitly only with separate availability/admission. This runner checks existing boot/pointer/serial-help behavior, not new Cellbox acceptance (`tests/integration/tests/desktop-x86.rs:7-17,63-187`). It does not provide a new editor, search or SSH test merely by booting successfully.

`QemuRunner::boot_x86_bios_with_accel` uses q35, 256 MiB, ISO boot, display disabled, loopback serial and QMP (`tests/integration/src/lib.rs:837-919`). It allocates QMP by bind/release/rebind: collision remains possible and must fail, never terminate another process. It specifies no explicit network-disable flag, no writable data disk, no entropy device and no private capture directory. Consequently do not treat it as an isolated networking/SSH/persistence fixture. The eventual slice runner must explicitly disable networking for local scenarios (`-nic none`), select private capture paths and attached disks, and prove its effective QEMU argv before use. Do not silently alter this existing runner in preparation.

Never run `run-desktop.sh` unattended: its termination and inbound SSH-forward behavior is outside this fixture. No Cellos inbound sshd may be used as a trusted server.

## Scenario matrix

Each row requires its implementing phase's admitted consumer; proposed package names are not executable targets today. Capture actual outputs/status/state rather than source assertions or mock echoes.

| Phase | Disposable inputs and actions | Required observations |
|---|---|---|
| 01 | Read-only split-tool census; selected matched source/image profile | Counts and bytes with exact feature/target equivalence; unknown provenance remains explicit |
| 02 | Two sessions; real child and descendant; retained duplicate writer; closed consumer; slow reader; cancel and holder death; stale generation | Exact byte sequence, distinct timeout/EOF, last-authorized-writer EOF, bounded backpressure, isolated subtree revocation, foreground and mode restoration |
| 03 | Signed/unsigned boundaries including u64 max; raw non-UTF-8 paths; malformed/deep/oversize JSON; invalid stateful/mixed/background forms with file/state sentinels | Exact native values; explicit conversions; bounded memory; no effects before admission; whole-job status and exactly-once reap |
| 04 | Init, serial shell, desktop, scripts, completion/help, image entries and every restricted caller/right edge | One suite executable, migrated callers, retained services, real denials without common-ELF privilege union; final retirement only after CP2 |
| 05 | Throwaway standalone SDK TUI plus actual GUI shell; fragmented ANSI/UTF-8; key/paste/resize; crash while raw/alternate-screen active | Changing real output and screenshots, independent SDK consumer, correct cursor/grid/foreground recovery, no fixed private interpreter |
| 06 | Multi-task Cell, restart and ID reuse, denied query, unavailable/reset/truncated sample | Stable incarnation identity, honest completeness, correct units and no double-counted shared heap |
| 07 | File larger than 64 KiB spanning read chunks; hidden/ignored/binary/link/denied paths; invalid regex; early consumer close | Upstream-derived search records with exact byte spans; status 0/1/2; bounded traversal and prompt cancellation |
| 08 | Disposable original/destination files; existing and concurrently created/changed Save As targets; injected short write/replace/durability failure | Original and destination bytes preserved on refusal/failure; dirty buffer retained; no stale-confirmation overwrite or false durability |
| 09 | Separately admitted other-host SSH server and real Tier-3 guest; secure host identity/auth; exec and PTY; resize, rekey and disconnect | Negotiated secure session; exact stdout/stderr/status and EOF; changed-key rejection; no secret logs/replay; distinct host vs guest evidence |
| 10 | Same matched scenario/data/features before and after; final signed image | Every CBX evidence row, all delta CP2 decisions, actual footprint/startup/heap/stack/stream/redraw measurements; no hardware/production extrapolation |

## SSH admission record

Before any server/key/network activity specify accountable operator, endpoint/route and bounded lifetime, server/version and guest provenance, host public-key fingerprint, authentication method and credential owner, private known-hosts path, entropy-provider proof, egress restrictions and cleanup owner. Use a dedicated isolated fixture network with no unsolicited inbound host forwarding. Unknown/mismatched host keys require explicit policy decisions; never auto-accept or reuse the unsafe local sshd. A fixture not supplied blocks only the named host/guest scenario; it cannot produce fabricated success.

## Evidence and cleanup

Per execution retain phase/CBX IDs, exact candidate digest and CP1, implementation identity/diff, full command/effective argv, input hashes, target/features, fixture/image/policy identities, sanitized serial log, actual screenshot/state and status. Distinguish pass, fail, blocked and not-run. A screenshot alone does not prove bytes, saves, grants or negotiated security. CP2 follows implementation evidence and precedes final consumer/image retirement.

Stop/reap only the owned process; revoke its disposable grants and remove its sockets, private copies, throwaway SDK consumer and temporary credentials after saving sanitized evidence. Preserve unknown write/remote outcomes and do not replay. Restoring code/images cannot undo persisted files, trust or remote commands.
