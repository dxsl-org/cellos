import subprocess
import os

env = os.environ.copy()
result = subprocess.run(
    ["/home/dmin/cellos/gh_2.55.0_linux_amd64/bin/gh", "attestation", "verify", "/tmp/testdir/catalog.json", "--bundle", "/tmp/testdir/mutated.jsonl", "--repo", "dmin/cellos"],
    capture_output=True,
    text=True,
    env=env
)
print("RETURNCODE:", result.returncode)
print("STDOUT:", repr(result.stdout))
print("STDERR:", repr(result.stderr))
