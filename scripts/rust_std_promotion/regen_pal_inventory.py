#!/usr/bin/env python3
"""Re-pin the rust-std PAL feasibility inventory to a new nightly's rust-src.

A rustc move invalidates the PAL compiler-strategy decision
(`artifacts/compiler-strategy-decision.md`, "Maintenance Cost and
Invalidation"). This tool does the mechanical half of the re-inventory so the
reviewers and signers judge content, not hashes:

* `pal-hook-support-map.json`: toolchain block, every `source_manifest` hash and
  its `source_digest`, and the `sys/mod.rs` module manifest (order, line,
  visibility, source hash) re-read from the new tree;
* `approval-input-manifest.json`: every input hash (rust-src and repository)
  and the rust-src root, written in its canonical form;
* the new manifest digest replaces the old one in every excluded record.

It never changes a hook's classification, contract or evidence text. It
prints, for the reviewers, every pinned source that changed and the hooks whose
evidence points into one. Paths upstream moved are mapped with `--rename`;
a pinned source that is missing and unmapped is an error.

Usage:
  scripts/rust_std_promotion/regen_pal_inventory.py --channel nightly-YYYY-MM-DD \
      [--rename OLD=NEW ...] [--write]
  scripts/rust_std_promotion/regen_pal_inventory.py --bind-records
Without --write it reports and changes nothing. --write rewrites the support
map and the manifest only. --bind-records then puts the manifest's digest into
the excluded records (decision package and the four approvals); run it only
once both COMPILER-INTEGRATION signers have signed the re-decision, because
those records are what claims the approvals cover these inputs.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / ".agents/260821-1800-tier1-rust-std-pal-feasibility"
SUPPORT_MAP = PACKAGE / "artifacts/pal-hook-support-map.json"
MANIFEST = PACKAGE / "artifacts/approval-input-manifest.json"
SYS_MOD = "library/std/src/sys/mod.rs"
DECLARATION = re.compile(r"^(pub )?mod (\w+);$")
RUST_SRC = "rust-src://"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical(value: object) -> bytes:
    """The manifest's canonical form: sorted keys, comma/colon, ASCII, one LF."""
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def toolchain_facts(channel: str) -> tuple[Path, str, str]:
    """rust-src root, 9-char commit hash and rustc release of an installed nightly."""
    sysroot = subprocess.run(["rustc", f"+{channel}", "--print", "sysroot"], check=True,
                             capture_output=True, text=True).stdout.strip()
    verbose = subprocess.run(["rustc", f"+{channel}", "-vV"], check=True, capture_output=True, text=True).stdout
    fields = dict(line.split(": ", 1) for line in verbose.splitlines() if ": " in line)
    root = Path(sysroot) / "lib/rustlib/src/rust"
    if not (root / SYS_MOD).is_file():
        raise SystemExit(f"rust-src missing for {channel}: {root}")
    return root, fields["commit-hash"][:9], fields["release"]


def declarations(text: str) -> list[tuple[int, str, str]]:
    """(1-based line, module, visibility) of the leading `mod` block of sys/mod.rs."""
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        match = DECLARATION.match(line.strip())
        if match:
            found.append((number, match.group(2), "public" if match.group(1) else "private"))
        elif found and line.strip() and not line.startswith("#"):
            break
    return found


def rename_everywhere(value: object, renames: dict[str, str]) -> object:
    """Apply path renames inside every string of a JSON value."""
    if isinstance(value, str):
        for old, new in renames.items():
            value = value.replace(old, new)
        return value
    if isinstance(value, list):
        return [rename_everywhere(item, renames) for item in value]
    if isinstance(value, dict):
        return {key: rename_everywhere(item, renames) for key, item in value.items()}
    return value


def regen_support_map(support: dict, root: Path, channel: str, commit: str, release: str) -> list[str]:
    """Rewrite the map in place; return the pinned paths whose bytes changed."""
    changed = []
    for entry in support["source_manifest"]:
        path = root / entry["path"]
        if not path.is_file():
            raise SystemExit(f"pinned source missing in {channel}: {entry['path']} (map it with --rename)")
        digest = sha256(path)
        if digest != entry["sha256"]:
            changed.append(entry["path"])
        entry["sha256"] = digest
    rows = {row["module"]: row for row in support["scope"]["sys_module_manifest"]}
    declared = declarations((root / SYS_MOD).read_text())
    if {module for _, module, _ in declared} != set(rows):
        raise SystemExit(f"sys module set changed: {sorted({m for _, m, _ in declared} ^ set(rows))}")
    support["scope"]["sys_module_manifest"] = [
        {**rows[module], "declaration_line": line, "evidence": f"{SYS_MOD}:{line}",
         "visibility": visibility, "source_sha256": sha256(root / rows[module]["source_path"])}
        for line, module, visibility in declared
    ]
    first, last = declared[0][0], declared[-1][0]
    support["scope"]["basis"] = re.sub(rf"{re.escape(SYS_MOD)}:\d+-\d+", f"{SYS_MOD}:{first}-{last}",
                                       support["scope"]["basis"])
    support["toolchain"].update({
        "channel": channel, "commit_hash": commit, "rustc_version": release, "rust_src_root": str(root),
        "source_digest": hashlib.sha256(canonical(support["source_manifest"])).hexdigest(),
    })
    return changed


def regen_manifest(manifest: dict, root: Path) -> None:
    manifest["source_roots"]["rust-src"] = str(root)
    for entry in manifest["inputs"]:
        path = root / entry["path"][len(RUST_SRC):] if entry["path"].startswith(RUST_SRC) else ROOT / entry["path"]
        if not path.is_file():
            raise SystemExit(f"approval input missing: {entry['path']}")
        entry["sha256"] = sha256(path)


def hooks_touching(support: dict, changed: list[str]) -> dict[str, list[str]]:
    touching: dict[str, list[str]] = {}
    for hook in support["hooks"]:
        text = json.dumps(hook)
        hits = sorted({path for path in changed if path in text})
        if hits:
            touching[hook["hook_id"]] = hits
    return touching


def bind_records() -> int:
    """Move every excluded record from the digest the four approvals share to the manifest's.

    Other digests in the decision package (source digest, map digest) are prose
    the re-decision updates by hand.
    """
    manifest = json.loads(MANIFEST.read_text())
    digest = sha256(MANIFEST)
    records = manifest["excluded_self_referential_records"][1:]
    bound = {
        found
        for record in records if "/approvals/" in record
        for found in re.findall(r"\b[0-9a-f]{64}\b", (ROOT / record).read_text())
    }
    if len(bound) != 1:
        raise SystemExit(f"the approvals do not share one bound digest: {sorted(bound)}")
    old = bound.pop()
    for record in records:
        path = ROOT / record
        path.write_text(path.read_text().replace(old, digest))
        print(f"{record}: {old} -> {digest}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--channel")
    parser.add_argument("--rename", action="append", default=[], metavar="OLD=NEW")
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--bind-records", action="store_true")
    args = parser.parse_args()
    if args.bind_records:
        return bind_records()
    if not args.channel:
        parser.error("--channel is required unless --bind-records")
    renames = dict(item.split("=", 1) for item in args.rename)
    root, commit, release = toolchain_facts(args.channel)

    old_manifest_digest = sha256(MANIFEST)
    support = rename_everywhere(json.loads(SUPPORT_MAP.read_text()), renames)
    manifest = rename_everywhere(json.loads(MANIFEST.read_text()), renames)
    changed = regen_support_map(support, root, args.channel, commit, release)
    support_bytes = (json.dumps(support, indent=2, ensure_ascii=True) + "\n").encode()

    # The support map is itself an approval input: hash it as it will be written.
    regen_manifest(manifest, root)
    for entry in manifest["inputs"]:
        if ROOT / entry["path"] == SUPPORT_MAP:
            entry["sha256"] = hashlib.sha256(support_bytes).hexdigest()
    manifest_bytes = canonical(manifest)
    new_manifest_digest = hashlib.sha256(manifest_bytes).hexdigest()

    print(f"toolchain {args.channel} ({release}, {commit}); rust-src {root}")
    print(f"pinned sources changed: {len(changed)} of {len(support['source_manifest'])}")
    for path in changed:
        print(f"  {path}")
    print("hooks whose evidence points into a changed source (re-inventory these):")
    for hook_id, paths in hooks_touching(support, changed).items():
        print(f"  {hook_id}: {', '.join(paths)}")
    print(f"approval manifest digest {old_manifest_digest} -> {new_manifest_digest}")
    if not args.write:
        print("dry run: nothing written (pass --write)")
        return 0

    SUPPORT_MAP.write_bytes(support_bytes)
    MANIFEST.write_bytes(manifest_bytes)
    print("written; records stay bound to the old digest until --bind-records")
    return 0


if __name__ == "__main__":
    sys.exit(main())
