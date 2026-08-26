# Phase 00 — Remote-Call API Contract (Design Doc)

> **Status:** Draft — cần user approval trước khi implement P01.
> **Scope:** Wire format, dispatch protocol, error semantics, ostd API surface.
> **No code trong phase này** — chỉ spec.

---

## Vấn đề cần giải quyết

L.0+L.1 đã build `RemoteServiceProxy` nhưng API thực tế là raw TID:
```rust
// libs/ostd/src/cluster.rs — hiện tại
pub fn lookup_remote(service_id: u16) -> Option<usize>  // trả về remote TID, thô
```

Không có typed call, không có codec, không có error model. Cần thiết kế lớp này trước khi build transport internet.

**Constraint:** Không serialize Rust closures — impossible over network. API phải là typed request/response với explicit codec.

---

## Wire Format

Tất cả remote calls đi qua Noise KKpsk0 session (đã encrypted + authenticated). Đây là framing bên trong Noise transport records.

### Request Frame

```
Offset  Size  Field
0       4     magic = b"RPCQ"
4       1     version = 1
5       4     request_id (u32 LE, random từ BrokerRng — match với response)
9       2     service_id (u16 LE)
11      2     method_id (u16 LE)
13      4     timeout_ms (u32 LE)
17      2     payload_len (u16 LE)
19      N     payload (postcard-encoded args, max ~4000B)
```

**Total overhead:** 19 bytes + payload. IPC_BUF_SIZE = 4096B → max payload ≈ 4077B.

### Response Frame

```
Offset  Size  Field
0       4     magic = b"RPCR"
4       1     version = 1
5       4     request_id (u32 LE — phải match request)
9       1     status (xem bảng)
10      2     payload_len (u16 LE)
12      N     payload (postcard-encoded return value hoặc error string)
```

### Status Codes

| Code | Meaning |
|---|---|
| 0x00 | OK — payload là return value |
| 0x01 | TIMEOUT — timeout_ms exceeded trước khi service trả lời |
| 0x02 | NO_SERVICE — service_id không registered trên remote |
| 0x03 | NO_METHOD — method_id không recognized bởi service |
| 0x04 | SERVICE_ERROR — service trả lời với error, payload là error string |
| 0x05 | BROKER_ERROR — broker không thể forward (peer disconnected, etc.) |

---

## Service Dispatch Protocol

### Registration (local service → local broker)

Khi một service Cell muốn expose methods cho remote peers:

```
Service Cell → net-broker (via sys_send):
  RegisterRemoteService { service_id: u16, my_tid: TaskId }

net-broker → Service Cell:
  RegisterRemoteServiceAck { ok: bool }
```

Broker duy trì table: `service_id → local_tid`. Chỉ registered services nhận được remote calls.

### Incoming call flow

```
Peer A                    net-broker B                 Service Cell B
  │                           │                              │
  │── Noise record ──────────►│                              │
  │   (RemoteRequest frame)   │                              │
  │                           │─ sys_send(service_id) ──────►│
  │                           │  (forward payload)           │
  │                           │◄─ sys_send(response) ────────│
  │◄── Noise record ──────────│                              │
  │   (RemoteResponse frame)  │                              │
```

**Timeout enforcement:** Broker đặt timer `timeout_ms`. Nếu local service không trả lời trong window → broker tự trả `status=TIMEOUT` cho peer A.

**IPC forward format** (broker → service, dùng existing IPC):
```rust
// Reuse AppEvent::Message { sender_tid, data } đã có
// data = raw request payload (không cần RPC header — broker đã strip)
// sender_tid = broker's tid (service reply về broker, không reply về peer)
```

---

## ostd API

### Trait definition

```rust
// libs/ostd/src/ (không phải libs/api — không cần Law 1)
pub trait RemoteMethod: Sized {
    type Args:   postcard::experimental::max_size::MaxSize + serde::Serialize;
    type Return: serde::de::DeserializeOwned;
    const SERVICE_ID: u16;
    const METHOD_ID:  u16;
}
```

### Caller side

```rust
// ctx.cluster() trả về ClusterRef
impl ClusterRef {
    /// Gọi một method trên remote peer. Blocking.
    pub fn call<M: RemoteMethod>(
        &self,
        peer_node_id: &CellNetId,     // from P01
        args: M::Args,
        timeout_ms: u32,
    ) -> ViResult<M::Return>;
}
```

**Example usage:**

```rust
// Khai báo remote method:
pub struct ReadTemperature;
impl RemoteMethod for ReadTemperature {
    type Args   = ();
    type Return = f32;
    const SERVICE_ID: u16 = 0x0010;  // SensorService
    const METHOD_ID:  u16 = 0x0001;  // read_temperature
}

// Gọi từ Cell A:
let temp: f32 = ctx.cluster()
    .call::<ReadTemperature>(&robot_b_node_id, (), 5000)?;
```

### Service side

```rust
// Service Cell đăng ký với broker và handle method calls:
impl AppHandler for SensorService {
    fn handle(&mut self, ctx: &mut AppContext, event: AppEvent) {
        match event {
            AppEvent::Message { sender_tid, data } => {
                // data = raw postcard-encoded RemoteMethod::Args
                // Broker đã decode method_id và forward payload
                let method_id = data[0]; // convention: first byte = method_id
                match method_id {
                    0x01 => {
                        let temp = self.read_sensor();
                        ctx.reply(sender_tid, postcard::to_slice(&temp, &mut buf)?);
                    }
                    _ => ctx.reply_error(sender_tid, "unknown method"),
                }
            }
            _ => {}
        }
    }
}
```

---

## Error Handling

```rust
#[derive(Debug)]
pub enum RemoteCallError {
    Timeout,
    NoService,
    NoMethod,
    ServiceError(heapless::String<128>),
    BrokerError,
    CodecError,      // postcard decode fail
    NoPeer,          // peer NodeId không có kết nối
}

pub type RemoteResult<T> = Result<T, RemoteCallError>;
```

**Developer experience:** Caller nhận `RemoteResult<T>`, không cần handle raw bytes. Broker + ostd layer absorb framing complexity.

---

## Constraints

1. **Max payload 4000B** per request/response (IPC_BUF_SIZE limit).
   → Large data: dùng Grant API (sys_grant_pages) riêng, không qua remote call.

2. **Blocking call only** (G1). Async wrapper cho G2.
   → Caller blocks đến khi response hoặc timeout.

3. **No streaming** (G1). One request → one response.

4. **No callback** — caller phải poll hoặc dùng timeout. Service không push.

5. **service_id namespace** (append-only như syscall IDs):
   - 0x0000–0x000F: reserved (system services)
   - 0x0010–0x00FF: robot/sensor services
   - 0x0100–0xFFFF: application services

---

## Non-goals (explicitly excluded)

- Location-transparent IPC (caller luôn biết call là remote)
- Closure serialization (không thể, không cần)
- Service discovery (caller biết service_id trước)
- Multi-return / streaming responses
- Async call / futures (G2)

---

## Decisions từ Research (8 SAS/LBI OS + distributed systems)

### Q1: Method dispatch — 8-byte FNV-1a-64 service key + postcard discriminant

**Kết luận:** Không dùng `method_id` explicit riêng. Thay vào đó:
- **Service key (8B):** `FNV-1a-64(service_name_string)` — computed at compile time. Đây là **cùng pattern** với `ClusterId::from_name` đã có trong `libs/api/src/cluster.rs`. Broker dùng để route request tới đúng local service Cell.
- **Method ID:** đã nằm sẵn trong **postcard discriminant** của request enum — đây là pattern hiện tại của `VfsRequest`, `NetRequest`. Không thêm field mới.

```
Frame layout (bên trong Noise session):
  service_key(8) || postcard_payload(N)
                    ↑ postcard discriminant = method id (đã có)
```

**Tại sao:** Singularity dùng compile-time state machine, FIDL dùng SHA-256 ordinal, postcard-rpc dùng FNV hash key. Tất cả đều compile-time integer, không string trên wire. FNV-1a-64 đã có trong Cellos, overhead 0. Nhỏ nhất có thể.

**Đây là delta DUY NHẤT so với L.0+L.1:** broker hiện tại dùng service_id (u16) để lookup TID. Chỉ cần mở rộng routing table sang cross-machine với service_key thay vì thêm method_id field.

---

### Q2: Service registration — compile-time static, dùng sys_lookup_service hiện tại

**Kếtluận:** Không cần syscall mới, không cần runtime broker. Dùng **pattern hiện tại**:

```rust
// libs/api/src/syscall.rs — đã có
pub mod service {
    pub const VFS:        u16 = 1;
    pub const NET:        u16 = 2;
    pub const NET_BROKER: u16 = 8;
    // Thêm:
    pub const SENSOR:     u16 = 0x0010;  // append-only
    pub const ACTUATOR:   u16 = 0x0011;
}
// sys_lookup_service(SERVICE::SENSOR) → TaskId
// Broker dùng service_key → lookup local TID → forward
```

**Tại sao:** Fuchsia dùng manifest + directory (như Plan 9). Barrelfish dùng nameservice Cell runtime. Cellos đã có `sys_register_service` + `sys_lookup_service` — đây là đúng approach, không cần reinvent. G1 = fixed topology (2 robots) → static compile-time là đủ và YAGNI.

G2 nếu cần dynamic: VFS `/svc/sensor`, `/svc/actuator` path lookup — Fuchsia-inspired, dùng VFS Cell đã có.

---

### Q3: In-flight concurrency — 1 per peer (G1), seq_no cho G2

**Kết luận:** G1 = **strictly 1 in-flight** per Noise session. Giống seL4 rendezvous.

**Tại sao:**
- seL4: 1 in-flight per endpoint → provable correctness, zero complexity
- Erlang `gen_server:call`: blocking, timeout, 1-at-a-time (server side) — tuy nhiên dùng `ref` correlation tag cho N concurrent callers
- postcard-rpc: `u32 seq_no` cho N in-flight — nhưng chỉ cần khi có multiple concurrent callers

Với G1 blocking API (`ctx.cluster().call(...)` blocks cho tới khi có response), 1 in-flight là đủ. Nếu G2 cần async concurrent calls, thêm `u32 seq_no` vào frame header — đây là Erlang's `ref` pattern.

**Không thêm seq_no vào G1 wire format** — YAGNI, và thêm sau là backward compatible (extend header version byte).

---

## Final Wire Format (updated)

### Request Frame (revised)

```
Offset  Size  Field
0       8     service_key (u64 LE = FNV-1a-64(service_name), compile-time const)
8       1     version = 1
9       4     request_id (u32 LE, random — cho timeout matching, KHÔNG phải seq_no)
13      4     timeout_ms (u32 LE)
17      2     payload_len (u16 LE)
19      N     payload (postcard-encoded Request enum, discriminant = method_id)
```

**Thay đổi từ draft:** thay `service_id(2) + method_id(2)` bằng `service_key(8)`. Method đã trong postcard discriminant.

### Response Frame (unchanged cơ bản)

```
Offset  Size  Field
0       8     service_key (echo — routing reference)
8       1     version = 1
9       4     request_id (u32 LE — match với request)
13      1     status
14      2     payload_len (u16 LE)
16      N     payload (postcard-encoded Response enum hoặc error string)
```

---

## Approval Gate

Trước khi implement P01–P03, user phải approve design này.

**3 decisions đã được chốt bởi research:**
1. ✅ Service key = FNV-1a-64, compile-time, cùng pattern ClusterId (Q1)
2. ✅ Registration = static sys_register/lookup_service hiện tại (Q2)
3. ✅ 1 in-flight G1, seq_no G2 nếu cần (Q3)

**Sau khi approve:**
- P01 implement `CellNetId`, `PeerTicket`, NodeId→Noise binding
- P02/P03 implement STUN + relay transport
- `RemoteMethod` trait + service_key dispatch implement sau khi transport stable
