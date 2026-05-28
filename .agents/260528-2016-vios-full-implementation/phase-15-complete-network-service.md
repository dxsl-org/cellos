# Phase 15 — Complete Network Service

**Effort:** 200h | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 04

## Overview

Implement the network stack as a Cell: VirtIO net driver, TCP/IPv4 stack via `smoltcp`, DHCP client, BSD-style socket syscalls for Cells. After this phase, the shell can perform `curl http://10.0.2.2/` against the QEMU host network and receive a real HTTP response.

## Context Links

- `docs/07-networking.md` — network architecture
- `cells/services/net/src/lib.rs` — current stub
- `cells/drivers/net/src/lib.rs` — VirtIO net driver stub
- `libs/api/src/net.rs` — `ViSocket` trait, socket types
- Phase 04 (VirtIO patterns), Phase 07 (capability model for socket handles)

## Key Insights

- `smoltcp` is the de facto `no_std` TCP/IP stack in Rust; mature, MIT/Apache. Pin a version (`0.11.x` as of 2026-05). Single-threaded by design — runs entirely inside the net Cell.
- VirtIO net device exposes 2 virtqueues: RX (queue 0) and TX (queue 1). Same descriptor-ring patterns as virtio_blk from Phase 04.
- Socket model: each open socket is a CapId (Phase 07 pattern). Cells call into net cell via IPC; net cell wraps smoltcp socket and ticks its state machine on a timer + on every IPC.
- DHCP: smoltcp ships a `Dhcpv4Socket`. On boot, net cell starts DHCP, waits for IP, advertises ready via Config Cell or a system event.

## Requirements

**Functional**
- VirtIO net driver enumerates, transmits and receives frames
- TCP/IPv4 and UDP/IPv4 supported via smoltcp
- DHCP client acquires IP from QEMU's built-in DHCP (default 10.0.2.x)
- Cells use BSD-style API: `socket → bind → listen → accept` or `connect`, `send`, `recv`, `close`
- Sample app `curl` (in Phase 17) does HTTP GET successfully

**Non-functional**
- Throughput: ≥ 50 Mbps TCP in QEMU
- Latency: < 10ms RTT for localhost-style loopback
- Zero `unsafe` in net cell
- Concurrent: ≥ 16 simultaneous sockets

## Architecture

```
Cell (curl / ssh / app)
   │  IPC Call: socket, connect, send, recv, close
   ▼
Net Cell (cells/services/net)
   ├─ socket handle table: CapId → smoltcp::SocketHandle
   ├─ smoltcp::Interface
   │    └─ device: VirtioNetDevice (poll-driven)
   ├─ DHCP socket
   └─ timer pulse (every 100ms): smoltcp.poll(now)
        │
        ▼
   VirtIO net driver (cells/drivers/net) — wraps kernel virtio_net
        │
        ▼
   kernel/src/task/drivers/virtio_net.rs (NEW)
        │ IRQ + descriptor ring
        ▼
   QEMU virtual NIC → host network (user-mode netdev)
```

## Related Code Files

**Modify:**
- `cells/services/net/src/lib.rs` — full net cell implementation
- `cells/drivers/net/src/lib.rs` — VirtIO net wrapper for the cell
- `libs/api/src/net.rs` — extend with socket syscall shapes + error types
- `libs/api/src/syscall.rs` — add socket-related variants (or route through net cell IPC; pick IPC for capability cleanliness as in Phase 13/14)
- `libs/ostd/src/io.rs` — high-level `TcpStream`, `UdpSocket` wrappers built on the IPC API

**Create:**
- `kernel/src/task/drivers/virtio_net.rs` — VirtIO net kernel driver (mirrors `virtio_blk` patterns from Phase 04)
- `cells/services/net/src/interface.rs` — smoltcp interface setup, polling loop
- `cells/services/net/src/socket_table.rs` — CapId-keyed socket state
- `cells/services/net/src/dhcp.rs` — DHCPv4 boot client
- `cells/services/net/src/poll_driver.rs` — timer + IPC merge into smoltcp.poll
- `tests/integration/network_loopback.rs` — net cell does loopback TCP echo
- `tests/integration/network_curl.rs` — HTTP GET against QEMU host
- `docs/network-api.md` — socket IPC schema

## Implementation Steps

1. **Kernel VirtIO net driver `virtio_net.rs`** (reuse patterns from `virtio_blk.rs`):
   - Discover device via MMIO
   - Set up 2 virtqueues (RX, TX), feature negotiation (CSUM, MAC, CTRL_VQ optional)
   - RX: pre-publish buffers (Box<[u8; 2048]>) to queue
   - TX: caller hands owned buf, driver chains hdr + payload, notifies, frees on completion
   - IRQ: drain used rings for both queues
2. **smoltcp device adapter** `cells/services/net/src/interface.rs`:
   ```rust
   pub struct VirtioNetDevice { rx: VecDeque<Box<[u8]>>, tx_inflight: usize }
   impl smoltcp::phy::Device for VirtioNetDevice {
       fn receive(&mut self, _ts: Instant) -> Option<(RxToken, TxToken)> { … }
       fn transmit(&mut self, _ts: Instant) -> Option<TxToken> { … }
       fn capabilities(&self) -> DeviceCapabilities { … }
   }
   ```
3. **Build interface in net cell main**:
   - Allocate static SocketStorage for max sockets (16+ DHCP + ARP)
   - Initialize `smoltcp::Interface` with device, MAC address (read from VirtIO config space), neighbor cache
4. **DHCP boot** `cells/services/net/src/dhcp.rs`:
   - Add `Dhcpv4Socket`
   - Spin poll loop until `dhcp_socket.poll()` returns `Some(Config)`
   - Set static IP + default gateway on the interface
   - Publish IP info via Config Cell (`net.ip = 10.0.2.15`)
5. **Socket IPC handler**:
   - Messages: `Socket(domain, type) -> Cap`, `Bind(cap, addr)`, `Listen(cap, backlog)`, `Accept(cap) -> Cap+peer`, `Connect(cap, addr)`, `Send(cap, buf) -> n`, `Recv(cap, buf) -> (buf, n)`, `Close(cap)`
   - Use Phase 07's capability transfer for new Cap returns
6. **Poll driver** `cells/services/net/src/poll_driver.rs`:
   - Timer firing every 50ms calls `interface.poll(Instant::now())`
   - Any incoming IPC also triggers a poll (to drain readable data)
   - When polled, walk all sockets, wake waiters whose buffers gained bytes
7. **OSTD wrappers** `libs/ostd/src/io.rs`:
   - `TcpStream::connect(addr) → Result<TcpStream>` — IPC dance
   - `TcpStream::read/write` — owned buf
   - `UdpSocket::bind / send_to / recv_from`
8. **Test loopback** `tests/integration/network_loopback.rs`:
   - Server cell: socket, bind, listen, accept, recv 4 bytes, send 4 bytes
   - Client cell: socket, connect 127.0.0.1, send 4 bytes, recv 4 bytes
   - Assert echo
9. **Test curl** `tests/integration/network_curl.rs`:
   - Boot QEMU with `-netdev user,id=net0,hostfwd=tcp::5555-:5555`
   - Run a simple HTTP server on host port 5555 (Python `http.server`)
   - Shell: `curl http://10.0.2.2:5555/`
   - Assert 200 OK + body
10. **Document** `docs/network-api.md`: IPC schema, error model, supported socket options.

## Todo List

- [ ] Kernel `virtio_net.rs` driver (mirror virtio_blk patterns)
- [ ] smoltcp Device adapter in net cell
- [ ] Build smoltcp Interface, MAC from VirtIO config
- [ ] DHCPv4 boot client
- [ ] Socket IPC handler (10 message variants)
- [ ] Poll driver: timer + IPC + smoltcp.poll
- [ ] OSTD wrappers: TcpStream, UdpSocket
- [ ] Loopback integration test
- [ ] Curl integration test (HTTP GET vs host port)
- [ ] Document `docs/network-api.md`
- [ ] Bench: ≥ 50 Mbps TCP
- [ ] CI green

## Success Criteria

- DHCP acquires IP within 3s of boot
- TCP loopback echo works
- HTTP GET against host returns 200 OK
- 50 Mbps TCP sustained in QEMU bench
- Net cell `#![forbid(unsafe_code)]`
- 16 concurrent sockets without crash
- < 10ms RTT loopback

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| smoltcp API changes (pre-1.0 crate) | High | Med | Pin minor; integration test catches breakage early |
| QEMU usermode networking quirks (NAT, ICMP) | Cert | Low | Document; not all curl behaviors will work (e.g., raw ICMP) — fine for HTTP |
| TX descriptor leak under load | Med | Med | Refcount inflight bufs; assert free count returns to baseline at idle |
| Concurrent poll + IPC racing | High | Med | Single-threaded net cell; serialize all access via async loop |
| MAC address spoofability across cells | Low | Low | Cells go through net-cell IPC; raw frame send not exposed |

## Security Considerations

- Sockets are capabilities (Phase 07 pattern); a cell cannot use another cell's socket
- Source-port allocation: net cell assigns ephemeral ports (32768–60999); a cell cannot bind to a system port (< 1024) unless its policy grants `NET_BIND_LOW`
- DoS: rate-limit per-cell socket count and send-buffer bytes; cell exceeding gets `ViError::QuotaExceeded`
- TLS not in v1.0 scope; document; reuse OS-level network without crypto

## Rollback

Net cell is additive; revert removes networking but doesn't break boot or shell. Phase 17's network tools (`curl`, `ping`, `nc`) become unbuildable if the API surface goes away — keep them gated behind a `net` feature flag.

## Next Steps

Phase 17 builds `curl`, `ping`, `nc`, `wget` on top of the OSTD wrappers. Phase 22 benchmarks throughput. IPv6 and TLS are post-v1.0 stretch.
