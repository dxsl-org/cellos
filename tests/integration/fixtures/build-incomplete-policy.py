#!/usr/bin/env python3
"""Build a validly signed policy with one P-TRUST row omitted for runtime tests."""

import argparse
import importlib.util
from pathlib import Path


def load_signer(repo: Path):
    script = repo / "scripts" / "sign-policy.py"
    spec = importlib.util.spec_from_file_location("cellos_sign_policy", script)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {script}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--omit", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    repo = args.repo.resolve()
    signer = load_signer(repo)
    p_trust_paths = signer.ptrust_paths(repo / "kernel/src/task/cap.rs")
    if args.omit not in p_trust_paths:
        parser.error(f"--omit must name a P-TRUST path, got {args.omit!r}")

    entries = [entry for entry in signer.DEV_POLICY if entry[0] != args.omit]
    if len(entries) + 1 != len(signer.DEV_POLICY):
        parser.error(f"policy does not contain exactly one {args.omit!r} row")
    body = signer.build_body(entries, 0)
    signer.assert_round_trip(body, entries, 0)
    _, signature = signer.sign(body)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(body + signature)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
