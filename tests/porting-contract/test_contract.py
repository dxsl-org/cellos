#!/usr/bin/env python3
"""Host-side drift checks for the generated POSIX-shim contract."""
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GEN = ROOT / "scripts/gen-posix-shim-contract.py"
DOC = ROOT / "docs/guides/posix-shim-contract.generated.md"
SYSIO = ROOT / "libs/api/src/services/posix/sysio.rs"

def run(expect_ok: bool):
    result = subprocess.run(
        ["python3", str(GEN), "--check"], cwd=ROOT, text=True, capture_output=True
    )
    if (result.returncode == 0) != expect_ok:
        raise AssertionError(result.stdout + result.stderr)

run(True)
original_doc = DOC.read_text()
try:
    DOC.write_text(original_doc.replace("| `_fcntl`", "| `_fcntl_removed`", 1))
    run(False)
finally:
    DOC.write_text(original_doc)

original_sysio = SYSIO.read_text()
try:
    needle = "pub unsafe extern \"C\" fn _fcntl(_fd: c_int, _cmd: c_int, _arg: c_int) -> c_int {\n    -1\n}"
    assert needle in original_sysio
    SYSIO.write_text(original_sysio.replace(needle, needle.replace("-1", "0"), 1))
    run(False)
finally:
    SYSIO.write_text(original_sysio)

run(True)
print("porting contract drift checks: PASS")
