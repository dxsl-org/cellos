import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = ROOT / "scripts/validate-evidence-bundle.sh"
REPOSITORY = "dxsl-org/cellos"
REVISION = "8a2cb1cc1109011ba74f2633f2f4f876b0af8cdf"
WORKFLOW = f"{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main"


class TestAdversarialBundles(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        root = Path(self.temporary.name)
        self.directory = root / "evidence"
        self.state_directory = root / "trusted-state"
        self.directory.mkdir()
        self.state_directory.mkdir(mode=0o700)
        self.manifest = self.directory / "manifest.json"
        self.bundle = self.directory / "bundle.jsonl"
        self.store = self.state_directory / "consumed-sequence.json"
        self.bundle.write_text("{}\n", encoding="utf-8")
        self.fake_gh = root / "gh"
        self.fake_gh.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "if os.environ.get('FAKE_GH_REJECT') == '1': sys.exit(1)\n"
            "digest = os.environ['FAKE_SUBJECT_DIGEST']\n"
            "print(json.dumps([{'verificationResult': {'statement': {'subject': "
            "[{'digest': {'sha256': digest}}]}}}]))\n",
            encoding="utf-8",
        )
        self.fake_gh.chmod(0o700)
        (self.directory / "inputs").mkdir()
        (self.directory / "logs").mkdir()
        self.input = self.directory / "inputs/catalog.json"
        self.log = self.directory / "logs/test.log"
        self.input.write_text('{"phase_status":"BLOCKED"}\n', encoding="utf-8")
        self.log.write_text("test result: ok\n", encoding="utf-8")
        subprocess.run(
            [
                str(ROOT / "scripts/consume-evidence-sequence.py"),
                "--store", str(self.store), "--repository", REPOSITORY,
                "--workflow-ref", WORKFLOW, "--initialize",
            ],
            cwd=ROOT,
            check=True,
            capture_output=True,
        )

    def tearDown(self):
        self.temporary.cleanup()

    @staticmethod
    def member(path: Path, kind: str) -> dict:
        data = path.read_bytes()
        return {
            "name": path.name,
            "path": f"{kind}/{path.name}",
            "sha256": hashlib.sha256(data).hexdigest(),
            "bytes": len(data),
        }

    def write_manifest(self, sequence: str, *, workflow: str = WORKFLOW, runner: str = "github-hosted:Linux:X64"):
        value = {
            "schema": "cellos.authenticated-evidence/v1",
            "revision": REVISION,
            "sequence": sequence,
            "runner": runner,
            "workflow_ref": workflow,
            "command": "boot-suite",
            "result": "passed",
            "environment": {"ref": "refs/heads/main"},
            "inputs": [self.member(self.input, "inputs")],
            "logs": [self.member(self.log, "logs")],
            "images": [],
        }
        self.manifest.write_text(json.dumps(value), encoding="utf-8")

    def run_validator(
        self,
        sequence: str,
        *,
        reject_attestation: bool = False,
        subject_digest: str | None = None,
    ) -> subprocess.CompletedProcess:
        environment = os.environ.copy()
        environment["FAKE_SUBJECT_DIGEST"] = (
            subject_digest or hashlib.sha256(self.manifest.read_bytes()).hexdigest()
        )
        if reject_attestation:
            environment["FAKE_GH_REJECT"] = "1"
        return subprocess.run(
            [
                str(VALIDATOR),
                str(self.manifest),
                str(self.bundle),
                REPOSITORY,
                REVISION,
                sequence,
                str(self.store),
                "--gh-binary",
                str(self.fake_gh),
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            env=environment,
        )

    def test_exact_replay_is_rejected(self):
        self.write_manifest("100:1")
        self.assertEqual(self.run_validator("100:1").returncode, 0)
        replay = self.run_validator("100:1")
        self.assertNotEqual(replay.returncode, 0)
        self.assertIn("replay or regression", replay.stderr)

    def test_sequence_regression_is_rejected(self):
        self.write_manifest("100:2")
        self.assertEqual(self.run_validator("100:2").returncode, 0)
        self.write_manifest("99:99")
        self.assertNotEqual(self.run_validator("99:99").returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "100:2")

    def test_strictly_newer_sequence_replaces_store(self):
        self.write_manifest("100:1")
        self.assertEqual(self.run_validator("100:1").returncode, 0)
        self.write_manifest("100:2")
        self.assertEqual(self.run_validator("100:2").returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "100:2")

    def test_content_mutation_does_not_consume_sequence(self):
        self.write_manifest("101:1")
        self.log.write_text("mutated\n", encoding="utf-8")
        result = self.run_validator("101:1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "0:0")

    def test_failed_attestation_does_not_consume_sequence(self):
        self.write_manifest("102:1")
        result = self.run_validator("102:1", reject_attestation=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "0:0")

    def test_wrong_manifest_identity_does_not_consume_sequence(self):
        self.write_manifest("103:1", workflow="attacker/cellos/.github/workflows/ci.yml@refs/heads/main")
        self.assertNotEqual(self.run_validator("103:1").returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "0:0")
        self.write_manifest("103:1", runner="self-hosted:Linux:X64")
        self.assertNotEqual(self.run_validator("103:1").returncode, 0)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "0:0")

    def test_unprovisioned_store_fails_closed(self):
        self.store.unlink()
        self.write_manifest("104:1")
        result = self.run_validator("104:1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not provisioned", result.stderr)

    def test_attested_subject_mismatch_does_not_consume_sequence(self):
        self.write_manifest("104:1")
        result = self.run_validator("104:1", subject_digest="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("differs from attested subject", result.stderr)
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "0:0")

    def test_malformed_store_fails_closed(self):
        self.store.write_text("{", encoding="utf-8")
        self.write_manifest("104:1")
        self.assertNotEqual(self.run_validator("104:1").returncode, 0)
        self.assertEqual(self.store.read_text(), "{")

    def test_concurrent_consumers_allow_exactly_one(self):
        self.write_manifest("105:1")
        command = [
            str(VALIDATOR), str(self.manifest), str(self.bundle), REPOSITORY, REVISION,
            "105:1", str(self.store), "--gh-binary", str(self.fake_gh),
        ]
        environment = os.environ.copy()
        environment["FAKE_SUBJECT_DIGEST"] = hashlib.sha256(self.manifest.read_bytes()).hexdigest()
        processes = [
            subprocess.Popen(
                command, cwd=ROOT, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, env=environment,
            )
            for _ in range(2)
        ]
        returncodes = sorted(process.wait() for process in processes)
        self.assertEqual(returncodes, [0, 1])
        self.assertEqual(json.loads(self.store.read_text())["sequence"], "105:1")


if __name__ == "__main__":
    unittest.main()
