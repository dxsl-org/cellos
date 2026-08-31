from dataclasses import replace

import path_bootstrap
from manifest import SignedTimeManifest
from roughtime_config import (
    MAX_PACKET_BYTES, PROVIDER_HOST, PROVIDER_PORT, PROVIDER_PROTOCOL,
    PROVIDER_PUBLIC_KEY, PROVIDER_TIMEOUT_MILLISECONDS, PROVIDER_TRANSPORT,
    PROVIDER_VERSION, REQUEST_MESSAGE_BYTES,
)

ENDPOINT_DIGEST = bytes.fromhex("11" * 32)
KMS_DIGEST = bytes.fromhex("22" * 32)
KMS_UUID = "00000000-0000-4000-8000-000000000000"
KMS_MRK = "mrk-" + "0" * 32


def kms_arn(
    *,
    partition="aws",
    region="us-east-1",
    account="000000000000",
    resource=f"key/{KMS_UUID}",
):
    return f"arn:{partition}:kms:{region}:{account}:{resource}"


KMS_ARN = kms_arn()


def valid_manifest(**changes):
    value = SignedTimeManifest(
        schema_version=1,
        classification="DEV_REFERENCE",
        protocol_version=1,
        source_id="cellos-dev-time-v1",
        aws_region="us-east-1",
        endpoint_url="https://time.example.com/v1/time",
        endpoint_spki_sha256=ENDPOINT_DIGEST,
        source_epoch=7,
        kms_key_id=KMS_ARN,
        kms_public_key_der_sha256=KMS_DIGEST,
        signing_algorithm="ECDSA_SHA_256",
        upstream_identity=PROVIDER_HOST,
        max_sample_age_seconds=5,
        max_uncertainty_seconds=2,
        upstream_protocol=PROVIDER_PROTOCOL,
        upstream_transport=PROVIDER_TRANSPORT,
        upstream_host=PROVIDER_HOST,
        upstream_port=PROVIDER_PORT,
        upstream_public_key=PROVIDER_PUBLIC_KEY,
        upstream_version=PROVIDER_VERSION,
        upstream_timeout_milliseconds=PROVIDER_TIMEOUT_MILLISECONDS,
        upstream_request_message_bytes=REQUEST_MESSAGE_BYTES,
        upstream_max_packet_bytes=MAX_PACKET_BYTES,
    )
    return replace(value, **changes)


GOLDEN = (
    b'{"aws_region":"us-east-1","classification":"DEV_REFERENCE",'
    b'"endpoint_spki_sha256":"' + b"11" * 32
    + b'","endpoint_url":"https://time.example.com/v1/time",'
    b'"kms_key_id":"' + KMS_ARN.encode("ascii")
    + b'","kms_public_key_der_sha256":"' + b"22" * 32
    + b'","max_sample_age_seconds":5,"max_uncertainty_seconds":2,'
    b'"protocol_version":1,"schema_version":1,'
    b'"signing_algorithm":"ECDSA_SHA_256","source_epoch":7,'
    b'"source_id":"cellos-dev-time-v1",'
    b'"upstream_host":"roughtime.cloudflare.com",'
    b'"upstream_identity":"roughtime.cloudflare.com",'
    b'"upstream_max_packet_bytes":1024,"upstream_port":2003,'
    b'"upstream_protocol":"roughtime-draft-11",'
    b'"upstream_public_key":"0GD7c3yP8xEc4Zl2zeuN2SlLvDVVocjsPSL8/Rl/7zg=",'
    b'"upstream_request_message_bytes":1012,'
    b'"upstream_timeout_milliseconds":2000,"upstream_transport":"udp",'
    b'"upstream_version":2147483659}'
)
