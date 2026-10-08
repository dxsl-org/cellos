# Design gate: HTTPD concurrency needs shared public ABI evolution

Status: **first design approval granted by user in this conversation** (`Law 1` question: “Duyệt thiết kế ABI”). Second exact-diff/evidence approval is pending; no approval to promote.

## Current API evidence

- `libs/api/src/services/ipc.rs:323-400`: NetRequest/NetResponse postcard enums; TCP accept returns opaque Err(0xFE) on not-ready; recv empty/send zero ambiguous; no socket readiness contract. Must append new variants, never insert/reinterpret old discriminants.
- `libs/api/src/abi/completion.rs:47-63`: only NET_RX/TIMER sources; no RPC completion source; existing record is 24 bytes and per-cell capacity 32.
- `libs/ostd/src/ipc.rs:64-76,113-180`: service_call and service_call_bounded park a caller TID waiting for peer, and sender mask does not correlate multiple outstanding requests.
- `docs/code-standards.md:40-50`: two explicit accountable-maintainer Law 1 confirmations, design before editing and exact-delta/evidence after implementation.

## Requested design approval (first checkpoint)

1. Permit **append-only public IPC ABI evolution** for a bounded kernel-owned asynchronous request/reply operation: submit owned/copied request bytes to an attested target generation without blocking on rendezvous; reserve bounded completion storage before acceptance; retrieve completion correlated to the exact submission; classify Busy, pre-dispatch timeout, peer death, post-dispatch indeterminate, and late reply. Preserve legacy send/recv/try_send/recv masks and grant lifetime. Names, numeric syscall IDs, error layout and wire encoding are to be frozen in a reviewed exact delta before implementation, never taken from this text as an assigned ABI.
2. Permit **append-only NetRequest/NetResponse variants** for owner-generation-scoped batched TCP readiness and explicit not-ready/EOF/error results without changing the meaning/discriminants of existing variants; one event/interest batch can represent up to profile N sockets rather than reserving one of 32 completion slots per socket.
3. Permit minimal governance-scoped changes to public completion source only if the shared async mechanism requires them. Keep currently supported TIMER/NET_RX behavior and 24-byte record compatible; do not reinterpret reserved fields without exact-delta review.
4. Scope is httpd/net/VFS/AI usage of this shared mechanism for one-cell concurrent request handling. This is **not** authorization for arbitrary remote RPC semantics, HTTP/2 or a separate generic reactor API without demonstrated need.

## Review conditions before an ABI edit

- Produce exact enumerated IDs, struct layouts, wire encodings and exhaustive callsite/reference audit; publish as an exact delta under this plan, then ensure the first design approval covers any deviation. Two approvals cannot be simulated by automation or this plan document.
- Show bounded memory equation for 256 in-flight sockets; no per-socket completion slot unless capacity separately qualified.
- Prove owner death, restart generation, lost wakeup, no dropped accepted completion and cancellation/grant lifetime before rollout.
- Second explicit accountable-maintainer approval is required after reviewing implemented exact ABI diff and evidence; no promotion before it.

## Proposed exact additive surface for implementation review

- `ViSyscall` appended IDs 256–261 respectively `IpcSubmit`, `IpcTake`, `IpcWait`, `IpcCancel`, `IpcCurrent`, `IpcReply`. `IpcSubmit(target_tid, bytes_ptr, bytes_len, reserved=0)` returns positive operation ID or explicit Busy/dead/error; kernel owns submitted bytes. `IpcTake(op_id, out_ptr, out_len, status_ptr)` returns 0 pending or 1 terminal; a successful take writes version-1 16-byte LE status: version u32, kind u32, reply_len u32, reserved u32. Kind 0=reply, 1=peer death, 2=pre-dispatch timeout, 3=post-dispatch indeterminate, 4=cancelled. `IpcWait(timeout_ticks)` waits for any retained terminal; `IpcCancel(op_id)` must not free in-flight grant data. `IpcCurrent()` obtains authenticated deferred reply token on service side; `IpcReply(op_id, response_ptr, response_len)` delivers a late reply to the exact caller/generation. Actual register encodings and allowlist bit must be frozen after full implementation diff; no existing syscall IDs or meanings change.
- Append `NetRequest` variant indices 16–19: `TcpReady{interests:&[u8],cursor:u16,wait:bool}`, `TcpRecvReady{cap_id:u32,buf_len:u32}`, `TcpSendReady{cap_id:u32,data:&[u8]}`, `TcpCloseGraceful{cap_id:u32}`. Append `NetResponse` indices 6–9: `TcpReady{events:&[u8],next_cursor:u16}`, `NotReady`, `Eof`, `WriteProgress(u32)`. Readiness interests/events are packed **5-byte records** (`u32 cap_id` little-endian + `u8 flags`), with flags ACCEPT=1 READ=2 WRITE=4 EOF=8 ERROR=16; max 256 interest records =1280 bytes plus envelope, event response paginated. Structured borrowed slices are not deserializeable with the existing no_alloc borrowed postcard client, hence packed bytes; malformed byte length/mask must be rejected.
- One in-flight retained readiness request per owner/generation, at most 64 outstanding IPC operations per caller; 256 idle sockets share one wait registration, not 256 completion slots. Existing 24-byte completion record and NET_RX/TIMER source semantics stay unchanged.
- These signatures come from the implementing agents' proposals and are **provisional** pending source-level ID/reference audit; any changed numeric layout or behavior must be documented here before review. Approval above covered design scope, not assertion that this provisional delta is already valid.

## Non-ABI work already reachable

HTTP body framing in httpd was independently corrected without changing libs/api/types; baseline QEMU responded 200 to Content-Length 100 with one body byte, rebuilt QEMU responded 408 instead. This fixes one prerequisite only and is **not** concurrent HTTP service.
