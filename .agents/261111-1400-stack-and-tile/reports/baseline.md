# Baseline before Stack & Tile (2026-11-11)

Tree: `main` at `c2776928b` plus this session's uncommitted chrome work
(icon+title tab, window menu, SET_ICON, soft shadow).

| Lane | Tree | Result | Evidence |
|------|------|--------|----------|
| `scripts/qemu-phase04-gui-test.sh` (x86 KVM) | working tree before Stack & Tile | 295 PASS, 0 FAIL | run earlier this session (menu-close flow included) |
| `tests/integration/tests/window-policy.rs` (RV64) | working tree before Stack & Tile | FAIL at step 3: `front probe did not start` (20 s) | `/tmp/st-baseline/window-policy.log` |
| `tests/integration/tests/window-policy.rs` (RV64) | `HEAD` (`c2776928b`, clean worktree `/tmp/st-head`) | FAIL, same step, same message | `/tmp/st-baseline/head-window-policy.log` |

## window-policy failure (pre-existing, not caused by this plan)

The test types `window-policy-probe front &` on the serial console after the
`back` probe opened its window. The `back` probe logs `[window-policy-probe back] key`
— the typed line reached the focused window, not the shell — and the shell
never starts `front`. `HEAD` fails identically, so the failure predates this
plan. Stack & Tile checkpoints compare against this state: the lane must not
fail *earlier* than step 3.

## Build notes

- `gen_disk.ps1` rebuilt `disk_v3.img` and the RV64 kernel from the working tree;
  originals saved at `/tmp/st-baseline/{disk_v3.img,cellos-kernel}.bak`.
- Optional cells `doom` and `tetris-lua` fail to link on this host
  (`rust-lld: unable to find library -lc`): missing newlib, unrelated.
