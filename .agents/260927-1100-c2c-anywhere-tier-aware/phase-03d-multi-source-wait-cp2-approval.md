# C2C Phase 03 slice d CP2 approval record (Law-1 confirmation 2 + closure amendments)

Recorded 2026-11-02. The accountable maintainer/owner (the user) answered **YES**, through the integrating session, to the question of `reports/phase-03d-cp2-package.md` §8:

> Do you approve the implemented delta of `libs/api/src/abi/completion.rs` (`45656c0b2225a38a…`) and `libs/api/src/abi/syscall.rs` (`d0388f9e995da1b0…`: documentation plus the `__vicell_syscall_allowlist` export), with the kernel, SDK, test and lane changes and the evidence in this package, closure amendments A1–A5 and deviations D9–D17? On approval, the Spec 17/20/03b edits, `CHANGELOG.md` and `scripts/check-waitcompletion-ipc-law1.sh` land in the same change and the surface is FROZEN.

Recorded values:

| ID | Decision |
|---|---|
| C2-1 | Law-1 confirmation 2 given: the CP2 §1 public ABI delta is FROZEN. It covers `source::IPC = 1 << 2`; `is_wait_mask_supported`; mixed `NET_RX \| TIMER \| IPC` masks on `WaitCompletion = 242`; one source per v1 24-byte record; IPC `result` = operation token; and the exported `__vicell_syscall_allowlist` symbol. |
| C2-2 | Closure amendments A1–A5 (CP2 §0) ratified: `completion_selftest.rs` assertion, bench-probe test allowlist, the CP2 package, `libs/ostd/src/{lib,runtime}.rs` export, `kernel/src/task/completion.rs` pre-sized waiter table. |
| C2-3 | Deviations D9–D17 (CP2 §6) accepted as recorded, including D16 (`qemu-lua-vnet` blocked by pre-existing, slice-independent faults) and D17 (the `igb-x86` image is private, not canonical). |

**Rulings recorded with the approval (integrating session instruction, same day):**
- `CHANGELOG.md` is **not** edited in this slice, although the §8 question listed it. A later change owns the entry.
- The digest-pin script is named `scripts/check-multi-source-wait-law1-digests.sh`, not the `check-waitcompletion-ipc-law1.sh` named in §8. It follows `scripts/check-lookupservicebound-law1-digests.sh`, and like that script it is referenced from the specs and evidence rather than a CI job: no workflow invokes the precedent (`.github/workflows/ci.yml` runs only `check-ai-law1-digests.sh`, at line 1536).
- No commit is made. The slice stays uncommitted for the integrator.

**Authorizes:**
- the FROZEN surface above; any later removal, rename, value or layout change, or added source needs the ABI process again (two fresh confirmations);
- the held documentation edits: Spec 17 §9 (2026-11-02 entry), Spec 20 v3.4 (§2.4, §2.6, §3, §4, §5, §7) and Spec 03b (overview, Point 1, deferred sources);
- the digest-pin script and the record updates (`phase-03-async-ipc.md`, `plan.md`, `plan-portfolio.md`, `cell-contract-0024-locks.md`, `.agents/TODO.md`).

**Does not authorize:**
- the broker `NetworkPoller` cutover (D-5: a separate slice, its own CP1);
- an x86 NET_RX producer, remote/relay/pipelining, physical or production claims;
- canonical image, signing policy or `POLICY.BIN` changes;
- release of any whole-plan lock relation (all 38 stay), or any further edit under the slice-d file-bounded exception, which ends with this record.

**Post-confirmation edits to bound source (formatting only):** `cargo fmt --all` re-laid out four slice files after CP2 §5 was hashed. rustfmt does not change meaning, and no other file changed. The final digests below supersede CP2 §5 for:
- `kernel/src/task/async_ipc.rs` (`96e9582799d96064…` → `d04166eb…`);
- `kernel/src/task/completion_wait.rs` (`46d8727f0242b535…` → `670279bc…`);
- `libs/ostd/src/ipc.rs` (`26a353cf9f8976b3…` → `a6f58e02…`);
- `cells/tests/bench/src/scenarios/async_lifecycle.rs` (`73ae1da015f29735…` → `e89a8db4…`).

The lane ISOs and the evidence log were built from the pre-format bytes. The two confirmed `libs/api` files are byte-identical to the §8 digests.

**Verification at binding (2026-11-02, HEAD plus the uncommitted delta):**
- `bash scripts/check-multi-source-wait-law1-digests.sh`: PASS. A copy with `IPC = 1 << 3` fails with rc=1 and one per-item message.
- `bash scripts/check-lookupservicebound-law1-digests.sh`: PASS.
- `cargo fmt --all --check`: clean.
- clippy `-D warnings`, rc=0:
  - riscv64 workspace (CI exclusions, `-Z build-std=core,alloc`);
  - `-p cellos-kernel` on `aarch64-unknown-none-softfloat`;
  - `-p cellos-kernel` on `x86_64-unknown-none`.
- Host tests, `--target x86_64-unknown-linux-gnu`:
  - `-p api`: 184 + 2 (doc tests: 4 ignored);
  - `-p ostd`: 86 + 7 + 19 (2 ignored);
  - `-p cellos-kernel`: 392.
- Lanes as recorded in CP2 §4 and `docs/evidence/c2c-multi-source-wait-x86.txt`: lifecycle 3/3; `igb-x86` 2/2 (private image); 0 `denied for tid`.

**Source HEAD at binding:** `0effc2250688cc351d64dd2733fcef66f9941939`. The diff from HEAD is uncommitted:
- source: 19 files under `kernel`, `libs`, `cells`, `tests` (+2377/−79, after formatting);
- documents: specs 03b/17/20 and the `.agents` records;
- untracked: the slice documents, `reports/`, `docs/evidence/c2c-multi-source-wait-x86.txt` (the `.log` is git-ignored), the new script, and the pre-existing `.agents/archive/`.

**Digest form:** SHA-256 of whole files including the final newline, with no normalization; no digest is stored inside the file it hashes.

## Bound documents and source
| Path | SHA-256 |
|---|---|
| `.agents/260927-1100-c2c-anywhere-tier-aware/reports/phase-03d-cp2-package.md` | `8a32ab56c99e99f598b43e10c55837df8832de51f7d069334bd618fb01b74789` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/reports/phase-03d-prototype.md` | `bb5ad68c8ff8ee24f5734d1f9f5d74579a2ef816ee6d50649940fa7a3ddd455b` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp1.md` | `f74b9a0f652723a1149c57df60dee70dc622626d38a0686a3f5f3c29c49d349d` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp1-approval.md` | `67eab7028c35b92bbce817cc0f4df8c61c137d7643783117ce4c070d9cb8b482` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | `691e0dda9967aa818409e1892607494c328c3d80444e2e1981354caf3333aba8` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md` | `3c09006649e0fcbd011b03ef4146079cce6af3bc699b93bad2a944804c4b8b18` |
| `.agents/cell-contract-0024-locks.md` | `ba004baa4fb5a9a2cef0d6dca24bb4091e1a212adf7c6c3805ec533ce153ad03` |
| `.agents/plan-portfolio.md` | `f66fdbc57e4fe27f58d79e32d9e2e61c117ccb5e3c3223a8ae219fd397a4f1eb` |
| `.agents/TODO.md` | `e2f81b776d63bb39f3b1c17527cb35492c6d8b384d76fdbd7dab9b025a56e9c6` |
| `docs/specs/17-ipc-wire-contract.md` | `916f4cb1db49108632d31080676a2b38b027c94bb46a89281798e676ea4095dd` |
| `docs/specs/20-unified-ipc-contract.md` | `4dc8b4cd904bcd9ec0b0088ab1efd4ef51942d6788648441b00bf979d0f18e75` |
| `docs/specs/03b-async-reactor-adr.md` | `ed65a300315c35f0514ce6e00ae97709fe4fcb3e01f11d6e05cbb3a229af4fa8` |
| `docs/evidence/c2c-multi-source-wait-x86.txt` | `f393d956547e4fc38b0b226d5e76f7b703075a9c7071ca4024dce9ab37a82454` |
| `docs/evidence/c2c-multi-source-wait-x86.log` | `6d1b13c9e15cfed786c0a877f3b332f9f57b8420918a50b370083ad9626262c1` |
| `scripts/check-multi-source-wait-law1-digests.sh` | `cbaeadfa85fa992ca786186931a21256c2535b26b6b5bcc1c615cc134d20850a` |
| `libs/api/src/abi/completion.rs` | `45656c0b2225a38a7dac7c82ee74de2fdadcba02fe7571cb6f865bc49c4c64c2` |
| `libs/api/src/abi/syscall.rs` | `d0388f9e995da1b0df93b2c509ee3468022601ad742a0e0a78c4dac0db3d98a9` |
| `libs/ostd/src/ipc.rs` | `a6f58e02d29fef3423bd2df918458f6266788e07afe2424ee42c144d3f02db64` |
| `libs/ostd/src/lib.rs` | `10750738a023f1181ec94b2b8a102305fec4e9fbae7f254327b2e0cd4bd1bc89` |
| `libs/ostd/src/runtime.rs` | `879b0e9c12e1eae0940302aef0c5c4330c669057fb09abeb37fc492959a3b286` |
| `libs/ostd/src/syscall.rs` | `e672f9904a653caf7b2c3666b7177e7788cc8548b2a85169af5af6f8d29861d2` |
| `kernel/src/task.rs` | `e78bbb3413cba46ba3c29501b4c969da1c3aa484c1095e2ec45d1c465d1b1c88` |
| `kernel/src/task/async_ipc.rs` | `d04166ebd27606266fde5ca12e1f2f627f35e3338105eaf1bcd2b38ac2583912` |
| `kernel/src/task/completion.rs` | `c3e3388b32ef8ab3e4c9cbaab628beab8d5ca1e7ec4a740fdee6992133d5e3db` |
| `kernel/src/task/completion_selftest.rs` | `8d16f45ea69d567950648cf2a13cba19058dd46d05f793d3f03d37d4fdab3dca` |
| `kernel/src/task/completion_wait.rs` | `670279bc3cc6807d587c0490faaf3eeea19d68b003498ee6b04e71aec85809d1` |
| `kernel/src/task/ipc_pending_selftest.rs` | `2e6ca23137b759659b2f6f175acac92c425a1ea3188498889541462e82aa5792` |
| `kernel/src/task/scheduler.rs` | `8267292a2c5d4d3b0e3dc5d53a3094fdd043b2be363bd7e73da373483173f8c2` |
| `kernel/src/task/tcb.rs` | `39add25c90710faac2fe1758917723b578414c2a7e0a0be9e9209289ec375c0c` |
| `kernel/src/task/waker.rs` | `91a429c2e81f9b293d227fcc7d38a2c67b5ff5342aedc1927a76c7c24a041e76` |
| `cells/tests/bench/src/bench-probe.rs` | `7bf32daeeca502e4df24ff2cb0522f1dd997654c622b1bbef253a6fa65e54d95` |
| `cells/tests/bench/src/scenarios/async_lifecycle.rs` | `e89a8db4cc137b3281ca4d5218650ae41030eee2066806401f05434b01cd4e2b` |
| `tests/integration/src/lib.rs` | `fa3122e1fb60348eb36b503979a21d4018c1650a718c0ab6d14954182754acfd` |
| `tests/integration/tests/local-service-lifecycle-x86.rs` | `0914cb9e71e1e141b0f0c68f07b9199720b58c8f805c989d8fbd7b893b284713` |
