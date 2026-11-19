# Stack & Tile — verification (2026-11-11)

| Check | Result | Evidence |
|-------|--------|----------|
| `cargo test -p service-compositor --target x86_64-unknown-linux-gnu` | 89 passed | stacks, menu targets, gestures, tile groups, shared-edge resize, authority, lifecycle-past-configure |
| `cargo test -p service-input --target x86_64-unknown-linux-gnu` | 52 passed | modifier transitions on the pointer channel |
| `cargo check -p service-compositor --target riscv64gc-unknown-none-elf` | clean, no warnings | |
| `scripts/qemu-stack-tile-test.sh` (new lane, x86 KVM) | 12 PASS, 0 FAIL (before and after review fixes) | stack from menu, pick tab, Win+drag out, Win+drag beside (flush tile), group move; `/tmp/st-baseline/stack-tile-lane{,-review}.log` |
| `scripts/qemu-phase04-gui-test.sh` (x86 KVM) | PASS: 293 GUI checks, hashes unchanged (before and after review fixes) | equals baseline; `/tmp/st-baseline/gui-proof-{final,review}.log`. First run failed (menu Close moved below "Stack onto"); fixed by the section order Window, Close, Group |
| `tests/integration/tests/window-policy.rs` (RV64) | FAIL at step 3, same as baseline and clean `HEAD` | pre-existing (`reports/baseline.md`); `/tmp/st-baseline/window-policy-final.log` |
| Manual KVM desktop (Terminal, Files, Ocel) | stack/pick/detach/close menus, Win+drag ghost + target, tile flush, group move, shared-edge resize | screenshots `/tmp/cellos-play/{m*,o*,t*}.png` |

Shared RV64 artifacts (`disk_v3.img`, RV64 kernel) were rebuilt by `gen_disk.ps1` for the
window-policy runs and restored afterwards from `/tmp/st-baseline/*.bak`.

Known gap: Files and Ocel do not implement `WINDOW_CONFIGURE`, so in a stack or tile
group they keep their own size (they can still be closed and minimized).

## Independent review (haily-reviewer, 2026-11-11)

Verdict: conditional. All findings fixed and re-verified (host tests 89, both QEMU lanes above):

| Finding | Severity | Fix |
|---------|----------|-----|
| Maximize did not untile a tiled window | High | `actions::apply` untiles first; test `maximizing_the_middle_of_a_tile_group_untiles_it` |
| Input pointer stash (3) could overflow with modifier releases on unplug | Medium | `POINTER_STASH = 3 + 8` |
| Close all stalled / resumed stale queue | Medium | advance when the asked tab goes away unanswered; picking or closing a tab by hand clears the queue; test |
| Shown app of a shared frame could change other apps' windows; group restore woke self-minimized windows | Medium | client MINIMIZE/MAXIMIZE/RESTORE-from-maximized refused on shared frames; only group-minimized windows come back; test |
| Client move/snap/fullscreen kept tile joins; settle unbounded | Medium | such moves untile; hidden-tab moves ignored; proposals capped at `MAX_CONTENT` |
| Glue geometry could overflow on far-off windows | Low | saturating arithmetic, unsigned gaps; test |
| `settle` used hidden tab geometry | Low | only the shown tab settles |
| IPC regroup left the newly shown tab at a stale z slot | Low | `keep_place` swap |
| Grant ops reconciled before owner/success checks | Low | reconcile only after a successful own-surface op |
| Drop on a vanished target detached instead of cancelling | Low | nothing moves |
