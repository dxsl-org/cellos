## Slice: ScriptEnginesAi — files read: 41 — unsafe blocks reviewed: ~48

Vendored vs project code identified:
- `cells/services/ocel-quickjs/vendor/quickjs/*` = upstream QuickJS **2026-06-04** (`VERSION`, `Changelog:1`), one local deviation (`VIOS_NO_ATOMICS`, `quickjs.c:71-78`). Project code: `src/engine.rs`, `src/native_layout.rs`, `src/ffi.rs`, `src/c/qjs_cellos_glue.c`, `src/prelude.js`, `build.rs`.
- `cells/services/ocel-pdf/vendor/mupdf/*` (1232 files) = upstream **MuPDF 1.26.1**, unmodified per `PROVENANCE:4-9`; `FZ_ENABLE_JS 0` (`vendor/cellos-config.h:16`). Project glue: `src/main.rs`, `src/engine.rs`, `src/c/renderer.c`, `src/c/libc.c`, `build.rs`.
- `cells/runtimes/lua/src/c/*` = upstream **Lua 5.4.7** (`lua.h:19-24`). Project code: `src/main.rs`, `bindings_*.rs`, `repl_session.rs`, `glue/*.c`.
- AI stack (`libs/gguf-rs`, `ai-engine`, `ai-tokenizer`, `tensor-math`, `ai-proto`, `ai-sdk`, `cells/services/ai`) is all first-party `#![forbid(unsafe_code)]` Rust.

Known CVEs [INFERENCE — not verified against a CVE database in this session]: no public CVE is known to me for QuickJS releases after 2025-09-13 or for Lua 5.4.7 as shipped; MuPDF 1.26.1 predates later 1.26.x point releases that carried decoder (JBIG2/JPX/FreeType) bug fixes, and the renderer here enables JPX, JBIG2 and ICC decoders (`cellos-config.h:22-23`, `PROVENANCE:45-47`), so the un-upgraded surface is the raster/codec path inside Tier 2.

### Findings

#### [S-1] Lua runtime cell loads arbitrary bytecode and runs in Tier 1 SAS — Severity: High — STRIDE: E — OWASP: A08
- Where: `cells/runtimes/lua/src/main.rs:10` (manifest without `tier =`), `:95-119` (sandbox strips only `io.popen`/`os.execute`/`debug`/`package.loadlib`; searcher calls `load(src, '@'..path)` with default `"bt"` mode), `:296-307` (`luaL_loadbufferx(..., mode = NULL)` on any VFS file given in argv); `kernel/src/loader/governed_spawn.rs:60-71` (a signed cell is sent to Tier 2 only if its manifest class is FFI/UNTRUSTED); `scripts/unsafe-allowlist.toml:97-135` (Lua cell allowlisted as "Tier 1b").
- What: The Lua interpreter (C, no bytecode verifier) accepts precompiled chunks via `load()`, `require()`'s VFS searcher and the top-level script loader. `string.dump` is not removed. Unlike `ocel-quickjs`/`ocel-pdf` (`PROTECTION_CLASS_UNTRUSTED`), the Lua cell's manifest carries no tier, so once signed by the image lane it is admitted to the shared address space (Tier 1).
- Exploit: Any VFS-resident `.lua` file (untrusted disk input, or bytes fetched via `vnet.recv` and written with `vfs.write` from a script) containing a crafted Lua 5.4 bytecode chunk — well-known to yield arbitrary read/write inside the interpreter process. In SAS that is the whole system, not just the cell.
- Fix: Set `tier = PROTECTION_CLASS_UNTRUSTED` in the Lua manifest; wrap `load`/`loadstring`/`dofile`/`loadfile` to force mode `"t"` and pass `"t"` to `luaL_loadbufferx` at `main.rs:296-303`; delete `string.dump`.

#### [S-2] Network-sourced LLM output drives `spawn_cell`/`kill_cell`/`write_file` with no confirmation; tool cells accept `Invoke` from any sender — Severity: High — STRIDE: S/E — OWASP: A01/A03
- Where: `cells/apps/hypha/core/src/main.rs:182-201` (every `ToolCalls` reply executed unconditionally), `cells/apps/hypha/tools/spawn/src/main.rs:24,44-50,57-69` (holds `spawn = true`; `handle()` dispatches `AgentToolRequest::Invoke` from *any* `sender`; `spawn_cell` calls `sys_spawn_from_path(path)` with the LLM's path, `kill_cell` → `sys_force_exit`), `cells/apps/hypha/llm-gateway/src/main.rs:45-48` (`USE_TLS = false` → plaintext HTTP to `10.0.2.2:8080`), `cells/apps/hypha/tools/fs/src/main.rs:61-84` (`read_file`/`write_file` of any path).
- What: The "LLM" is a plaintext HTTP peer; whatever JSON it returns beginning with `TOOL_CALL:` is executed by a SpawnCap-holding cell without an operator prompt. Separately, `tool-spawn` is a confused deputy: any cell that learns its tid can send `Invoke{spawn_cell|kill_cell}` and gain spawn/kill authority it does not hold.
- Exploit: On-path network attacker (or anyone who can answer port 8080) replies `TOOL_CALL: {"name":"spawn_cell","args":{"path":"/bin/<x>"}}` or `kill_cell {"tid":N}` / `write_file`. Locally, an unprivileged cell sends the same postcard frame to `tool-spawn` directly.
- Fix: Require explicit user confirmation (y/n on the hypha console) before any spawn/kill/write tool; make `tool-spawn`/`tool-fs` accept `Invoke` only from the hypha core tid they were spawned by (filter on `sender`), and constrain `spawn_cell` to an allowlist of paths. (Demo app, but it is the only privileged consumer of AI output in the tree.)

#### [S-3] Committed dev CA private key is the net service's default trust root — Severity: High — STRIDE: S — OWASP: A02/A08
- Where: `tools/hypha-mock-llm/ca-key.pem` (tracked; `.gitignore:4` says intentional), `tools/hypha-mock-llm/gen-dev-ca.py:5-16` (writes the CA into `cells/services/net/roots/private.der`), `cells/services/net/Cargo.toml:14,19` (`default = ["tls-roots-embedded", "tls-ca-private"]`).
- What: `openssl` fingerprint of `ca.pem` and of `cells/services/net/roots/private.der` are identical (`36:33:5E:76…25:D1`, CN "Cellos Dev CA", valid to 2036). Every default build of `service-net` therefore trusts a CA whose private key is public, and the "verified" TLS path (`tls-ca-private`) is no stronger than `tls-insecure`.
- Exploit: Anyone with the repo mints a leaf cert for any hostname with `ca-key.pem` and MITMs every TLS session a default-built device makes (sshd peers, LLM gateway in TLS mode, future fleet endpoints).
- Fix: Remove `ca-key.pem` from git history; make `tls-ca-private` refuse to build when `private.der` matches the dev CA fingerprint (or gate the dev root behind a non-default `tls-dev-ca` feature); document that `private.der` must be replaced before any non-lab image.

#### [S-4] AI service KV cache grows unbounded after admission → OOM kills the service — Severity: Medium — STRIDE: D — OWASP: A04
- Where: `libs/ai-engine/src/lib.rs:418-433` (`KvCache::ensure` resizes to up to `n_ctx × n_layer × kv_dim` f32 ×2 with no budget check), `:596-623` (`memory_limit` checked once at load with zero sessions), `:700-736` (`submit` admits up to 4 sessions, no memory check), `cells/services/ai/src/main.rs:66-68,85` (16 MiB heap / 14 MiB engine limit), `libs/ostd/src/heap.rs:111-112` (`alloc_error_handler` → cell dies).
- What: Resident memory is checked only in `Engine::load`. Each session's cache then grows geometrically as it advances, up to `n_ctx` (≤4096) positions. For a 25 MB model in the 29 MiB `large-arena` build, a single session a few hundred tokens deep can exceed the remaining heap; `Vec::resize` then aborts the cell.
- Exploit: Any cell with IPC access to `/bin/ai` submits a ~2 KB prompt with `max_tokens = 1024` (both within `ai-proto` limits) and polls; repeat for 4 sessions. The service OOM-exits for every client.
- Fix: In `generate`, before `kv.ensure`, compute the post-growth `resident_bytes()` and return `AiError::Busy`/`ContextFull` when it would exceed the limit; or cap `n_ctx` per session to what the arena can hold at admission.

#### [S-5] `InferEmbed` does up to 2048 forward passes in one IPC turn, then may send no reply — Severity: Medium — STRIDE: D — OWASP: A04
- Where: `libs/ai-engine/src/lib.rs:878-903` (`embed` runs `forward` for every token synchronously, truncated only to `n_ctx`), `libs/ai-proto/src/lib.rs:383-387` (text ≤ 2048 bytes), `cells/services/ai/src/main.rs:330-335` (`encode_embedding_values(...)?` returns `None` → no reply when `n_embd > MAX_EMBED_DIM` (768)).
- What: Generation is budgeted at `STEPS_PER_POLL = 4` steps per turn, but embedding is not: one request monopolises the single-threaded loop for up to `n_ctx` forward passes. If the model's `n_embd` exceeds 768, all that work is done and then the request is silently dropped (the client blocks or times out).
- Exploit: Send `InferEmbed` with 2048 bytes of single-byte tokens repeatedly; other callers' polls stall.
- Fix: Bound embed to a token count the loop can afford (or step it like generate); refuse `InferEmbed` up front when `cfg.n_embd > MAX_EMBED_DIM` with `NotSupported`.

#### [S-6] QuickJS execution budget is instruction-count only (~1e9 ops per eval), no wall-clock bound — Severity: Medium — STRIDE: D — OWASP: A04
- Where: `cells/services/ocel-quickjs/src/engine.rs:36-51,202-203` (`MAX_INTERRUPT_POLLS = 100_000`), `vendor/quickjs/quickjs.c:517` (`JS_INTERRUPT_COUNTER_INIT 10000` → the handler fires every 10k interpreter ops).
- What: One `eval_raw` may execute ≈10⁹ bytecode operations before `interrupt_budget` fires; the cell is single-threaded (`main.rs:107-164`), so every other viewer context and the native-layout exchange wait. The engine's own 500-tick resume timeout (`native_layout.rs:235`) protects the reverse direction only.
- Exploit: Page script `for(;;){}` (or regex/`sort` work that polls rarely) stalls the engine for tens of seconds per `EvalScript`, repeatable per document.
- Fix: Make `interrupt_budget` compare `sys_get_scheduler_ticks()` against a deadline (e.g. 100 ticks) in addition to the counter.

#### [S-7] ocel-pdf render deadline is cooperative (allocation checkpoints only) — Severity: Medium — STRIDE: D — OWASP: A04
- Where: `cells/services/ocel-pdf/src/c/renderer.c:12-25,40-46,66,176-177` (`work_expired()` consulted only in `bounded_malloc/realloc` and after `fz_run_page`; `fz_run_page(..., cookie = NULL)`), `src/main.rs:175-217` (synchronous loop).
- What: A content stream that draws without allocating (e.g. hundreds of thousands of full-page fills within the 2 MiB input limit) never hits a checkpoint; the 400-tick deadline is observed only after `fz_run_page` returns. The service is single-owner, so the viewer and every other prospective owner wait.
- Exploit: Hostile PDF with a 2 MB content stream of `0 0 1000 1000 re f` repeated → minutes of rasterization per `RenderPage`.
- Fix: Pass an `fz_cookie` whose `abort` is set from the `work_expired()` check inside `bounded_malloc`, and additionally insert a `fz_cookie` progress callback; or bound content-stream operator count via `fz_cookie.progress_max`.

#### [S-8] GGUF bool arrays sized as 8 bytes per element — Severity: Low — STRIDE: - — OWASP: -
- Where: `libs/gguf-rs/src/lib.rs:902` (`value_type::BOOL ... => Some(8)`) vs `:858` (scalar bool read as 1 byte).
- What: GGUF encodes `bool` as one byte. An array of bools is consumed at 8 bytes/element, so the reader either reports `BadArrayLength` on a valid file or mis-positions the cursor for every later key/tensor entry. Not memory-unsafe (all reads are bounds-checked), but a valid model is rejected or misparsed. No test covers bool arrays (`:1076-1078` tests scalars only).
- Fix: Return `Some(1)` for `BOOL`.

#### [S-9] Quadratic metadata dedup on hostile GGUF header — Severity: Low — STRIDE: D — OWASP: -
- Where: `libs/gguf-rs/src/lib.rs:346-352` (`metadata.iter().any(...)` per key, `metadata_count` up to the file size / ~14 bytes).
- What: A 25 MB file with ~1.8 M tiny keys costs ~10¹² string compares at load. Disk input only; the AI cell loads at boot.
- Fix: Dedup with a `BTreeSet<&str>` or cap `metadata_count` (e.g. 64 K).

#### [S-10] ocel-pdf `Open{path}` lets any client make the PDF cell open arbitrary VFS paths — Severity: Low — STRIDE: I — OWASP: A01
- Where: `cells/services/ocel-pdf/src/main.rs:41-45,89-98` (path from the IPC caller; only length/NUL checked), `:21-32` (`OpenCap`/`ReadCap` in allowlist).
- What: Confused deputy: the PDF cell reads whatever its own caps allow and returns rasterised pixels (and MuPDF's error text) to the requester. Only valid PDFs render, so disclosure is limited to PDF files and parse-error strings.
- Fix: Restrict `Open` to a per-owner root (e.g. caller-supplied cap/grant rather than a path), or an allowlisted prefix.

#### [S-11] Token-drain reply failure silently loses generated tokens; poll may send no reply — Severity: Low — STRIDE: - — OWASP: -
- Where: `cells/services/ai/src/main.rs:378-400` (`drain` removes ids from `pending` before `encode(...)`; `.ok()` → `None` → no reply, tokens gone), `libs/ai-engine/src/lib.rs:820-826` (token text appended from `token_bytes` of arbitrary vocab length).
- What: A vocabulary with long token strings (hostile model file) makes 16 tokens' text overflow the 4 KiB reply; the caller gets nothing and the tokens are dropped from the session.
- Fix: Encode first, then commit the drain; cap `text` at `MAX_REPLY_TEXT_BYTES` and carry the remainder.

#### [S-12] Tokenizer special-token scan is O(specials × text²) and user text can inject control tokens — Severity: Low — STRIDE: D/T — OWASP: -
- Where: `libs/ai-tokenizer/src/lib.rs:462-500` (per iteration `rest.find(token)` for every special; prompt text matched against CONTROL/USER_DEFINED tokens), `:332-345`.
- What: A model with many control tokens plus a 2 KB prompt costs billions of byte compares per `submit`; and a caller can embed `<|im_start|>`-style control tokens in plain text (prompt-injection of chat-template structure). Both are per-request, tenant-scoped.
- Fix: Use a single pass with a trie/Aho-Corasick; only parse specials when the caller explicitly opts in.

#### [S-13] Mock LLM proxy binds `0.0.0.0` with no auth and emits spawn/kill tool calls — Severity: Info — STRIDE: S — OWASP: A05
- Where: `tools/hypha-mock-llm/mock_proxy.py:37-38,113-122,175-204`.
- What: Dev host tool; any LAN peer that can reach the host port can cause the guest to execute `spawn_cell`/`kill_cell` (see S-2). Bind to `127.0.0.1`/the SLIRP interface by default.

#### [S-14] Stale heap comment in ocel-quickjs — Severity: Info — STRIDE: - — OWASP: -
- Where: `cells/services/ocel-quickjs/src/main.rs:38-41` says "3 MiB accounted heap"; actual `MAX_HEAP_BYTES = 20 MiB` (`engine.rs:76`) inside a 24 MiB cell heap. Update the comment; the 4 MiB headroom is what the Rust side has for transfer buffers and snapshot source.

### Verified-safe areas
- QuickJS host surface is a single native binding, `__vios_nativeLayout`, defined non-writable/non-configurable (`qjs_cellos_glue.c:65-73`); all prelude hooks use `Object.defineProperty` defaults (`prelude.js:64,807-881`); no file/net/IPC primitive is reachable from script. The native callback bounds queries (64), mutation count/bytes, validates UTF-8, and only exchanges with the recorded owner tid/context/query (`native_layout.rs:48-116,214-331`).
- Script injection into generated JS is escaped for `"`, `\`, CR/LF, control chars and U+2028/2029 (`engine.rs:707-723`); untrusted scripts run through `__vios_evalScript("…")` (`engine.rs:536-551`).
- JS→C string handoff is pre-bounded by UTF-16 length before `JS_ToCStringLen2` (`qjs_cellos_glue.c:27-43`), and Rust re-checks UTF-8 (`engine.rs:299-301`). Heap/stack limits set (`engine.rs:151-153`; 192 KiB JS stack vs 256 KiB cell stack, IPC buffers 4 KiB).
- QuickJS and ocel-pdf are `PROTECTION_CLASS_UNTRUSTED` (Tier 2) with minimal syscall allowlists (`ocel-quickjs/main.rs:30-36`, `ocel-pdf/main.rs:15-32`).
- MuPDF glue: input ≤ 2 MiB, 10 MiB accounted allocator with overflow-safe accounting (`renderer.c:40-77`), page pixel cap with ceil-adjust (`:158-170`), stride/components/alpha verified before in-place BGRA swap (`:184-196`), chunked pixel reads bounds-checked in both C and Rust (`:214-219`, `engine.rs:139-163`), owner-TID lease on the single handle (`main.rs:117-121,143-147`), `fz_try` never crosses a Rust frame; `strftime`/`parse_integer` shims are bounds/overflow-checked (`libc.c:70-112,198-227`).
- GGUF: every read is cursor-bounded (`lib.rs:779-845`); tensor byte length re-derived from dtype×dims with `checked_mul` and range-checked at parse and at access (`:373-379,400-409,672-688`); `n_dims` 1..=4; alignment 0 rejected; capacity hints capped (`:945-953`).
- ai-engine: `read_config` validates head divisibility, non-zero dims, `n_ctx ≤ 4096`, vocab ≤ 262 144 (`:1162-1199`); every weight tensor's shape is checked against config (`:1368-1370,1399-1401`); f32 views via `zerocopy` alignment check (`:229-236`); `forward` rejects out-of-range token/position (`:945-947`); sessions owner-bound in the service (`main.rs:426-431`).
- ai-sdk transport decodes only replies from the resolved service tid (`ostd_transport.rs:45-48`).
- silo-guest mailbox: fixed 96-byte input snapshot, padding canonicality enforced, `len ≤ 64` check before slicing (`mailbox.rs:60-87`, `main.rs:101-109`), monotonic request sequence (`main.rs:71-79`).
- Lua FFI marshalling: every `lua_tolstring` length is used as-is from Lua (valid by API contract), `os.execute` path copy clamps to 511 (`bindings_io.rs:83-86`); `ViCell_os_execute` is not registered and `SpawnFromPath` is not in the Lua allowlist (`main.rs:11-20,255-261`).

### Not covered
- Vendored engine internals: `quickjs.c`/`libregexp.c`/`libunicode.c`/`dtoa.c`, all of `vendor/mupdf/**`, Lua `src/c/src/*.c` (only version/interrupt constants checked).
- `cells/services/ocel-quickjs/src/float_abi.rs`, `src/engine/tests.rs`, `tests/host_dom.c`, `ocel-pdf/smoke/*` (test/harness code).
- `cells/runtimes/lua/src/bindings_net.rs` body, `bindings_vfs_handle_read.rs`, `repl_session.rs` beyond the grep'd lines, `glue/lua_vios_glue.c`, `build.rs`.
- `libs/dom-arena` (`JsServiceSession` owner/idle logic) beyond constants and the `Eval` routing lines; `libs/ai-engine` tests; `libs/tensor-math/src/lib.rs` kernels beyond declarations; `libs/ai-sdk/src/testing.rs`.
- `cells/guests/silo-guest/src/crypto.rs` (left to KernelAuthorityCrypto/AuthorityKms); hypha `tools/fs`/`tools/sys` bodies beyond the grep'd dispatch lines; other `cells/apps/*`.