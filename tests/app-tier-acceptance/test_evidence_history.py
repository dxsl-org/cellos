"""A recorded log or artifact stays verifiable after its working copy moves on.

The ledger records evidence of a moment. A file that later receives an ordinary
edit - a log appended to, a patch revised - must not invalidate the record, so a
digest that some commit still holds resolves from history; a digest that no commit
holds is refused exactly as before.
"""

from __future__ import annotations

import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from app_tier_acceptance import checks  # noqa: E402


def recorded(content: bytes) -> tuple[str, int]:
    """Return the (sha256, size_bytes) pair the ledger would record."""
    return hashlib.sha256(content).hexdigest(), len(content)


class EvidenceHistoryTests(unittest.TestCase):
    """Evidence resolves against its recorded bytes, not the current file."""

    def git(self, root: Path, *args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=root, capture_output=True, text=True, check=True
        ).stdout

    def committed(self, content: bytes) -> Path:
        root = Path(tempfile.mkdtemp())
        target = root / "evidence/artifact.bin"
        target.parent.mkdir(parents=True)
        target.write_bytes(content)
        self.git(root, "init")
        self.git(root, "add", ".")
        self.git(root, "-c", "user.email=a@b", "-c", "user.name=test", "commit", "-m", "record")
        return root

    def test_working_copy_may_move_on_when_history_holds_the_bytes(self) -> None:
        original = b"recorded bytes\n"
        root = self.committed(original)
        digest, size = recorded(original)
        (root / "evidence/artifact.bin").write_bytes(b"the file legitimately moved on\n")
        checks.safe_file(root, "evidence/artifact.bin", digest, size, "artifact")
        checks.safe_file(root, "evidence/artifact.bin", digest, size, "log")

    def test_digest_that_no_commit_holds_is_refused(self) -> None:
        root = self.committed(b"recorded bytes\n")
        digest, size = recorded(b"bytes that were never recorded\n")
        with self.assertRaises(ValueError):
            checks.safe_file(root, "evidence/artifact.bin", digest, size, "artifact")

    def test_uncommitted_evidence_still_resolves_from_the_working_copy(self) -> None:
        root = Path(tempfile.mkdtemp())
        target = root / "evidence/dirty.log"
        target.parent.mkdir(parents=True)
        target.write_bytes(b"dirty log\n")
        digest, size = recorded(b"dirty log\n")
        checks.safe_file(root, "evidence/dirty.log", digest, size, "log")

    def test_source_evidence_never_falls_back_to_history(self) -> None:
        root = self.committed(b"documented api\n")
        digest, size = recorded(b"documented api\n")
        with self.assertRaises(ValueError):
            checks.safe_file(root, "docs/api.rs", digest, size, "source")


if __name__ == "__main__":
    unittest.main()
