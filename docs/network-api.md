# ViOS Network Service API

The Net Service Cell (`cells/services/net/`) drives a smoltcp TCP/IPv4 stack
backed by the kernel VirtIO NIC driver and provides BSD-style socket IPC.

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
        │    └─ VirtioNetDevice (RX queue + IPC TX)
        └─ DHCP client (acquires 10.0.2.15 from QEMU)
                │
                ▼ IPC frames
        Kernel VirtIO net driver (kernel/src/task/drivers/virtio_net.rs)
                │ DMA + IRQ
                ▼
        QEMU VirtIO NIC → host user-mode network (10.0.2.0/24)
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

Raw Ethernet frames arrive from `virtio_net::handle_irq()`.

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

| Opcode | Name | Payload | Reply |
|--------|------|---------|-------|
| `0x10` | SOCKET_TCP | — | CapId (u64 LE, 0 = error) |
| `0x11` | SOCKET_UDP | — | CapId (u64 LE) |
| `0x12` | CONNECT | `addr[4] port[2]` (IPv4 + port LE) | `0x00` ok / `0xFF` err |
| `0x13` | SEND | data bytes | bytes_sent (u32 LE) |
| `0x14` | RECV | buf_len (u32 LE) | data bytes |
| `0x15` | CLOSE | — | `0x00` ok |
| `0x16` | BIND | `port[2]` (u16 LE) | `0x00` ok |
| `0x17` | LISTEN | `port[2]` (u16 LE) | CapId for accept |
| `0x18` | ACCEPT | — | CapId for new stream |
| `0x20` | GET_LOCAL_IP | — | `4` bytes IPv4 |

> **Note:** CONNECT, SEND, RECV, BIND, LISTEN, ACCEPT return `0xFF` (not yet
> implemented) until Phase 17 wires the full data path.

---

## Socket CapId Lifecycle

```
SOCKET_TCP → CapId (N)
     │
     ├─ CONNECT(N, addr, port)  → ok
     ├─ SEND(N, data)           → bytes_sent
     ├─ RECV(N, buf_len)        → data
     │
     └─ CLOSE(N)                → ok  (smoltcp socket freed)
```

CapId 0 is reserved for errors.  Maximum 16 concurrent user sockets (+ 1 DHCP
+ 1 ARP management socket = 18 total).

---

## MAC Address

The net cell uses a fixed MAC address:
`52:54:00:12:34:56` (locally-administered unicast, QEMU-compatible).

The kernel driver reads the actual MAC from VirtIO config space at init time.

---

## Performance Targets (PDR)

| Metric | Target |
|--------|--------|
| TCP throughput | ≥ 50 Mbps in QEMU user-mode |
| Loopback RTT | < 10 ms |
| Max simultaneous sockets | ≥ 16 |

---

## Files

| File | Purpose |
|------|---------|
| `kernel/src/task/drivers/virtio_net.rs` | VirtIO NIC driver (DMA, IRQ) |
| `cells/services/net/src/lib.rs` | Cell entry point + IPC receive loop |
| `cells/services/net/src/interface.rs` | smoltcp Device adapter (RX queue, TX via IPC) |
| `cells/services/net/src/socket_table.rs` | CapId → smoltcp SocketHandle mapping |
| `cells/services/net/src/dhcp.rs` | DHCPv4 boot client |
| `cells/services/net/src/poll_driver.rs` | IPC opcode constants + message decoder |
| `tests/integration/network_loopback.rs` | QEMU-driven DHCP + TCP tests |
