#!/usr/bin/env python3
"""Phase 10 S10-3.3 import mode (D3-1 A) shared by the private QEMU lane runners.

A runner started with `--candidate <candidate.json>` does not build the cells it shares
with the candidate image. It calls this helper in three steps:

  import    Copy every shared cell (the lane's image paths that the candidate profile also
            packages, plus `init`) from the candidate into the lane's private release dir,
            each verified against the sha256 and signed-payload sha256 that
            `candidate.json` records. Records the sha256 of `candidate.json` itself and
            lists every other lane path as lane-only. Writes `candidate-import.json`.
  verify    After the runner re-signed the copies with its per-run key (its existing
            private-fixture signing call) and assembled the image: the signed-payload
            sha256 of every re-signed copy must equal the candidate's (only the 64
            signature bytes differ), and the assembled image/init must carry exactly
            the verified bytes. Writes `payload-match.json`; any mismatch exits 1, so
            the runner stops before its kernel build and boot.
  annotate  Write `candidate-provenance.json` (candidate.json sha256, mode=import,
            kernel fixture features, payload-match digest) and merge it as
            `candidate_import` into the lane's provenance/verdict JSON.

Signed payload = sha256 of `scripts/sign-cell.py` `_signed_payload(elf)`: every ELF byte
except the 64-byte `__ViCell_sig` payload. Standard library + cryptography only.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CANDIDATE_SCHEMA = "cellos-x86-candidate/1"
IMPORT_RECORD = "candidate-import.json"
PAYLOAD_MATCH = "payload-match.json"
CANDIDATE_PROVENANCE = "candidate-provenance.json"


class ImportRefused(Exception):
    """The candidate, a copy or a re-signed copy does not match; nothing may boot."""


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


_SIGNER = None
_READER = None


def signer():
    global _SIGNER
    if _SIGNER is None:
        _SIGNER = _load("phase10_import_sign_cell", REPO / "scripts" / "sign-cell.py")
    return _SIGNER


def reader():
    global _READER
    if _READER is None:
        _READER = _load("phase10_import_boot_config", REPO / "scripts" / "generate-boot-config.py")
    return _READER


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def payload_sha256(data: bytes) -> str | None:
    """Signed-payload sha256 of a signed ELF, or None when it carries no signature section."""
    try:
        return sha(signer()._signed_payload(data))
    except Exception:  # sign-cell asserts on a malformed or unsigned ELF
        return None


def signature_valid(data: bytes, public_key: bytes) -> bool:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    s = signer()
    try:
        bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = s._section_table(data)
        sig = s._find_section(data, s.SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits)
        Ed25519PublicKey.from_public_bytes(public_key).verify(sig, s._signed_payload(data))
        return True
    except Exception:
        return False


def write_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def _replace_file(dest: Path, data: bytes) -> None:
    """Unlink first (cargo uplifts by hard link), then write a fresh 0755 file."""
    temporary = dest.with_name(dest.name + ".candidate-import")
    temporary.unlink(missing_ok=True)
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o755)
    with os.fdopen(fd, "wb") as out:
        out.write(data)
    dest.unlink(missing_ok=True)
    os.replace(temporary, dest)


def _candidate_bytes(row: dict, path_key: str, what: str) -> bytes:
    """Read one candidate file and require its recorded sha256 and signed payload."""
    source = Path(row[path_key])
    if not source.is_file():
        raise ImportRefused(f"{what}: candidate file missing: {source}")
    data = source.read_bytes()
    if sha(data) != row.get("sha256"):
        raise ImportRefused(f"{what}: candidate file sha256 {sha(data)} != candidate.json {row.get('sha256')}")
    if not row.get("payload_sha256"):
        raise ImportRefused(f"{what}: candidate.json records no signed payload (unsigned file)")
    if payload_sha256(data) != row["payload_sha256"]:
        raise ImportRefused(f"{what}: candidate file signed payload differs from candidate.json")
    return data


def parse_pairs(values: list[str], flag: str) -> list[tuple[str, str]]:
    pairs = []
    for value in values or []:
        key, sep, rest = value.partition("=")
        if not sep or not key.startswith("/") or not rest:
            raise ImportRefused(f"{flag} needs IMAGE_PATH=VALUE, got {value!r}")
        pairs.append((key, rest))
    return pairs


def do_import(candidate: Path, profile: str, lane: str, release_dir: Path, evidence: Path,
              cells: list[tuple[str, str]], init: Path, lane_only: dict[str, str]) -> dict:
    raw = candidate.read_bytes()
    record = json.loads(raw)
    if record.get("schema") != CANDIDATE_SCHEMA:
        raise ImportRefused(f"{candidate}: schema {record.get('schema')!r} is not {CANDIDATE_SCHEMA!r}")
    if record.get("failures"):
        raise ImportRefused(f"{candidate}: the candidate records failures: {record['failures']}")
    prof = record.get("profiles", {}).get(profile)
    if not prof or "files" not in prof or "init" not in prof:
        raise ImportRefused(f"{candidate}: no image profile {profile!r}")
    release = release_dir.resolve()

    def inside_release(dest: Path) -> Path:
        resolved = dest.parent.resolve() / dest.name
        if resolved.parent != release:
            raise ImportRefused(f"{dest}: copy target outside the lane release dir {release}")
        return resolved

    unknown = sorted(set(lane_only) - {path for path, _ in cells})
    if unknown:
        raise ImportRefused(f"--lane-only names paths the lane does not package: {unknown}")

    shared, only, by_dest = [], [], {}
    for path, lane_file in cells:
        dest = Path(lane_file)
        if path in lane_only or path not in prof["files"]:
            reason = lane_only.get(path) or "not packaged in the candidate profile"
            only.append({"path": path, "lane_file": str(dest), "reason": reason})
            continue
        row = prof["files"][path]
        data = _candidate_bytes(row, "file", path)
        target = inside_release(dest)
        previous = by_dest.get(target)
        if previous is not None and previous != row["sha256"]:
            raise ImportRefused(f"{dest}: the lane packages one file at paths whose candidate bytes differ")
        if previous is None:
            _replace_file(target, data)
            by_dest[target] = row["sha256"]
        if sha(target.read_bytes()) != row["sha256"]:
            raise ImportRefused(f"{dest}: copy sha256 differs from candidate.json")
        shared.append({"path": path, "lane_file": str(dest), "candidate_file": row["file"],
                       "candidate_sha256": row["sha256"], "candidate_payload_sha256": row["payload_sha256"]})
    row = prof["init"]
    data = _candidate_bytes(row, "path", "init")
    target = inside_release(init)
    _replace_file(target, data)
    if sha(target.read_bytes()) != row["sha256"]:
        raise ImportRefused(f"{init}: copy sha256 differs from candidate.json")
    shared.append({"path": "init", "lane_file": str(init), "candidate_file": row["path"],
                   "candidate_sha256": row["sha256"], "candidate_payload_sha256": row["payload_sha256"]})
    out = {
        "schema": "cellos-phase10-candidate-import/1",
        "mode": "import",
        "lane": lane,
        "profile": profile,
        "candidate_json": {"path": str(candidate), "sha256": sha(raw)},
        "candidate_root": record.get("root"),
        "shared": shared,
        "lane_only": only,
    }
    write_json(evidence / IMPORT_RECORD, out)
    return out


def _load_record(evidence: Path) -> dict:
    path = evidence / IMPORT_RECORD
    if not path.is_file():
        raise ImportRefused(f"{path}: no import record (run `import` first)")
    return json.loads(path.read_text())


def do_verify(evidence: Path, image: Path | None, init_image: Path | None,
              trust_pub: Path | None) -> dict:
    record = _load_record(evidence)
    failures = []
    candidate = Path(record["candidate_json"]["path"])
    now = sha(candidate.read_bytes()) if candidate.is_file() else None
    if now != record["candidate_json"]["sha256"]:
        failures.append(f"candidate.json changed since import: {now} != {record['candidate_json']['sha256']}")
    public_key = trust_pub.read_bytes() if trust_pub else None
    image_files = reader().image_files(image) if image else None

    def in_image(path: str, data: bytes):
        if path == "init":
            return None if init_image is None else init_image.read_bytes() == data
        if image_files is None:
            return None
        return image_files.get(path) == data

    rows = []
    for entry in record["shared"]:
        data = Path(entry["lane_file"]).read_bytes()
        lane_payload = payload_sha256(data)
        row = {
            "path": entry["path"],
            "lane_file": entry["lane_file"],
            "candidate_sha256": entry["candidate_sha256"],
            "candidate_payload_sha256": entry["candidate_payload_sha256"],
            "lane_sha256": sha(data),
            "lane_payload_sha256": lane_payload,
            "equal": lane_payload is not None and lane_payload == entry["candidate_payload_sha256"],
            "byte_identical": sha(data) == entry["candidate_sha256"],
            "in_image": in_image(entry["path"], data),
        }
        if public_key is not None:
            row["lane_signature_valid"] = signature_valid(data, public_key)
            if not row["lane_signature_valid"]:
                failures.append(f"{entry['path']}: lane signature does not verify with the lane key")
        if not row["equal"]:
            failures.append(f"{entry['path']}: signed payload {lane_payload} != candidate {entry['candidate_payload_sha256']}")
        if row["in_image"] is False:
            failures.append(f"{entry['path']}: assembled image bytes differ from the verified lane file")
        rows.append(row)
    only = []
    for entry in record["lane_only"]:
        data = Path(entry["lane_file"]).read_bytes()
        only.append({**entry, "lane_only": True, "lane_sha256": sha(data),
                     "lane_payload_sha256": payload_sha256(data), "in_image": in_image(entry["path"], data)})
        if only[-1]["in_image"] is False:
            failures.append(f"{entry['path']}: assembled image bytes differ from the lane file")
    out = {
        "schema": "cellos-phase10-payload-match/1",
        "mode": "import",
        "lane": record["lane"],
        "profile": record["profile"],
        "candidate_json_sha256": record["candidate_json"]["sha256"],
        "binaries": rows,
        "lane_only": only,
        "all_equal": not failures,
        "failures": failures,
    }
    write_json(evidence / PAYLOAD_MATCH, out)
    return out


def do_annotate(evidence: Path, kernel_features: str | None, cellbox_features: str | None,
                into: list[Path]) -> dict:
    record = _load_record(evidence)
    target = evidence / CANDIDATE_PROVENANCE
    if kernel_features is not None:
        match_path = evidence / PAYLOAD_MATCH
        if not match_path.is_file():
            raise ImportRefused(f"{match_path}: no payload match (run `verify` first)")
        match = json.loads(match_path.read_text())
        if not match["all_equal"]:
            raise ImportRefused(f"{match_path}: payload match failed; nothing to annotate")
        info = {
            "mode": "import",
            "lane": record["lane"],
            "profile": record["profile"],
            "candidate_json": record["candidate_json"]["path"],
            "candidate_json_sha256": record["candidate_json"]["sha256"],
            "kernel_fixture_features": kernel_features,
            "cellbox_features": cellbox_features or "default",
            "payload_match_sha256": sha(match_path.read_bytes()),
            "payload_match_all_equal": True,
            "shared": [row["path"] for row in match["binaries"]],
            "lane_only": [row["path"] for row in match["lane_only"]],
            "counts_for": "behaviour of the payload-matched cells only; never kernel-level claims (S10-3.3)",
        }
        write_json(target, info)
    else:
        if not target.is_file():
            raise ImportRefused(f"{target}: missing (annotate with --kernel-features first)")
        info = json.loads(target.read_text())
    for path in into:
        value = json.loads(path.read_text())
        value["candidate_import"] = info
        write_json(path, value)
    return info


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    imp = sub.add_parser("import", help="copy and verify the shared candidate cells")
    imp.add_argument("--candidate", type=Path, required=True)
    imp.add_argument("--profile", required=True, choices=("headless", "desktop"))
    imp.add_argument("--lane", required=True)
    imp.add_argument("--release-dir", type=Path, required=True)
    imp.add_argument("--evidence", type=Path, required=True)
    imp.add_argument("--init", type=Path, required=True, help="lane init file (app-init)")
    imp.add_argument("--cell", action="append", default=[], metavar="IMAGE_PATH=LANE_FILE",
                     help="every path the lane packages and the lane file behind it")
    imp.add_argument("--lane-only", action="append", default=[], metavar="IMAGE_PATH=REASON",
                     help="keep a lane build although the candidate packages the path (lane flavour)")
    ver = sub.add_parser("verify", help="payload-match the re-signed copies before boot")
    ver.add_argument("--evidence", type=Path, required=True)
    ver.add_argument("--image", type=Path, help="assembled kernel_fs.img to check against the lane files")
    ver.add_argument("--init-image", type=Path, help="the init copy the kernel embeds")
    ver.add_argument("--trust-pub", type=Path, help="raw 32-byte public key the lane kernel trusts")
    ann = sub.add_parser("annotate", help="record candidate provenance in lane JSON files")
    ann.add_argument("--evidence", type=Path, required=True)
    ann.add_argument("--kernel-features", help="kernel fixture features (writes candidate-provenance.json)")
    ann.add_argument("--cellbox-features")
    ann.add_argument("--into", type=Path, action="append", default=[])
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "import":
            lane_only = dict(parse_pairs(args.lane_only, "--lane-only"))
            out = do_import(args.candidate.resolve(), args.profile, args.lane, args.release_dir,
                            args.evidence, parse_pairs(args.cell, "--cell"), args.init, lane_only)
            print(f"candidate import: candidate.json sha256 {out['candidate_json']['sha256']}; "
                  f"{len(out['shared'])} shared (verified copies), {len(out['lane_only'])} lane-only")
            for row in out["lane_only"]:
                print(f"    lane-only {row['path']}: {row['reason']}")
        elif args.command == "verify":
            out = do_verify(args.evidence, args.image, args.init_image, args.trust_pub)
            if out["failures"]:
                for failure in out["failures"]:
                    print("FAIL: payload match: " + failure, file=sys.stderr)
                print("FAIL: payload match failed; refusing to boot", file=sys.stderr)
                return 1
            print(f"PASS: payload match: {len(out['binaries'])} re-signed candidate cells equal the candidate's "
                  f"signed payloads ({sum(r['byte_identical'] for r in out['binaries'])} byte-identical); "
                  f"{len(out['lane_only'])} lane-only")
        else:
            info = do_annotate(args.evidence, args.kernel_features, args.cellbox_features, args.into)
            print(f"candidate provenance: mode={info['mode']} candidate.json {info['candidate_json_sha256']} "
                  f"kernel features {info['kernel_fixture_features']}")
    except ImportRefused as exc:
        print(f"FAIL: candidate import: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
