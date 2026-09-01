"""Side-effect-free runtime contracts derived from one validated manifest."""

import hashlib
import hmac

from clock_policy import ClockPolicy
from lineage import LineageContract, LineageError, admit_lineage_contract
from manifest import ManifestError, validate_manifest
from manifest_model import SignedTimeManifest
from roughtime_config import RoughtimeProviderConfig

_ERROR = "invalid signed-time manifest"


def derive_clock_policy(manifest: SignedTimeManifest) -> ClockPolicy:
    """Derive the clock boundary without ambient inputs or I/O."""
    validate_manifest(manifest)
    return ClockPolicy(
        manifest.upstream_identity,
        manifest.source_epoch,
        manifest.max_sample_age_seconds,
        manifest.max_uncertainty_seconds,
    )


def derive_kms_key_pins(manifest: SignedTimeManifest) -> tuple[str, bytes]:
    """Return the exact response KMS key ARN and DER-SPKI SHA-256 pin."""
    validate_manifest(manifest)
    return manifest.kms_key_id, manifest.kms_public_key_der_sha256


def derive_lineage_key_pins(manifest: SignedTimeManifest) -> tuple[str, bytes]:
    """Return the non-signing runtime pin for the dedicated lineage KMS key."""
    validate_manifest(manifest)
    return manifest.lineage_kms_key_id, manifest.lineage_public_key_der_sha256


def derive_roughtime_config(manifest: SignedTimeManifest) -> RoughtimeProviderConfig:
    """Derive the sole pinned UDP provider configuration without I/O."""
    validate_manifest(manifest)
    return RoughtimeProviderConfig(*(getattr(manifest, name) for name in (
        "upstream_protocol", "upstream_transport", "upstream_host",
        "upstream_port", "upstream_public_key", "upstream_version",
        "upstream_timeout_milliseconds", "upstream_request_message_bytes",
        "upstream_max_packet_bytes",
    )))


def derive_lineage_contract(
    manifest: SignedTimeManifest,
    lineage_public_key_der: bytes,
    previous: LineageContract | None = None,
) -> LineageContract:
    """Authenticate and bind the manifest's selected allocator lineage head."""
    validate_manifest(manifest)
    failed = False
    try:
        if (
            type(lineage_public_key_der) is not bytes
            or not hmac.compare_digest(
                hashlib.sha256(lineage_public_key_der).digest(),
                manifest.lineage_public_key_der_sha256,
            )
        ):
            raise ValueError("lineage public key mismatch")
        contract = admit_lineage_contract(
            manifest.lineage_table_name,
            manifest.lineage_table_id,
            manifest.lineage_transition,
            lineage_public_key_der,
            previous,
        )
        transition = contract.transition
        if (
            transition.source_epoch != manifest.source_epoch
            or transition.allocator_table_name != manifest.allocator_table_name
            or transition.allocator_table_id != manifest.allocator_table_id
            or transition.response_key_id != manifest.kms_key_id
            or not hmac.compare_digest(
                transition.response_public_key_der_sha256,
                manifest.kms_public_key_der_sha256,
            )
        ):
            raise ValueError("lineage transition does not bind manifest")
    except (LineageError, TypeError, ValueError):
        failed = True
        contract = None
    if failed or type(contract) is not LineageContract:
        raise ManifestError(_ERROR) from None
    return contract
