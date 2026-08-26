# Phase 02 — Function Positional Args (X-2)

**Priority:** P1 | **Effort:** ~2h | **Status:** pending | **Files:** 1

## Context Links
- Function dispatch: `cells/apps/shell/src/executor.rs:535-549`
- Var store: `set_var` (149), `get_var` (178), `unset_var` (136); 16-slot static `VARS` (130-134)
- Expansion: `expand_token` (199-230); `$?` special case at 207-212
- `i32_to_str` (452) — **returns `&'static str` from a SINGLE static buffer**

## Overview
Bind `$1..$9`, `$#`, `$@` for user-defined function bodies, then restore the
prior values on return so nested/sequential calls don't leak args.

## Key Insights (verified corrections to brief)
- `i32_to_str` reuses one static buffer — calling it twice invalidates the first
  result. The brief's `saved.push((String::from(k), get_var(k)))` then later
  `set_var(k, args[i])` is fine ONLY if `k` is materialized to an owned `String`
  immediately and `i32_to_str` is not interleaved with a still-live borrow.
- `get_var` returns `&'static str` into `VARS` — saving it across a `set_var`
  that overwrites the SAME slot would alias. Save by copying into an owned
  `String` (heap) before any `set_var`.
- `expand_token` only expands `$NAME` (alpha+`_`) and `$?`. `$#` and `$@` need
  explicit special cases like `$?`.

## Architecture / Data Flow
Per function call: snapshot `$1..$N, $#, $@` (owned copies) → set new values →
`parse`+`execute` body → restore snapshot (re-`set_var` or `unset_var`).

## Related Code Files
- Modify: `cells/apps/shell/src/executor.rs` — function-call block (535-549) + `expand_token` (199-230)

## Implementation Steps
1. In the `if let Some(body) = get_function(prog)` block (537), BEFORE the
   `parse`/`execute`:
   - `let nargs = args.len().min(9);`
   - Build `saved: Vec<(String, Option<String>)>` — for `i in 1..=nargs`:
     key = `alloc::string::ToString` of the index (use a small local
     `usize→String` helper, NOT `i32_to_str`, to avoid the shared-buffer trap);
     value = `get_var(&key).map(String::from)` (own it now).
   - Also snapshot `"#"` and `"@"`.
   - Then `set_var(&key, args[i-1])` for each; `set_var("#", &count_string)`;
     `set_var("@", &args.join(" "))`.
2. Execute body (existing `parse`+`execute`).
3. Restore: for each saved `(k, v)` → `Some(old) => set_var(&k,&old)`,
   `None => unset_var(&k)`. Restore/unset `"#"` and `"@"`.
4. In `expand_token`, after the `$?` branch (212), add:
   - `next == b'#'` → push `get_var("#")` value; `i += 2`.
   - `next == b'@'` → push `get_var("@")` value; `i += 2`.
5. Build shell, regenerate disk, boot, test.

## Todo List
- [ ] usize→String index helper (avoid i32_to_str shared buffer)
- [ ] Snapshot $1..$N, $#, $@ as owned values
- [ ] Set new positional vars + $# + $@
- [ ] Restore/unset after body
- [ ] `$#` / `$@` cases in expand_token
- [ ] Build + boot + manual test

## Success Criteria
- `double() { echo $1 $2; }; double ALPHA BETA` → prints `ALPHA BETA`.
- `argc() { echo $#; }; argc a b c` → prints `3`.
- `all() { echo $@; }; all x y z` → prints `x y z`.
- After a function returns, a top-level `echo $1` shows the prior value (or
  empty) — confirms restore works.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| `i32_to_str` shared-buffer aliasing | Med×High | Use a dedicated owned-String index helper, never the static one |
| `get_var` `&'static` aliases slot being rewritten | Med×High | Copy to owned `String` before any `set_var` |
| >9 args silently dropped | Low×Low | Cap at 9 (POSIX-ish), document; matches brief |
| VARS 16-slot store overflow ($1-$9 + #@ + user vars) | Med×Med | 9+2 positional ≈ 11 slots; warn that deep scripts may exhaust 16-slot store — note as known limit |

## Rollback
Remove the snapshot/set/restore block and the two `expand_token` cases. No state
persists across reboot (VARS is in-RAM only).

## Security Considerations
None new — positional values are caller-supplied strings already trusted as argv.

## Next Steps
Phase 03 edits the same `expand_token`; land 02 first to avoid conflicting diffs.
