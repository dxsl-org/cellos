# Phase02 CP1 amendment A2 — close the remaining serial/stream limitations

Status: **approved by the user (“Duyệt A2”) and implemented; verification in [the report](reports/phase-02-runtime-evidence.md)**. One edit outside the written list (`cells/tools/shell/src/top/render.rs`) was ratified by the user (“duyệt cả 2 việc”). Extends the file closure of the approved [Phase02 annex](phase-02-source-admission.md) (SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`) and amendment A1 (`ae6be1c3…`). No new syscall, selector, record, status, right, launch edge or rollout/CP2 change. Origin: the "still open" list of [phase-02-runtime-evidence.md](reports/phase-02-runtime-evidence.md).

## Added source closure (current SHA-256, before any A2 edit)

| Path | SHA-256 | Purpose |
|---|---|---|
| `kernel/src/task/drivers/uart.rs` | `fe56f3509ea9425653c70c5d5c6aca99760938a1359e9736aeb3529e65da7162` | stop rewriting CR→LF in the x86 RX ring (line 447) |
| `kernel/src/task/drivers/console_drv.rs` | `ad4ceb3d6b61cea2c34a756f425ca963bc59427543985be5f7ccab94e9e60ed0` | keep the legacy no-input-service fallback byte-compatible |
| `libs/api/src/services/session.rs` | `3a6c5bd64b62a4fe4a5c23ca24d448ac8030933b97d70e03bc6f3d55d520a9eb` | wipe `SerialDecoder.pending` after every completed/aborted sequence |
| `cells/tools/shell/src/{cmd_fs,commands,async_utils,cmd_cwd,shell,cmd_sys,top,snapshot_client,shell_state}.rs` | recorded at first edit | route builtin output/input through the session sink |

`cells/tools/shell/src/shell_test.rs` (test harness, 18 prints) stays outside. Existing admitted files `cells/services/input/src/session.rs` (remove the LF→CR canonicalization) and `cells/tools/shell/src/{main,executor}.rs` are already in the closure. No directory grant; no other file.

## Required behavior

1. **UART Enter stays CR (fixes Ctrl-J ambiguity).** The x86 RX interrupt path stores received bytes unchanged. The only reader of that ring is `console_drv` (`getchar`, `poll_rhr`). The relay to the input service therefore carries raw bytes (CR for Enter, LF for Ctrl-J). The legacy fallback that runs only when **no input service** is registered (`self.buffer.push_back` for `sys_read(fd 0)`) converts CR→LF itself, so every legacy consumer sees exactly today's bytes. Other architectures' `poll_rhr` paths already deliver raw bytes. Input removes its LF→CR mapping, so Ctrl-J becomes Key(`j`,Control) again. Regression: a pure kernel case proving raw relay vs legacy conversion, and an image check typing Ctrl-J (0x0a) then Enter.
2. **Physical echo-off remains un-enforceable by Cellos and stays refused.** The echo is performed by the host terminal on the other side of the UART, which no Cell or kernel can disable. A2 makes no change; physical sessions keep answering `Unsupported` to Echo-off and `read -s`, and evidence keeps the distinction. (Listed so approval does not imply otherwise.)
3. **SerialDecoder hygiene.** `clear_action` and the error/abort paths zero `pending` (and the paste state it covers); a host test proves no partial-sequence byte survives a completed or rejected sequence.
4. **Builtin I/O through the session.** Replace every `ostd::io::print/println/eprint*` in the listed shell files with the existing `executor::shell_print`/`shell_stderr` sink (capture-aware, SDK stdout/stderr streams in a serial session, legacy console otherwise). No output text changes. `rm -i` confirmation and `top` interactive quit read through `executor::terminal_read_line`/a single-key variant instead of fd 0 when a serial session is active, so they work and are cancellable; the A-closure guards that currently refuse them are then removed. Legacy GUI-shell behavior (no serial session) is unchanged. Regression: host tests for the capture sink and the guard removal; image check driving `rm -i` and `top` over serial.
5. **Not in scope:** designated-console principal for the physical UART in headless images, cross-process frontends, GUI/Ssh, operation `Completed`, issuer death on the live image, multi-hart, riscv64/aarch64 runtime — each needs a different design or approval.

## Verification and rollback

Re-run: host suites, kernel AP lane (+ the new raw-relay case), TCG and KVM image fixtures (extended with Ctrl-J/Enter, `rm -i`, `top`). Same private/fail-closed runner and quarantine rules as the approved annex; no canonical artifact, key or image is touched. Rollback reverts only these files; bytes already delivered over a UART cannot be recalled. This amendment is not CP2, rollout or acceptance approval.
