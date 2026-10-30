# KSSH cleanup item 2 CP1: repair the G-S1a ceiling gate `scripts/cellbox-ceiling-diff.py` (proposal; documents only; nothing applied)

Written 2026-10-30 at HEAD `ca4b872e6361174af46b8984a0f02bcdbc84a76f`, `git status --short` empty when read. **Documents only.** No source, script, lock, key, image, network or commit action was made. Commands run: `git show/log/diff/archive/merge-base/apply --check`, `grep`, `sha256sum`, the ceiling script itself, and read-only experiments in throw-away `/tmp` directories (exported copies, removed afterwards). Everything marked **observed** was run in this session; `[INFERENCE]` marks the rest.

## 0. Summary

| | |
|---|---|
| Defect | `python3 scripts/cellbox-ceiling-diff.py` with no arguments exits **2** at HEAD: its default baseline `git show HEAD:cells/tools/shell/src/main.rs` names a path the Cellbox commit `5043e2cf5` deleted (observed: `ceiling-diff: error: git show HEAD:cells/tools/shell/src/main.rs failed: fatal: path 'cells/tools/shell/src/main.rs' does not exist in 'HEAD'`, exit 2). |
| Broken callers | `scripts/qemu-phase04-terminal-test.sh:259` and `scripts/qemu-phase04-gui-test.sh:243`. Both run under `set -euo pipefail` through `| tee`, so exit 2 aborts the lane before any build. `scripts/qemu-phase09-ssh-test.sh:212` is already pinned (K8) and passes. |
| Right fix | **Pin the default baseline inside the script** to `2f0bc06599808be3ab0e4b3df0d79ea08c5317aa` (parent of `5043e2cf5`, the last tree that has the shell). One file, no runner edit. There is no "current location" of the shell ceiling to read: the shell is retired, so the comparator must be a pinned historical tree. |
| K3 delta | The script's ceilings **do not and need not** include the K3 delta. It compares only `declare_manifest!`/`declare_syscalls!` of `cells/tools/cellbox/src/main.rs`, which no KSSH slice changed. K3's right (`kms_ssh_sign`) is a kernel `CapSet` field and launch-edge ceiling, guarded by the digest-pin gate in `qemu-phase04-terminal-test.sh:317-330` (all five pins equal the current files, observed). |
| Closure | `scripts/cellbox-ceiling-diff.py` (+4/-1 lines) and `CHANGELOG.md` (one `### Fixed` bullet). No Law-1 object, no lock row names either file. |
| Proof | Gate passes on the current tree (exit 0) and fails (exit 1) on a deliberate widening: added syscall, widened manifest, removed syscall (all observed in an exported copy, section 6). |

## 1. Findings (file:line evidence)

**F1. The script's default baseline no longer exists.**
- `scripts/cellbox-ceiling-diff.py:6` (docstring) and `:26` `DEFAULT_BASELINE_PATH = "cells/tools/shell/src/main.rs"`; `:93` `--baseline-ref` default `"HEAD"`; `read_baseline` (`:77-87`) runs `git show <ref>:<path>` and maps a failure to `ExtractionError` → exit 2 (`:107-109`).
- `git log -- cells/tools/shell/src/main.rs` ends at `5043e2cf5` "replace cells/tools/shell with the modular Cellbox multicall suite" (2026-10-29). The script itself was last changed by `dc4919c0a`, before that commit.
- The candidate side is fine: `DEFAULT_CANDIDATE` (`:25`) `cells/tools/cellbox/src/main.rs` exists (sha in section 4).

**F2. Callers (grep over the tree excluding `.git`, `target`, `.agents`; `.agents` documents only quote the script).**

| Caller | Line | Call | State at HEAD |
|---|---|---|---|
| `scripts/qemu-phase04-terminal-test.sh` | `:259` | default args, `\| tee "$EVIDENCE/cellbox-ceiling.txt"`, `set -euo pipefail` at `:147` | **fails exit 2**; runs before `cargo tree` (`:261`) and every build (`:361+`) |
| same | `:267-269` | `cells/apps/terminal/src/main.rs --baseline-ref HEAD --baseline-path <same> … \|\| true` (informational) | exit 0 but vacuous, see F4 |
| `scripts/qemu-phase04-gui-test.sh` | `:243` | default args, `\| tee`, `set -euo pipefail` at `:103` | **fails exit 2**, before image work |
| same | `:244-251` | terminal and files informational deltas against `HEAD` | exit 0 but vacuous, see F4 |
| `scripts/qemu-phase09-ssh-test.sh` | `:135-136`, `:212` | `--baseline-ref "$CEILING_BASELINE"` = `2f0bc06599808be3ab0e4b3df0d79ea08c5317aa` | **passes** (observed: baseline 32 syscalls, candidate 34, 6 manifest keys, `G-S1a PASS`, exit 0) |

No other caller: `qemu-phase02-session-test.sh`, `qemu-phase03-pipeline-test.sh`, `qemu-phase10-canonical-test.sh`, `phase10_canonical_drive.py`, `phase04_gui_drive.py`, `test_phase10_candidate_import.py` (its `RUNNERS` list at `:39-40` indexes runner text for other guards, not this call) do not run the script. No CI file, Makefile or test references it. The runner header comment `qemu-phase04-terminal-test.sh:133` only names it. [INFERENCE for the grep's exhaustiveness: three `*.img` files over 256 MiB were skipped by grep; they are not text.]

**F3. History of the gap.** `k8-cp1.md:18,65,431` found it, pinned only the ssh runner (K8 closure) and left the terminal/GUI calls as decision D8-7 "out of K8 (reported only)". `plan.md:83-84` lists "ceiling-diff calls at HEAD" as open outside the program. The Phase 04 CP2 dossiers still say the gate "passes against HEAD's `cells/tools/shell/src/main.rs`" (`phase-04-s1-s3-cp2-dossier.md:40`), true only before `5043e2cf5`.

**F4. The informational Terminal/Files calls are now vacuous.** Both compare the working-tree app file with `HEAD`'s copy of the same file. The Terminal `SetTimer` and Files `VfsMutate` deltas were committed in `2f0bc0659`, so on a clean tree they are always `G-S1a PASS` (observed). Against `8fe158478` (parent of `2f0bc0659`) they reproduce the historical deltas (observed: Terminal 13→14 `ADDED SetTimer`; Files 13→14 `ADDED VfsMutate`), which is what archived logs show as "informational `G-S1a FAIL`". They are `|| true`, never gates. Not a regression of this item; recorded for decision D-2.

**F5. What the gate checks, and the K3 question.**
- `extract` (`:55-74`) strips comments (`:33`) and reads `declare_manifest!(…)` key/values and `declare_syscalls![…]` identifiers from one file; fails on a missing, duplicate or malformed entry (exit 2, fail-closed). Added syscalls outside `--allow-added` (default `Write`, `GetRandom`, `:94`), removed syscalls and any manifest key change are problems (exit 1).
- Candidate `cells/tools/cellbox/src/main.rs` was last changed by `5043e2cf5`; `git diff --stat 5043e2cf5 HEAD` over it is empty (observed; only `cells/tools/cellbox/Cargo.toml` changed, +7). K3, K5, K6, K7, K4b, K8 therefore added no syscall and no manifest key to the cellbox ceiling. Its ceiling at HEAD = shell ceiling (32) + `Write` + `GetRandom` = 34, equal to what `k8-cp1.md:65` recorded.
- K3's delta is `kms_ssh_sign` / `kms_ssh_admin` on `CapSet`, the `SSH_CLIENT_EDGE` launch ceiling and policy v8. `k3-cp1.md:213`: "`cellbox-ceiling-diff.py` is a syscall-ceiling check and stays unchanged by K3". That delta is held by (a) `launch_profile/tests.rs` host tests and (b) the digest pins `qemu-phase04-terminal-test.sh:317-330`, re-pinned for K3 and K7 in the comment block `:308-316`. Current digests of those five files equal the pins (observed): `mod.rs b8db3b80…ec04c`, `profiles.rs 78058780…2930`, `targets.rs 45834cae…a48f`, `tests.rs b41874eb…c3d`, `libs/api/src/abi/cell_session.rs 590fe5cc…1595`.
- The script also does not cover other cells' ceilings (`kms-ssh-admin`, sys-tools, net-tools).

**F6. Why a pin, not "read the current location".**
- `HEAD:cells/tools/cellbox/src/main.rs` as baseline is vacuous on a committed tree (it compares the file with itself) and would only notice uncommitted edits. The gate's meaning (script docstring `:2`) is "the cellbox ceiling must equal the shell ceiling it replaces" plus allow-listed approved additions; that needs the shell's last tree.
- `2f0bc0659` is an ancestor of HEAD (observed, `merge-base --is-ancestor`), repository not shallow (observed). A missing object still fails closed with exit 2, never a silent pass.
- The lane runs the candidate from the **working tree**, so a dirty-tree widening is caught at lane time; the baseline never moves.

**F7. Freeze coupling.** `reports/k8-evidence/frozen-sources-rt1.sha256` lists `scripts/cellbox-ceiling-diff.py` (`:15`), the terminal runner (`:12`), the GUI runner (`:13`) and the ssh runner (`:44`). `sha256sum -c` on the RT1 file passes now (observed rc 0). The T0 file `frozen-sources.sha256` already fails for six later-edited files (ssh runner, `kssh.rs`, `kssh_keys.py`, `kssh_judge.py`, `test_kssh_judge.py`, `CHANGELOG.md`; observed). The K8 runner refuses reruns first (`RUN_CONSUMED-k8-rt1` exists; check at `qemu-phase09-ssh-test.sh:202`, freeze check `:205-207`), so a script edit invalidates no runnable K8 check, but the RT1 freeze record stops matching the tree; that is expected and is recorded, not repaired, in the CP2 dossier.

## 2. Exact minimal fix (not applied)

One file, `scripts/cellbox-ceiling-diff.py`, applied unchanged on the pre-edit digest (`git apply --check` passed on HEAD, observed):

```diff
--- a/scripts/cellbox-ceiling-diff.py
+++ b/scripts/cellbox-ceiling-diff.py
@@ -3,7 +3,8 @@
 
 Extracts the `declare_manifest!(...)` key/value pairs and the
 `declare_syscalls![...]` identifier list from a candidate `main.rs` and from a
-baseline (default: `git show HEAD:cells/tools/shell/src/main.rs`) and fails when
+baseline (default: `git show 2f0bc0659:cells/tools/shell/src/main.rs`, the last tree that
+still has the retired shell; 5043e2cf5 removed `cells/tools/shell`) and fails when
 the two sets differ in either direction (an added *or* a removed entry).
 
 Usage:
@@ -24,6 +25,9 @@
 ROOT = Path(__file__).resolve().parent.parent
 DEFAULT_CANDIDATE = "cells/tools/cellbox/src/main.rs"
 DEFAULT_BASELINE_PATH = "cells/tools/shell/src/main.rs"
+# Parent of 5043e2cf5, the Cellbox commit that deleted cells/tools/shell: the last tree that
+# carries the shell ceiling the cellbox ceiling is compared with (plus the approved additions).
+DEFAULT_BASELINE_REF = "2f0bc06599808be3ab0e4b3df0d79ea08c5317aa"
 
 
 class ExtractionError(Exception):
@@ -90,7 +94,7 @@
 def main() -> int:
     parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
     parser.add_argument("candidate", nargs="?", default=DEFAULT_CANDIDATE)
-    parser.add_argument("--baseline-ref", default="HEAD")
+    parser.add_argument("--baseline-ref", default=DEFAULT_BASELINE_REF)
     parser.add_argument("--allow-added", action="append", default=["Write", "GetRandom"], help="syscalls added by an approved delta (default: Write, Phase 02 CP2 closure; GetRandom, Phase 09 D79-9 as revised)")
     parser.add_argument("--baseline-path", default=DEFAULT_BASELINE_PATH)
     parser.add_argument("--baseline-file", help="read the baseline from a file instead of git")
```

Result digest if applied byte-for-byte: `scripts/cellbox-ceiling-diff.py` = `9ede3b6a6e265d184395ff4cabb2e3d176e738da1611f7ccdf01a1c3eb690d41` (computed on an exported copy; CP2 must re-hash).

Consequences:
- The default call in the terminal (`:259`) and GUI (`:243`) runners and the explicit pin in the ssh runner (`:212`) all use the same baseline; output of the default call is byte-identical to the explicit pin (observed `cmp`).
- Callers passing `--baseline-ref HEAD --baseline-path <app main.rs>` (informational Terminal/Files) are unchanged (observed: still `G-S1a PASS`).
- **Runners are not edited.** In particular the terminal runner, which a K3/K7 lock row names for digest pins only, and the ssh runner, which carries the K8 freeze, stay byte-identical. The redundant `CEILING_BASELINE` pin in `qemu-phase09-ssh-test.sh:135-136,212` is left (same value, no conflict).
- Not changed: allow-list, extraction, exit codes, messages.

`CHANGELOG.md` (sha in section 4), `## [Unreleased]` → `### Fixed` (line 434): one bullet saying the G-S1a gate's default baseline is pinned to `2f0bc0659` because `cells/tools/shell` no longer exists at HEAD, which restores the Phase 04 terminal and GUI lanes' repository check; the ssh lane's pin is unchanged; no ceiling changed.

## 3. Alternatives considered

| Option | Edit | Verdict |
|---|---|---|
| **A** pin the default inside the script (this proposal) | 1 script | Fixes all current and future default callers; smallest closure; keeps runner/K8 freeze files untouched |
| B pass `--baseline-ref 2f0bc06…` in the terminal and GUI runners (as K8 did for ssh) | 2 runners | Same effect but 2 more frozen files, the terminal runner is K3/K7-named, GUI is also covered by the runner guard; every future caller must remember the pin. Overlaps any peer edit of runners |
| C commit a small baseline snapshot (JSON of the 32 syscalls + 6 manifest keys + provenance) and read it with `--baseline-file` | new file + script + callers | Survives shallow clones/history rewrite, but new baseline artifact to keep honest, bigger closure and a new source of truth for the shell ceiling. Not needed while history is intact |
| D baseline = `HEAD:cells/tools/cellbox/src/main.rs` | callers | **Rejected**: vacuous on a committed tree, loses the "equals the shell ceiling" meaning (F6) |
| E delete the G-S1a call | runners | **Rejected**: it is a named gate (`phase-04-entry-dossier.md:29`, K8 `G-6`) and removal of a check is a scope reduction |
| F extend the script to also check the K3 launch ceiling | script | **Rejected here**: different construct, already guarded by the host tests and digest pins; widens scope and the frozen-file blast radius |

## 4. File closure and current SHA-256

Edited by this item:

| Path | Edit | Current SHA-256 (size) |
|---|---|---|
| `scripts/cellbox-ceiling-diff.py` | section 2 diff | `34f2a939f29d61208ceebddc34b3da40310960a4c6fa1b18bdda78a0ec331c71` (5728 B) |
| `CHANGELOG.md` | one `### Fixed` bullet under `[Unreleased]` | `5c2022ac045b81cae884e8204994d0ffd4f2b116633d4f1181d89ccb87dceeb1` (402100 B) |

Read-only inputs the proof depends on (must be byte-equal at CP2):

| Path | SHA-256 |
|---|---|
| `cells/tools/cellbox/src/main.rs` (the candidate) | `517be1ec94ab581f4bfc55fdc9a16d1c5af8a3ed1a9f64d7b741209c907a93f8` |
| `scripts/qemu-phase04-terminal-test.sh` | `776e5897ad6b69a8f834e649a1f9366af5058c20f25f5a0cb52e415f893ab36a` |
| `scripts/qemu-phase04-gui-test.sh` | `ed13fd47d3979cd430c0480aabd1e3d5763a1179660481d0b9db94db3e12eee8` |
| `scripts/qemu-phase09-ssh-test.sh` | `89b7d76062e0fd20741acde18e97e0bc764013b05362a824d060911af9bd0fc7` |
| git object `2f0bc06599808be3ab0e4b3df0d79ea08c5317aa:cells/tools/shell/src/main.rs` (content) | `27171babfc24d4a208804c32949527ad1ee3a50013ad1e060af1978e6eb734ec` |

Not edited: no `kernel/**`, `libs/**` (no `libs/api`, `libs/types`), `cells/**`, `Cargo.toml`/`Cargo.lock`, signer, image or policy path. Option B would add the two runners to the closure.

## 5. Locks, Law-1, overlapping plans

- **CELL-CONTRACT-0024 rows.** `grep` of `.agents/cell-contract-0024-locks.md` for `cellbox-ceiling`, `ceiling-diff`, `qemu-phase09`, `qemu-phase04-gui` finds nothing. The only script paths in the locks are on line 117 (KSSH K3 bounded slice): `scripts/sign-policy.py` and `scripts/qemu-phase04-terminal-test.sh` "digest pins" (the K7 row, line 121, repeats the pin scope: `k7-cp1.md:21` "digest pins of the changed files only"). Option A edits neither; Option B would edit the terminal runner outside the pin-only scope, so it would need an owner ruling or a bounded exception. **No lock row names `scripts/cellbox-ceiling-diff.py` or `CHANGELOG.md` edits of this kind.** [INFERENCE: the inventory lists plans and a few exception paths, not every file; the negative is a grep, to be re-checked at slice start.]
- **Law-1.** No ABI object (`libs/api`, `libs/types`), capability, handle, identity, delegation or revocation change; no confirmation text needed. The gate only reads source text.
- **`plan-portfolio.md`.** Grep for `qemu-phase`, `ceiling-diff`, `G-S1a`, `scripts/**` finds nothing relevant; its Cellbox section (`:106-132`) schedules private evidence lanes, not this script. [INFERENCE: grep-level.]
- **Cellbox plan `261020-1750-cellbox-cli`.** The script is a closure file of an already approved slice: `phase-04-s1-s3-cp2-dossier.md:98` records `scripts/cellbox-ceiling-diff.py` at `34f2a939…`, and G-S1a is a Phase 04 gate (`phase-04-entry-dossier.md:29`, `phase-04-s2-authority-proposal.md:26`, `phase-04-s4-retirement-proposal.md:41`). The fix restores that gate without changing its meaning. The owner should record it as a post-CP2 amendment of that closure file (no new Cellbox CP1 is needed; no behaviour changes for passing callers).
- **KSSH program.** `k8-cp1.md` D8-7 left this out of K8; `plan.md:83-84` names it open. This item closes it. **Peer cleanup items** (F1 waiver, CI, CQ [INFERENCE: names from the roster, documents not read]): the F1 waiver text lives in `qemu-phase09-ssh-test.sh:331-348`; Option A does not touch that runner, so there is no overlap. Option B would.
- Not examined for overlap: `260825-sdk-delivery`, `260712-1902-dice-attestation-identity`, `260819-1409-cell-to-cell-anywhere-core` (grep-level in K2/K3/K8; none names the script).

## 6. Verification (to run at CP2; the observed values are from this session)

**V1 current tree passes.** `python3 scripts/cellbox-ceiling-diff.py` → exit 0. Observed with the patched script (exported copy, `GIT_DIR` pointing at the repo):
```
note: GetRandom added vs baseline by an approved delta
note: Write added vs baseline by an approved delta
baseline : git 2f0bc06599808be3ab0e4b3df0d79ea08c5317aa:cells/tools/shell/src/main.rs (32 syscalls, 6 manifest keys)
candidate: …/cells/tools/cellbox/src/main.rs (34 syscalls, 6 manifest keys)
G-S1a PASS: declare_manifest and declare_syscalls sets are identical
```
Runner form under strict mode: `bash -c 'set -euo pipefail; python3 scripts/cellbox-ceiling-diff.py | tee "$TMP/cellbox-ceiling.txt" >/dev/null; echo lane-continues'` → prints `lane-continues`, rc 0 (observed). The unpatched script in the same form: rc 2, no `lane-continues` (observed).

**V2 deliberate widening still fails.** Do it on a throw-away export so the repo is never edited:
```
X=$(mktemp -d); git archive HEAD scripts/cellbox-ceiling-diff.py cells/tools/cellbox/src/main.rs | tar -x -C "$X"
# apply the section 2 diff inside $X, then:
cd "$X"; export GIT_DIR=/home/dmin/cellos/.git
# mutation 1: add a syscall   -> replace "    GetRandom,\n];" with "    GetRandom,\n    Shutdown,\n];"
# mutation 2: widen manifest  -> replace "    network = false," with "    network = true,"
# mutation 3: remove a syscall-> delete the "    Getcwd,\n" line
python3 scripts/cellbox-ceiling-diff.py; echo rc=$?     # after each mutation, restore main.rs between them
```
Observed (35/34/33 syscalls respectively):
- mutation 1: `G-S1a FAIL … - syscall ADDED vs baseline: Shutdown`, rc 1; the strict-mode runner form aborts before `lane-continues`.
- mutation 2: `G-S1a FAIL … - manifest network: baseline=false candidate=true`, rc 1.
- mutation 3: `G-S1a FAIL … - syscall REMOVED vs baseline: Getcwd`, rc 1.
Equivalent without any export: pass a mutated copy as the candidate (`python3 scripts/cellbox-ceiling-diff.py /tmp/mutated-main.rs`; observed rc 1 for added syscall, widened manifest and removed syscall).

**V3 consistency and overrides.** `cmp` of default output vs `--baseline-ref 2f0bc06599808be3ab0e4b3df0d79ea08c5317aa` output: identical (observed). `cells/apps/terminal/src/main.rs --baseline-ref HEAD --baseline-path cells/apps/terminal/src/main.rs`: rc 0 `G-S1a PASS` (observed). Optional: same call with `--baseline-ref 8fe158478` shows `ADDED SetTimer` (Terminal) and `ADDED VfsMutate` (Files), rc 1 (observed).

**V4 static.** `python3 -m py_compile scripts/cellbox-ceiling-diff.py`; `bash -n` on the three runners (unchanged, expected OK); `git diff --stat` shows only the two closure files; `scripts/cellos-sign --check` unaffected [INFERENCE: it does not read this script]; no build, QEMU or image lane is required for this item. A full private Phase 04 terminal run is **not** part of acceptance (it would take hours on TCG and exercises none of this change beyond V1's line); if the owner wants end-to-end proof, the first lines of the terminal runner up to `:259` are the whole exercised path.

## 7. Risks

| Risk | Mitigation / residual |
|---|---|
| History rewrite or shallow clone drops `2f0bc0659` | The script fails closed (exit 2 with the git error); option C (snapshot file) removes the dependency. The commit is a reachable ancestor now; residual accepted, same exposure the ssh runner already carries |
| The pin hides real drift because the allow-list masks `Write`/`GetRandom` additions | Unchanged behaviour. Any further addition fails; removals fail (V2) |
| Editing the script changes a K8-frozen file (`frozen-sources-rt1.sha256:15`) | RT1 reruns are refused anyway (`RUN_CONSUMED-k8-rt1`); record the old and new digest in the CP2 dossier |
| Someone reads "G-S1a PASS" as covering K3's launch ceiling | It does not (F5); the CP2 text and CHANGELOG bullet say what it covers, K3 stays under the digest pins and host tests |
| Informational Terminal/Files deltas stay vacuous | Pre-existing; decision D-2 |
| Allowlist semantic change for callers passing an explicit non-HEAD ref | None: the only change is the default value; explicit `--baseline-ref` wins |
| Terminal/GUI lanes may have other failures at HEAD hidden behind this abort | [INFERENCE] none checked: the lanes were not run. The ssh lane's earlier steps are known to pass (K8) |

Rollback: revert the one script edit and the CHANGELOG bullet; no persisted state, image, key or lock touched.

## 8. Decision table for the owner

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D-1 | Where the pin lives | **A** inside the script (default value) / B in each runner / C snapshot file / D-E-F rejected | **A** |
| D-2 | Informational Terminal/Files delta calls (`terminal-test.sh:267-269`, `gui-test.sh:244-251`) | leave (vacuous at clean HEAD) / re-point to `--baseline-ref 8fe158478` in both runners (restores the historical delta record, edits two frozen runners) | **leave**; revisit only with a runner-owning slice |
| D-3 | Cover the K3/K7 launch ceilings in this script | no (host tests and digest pins own them) / yes (new extractor) | **no** |
| D-4 | Drop the now redundant `CEILING_BASELINE` pin in `qemu-phase09-ssh-test.sh` | leave / remove | **leave** (frozen file, same value) |
| D-5 | Record as post-CP2 amendment of the Phase 04 S1 closure file and close `plan.md:83-84` item "ceiling-diff calls" | approve / ask for option B closure | **approve** after the owner confirms no lock row applies (section 5 `[INFERENCE]`) |
| D-6 | End-to-end proof by a full private terminal lane | not required (V1-V4) / required | **not required** |

Nothing in this document authorises an edit before an approval record exists.
