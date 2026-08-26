# Boot Suite De-Rot Triage (Bước 1 mục 1) — 2026-07-06

Full `cargo test --test boot` baseline: **30 passed / 23 failed** (329 s, RV64 QEMU).

## ═══ UPDATE 2026-07-07 — Category B FIXED, suite 30 → 40 pass ═══

**Category B root cause found & fixed** (2 commits): VFS writes ALWAYS WORKED —
the shell's `sys_recv(0)` wildcard consumed a QUEUED INPUT KEY EVENT as the VFS
reply (poisoned by the 2026-06-29 input pending_msgs queue), decoded it as a
failure, and orphaned the real reply → every later VFS conversation desynced
(vcat hang). Fix:
1. kernel: Recv/RecvTimeout pending_msgs DRAIN now honours the recv mask
   (masked recv skips non-matching queued messages, leaves them for the
   wildcard read loop).
2. shell: every service conversation (vfs_req_ok, read_file_vfs, ReadAsync/
   Poll, ListDir walkers, config client) now recvs MASKED to the service tid.
3. console_drv: EV_ASCII relay backpressure — a failed ipc_post_nonblock
   (input queue full during paste-speed burst) parks the byte in
   PENDING_ASCII and retries next tick instead of silently dropping it
   (mid-line char loss "vappend /data/…" → "vappend ta/…" eliminated).

**Suite: 40/53 pass.** Newly stable (2/2 runs, added to CI allowlist):
wget_downloads_to_vfs, shell_or_operator, shell_exit_code_var, shell_echo_e,
shell_function_positional_args, shell_variable_persists, shell_source_script,
vfs_fat16_reboot_persistence.

## ═══ UPDATE 2026-07-07 (4) — CLI-exit-crash investigated, clean fix BLOCKED ═══

**Root cause of `scause=0xc sepc=0` on mqtt/wget exit (FOUND):** cell-build's
default `emit_linker_script()` sets `ENTRY(main)`, so the ELF entry is bare
`main` and the ostd `_start` crt0 (init-array + post-`main` Exit ecall) is
bypassed. A cell whose `main` loops forever never notices; a finite CLI cell
returns from `main` into `ret` with `ra=0` → jump to 0. (`_start` was ALSO
being GC'd — `.text.boot` pulled without `KEEP`, symbol only named by the
non-GC-root `ENTRY` — so even flipping ENTRY needs a `#[used]` root or KEEP.)

**Clean fix (ENTRY=_start) attempted and REVERTED** — commits ac5d377d then
2903bd1c: entering at `_start` DID eliminate the fault on riscv64 (verified
`scause=0xc` count 0→ for mqtt/wget), but `_start`'s init-array asm uses
ABSOLUTE addressing (`ldr =__init_array_start` on aarch64, `lea` on x86) that
`rust-lld` rejects under `-pie`: "R_AARCH64_ABS64 cannot be used against
symbol; recompile with -fPIC". It only linked on riscv64 (PC-relative `la`),
hiding the breakage; forcing `_start` into aarch64/x86 cells broke their link.

**Verdict: deferred.** The crash is cosmetic (cell finished its work; wget's
test passes WITH it). The real fix is a prerequisite of its own: make `_start`
PC-relative on all three arches (adrp/adr on aarch64, RIP-relative lea on x86,
already-correct auipc on riscv), THEN default to `ENTRY(_start)` + `KEEP` or
`#[used]`. That is a focused ostd/crt0 task, not part of de-rot. cell-build's
doc comment now records the blocker. Suite stays 48/53.

## ═══ UPDATE 2026-07-07 (3) — httpd_dynamic + input marker fixed, suite → 48 pass ═══

- **network_httpd_dynamic_content FIXED (real bug):** `httpd 9092 /file &` ran
  through spawn_external which ALWAYS sys_wait'd the child. httpd loops forever
  → shell parked in sys_wait → the test's second `vwrite V2` never ran → GET2
  served stale V1. httpd_serves_file passed only because it issues ONE GET.
  Fix: `BG_SPAWN` flag makes spawn_external skip sys_wait for `cmd &`.
- **input_service_registered_at_boot FIXED (marker drift):** the kernel's
  `set_input_cell` logs at `info!`, suppressed because `/bin/input` spawns
  after the kernel drops to Warn. Bumped to `warn!` (one-time boot-integrity
  event). Registration always happened; only the log was invisible.

**Suite 48/53. CI allowlist 25 tests.** Remaining 5 = the genuinely-hard/
low-leverage tail (documented, not chased):
- gpu_framebuffer_initialises — default boot omits the GPU device (documented
  as boot-blocking in lib.rs) AND the marker moved to the virtio-gpu Driver
  Cell; needs a GPU-attached boot variant, not a marker swap.
- input_bare_cell / input_keyboard_e2e — QMP `sendkey` → VirtIO-keyboard path
  (not serial); fragile monitor-socket dependency + marker drift.
- mqtt_subscribe — documented-fragile mock timing race (CONNACK+SUBACK in one
  TCP segment → mqtt_recv consumes both); the real net-recv path works
  (curl/wget/httpd green). mqtt cell also crashes scause=0xc sepc=0 on exit
  (CLI-cell return-to-null lifecycle bug, shared with wget — separate).
- bench_all_pass — 180 s wall-clock timeout under threads=2 QEMU contention,
  not a functional failure; run solo or bump the timeout.

## ═══ UPDATE 2026-07-07 (2) — C′ SOLVED, suite 40 → 46 pass ═══

**C′ root cause (measured with per-layer kernel counters):** all 70 burst bytes
reached the input service (`post_ok=70`), but input→shell showed
`queue=116 drop=21` — the focused cell's pending_msgs hit the shared 64-slot
HOTSWAP bound and silently dropped 21 events (~10 chars). The shell drains one
event per loop iteration and each echo is an SBI call per byte (slow on TCG),
so backlog accumulates ACROSS commands. Fix: dedicated
`INPUT_EVENT_QUEUE_DEPTH = 512` for the input-caller branch of ipc_try_send.

**Second finding — echo-vs-output test-bug class (5 tests):** negative
assertions like `!output_contains("CASE_WILD")` matched the typed command's own
serial ECHO. They passed historically only because char loss MANGLED the echo;
fixing input made echoes complete and exposed them. Fixed to match output lines
(`": CASE_WILD"` = "USER: CASE_WILD"): case_statement, or_operator (×2 sites),
and_operator, while_loop, if_else_branch.

**Suite: 46/53.** CI allowlist expanded to 22 tests.

**Remaining 7 red (all triaged, none input-class):**
- input_bare_cell / input_keyboard_e2e / input_service_registered_at_boot —
  need live-input markers (log-level/marker drift).
- gpu_framebuffer_initialises — marker retired with GPU cell exile.
- mqtt_subscribe — REAL bug: SUBACK never received (mqtt client net-recv path);
  mqtt cell also crashes scause=0xc sepc=0 on exit (same as wget — CLI cell
  return-to-null on exit, separate lifecycle bug).
- bench_all_pass — 180 s timeout under threads=2 contention; run solo/bump.
- network_httpd_dynamic_content — vwrite OK + "httpd: listening" OK now;
  fails at the GET/hostfwd serve step — real httpd/net bug, next candidate.

**Remaining 13 red — refined triage (2026-07-07 morning, superseded above):**
- **C′ residual typed-input stall (~6 tests, some flaky run-to-run):** on the
  3rd+ typed command of a session, input freezes mid-line (~char 7-8); shell
  alive (200 ms recv-timeout renewing), input service alive, ALL queues empty,
  PENDING_ASCII empty — the keystroke vanishes between input-svc dispatch and
  the shell. NOT the spawn_external drain (built-ins affected). Suspects:
  fb-console input relay competing for events (new 08d5d2be cell), focus
  re-request timing at prompt, or dispatcher try_send edge. Affects:
  shell_case_statement, shell_if_else_branch (flaky), shell_source_multi_command,
  shell_test_builtin, shell_while_loop, vfs_fat16_append/subdir_persistence,
  network_httpd_dynamic_content, mqtt_subscribe (types long command).
- input_bare_cell / input_keyboard_e2e / input_service_registered_at_boot:
  need live-input markers ("[input] registered input service TID" not printed
  at current log level) — marker/log-level drift + C′.
- gpu_framebuffer_initialises: marker gone (GPU cell exile) — needs new marker.
- bench_all_pass: 180 s timeout under threads=2 contention — run solo or bump.
- Harness gap: when a guest dies mid-test the reader thread blocks forever
  (observed httpd_serves hang with zero QEMU processes) — add read timeout.

Key outcome: the suite's redness is **mostly REAL BUGS, not assertion rot.** This
is the RC-2 payoff — the suite living outside CI hid a total VFS-write regression.
De-rot (fixing drifted assertions) applies to only ~2 tests; the rest need real
fixes. Classification below, verified by interactive QEMU probes (typed the exact
commands, slow + fast, on a fresh disk copy).

## Category A — assertion drift (system correct, marker moved) — FIXABLE by de-rot
| Test | Was asserting | Reality | Action |
|------|---------------|---------|--------|
| `boots_to_shell_prompt` | `"user_hello"` \|\| `"U-mode"` | retired ring-3 smoke cell; shell reaches `ViCell >` + VFS/Shell banners (all U-mode output) | ✅ FIXED — assert on `"ViCell >"` (U-mode-only print) |
| `gpu_framebuffer_initialises` | `"Framebuffer setup success"` | 0 files emit this string anymore; GPU path changed | ⏳ needs the current framebuffer marker (GPU device boot) — deferred |

## Category B — real bug: VFS write broken (BLOCKS ~10 tests) — NEEDS A FIX, not de-rot
Interactive probe (slow-typed, fresh disk): `vwrite /tmp/x AAABBB` → `failed to
write`; `vwrite /data/big.txt AAA` → `failed to write`; `vcat` then HANGS the shell.
Both RamFS (`/tmp`) and littlefs (`/data`) writes return non-Ok.
- Localized: request reaches VFS; `access.can_write` returns true for both prefixes
  (allow_write_all); MountTable longest-prefix routing resolves `/tmp`→RamFS,
  `/data`→littlefs correctly. So the failure is deeper — inside `backend.write`,
  the postcard decode of `Write{path,content}`, or reply delivery to the shell.
- **NOT caused by this session**: VFS `main.rs`/`dispatch.rs`/backends are in none
  of the net-campaign or input diffs; `/tmp` RamFS write touches no MMIO/block/
  timeout path that was changed. Pre-existing, masked while DHCP was dead (the
  whole suite failed at DHCP before, never reaching a `vwrite`).
- Blocks: `vfs_fat16_append`, `vfs_fat16_subdir_persistence`, `vfs_fat16_reboot_persistence`,
  `shell_test_builtin` (writes /data/tf.txt), `shell_source_script` (writes /data/run.sh),
  `network_wget_downloads_to_vfs`, `network_httpd_serves_file`, `network_httpd_dynamic_content`,
  and likely `shell_or_operator`/`shell_while_loop` (use vcat which hangs).
- **Recommended next task**: dedicated VFS-write debug session — highest leverage,
  one fix likely turns ~10 red tests green.

## Category C — real bug: input burst loss on long typed commands
Probe: fast-typed `case $STATUS in ok) echo CASE_EXACT …` echoed as `CASE_EXA` and
`;;`→`;` — dropped chars mid-line. The 2026-06-29 "burst keystroke loss" fix
(pending_msgs queue) reduced but did not eliminate it. Deterministic on long lines.
- Affects: `shell_case_statement`, `shell_echo_e`, `shell_function_positional_args`,
  `shell_exit_code_var`, `shell_source_multi_command`, `shell_variable_persists`,
  and the input-focused `input_bare_cell`/`input_keyboard_e2e`/`input_service_registered_at_boot`
  (need a live input path), `mqtt_subscribe`.
- Separate follow-up (input path); not de-rot.

## Category D — separate real bug (not a direct test failure but seen every boot)
- **Platform cell crash**: `Cell 1 terminated: scause=0xd sepc=0x10000032e stval=0x30000000`
  every boot — the Platform Cell faults loading the ECAM window at 0x30000000 on
  RV64 QEMU virt (no PCIe there). It exits anyway (one-shot), so DHCP/shell survive,
  but it should exit cleanly, not fault. Also `wget` cell exits with `scause=0xc
  sepc=0` (jump to null on return-from-main).

## CI allowlist (expand as tests de-rot)
Green + gated in `boot-suite`: `boots_to_shell_prompt`, `network_dhcp`,
`network_tcp_send`, `network_curl`, `network_tcp_listen`.

## Sequencing recommendation
1. **VFS-write fix** (Category B) — unblocks the largest cluster; do first.
2. Input burst loss (Category C) — unblocks the shell_* cluster.
3. Platform/wget clean-exit (Category D).
4. gpu marker (Category A) — cosmetic, last.
After each, expand the CI allowlist to the newly-green tests.
