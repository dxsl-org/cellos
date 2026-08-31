from dataclasses import replace
from base64 import b64encode

import path_bootstrap
from manifest_model import SignedTimeManifest
from lineage_test_support import (
    ALLOCATOR_TABLE, ALLOCATOR_TABLE_ID, LINEAGE_KEY_ID, LINEAGE_PUBLIC_DIGEST,
    LINEAGE_TABLE, LINEAGE_TABLE_ID, RESPONSE_KEY_DIGEST, RESPONSE_KEY_ID,
)
from roughtime_config import (
    MAX_PACKET_BYTES, PROVIDER_HOST, PROVIDER_PORT, PROVIDER_PROTOCOL,
    PROVIDER_PUBLIC_KEY, PROVIDER_TIMEOUT_MILLISECONDS, PROVIDER_TRANSPORT,
    PROVIDER_VERSION, REQUEST_MESSAGE_BYTES,
)

ENDPOINT_DIGEST = bytes.fromhex("11" * 32)
KMS_DIGEST = RESPONSE_KEY_DIGEST
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


KMS_ARN = RESPONSE_KEY_ID
GENESIS_TRANSITION = bytes.fromhex(
    "aa0101027263656c6c6f732d6465762d74696d652d763103010458200000000000000000"
    "00000000000000000000000000000000000000000000000005782063656c6c6f732d64"
    "65762d7369676e65642d74696d652d616c6c6f6361746f7206782431313131313131312d"
    "323232322d343333332d383434342d35353535353535353535353507784b61726e3a6177"
    "733a6b6d733a75732d656173742d313a3030303030303030303030303a6b65792f303030"
    "30303030302d303030302d343030302d383030302d303030303030303030303030085820"
    "2222222222222222222222222222222222222222222222222222222222222222096a696e"
    "697469616c697a650a5847304502210090384db87425ee65f06d94cbc579007d1b32905b"
    "65c22a7fdb680667c2db93e7022064fe8e77cb7d27762093fc71ba0f085edda708e6c0be"
    "e70142578bbd1f673616"
)


def valid_manifest(**changes):
    value = SignedTimeManifest(
        schema_version=2,
        classification="DEV_REFERENCE",
        protocol_version=1,
        source_id="cellos-dev-time-v1",
        aws_region="us-east-1",
        endpoint_url="https://time.example.com/v1/time",
        endpoint_spki_sha256=ENDPOINT_DIGEST,
        source_epoch=1,
        kms_key_id=KMS_ARN,
        kms_public_key_der_sha256=KMS_DIGEST,
        signing_algorithm="ECDSA_SHA_256",
        allocator_table_name=ALLOCATOR_TABLE,
        allocator_table_id=ALLOCATOR_TABLE_ID,
        lineage_table_name=LINEAGE_TABLE,
        lineage_table_id=LINEAGE_TABLE_ID,
        lineage_kms_key_id=LINEAGE_KEY_ID,
        lineage_public_key_der_sha256=LINEAGE_PUBLIC_DIGEST,
        lineage_transition=GENESIS_TRANSITION,
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
    b'{"allocator_table_id":"' + ALLOCATOR_TABLE_ID.encode("ascii")
    + b'","allocator_table_name":"' + ALLOCATOR_TABLE.encode("ascii")
    + b'","aws_region":"us-east-1","classification":"DEV_REFERENCE",'
    b'"endpoint_spki_sha256":"' + b"11" * 32
    + b'","endpoint_url":"https://time.example.com/v1/time",'
    b'"kms_key_id":"' + KMS_ARN.encode("ascii")
    + b'","kms_public_key_der_sha256":"' + b"22" * 32
    + b'","lineage_kms_key_id":"' + LINEAGE_KEY_ID.encode("ascii")
    + b'","lineage_public_key_der_sha256":"'
    + LINEAGE_PUBLIC_DIGEST.hex().encode("ascii")
    + b'","lineage_table_id":"' + LINEAGE_TABLE_ID.encode("ascii")
    + b'","lineage_table_name":"' + LINEAGE_TABLE.encode("ascii")
    + b'","lineage_transition":"' + b64encode(GENESIS_TRANSITION)
    + b'","max_sample_age_seconds":5,"max_uncertainty_seconds":2,'
    b'"protocol_version":1,"schema_version":2,'
    b'"signing_algorithm":"ECDSA_SHA_256","source_epoch":1,'
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
