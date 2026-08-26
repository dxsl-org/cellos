# Phase 09 — `NoEntry` fail-closed for P-TRUST-minting paths

- Phase file: `/home/dmin/cellos/.agents/260727-2101-midori-lessons-cellos/phase-09-noentry-fail-closed.md`
- Branch: `feat/wx-post-reloc-and-f1-signing` (no commits made, no rebase, no amend)
- Date: 2026-07-30

## What changed

| File | Change |
|------|--------|
| `/home/dmin/cellos/kernel/src/task/cap.rs` | `+28/-0` — `CapSet::path_mints_ptrust(path)` and `CapSet::without_ptrust()`, both directly under `with_path_caps`; the helper derives its answer from `with_path_caps` itself, so the match arms remain the only path list. |
| `/home/dmin/cellos/kernel/src/policy.rs` | `+140/-14` — `PolicyDecision::NoEntry { policy_loaded: bool }`; `decision_to_caps` strips P-TRUST when `policy_loaded && path_mints_ptrust`; `apply` emits the audit event + a serial-log line; `self_test` gained `no_entry_ptrust_cases`. |
| `/home/dmin/cellos/kernel/src/audit.rs` | `+11/-0` — `AuditEvent::PolicyNoEntryStripped = 26`, payload `encode_u32x2(tid, mask)` (bit0 pcie_driver, bit1 platform, bit2 supervisor). |
| `/home/dmin/cellos/scripts/sign-policy.py` | `+71/-2` — `ptrust_paths()` parses `with_path_caps` out of `kernel/src/task/cap.rs`; `assert_ptrust_covered()` runs unconditionally in `main()` next to `assert_round_trip`. |

Requirement 5 holds: no ABI item and no blob-format change. `build_body` / `decode_body` are untouched
and the baked blob is byte-identical (596 bytes, 23 entries) before and after.

## Requirement coverage

1. **Strip + audit on `NoEntry` for a minting path** — `policy.rs` `decision_to_caps`; the event is
   emitted from `apply` (the only place that knows the tid and the reason). It fires even when the
   request carried none of the three bits, because the coverage gap is itself the finding; `mask == 0`
   says so.
2. **Path outside the table unchanged** — the strip is gated on `CapSet::path_mints_ptrust`.
3. **`PolicyState::Absent` unchanged** — `lookup` maps `Absent | None` to `NoEntry { policy_loaded: false }`,
   and the strip requires `policy_loaded == true`. Pinned by a self-test row and by mutation B below.
4. **Bake-time gate** — a missing P-TRUST entry now aborts the bake before any file is written.
   CI exercises it: `scripts/build-boot-ramdisk-ci.sh:77` calls `sign-policy.py --out` on the rv64 lane.
5. **No ABI / format change** — see above.
6. **Boot 3 arch + peripheral demos + suite** — NOT VERIFIED, and not verifiable here: this box has no
   QEMU and no cross-gcc/objcopy, so no image can be assembled or booted. Substituted with an
   out-of-tree host harness (below) that runs the real self-test code and one end-to-end signed-blob case.

## Verification

All commands run from `/home/dmin/cellos`.

```
rustfmt --edition 2021 --check kernel/src/{policy,audit}.rs kernel/src/task/cap.rs   → clean
cargo fmt --all --check                                                              → clean
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf  -Z build-std=core,alloc → 0 errors
cargo check -p vicell-kernel --target x86_64-unknown-none         -Z build-std=core,alloc → 0 errors
cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc → Finished (links)
cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings → Finished
cargo check … --features policy-required                → 0 errors (cfg'd branch still compiles)
cargo check … --features policy-required,maintenance-mode → 0 errors
python3 scripts/cellos-sign --check --strict            → OK F5, OK F1 (77 crates / 340 files)
python3 scripts/test_cellos_sign.py                     → OK
python3 scripts/sign-policy.py --help                   → usage printed
python3 scripts/sign-policy.py                          → 596-byte blob, 23 entries (coverage gate passed)
```

### Bake-check break test (the gate bites)

Deleted the `("/bin/nvme", …)` row from `DEV_POLICY`, then:

```
$ python3 scripts/sign-policy.py --out …/BROKEN.BIN
P-TRUST paths missing from the policy table — the kernel would strip their privileged caps at runtime: /bin/nvme
exit=1
$ ls …/BROKEN.BIN → No such file or directory      # aborts before writing
```

Row restored; the re-baked blob is byte-identical (`cmp`) to the pre-break output, which also proves the
row went back in its original position (path order is part of the signed body).

Guard behaviour, so the check can never pass vacuously:

```
ptrust_paths(<file with no with_path_caps>) → exit "no paths parsed … check would be vacuous"
ptrust_paths(<missing file>)                → exit "cannot read … P-TRUST coverage check cannot run"
```

### Out-of-tree host harness (substitute for the boot gate)

`…/scratchpad/ph` — a std crate that `#[path]`-includes the REAL `kernel/src/policy.rs`,
`kernel/src/task/cap.rs`, `kernel/src/ed25519.rs` and `kernel/src/task/p_trust_selftest.rs`, stubbing only
`sync::Spinlock`, `audit`, `fs`, `cpu_features` and the TCB cap fields (`resource_registry` bit values
mirrored exactly). 9 tests, all passing:

- `policy::self_test()` returns true — the boot self-test with the three new rows actually passes
  (default posture AND `--features policy-required`).
- `p_trust_selftest::self_test()` still true; the three in-tree `cap.rs` unit tests still pass.
- `path_mints_ptrust` is true for all 8 minting paths and false for `/bin/app`, `/bin/vfs`, `/bin/shell`,
  `/bin/net`, `/bin/init`, `""`.
- End-to-end with a real dev-signed blob deliberately missing `/bin/nvme`: `apply("/bin/nvme", 42, …)`
  drops `pcie_driver`, keeps `spawn`, emits exactly one `PolicyNoEntryStripped` with payload
  `(tid=42, mask=0b001)`; the listed `/bin/block` keeps `pcie_driver` with no event; the unlisted ordinary
  `/bin/app` is unchanged with no event.

**Mutation checks** (copies of `policy.rs`, never the repo file):

- A — revert the strip (`let _ = policy_loaded;`): `policy::self_test()` fails with
  `[selftest] policy: FAIL — NoEntry kept privileged caps for /bin/nvme`, and the end-to-end test fails on
  "P-TRUST cap survived a missing entry".
- B — strip regardless of `policy_loaded` (blanket fail-closed): `policy::self_test()` fails with
  `[selftest] policy: FAIL — absent policy changed behaviour for /bin/nvme`.

So both directions of requirement 3 are genuinely pinned by in-tree code, not just by the harness.

## Concerns

- **Runtime is UNVERIFIED.** No QEMU / cross-gcc here, so "boots on 3 arches, 3 peripheral demos green,
  0 audit events in a standard boot" was not executed. The harness is the strongest available evidence
  and it exercises the real source text, but it is not a boot.
- **Intersection re-confirmed as empty, with a correction to the brief.** `with_path_caps` mints P-TRUST
  for **8** paths, not 7: `/bin/nvme`, `/bin/e1000`, `/bin/virtio-net`, `/bin/block`, `/bin/input`,
  `/bin/virtio-gpu` (pcie_driver), `/bin/platform`, `/bin/supervisor`. `/bin/e1000` was missing from the
  task brief's list; it IS in `DEV_POLICY` (`scripts/sign-policy.py:90`). All 8 have entries, so the new
  rule denies nothing today and no audit event should appear in a standard boot.
- **`PolicyState::Absent` untouched** — the distinction is carried in the decision itself
  (`NoEntry { policy_loaded }`) rather than re-derived, `lookup` sets `false` for `Absent | None`, and the
  strip is gated on `true`. Mutation B is the proof that removing that gate is caught.
- **Audit discriminant 26** is the next free byte (25 = `ThreadCapReached`). `audit.rs` already documents
  one collision from parallel branches both claiming 23 — if another phase in this session adds an event,
  re-check the byte before merging.
- **Shared working tree.** `scripts/check-baseline.sh` and `kernel/src/embedded/init` are modified by
  another phase; no overlap with the four files above, and nothing outside them was touched.
- One dropped line in `sign-policy.py`: the `DEV_POLICY` comment used to point at
  `.agents/…/phase-03-policy-cap-coverage.md`. Removed per the no-plan-references-in-source rule (and the
  path is gitignored, so it does not ship); the substance was rewritten inline.

**Status:** DONE_WITH_CONCERNS
**Summary:** A loaded-but-incomplete policy now strips only the privileged (P-TRUST) caps for paths whose
install path mints them, audits the gap, and leaves absent-policy and ordinary-path behaviour untouched;
`sign-policy.py` refuses to bake a blob that omits such a path.
**Verification:** rustfmt + `cargo fmt --all --check` clean; rv64/x86_64 `cargo check`, aarch64
`cargo build`, rv64 clippy `-D warnings` all clean, plus `--features policy-required` and
`policy-required,maintenance-mode`; `cellos-sign --check --strict` OK; `sign-policy.py` bakes a
byte-identical 596-byte blob. Break test: removing `("/bin/nvme", …)` from `DEV_POLICY` aborts the bake
with `P-TRUST paths missing from the policy table … /bin/nvme`, exit 1, no file written; restored and
`cmp`-identical. Out-of-tree harness ran the real `policy::self_test()` (9 tests green) and an end-to-end
signed blob missing `/bin/nvme` → `pcie_driver` dropped, `spawn` kept, one `PolicyNoEntryStripped(42, 0b001)`;
two mutations both caught.
**Concerns/Blockers:** Boot/QEMU verification impossible on this box (no QEMU, no cross-gcc) — runtime
claims remain UNVERIFIED. `PolicyState::Absent` kept unchanged by carrying `policy_loaded: bool` on
`NoEntry` and gating the strip on `true`. Re-confirmed the dangerous intersection is empty: 8 P-TRUST
minting paths (the brief listed 7, omitting `/bin/e1000`), all present in `DEV_POLICY`.
