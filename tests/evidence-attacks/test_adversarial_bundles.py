import json
import base64
import subprocess
import tempfile
import os
import unittest
from pathlib import Path

VALIDATOR_SCRIPT = Path("scripts/validate-evidence-bundle.sh").resolve()
GH_BINARY = Path("gh_2.55.0_linux_amd64/bin/gh").resolve()

class TestAdversarialBundles(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.artifact_path = Path(self.temp_dir.name) / "catalog.json"
        self.artifact_path.write_text('{"phase_status": "BLOCKED"}')
        self.repo = "dmin/cellos"
        
        if not GH_BINARY.exists():
            # If we don't have the downloaded gh locally in CI, we just skip or assume `gh` in PATH
            self.gh_cmd = "gh"
        else:
            self.gh_cmd = str(GH_BINARY)

    def tearDown(self):
        self.temp_dir.cleanup()

    def run_validator(self, bundle_path):
        env = os.environ.copy()
        result = subprocess.run(
            [str(VALIDATOR_SCRIPT), str(self.artifact_path), str(bundle_path), self.repo, "--gh-binary", self.gh_cmd],
            capture_output=True,
            text=True,
            env=env
        )
        return result

    def test_truncation_attack(self):
        """Test that a truncated (invalid JSON) bundle is rejected."""
        bundle_path = Path(self.temp_dir.name) / "truncated.jsonl"
        bundle_path.write_text('{"mediaType": "application/vnd.dev.sigstore.bundle+json;version=0.1", "dsse')
        
        result = self.run_validator(bundle_path)
        self.assertNotEqual(result.returncode, 0, f"Validator should reject truncated bundle. stdout: {result.stdout}, stderr: {result.stderr}")
        self.assertIn("FAIL", result.stderr)

    def test_mutation_attack(self):
        """Test that a mutated bundle (invalid base64 payload) is rejected."""
        bundle_path = Path(self.temp_dir.name) / "mutated.jsonl"
        fake_bundle = {
            "mediaType": "application/vnd.dev.sigstore.bundle+json;version=0.1",
            "verificationMaterial": {
                "x509CertificateChain": {"certificates": [{"rawBytes": base64.b64encode(b"fake_cert").decode()}]}
            },
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": base64.b64encode(b'{"subject": [{"digest": {"sha256": "fake"}}]}').decode(),
                "signatures": [{"sig": base64.b64encode(b"fake_sig").decode()}]
            }
        }
        bundle_path.write_text(json.dumps(fake_bundle))
        
        # Mutate the artifact itself
        self.artifact_path.write_text("mutated content")
        
        result = self.run_validator(bundle_path)
        self.assertNotEqual(result.returncode, 0, f"Validator should reject mutated artifact/bundle. stdout: {result.stdout}, stderr: {result.stderr}")

    def test_wrong_runner_identity(self):
        """Test that a bundle from a wrong repo is rejected."""
        bundle_path = Path(self.temp_dir.name) / "wrong_id.jsonl"
        fake_bundle = {
            "mediaType": "application/vnd.dev.sigstore.bundle+json;version=0.1",
            "verificationMaterial": {
                "x509CertificateChain": {"certificates": [{"rawBytes": base64.b64encode(b"wrong_cert").decode()}]}
            },
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": base64.b64encode(b'{"subject": [{"digest": {"sha256": "fake"}}]}').decode(),
                "signatures": [{"sig": base64.b64encode(b"wrong_sig").decode()}]
            }
        }
        bundle_path.write_text(json.dumps(fake_bundle))
        
        # We simulate the wrong identity by providing a different expected repo
        self.repo = "attacker/cellos"
        
        result = self.run_validator(bundle_path)
        self.assertNotEqual(result.returncode, 0, f"Validator should reject wrong runner identity. stdout: {result.stdout}, stderr: {result.stderr}")

if __name__ == '__main__':
    unittest.main()
