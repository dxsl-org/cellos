# OS Gaps Register — surfaced by building Hypha

> **The core workflow.** Hypha is a real app, so building it exercises `ostd`/kernel paths that
> demos never touched. Each gap below is a missing OS module Hypha needs. We fill them
> **incrementally**: when a phase hits a gap, we either (a) work around it minimally to keep the
> phase moving, or (b) promote it to a proper OS module with its own plan.
>
> This register is the bridge between "app I want" and "OS work to do". Keep it current.

## Legend

- **Status**: 🔴 blocking · 🟡 needed soon · 🟢 nice-to-have · ✅ filled
- **Disposition**: `workaround` (hack to proceed) · `module` (promote to real OS feature) · `tbd`
- **Surfaced by**: which phase exposed it

## Register

| # | Gap | Surfaced | Sev | Disposition | Notes |
|---|-----|----------|-----|-------------|-------|
| G1 | **HTTP/1.1 client** — no HTTP lib exists; only raw TLS read/write ([tls.rs:31](../../libs/ostd/src/tls.rs#L31)). Hypha hand-rolls request building + response parsing. | P0 | 🔴 | module → `ostd::http` (minimal) | Start hand-rolled in llm-gateway; extract to `ostd` once stable. |
| G2 | **SSE / chunked streaming parse** — LLM responses stream (`text/event-stream` or chunked transfer). No parser today. | P0 | 🟡 | module | Needed for token streaming; v0 can use non-stream JSON response. |
| G3 | **Public-internet DNS over QEMU NAT** — `dns_lookup` exists ([net.rs:84](../../libs/ostd/src/clients/net.rs#L84)) but static QEMU table; real external resolution unverified. | P0 | ✅ | workaround confirmed — pinned `10.0.2.2` (QEMU user-net gateway); guest→host TCP round-trip verified (boot run #2). Public DNS still unvalidated — revisit when needed. | |
| G4 | **no_std JSON** — agent speaks JSON (LLM API bodies, tool args). Need a no_std serde-json. | P0 | 🔴 | module (add dep: `serde-json-core` or similar) | Confirm it fits SAS/`forbid(unsafe)` constraints. |
| G5 | **Streaming/large IPC ergonomics** — Grant works to 16 MiB but token-by-token streaming from gateway → core has no pattern. | P0/P1 | 🟡 | tbd | v1: buffer full reply in a Grant, then deliver. Streaming later. |
| G6 | **Secret storage** — API key lives in `/data` or `/etc`; no "secret" abstraction or access policy beyond capability isolation. | P0 | 🟢 | workaround (plain file, gateway-only read) | Good enough via LBI; revisit if multi-secret. |
| G7 | **Name-based / dynamic service discovery** — registry IDs are fixed well-knowns (VFS=1..COMPOSITOR=5, [api/src/syscall.rs:506](../../api/src/syscall.rs#L506)). App-defined tools need dynamic IDs or name lookup. | P2/P3 | 🟡 | module (extend registry: app-service ID range or name map) | Blocks scaling tools beyond a hardcoded set. |
| G8 | **Tokenizer / context accounting** — no token counting; can't size the context window or trim safely. | P5 | 🟡 | tbd | May approximate by bytes initially. |
| G9 | **Conversation/state store** — JSON files vs embedded DB. Haily used SQLite (Tier 1b). | P5 | 🟢 | tbd (JSON files v1; SQLite via Tier 1b later) | Start with append-only JSONL in `/data`. |
| G10 | **On-screen text input (ViUI)** — full on-screen keyboard is a G2 feature; G1 input is UART only. | P6 | 🟢 | defer (CLI/UART for now) | Only if GUI chat (P6) is pursued. |
| G11 | **Parallel tool execution** — real concurrency needs worker Cells; v1 is sequential. | later | 🟢 | tbd | Spawn-per-tool fan-out via `sys_spawn`. |
| G12 | **HTTP client for `tool-net`** (web fetch) — same as G1 but for arbitrary URLs, with redirect/size limits. | P3 | 🟢 | reuse G1 module | Depends on G1. |
| G13 | **Service registration needs SpawnCap** — only `init` can `register_service`, so app cells can't self-register; clients can't `lookup_service` an app service. | P1 | 🟡 | workaround (spawn-by-tid) → module | P1: `core` spawns gateway, talks by tid. Shared services need `init` to spawn+register — ties into G7. |
| G16 | **`NetClient.tcp_send` mis-handled the `Data(n)` reply** — handler correctly replies `Data(n)` (count); `nc`/`curl`/`wget`/`mqtt` use that via raw requests. But `NetClient.tcp_send` only accepted `R::Ok` → always `Err`. | P1 | ✅ fixed (NetClient) | Fixed in `clients/net.rs`: interpret `Data(n)` — `Ok` if `n==len`, else `WouldBlock`. Gateway retries with yields until the socket is Established. Handler left unchanged (curl/nc depend on `Data(n)`). |
| G17 | **net cell faults during embedded-tls handshake** — TLS connect to a real server (mock OpenSSL 3.2) → `scause=13` load page fault `stval=0x101000` in the net cell; supervisor restarts it (never-die ✓). Likely a bad-pointer bug in the TLS transport (`set_tls_context` raw ptrs) or embedded-tls 0.19 ↔ OpenSSL incompat. | P0/P1 | 🔴 needs investigation | Hypha avoids TLS for now (plaintext). To debug: addr2line `sepc=0x80212082` against the kernel ELF; try `num_tickets=0` / restricted TLS on the mock. Blocks the TLS path (and G14 hardening). |
| G15 | **Shell has no foreground wait / job control** — `spawn_external` fire-and-forgets ([executor.rs:951](../../cells/tools/shell/src/executor.rs#L951)); shell loops back to read stdin and races interactive child cells (e.g. `hypha`) for UART keystrokes. | P1 | 🔴 | ✅ filled (shell `sys_wait` on fg child) | Surfaced by the first boot run. Fast commands unaffected (Wait short-circuits on Terminated). Real job control (fg/bg) still absent. |
| G14 | **TLS does not authenticate the server** — net cell uses embedded-tls `UnsecureProvider` ([net/src/tls/socket.rs:56](../../cells/services/net/src/tls/socket.rs#L56)); cert chain is **not** verified (roadmap "validates cert chain" is inaccurate). Encrypted but MITM-able. | P0 | 🟡 | module (CA trust store / pinning) | Convenient now (mock proxy self-signed cert works); real security gap before any production/public endpoint. Ties to KMS/attestation (roadmap §G). |
| G19 | **Capability spawn-chain for hardware tools** — spawn grants `requested ∩ spawner_caps` ([loader.rs:251](../../kernel/src/loader.rs#L251)). `core` (network+spawn, no gpio) CANNOT spawn a `gpio` tool — the cap is stripped. Giving core gpio would defeat the least-privilege showcase. | P4 | 🟡 | workaround (`init` spawns `tool-peripheral` as Root + registers `service::HYPHA_PERIPHERAL=13`; core looks it up) → ties into G7/G13 | Structural for ALL hardware tools; a proper app-service registration API removes the well-known-id add. |
| G20 | **No MMIO release** — `MmioRegion` has no `Drop` and there is no `sys_release_mmio` ([mmio.rs:44](../../libs/ostd/src/mmio.rs#L44)); a region frees only on cell death (`release_for`, [resource_registry.rs:187](../../kernel/src/resource_registry.rs#L187)). A long-lived `tool-peripheral` holding PL061 blocks run-once demo cells (robot/periph/pwm) from GPIO. | P4 | 🟡 | workaround (lazy open + bounded `AlreadyExists` retry; showcase omits standalone demos) → module (release syscall) | Needed once >1 long-lived MMIO owner must coexist on one bus. |
| G21 | **Hypha stack not on ARM disk** — `format-disk-arm.ps1` builds only system + peripheral-demo cells; the Hypha cells were RISC-V-only. No ARM64 Hypha integration test existed. P4 targets ARM virt (PL061 GPIO). | P4 | 🟡 | module (add all 6 hypha cells to `format-disk-arm.ps1` for `aarch64-unknown-none-softfloat` + `hypha-p4-boot` ARM test) | ARM64 is now a first-class Hypha build target. |

## How a gap gets filled

1. Phase hits the gap → log it here (or update its row).
2. Decide disposition: `workaround` to keep the phase shipping, or `module` to do it properly.
3. If `module`: spin a dedicated plan (its own `.agents/<date>-<slug>/`), implement, link back here,
   mark ✅, and note the OS module that resulted.
4. Reflect material OS additions into `docs/project-roadmap.md` / `docs/project-changelog.md`
   per the documentation protocol.

## Progress log

### P0 (2026-06-21) — llm-gateway scaffold builds for riscv64
- **G1 (HTTP)** → `workaround` shipped: hand-rolled HTTP/1.0 POST in
  `cells/apps/hypha/llm-gateway/src/http.rs` (`build_post`/`build_chat_body`). Promote to
  `ostd::http` when `core` (P1) becomes a second consumer.
- **G3 (DNS/NAT)** → `workaround` chosen: pinned `PROXY_IP = 10.0.2.2` (QEMU user-net gateway =
  host) + a host-side OpenAI-compatible TLS proxy. Public-internet egress still UNVERIFIED — the
  live round-trip needs the host proxy running. Remains 🔴 until a boot run confirms it.
- **G4 (JSON)** → `workaround` shipped: hand-rolled `json_escape` + `extract_content` (single
  `"content"` key, with unescape). No no_std JSON dep added yet — revisit (serde-json-core) when
  responses get nested/streamed.
- **G5 (large IPC)** → deferred: P0 gateway is **standalone** (hardcoded prompt), no IPC/Grant yet.
  `LlmRequest::Complete{prompt}` carries the prompt inline; Grant arrives when prompts grow (P1+).

> Note: TLS-via-net-service needs **no** network capability in the gateway (manifest
> `network=false`) — it is plain IPC to the `net` cell, which holds `NetworkCap`. The strong
> kernel-gated capability story applies to `tool-fs`/`tool-peripheral` (part_data/gpio), not to
> network, which is service-mediated (net-cell-side policy = os-gap, roadmap §G.2).

### P1 (2026-06-21) — core chat loop + gateway-as-service build for riscv64
- **New gap G13 (service registration needs SpawnCap)**: `register_service` requires SpawnCap, which
  only `init` holds — an app cell cannot self-register. P1 sidesteps it: `core` *spawns* the gateway
  and talks by the returned tid (no registry). For shared services discoverable by *other* cells,
  `init` must spawn+register — ties into G7 (dynamic discovery). Disposition: `workaround` (spawn-by-tid).
- **G5 (large IPC)** reaffirmed: P1 prompt AND reply each capped to one ~4 KB IPC message
  (`core` trims transcript from the front; gateway truncates reply with a marker). Grant streaming
  is the real fix.
- **G4 (JSON)** unchanged: still hand-rolled in `http.rs`.
- **G3 (DNS/NAT)** unchanged: still 🔴 — needs the host proxy + a boot run to verify.

### Mock-proxy plumbing test (2026-06-21) — `tools/hypha-mock-llm/`
- Built a host-side **TLS 1.3 mock LLM** (`mock_proxy.py`): self-signed P-256 cert, echoes the
  prompt as OpenAI-compatible JSON. **Verified on the host** (python TLS-1.3 client → POST → 200 →
  echo JSON) — confirms the server shape the gateway expects.
- Surfaced **G14** (TLS unauthenticated — `UnsecureProvider`), which is *why* the self-signed cert
  works without a trust store.
- Still pending: a ViCell **boot run** with the proxy up to confirm guest→host (QEMU user-net) +
  the gateway's hand-rolled HTTP/JSON against this server. That closes the ViCell-side of G3.

### Boot run #1 (2026-06-21) — findings
- **TLS actually works**: the mock-proxy TLS handshake completed (proxy reached `handle_one_request`);
  the `10054` reset was just the user quitting QEMU, not a handshake failure.
- **No crash**: QEMU exited because the user quit (no reply shown), not a fault.
- **Root cause of "no reply" → G15** (shell didn't wait on the fg child → UART contention) — fixed.
- Added a **plaintext transport** to the gateway (`USE_TLS=false`, default; `transport.rs`) + a
  `--plain` mock mode (HTTP/8080) so the full plumbing can be verified without the TLS variable.
  Plain HTTP round-trip **verified host-side** (200 + echo). Gateway now logs each stage
  (`[gw] ... connected/sent/reading/response bytes`) for the next boot run.

### Boot run #2 (2026-06-22) — **full round-trip verified** ✅

Ran with `mock_proxy.py --plain` (HTTP/8080) on the host, QEMU user-net (SLIRP), RISC-V64.

```
ViCell > hypha
Hypha — ViCell's first AI agent (P1 chat). Type 'exit' to quit.
[hypha/llm-gateway] service ready

you> chao em
[gw] plaintext mode -> :8080
[gw] TCP connected; sending
[gw] sent; reading
[gw] response bytes: 359
hypha> Mock LLM here — the Cellos TLS+HTTP+JSON path works. You sent: user: chao em assistant:

you> exit
[hypha] bye
```

- **G3 (DNS/NAT) workaround CONFIRMED** — QEMU user-net NAT to `10.0.2.2:8080` works. Guest→host
  TCP plain HTTP round-trip proven end-to-end. Status → `workaround` (pinned IP).
- **G15 (shell fg wait) CONFIRMED fixed** — shell blocked on the foreground `hypha` child; no
  UART contention; output was clean.
- **G1 (hand-rolled HTTP/JSON) CONFIRMED working** — `build_post`/`build_chat_body`/`extract_content`
  correctly parse the mock response.
- **P0 + P1 goals met**: prompt → TCP → mock LLM → reply displayed → multi-turn context in heap →
  clean exit.

## Filled (✅)

| Gap | Resolution |
|-----|------------|
| G15 | Shell `sys_wait` on fg child — no UART race. Shipped, confirmed boot run #2. |
| G3 (workaround) | Host proxy at `10.0.2.2:8080` via QEMU SLIRP. Confirmed boot run #2. |

### Boot run #3 (2026-06-22) — **P2 full agentic loop VERIFIED** ✅

```
ViCell > hypha
Hypha — Cellos AI agent (P2: file tools). Type 'exit' to quit.
[hypha/llm-gateway] service ready
[tool-fs] ready

you> what files are in /bin?
[gw] plaintext mode -> :8080  [gw] TCP connected; sending  [gw] sent; reading
[gw] response bytes: 329        ← round 1: TOOL_CALL: list_dir /bin
[hypha] tool: list_dir          ← core dispatched to tool-fs
[gw] TCP connected; ...
[gw] response bytes: 527        ← round 2: synthesized text with tool_result
hypha> Based on the file system query, here is what I found:
       {"files":["init","shell","vfs","config","lua","python","doom","tetris",
       "tetris-lua","audio-demo","https-demo","ls","cat" ...
```

- **Full 2-round-trip agentic loop verified**: user question → TOOL_CALL → tool-fs → VfsClient → result → LLM synthesis → final reply
- **`tool-fs` + VfsClient working**: `list_dir("/bin")` via BootFsProxy returned 13+ binaries with correct `d:` / `f:` prefix stripped
- **`run_turn()` sub-loop working**: tool_call + tool_result appended to working prompt, second LLM call triggered automatically
- **Truncation in mock only**: 120-char snippet limit in mock proxy — real LLM processes full JSON
- **P2 status → COMPLETE**

### P4 planning (2026-07-12) — `tool-peripheral` plan written
- **New gaps G19/G20/G21 surfaced** while planning the G1 hardware showcase (see register).
- **G19 (spawn-chain)**: the P2/P3 "core spawns its own tools" pattern breaks for a
  `gpio` tool — `granted = requested ∩ spawner_caps` strips gpio because core holds
  none (and must not, per the showcase). Resolution: `init` spawns `tool-peripheral`
  (Root, exempt) + registers `service::HYPHA_PERIPHERAL`; core discovers via lookup.
- **G20 (no MMIO release)**: `MmioRegion` has no `Drop`; PL061 frees only on cell death.
  Workaround = lazy open + `AlreadyExists` retry (pwm-demo precedent); hold for life.
- **G21 (ARM disk)**: Hypha was RISC-V-only; P4 adds all 6 cells to `format-disk-arm.ps1`
  (`aarch64-unknown-none-softfloat`) + an ARM64 `hypha-p4-boot` spawn-gate test.
- **Reuse confirmed** (no new drivers): `Pl061Gpio`/`BitBangI2c`/`BitBangPwm` rlibs +
  `sht3x::parse` from robot-demo; app-owns-MMIO, no IPC broker.
