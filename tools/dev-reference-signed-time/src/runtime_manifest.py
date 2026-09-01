"""Strict packaged-manifest and Lambda environment admission."""

import os
from collections.abc import Mapping
from pathlib import Path

from manifest import decode_manifest
from manifest_model import MAX_MANIFEST_BYTES, SignedTimeManifest

_ERROR = "runtime manifest admission failed"
_ENV_BINDINGS = {
    "AWS_REGION": "aws_region",
    "SIGNED_TIME_TABLE_NAME": "allocator_table_name",
    "SIGNED_TIME_LINEAGE_TABLE_NAME": "lineage_table_name",
    "SIGNED_TIME_KMS_KEY_ARN": "kms_key_id",
    "SIGNED_TIME_LINEAGE_KMS_KEY_ARN": "lineage_kms_key_id",
}


class RuntimeManifestError(RuntimeError):
    """Stable value-free failure for packaged runtime configuration."""


def load_runtime_manifest(
    path: str | Path | None = None,
    environ: Mapping[str, str] = os.environ,
) -> SignedTimeManifest:
    """Load canonical packaged bytes and require exact environment bindings.

    ``path`` defaults to ``manifest.json`` beside this module. ``environ`` must
    expose the exact AWS region, table names, and KMS key ARNs selected by the
    manifest. Any file, schema, canonicality, or binding failure raises
    ``RuntimeManifestError``.
    """
    failed = False
    manifest = None
    try:
        selected = Path(__file__).with_name("manifest.json") if path is None else Path(path)
        with selected.open("rb") as source:
            data = source.read(MAX_MANIFEST_BYTES + 1)
        if len(data) > MAX_MANIFEST_BYTES or not isinstance(environ, Mapping):
            raise ValueError("invalid runtime manifest input")
        manifest = decode_manifest(data)
        for variable, field in _ENV_BINDINGS.items():
            if environ.get(variable) != getattr(manifest, field):
                raise ValueError("runtime environment does not match manifest")
    except Exception:
        failed = True
    if failed or type(manifest) is not SignedTimeManifest:
        raise RuntimeManifestError(_ERROR) from None
    return manifest
