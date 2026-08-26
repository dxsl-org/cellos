Verdict: complete. The `/bin/vfs` region fold is implemented, the loader post-policy raw grant is removed in `kernel/src/loader.rs`, signer/policy/request paths all carry bit 3, and the bounded VFS and `/srv` runtime lanes passed once the test-hooks artifact-state issue was removed from the lane.

Exact diff summary:
- `kernel/src/task/cap.rs` `+16/-5`: `/bin/vfs` now requests the cell-store region in `CapSet::with_path_caps`; self-test expects `block_regions == 0b1111`.
- `scripts/sign-policy.py` `+20/-2`: `/bin/vfs` policy entry widened to `0b1111`; host-side decode gate now exits unless baked `/bin/vfs` decodes as `0b1111`.
- `kernel/src/policy.rs` `+8/-5`: parser/self-test pins 4-bit block-region support and checks a v2 entry can preserve `0b1111`.
- `kernel/src/loader.rs` `+17/-10`: runtime success log for live `/bin/vfs block_regions=0b1111` kept; legacy `task.block_regions |= 0b1000` removed; blocking review fix added a pre-apply deny path that exits the spawned task and then deregisters `cell_quota` before returning `PermissionDenied` when `granted.block_regions != 0b1111`.
- `kernel/src/loader/boot_ceiling.rs` `+6/-5`: stale `VFS_REGIONS` comment updated to describe the actual request ∩ ceiling ∩ policy flow.

Assumption evidence:
- Host decode in the takeover worktree proves `/bin/vfs` now bakes as `('/bin/vfs', 1, 0, 0, 0, 0, 15, 0, 0, 0)`.
- `scripts/build-test-hooks-ci.sh` and `scripts/build-srv-test-ci.sh` both bake `/POLICY.BIN` into their runtime images; `inspect_fat.py` confirms `SFN POLICY.BIN` in `kernel/src/embedded-test-hooks/kernel_fs.img` and `kernel/src/embedded-srv-test/kernel_fs.img`.
- The debugger conclusion for the earlier red VFS lane was artifact state, not a product-code quota regression: a dirty/misaligned test-hooks image produced the false failure; a clean test-hooks rebuild produced `68 PASS, 0 FAIL`.
- The loader marker remains useful as a guardrail, but this run does not overclaim it as standalone proof of the full authority chain; the decisive evidence is the clean rebuilt runtime lane plus the baked policy decode/build checks.
- The blocking domain-review finding is closed: the VFS invariant no longer returns after `spawn_from_mem` with a live task/quota entry. The deny path now runs before task mutation, uses the established `sched.exit_task(...)` path under the scheduler lock, drops that lock, then calls `crate::memory::cell_quota::deregister(cell_id)`.

Tests and checks:
- Pass: `python3 scripts/sign-policy.py --out /tmp/midori-vfs-policy.bin`
- Pass: clean `bash scripts/build-test-hooks-ci.sh`
- Pass: clean `cargo test --target x86_64-unknown-linux-gnu --test vfs-quota -- --nocapture`
  - Guest output: `68 PASS, 0 FAIL`
- Pass: `bash scripts/build-srv-test-ci.sh`
- Pass: bounded `/srv` runtime lane: all three `redoxfs-srv` tests passed
- Pass: independent tester re-ran:
  - `cargo check`
  - `cargo test --target x86_64-unknown-linux-gnu --test vfs-quota -- --nocapture`
  - `bash scripts/build-srv-test-ci.sh`
  - `cargo test --target x86_64-unknown-linux-gnu --test redoxfs-srv -- --nocapture`
- Pass: review-fix verification lane in this worktree
  - `bash scripts/build-test-hooks-ci.sh`
  - `cargo test --target x86_64-unknown-linux-gnu --test vfs-quota -- --nocapture`
- Expected environment failure in this worktree run: `cargo check`
  - Failure cause: pre-existing workspace build environment issues in unrelated Lua/Tetris C builds (`signal.h` missing for the bare-metal toolchain)
  - Classification: environment/workspace issue outside the owned VFS files, not evidence against the loader fix
- Expected environment failure only: `cargo test -p vicell-kernel cap`
  - Failure cause: this `no_std` host-test configuration cannot find crate `test`
  - Classification: test harness/environment limit, not a phase-specific product regression

Caveats:
- Nested checkout admin state is still miswired for Git in WSL/PowerShell: `.worktrees/midori-vfs-region-fold/.git` points at a UNC admin path, while the real admin dir exists at `/home/dmin/cellos/.git/worktrees/midori-vfs-region-fold`. I classified this as generated checkout dirtiness and did not delete it.
- Generated artifact dirtiness remains possible in the worktree from image-build scripts, notably files like `kernel/src/embedded-test-hooks/init`. The earlier false red VFS lane came from artifact state, so future reruns should prefer a clean rebuild before reading a guest failure as code regression.
