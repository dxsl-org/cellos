# Phase 03 runtime evidence — private signed x86 fixture

Scope: Phase 02 CP2 approved (see [dossier](../phase-02-cp2-dossier.md)); user asked to start Phase 03. Userspace only: `libs/text-engine::pipeline`, shell `typed.rs`/`native_rows.rs`/`parser`/`executor`/`jobs`/`shell_state`, cellbox `transform.rs`. No kernel/ABI/selector/lock change. Canonical `cellbox`/`terminal`/`kernel_fs.img` stay quarantined and were not rebuilt or signed; `cellbox` was deliberately NOT included in the private image (the signer refuses that name).

## Executed (2026-10-21)
- `cargo test -p text-engine` host: 74 pass. `cargo test -p app-shell` host: 44 pass (incl. `typed.rs` admission and `parser.rs` syntax tests).
- Bare-metal PIC compile: app-shell (also with `shell_test`), cellbox.
- Private image (`scripts/qemu-phase03-pipeline-test.sh`, TCG, throw-away key, F1/F5 checked) driving the real shell over serial: **15/15 observation checks pass** ([commands + raw output](phase-03-evidence/pipeline-commands.json)): select/where/sort-by/take/to over `ps --records`; Int/UInt comparison; typed-JSONL redirect and `from jsonl --typed` round trip; `ls --records` to JSON; `top --records --frames 3 | take 2` stops after 2 rows; admission refusals with no effects (rows into `grep`, `cd` in a pipeline, typed `&`); bad JSON, unknown field and bad comparison type fail; exact `cat` bytes.

## Not met / not exercised
- **Typed or byte stages cannot feed or consume external (spawned) commands**: the shell has no byte pipe to a spawned Cell (SDK `spawn_foreground` wires stdin to the keyboard owner). Such pipelines are refused before effects (status 2). Plan step 4 (external byte adapters over Terminal SDK endpoints) is therefore **incomplete**.
- Mixed native/external job identity, `jobs`/`fg`/`bg` transitions: typed pipelines are one foreground job; `fg`/`bg` return 2; background typed pipelines are refused.
- In-VM `shell_test` scenarios (early-stop pull counts, sentinel-file admission, numeric exactness) were compiled but **not run** on an image.
- Cellbox standalone transforms (`select`, `where`, …) are real implementations but were **not executed**: no private image contains `cellbox`; engine unit tests cover the shared logic.
- `2>`/`2>>` on typed pipelines is refused (no separate stderr channel); `2>&1` not supported.
- Accessors: `ps`/`ls`/`top --records` rows include an extra `cpu_tenths` field; `--typed` accepted on row stages.
- Phase 04 cutover, CP2 for Phase 03, canonical packaging and rollout remain blocked.

## Amendment A3 follow-up (external stages)
Implemented (`libs/terminal::pipe_child`, shell `ExtRun`) but **not qualified**: exit status of fast-exiting external stages is unavailable (`Stale`), and no stdin-reading external filter exists to prove byte exactness. See [A3 results](../phase-03-external-bytes-amendment.md). Plan step 4 remains incomplete; criteria unchecked.

## A4 result (supersedes the A3 follow-up above)
External stages now work with exact exit status, exact bytes, early consumer exit, streaming and Ctrl-C on the private image: **39/39**. See [A4 results](../phase-03-child-status-amendment.md). Remaining open: in-VM `shell_test` run, cellbox standalone transform execution, background/`fg`/`bg` (refused), `2>`.
