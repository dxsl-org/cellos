# Prepared step — untrack build outputs (run after CI is green on `374658ac8`)

Owner condition (2026-11-10): untrack only after one pushed CI cycle on the explicit-embed commit. Local simulation already passed ([evidence](reports/phase-03-evidence.md#continuation-2026-11-10--ci-no-longer-reads-tracked-embed-binaries)).

## Preconditions

1. `origin/main` contains `374658ac8` (or its rebased equivalent).
2. That CI run is green for `build`, `qemu-boot`, `qemu-aarch64-boot`, `qemu-x86_64-boot`, `x86-c2c`, `unit-tests`, `lint`, `x86_64-lint`, `aarch64-lint`.
3. No job log contains `EMBEDDED_OVERRIDE not set: embedding the in-tree fallback`.

## Commands (files stay on disk)

```bash
git rm --cached -q $(git ls-files 'kernel/src/embedded*/init' 'kernel/src/embedded*/kernel_fs.img')
git rm --cached -r -q gh_2.55.0_linux_amd64
git rm --cached -q $(git ls-files 'limine/**' | grep -E '\.(o|a|d|deb)$')
cat >> .gitignore <<'IGN'

# Build outputs (architecture remediation Phase 03): assembled per build, never tracked.
/kernel/src/embedded*/init
/kernel/src/embedded*/kernel_fs.img
/gh_2.55.0_linux_amd64/
/limine/**/*.o
/limine/**/*.a
/limine/**/*.d
/limine/*.deb
IGN
```

`kernel/src/embedded-x86_64/manifest.json` stays tracked as the record of the canonical headless image; `build.rs` checks a locally assembled image against whichever manifest sits next to it.

## After

- Success criterion: `git ls-files 'kernel/src/embedded*'` lists no `init` or `kernel_fs.img`.
- History keeps the old blobs (no rewrite). Rollback: `git revert` the untrack commit.
- Close Phase 03 except the release item (blocked on release signing).
