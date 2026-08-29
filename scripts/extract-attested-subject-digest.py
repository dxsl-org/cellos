#!/usr/bin/env python3
"""Extract one SHA-256 subject digest from successful `gh attestation verify` JSON."""

import argparse
import json
from pathlib import Path


def main() -> int:
    """Reject ambiguous or malformed verifier output and print its sole digest."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("verification_result", type=Path)
    args = parser.parse_args()
    try:
        results = json.loads(args.verification_result.read_text(encoding="utf-8"))
        if not isinstance(results, list) or len(results) != 1:
            raise ValueError("expected exactly one verified attestation")
        subjects = results[0]["verificationResult"]["statement"]["subject"]
        if not isinstance(subjects, list) or len(subjects) != 1:
            raise ValueError("expected exactly one attested subject")
        digest = subjects[0]["digest"]["sha256"]
        if not isinstance(digest, str) or len(digest) != 64:
            raise ValueError("invalid attested SHA-256 digest")
        if any(character not in "0123456789abcdef" for character in digest):
            raise ValueError("invalid attested SHA-256 digest")
    except (KeyError, OSError, TypeError, json.JSONDecodeError, ValueError) as error:
        parser.exit(1, f"FAIL: {error}\n")
    print(digest)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
