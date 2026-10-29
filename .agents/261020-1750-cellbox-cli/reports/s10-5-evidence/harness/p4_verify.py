#!/usr/bin/env python3
"""S10-5 V1-V3 (and a re-run of the V1/V3 hash checks after later steps).

Usage: p4_verify.py [label]   -> writes <evidence>/p4-verify-<label>.json (default label 'v1-v3').
Exit non-zero on any failure."""
import hashlib, importlib.util, json, mmap, subprocess, sys
from pathlib import Path

REPO = Path("/home/dmin/cellos")
EVID = REPO / ".agents/261020-1750-cellbox-cli/reports/s10-5-evidence"
DUR = Path.home() / ".cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"
EMB = REPO / "kernel/src/embedded-x86_64"
KERNEL = REPO / "target/x86_64-unknown-none/release/cellos-kernel"
sys.path.insert(0, str(REPO / "scripts"))


def sha(p):
    with open(p, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def main():
    label = sys.argv[1] if len(sys.argv) > 1 else "v1-v3"
    res, fails = {}, []

    def check(name, ok, detail=None):
        res[name] = {"ok": bool(ok), **({"detail": detail} if detail is not None else {})}
        if not ok:
            fails.append(name)

    cj = json.loads((DUR / "candidate.json").read_text())
    d = cj["profiles"]["desktop"]
    # V1 byte identity
    pairs = {"init": (EMB / "init", DUR / "desktop/init"),
             "kernel_fs.img": (EMB / "kernel_fs.img", DUR / "desktop/kernel_fs.img"),
             "manifest.json": (EMB / "manifest.json", DUR / "desktop/manifest.json")}
    for n in ("autoload.toml", "services.toml", "system.toml"):
        pairs["boot-config/" + n] = (EMB / "boot-config" / n, DUR / "desktop/boot-config" / n)
    for n, (a, b) in pairs.items():
        same = a.read_bytes() == b.read_bytes()
        check(f"V1 {n} cmp+sha", same and sha(a) == sha(b), {"sha256": sha(a), "bytes": a.stat().st_size})
    check("V1 init sha == candidate.json", sha(EMB / "init") == d["init"]["sha256"])
    check("V1 kernel_fs.img sha == candidate.json", sha(EMB / "kernel_fs.img") == d["kernel_fs_img"]["sha256"])
    check("V1 manifest sha == candidate.json", sha(EMB / "manifest.json") == d["manifest_sha256"])
    check("V1 boot-config dir has exactly 3 files",
          sorted(p.name for p in (EMB / "boot-config").iterdir()) == ["autoload.toml", "services.toml", "system.toml"])
    check("V1 no temp leftovers in embedded dir",
          not [p.name for p in EMB.iterdir() if p.name.startswith(".promote")])
    spec = importlib.util.spec_from_file_location("g", REPO / "scripts/generate-boot-config.py")
    g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)
    from cellos_sign import signing
    files = g.image_files(str(EMB / "kernel_fs.img"))
    want = {p: e["sha256"] for p, e in d["files"].items()}
    got = {p: hashlib.sha256(v).hexdigest() for p, v in files.items()}
    check("V1 image has the 32 manifest paths", set(files) == set(want) and len(want) == 32,
          {"count": len(files), "extra": sorted(set(files) - set(want)), "missing": sorted(set(want) - set(files))})
    check("V1 every image file sha256 == candidate.json",
          all(got.get(p) == h for p, h in want.items()),
          [p for p, h in want.items() if got.get(p) != h])
    check("V1 no /bin/shell, /bin/sshd", not {"/bin/shell", "/bin/sshd"} & set(files))
    check("V1 no quarantined hash in any image file", not set(got.values()) & signing.BLOCKED_ARTIFACT_SHA256)
    check("V1 /bin/cellbox == /bin/cellbox-ssh", files["/bin/cellbox"] == files["/bin/cellbox-ssh"])
    import tomllib
    auto = tomllib.loads(files["/etc/cellos/autoload.toml"].decode())["cells"]
    shells = [r for r in auto if r.get("path") == "/bin/cellbox"]
    check("V1 autoload starts exactly one /bin/cellbox ['sh']", len(shells) == 1 and shells[0].get("args") == ["sh"])
    svc = tomllib.loads(files["/etc/cellos/services.toml"].decode())["cells"]
    check("V1 no sshd/shell/config service rows",
          not [r for r in svc if r.get("name") in ("sshd", "shell", "config")])

    # V2 kernel binding
    check("V2 kernel sha == desktop.kernel.elf", sha(KERNEL) == d["kernel"]["sha256"] == "dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a")
    with open(KERNEL, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        oi = m.find((EMB / "kernel_fs.img").read_bytes())
        on = m.find((EMB / "init").read_bytes())
    check("V2 kernel embeds kernel_fs.img at 2,064,272", oi == 2064272, oi)
    check("V2 kernel embeds init at 1,655,200", on == 1655200, on)
    r = subprocess.run([sys.executable, str(REPO / "scripts/x86/check_default_kernel.py"), str(KERNEL), str(EMB)],
                       capture_output=True, text=True)
    check("V2 check_default_kernel.py admits it", r.returncode == 0, r.stderr.strip())

    # V3 collateral
    before = json.loads((Path("/tmp/cellbox-s10-3d-resume/canonical/before.json")).read_text())
    changed = sorted(p for p, e in before.items() if sha(REPO / p) != e["sha256"])
    expected = sorted(["kernel/src/embedded-x86_64/init", "kernel/src/embedded-x86_64/kernel_fs.img",
                       "kernel/src/embedded-x86_64/boot-config/autoload.toml",
                       "kernel/src/embedded-x86_64/boot-config/services.toml",
                       "target/x86_64-unknown-none/release/cellos-kernel"])
    # boot-config/system.toml is byte-identical in the old and the promoted set (74ecea5c...), so it
    # is replaced but does not change; it is checked against the candidate in V1.
    check("V3 469-path delta is exactly the intended five (system.toml identical)", changed == expected,
          {"changed": changed, "unexpected": sorted(set(changed) - set(expected)),
           "not_changed": sorted(set(expected) - set(changed))})
    check("V3 boot-config/system.toml unchanged 74ecea5c",
          sha(EMB / "boot-config/system.toml") == "74ecea5cd57faacfa1e32d1b006870eb3a85dea5e0b4f9e95753a57ceb147293")
    check("V3 manifest.json added (untracked)", (EMB / "manifest.json").is_file())
    check("V3 candidate source dir hashes", all(sha(p) == h for p, h in [
        (Path("/tmp/cellbox-s10-3d-resume/cand/candidate.json"), "4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821")]))
    check("V3 durable candidate.json", sha(DUR / "candidate.json") == "4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821")
    check("V3 kernel/src/main.rs unchanged a9636fee", sha(REPO / "kernel/src/main.rs") == "a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64")
    check("V3 build/vicell-x86.iso unchanged eb1cb813", sha(REPO / "build/vicell-x86.iso") == "eb1cb8130476120a2f25ab63f6d1ef45036d497722005ffef93e62433abfef09")
    arch = sorted((Path.home() / ".cache/cellos-quarantine").iterdir())[-1]
    man = (arch / "MANIFEST.sha256").read_text().splitlines()
    check("V3 quarantine archive re-hash", all(sha(arch / line.split("  ", 1)[1]) == line.split("  ", 1)[0] for line in man), str(arch))

    (EVID / f"p4-verify-{label}.json").write_text(json.dumps({"results": res, "failures": fails}, indent=2, sort_keys=True) + "\n")
    print("FAILURES:" if fails else "ALL OK", fails)
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
