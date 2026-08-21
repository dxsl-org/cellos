"""Content-addressed cohorts used by a passing capability claim."""

from __future__ import annotations

import datetime as dt
import subprocess
from functools import lru_cache
from pathlib import Path

from . import public_api as sdk_source
from . import source
from .checks import GIT, HEX, canonical_digest, exact, integer, safe_file, text, timestamp

WITNESSES = {"source", "compile", "test_runtime", "delivery", "architecture", "tier"}
DETAIL_KEYS = {
    "source": {"source_path", "source_sha256", "public_api_sha256", "abi_version"},
    "compile": {"compiler", "target", "language", "feature_selection", "cfg", "cargo_features", "cargo_profile", "rustflags", "runtime_profile"},
    "test_runtime": {"test_name", "expected_outcome", "architecture", "environment", "hardware", "firmware_sha256"},
    "delivery": {"build", "manifest", "package", "signing", "verification", "development_only_scope"},
    "architecture": {"architecture", "target"},
    "tier": {"tier", "admission", "ipc", "grant", "mmio", "dma", "security_negative"},
}
DENOMINATOR_KEYS = {
    "compiler", "target", "language", "feature_selection", "cfg", "cargo_features", "cargo_profile", "rustflags", "runtime_profile",
    "source_path", "source_sha256", "public_api_sha256", "abi_version",
}
TARGET_PREFIX = {"riscv64": "riscv64", "aarch64": "aarch64", "x86_64": "x86_64"}


@lru_cache(maxsize=512)
def _git_identity(root: str, revision: str, tree: str) -> bool:
    """Resolve immutable commit/tree identities once per validation process."""
    path = Path(root)
    commit = subprocess.run(["git", "rev-parse", f"{revision}^{{commit}}"], cwd=path, capture_output=True, text=True)
    actual_tree = subprocess.run(["git", "rev-parse", f"{revision}^{{tree}}"], cwd=path, capture_output=True, text=True)
    return not commit.returncode and commit.stdout.strip() == revision and not actual_tree.returncode and actual_tree.stdout.strip() == tree


def git_identity(root: Path, revision: str, tree: str) -> None:
    """Require the claimed commit and tree to exist and correspond exactly."""
    if not _git_identity(str(root.resolve()), revision, tree):
        raise ValueError("cohort revision or base tree is not a resolvable Git identity")


def dirty_bundle(root: Path, bundle: object, dirty: object, revision: object, tree: object) -> None:
    """Require dirty claims to reproduce all changed and untracked bytes."""
    if not isinstance(dirty, bool) or not GIT.fullmatch(text(revision, "revision")) or not GIT.fullmatch(text(tree, "base tree")):
        raise ValueError("invalid revision or base tree")
    git_identity(root, revision, tree)
    if not dirty:
        if bundle is not None:
            raise ValueError("clean cohort has dirty bundle")
        return
    item = exact(bundle, {"base_revision", "base_tree", "patch", "untracked", "digest"}, "dirty bundle")
    if item["base_revision"] != revision or item["base_tree"] != tree:
        raise ValueError("dirty bundle base mismatch")
    patch = item["patch"]
    exact(patch, {"path", "sha256", "size_bytes", "kind"}, "dirty patch")
    safe_file(root, patch["path"], patch["sha256"], patch["size_bytes"], patch["kind"])
    if (root / patch["path"]).read_bytes() != subprocess.run(["git", "diff", "--binary", revision], cwd=root, capture_output=True).stdout:
        raise ValueError("dirty patch bytes do not match worktree")
    blobs = item["untracked"]
    if not isinstance(blobs, list) or [blob.get("path") for blob in blobs] != sorted(blob.get("path") for blob in blobs):
        raise ValueError("dirty bundle paths must be sorted")
    for blob in blobs:
        exact(blob, {"path", "sha256", "size_bytes", "kind"}, "dirty blob")
        safe_file(root, blob["path"], blob["sha256"], blob["size_bytes"], blob["kind"])
        if subprocess.run(["git", "cat-file", "-e", f"{revision}:{blob['path']}"], cwd=root, capture_output=True).returncode == 0:
            raise ValueError("dirty untracked blob exists at base")
    actual_untracked = subprocess.run(["git", "ls-files", "--others", "--exclude-standard"], cwd=root, capture_output=True, text=True)
    if actual_untracked.returncode or [blob["path"] for blob in blobs] != sorted(filter(None, actual_untracked.stdout.splitlines())):
        raise ValueError("dirty bundle does not enumerate current untracked files")
    material = {key: item[key] for key in ("base_revision", "base_tree", "patch", "untracked")}
    if item["digest"] != canonical_digest(material):
        raise ValueError("dirty bundle digest mismatch")


def validate(root: Path, value: object, claim: dict, subject: dict, as_of: dt.datetime) -> None:
    """Require six same-claim witnesses with one tuple, revision, and TTL clock."""
    keys = {"claim_id", "subject", "revision", "base_tree", "dirty", "dirty_bundle", "source_sha256", "matrix_sha256", "tuple", "denominator", "public_api", "witnesses"}
    item = exact(value, keys, "evidence cohort")
    if item["claim_id"] != claim["id"] or item["subject"] != claim["subject"] or item["tuple"] != claim["tuple"]:
        raise ValueError("cohort claim, subject, or tuple mismatch")
    if item["source_sha256"] != claim["source_sha256"] or item["matrix_sha256"] != claim["matrix_sha256"]:
        raise ValueError("cohort source binding mismatch")
    denominator = exact(item["denominator"], DENOMINATOR_KEYS, "SDK denominator")
    for key, value_ in denominator.items():
        text(value_, f"denominator {key}")
    if denominator["runtime_profile"] != claim["tuple"]["runtime_profile"]:
        raise ValueError("denominator runtime profile mismatch")
    actual_key = "|".join((denominator["compiler"], denominator["target"], denominator["language"], denominator["cfg"], denominator["rustflags"], denominator["feature_selection"], denominator["cargo_features"], denominator["cargo_profile"], denominator["runtime_profile"], claim["tuple"]["tier"]))
    if actual_key != source.denominator(denominator["target"], denominator["feature_selection"]):
        raise ValueError("denominator is not a ratified canonical tuple")
    if denominator["source_path"] != source.SOURCE_PATH or denominator["source_sha256"] != claim["source_sha256"]:
        raise ValueError("denominator source anchor mismatch")
    if not HEX.fullmatch(denominator["public_api_sha256"]):
        raise ValueError("denominator public API digest invalid")
    if not denominator["target"].startswith(TARGET_PREFIX[claim["tuple"]["cpu"]]):
        raise ValueError("denominator target architecture mismatch")
    public_api = item["public_api"]
    if denominator["public_api_sha256"] != sdk_source.validate(
        root, public_api, item["revision"], item["dirty"], denominator["abi_version"]
    ):
        raise ValueError("public API aggregate digest mismatch")
    dirty_bundle(root, item["dirty_bundle"], item["dirty"], item["revision"], item["base_tree"])
    witnesses = item["witnesses"]
    if not isinstance(witnesses, list) or {entry.get("class") for entry in witnesses} != WITNESSES:
        raise ValueError("cohort must have six distinct witnesses")
    for witness in witnesses:
        exact(
            witness,
            {"class", "recorded_at", "expires_at", "ttl_seconds", "owner", "runner", "command", "result", "details", "artifacts"},
            "witness",
        )
        kind = witness["class"]
        if kind not in WITNESSES or witness["result"] != "PASS":
            raise ValueError("witness class or result invalid")
        for key in ("owner", "runner", "command"):
            text(witness[key], f"witness {key}")
        details = exact(witness["details"], DETAIL_KEYS[kind], f"{kind} details")
        for key, detail in details.items():
            text(detail, f"{kind}.{key}")
        for key in (set(details) & {"source_sha256", "public_api_sha256"}):
            if not HEX.fullmatch(details[key]):
                raise ValueError(f"{kind}.{key}: SHA-256 required")
        if kind == "source" and details != {key: denominator[key] for key in DETAIL_KEYS["source"]}:
            raise ValueError("source witness denominator mismatch")
        if kind == "compile" and details != {key: denominator[key] for key in DETAIL_KEYS["compile"]}:
            raise ValueError("compile witness denominator mismatch")
        if kind == "architecture" and details != {"architecture": claim["tuple"]["cpu"], "target": denominator["target"]}:
            raise ValueError("architecture witness denominator mismatch")
        if kind in {"test_runtime", "architecture"} and details["architecture"] != claim["tuple"]["cpu"]:
            raise ValueError("architecture witness does not match claim")
        if kind == "test_runtime" and details["environment"] != claim["tuple"]["environment"]:
            raise ValueError("runtime witness does not match claim environment")
        if kind == "test_runtime":
            hardware = subject["board_revision"] if subject["environment"] == "physical" else subject["host_vmm"]
            firmware = subject["firmware_digest"] if subject["environment"] == "physical" else "N/A"
            if details["hardware"] != hardware or details["firmware_sha256"] != firmware:
                raise ValueError("runtime witness does not match execution subject")
        if kind == "delivery":
            gates = [details[key] for key in ("build", "manifest", "package", "signing", "verification")]
            if any(value_ not in {"PASS", "N/A"} for value_ in gates):
                raise ValueError("delivery gate result invalid")
            if ("N/A" in gates) == (details["development_only_scope"] == "N/A"):
                raise ValueError("delivery N/A needs an explicit development-only scope")
        if kind == "tier" and any(details[key] != claim["tuple"][key] for key in DETAIL_KEYS["tier"]):
            raise ValueError("tier witness does not match claim tuple")
        if witness["owner"] == witness["runner"]:
            raise ValueError("witness owner and runner must be independent")
        recorded, expires = timestamp(witness["recorded_at"], "witness recorded"), timestamp(witness["expires_at"], "witness expiry")
        ttl = integer(witness["ttl_seconds"], "witness ttl")
        if ttl <= 0 or expires != recorded + dt.timedelta(seconds=ttl) or recorded > as_of or expires <= as_of:
            raise ValueError("witness TTL invalid")
        artifacts = witness["artifacts"]
        if not isinstance(artifacts, list) or not artifacts:
            raise ValueError("witness needs raw artifacts")
        for artifact in artifacts:
            exact(artifact, {"path", "sha256", "size_bytes", "kind"}, "witness artifact")
            safe_file(root, artifact["path"], artifact["sha256"], artifact["size_bytes"], artifact["kind"])
        if kind == "source" and not any(
            artifact["path"] == denominator["source_path"]
            and artifact["sha256"] == denominator["source_sha256"]
            and artifact["kind"] == "source"
            for artifact in artifacts
        ):
            raise ValueError("source witness needs its bound source artifact")
        if kind == "source" and not all(artifact in artifacts for artifact in public_api):
            raise ValueError("source witness needs the complete public API snapshot")
        if kind in {"compile", "test_runtime", "tier"} and not any(
            artifact["kind"] == "log" for artifact in artifacts
        ):
            raise ValueError(f"{kind} witness needs a raw log")
