"""Fail-closed cold-start composition for the signed-time Lambda runtime."""

from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any

from handler import SignedTimeHandler
from kms_public_key import KmsPublicKeyLoader
from kms_signer import KmsSigner
from manifest_derivation import (
    derive_clock_policy,
    derive_kms_key_pins,
    derive_lineage_contract,
    derive_lineage_key_pins,
    derive_roughtime_config,
)
from roughtime_adapter import RoughtimeClockAdapter
from runtime_floor import load_host_time_floor
from runtime_manifest import load_runtime_manifest
from state_reader import DynamoStateReader
from state_store import DynamoStateStore
from table_identity import DynamoTableIdentityVerifier

_ERROR = "signed-time runtime composition failed"
ClientFactory = Callable[[str, str], Any]


class RuntimeCompositionError(RuntimeError):
    """Stable value-free cold-start composition failure."""


def _default_client_factory(service: str, region: str) -> Any:
    import boto3
    from botocore.config import Config

    return boto3.client(
        service,
        region_name=region,
        config=Config(
            connect_timeout=1,
            ignore_configured_endpoint_urls=True,
            read_timeout=3,
            retries={"total_max_attempts": 1, "mode": "standard"},
        ),
    )


def compose_runtime(
    manifest_path: str | Path | None = None,
    environ: Mapping[str, str] | None = None,
    client_factory: ClientFactory = _default_client_factory,
) -> SignedTimeHandler:
    """Build the sole validated runtime graph or raise ``RuntimeCompositionError``.

    ``manifest_path`` selects canonical packaged bytes, ``environ`` supplies
    exact deployment bindings, and ``client_factory`` receives one service name
    plus the manifest region. Both AWS clients disable SDK retries.
    """
    failed = False
    runtime = None
    try:
        if not callable(client_factory):
            raise TypeError("client factory is not callable")
        manifest = (
            load_runtime_manifest(manifest_path)
            if environ is None
            else load_runtime_manifest(manifest_path, environ)
        )
        dynamodb = client_factory("dynamodb", manifest.aws_region)
        kms = client_factory("kms", manifest.aws_region)
        response_key_id, response_digest = derive_kms_key_pins(manifest)
        lineage_key_id, lineage_digest = derive_lineage_key_pins(manifest)
        response_public_key = KmsPublicKeyLoader(
            kms.get_public_key, response_key_id, response_digest
        ).load()
        lineage_public_key = KmsPublicKeyLoader(
            kms.get_public_key, lineage_key_id, lineage_digest
        ).load()
        contract = derive_lineage_contract(manifest, lineage_public_key)
        DynamoTableIdentityVerifier(dynamodb, contract).verify()
        runtime = SignedTimeHandler(
            DynamoStateReader(dynamodb, contract),
            DynamoStateStore(dynamodb, contract),
            KmsSigner(kms, response_key_id, response_public_key),
            load_host_time_floor,
            RoughtimeClockAdapter(
                derive_clock_policy(manifest), derive_roughtime_config(manifest)
            ),
        )
    except Exception:
        failed = True
    if failed or type(runtime) is not SignedTimeHandler:
        raise RuntimeCompositionError(_ERROR) from None
    return runtime
