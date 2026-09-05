# Cellos Network Service API

The Net Service Cell (`cells/services/net/`) drives a smoltcp TCP/IPv4 stack
backed by either the kernel VirtIO NIC driver or the e1000 Driver Cell and
provides BSD-style socket IPC.

---

## Architecture

```
Consumer cell (shell / curl / app)
        │  IPC Call (socket, connect, send, recv, …)
        ▼
Net Cell (cells/services/net/)
        ├─ socket_table: CapId → smoltcp SocketHandle
        ├─ smoltcp Interface + SocketSet
        │    ├─ TcpSocket, UdpSocket, Dhcpv4Socket
        │    └─ Network device adapter (VirtIO or e1000)
        └─ DHCP client (acquires 10.0.2.15 from QEMU)
                │
                ▼ frame transport
        Kernel VirtIO driver or e1000 Driver Cell
                │ DMA + IRQ/polling
                ▼
        QEMU NIC → host user-mode network (10.0.2.0/24)
```

---

## DHCP Boot Sequence

1. Net Cell starts, adds `Dhcpv4Socket` to the smoltcp socket set.
2. On each poll loop, `dhcp::poll_dhcp()` ticks the DHCP state machine.
3. When the QEMU DHCP server grants a lease, the IP is applied to the smoltcp
   interface and logged: `[net] DHCP acquired — IP configured`.
4. The net Cell stores the IP in `local_ip` for `GET_LOCAL_IP` queries.

Default QEMU assignment: **10.0.2.15/24**, gateway **10.0.2.2**.

---

## Inbound IPC (kernel → net cell)

Raw Ethernet frames arrive from `virtio_net::handle_irq()` or the e1000
Driver-Cell polling bridge.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 | opcode `0x00` = RX_FRAME |
| 1 | N | raw Ethernet frame bytes |

---

## Outbound IPC (net cell → kernel)

TX frames are sent back via `interface::NetTxToken`.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 | opcode `0x01` = TX_FRAME |
| 1 | N | raw Ethernet frame bytes |

---

## Socket IPC (consumer cell → net cell)

All requests use a common envelope:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 1 | opcode (see table below) |
| 1 | 8 | CapId (0 for new-socket requests) |
| 9 | N | opcode-specific payload |

### Opcodes

| Opcode | Name | Payload | Reply | Status |
|--------|------|---------|-------|--------|
| `0x10` | SOCKET_TCP | — | CapId (u64 LE, 0 = error) | ✅ Impl |
| `0x11` | SOCKET_UDP | — | CapId (u64 LE) | ✅ Impl |
| `0x12` | CONNECT | `addr[4] port[2]` (IPv4 + port LE) | `0x00` ok / `0xFF` err | ✅ Impl |
| `0x13` | SEND | data bytes | bytes_sent (u32 LE) | ✅ Impl |
| `0x14` | RECV | buf_len (u32 LE) | data bytes | ✅ Impl |
| `0x15` | CLOSE | — | `0x00` ok | ✅ Impl |
| `0x16` | BIND | `port[2]` (u16 LE) | `0x00` ok / `0xFF` err | ✅ Impl |
| `0x17` | LISTEN | `port[2]` (u16 LE) | `0x00` ok / `0xFF` err | ✅ Impl |
| `0x18` | ACCEPT | — | CapId (u64 LE) for new stream | ✅ Impl |
| `0x19` | SOCKET_STATE | — | 1 byte state (see below) | ✅ Impl |
| `0x20` | GET_LOCAL_IP | — | `4` bytes IPv4 | ✅ Impl |
| `0x21` | SENDTO | `addr[4] port[2]` + data | bytes_sent (u32 LE) | ✅ Impl |
| `0x22` | RECVFROM | buf_len (u32 LE) | `[src_addr:4][src_port:2 LE][data]` | ✅ Impl |

**SOCKET_STATE (0x19) Reply**:
```
0x00 = Created
0x01 = Connecting
0x02 = Connected
0x03 = Listening
0x04 = CloseWait (FIN received)
0x05 = Closed
```

**Note:** The socket, TCP, and UDP opcodes in the table are implemented. The
higher-level `NetRequest::Resolve` service request is not: it currently returns
`0xFF` from `handlers.rs`.

---

## Socket CapId Lifecycle

### TCP Client
```
SOCKET_TCP → CapId (N)
     │
     ├─ CONNECT(N, addr, port)  → 0x00 ok / 0xFF err
     ├─ SEND(N, data)           → bytes_sent (u32 LE)
     ├─ RECV(N, buf_len)        → data bytes
     ├─ SOCKET_STATE(N)         → 1 byte state
     │
     └─ CLOSE(N)                → 0x00 ok  (smoltcp socket freed)
```

### TCP Server
```
SOCKET_TCP → CapId (N)
     │
     ├─ BIND(N, port)           → 0x00 ok / 0xFF err
     ├─ LISTEN(N, port)         → 0x00 ok / 0xFF err
     │
     └─ ACCEPT(N)               → CapId (M) for accepted stream
          │
          ├─ SEND(M, data)      → bytes_sent
          ├─ RECV(M, buf_len)   → data
          │
          └─ CLOSE(M)           → ok
```

### UDP Socket
```
SOCKET_UDP → CapId (N)
     │
     ├─ BIND(N, port)           → 0x00 ok (optional, assigns ephemeral port if port=0)
     ├─ SENDTO(N, addr, port, data) → bytes_sent (u32 LE)
     ├─ RECVFROM(N, buf_len)    → [src_addr:4][src_port:2 LE][data]
     │
     └─ CLOSE(N)                → ok
```

**CapId Allocation**: CapId 0 is reserved for errors. Maximum 16 concurrent user sockets (+ 1 DHCP + 1 ARP management socket = 18 total).

**Opcode-Specific Buffer Minimums**:
- CONNECT request: minimum 15 bytes (1 opcode + 8 cap + 4 addr + 2 port)
- RECV request: minimum 13 bytes (1 opcode + 8 cap + 4 buf_len)
- SENDTO: minimum message buffer depends on payload size

---

## MAC Address

The net cell uses a fixed MAC address:
`52:54:00:12:34:56` (locally-administered unicast, QEMU-compatible).

The kernel driver reads the actual MAC from VirtIO config space at init time.

---

## DNS Resolver

The Lua `vnet` binding implements a client-side resolver on top of net-service
UDP sockets. It uses this fallback chain:

1. **Static Hostname Table**: gateway → 10.0.2.2, dns → 10.0.2.3, localhost → 127.0.0.1
2. **IPv4 Literal**: If hostname is dotted-quad (e.g., 192.168.1.1), return as-is
3. **UDP A-Record Query**: Send DNS query to 10.0.2.3:53, parse answer section

This behavior lives in `cells/runtimes/lua/src/bindings_net.rs`; it must not be
confused with the unimplemented service-level `NetRequest::Resolve` request.

**Python status**: native MicroPython network bindings are historical only; Python workloads belong in the Tier 3 Linux VM path.

---

## Performance Targets (PDR)

| Metric | Target |
|--------|--------|
| TCP throughput | ≥ 50 Mbps in QEMU user-mode |
| UDP throughput | ≥ 100 Mbps (datagram-based) |
| Loopback RTT | < 10 ms |
| DNS lookup (static table) | < 1 ms |
| DNS lookup (UDP A-record) | < 100 ms |
| Max simultaneous sockets | ≥ 16 |

---

## Files

| File | Purpose |
|------|---------|
| `kernel/src/task/drivers/virtio_net.rs` | VirtIO NIC driver (DMA, IRQ) |
| `cells/drivers/e1000/` | e1000 PCIe Driver Cell (DMA rings, polled Tx/Rx) |
| `cells/services/net/src/main.rs` | Cell entry point + IPC receive loop |
| `cells/services/net/src/interface.rs` | smoltcp Device adapter (RX queue, TX via IPC) |
| `cells/services/net/src/socket_table.rs` | CapId → smoltcp SocketHandle mapping |
| `cells/services/net/src/dhcp.rs` | DHCPv4 boot client |
| `cells/services/net/src/poll_driver.rs` | IPC opcode constants + message decoder |
| `tests/integration/tests/boot.rs` | RISC-V boot and network integration scenarios |
| `tests/integration/tests/nic-riscv.rs` | RISC-V VirtIO NIC scenarios |
| `tests/integration/tests/nic-x86.rs` | q35 e1000 DHCP and VT-d data-plane gates |
| `tests/integration/tests/http-smoke.rs` | HTTP/TLS smoke scenarios |
