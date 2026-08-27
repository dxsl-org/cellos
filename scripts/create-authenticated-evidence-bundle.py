#!/usr/bin/env python3
"""Build a self-contained, content-addressed CI evidence bundle."""
import argparse
import hashlib
import json
import re
import shutil
from pathlib import Path

NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]*\Z")
SHA = re.compile(r"[0-9a-f]{40}\Z")


def hash_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stage(values, root, kind):
    target = root / kind
    target.mkdir(parents=True, exist_ok=True)
    records, names = [], set()
    for value in values:
        name, sep, source_name = value.partition("=")
        source = Path(source_name).resolve()
        if not sep or not NAME.fullmatch(name) or name in names or not source.is_file():
            raise ValueError(f"invalid {kind} member: {value}")
        names.add(name)
        destination = target / name
        shutil.copyfile(source, destination)
        records.append({"name": name, "path": f"{kind}/{name}", "sha256": hash_file(destination), "bytes": destination.stat().st_size})
    return sorted(records, key=lambda record: record["name"])


def environment(values):
    result = {}
    for value in values:
        key, sep, setting = value.partition("=")
        if not sep or not NAME.fullmatch(key) or key in result:
            raise ValueError(f"invalid environment setting: {value}")
        result[key] = setting
    return dict(sorted(result.items()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--runner", required=True)
    parser.add_argument("--workflow-ref", required=True)
    parser.add_argument("--sequence", required=True)
    parser.add_argument("--command", required=True)
    parser.add_argument("--input", action="append", default=[])
    parser.add_argument("--image", action="append", default=[])
    parser.add_argument("--log", action="append", default=[])
    parser.add_argument("--environment", action="append", default=[])
    args = parser.parse_args()
    if not SHA.fullmatch(args.revision) or not all((args.runner, args.workflow_ref, args.sequence, args.command)):
        raise SystemExit("revision, runner, workflow-ref, sequence, and command are required")
    if not args.input or not args.log:
        raise SystemExit("at least one input and raw log are required")
    output = args.output.resolve()
    root = output.parent
    root.mkdir(parents=True, exist_ok=True)
    bundle = {"schema": "cellos.authenticated-evidence/v1", "revision": args.revision,
              "runner": args.runner, "workflow_ref": args.workflow_ref, "sequence": args.sequence,
              "command": args.command, "result": "passed", "environment": environment(args.environment),
              "inputs": stage(args.input, root, "inputs"), "images": stage(args.image, root, "images"),
              "logs": stage(args.log, root, "logs")}
    output.write_text(json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        raise SystemExit(str(error)) from error
