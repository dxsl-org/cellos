#!/usr/bin/env python3
"""Verify manifest integrity after GitHub attestation verification."""

import argparse
from pathlib import Path

from authenticated_evidence import verify_bundle_bytes


def main() -> int:
    """Verify the attested digest, bundle contents, and expected CI identity."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--expected-revision", required=True)
    parser.add_argument("--expected-sequence", required=True)
    parser.add_argument("--expected-workflow-ref", required=True)
    parser.add_argument("--expected-runner-prefix", required=True)
    parser.add_argument("--expected-manifest-sha256", required=True)
    args = parser.parse_args()
    try:
        manifest = verify_bundle_bytes(args.manifest, args.expected_manifest_sha256)
        if manifest.get("revision") != args.expected_revision or manifest.get("sequence") != args.expected_sequence:
            raise ValueError("revision or sequence mismatch")
        if manifest.get("result") != "passed" or not isinstance(manifest.get("command"), str) or not manifest["command"]:
            raise ValueError("invalid evidence result or command")
        if manifest.get("workflow_ref") != args.expected_workflow_ref:
            raise ValueError("workflow identity mismatch")
        runner = manifest.get("runner")
        if not isinstance(runner, str) or not runner.startswith(args.expected_runner_prefix):
            raise ValueError("runner identity mismatch")
        if not isinstance(manifest.get("environment"), dict):
            raise ValueError("invalid environment")
    except (OSError, ValueError) as error:
        parser.exit(1, f"FAIL: {error}\n")
    print("PASS: authenticated evidence contents verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
