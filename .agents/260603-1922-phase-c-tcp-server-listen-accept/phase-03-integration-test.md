# Phase 3 — Integration test (guest as TCP server)

## Context Links

- `tests/integration/src/lib.rs:65-118` — `QemuRunner::boot` (the arg list to mirror).
- `tests/integration/src/lib.rs:122-145` — `wait_for`, `send_line`.
- `tests/integration/tests/boot.rs:26-56` — `kernel_path`, `disk_path`, `prerequisites_ok`.
- `tests/integration/tests/boot.rs:231-258` — `network_tcp_send_recv` (closest analog).
- `tests/integration/tests/boot.rs:11,13` — imports + `BOOT_TIMEOUT = 40`.

## Overview

- **Priority:** P2.
- **Status:** pending.
- Add `QemuRunner::boot_with_hostfwd` (SLIRP host→guest port forward) and a
  `network_tcp_listen_accept` test: guest `nc -l` is the server, host is the client.

## Key Insights

1. **Inverted roles vs Phase A.** `network_tcp_send_recv` runs a host echo server
   and the guest connects out (SLIRP routes guest→`10.0.2.2:port`→host). For a
   guest *server*, SLIRP needs an explicit `hostfwd=tcp:127.0.0.1:<host>-:<guest>`
   so the host client reaching `127.0.0.1:<host>` lands inside the guest.
2. **`boot()` arg list is specific** (`lib.rs:71-86`): `-m 256M`, `virtio-gpu-device`,
   `-monitor none`, and serial bridged over a *separate* host TCP socket
   (`-serial tcp:...`). `boot_with_hostfwd` must mirror this exactly, changing ONLY
   the `-netdev` string. Do NOT copy the context's simplified "128MB / user,id=net0"
   description — use the real values.
3. **Two host TCP endpoints, do not confuse them:** (a) the serial-console socket
   QEMU connects to as a client (`-serial tcp:127.0.0.1:<serial_port>`), and (b) the
   hostfwd port the *test* connects to as a client to reach the guest nc. They are
   independent ephemeral ports.
4. **bind+drop TOCTOU.** Discover a free host port via `TcpListener::bind(":0")`,
   read `local_addr().port()`, then `drop` so QEMU can bind it. A race window
   exists but is acceptable in test environments — document it inline.

## Requirements

**Functional**
- `boot_with_hostfwd(kernel, disk, guest_port) -> (QemuRunner, host_port)`.
- `network_tcp_listen_accept`: boot, wait shell + DHCP, `nc -l 9090`, host connects
  to `127.0.0.1:<host_port>`, sends `PING_ViCell\n`, assert serial shows `PING_ViCell`.

**Non-functional**
- Skip (not fail) when prerequisites missing (mirror `prerequisites_ok` gate).
- Keep all 23 existing tests green.

## Architecture / Data flow

```
host test ── connect 127.0.0.1:host_port ──► QEMU SLIRP ──hostfwd──► guest :9090 ──► nc -l
host test ── write "PING_ViCell\n" ──────────────────────────────────────────────────► nc
                                                              nc prints to serial ──► QemuRunner.output
host test ── wait_for("PING_ViCell") on serial socket ◄───────────────────────────────┘
```

## Related Code Files

**Modify**
- `tests/integration/src/lib.rs` — add `boot_with_hostfwd`.
- `tests/integration/tests/boot.rs` — add `network_tcp_listen_accept`; ensure
  `TcpStream`/`Duration` imports present (add to `use` if missing).

**Create / Delete:** none.

## Implementation Steps

### Step 1 — `lib.rs`: add `boot_with_hostfwd`

Refactor to avoid duplicating the whole `boot` body: extract a private
`boot_with_netdev(kernel, disk, netdev: &str) -> Self` containing the current
`boot` body with the `-netdev` value parameterized, then:

```rust
    /// Boot QEMU with the default user-mode NIC (no port forward).
    pub fn boot(kernel: &str, disk: &str) -> Self {
        Self::boot_with_netdev(kernel, disk, "user,id=net0")
    }

    /// Boot QEMU with a SLIRP hostfwd: host `127.0.0.1:<host_port>` → guest
    /// `guest_port`. Returns `(runner, host_port)`.
    ///
    /// Host port is discovered by binding `:0` then dropping the listener so
    /// QEMU can bind it. This is a benign TOCTOU race — acceptable in test envs.
    pub fn boot_with_hostfwd(kernel: &str, disk: &str, guest_port: u16) -> (Self, u16) {
        let probe = TcpListener::bind("127.0.0.1:0").expect("probe bind");
        let host_port = probe.local_addr().unwrap().port();
        drop(probe); // release so QEMU/SLIRP can bind it momentarily

        let netdev = format!(
            "user,id=net0,hostfwd=tcp:127.0.0.1:{host_port}-:{guest_port}"
        );
        (Self::boot_with_netdev(kernel, disk, &netdev), host_port)
    }
```

The extracted `boot_with_netdev` is the EXACT body of the current `boot`
(`lib.rs:65-118`), with line 80 `"-netdev", "user,id=net0",` replaced by
`"-netdev", netdev,`. All other args (`-m 256M`, `virtio-gpu-device`,
`-monitor none`, `-serial tcp:...`, the serial-socket accept, the reader thread)
stay identical. `TcpListener`/`TcpStream` are already imported (`lib.rs:13`).

### Step 2 — `boot.rs`: add the test

Place after `network_tcp_send_recv` (`boot.rs:258`). Ensure these are imported at
the top of `boot.rs` (add any missing):

```rust
use std::io::Write;                       // for stream.write_all
use std::net::TcpStream;                  // for host→guest connect
use std::time::Duration;                  // for sleeps
use ViCell_integration_tests::QemuRunner;   // boot_with_hostfwd is a method on it
```

(`QemuRunner`, `qemu_binary`, `spawn_echo_server`, `spawn_http_server` are already
imported at `boot.rs:11`; add only what is genuinely missing.)

```rust
/// Phase C: guest acts as a TCP SERVER. `nc -l 9090` listens; the host connects
/// through QEMU SLIRP hostfwd, sends "PING_ViCell\n", and nc echoes the bytes to
/// serial — proving LISTEN/ACCEPT and the inbound data-path work end-to-end.
#[test]
fn network_tcp_listen_accept() {
    if !prerequisites_ok() {
        return;
    }

    let (mut qemu, host_port) =
        QemuRunner::boot_with_hostfwd(&kernel_path(), &disk_path(), 9090);

    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.wait_for("DHCP acquired", 40)
        .unwrap_or_else(|e| panic!("DHCP failed: {e}\n--- output ---\n{}", qemu.dump()));

    std::thread::sleep(Duration::from_millis(500));

    // Start the guest server.
    qemu.send_line("nc -l 9090");
    qemu.wait_for("listening", 10)
        .unwrap_or_else(|e| panic!("nc did not listen: {e}\n--- output ---\n{}", qemu.dump()));

    // Give nc time to reach the ACCEPT poll loop, then connect from the host.
    std::thread::sleep(Duration::from_millis(1000));
    let mut stream = TcpStream::connect(format!("127.0.0.1:{host_port}"))
        .unwrap_or_else(|e| panic!("host connect to guest failed: {e}\n--- output ---\n{}", qemu.dump()));

    qemu.wait_for("connected", 15)
        .unwrap_or_else(|e| panic!("nc did not accept: {e}\n--- output ---\n{}", qemu.dump()));

    // Send a probe; nc echoes it back to serial.
    stream.write_all(b"PING_ViCell\n").expect("write failed");
    let _ = stream.flush();

    qemu.wait_for("PING_ViCell", 20)
        .unwrap_or_else(|e| panic!("guest did not receive probe: {e}\n--- output ---\n{}", qemu.dump()));
}
```

### Step 3 — run the full suite

```
cargo test -p ViCell-integration-tests --test boot -- --test-threads=1
```

Single-threaded: each test spawns its own QEMU + binds ephemeral ports; parallel
runs risk QEMU resource and port contention. Confirm `network_tcp_listen_accept`
passes AND the existing 23 stay green.

## Todo List

- [ ] lib.rs: extract `boot_with_netdev`, keep `boot` as thin wrapper
- [ ] lib.rs: add `boot_with_hostfwd` with bind+drop port discovery
- [ ] boot.rs: add `network_tcp_listen_accept` + any missing imports
- [ ] Build kernel + disk, run full suite single-threaded, 24/24 green

## Success Criteria

- `network_tcp_listen_accept` passes: serial shows `listening`, `connected`, `PING_ViCell`.
- All 23 prior tests still pass (24 total).
- No new clippy/build warnings in the test crate.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| hostfwd port grabbed between drop and QEMU bind | Low | Test flake | Document TOCTOU; rerun on transient failure; ephemeral range collisions rare. |
| nc not yet in ACCEPT loop when host connects | Med | Test flake | 1000ms sleep after "listening"; ACCEPT poll loop also tolerates pre-arrival. |
| `boot_with_netdev` refactor breaks `boot` | Low | All net tests fail | Body is byte-identical except netdev; verify 23 tests still pass. |
| Parallel test QEMU/port contention | Med | Flake | Run `--test-threads=1` (matches existing reboot-persistence tests' needs). |
| SLIRP doesn't route hostfwd before DHCP | Low | Connect fails | Gate on "DHCP acquired" before connecting (matches Phase A pattern). |

## Backwards Compatibility

- `boot` signature unchanged — existing 23 tests call it identically. The refactor
  only extracts shared body; behavior is preserved.
- New public method `boot_with_hostfwd` is purely additive.

## Security Considerations

- hostfwd binds `127.0.0.1` only (not `0.0.0.0`) — no exposure beyond loopback.

## Next Steps

- This is the final phase. On green, update `docs/project-changelog.md` and
  `docs/development-roadmap.md` (Phase C → complete) per documentation-management rules.
- Rollback: revert both test files; `boot_with_hostfwd` and the new test vanish,
  `boot` reverts to its inline form.

## Consolidated Unresolved Questions (all phases)

1. **`SocketState::Closed` dead-code** (Phase 1): after removing the enum-level
   `#[allow(dead_code)]`, does clippy flag `Closed`? Decide narrow allow vs. keep.
2. **`ostd::io::print_usize` availability** (Phase 2): is it `pub` and reachable
   from net-tools binaries? If not, use an itoa fallback (literal "listening"
   suffices for the test).
3. **net-tools shared module** (Phase 2): do nc/curl share a lib module where
   `resolve_host` belongs, or duplicate per KISS? Verify before choosing.
4. **hostfwd timing vs DHCP** (Phase 3): confirm SLIRP hostfwd is active
   immediately at boot (it is config-time, independent of guest DHCP) — the DHCP
   gate is only for the guest's *outbound* identity, not inbound forwarding. If
   the connect still races, add a short retry loop around `TcpStream::connect`.
