# Hypha — Architecture

> All API anchors below are verified against the live source tree (file:line). No invented
> primitives — every mechanism Hypha relies on exists today or is logged in [os-gaps.md](./os-gaps.md).

## 0. The design inversion (vs a Unix-style agent)

A conventional AI agent is **a process orchestrating other processes** (`fork`/`exec` shell,
git, browser). Its danger is *ambient authority*: it can touch any file, run any binary.
Mitigations (SSRF filters, secret blocklists) are best-effort and in-process.

Hypha inverts this:

> **Hypha is a Cell that orchestrates other Cells via IPC + `sys_spawn_from_path`. Each "tool"
> is a separate Cell whose manifest declares exactly its capability — kernel-enforced at spawn
> ([kernel/src/loader.rs:145-160](../../kernel/src/loader.rs#L145-L160)) and per-syscall.**

`hypha` core holds only `network + spawn`. It *cannot* touch GPIO or arbitrary disk — it must
delegate to a tool Cell that alone holds that capability. This is LBI least-privilege, enforced
by compiler + kernel, not patched in userspace.

## 1. Cell topology

```
                    init / supervisor  (notify_on_exit → respawn → re-register)
                              │ sys_spawn_from_path (12) · sys_notify_on_exit (204)
        ┌─────────────────────┼──────────────────────────────────────┐
        ▼                     ▼                                        ▼
   UI / shell  ◄── IPC ──►  hypha core  ◄── TLS 1.3 (Grant) ──►  llm-gateway ──► net svc
  (UART | ViUI)  user/reply  - agentic loop                      (API key +     (smoltcp +
                             - conversation                       network cap;    mbedTLS)
                             - tool dispatch                       swappable→NPU)
                             manifest: network+spawn
                              │ sys_lookup_service (206) + sys_send/recv (postcard)
        ┌──────────┬──────────┼───────────┬──────────────────┐
        ▼          ▼          ▼           ▼                  ▼
    tool-fs    tool-sys   tool-spawn  tool-peripheral     tool-net
   part_data    (none)      spawn      gpio+uart          network
   →only /data  →read-only  →only spawn →ONLY Cell that    →only outbound
                            Cells       can touch GPIO      HTTP
```

**Why `llm-gateway` is a separate Cell**, not a module inside core:
1. Isolates the **API key** + `network` capability into one Cell; core never sees the secret.
2. **Swappable backend** — today remote Anthropic over TLS; on G3, local NPU via Tier 1b FFI —
   core is unchanged.
3. It absorbs the dirty work: hand-rolled HTTP/1.1 + SSE parsing + chunking large prompts.

## 2. Agentic loop → ViCell primitives

Runs async, single-cell, via `block_on` + `yield_now`
([libs/ostd/src/executor.rs:9](../../libs/ostd/src/executor.rs#L9)).

```
on AppEvent::Message{data}      ← user input (UART/UI)            [libs/ostd/src/app.rs:70]
  conversation.push(user, data)
  loop:                                                          ← tool-use sub-loop
    prompt = serialize(conversation + TOOL_DEFS)                 ← tens of KB → Grant
    reply  = llm_complete(prompt)         IPC → llm-gateway      ← sys_grant_alloc/share/slice
    match reply:
      Text(t)       → send t to UI; break
      ToolCalls(cs) → for c in cs:
                        tid    = lookup_service(tool_id(c))      ← (206) or spawn (12)
                        result = send+recv typed(tid, c)         ← postcard sys_send/sys_recv
                        conversation.push(tool_result, result)
                      # loop again: LLM sees results, reasons on
on AppEvent::ShutdownWith → sys_exit(0)
```

| Agent concept | ViCell mechanism | Anchor |
|---|---|---|
| LLM call | IPC → llm-gateway; large payload via **Grant** | IPC buf 4096B [api/src/ipc.rs:21]; grant [libs/ostd/src/syscall.rs:957] |
| HTTPS | hand-roll HTTP on `tls_connect/write/read` | [libs/ostd/src/tls.rs:31]; [cells/demos/https-demo/src/main.rs](../../cells/demos/https-demo/src/main.rs) |
| Find tool | `sys_lookup_service` (dynamic, no hardcoded tid) | [api/src/syscall.rs:506-517] |
| Call tool | typed postcard `sys_send`/`sys_recv` | loop [cells/services/vfs/src/main.rs:94-123] |
| Run one-shot tool | `sys_spawn_from_path("/bin/tool-x")` | [libs/ostd/src/syscall.rs:245]; loader [kernel/src/loader.rs:37] |
| State / memory | files in `/data` | `VfsClient` [libs/ostd/src/clients/vfs.rs:13] |
| Owned buffers (Law 2) | prompt/response `Vec<u8>`/`Box<[u8]>` | `AppEvent.data: Vec<u8>` [libs/ostd/src/app.rs:74] |
| Time / random | `sys_get_wall_secs` / `sys_get_random` | [libs/ostd/src/syscall.rs:950,1060] |
| Never-die | supervisor `notify_on_exit` + `arm_heartbeat` | [libs/ostd/src/app.rs:214]; [kernel/src/task/syscall.rs:1180] |

## 3. IPC protocols (the heart)

Shared crate (e.g. `libs/agent-proto`), postcard-encoded like the rest of the system.

```rust
// core ↔ tool Cells
enum AgentToolRequest<'a>  { Invoke { name: &'a str, args_json: &'a str } }
enum AgentToolResponse     { Ok { result_json: String }, Err { message: String } }

// core ↔ llm-gateway  (large prompt/response referenced via Grant, not copied)
enum LlmRequest  { Complete { grant_id: usize, len: u32 } }
enum LlmReply    { Text(String), ToolCalls(Vec<ToolCall>) }
```

Each tool publishes its own JSON schema (name + description) which core folds into the prompt —
standard tool-calling, except a "tool" is a capability-bearing Cell, not an in-process function.

## 4. Capability model — the sales pitch

| Cell | manifest flags | Can touch | Cannot touch |
|---|---|---|---|
| `hypha` core | `network, spawn` | LLM, spawn tools | GPIO, /data, MMIO |
| `llm-gateway` | `network` | net + API key | spawn, fs, GPIO |
| `tool-fs` | `part_data` | only `/data` | net, GPIO, spawn |
| `tool-peripheral` | `gpio, uart` | **only Cell** touching GPIO/I2C | net, /data, spawn |
| `tool-sys` | (none) | read-only state | every side-effect |

**Story**: even if the LLM is prompt-injected to "wipe /data", core *has no such capability*;
it must go through `tool-fs`, which only sees `/data` and nothing else; GPIO is forever
unreachable to core. Every boundary is kernel-enforced. Manifest format:
[api/src/manifest.rs:22-51](../../api/src/manifest.rs#L22-L51).

## 5. Red-team (known pain — see os-gaps for fills)

| Risk | Sev | Handling |
|---|---|---|
| Is any LLM reachable? DNS = static QEMU table; public-net-over-NAT unverified | 🔴 | host LLM proxy via 10.0.2.2; pin IP; `dns_lookup` exists [net.rs:84] |
| No HTTP library | 🟡 | gateway hand-rolls HTTP/1.1 + SSE (os-gap G1) |
| IPC 4096B, messages copied | 🟡 | Grant for prompt/response from day one |
| Memory quota + growing history | 🟡 | full history → `/data`, sliding window in heap, summarize old turns |
| No real threads | 🟢 | v1 sequential tools; worker Cells later |
| API key is a secret | 🟢 | in `/data` or `/etc`, only llm-gateway reads — capability isolation |
