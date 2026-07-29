#!/usr/bin/env python3
"""Fail if the local cargo config template and CI disagree on codegen flags.

`.cargo/config.toml` is gitignored — it carries machine-specific absolute paths —
so it never reaches CI. CI therefore mirrors its per-target rustflags as
`CARGO_TARGET_*_RUSTFLAGS` env vars at the top of the workflow, and the template
those local configs are generated from lives in scripts/cargo-config-linux.toml.

Two copies of the same flags is a drift hazard, and the flags are not cosmetic:
they decide relocation model and target features. Drift shows up as a build that
succeeds locally and fails in CI (or worse, one that links but faults at runtime)
with no code defect anywhere. This check turns the "keep them in sync" comment
into an enforced invariant.

Exit 0 when every target matches, 1 otherwise.
"""

import pathlib
import re
import sys
import tomllib

REPO = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = REPO / "scripts" / "cargo-config-linux.toml"
WORKFLOW = REPO / ".github" / "workflows" / "ci.yml"

# target triple -> the CI env var that mirrors its rustflags
TARGETS = {
    "riscv64gc-unknown-none-elf": "CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS",
    "aarch64-unknown-none-softfloat": "CARGO_TARGET_AARCH64_UNKNOWN_NONE_SOFTFLOAT_RUSTFLAGS",
    "x86_64-unknown-none": "CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS",
}


def workflow_flags(text: str, var: str) -> list[str] | None:
    """The workflow-level value of `var`, split into argv.

    Deliberately anchored to the FIRST occurrence: that is the workflow-level
    `env:` block. Later occurrences are per-step overrides (e.g. x86_64 cells
    build pic instead of static) and are expected to differ.
    """
    match = re.search(rf"^\s*{re.escape(var)}:\s*\"([^\"]*)\"", text, re.M)
    return match.group(1).split() if match else None


def main() -> int:
    for path in (TEMPLATE, WORKFLOW):
        if not path.is_file():
            print(f"FAIL: {path.relative_to(REPO)} not found", file=sys.stderr)
            return 1

    template = tomllib.loads(TEMPLATE.read_text(encoding="utf-8"))
    workflow = WORKFLOW.read_text(encoding="utf-8")

    problems = []
    for target, var in TARGETS.items():
        local = template.get("target", {}).get(target, {}).get("rustflags")
        remote = workflow_flags(workflow, var)

        if local is None:
            problems.append(f"{target}: no [target.{target}] rustflags in the template")
            continue
        if remote is None:
            problems.append(f"{target}: {var} not found at workflow level in ci.yml")
            continue
        if local != remote:
            problems.append(
                f"{target}: flags differ\n"
                f"    ci.yml   : {' '.join(remote)}\n"
                f"    template : {' '.join(local)}"
            )
        else:
            print(f"  ok  {target}: {' '.join(local)}")

    if problems:
        print("\nFAIL: cargo config and CI disagree on codegen flags", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(
            "\nFix BOTH scripts/cargo-config-linux.toml and the workflow-level env\n"
            "block in .github/workflows/ci.yml, then re-run. Developers with an\n"
            "existing .cargo/config.toml must regenerate it: ./scripts/dev-setup.sh",
            file=sys.stderr,
        )
        return 1

    print(f"\nPASS: {len(TARGETS)} targets agree between the template and CI")
    return 0


if __name__ == "__main__":
    sys.exit(main())
