"""Step execution primitives for scripts/ci-local.py, mirroring the runner."""

import itertools
import os
import signal
import subprocess
import sys
from pathlib import Path


def read_env_file(path):
    """Parse GITHUB_ENV / GITHUB_OUTPUT syntax: KEY=VALUE and KEY<<DELIM blocks."""
    out, lines = {}, Path(path).read_text().splitlines()
    it = iter(lines)
    for line in it:
        if "<<" in line and ("=" not in line or line.index("<<") < line.index("=")):
            key, delim = line.split("<<", 1)
            out[key] = "\n".join(itertools.takewhile(lambda l: l != delim, it))
        elif "=" in line:
            key, value = line.split("=", 1)
            out[key] = value
    return out


def shell_argv(shell, script):
    if shell in (None, "bash"):
        flags = ["-e"] if shell is None else ["--noprofile", "--norc", "-eo", "pipefail"]
        return ["bash", *flags, script]
    if shell == "python":
        return [sys.executable, script]
    if shell == "sh":
        return ["sh", "-e", script]
    return [part.replace("{0}", script) for part in shell.split()]


def run_process(argv, env, cwd, timeout_min):
    proc = subprocess.Popen(argv, env=env, cwd=cwd, start_new_session=True)
    try:
        return proc.wait(timeout=timeout_min * 60 if timeout_min else None)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
        print(f"[ci-local] step exceeded timeout-minutes: {timeout_min}")
        return 124
    except KeyboardInterrupt:
        os.killpg(proc.pid, signal.SIGKILL)
        raise

