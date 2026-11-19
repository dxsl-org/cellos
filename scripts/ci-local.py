#!/usr/bin/env python3
"""Run one .github/workflows/ci.yml job on this machine, step by step.

Reproduces a CI failure before pushing instead of spending runner minutes:
the job's `run:` steps execute with the workflow/job/step `env:`, the same
shells, `working-directory`, `timeout-minutes`, `if:` conditions and
GITHUB_ENV/GITHUB_PATH/GITHUB_OUTPUT handling. `uses:` actions are skipped
(toolchains are expected installed; artifacts are already in target/),
except the repo's apt-install action, which is checked locally.

  scripts/ci-local.py list
  scripts/ci-local.py run lint --skip 'Clippy*'
  scripts/ci-local.py run build --matrix short=rv64
  scripts/ci-local.py run qemu-x86_64-boot --only 'AHCI*' --dry-run

The process environment is rebuilt from a small allowlist, so variables a
shell session leaks (CARGO_BUILD_TARGET, CC_*) do not reach the steps.
"""

import argparse
import fnmatch
import itertools
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ci_local.expr import Evaluator, to_str  # noqa: E402
from ci_local.process import read_env_file, run_process, shell_argv  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/ci.yml"
APT_ACTION = "./.github/actions/apt-install"
KEEP_ENV = ("HOME", "USER", "LOGNAME", "PATH", "LANG", "LC_ALL", "TERM", "SHELL",
            "RUSTUP_HOME", "CARGO_HOME")


def matrix_entries(job):
    matrix = (job.get("strategy") or {}).get("matrix") or {}
    axes = {k: v for k, v in matrix.items() if k not in ("include", "exclude")}
    entries = [dict(zip(axes, combo)) for combo in itertools.product(*axes.values())]
    return (entries if axes else []) + list(matrix.get("include", [])) or [{}]


def pick_matrix(job, wanted):
    hits = [e for e in matrix_entries(job)
            if all(to_str(e.get(k)) == v for k, v in wanted.items())]
    if not hits:
        sys.exit(f"no matrix entry matches {wanted}; entries: {matrix_entries(job)}")
    if len(hits) > 1:
        print(f"[ci-local] {len(hits)} matrix entries match; using {hits[0]} (narrow with --matrix)")
    return hits[0]


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True,
                          text=True).stdout.strip()


def run_job(wf, job_id, args):
    job = wf["jobs"][job_id]
    tmp = Path(tempfile.mkdtemp(prefix=f"ci-local-{job_id}-"))
    files = {k: tmp / k.lower() for k in ("GITHUB_ENV", "GITHUB_PATH", "GITHUB_OUTPUT",
                                          "GITHUB_STEP_SUMMARY")}
    env = {k: os.environ[k] for k in KEEP_ENV if k in os.environ}
    env.update(CI="true", GITHUB_ACTIONS="true", GITHUB_WORKSPACE=str(ROOT),
               RUNNER_TEMP=str(tmp / "runner-temp"), RUNNER_OS="Linux",
               RUNNER_ARCH="X64", GITHUB_EVENT_NAME="push")
    (tmp / "runner-temp").mkdir()
    state = {"status": "success"}
    steps_ctx = {}
    # A push event as GitHub would deliver it for pushing HEAD to main now:
    # `before` is the remote branch tip the push would replace.
    ctx = {
        "github": {"workspace": str(ROOT), "event_name": "push", "run_id": "0",
                   "run_attempt": "1", "run_number": "0", "ref": "refs/heads/main",
                   "ref_name": "main", "workflow": wf.get("name", "CI"),
                   "event": {"before": git("rev-parse", "origin/main"),
                             "repository": {"default_branch": "main"}},
                   "sha": git("rev-parse", "HEAD")},
        "runner": {"temp": env["RUNNER_TEMP"], "os": "Linux", "arch": "X64",
                   "environment": "self-hosted"},
        "matrix": pick_matrix(job, args.matrix), "env": env, "steps": steps_ctx,
        "job": {"status": "success"}, "vars": {}, "secrets": {}, "needs": {}, "inputs": {},
    }
    ev = Evaluator(ctx, status=lambda: state["status"])
    for scope in (wf.get("env") or {}, job.get("env") or {}):
        env.update({k: to_str(ev.interpolate(v)) for k, v in scope.items()})
    workdir = ((job.get("defaults") or {}).get("run") or {}).get("working-directory")
    results = []
    for index, step in enumerate(job.get("steps", []), 1):
        name = step.get("name") or step.get("uses") or step["run"].strip().splitlines()[0]
        name = to_str(ev.interpolate(name))
        selected = (not args.only or any(fnmatch.fnmatch(name, g) for g in args.only)) \
            and not any(fnmatch.fnmatch(name, g) for g in args.skip)
        if not selected or not ev.condition(step.get("if")):
            results.append((name, "skipped", 0.0))
            continue
        uses = step.get("uses")
        if uses and uses != APT_ACTION:
            results.append((name, f"skipped (uses {uses.split('@')[0]})", 0.0))
            continue
        step_env = dict(env)
        step_env.update({k: to_str(ev.interpolate(v)) for k, v in (step.get("env") or {}).items()})
        for key, path in files.items():
            path.write_text("")
            step_env[key] = str(path)
        if uses:
            step_env["APT_PACKAGES"] = to_str(ev.interpolate(step["with"]["packages"]))
            argv = ["bash", str(ROOT / ".github/actions/apt-install/apt-install.sh"), "check"]
        else:
            script = tmp / f"step-{index}.sh"
            script.write_text(ev.interpolate(step["run"]))
            argv = shell_argv(step.get("shell"), str(script))
        cwd = ROOT / to_str(ev.interpolate(step.get("working-directory") or workdir or "."))
        print(f"\n[ci-local] ── {index}. {name}", flush=True)
        if args.dry_run:
            print("   " + ("\n   ".join(Path(argv[-1]).read_text().splitlines())
                          if not uses else f"apt-install check: {step_env['APT_PACKAGES']}"))
            results.append((name, "dry-run", 0.0))
            continue
        started = time.monotonic()
        code = run_process(argv, step_env, cwd, step.get("timeout-minutes"))
        elapsed = time.monotonic() - started
        outputs = read_env_file(files["GITHUB_OUTPUT"])
        env.update(read_env_file(files["GITHUB_ENV"]))
        extra_path = [p for p in files["GITHUB_PATH"].read_text().splitlines() if p]
        if extra_path:
            env["PATH"] = os.pathsep.join(extra_path[::-1] + [env.get("PATH", "")])
        outcome = "success" if code == 0 else "failure"
        if "id" in step:
            steps_ctx[step["id"]] = {"outcome": outcome, "outputs": outputs,
                                     "conclusion": "success" if step.get("continue-on-error")
                                     else outcome}
        if code and not step.get("continue-on-error"):
            state["status"] = ctx["job"]["status"] = "failure"
        results.append((name, f"{outcome} (exit {code})" if code else outcome, elapsed))
        if code and args.fail_fast:
            break
    print(f"\n[ci-local] job {job_id} — {state['status']}")
    for name, outcome, elapsed in results:
        print(f"  {elapsed:7.1f}s  {outcome:<28} {name}")
    # Steps may `git worktree add` under RUNNER_TEMP; a hosted VM discards
    # them, here they must be unregistered with the directory.
    shutil.rmtree(tmp, ignore_errors=True)
    git("worktree", "prune")
    return 0 if state["status"] == "success" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list", help="list job ids and names")
    run = sub.add_parser("run", help="run one job")
    run.add_argument("job")
    run.add_argument("--matrix", action="append", default=[], metavar="KEY=VALUE")
    run.add_argument("--only", action="append", default=[], metavar="GLOB",
                     help="run only steps whose name matches (repeatable)")
    run.add_argument("--skip", action="append", default=[], metavar="GLOB",
                     help="skip steps whose name matches (repeatable)")
    run.add_argument("--dry-run", action="store_true", help="print the scripts, run nothing")
    run.add_argument("--fail-fast", action="store_true", help="stop at the first failed step")
    args = parser.parse_args()
    wf = yaml.safe_load(WORKFLOW.read_text())
    if args.cmd == "list":
        for job_id, job in wf["jobs"].items():
            print(f"{job_id:<46} {job.get('name', '')}")
        return 0
    if args.job not in wf["jobs"]:
        sys.exit(f"unknown job {args.job!r}; see `scripts/ci-local.py list`")
    args.matrix = dict(m.split("=", 1) for m in args.matrix)
    return run_job(wf, args.job, args)


if __name__ == "__main__":
    sys.exit(main())
