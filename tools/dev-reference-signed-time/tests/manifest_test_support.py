from dataclasses import replace

import path_bootstrap
from manifest import SignedTimeManifest

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
        upstream_identity="authenticated-clock.example.com",
        max_sample_age_seconds=5,
        max_uncertainty_seconds=2,
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
    b'"upstream_identity":"authenticated-clock.example.com"}'
)
