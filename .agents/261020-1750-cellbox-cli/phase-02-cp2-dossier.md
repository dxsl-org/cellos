# Phase 02 CP2 dossier — implementation review request
**Status: CP2 APPROVED by the user on 2026-10-21 (“Duyệt CP2 và bắt đầu Phase 03”) as scoped in “What CP2 would authorize”; quarantine, signing, packaging and rollout remain blocked.** Prepared 2026-10-21 from the saved evidence; no new execution besides hashing.
## Authority chain
CP1 annex SHA `73bc7483…ae67` + [A1](phase-02-publication-reservation-amendment.md) + [A2](phase-02-limitations-amendment.md) + user ratification of `top/render.rs`. All recorded in [phase-02-cp1-approval.md](phase-02-cp1-approval.md).
## Evidence
[phase-02-runtime-evidence.md](reports/phase-02-runtime-evidence.md): kernel AP lane 35/35; private signed x86 image 48/48 on TCG and KVM plus Ctrl-J, `top`+`q`, `rm -i` checks; host api/ostd/terminal-sdk/service-input/service-vfs/text-engine suites green; kernel compiles on x86_64/riscv64/aarch64. Raw sanitized records in `reports/phase-02-evidence/`.
## Not covered (see report): physical echo-off, kernel-attested console principal, cross-process frontends, issuer death on image, wall-clock commit/abort race, multi-hart, `cellos_g1_cpu`, riscv64/aarch64 runtime. Canonical artifacts (`cellbox`, `terminal`, `kernel_fs.img`) remain quarantined and were NOT rebuilt or re-signed.
## What CP2 would authorize (proposal)
Accept the source files below as the Phase 02 implementation baseline, and permit Phase 03 to consume the SDK/session contracts. It would NOT lift the quarantine, canonical packaging, signing or rollout, and does not close CBX-03..05 acceptance rows (those need the canonical image qualification of Phase 10).
## File inventory (current SHA-256)
| Path | State vs 78-path snapshot | SHA-256 |
|---|---|---|
| `Cargo.toml` | modified | `31d831c4aaa16ac648e1c3779dd0c1e6fbdae06decef7499584ecad7f4886872` |
| `cells/services/input/Cargo.toml` | unchanged | `3262831baa9c6a0063bd0b3e5d2138625c533909a4d35c97f2f791858b6063d7` |
| `cells/services/input/src/dispatcher.rs` | modified | `bdb22652757106cfb156843819f68087fc6c3e75900f41ddbf95a46831faefb5` |
| `cells/services/input/src/main.rs` | modified | `76bd2d3528740cc80a443706ae9dca19121ccd2d6b48237769ace1a2d811ca77` |
| `cells/services/input/src/session.rs` | new | `457ac5af04b34d65f8636bac8f6078dadee321e0915c0ad805bacc1033054c02` |
| `cells/services/vfs/Cargo.toml` | modified | `17181daa978837ada29d85935d89de1b05c1a5c565c5b219feb01cc1d9f79b03` |
| `cells/services/vfs/src/caller.rs` | unchanged | `2e24596951ab56adeae949ba52dbce90fc73960377170d54d90fd486789827b9` |
| `cells/services/vfs/src/dir_admission.rs` | modified | `3fccb72dc186d8491303ee8495cf1903df62818a572e8dda0ca7f7149330b559` |
| `cells/services/vfs/src/dirs.rs` | modified | `e21758be8d44cc2542e56ebab2e87c54c089ba407de2211b1b9c9cfa9f20fee7` |
| `cells/services/vfs/src/dirs/bind.rs` | modified | `1dffaea2502e83dc9935968e7712d83bc9934620480c674e71e284993b5e123f` |
| `cells/services/vfs/src/dirs/lifecycle.rs` | unchanged | `180d77137e7927501debe46b245ed452176bc78c5e128545a4851088b4132d24` |
| `cells/services/vfs/src/dispatch.rs` | modified | `8765de44f79c17048db328f5d11322f10bb100f6d06abbec26050e11529b83f0` |
| `cells/services/vfs/src/dispatch_dirs.rs` | unchanged | `7e90c82b94204d5a72f883dabfc2c93c7548969ca8a19d47bdae34a5ca63c443` |
| `cells/services/vfs/src/main.rs` | modified | `141362e5575168a9741499eb15cb8005640c216b5e6478b843b7ce6ad5dc5d4c` |
| `cells/services/vfs/src/prepared_cwd.rs` | new | `60bf161000253374ea006417690bd569ddb7bb7ca331d172a3e02ec071616b8d` |
| `cells/tests/phase02-session-child/Cargo.toml` | new | `007e829ccc583212ecc0aba1c62d8614b9cb1a63cca1d9436cfdd19dd93a531b` |
| `cells/tests/phase02-session-child/src/main.rs` | new | `d94485f0c9a63307163b721533d2e59a35db2757d5b409dc6862eeeab574f54b` |
| `cells/tests/phase02-session-parent/Cargo.toml` | new | `39f5405461ac19112ab2c81c34f68b489e4b553b7c5742af5c0c716f0b2b14f0` |
| `cells/tests/phase02-session-parent/src/main.rs` | new | `319f8d46fe3e9b3af3122830eb8c836811b6cc1726714d6c59bab06a8cebfb9a` |
| `cells/tools/init/Cargo.toml` | unchanged | `bd00edad39ca2e5c38c6453cf75c1556bf1168ed4cdc0b1a8fc42bc31fcf1d9c` |
| `cells/tools/init/src/boot.rs` | unchanged | `a33365dbe122a3388e65e4dee8e60f8be2692a77dc4f53979886a6e74817eaa4` |
| `cells/tools/init/src/configuration.rs` | unchanged | `b8e0ec2c09083525cfb3a08a0a2908b32d2dd9e7441c6eb2f01add093583ea2a` |
| `cells/tools/shell/Cargo.toml` | modified | `9e07bdddfad4a153beac0e4dda8e7ec54ad3618150faaff095586975b9cd0183` |
| `cells/tools/shell/src/executor.rs` | modified | `e0913acd1b5e036dc44935d0f5c1a250a8268d9dd4d67651086f41211acecb68` |
| `cells/tools/shell/src/main.rs` | modified | `7e0be7fbd730a80357685f89c540d76fb424a49bb2dbd24fdaaca65b4b3a8963` |
| `kernel/Cargo.toml` | modified | `bc59c142d71d4786317c57921ff2ddc0da464b7c8f54428bcf9dbd67d64193cf` |
| `kernel/build.rs` | modified | `c46adb3053a91e3513f525017f4cb69601a87c882f2e8195951c09a0518cc22b` |
| `kernel/src/cell/cspace.rs` | modified | `802e5fd71f91f0e1b8cd514e5b37ef7932850c8cc91f4a45b285de0bd965f178` |
| `kernel/src/loader/atomic_publication_tests/baseline.rs` | unchanged | `55b792ddce823a9e60630454877726e5395d207d74720d272d8f8d52a76fc621` |
| `kernel/src/loader/atomic_publication_tests/cases.rs` | unchanged | `55837ef0e780c916e3f64207fcd34d0b1297ba929798dca6eae3b54924041fb3` |
| `kernel/src/loader/atomic_publication_tests/denials.rs` | modified | `01eb9f25d19869a43ac707618e1c4feb8c1b6ff175ac8e6b95d0f2daf66139d3` |
| `kernel/src/loader/atomic_publication_tests/harness.rs` | unchanged | `55628cf059fee47790e6f08e1b3eff9621f68f4aedf05dcb9d5edc93bdac1af2` |
| `kernel/src/loader/atomic_publication_tests/probe.rs` | unchanged | `e780f0d3b61557d40a48487d9d76b5d0ad729c24a824af90753bcd67d8fa1374` |
| `kernel/src/loader/atomic_publication_tests/snapshot.rs` | unchanged | `da5536dee2a1d954dd81b113668e6c927cf298624f84de12625871b506c9810e` |
| `kernel/src/loader/atomic_publication_tests/success.rs` | unchanged | `d7c1b61b3eeeb876380741c09c466433e67b52e7828b76b58b3499e2d3d1bc5b` |
| `kernel/src/loader/governed_spawn.rs` | modified | `1a471279528715f6e70266677f4cad153eea52dc329198b560702e65047595db` |
| `kernel/src/loader/launch_profile/mod.rs` | modified | `53e9e41a0819117504f235f3d0be4377c58ad721a4b06ee055ce546ff3281bdb` |
| `kernel/src/loader/launch_profile/profiles.rs` | modified | `0e524309fff1538b12b7202ff4904c50006fd52603b35bc7f2ee91b2df637e55` |
| `kernel/src/loader/launch_profile/targets.rs` | unchanged | `8892a225c501e1c0ee9cec390a410c6cd9fb325db09f04043c6e214444ffd3f2` |
| `kernel/src/loader/launch_profile/tests.rs` | unchanged | `f22aeae72bc40f65ea8b8d60f72196d303655f7a06a14c9c155e2e13b5a7a265` |
| `kernel/src/memory/cell_quota.rs` | unchanged | `feab58aca5d6fbdcf2dd5c82aa70295773746473f594783f4e220568926cffdd` |
| `kernel/src/signing.rs` | modified | `aaff27aba0546647ca756c68ff17e58410d838cd3c14f40eb8f9326a8250f7d6` |
| `kernel/src/task.rs` | modified | `70885cb434c03e82ef2d56d2d5250fb513adf6f09db63b3c5ec32aed9aff4223` |
| `kernel/src/task/cell_session.rs` | new | `57aeb7d4cae61a99113a2bf6ed3750bee73ea38fe2fe555360cb9a2c022ccda7` |
| `kernel/src/task/child_context.rs` | new | `473cc6e6af6015400544a7a89704c59e736826eed62968e4ec65c20905f06b41` |
| `kernel/src/task/dir_inherit.rs` | unchanged | `4469db2da84ec2350a91aab2ac9cfec7b977227c7b82b1204d96790215bf2fae` |
| `kernel/src/task/elf_prepare.rs` | unchanged | `925a8df3564d381cb7f88ee707d44bd0fd220bc69514f405e8717dbcb89b8162` |
| `kernel/src/task/launch.rs` | modified | `248d7ef6c5fb3d7cc8725b74f5810e7331f44b4600b7d88950b4ab8c25a30c37` |
| `kernel/src/task/pipe.rs` | modified | `40e92c85ffb08b408e64e8230659795bcebba9266ba11a0e50ef1f93a4c2eae1` |
| `kernel/src/task/scheduler.rs` | modified | `e891b336509b09202767e5d57f4abf9a0b532bd8a055fd0ded7ade20053914e1` |
| `kernel/src/task/syscall.rs` | modified | `ea8443a69677eeaaa7979ee23c4814796ec9b9d7228b08683a58e367a726f130` |
| `kernel/src/task/tcb.rs` | modified | `9def966e688f158cb443a59870feb23a90dec50473b9e044a3f0c707673d6568` |
| `libs/api/src/abi.rs` | modified | `87c6eb425bc4b6f5d8f13c8b598044a45ea63eae70c57031ba166c4739dd90df` |
| `libs/api/src/abi/cell_session.rs` | new | `7c5530ee437820b6f5423775ecee7ed6702a8be296fb512902e42366791a97f8` |
| `libs/api/src/abi/syscall.rs` | modified | `6589f11c81408ab2b55df75693bf1d42067e497c6cdf38b89da44af893512c38` |
| `libs/api/src/abi/syscall_tests.rs` | modified | `2d32657a3189d5da7454a53221550f6d1402f686eec5d0c3e7d84886f40c03f0` |
| `libs/api/src/services.rs` | modified | `2c3182c7ec0147c72146ba23ff4c27382d55cbbc9d4197226b2d4161ee18812c` |
| `libs/api/src/services/ipc.rs` | modified | `b3bf300eb327eb7f2214c423eb7ac14abddc0b3e856bfe7a56731af8974f23c3` |
| `libs/api/src/services/session.rs` | new | `b8f902ea001ea350ca9b6389fa25fd1e997c1f95dddd70aaa9d1256f15250384` |
| `libs/ostd/Cargo.toml` | unchanged | `9b5c64aa81400b8151ff4d133faf6a24266dc704066eb23860905d4cfde41526` |
| `libs/ostd/src/child_context.rs` | new | `db3529fd4664b54fe5fd53461bb4bed8af1fff4b619677f26ec3eecb39237f9e` |
| `libs/ostd/src/clients/input.rs` | unchanged | `2f4adddf5040f602ce5fff34258ded250ef43b272eb97630f829aac0e29c50d7` |
| `libs/ostd/src/input.rs` | unchanged | `24d3b46e0e93eecdd7fb6da9d9b3c5ac8ce85f840e0a1f413cf684d320686e01` |
| `libs/ostd/src/lib.rs` | modified | `fd1c3782c058a4baf72728123afc0c7bb429cc1aa956ec4ef81db180f3414d5d` |
| `libs/ostd/src/session.rs` | new | `699159e3689e9b21ea7a1b743e23b3cb640536b4c8d58f5f4334e15de784597c` |
| `libs/ostd/src/syscall.rs` | modified | `0053200d678c9554a6c402f9d71bbeec3e82001e63cc0b2dc8a2a0d9ea9405fe` |
| `libs/terminal/Cargo.toml` | modified | `cb8d89dbdbad328045d98f5fd654d8aeff122d3abcee6d8c368b5e8f201b0e69` |
| `libs/terminal/src/codec.rs` | modified | `447c3c0a8af242793ecf48b38863b9c03415faf4901b5c29bbba3d898aa57282` |
| `libs/terminal/src/context.rs` | new | `cdb41ad1a7382911eade6a22df9c33c69977bdac2e3d465a83ebe09c9108899e` |
| `libs/terminal/src/controls.rs` | modified | `1ab45ad658356097edfbb69a4f27d53d48a1ae2f2199c974b679bed98ba5bb6a` |
| `libs/terminal/src/error.rs` | unchanged | `b4edc14bdc8a48c9823cf7cd3de2d14efcb9d3422439e5f691dcb0950bb5fc58` |
| `libs/terminal/src/input.rs` | new | `e7a7ca9b514f80597668a4d5f1e6b48c4a0547c495ead45a9c0e8deaaf9a0ed0` |
| `libs/terminal/src/lib.rs` | modified | `ba75fbdab88120d8c0eedc3888ed47703f2b4ae59de35eddc13d7408d3c5c2eb` |
| `libs/terminal/src/mode.rs` | modified | `2367782ad78c3fd047bbb763deac3716dce806bf455f210d1cdcaed51191b701` |
| `libs/terminal/src/session.rs` | modified | `1208e5fc9dcc03ce44ee5d214d0cda5908de719b3eaca047f58457451d6477c7` |
| `libs/terminal/src/stream.rs` | modified | `a05fc305f267c38af44fafcb046f9a0b364c2e7ce291851f8a659a03d559b212` |
| `libs/terminal/src/tests.rs` | modified | `213aa04f30f5b89e68a74cf744b7ed096b85126083d525dc6ee7cd3330429824` |
| `scripts/qemu-phase02-session-test.sh` | new | `d0861d1044b60a3ecaab3e41b9e2b88fbbfecaa6e5791ec2e5a26783874fd4ce` |
| `kernel/src/measurement_log.rs` | outside original 78 (admitted by A1/A2/ratification) | `45071c4ba42469590f328ba9297af6d6c5db0002bb02e7b68317da79c8cf8f6d` |
| `kernel/src/task/hart_local/ready.rs` | outside original 78 (admitted by A1/A2/ratification) | `47f65eabf212fcd95e66ee18e698ee66e3c0a012feaa4d10baa5088927e7506e` |
| `kernel/src/task/drivers/uart.rs` | outside original 78 (admitted by A1/A2/ratification) | `b70d23ff76bc6c49813a3c567d80ec2fac86507e82acb55f2fc6d0da63afdeed` |
| `kernel/src/task/drivers/console_drv.rs` | outside original 78 (admitted by A1/A2/ratification) | `9283611d03bfd7d0715cd239ff9b81d3bd311ef5f19955b96a34b51b7423848e` |
| `cells/tools/shell/src/top/render.rs` | outside original 78 (admitted by A1/A2/ratification) | `da5814b57f6a2678ce6636945fd8ad2fe898f932594b19a1939bea94335904c6` |
