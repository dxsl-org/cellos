"""Immutable values and public constants for signed-time manifest schema 2."""

from dataclasses import dataclass

MAX_MANIFEST_BYTES = 4096
SCHEMA_VERSION = 2
CLASSIFICATION = "DEV_REFERENCE"
PRODUCTION_REJECTION_MARKERS = frozenset({
    "AWS_DEV_SIGNED_TIME",
    "DEV_REFERENCE",
    "SOFTWARE_HARNESS",
    "aws-dev-signed-time",
    "cellos-dev-time-v1",
})


@dataclass(frozen=True, slots=True)
class SignedTimeManifest:
    """Complete canonical configuration required before signed-time startup."""

    schema_version: int
    classification: str
    protocol_version: int
    source_id: str
    aws_region: str
    endpoint_url: str
    endpoint_spki_sha256: bytes
    source_epoch: int
    kms_key_id: str
    kms_public_key_der_sha256: bytes
    signing_algorithm: str
    allocator_table_name: str
    allocator_table_id: str
    lineage_table_name: str
    lineage_table_id: str
    lineage_kms_key_id: str
    lineage_public_key_der_sha256: bytes
    lineage_transition: bytes
    upstream_identity: str
    max_sample_age_seconds: int
    max_uncertainty_seconds: int
    upstream_protocol: str
    upstream_transport: str
    upstream_host: str
    upstream_port: int
    upstream_public_key: bytes
    upstream_version: int
    upstream_timeout_milliseconds: int
    upstream_request_message_bytes: int
    upstream_max_packet_bytes: int
