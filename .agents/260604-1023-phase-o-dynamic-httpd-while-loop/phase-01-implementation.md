# Phase O — Phase 01: Implementation

## Context links

- Plan: [plan.md](./plan.md)
- httpd: `cells/apps/net-tools/src/bin/httpd.rs`
- Parser: `cells/apps/shell/src/parser.rs`
- Executor: `cells/apps/shell/src/executor.rs`
- Tests: `tests/integration/tests/boot.rs`
- Pattern reference (Phase N if/then/fi): parser.rs:84-93, 173-175, 216-244

## Overview

- **Priority**: P2
- **Status**: pending
- **Description**: Two features. (O-1) httpd reads its served file from VFS on every request so content stays live; missing file → HTTP 404. (O-2) Shell gains `while COND; do BODY; done` using the keyword-as-Word pattern.

## Key insights (verified)

- httpd currently caches the file once at httpd.rs:187-194 (outside accept loop) and reuses `file_buf[..file_len]` for every request (line 255). Header built at lines 244-252 with hardcoded `HTTP/1.0 200 OK`.
- `vfs_read` (httpd.rs:65-79) already returns `usize` (0 = not found / empty). Reuse as-is per request.
- Parser dispatches `if` BEFORE semicolon split (parser.rs:173-175). `while` must mirror this — semicolons inside the construct are structural, not sequence separators.
- `parse_tokens` (parser.rs:193-205) strips leading/trailing semicolons and handles nested sequences — reuse for cond/body slices.
- Executor: `Ast::If` returns 0 when neither branch runs (executor.rs:112-121); convention `Ok→0/Err→1` at executor.rs:268. `Ast::While` returns 0 after loop.
- `vcat` missing → `Err(ViError::NotFound)` → exit 1 (cmd_fs.rs:381). `vcat` found → 0. This is the loop's natural terminator.
- `rm` built-in unlinks VFS path (executor.rs:242 → cmd_fs.rs:241).

## Requirements

**Functional**
- httpd serves current VFS content per request; 404 when file absent.
- `while COND; do BODY; done` runs BODY while COND exits 0; exits when COND non-zero.
- Keywords `while`/`do`/`done` remain usable as plain arguments to external commands (no Tok variants).

**Non-functional**
- No `libs/api` / `libs/types` edits (Law 1).
- Cells stay `#![forbid(unsafe_code)]` (Law 4) — no unsafe added.
- File stays compilable; clippy clean.

## Architecture / data flow

**O-1 httpd per-request (data flow)**
```
accept(stream_cap)
  → drain_request(stream_cap)
  → file_buf[4096]; file_len = vfs_read(path, &mut file_buf)
  → if file_len == 0:  tcp_send(404 header)           [no body]
     else:             tcp_send(200 header + Content-Length)
                       tcp_send(file_buf[..file_len])
  → yield-flush loop → close_cap(stream_cap)
```

**O-2 while (parse → ast → exec)**
```
parse(line)
  → tokens.first()==Word("while")?  → parse_while_stmt(tokens)
       do_pos   = first Word("do")
       done_pos = last  Word("done")
       cond = parse_tokens(tokens[1..do_pos])
       body = parse_tokens(tokens[do_pos+1..done_pos])
       Ast::While { cond, body }
  → no do/done → fall through to existing parse path
execute(While{cond,body})
  → loop { if execute(cond)!=0 break; execute(body) }  → 0
```

## Related code files

**Modify**
- `cells/apps/net-tools/src/bin/httpd.rs` — move vfs_read into loop, add 404 branch, build 200 header per request.
- `cells/apps/shell/src/parser.rs` — add `Ast::While`, `parse_while_stmt`, dispatch in `parse()`.
- `cells/apps/shell/src/executor.rs` — add `Ast::While` arm.
- `tests/integration/tests/boot.rs` — add `network_httpd_dynamic_content`, `shell_while_loop`.

**Create / delete**: none.

## Implementation steps

### O-1 — Dynamic httpd

1. Delete the pre-loop cache block at httpd.rs:186-194 (the `let mut file_buf`, `let file_len`, and the `if file_len == 0 { ... return; }` guard). Keep `path` in scope.
2. Inside `loop { ... }`, after `drain_request(stream_cap);` (line 242), add:
   ```rust
   let mut file_buf = [0u8; 4096];
   let file_len = vfs_read(path, &mut file_buf);
   ```
3. Replace the header build + send (lines 244-255) with a branch:
   - `file_len == 0`: send a fixed 404, e.g.
     `b"HTTP/1.0 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 0\r\n\r\n"` via `tcp_send`; no body.
   - else: build the existing 200 header (status + `write_content_length(file_len, ...)` + `\r\n`), `tcp_send(header)`, then `tcp_send(&file_buf[..file_len])`.
4. Keep the post-send yield-flush loop (lines 257-260) and `close_cap(stream_cap)` for both branches.
5. Update the module doc-comment (httpd.rs:5-8) to say content is read per request, not cached.

### O-2 — Shell while loop

6. parser.rs `Ast` enum (after the `If` variant, ~line 68): add
   ```rust
   /// `while COND; do BODY; done` — loop while COND exits 0.
   While {
       cond: alloc::boxed::Box<Ast>,
       body: alloc::boxed::Box<Ast>,
   },
   ```
7. parser.rs `parse()` — add BEFORE the semicolon split, right after the `if` dispatch (parser.rs:175):
   ```rust
   if tokens.first() == Some(&Tok::Word("while".into())) {
       return parse_while_stmt(&tokens);
   }
   ```
8. Add `parse_while_stmt` (near `parse_if_stmt`), reusing `is_kw` + `parse_tokens`:
   ```rust
   fn parse_while_stmt(tokens: &[Tok]) -> Ast {
       let do_pos   = tokens.iter().position(|t| is_kw(t, "do"));
       let done_pos = tokens.iter().rposition(|t| is_kw(t, "done"));
       // Malformed (missing do/done): fall back to normal parsing.
       let (dp, np) = match (do_pos, done_pos) {
           (Some(d), Some(n)) if n > d => (d, n),
           _ => return parse_tokens(tokens),
       };
       let cond = parse_tokens(&tokens[1..dp]);
       let body = parse_tokens(&tokens[dp + 1..np]);
       Ast::While {
           cond: alloc::boxed::Box::new(cond),
           body: alloc::boxed::Box::new(body),
       }
   }
   ```
   Note: fallback calls `parse_tokens` (not `parse()`) to avoid re-dispatching on the `while` word and recursing infinitely.
9. executor.rs `execute()` — add arm after `Ast::If` (executor.rs:121):
   ```rust
   Ast::While { cond, body } => {
       loop {
           if execute(cond, jobs) != 0 { break; }
           execute(body, jobs);
       }
       0
   }
   ```
10. No tokenizer change — `do`/`done`/`while` stay as `Word` (Phase N rule). Verify `parse_cmd`'s `_ => {}` arm (parser.rs:305) is untouched.

### Tests

11. Add `network_httpd_dynamic_content` (after `network_httpd_serves_file`, boot.rs:947). Boot with hostfwd port 9092; `wait_for("DHCP acquired")`; `vwrite /tmp/v1.txt CONTENT_V1`; `httpd 9092 /tmp/v1.txt &`; `wait_for("httpd: listening")`. GET → assert body contains `CONTENT_V1`. Then `vwrite /tmp/v1.txt CONTENT_V2`; new `TcpStream::connect` + GET → assert `CONTENT_V2` AND not `CONTENT_V1`. Use a fresh stream per request (server closes after each).
12. Add `shell_while_loop` (after `shell_if_else_branch`, boot.rs:995):
    - False condition: `while vcat /no/such/file; do echo SHOULD_NOT_APPEAR; done` → `wait_for("ViCell >")`; assert `!output_contains("SHOULD_NOT_APPEAR")`.
    - True-once: `vwrite /tmp/wflag.txt X` → `wait_for("ViCell >")`; `while vcat /tmp/wflag.txt; do echo WHILE_BODY; rm /tmp/wflag.txt; done` → `wait_for("WHILE_BODY")`; then `wait_for("ViCell >")` to confirm the loop terminated (no hang).

### Build / verify

13. Host unit tests for parser: `cargo test -p app-shell` (parse module is `#[cfg(test)]`). Add an optional host test asserting `matches!(parse("while echo a; do echo b; done"), Ast::While{..})` and that `while` survives as arg when no `do`/`done`.
14. Shell pipeline (O-2): steps 1–5 from plan context (build app-shell → cp to embedded → mkfat32 → build kernel → write-cell-table).
15. httpd-only (O-1): step 5 only (rebuild disk cell table with new httpd binary).
16. `cargo clippy -- -D warnings` on shell + net-tools; `cargo fmt --all`.
17. Run integration suite — expect 41 + 2 new = 43 passing.

## Todo list

- [ ] O-1.1 Remove pre-loop file cache (httpd.rs:186-194)
- [ ] O-1.2 Add per-request `vfs_read` after `drain_request`
- [ ] O-1.3 404 branch + 200 branch, both reach yield-flush + close
- [ ] O-1.4 Update httpd doc-comment
- [ ] O-2.1 `Ast::While` variant in parser.rs
- [ ] O-2.2 `while` dispatch in `parse()` before semicolon split
- [ ] O-2.3 `parse_while_stmt` with do/done fallback to `parse_tokens`
- [ ] O-2.4 `Ast::While` arm in executor.rs
- [ ] T.1 `network_httpd_dynamic_content`
- [ ] T.2 `shell_while_loop` (false-cond + true-once)
- [ ] T.3 parser host unit tests
- [ ] B.1 Build pipeline + clippy + fmt
- [ ] B.2 Full integration run (43 green)

## Success criteria

- `network_httpd_dynamic_content` green: second GET returns `CONTENT_V2` without httpd restart.
- httpd 404 on missing file (manual or test-observed: no crash, connection closes cleanly).
- `shell_while_loop` green: body runs once for flag file, 0 times for false cond, loop exits.
- All prior 41 tests still pass (esp. `network_httpd_serves_file`, `shell_if_*`, `lua` if/then).
- clippy `-D warnings` clean; no unsafe added; no `libs/api` change.

## Risk assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `while` recursion via `parse()` self-dispatch on fallback | Med | High (stack overflow) | Fallback uses `parse_tokens`, not `parse` (step 8) |
| Infinite while in test if `rm` doesn't unlink VFS or `vcat` keeps returning 0 | Med | High (test hang) | Verified `rm`→OP_UNLINK + `vcat`→Err on miss; bound test with `wait_for` timeout; CMD_TIMEOUT guards |
| httpd 404 path leaves socket half-open / hangs host read | Low | Med | Both branches share the same yield-flush + `close_cap`; 404 has Content-Length: 0 so host read completes |
| Per-request `vfs_read` slows serving / re-enters VFS IPC under load | Low | Low | One conn at a time (existing design); same IPC already used once at startup |
| Keyword leak: `while`/`do`/`done` eaten from external args | Low | Med | No Tok variants added; matched by string only (Phase N pattern); add host test in step 13 |

## Rollback

- O-1: revert httpd.rs, rerun step 5 — restores cached-content behavior. No persisted state.
- O-2: revert parser.rs + executor.rs + the new test, rerun steps 1–5. `Ast::While` is additive; removing it plus its single match arm restores prior exhaustive match.
- Independent: rolling back one feature does not affect the other (disjoint files; cell-table rebuild is idempotent).

## Security considerations

- httpd already discards request body and serves a fixed operator-supplied path; per-request read does not widen the surface (still only `path` from argv). No path traversal introduced — VFS resolves the same fixed path each time.
- `while` executes only already-authorized shell built-ins / spawned `/bin/*` cells; no new privilege path.

## Next steps

- Depends on: nothing (both features self-contained).
- Follow-up: document `while` in shell spec `docs/specs/11-shell.md` and update `docs/project-changelog.md` (Phase O entry) after green.
