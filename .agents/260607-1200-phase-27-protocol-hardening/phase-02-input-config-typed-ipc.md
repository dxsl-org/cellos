# Phase 02: Input + Config Typed IPC Migration

**Status**: 📋 Planned  
**Priority**: P2 (independent of Phase 01)  
**Effort**: ~1 day  
**Stage**: G1

---

## Overview

The input and config services use hand-rolled raw byte protocols.  Phase 27 added
`InputRequest`/`InputResponse` and `ConfigRequest`/`ConfigResponse` to `libs/api/src/ipc.rs`,
but neither service was migrated.  This phase completes the migration.

**Input service caveat** — the kernel-to-input EV_KEY path (byte[0]=0, bytes 1-8 = scancode +
value) is a raw kernel push, not a typed IPC call from a consumer cell.  That path STAYS raw.
Only the focus-management path (opcode 0x20 `OP_SET_FOCUS`) migrates to `InputRequest`.

---

## Requirements

### Config service
1. Replace the raw byte dispatch (`buf[0] == 1` / `buf[0] == 2`) with
   `api::ipc::decode::<ConfigRequest>(&buf)`.
2. Responses encoded with `api::ipc::encode(&ConfigResponse, &mut resp)`.
3. Current response contract for `Get` returns a (ptr, len) pair pointing into the service's
   heap — this leaks SAS internal addresses to callers.  Replace with `ConfigResponse::Value`
   carrying the value bytes inline (max 256 bytes).
4. Add `declare_manifest!` and `declare_syscalls![Send, Recv, TryRecv, Log, Heartbeat,
   LookupService, StateStash, StateRestore]` to config service main.rs.
5. Update shell `config_client.rs` to encode with `api::ipc::encode::<ConfigRequest>`.

### Input service
1. The `OP_SET_FOCUS` (0x20) branch in `handle_message()` migrates to:
   `api::ipc::decode::<InputRequest>(&buf)`.
2. Replies encoded with `api::ipc::encode(&InputResponse, &mut resp)`.
3. Add `declare_manifest!` and `declare_syscalls![Send, Recv, Log, Heartbeat]` to input
   service main.rs.

---

## Key Insights

- Config's current `Get` response sends raw SAS pointers (`ptr as u64`, `len as u64`) to
  consumers — callers dereference the pointer directly.  This is SAS-valid but exposes heap
  addresses.  After migration, `ConfigResponse::Value(&str)` carries the bytes inline; the
  shell's `config_client.rs` must be updated to read from the `&str` instead of casting a
  pointer.
- The input service `handle_message()` handles two kinds of inbound messages: kernel EV_KEY
  events (raw) and compositor/shell focus requests (currently opcode 0x20).  Only the second
  kind migrates.  Distinguishing them: kernel events always have `buf[0] == 0` (EV_KEY);
  focus requests can be detected by trying to decode as `InputRequest` after ruling out the
  raw EV_KEY path.
- Config service uses `Mutex<ConfigStore>` from `ostd::prelude`.  The main recv loop is
  single-threaded — no concurrency concern.

---

## Related Code Files

**Modify:**
- `cells/services/config/src/main.rs` — replace raw protocol with typed IPC
- `cells/services/input/src/main.rs` — migrate `OP_SET_FOCUS` path
- `cells/apps/shell/src/config_client.rs` — update to encode `ConfigRequest`

---

## Implementation Steps

### Config service

1. Add `api::declare_manifest!(block_io=false, network=false, spawn=false)` and
   `api::declare_syscalls![Send, Recv, TryRecv, Log, Heartbeat, LookupService,
   StateStash, StateRestore]` at module scope.
2. In `main()` recv loop, replace the `buf[0] == 1 / 2` match with:
   ```rust
   match api::ipc::decode::<api::ipc::ConfigRequest>(&buf) {
       Ok(ConfigRequest::Get(key)) => { … encode ConfigResponse::Value(val) … }
       Ok(ConfigRequest::Set { key, value }) => { … encode ConfigResponse::Ok … }
       Ok(ConfigRequest::Delete(key)) => { … }
       Ok(ConfigRequest::List) => { … }
       Err(_) => { api::ipc::encode(&ConfigResponse::Err(0xFF), &mut resp); }
   }
   ```
3. Remove the raw ptr/len response (the `resp[0..8].copy_from_slice(ptr)` block).
   The value is now encoded inline in `ConfigResponse::Value`.
4. `cargo check` config service.

### Shell config_client.rs

5. Locate the `config_client.rs` usage — find how it currently sends Get/Set messages and
   reads the ptr/len reply.
6. Replace with `api::ipc::encode(&ConfigRequest::Get(key), &mut buf)` and
   `api::ipc::decode::<ConfigResponse>(&resp_buf)`.

### Input service

7. In `handle_message()`, keep the `EV_KEY` path (raw bytes, `buf[0] == 0`).
8. For non-EV_KEY messages, try `api::ipc::decode::<api::ipc::InputRequest>(&buf)`:
   - `InputRequest::SetFocus { cell_tid }` → update `dispatcher.focused_tid`.
   - `InputRequest::GetFocus` → send back `InputResponse::Focus(tid)`.
   - `InputRequest::ClearFocus { cell_tid }` → clear if it matches.
   - Decode error → ignore (drop silently; input is not a critical RPC path).
9. Add `declare_manifest!` and `declare_syscalls!` at module scope.
10. `cargo check` input service.

---

## Todo List

- [ ] Config: replace raw byte protocol with typed IPC
- [ ] Config: remove SAS pointer leakage in `Get` response
- [ ] Config: add `declare_manifest!` + `declare_syscalls!`
- [ ] Shell `config_client.rs`: update to use typed encoding
- [ ] Input: migrate `OP_SET_FOCUS` to `InputRequest::SetFocus`
- [ ] Input: add `declare_manifest!` + `declare_syscalls!`
- [ ] `cargo check` all modified cells

---

## Success Criteria

- [ ] `buf[0] == 1 / 2` raw dispatch removed from config service.
- [ ] Raw `ptr/len` response replaced with `ConfigResponse::Value`.
- [ ] `OP_SET_FOCUS` constant removed from input service; `InputRequest` used instead.
- [ ] Shell `config_client.rs` uses `api::ipc::encode` + `decode`.
- [ ] `cargo check` clean on all affected crates.

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Config Get value lifetime — `&str` in response borrows from config store heap | Medium | Service must encode the value into a stack buffer before replying; borrow does not escape the response encoding call |
| Shell startup breaks if config_client changes break `PATH`/`OS` reads | Medium | Test shell boot after change |
| Input kernel EV_KEY path accidentally migrated | Low | Keep `if buf[0] == EV_KEY { … } else { decode::<InputRequest>(…) }` structure |
