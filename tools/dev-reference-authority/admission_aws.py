"""Closed offline checks for the authorized AWS DEV identity evidence."""

import hashlib
from pathlib import Path

from admission_schema import AdmissionError, load_json


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def attachment_path(evidence_root: Path, name: str) -> Path | None:
    relative = Path(name)
    if relative.is_absolute() or name != relative.as_posix() or relative == Path(".") or ".." in relative.parts:
        return None
    candidate = (evidence_root / relative).resolve()
    try:
        candidate.relative_to(evidence_root)
    except ValueError:
        return None
    return candidate if candidate != evidence_root else None


def aws_identity_problems(account: dict, evidence_dir: Path) -> list[str]:
    attachment = account["identity_evidence"]
    path = attachment_path(evidence_dir.resolve(), attachment["name"])
    if path is None:
        return ["AWS identity evidence path must be relative, canonical, and beneath evidence directory"]
    if not path.is_file():
        return [f"AWS identity evidence missing: {attachment['name']}"]
    if file_sha256(path) != attachment["sha256"]:
        return ["AWS identity evidence sha256 mismatch"]
    try:
        captured = load_json(path)
    except AdmissionError as exc:
        return [f"AWS identity evidence invalid: {exc}"]
    if not isinstance(captured, dict):
        return ["AWS identity evidence must be an object"]
    problems = []
    account_id = account["account_id"]
    if captured.get("Account") != account_id:
        problems.append("captured AWS account does not match inventory")
    arn = captured.get("Arn")
    if not isinstance(arn, str) or f":{account_id}:" not in arn:
        problems.append("captured AWS ARN does not bind the inventory account")
    if arn == f"arn:aws:iam::{account_id}:root":
        problems.append("captured AWS principal must not be account root")
    if captured.get("ConfiguredRegion") != account["region"]:
        problems.append("captured AWS region does not match inventory")
    problems.append(
        "AWS read-only permissions cannot be proven by the currently authorized "
        "identity-and-region commands; admission remains blocked"
    )
    return problems
