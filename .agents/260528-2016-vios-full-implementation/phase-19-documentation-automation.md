# Phase 19 — Documentation Automation

**Effort:** 40h | **Priority:** P2 | **Status:** pending | **Blockers:** Phase 02, Phase 11

## Overview

Automate documentation: `cargo doc` → GitHub Pages on every main push, conventional-commits → CHANGELOG via git-cliff on tag push, structured issue/PR templates, CONTRIBUTING.md as the canonical entry point. Sets the project up for predictable releases and contributor onboarding (Phase 23 builds on this).

## Context Links

- `docs/ONBOARDING.md` — existing onboarding (needs polish for community)
- `docs/development-roadmap.md`, `docs/project-changelog.md` — already maintained docs
- Phase 02 — CI exists; this phase adds two new workflows
- Phase 11 — tests must be green for releases

## Key Insights

- `cargo doc` for a `no_std` workspace requires `--target` per build and `-Z build-std`. To produce a public-readable site, build for one target (RV64) and publish.
- GitHub Pages from `gh-pages` branch via `peaceiris/actions-gh-pages@v3` is straightforward; deploys are read-only artifacts.
- git-cliff parses conventional commits (`feat:`, `fix:`, `chore:`, `docs:` …) and groups them into a markdown CHANGELOG. Output committed back to repo + attached to release.
- Releases: trigger on tag matching `v*` regex. GitHub Release body = the CHANGELOG section for that version + artifacts (compiled kernel ELFs, disk image).
- llms.txt = a simple discoverable index of project docs for LLM consumption. Generated from `docs/` index.

## Requirements

**Functional**
- Push to main → docs site published to GitHub Pages within 5 min
- Push tag `v0.X.Y` → CHANGELOG updated, GitHub Release created, kernel artifacts attached
- New issue → presented with bug/feature template options
- New PR → checklist template auto-populates description
- `CONTRIBUTING.md` covers: setup, build, test, PR rules, where-to-start
- `llms.txt` lists key docs with descriptions; auto-updated on docs changes

**Non-functional**
- Docs site loads < 2s (rustdoc is static HTML)
- Release pipeline < 10 min from tag push to published release

## Architecture

```
push main          ───► docs.yml ───► cargo doc → gh-pages
push tag v*        ───► release.yml ─► git-cliff → CHANGELOG.md
                                      └► softprops/action-gh-release → upload artifacts

docs/* changes     ───► docs.yml also runs `gen-llms-txt.sh` → updates llms.txt
PR opened          ───► template populates body
Issue opened       ───► template picker
```

## Related Code Files

**Create:**
- `.github/workflows/docs.yml` — cargo doc build + gh-pages deploy
- `.github/workflows/release.yml` — tag-triggered changelog + release
- `cliff.toml` — git-cliff configuration
- `CONTRIBUTING.md` (root) — quick contributor entry; under 200 lines; links to deeper docs
- `CODE_OF_CONDUCT.md` (root) — Contributor Covenant v2.1 verbatim
- `CHANGELOG.md` (root, seeded with v0.2.0 entry from existing `docs/project-changelog.md`)
- `llms.txt` (root) — generated index
- `scripts/gen-llms-txt.sh` — walks `docs/` + reads frontmatter / first paragraph
- `.github/pull_request_template.md` (already created in Phase 02; extend with docs checklist)
- `.github/ISSUE_TEMPLATE/{bug_report,feature_request,config}.md|yml` (already from Phase 02 — verify)
- `docs/release-process.md` — how to cut a release: tag, push, verify CI

**Modify:**
- `README.md` — add badges (CI, docs, license, latest release) + link to CONTRIBUTING
- `Cargo.toml` workspace — ensure `[workspace.package]` has accurate `description`, `repository`, `license`, `documentation` fields (rustdoc uses these)
- `docs/ONBOARDING.md` — append "Common errors" section, total time estimate (Linux 30 min / Windows 45 min)
- `docs/project-changelog.md` — note it is now generated; preserve historical entries as `docs/changelog-historical.md`

## Implementation Steps

1. **`cliff.toml`** — config for conventional commits:
   ```toml
   [changelog]
   header = "# Changelog\n\n"
   body = """
   ## [{{ version | default(value="unreleased") }}] - {{ timestamp | date(format="%Y-%m-%d") }}
   {% for group, commits in commits | group_by(attribute="group") %}
   ### {{ group | upper_first }}
   {% for commit in commits %}- {{ commit.message }} ({{ commit.id | truncate(length=7, end="") }})
   {% endfor %}{% endfor %}
   """
   [git]
   conventional_commits = true
   commit_parsers = [
     { message = "^feat", group = "Features" },
     { message = "^fix", group = "Bug Fixes" },
     { message = "^perf", group = "Performance" },
     { message = "^refactor", group = "Refactor" },
     { message = "^docs", group = "Documentation" },
     { message = "^test", group = "Tests" },
     { message = "^chore", group = "Chore", skip = false },
   ]
   ```
2. **`.github/workflows/docs.yml`** (push to main):
   - Checkout
   - `dtolnay/rust-toolchain@master` reading `rust-toolchain.toml`
   - `Swatinem/rust-cache@v2`
   - `cargo doc --workspace --no-deps --target riscv64gc-unknown-none-elf -Z build-std=core,alloc --target-dir target-doc`
   - `bash scripts/gen-llms-txt.sh > llms.txt`
   - `peaceiris/actions-gh-pages@v3` deploys `target-doc/riscv64gc-unknown-none-elf/doc` to `gh-pages` branch with custom `index.html` redirector to the `kernel` crate
3. **`.github/workflows/release.yml`** (push tag `v*`):
   - Checkout with full history (`fetch-depth: 0`)
   - Build kernel for all 3 archs (matrix)
   - Generate disk image via `gen_disk.ps1` equivalent bash
   - Run `git-cliff -o CHANGELOG.md --latest`
   - Commit CHANGELOG.md back to main (gated)
   - `softprops/action-gh-release@v2`:
     - body_path: `CHANGELOG.md` latest section
     - files: kernel-rv64, kernel-aarch64, kernel-x86_64, disk.img
4. **`scripts/gen-llms-txt.sh`**:
   - Walk `docs/*.md`
   - For each: extract H1 title + first non-empty paragraph
   - Emit:
     ```
     # ViOS
     > Cellular Single-Address-Space OS in Rust
     
     ## Docs
     - [Architecture](docs/system-architecture.md): one-line summary
     - [Code Standards](docs/code-standards.md): one-line summary
     …
     ```
5. **Seed `CHANGELOG.md`** with v0.2.0 historical entry (manually written, pulled from existing `docs/project-changelog.md` highlights). git-cliff appends going forward.
6. **`CONTRIBUTING.md`** — keep under 200 lines:
   - 5-line quick start (clone, install nightly, `./run.ps1`)
   - PR checklist (lint, test, sign-off)
   - Commit format (conventional commits) + examples
   - Where to start: link to `good-first-issue` label
   - Branch naming: `feat/<short>`, `fix/<short>`, `docs/<short>`
   - Link to deeper docs: ARCHITECTURE, CODING_GUIDE, security-model
7. **`CODE_OF_CONDUCT.md`** — verbatim Contributor Covenant v2.1, contact email = the repo's security email
8. **README badges** — add: build status (Phase 02), docs site link, latest release, license (MIT or Apache-2.0)
9. **Test the docs workflow**:
   - Push a small docs-only commit; verify gh-pages deploys; visit URL
10. **Test the release workflow**:
    - Cut a `v0.2.1-test` tag on a side branch; verify release created with artifacts; delete after
11. **Document release process** in `docs/release-process.md`:
    - Pre-release checklist: all CI green, perf benchmarks within bounds, security weekly clean
    - Tag: `git tag -a v0.X.Y -m "Release v0.X.Y"`
    - Push: `git push origin v0.X.Y`
    - Verify release page
    - Announce via Discussions

## Todo List

- [ ] Create `cliff.toml`
- [ ] Create `.github/workflows/docs.yml`
- [ ] Create `.github/workflows/release.yml`
- [ ] Create `scripts/gen-llms-txt.sh`
- [ ] Seed `CHANGELOG.md` with v0.2.0 entry
- [ ] Write `CONTRIBUTING.md`
- [ ] Add `CODE_OF_CONDUCT.md` (Contributor Covenant v2.1)
- [ ] Add badges to `README.md`
- [ ] Update `docs/ONBOARDING.md` with common errors + time estimates
- [ ] Test docs.yml on a docs-only commit
- [ ] Test release.yml on a side-branch tag
- [ ] Write `docs/release-process.md`
- [ ] CI green for both workflows

## Success Criteria

- Docs site reachable at `https://<org>.github.io/vios/` and includes all crates
- Tag push produces a clean GitHub Release with CHANGELOG + binaries
- `CONTRIBUTING.md` < 200 lines, links resolved
- `llms.txt` updated on every docs change
- New contributor (per Phase 23 testing) can read CONTRIBUTING + build in <45 min

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `cargo doc` blowups on `no_std`/`alloc`-heavy macros | Med | Med | Build with `--no-deps`; fix doc errors in scope as separate small PRs |
| GitHub Pages deploy quota or branch protection | Low | Low | Use `gh-pages` branch with `force-push` allowed only by Actions |
| git-cliff misparses some commits → release notes ugly | Med | Low | Iterate `cliff.toml` after first real release; document commit format expectations |
| Release artifacts blow up GitHub release size limits (2 GB per file) | Low | Low | Disk image strip unused bins; per-arch separate uploads |
| CONTRIBUTING.md drift vs reality | Cert | Low | Quarterly review cadence; track via issue in `area:docs` |
| Auto-commit of CHANGELOG creates loop | Med | Med | Gate on `actor != github-actions`; verify before merge |

## Security Considerations

- Release artifacts (kernel ELFs) are publicly downloadable — fine, but no embedded secrets
- `gen-llms-txt.sh` only reads `docs/`; never includes ENV vars
- `release.yml` uses `GITHUB_TOKEN` with `contents: write` scope only

## Rollback

Workflows are inert until triggered. Revert removes them; gh-pages deploys stop but existing site stays until next push. CHANGELOG and CONTRIBUTING are pure docs; safe to keep on revert.

## Next Steps

Phase 23 (community) layers issue labels, discussions templates, dev-setup script. Phase 22 publishes benchmark reports to the docs site. Every subsequent release exercises this pipeline.
