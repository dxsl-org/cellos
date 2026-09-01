"""Bounded, side-effect-free validation for signed-time manifest strings."""

import re
from typing import Any
from urllib.parse import urlsplit

MAX_AWS_REGION_CHARS = 32
MAX_ENDPOINT_URL_CHARS = 270
MAX_KMS_KEY_ID_CHARS = 105
# With every other field at its valid maximum, 263 astral scalars encode to
# 4085 bytes under ensure_ascii; one more would cross the 4096-byte limit.
MAX_UPSTREAM_IDENTITY_CHARS = 263

_DNS_HOST = re.compile(
    r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?"
    r"(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*"
).fullmatch
_COMMERCIAL_REGION = re.compile(
    r"(?!cn-)[a-z]{2}-[a-z]+-[1-9][0-9]*"
).fullmatch
_GOVCLOUD_REGION = re.compile(r"us-gov-(?:east|west)-1").fullmatch
_CHINA_REGION = re.compile(r"cn-(?:north|northwest)-1").fullmatch
_KMS_ARN = re.compile(
    r"arn:(aws|aws-us-gov|aws-cn):kms:([^:]+):([0-9]{12}):"
    r"key/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|"
    r"mrk-[0-9a-f]{32})"
).fullmatch
_REGION_MATCHERS = {
    "aws": _COMMERCIAL_REGION,
    "aws-us-gov": _GOVCLOUD_REGION,
    "aws-cn": _CHINA_REGION,
}


def bounded_strings_are_valid(
    aws_region: Any,
    endpoint_url: Any,
    kms_key_id: Any,
    upstream_identity: Any,
) -> bool:
    return (
        type(aws_region) is str
        and 0 < len(aws_region) <= MAX_AWS_REGION_CHARS
        and type(endpoint_url) is str
        and 0 < len(endpoint_url) <= MAX_ENDPOINT_URL_CHARS
        and type(kms_key_id) is str
        and 0 < len(kms_key_id) <= MAX_KMS_KEY_ID_CHARS
        and type(upstream_identity) is str
        and 0 < len(upstream_identity) <= MAX_UPSTREAM_IDENTITY_CHARS
    )


def endpoint_is_valid(value: str) -> bool:
    try:
        parsed = urlsplit(value)
        host = parsed.hostname
    except (TypeError, ValueError):
        return False
    return (
        parsed.scheme == "https"
        and host is not None
        and len(host) <= 253
        and host.isascii()
        and _DNS_HOST(host) is not None
        and parsed.netloc == host
        and parsed.path == "/v1/time"
        and not parsed.query
        and not parsed.fragment
        and value == f"https://{host}/v1/time"
    )


def kms_arn_is_valid(value: str, region: str) -> bool:
    match = _KMS_ARN(value)
    if match is None:
        return False
    partition, arn_region, _account, _resource = match.groups()
    if arn_region != region:
        return False
    region_matcher = _REGION_MATCHERS[partition]
    return region_matcher(region) is not None
