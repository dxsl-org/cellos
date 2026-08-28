import json
import base64
import os
import subprocess

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
with open("/tmp/testdir/mutated.jsonl", "w") as f:
    json.dump(fake_bundle, f)

with open("/tmp/testdir/catalog.json", "w") as f:
    f.write("mutated content")

result = subprocess.run(
    ["/home/dmin/cellos/gh_2.55.0_linux_amd64/bin/gh", "attestation", "verify", "/tmp/testdir/catalog.json", "--bundle", "/tmp/testdir/mutated.jsonl", "--repo", "dmin/cellos"],
    capture_output=True,
    text=True
)
print("RETURNCODE:", result.returncode)
print("STDOUT:", repr(result.stdout))
print("STDERR:", repr(result.stderr))
