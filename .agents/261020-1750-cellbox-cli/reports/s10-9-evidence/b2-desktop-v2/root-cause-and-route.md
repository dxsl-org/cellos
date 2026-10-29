# S10-9.3 B2 desktop v2: why the 7.1 image failed, and what v2 changes

Read-only findings (no VM) before any build:

1. The 7.1/7.2 failure line `USER: OOM: cell heap exhausted — size=4341072` is exactly the byte size of the source-built, signed
   `/bin/ocel` (desktop feature) that 7.1 injected (`signed/desktop-ocel` = 4,341,072 B; the tracked ocel was 3,903,536 B).
2. 7.1 kept the TRACKED historical `/bin/vfs` (425,600 B, sha256 prefix 0d6735c6). Its heap segment (`LOAD RW 0x100058000-0x100558040`,
   memsz 5,242,944 = 4 MiB heap + 1 MiB, the 1 MiB part inferred) is smaller than the new ocel ELF. The same commit's vfs *source*
   (`cells/services/vfs/src/main.rs:106`, `VFS_HEAP_BYTES = 16 MiB`) builds a vfs with memsz 17,825,856 (`0x1100040`,
   v2 serial: `LOAD RW 0x100058000-0x101158040`). So the tracked service bytes are stale versus the commit's own sources and
   versus the rebuilt ocel. [INFERENCE: mechanism] the stale vfs cannot hold a 4,341,072 B buffer in its 4 MiB heap, task 5 exits 238 and the
   TLB-shootdown panic follows (the OOM log line carries no cell tag; task 5 = vfs by log order, as in the 7.2 comparison).
   Contract partner (init) is unchanged: the tracked init (sha256 0c2f607b..) boots both images.
3. `run-desktop.sh --rebuild` (commit's own route) only rebuilds ocel and service-net and keeps every other tracked byte. 7.1 replayed that
   route. It also never contained `/bin/desktop`, `/bin/terminal`, `/bin/files`, `/bin/chrome` and its autoload started ocel + shell, so
   the `[desktop] Ready. Entering event loop.` marker could not have appeared even without the OOM.
4. The commit's `scripts/package-desktop-x86.py` (not blocked at this commit) is the route that overlays compositor, desktop, files,
   terminal, chrome, ocel, net, vfs, shell onto the tracked base and rewrites autoload to desktop + shell. v2 replays its body step by
   step (script not executed: it signs with the unscoped whole-workspace F1 which refuses, and has no --locked/--offline), with the
   signing replaced by the authorised scoped mirror signing.

v2 contents: every desktop service/driver/app rebuilt from source at the commit (list in manifest.json `rebuilt_from_source_and_signed`),
scoped-signed; kept as tracked historical bytes (disclosed): `/bin/ocel-quickjs` (rebuild impossible without a fourth delta, an
allowlist entry for `cells/services/ocel-quickjs/src/native_layout.rs`; F1 whole-workspace refusal retained in
`build/logs/whole-workspace-check.log`; not spawned at boot), `/bin/tier2-smoke`, `/bin/tier2-exploit` (unsigned test fixtures, not
spawned), `/POLICY.BIN`, data files, toml configs, embedded `init`.
Other disclosed facts: cargo reused the worktree's incremental cache (leaf crates recompiled, dependency crates fresh; sshd compiled 18
crates anew); rebuilt artifacts that also existed in 7.1 (shell, ls/cat/echo/ps/kill, vfs, config, input, platform, six drivers, ocel,
net) are byte-identical unsigned and signed to the 7.1 builds; embedded `kernel_fs.img` grew 12,582,912 -> 16,777,216 B (mkfat32 sizing
for the added desktop/files/terminal/chrome apps); hooks kernel differs only by feature `test-hooks`.
