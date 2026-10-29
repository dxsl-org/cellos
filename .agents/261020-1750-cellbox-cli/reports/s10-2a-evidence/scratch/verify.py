#!/usr/bin/env python3
"""S10-2a: verify every packaged cell signature against the dev public key (sign-cell.py --verify),
the unsigned tier2 fixtures and cargo-target binaries stay unsigned, read-back equals manifest."""
import hashlib, importlib.util, json, subprocess, sys
from pathlib import Path

W = Path(sys.argv[1]); REPO = Path("/home/dmin/cellos")
spec = importlib.util.spec_from_file_location("r", REPO / "scripts/generate-boot-config.py")
reader = importlib.util.module_from_spec(spec); spec.loader.exec_module(reader)
sys.path.insert(0, str(REPO / "scripts")); from cellos_sign import signing


def verify(path):
    r = subprocess.run([sys.executable, str(REPO / "scripts/sign-cell.py"), "--verify", "--in", str(path)],
                       capture_output=True, text=True)
    return r.returncode == 0


ok = True
for profile in ("headless", "desktop"):
    out = W / profile
    m = json.loads((out / "manifest.json").read_text())
    files = reader.image_files(out / "kernel_fs.img")
    extract = W / "extract" / profile
    extract.mkdir(parents=True, exist_ok=True)
    print(f"== {profile}: key_class={m['key_class']} route={m['signing_route']} "
          f"kernel_fs.img {m['kernel_fs_img']['sha256']} ({m['kernel_fs_img']['size']} B)")
    assert hashlib.sha256((out / "kernel_fs.img").read_bytes()).hexdigest() == m["kernel_fs_img"]["sha256"]
    for path, entry in sorted(m["files"].items()):
        if "signed" not in entry:
            continue
        f = extract / path.strip("/").replace("/", "_")
        f.write_bytes(files[path])
        good = verify(f)
        expected = entry["signed"]
        status = "PASS" if good == expected else "FAIL"
        ok &= good == expected
        print(f"  {status} {path:20} signed={expected} dev-signature-valid={good} sha256={entry['sha256'][:16]}")
    good = verify(out / "init")
    ok &= good
    print(f"  {'PASS' if good else 'FAIL'} init                 signed=True dev-signature-valid={good} "
          f"sha256={m['init']['sha256'][:16]}")
    same = files["/bin/cellbox"] == files["/bin/cellbox-ssh"]
    ok &= same
    print(f"  alias byte-equal: {same}; blocked hashes: "
          f"{[p for p, d in files.items() if hashlib.sha256(d).hexdigest() in signing.BLOCKED_ARTIFACT_SHA256] or 'none'}"
          f"; /bin/shell {'/bin/shell' in files}; /bin/sshd {'/bin/sshd' in files}")
rel = W / "target/x86_64-unknown-none/release"
names = sorted({e["binary"] for p in ("headless", "desktop")
                for e in json.loads((W / p / "manifest.json").read_text())["files"].values() if "binary" in e}
               | {"app-init"})
unsigned = [n for n in names if not verify(rel / n)]
ok &= len(unsigned) == len(names)
print(f"cargo target binaries without a valid signature (copy-then-sign): {len(unsigned)}/{len(names)}")
print("VERDICT", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
