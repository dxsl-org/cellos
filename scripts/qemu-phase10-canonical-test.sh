#!/usr/bin/env bash
# Phase 10 S10-3.2 L0: the canonical runner. Boots the EXACT candidate bytes recorded in a
# candidate.json (phase-10-s0-proposal.md §2 S10-3.1) - no fixture kernel, no rebuilt cell,
# no lane key - and judges them on the physical UART (headless) or on the GUI (desktop).
#
# README (what this proves, and what it does not)
# -----------------------------------------------
# Inputs: the profile's ISO, kernel, kernel_fs.img, init, manifest.json and boot-config files
# must hash to candidate.json, or the runner refuses before anything is built or booted. The
# ISO is attached read-only (and re-hashed after the run). Nothing canonical is written: the
# work directory must be a private /tmp/cellbox-phase10-* directory that aliases none of
# kernel/src/, target/ or build/.
#
# Data comes only from disposable NVMe fixture disks built here with the existing tools:
#   disk-main  scripts/phase07_rg_fixture.py (P1 FAT32 /mnt/sd: the profile's own boot config,
#              the /rg multi-chunk/ignore/hidden/binary/encoding tree; P5 CellosFS /srv with the
#              read-denied `cellos/` child), plus the cat/edit/VT data tree written by
#              `phase10_canonical_drive.py fixtures` (the qemu-phase04-gui-test.sh VT bytes and
#              scripts/phase05_vt_hostile.py) copied onto the same P1 volume with mtools, as
#              phase07_rg_fixture.py copies /rg. The canonical VFS mounts one FAT volume
#              (/mnt/sd, P1 of the first NVMe disk), so this is the FAT data volume.
#   disk-ssh   scripts/phase09_trust_disk.py (P1 the profile's boot config, P5 /srv with the
#              preprovisioned trust store for the D79-1 fixture: 10.0.2.2:PORT exact, host:PORT
#              a different key).
# P1 is the config override volume, so both disks carry the profile's own boot-config bytes
# (hash-checked against the packaged /etc/cellos/*.toml): the boot plan stays the candidate's.
#
# One QEMU at a time, owned by the driver, with private unix serial and QMP sockets under
# $WORK/sock. Each profile run is two sequential boots of the same ISO:
#   main  `-netdev user,restrict=on` (no forward, nothing reaches the host): every marker
#         except the ssh scenario;
#   ssh   the D79-1 Docker OpenSSH fixture on 127.0.0.1:<random> (scripts/phase09_ssh_fixture.sh),
#         reached as 10.0.2.2 through `-netdev user` without restrict, `-cpu qemu64,+rdrand,+rdseed`,
#         still without any hostfwd/guestfwd: P10C-ssh-serial / P10G-ssh only.
# Headless drives the serial `cellbox sh` on the UART; desktop drives only the GUI (QMP input
# and screendump, the pixel oracle imported from scripts/phase04_gui_drive.py); UART bytes on
# the desktop go to the focused GUI Terminal (decision (c)), so the desktop serial shell is
# never scripted.
#
# Output ($WORK/evidence): verdict.json (one record per marker, written by the driver),
# serial logs per boot, judged frames (screens/), provenance.json (candidate hashes, QEMU
# version and argv, fixture disk hashes), evidence.sha256, and no-secret.json: the dev signing
# seed, the fixture password and the fixture host private key are absent from every log.
# TCG is the qualifying accelerator; `--accel kvm` runs are marked non-qualifying.
# Exit status: 0 every marker passed; 1 a failure after QEMU started; 2 usage;
# 3 a pre-QEMU failure (candidate not booted, including fixture/input/preparation failures).
# Status 3 neither counts as a run nor changes the 6/6 streak (S10-3c H2).
#
# NOT proven here: anything a fixture kernel row is needed for (L3), the Tier-3 guest ssh
# endpoint, publickey auth, physical hardware.
#
# Usage:
#   W=$(mktemp -d /tmp/cellbox-phase10-XXXX)
#   bash scripts/qemu-phase10-canonical-test.sh --candidate CANDIDATE.json --profile headless|desktop --work "$W"
# Optional: --accel tcg|kvm (default tcg), --window SECONDS (boot window, default 900).
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PYTHON_BIN="${PYTHON_BIN:-python3}"

CAND="" PROFILE="" W="" ACCEL="tcg" WINDOW="900"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --candidate) CAND="$2"; shift 2 ;;
        --profile) PROFILE="$2"; shift 2 ;;
        --work) W="$2"; shift 2 ;;
        --accel) ACCEL="$2"; shift 2 ;;
        --window) WINDOW="$2"; shift 2 ;;
        *) echo "usage: $0 --candidate FILE --profile headless|desktop --work /tmp/cellbox-phase10-XXXX [--accel tcg|kvm] [--window S]" >&2; exit 2 ;;
    esac
done
[[ -n "$CAND" && -n "$PROFILE" && -n "$W" ]] || { echo "FAIL: --candidate, --profile and --work are required" >&2; exit 2; }
case "$PROFILE" in headless|desktop) ;; *) echo "FAIL: --profile must be headless or desktop" >&2; exit 2 ;; esac
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: --accel must be tcg or kvm" >&2; exit 2 ;; esac
[[ "$WINDOW" =~ ^[0-9]+$ ]] || { echo "FAIL: --window must be seconds" >&2; exit 2; }
# Until the driver successfully launches QEMU, *every* failure is infrastructure, not a run.
# Only consult the launch record after the work directory has been validated and invalidated.
BOOT_TRACKING=0
finish() {
    local status=$?
    trap - EXIT
    if (( status != 0 )) && {
        (( BOOT_TRACKING == 0 )) || [[ ! -f "$W/evidence/qemu-started" ]];
    }; then
        echo "FAIL: candidate not booted: pre-QEMU preparation failed (status 3, not a run)" >&2
        status=3
    fi
    if [[ -n "${FIXTURE:-}" ]]; then
        bash scripts/phase09_ssh_fixture.sh stop "$FIXTURE" >/dev/null 2>&1 || true
        rm -f "$W/scratch/fixture-password" "$W/scratch/fixture-host-key"
    fi
    exit "$status"
}
trap finish EXIT
[[ -f "$CAND" && ! -L "$CAND" ]] || { echo "FAIL: candidate.json missing or a symlink: $CAND" >&2; exit 1; }
case "$W" in /tmp/cellbox-phase10-*) ;; *) echo "FAIL: work directory must be /tmp/cellbox-phase10-*" >&2; exit 1 ;; esac
[[ -d "$W" && ! -L "$W" ]] || { echo "FAIL: work directory missing or a symlink: $W" >&2; exit 1; }
for guarded in "$ROOT/kernel/src" "$ROOT/target" "$ROOT/build"; do
    [[ "$(realpath -m "$W")" != "$(realpath -m "$guarded")"* ]] || { echo "FAIL: work dir aliases $guarded" >&2; exit 1; }
done
"$PYTHON_BIN" - "$W" <<'PY'
import os, pathlib, sys
w = pathlib.Path(sys.argv[1])
if w.is_symlink() or w.resolve().parent != pathlib.Path("/tmp") or not w.name.startswith("cellbox-phase10-"):
    raise SystemExit("FAIL: work directory must be a real direct /tmp/cellbox-phase10-* directory")
if w.stat().st_uid != os.getuid():
    raise SystemExit("FAIL: work directory is not owned by this uid")
for name in ("evidence", "fixtures", "disk-main", "disk-ssh", "fixture", "sock", "scratch", "glyph-host", "host-target"):
    path = w / name
    if path.is_symlink() or not path.resolve().is_relative_to(w.resolve()):
        raise SystemExit("FAIL: private output root aliases another directory: " + name)
    if path.exists() and any(p.is_symlink() for p in path.rglob("*")):
        raise SystemExit("FAIL: private output tree contains a symlink: " + name)
PY

EVIDENCE="$W/evidence"
FIXTURE="$W/fixture"
# Invalidate every output first (a stale disk or verdict must never be reused).
rm -rf "$EVIDENCE" "$W/fixtures" "$W/disk-main" "$W/disk-ssh" "$FIXTURE" "$W/sock" "$W/scratch" "$W/glyph-host"
mkdir -p "$EVIDENCE" "$W/fixtures" "$W/disk-main" "$W/disk-ssh" "$FIXTURE" "$W/sock" "$W/scratch"
chmod 700 "$FIXTURE" "$W/sock"
BOOT_TRACKING=1
T_START=$(date +%s)

echo "==> Verifying every input against candidate.json ($PROFILE)"
"$PYTHON_BIN" - "$CAND" "$PROFILE" "$EVIDENCE/inputs.json" <<'PY'
import hashlib, json, sys
from pathlib import Path
cand_path, profile, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
c = json.loads(cand_path.read_text())
if c.get("schema") != "cellos-x86-candidate/1":
    raise SystemExit("FAIL: candidate.json schema is not cellos-x86-candidate/1")
if c.get("failures"):
    raise SystemExit(f"FAIL: candidate.json records failures: {c['failures']}")
p = c["profiles"][profile]
bad = []
def need(label, path, want):
    if not Path(path).is_file() or Path(path).is_symlink():
        bad.append(f"{label}: missing {path}")
    elif sha(path) != want:
        bad.append(f"{label}: {path} sha256 differs from candidate.json")
need("iso", p["iso"]["path"], p["iso"]["sha256"])
need("kernel", p["kernel"]["path"], p["kernel"]["sha256"])
need("kernel_fs_img", p["kernel_fs_img"]["path"], p["kernel_fs_img"]["sha256"])
need("init", p["init"]["path"], p["init"]["sha256"])
need("manifest", Path(p["dir"]) / "manifest.json", p["manifest_sha256"])
if json.loads((Path(p["dir"]) / "manifest.json").read_text()) != p["manifest"]:
    bad.append("manifest.json differs from the manifest recorded in candidate.json")
if p["manifest"]["kernel_fs_img"]["sha256"] != p["kernel_fs_img"]["sha256"]:
    bad.append("manifest kernel_fs_img sha256 differs from candidate.json")
if not (p["kernel"].get("embeds_kernel_fs_img") and p["kernel"].get("embeds_init")):
    bad.append("candidate.json does not record that the kernel embeds kernel_fs.img and init")
for path, rec in p["files"].items():
    need("file " + path, rec["file"], rec["sha256"])
    if rec.get("signed") and rec.get("dev_signature_valid") is not True:
        bad.append(f"file {path}: signed but no valid dev signature recorded")
cfg = Path(p["dir"]) / "boot-config"
for name in ("autoload.toml", "services.toml", "system.toml"):
    rec = p["files"].get("/etc/cellos/" + name)
    if rec is None:
        bad.append(f"/etc/cellos/{name} not packaged")
    else:
        need("boot-config " + name, cfg / name, rec["sha256"])
if bad:
    raise SystemExit("FAIL: candidate inputs refused:\n  " + "\n  ".join(bad))
out.write_text(json.dumps({
    "candidate_json": str(cand_path.resolve()), "candidate_sha256": sha(cand_path), "profile": profile,
    "iso": p["iso"], "kernel": {k: p["kernel"][k] for k in ("path", "sha256", "size", "features")},
    "kernel_fs_img": p["kernel_fs_img"], "init": {k: p["init"][k] for k in ("path", "sha256", "size", "payload_sha256")},
    "manifest_sha256": p["manifest_sha256"], "boot_config": str(cfg), "dir": p["dir"],
    "files": {k: {"sha256": v["sha256"], "payload_sha256": v.get("payload_sha256")} for k, v in p["files"].items()},
}, indent=1) + "\n")
print(f"inputs verified: iso {p['iso']['sha256'][:16]}… kernel {p['kernel']['sha256'][:16]}… "
      f"({'+'.join(p['kernel']['features'])}) image {p['kernel_fs_img']['sha256'][:16]}… {len(p['files'])} files")
PY
ISO="$("$PYTHON_BIN" -c 'import json,sys; print(json.load(open(sys.argv[1]))["iso"]["path"])' "$EVIDENCE/inputs.json")"
BOOTCFG="$("$PYTHON_BIN" -c 'import json,sys; print(json.load(open(sys.argv[1]))["boot_config"])' "$EVIDENCE/inputs.json")"
ISO_SHA="$(sha256sum "$ISO" | awk '{print $1}')"

GLYPH_ARGS=()
if [[ "$PROFILE" == desktop ]]; then
    echo "==> Building the host glyph renderer (phase04-gui-glyphs; private target dir)"
    mkdir -p "$W/glyph-host/src"
    sed "s#@ROOT@#$ROOT#g" scripts/phase04-gui-glyphs/Cargo.toml.in > "$W/glyph-host/Cargo.toml"
    cp scripts/phase04-gui-glyphs/src/main.rs "$W/glyph-host/src/main.rs"
    cp Cargo.lock "$W/glyph-host/Cargo.lock"
    CARGO_TARGET_DIR="$W/host-target" cargo build --release --offline --target x86_64-unknown-linux-gnu \
        --manifest-path "$W/glyph-host/Cargo.toml"
    GLYPH_BIN="$W/host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs"
    [[ -x "$GLYPH_BIN" ]] || { echo "FAIL: glyph renderer not built" >&2; exit 1; }
    GLYPH_ARGS=(--glyph-bin "$GLYPH_BIN")
fi

echo "==> Main fixture disk: rg tree + /srv denied child (phase07_rg_fixture.py) and the FAT data tree"
"$PYTHON_BIN" scripts/phase07_rg_fixture.py "$W/disk-main/disk.img" "$W/disk-main/rg-fixture-expected.json" "$BOOTCFG"
"$PYTHON_BIN" scripts/phase10_canonical_drive.py fixtures "$W/fixtures/data" --manifest "$EVIDENCE/data-fixtures.json"
P1="$W/disk-main/disk.img@@$((2048 * 512))"
for top in "$W"/fixtures/data/*; do
    MTOOLS_SKIP_CHECK=1 mcopy -s -i "$P1" "$top" ::/
done
"$PYTHON_BIN" - "$P1" "$EVIDENCE/data-fixtures.json" "$W/fixtures/data" <<'PY'
import json, os, subprocess, sys, tempfile
from pathlib import Path
target, manifest, src = sys.argv[1], json.loads(Path(sys.argv[2]).read_text()), Path(sys.argv[3])
env = dict(os.environ, MTOOLS_SKIP_CHECK="1")
with tempfile.TemporaryDirectory() as tmp:
    for guest, rec in manifest.items():
        rel = guest[len("/mnt/sd/"):]
        out = Path(tmp) / rel.replace("/", "_")
        subprocess.run(["mcopy", "-n", "-i", target, "::/" + rel, str(out)], check=True, env=env)
        if out.read_bytes() != (src / rel).read_bytes():
            raise SystemExit(f"FAIL: {guest} reads back different bytes from the fixture disk")
print(f"data tree on P1: {len(manifest)} files read back exactly")
PY
cp "$W/disk-main/rg-fixture-expected.json" "$EVIDENCE/rg-fixture-expected.json"

echo "==> Starting the disposable OpenSSH fixture (D79-1)"
# The EXIT trap owns cleanup, including a partially started fixture and every preparation error.
if ! bash scripts/phase09_ssh_fixture.sh start "$FIXTURE" > "$EVIDENCE/fixture.txt"; then
    echo "FAIL: candidate not booted: the ssh fixture did not start (status 3, not a run)" >&2
    exit 3
fi
cat "$EVIDENCE/fixture.txt"
PORT="$(cat "$FIXTURE/port")"
cp "$FIXTURE/host_ed25519.pub" "$FIXTURE/sshd-version" "$FIXTURE/fingerprint" "$EVIDENCE/"
# Secrets the no-secret check must not find anywhere (kept outside the evidence tree).
install -m 600 "$FIXTURE/password" "$W/scratch/fixture-password"
install -m 600 "$FIXTURE/host_ed25519" "$W/scratch/fixture-host-key"

echo "==> SSH fixture disk: the protected trust store (phase09_trust_disk.py)"
"$PYTHON_BIN" scripts/phase09_trust_disk.py "$W/disk-ssh/disk.img" "$BOOTCFG" "$FIXTURE/host_ed25519.pub" "$PORT"
sha256sum "$W/disk-main/disk.img" "$W/disk-ssh/disk.img" | sed "s#$W#\$WORK#" > "$EVIDENCE/fixture-disks.sha256"

echo "==> Booting the candidate ($PROFILE, $ACCEL): main boot (restricted SLIRP), then the ssh boot"
set +e
"$PYTHON_BIN" scripts/phase10_canonical_drive.py run --profile "$PROFILE" --work "$W" --iso "$ISO" \
    --evidence "$EVIDENCE" --candidate "$CAND" --disk-main "$W/disk-main/disk.img" --disk-ssh "$W/disk-ssh/disk.img" \
    --ssh-dir "$FIXTURE" --rg-expected "$W/disk-main/rg-fixture-expected.json" --accel "$ACCEL" --window "$WINDOW" \
    --secret-file "$W/scratch/fixture-password" --secret-file "$W/scratch/fixture-host-key" "${GLYPH_ARGS[@]}"
DRIVE_STATUS=$?
set -e

bash scripts/phase09_ssh_fixture.sh stop "$FIXTURE"
cp "$FIXTURE/sshd.log" "$EVIDENCE/sshd.log"

echo "==> Provenance, no-secret check, evidence digest"
"$PYTHON_BIN" - "$W" "$EVIDENCE" "$ISO" "$ISO_SHA" "$ACCEL" "$(( $(date +%s) - T_START ))" <<'PY'
import hashlib, importlib.util, json, subprocess, sys
from pathlib import Path
w, ev, iso, iso_sha, accel, duration = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5], int(sys.argv[6])
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
inputs = json.loads((ev / "inputs.json").read_text())
prov = {
    "schema": "cellos-phase10-l0-provenance/1",
    "candidate": {k: inputs[k] for k in ("candidate_json", "candidate_sha256", "profile", "iso", "kernel",
                                          "kernel_fs_img", "init", "manifest_sha256")},
    "iso_sha256_after_run": sha(iso),
    "iso_unchanged": sha(iso) == iso_sha,
    "qemu_version": subprocess.check_output(["qemu-system-x86_64", "--version"], text=True).splitlines()[0],
    "qemu_argv": {p.stem.removeprefix("qemu-argv-"): p.read_text().strip() for p in sorted(ev.glob("qemu-argv-*.txt"))},
    "accel": accel, "qualifying": accel == "tcg",
    "fixture_disks_sha256_before_boot": (ev / "fixture-disks.sha256").read_text().split("\n"),
    "data_fixtures": json.loads((ev / "data-fixtures.json").read_text()),
    "sshd_version": (ev / "sshd-version").read_text().strip(),
    "fixture_host_key_fingerprint": (ev / "fingerprint").read_text().strip(),
    "runner_sha256": {n: sha(Path("scripts") / n) for n in ("qemu-phase10-canonical-test.sh", "phase10_canonical_drive.py",
                                                            "phase04_gui_drive.py", "phase07_rg_fixture.py", "phase09_trust_disk.py",
                                                            "phase09_ssh_fixture.sh", "phase05_vt_hostile.py")},
    "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
    "run_seconds": duration,
}
glyph = w / "host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs"
if glyph.exists():
    prov["glyph_renderer_sha256"] = sha(glyph)
(ev / "provenance.json").write_text(json.dumps(prov, indent=2) + "\n")
spec = importlib.util.spec_from_file_location("sign_cell", "scripts/sign-cell.py")
mod = importlib.util.module_from_spec(spec)
sys.modules["sign_cell"] = mod
spec.loader.exec_module(mod)
secrets = {"dev-signing-seed": mod.DEV_SEED, "fixture-password": (w / "scratch/fixture-password").read_bytes().strip()}
key = (w / "scratch/fixture-host-key").read_bytes()
secrets.update({f"fixture-host-key-line-{i}": l for i, l in enumerate(key.splitlines()) if len(l) > 20 and b"PRIVATE KEY" not in l})
hits = []
for p in sorted(ev.rglob("*")):
    if p.is_file() and p.suffix != ".png":
        data = p.read_bytes()
        hits += [f"{p.relative_to(ev)}: {n}" for n, s in secrets.items() if s in data or s.hex().encode() in data]
        if b"PRIVATE KEY" in data:
            hits.append(f"{p.relative_to(ev)}: PRIVATE KEY block")
(ev / "no-secret.json").write_text(json.dumps({"checked": sorted(secrets), "hits": hits}, indent=1) + "\n")
lines = [f"{sha(p)}  {p.relative_to(ev)}" for p in sorted(ev.rglob("*")) if p.is_file() and p.name != "evidence.sha256"]
(ev / "evidence.sha256").write_text("\n".join(lines) + "\n")
print("ISO unchanged:", prov["iso_unchanged"], "| no-secret hits:", hits or "none", "| run", duration, "s")
if hits or not prov["iso_unchanged"]:
    raise SystemExit(1)
PY
rm -f "$W/scratch/fixture-password" "$W/scratch/fixture-host-key"
exit "$DRIVE_STATUS"
