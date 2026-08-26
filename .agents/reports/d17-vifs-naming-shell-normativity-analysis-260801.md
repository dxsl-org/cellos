# D17 — Retired viFS names and the status of the shell document

**Status:** ruled/applied 2026-08-01. Docs and one code comment updated; no runtime or ABI changed.

**Question:** are Spec 00-fork §6 and Spec 11-shell superseded by Spec 09's MountTable
decision, and is Spec 11 normative at all?

## Answer first

**Spec 09 owns the filesystem architecture. Spec 00-fork is not wholly superseded, but its
filesystem section is. Spec 11-shell should not remain normative in its current form.** It
is a historical tutorial/design sketch whose paths, APIs, filesystem names, execution
model, and safety claims diverge materially from the shipped shell.

The correction must preserve a case-sensitive distinction:

- `viFS1` = retired design name for a RedoxFS fork;
- `viFS2` = retired TFS/B-tree design;
- `VIFS1` = still-valid kernel identifier for the embedded FAT16 BootFS/initramfs.

A global replacement or deletion would corrupt valid code and bring-up documentation.

## 1. The binding filesystem decision

Spec 09 records the 2026-06-10 decision: one VFS service owns a longest-prefix
`MountTable`, with BootFS, RamFS, FAT32, littlefs, and RedoxFS backends. Dual `viFS1` /
`viFS2` was withdrawn; uppercase kernel `VIFS1` was retained solely as the BootFS name
(`docs/specs/09-vfs.md:32-57`). Code matches that model through
`cells/services/vfs/src/mount.rs` and `manager.rs`.

Spec 00-fork §6 still prescribes `viFS1 (Classic) = RedoxFS` and `viFS2 (Modern) = TFS` as
future products (`docs/specs/00-fork.md:61-71`). That section is superseded. The rest of
00-fork covers independent source-reuse policy and need not be retired merely because one
table drifted.

The clean ownership is:

- 00-fork: non-normative source/reference strategy;
- 09-vfs: filesystem architecture and backend-selection decision;
- code/generated status: what each backend currently implements.

## 2. Why Spec 11 is not a current specification

`docs/specs/11-shell.md` has no status header or anchors, but `docs/README.md` lists it
under “Design Specifications.” Readers therefore have no reliable signal that it is only
a sketch.

Its substantive claims are stale:

- It places the shell in `cells/apps/shell`; code lives in `cells/tools/shell`.
- It describes an incremental four-command shell; the shipped shell has a parser,
  executor, jobs, aliases, history, text tools, hot-swap state, and many commands.
- Its `ls` pseudocode receives a `Box<dyn ViFile>` and iterates `dir.read_dir()` across
  `viFS1`/`viFS2`. Actual `cmd_ls` calls `ostd::fs::read_dir`, which uses `sys_open` /
  `sys_read_dir` and stack `DirEntry` values
  (`cells/tools/shell/src/commands.rs:124-148`, `libs/ostd/src/fs.rs:16-48`).
- It says external execution goes through `ViVmRuntime` and zero-copy maps a RAM-disk
  file. Actual execution stages argv and calls `sys_spawn_from_path`
  (`cells/tools/shell/src/executor.rs:866-900`).
- It promises absolute LBI/panic recovery in terms that D11–D13 have already narrowed.

This is narrative guidance, not stable decisions/invariants of the kind Spec 21 permits
in Layer 1. Editing two viFS names would leave the larger false document looking current.

## 3. The stale naming surface is wider than the docket

Case-sensitive search finds retired lowercase names in active sources beyond 00-fork and
11-shell:

- `docs/specs/00-context.md:128` still defines the old naming convention;
- `docs/code-standards.md:112` repeats it as an active coding rule;
- `cells/services/vfs/src/page_cache.rs:22` says viFS2/WAL will become the backend;
- Spec 09 and roadmap use the names correctly only to document their retirement.

These must be handled in the same ruling pass or new code will continue following a
withdrawn naming rule. Historical changelog/research statements and Spec 09's retirement
record should remain intact.

Uppercase `VIFS1` occurs widely in the kernel, loader, policy, tests, and board bring-up
docs. `kernel/src/fs.rs:16-18` explicitly documents the naming distinction. Those uses are
not D17 violations.

## Recommended ruling [FINAL]

**Approve recommendation A:**

1. Keep 00-fork as a non-normative reference-strategy document, but replace §6's two
   retired product rows with a pointer to Spec 09 and the MountTable/backend policy.
2. Mark 11-shell historical/superseded and remove it from the active spec index. Do not
   pretend two line edits make it current.
3. If a normative shell contract is still valuable, write a new concise spec from the
   shipped parser/executor/VFS/spawn interfaces; otherwise link readers to shell docs/code.
4. Correct active old naming rules in 00-context, code-standards, and the page-cache
   comment. Preserve Spec 09/roadmap retirement prose and historical changelog/research.
5. Reserve uppercase `VIFS1` explicitly for BootFS/initramfs; do not introduce a `VIFS2`.

### Rejected alternatives

- **Supersede all of 00-fork:** discards unrelated reuse policy because one section drifted.
- **Replace only viFS1/viFS2 in 11-shell:** leaves a deeply false document presented as a
  design specification.
- **Global search-and-replace:** damages valid `VIFS1` implementation terminology and
  historical decision records.
- **Let 11-shell remain ambiguously unversioned:** violates Spec 21's goal of making stale
  claims impossible to mistake for current architecture.
