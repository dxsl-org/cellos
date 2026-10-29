#!/usr/bin/env python3
"""S10-3 C6: write W/cand/candidate.json, the single record of the candidate.

Extracts every packaged file of each profile image to W/cand/<profile>/files/, records its sha256
and signed-payload sha256 (sign-cell.py `_signed_payload`; null for unsigned files), verifies every
signed cell and init against the DEV public key, proves each kernel ELF embeds the exact
kernel_fs.img and init bytes, and records the ISOs. Prints the candidate.json sha256.
"""
import hashlib, importlib.util, json, subprocess, sys
from pathlib import Path

W = Path(sys.argv[1])
REPO = Path("/home/dmin/cellos")
CAND = W / "cand"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


reader = load("p10_reader", REPO / "scripts/generate-boot-config.py")
signer = load("p10_signer", REPO / "scripts/sign-cell.py")
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey  # noqa: E402

DEV_PUB = signer._pub_bytes(signer._priv_from_seed(signer.DEV_SEED))
sha = lambda data: hashlib.sha256(data).hexdigest()


def payload_and_verify(data):
    """(signed-payload sha256, dev signature valid) for a signed ELF."""
    bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = signer._section_table(data)
    sig = signer._find_section(data, signer.SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits)
    payload = signer._signed_payload(data)
    try:
        Ed25519PublicKey.from_public_bytes(DEV_PUB).verify(sig, payload)
        valid = True
    except Exception:
        valid = False
    return sha(payload), valid


def kernel_record(elf, features, image, init):
    data = elf.read_bytes()
    return {"path": str(elf), "sha256": sha(data), "size": len(data), "features": features,
            "embeds_kernel_fs_img": data.find(image) >= 0, "embeds_init": data.find(init) >= 0}


def git(*args):
    return subprocess.run(["git", *args], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()


record = {"schema": "cellos-x86-candidate/1", "root": str(CAND), "profiles": {}}
failures = []
for profile in ("headless", "desktop"):
    out = CAND / profile
    manifest_raw = (out / "manifest.json").read_bytes()
    manifest = json.loads(manifest_raw)
    image = (out / "kernel_fs.img").read_bytes()
    init = (out / "init").read_bytes()
    files = reader.image_files(out / "kernel_fs.img")
    extract = out / "files"
    extract.mkdir(exist_ok=True)
    entries = {}
    for path, data in sorted(files.items()):
        entry = manifest["files"][path]
        if sha(data) != entry["sha256"]:
            failures.append(f"{profile}{path}: image bytes differ from manifest")
        name = path.strip("/").replace("/", "-") if path.startswith("/etc/") else Path(path).name
        target = extract / name
        target.write_bytes(data)
        row = {"file": str(target), "sha256": sha(data), "size": len(data),
               "binary": entry.get("binary"), "package": entry.get("package"),
               "signed": entry.get("signed", False), "payload_sha256": None, "dev_signature_valid": None}
        if row["signed"]:
            row["payload_sha256"], row["dev_signature_valid"] = payload_and_verify(data)
            if not row["dev_signature_valid"]:
                failures.append(f"{profile}{path}: dev signature does not verify")
        entries[path] = row
    init_payload, init_valid = payload_and_verify(init)
    if not init_valid:
        failures.append(f"{profile} init: dev signature does not verify")
    if manifest["kernel_fs_img"]["sha256"] != sha(image) or manifest["init"]["sha256"] != sha(init):
        failures.append(f"{profile}: kernel_fs.img/init differ from manifest")
    kernel = kernel_record(CAND / f"{profile}.kernel.elf", ["default"], image, init)
    iso = (CAND / f"{profile}.iso").read_bytes()
    record["profiles"][profile] = {
        "dir": str(out),
        "manifest_sha256": sha(manifest_raw),
        "manifest": manifest,
        "files": entries,
        "init": {"path": str(out / "init"), "sha256": sha(init), "size": len(init),
                 "payload_sha256": init_payload, "dev_signature_valid": init_valid},
        "kernel_fs_img": {"path": str(out / "kernel_fs.img"), "sha256": sha(image), "size": len(image)},
        "kernel": kernel,
        "iso": {"path": str(CAND / f"{profile}.iso"), "sha256": sha(iso), "size": len(iso)},
    }
    if not (kernel["embeds_kernel_fs_img"] and kernel["embeds_init"]):
        failures.append(f"{profile}: kernel ELF does not embed the exact image/init")
headless = record["profiles"]["headless"]
image = Path(headless["kernel_fs_img"]["path"]).read_bytes()
init = Path(headless["init"]["path"]).read_bytes()
pc_kernel = kernel_record(CAND / "pc.kernel.elf", ["default", "board-x86-pc"], image, init)
pc_iso = (CAND / "pc.iso").read_bytes()
record["profiles"]["pc"] = {"image_profile": "headless", "kernel": pc_kernel,
                            "iso": {"path": str(CAND / "pc.iso"), "sha256": sha(pc_iso), "size": len(pc_iso)}}
if not (pc_kernel["embeds_kernel_fs_img"] and pc_kernel["embeds_init"]):
    failures.append("pc: kernel ELF does not embed the exact headless image/init")
record["source"] = {
    "git_head": git("rev-parse", "HEAD"),
    "manifest_source_headless": headless["manifest"]["source"],
    "recipes_sha256": {p: sha((REPO / p).read_bytes()) for p in (
        "scripts/assemble-x86-image.py", "scripts/cellos_sign/signing.py", "scripts/cellos_sign/cli.py",
        "scripts/sign-cell.py", "scripts/sign-policy.py", "tools/mkfat32.py", "scripts/x86/make-iso-ci.sh",
        "Cargo.lock")},
    "rustc": subprocess.run(["rustc", "-Vv"], cwd=REPO, capture_output=True, text=True).stdout,
    "dev_cell_public_key_sha256": sha(DEV_PUB),
}
record["failures"] = failures
path = CAND / "candidate.json"
path.write_text(json.dumps(record, indent=2) + "\n")
print("candidate.json sha256", sha(path.read_bytes()))
for profile in ("headless", "desktop", "pc"):
    p = record["profiles"][profile]
    print(profile, "kernel", p["kernel"]["sha256"], p["kernel"]["size"], "embeds",
          p["kernel"]["embeds_kernel_fs_img"], p["kernel"]["embeds_init"], "iso", p["iso"]["sha256"])
print("failures:", failures or "none")
sys.exit(1 if failures else 0)
