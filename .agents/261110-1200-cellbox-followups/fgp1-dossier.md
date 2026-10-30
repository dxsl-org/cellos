# F-GP-1 dossier: Slice A (host-only) result

Date 2026-10-31. HEAD `11e44bf1c`. Scope: Slice A of [fgp1-diagnosis.md](fgp1-diagnosis.md) only. **Slice B was not run** (user priority change); no boot, no build of the lane image, no product source change.

## Conclusion

**Not reproduced in the vissh/sunset host harness. F-GP-1 stays an open finding (cause undetermined).** The real `vissh::session::run` client, driven with the lane's transport shape (partial/zero acceptance of 0/1/7/300/545/3840/4096 bytes per write, reads of 0/1/13/2048 bytes, stdin of at most 512 bytes per read with periodic `Idle`, 512 KiB pseudo-random blob with NUL/0xFF/ASCII heads), delivered every byte in order and the echo came back exact. No `BigPacket`/`BadDecrypt`-class failure appeared in any seed. This closes H1 *inside vissh/sunset over that policy space*; it says nothing about Net/smoltcp, the L2 bridge, slirp or the guest (H2-H4 remain open; they need Slice B's capture).

## What was done (observed)

Edited only `libs/vissh/tests/session.rs` (pre-edit SHA-256 `31a52d78…4f42d`, matched the draft; post-edit `ea8869d613deebf85f0c657f8841093be86e370794b79c11e613e26744401738`). Test-harness additions: `ToServer` records the server's input error (`tolerant`, `wire_err`) instead of panicking; `Me` gets `max_read`/`idle_every`; a `Lossy` transport wrapper; four new tests; `VISSH_SEEDS`/`VISSH_SEED0` env for soak.

`cargo test --offline --locked -p vissh --target x86_64-unknown-linux-gnu`: before 37 tests (args 4, kex 7, no_entropy 1, session 21, userauth 4), after **41** (session 25), all pass.

| Test | Result |
|---|---|
| T-partial (partial acceptance, short reads, idle stdin, 512 KiB, production rekey policy) | 2000 seeds clean (4,301,761 writes, 2,044,620 partial, 614,344 zero-acceptance) |
| T-partial-rekey (same policy, 160 KiB, rekey every 24 KiB) | 2000 seeds clean |
| T-busy (write error after N accepted bytes, read error after N calls) | 2000 seeds in the soak, plus 500 fresh seeds: always `TransportLost`, server stdin always a prefix of the input, no server error |
| T-sig calibration (injected faults, first server error class) | see below |

T-sig (measured): lost whole chunk (`DropChunk`), duplicated chunk (`DupChunk`), flipped top bit of the length field (`FlipLen`) -> first server error `BigPacket` (= OpenSSH `Bad packet length`); 7 bytes removed mid-packet (`DropMid`) or one bit flipped mid-packet (`FlipMid`) -> `BadDecrypt` (= `Corrupted MAC on input`). This confirms the diagnosis §0.2 reading for this library: the logged `Bad packet length 2270388607` means a packet-start desync (lost/duplicated packet or counter skew), not a mid-packet byte hit. Counts: 16 seeds all as stated; soak 2000 seeds DropChunk 2000/2000 BigPacket; 500 fresh seeds each fault 500/500 as stated. One caveat found by the soak: for lost/duplicated packets the decrypted length is key-random, so with probability about 35000/2^32 per event it fits Sunset's 35,000-byte buffer and the server reports `SSHProto` (block-size check) instead of `BigPacket`; it happened once (DupChunk, seed 1106, in the soak run before the tolerance was added). The test now allows `n/500` such in-range lengths for those two faults only.

Evidence texts (`evidence/fgp1/`): `slice-a-session-16seeds.txt`, `slice-a-session-soak-2000.txt` (the soak that hit the SSHProto event and stopped T-sig at DupChunk seed 1106), `slice-a-tsig-tbusy-500seeds.txt` (note: that job's output file was written by a run that finished after the interjection started; it holds the 500-seed result lines above), `slice-a-vissh-after.txt`.

## Slice B work reverted

Part of Slice B (driver loop feature, guest sampler, lane `pipe-diag` mode, judge `pipe-diag`, pcap analyzer) had been drafted in the working tree when the stop arrived. All of it was reverted with `git apply -R`; the pcap analyzer files were removed. Verified unchanged from HEAD: `scripts/qemu-tier3-ssh-guest-x86.sh` `cab76015…b603`, `scripts/tier3_ssh_guest_judge.py` `5f5de879…2aa`, `scripts/test_tier3_ssh_guest_judge.py` `f7135293…0ad9`, `tests/guests/x86-sshd/guest-init.sh` `4e69ca6b…6b22`, `scripts/prepare-tier3-sshd-initramfs-x86.sh` `de46476e…6846`, `cells/tests/phase04-terminal-driver/{Cargo.toml,src/ssh.rs,src/ssh/guest.rs}` `14af10d7…`, `1b3f1f9e…`, `8ab32452…`. Only working-tree change: `libs/vissh/tests/session.rs`. No keys, ISO, images or pcaps were created.

## Notes for whoever runs Slice B later

- `libs/vissh/tests/session.rs` is now dirty and changes the `libs/vissh` tree digest pinned in the lane (`f032373672fd…` -> now `f06e388bfa32…` with this edit); the lane's dirty-tree allow-list and that pin need updating in the same CP.
- HEAD moved: the lane's pins for `kernel/src/task/{scheduler,syscall,tcb}.rs` and `cells/services/net/src/{socket_table.rs,socket_table/tests.rs,service-runtime.rs,service-runtime-owner-death-tests.rs}` no longer match (NET-GCD/OSD); they need re-pinning (7 files, not 4).
- The pre-registered O-C/O-D/O-E labels need the B+ client counters and the B4 guest-only loop; a base boot can only separate O-A, O-B and "all clean" (O-C/D/E together), O-F.
- Not observed: anything about the Net cell, smoltcp, slirp, e1000 or the guest. `cargo clippy` and `cellos-sign --check` were not run (stopped by priority change).
