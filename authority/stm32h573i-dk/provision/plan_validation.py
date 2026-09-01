"""Closed validation for STM32/TPM provisioning-plan inputs."""
from mutation_artifact import MUTATION_ORDER, validate_mutations
SCHEMA = "cellos-stm32-provision-input-v1"
DIGEST_FIELDS = (
    "stirot_image_sha256",
    "stirot_policy_sha256",
    "approved_sram_loader_sha256",
    "manifest_verification_key_sha256",
)
TPM_FIELDS = (
    "manufacturer",
    "opn",
    "stable_identity_handle",
    "active_relay_handle",
    "pending_relay_handle",
    "nv_counter_index",
    "nv_attributes",
    "stable_identity_template_sha256",
    "active_relay_template_sha256",
    "pending_relay_template_sha256",
    "nv_public_template_sha256",
    "authorization_policy_sha256",
)
NV_ATTRIBUTES = (
    "TPMA_NV_AUTHREAD",
    "TPMA_NV_AUTHWRITE",
    "TPMA_NV_COUNTER",
    "TPMA_NV_NO_DA",
)
FORBIDDEN_PLACEHOLDERS = ("todo", "tbd", "unknown", "placeholder", "fill-me")

class ProvisioningPlanError(ValueError):
    """The input cannot produce an approval-grade provisioning plan."""

def _exact_fields(value: object, expected: set[str], path: str) -> dict:
    if not isinstance(value, dict):
        raise ProvisioningPlanError(f"{path} must be an object")
    actual = set(value)
    if actual != expected:
        raise ProvisioningPlanError(
            f"{path} fields differ: missing={sorted(expected - actual)} "
            f"unexpected={sorted(actual - expected)}"
        )
    return value

def _closed_text(value: object, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ProvisioningPlanError(f"{path} must be non-empty text")
    lowered = value.casefold()
    if any(marker in lowered for marker in FORBIDDEN_PLACEHOLDERS):
        raise ProvisioningPlanError(f"{path} contains a placeholder")
    return value

def _require_sha256(value: object, path: str) -> None:
    if not isinstance(value, str) or len(value) != 64 or any(
        character not in "0123456789abcdef" for character in value
    ):
        raise ProvisioningPlanError(f"{path} must be lowercase sha256")

def _is_synthetic_digest(value: str) -> bool:
    if len(set(value)) < 8:
        return True
    return any(
        value == value[:period] * (len(value) // period)
        for period in (1, 2, 4, 8, 16, 32)
    )

def validate_configuration(configuration: object) -> dict:
    root = _exact_fields(
        configuration,
        {
            "schema", "classification", "software_harness_fixture",
            "plan_revision", "preclosure_verification_sha256",
            "digests", "tpm", "mutations",
        },
        "$",
    )
    if root["schema"] != SCHEMA or root["classification"] != "DEV_REFERENCE":
        raise ProvisioningPlanError("schema and classification must be exact")
    revision = root["plan_revision"]
    if not isinstance(revision, int) or isinstance(revision, bool) or revision < 1:
        raise ProvisioningPlanError("plan_revision must be a positive integer")
    fixture = root["software_harness_fixture"]
    if not isinstance(fixture, bool):
        raise ProvisioningPlanError("software_harness_fixture must be boolean")
    _require_sha256(
        root["preclosure_verification_sha256"],
        "$.preclosure_verification_sha256",
    )
    if not fixture and _is_synthetic_digest(root["preclosure_verification_sha256"]):
        raise ProvisioningPlanError(
            "$.preclosure_verification_sha256 is a synthetic sentinel"
        )
    digests = _exact_fields(root["digests"], set(DIGEST_FIELDS), "$.digests")
    for name in DIGEST_FIELDS:
        _require_sha256(digests[name], f"$.digests.{name}")
        if not fixture and _is_synthetic_digest(digests[name]):
            raise ProvisioningPlanError(f"$.digests.{name} is a synthetic sentinel")
    tpm = _exact_fields(root["tpm"], set(TPM_FIELDS), "$.tpm")
    if tpm["manufacturer"] != "IFX" or tpm["opn"] != "TPM9672FW1523PCEBTOBO1":
        raise ProvisioningPlanError("TPM manufacturer/OPN does not match the admitted lane")
    handles = [tpm[name] for name in TPM_FIELDS[2:6]]
    if any(
        not isinstance(value, str)
        or len(value) != 10
        or not value.startswith("0x")
        or any(character not in "0123456789abcdef" for character in value[2:])
        for value in handles
    ):
        raise ProvisioningPlanError("TPM handles must be canonical 32-bit lowercase hex")
    if len(set(handles)) != len(handles):
        raise ProvisioningPlanError("TPM handles must be distinct")
    if any(not 0x81000000 <= int(value, 16) <= 0x81FFFFFF for value in handles[:3]):
        raise ProvisioningPlanError("TPM persistent handles are outside the persistent range")
    if not 0x01000000 <= int(handles[3], 16) <= 0x01FFFFFF:
        raise ProvisioningPlanError("TPM NV index is outside the NV range")
    attributes = tpm["nv_attributes"]
    if not isinstance(attributes, list) or tuple(attributes) != NV_ATTRIBUTES:
        raise ProvisioningPlanError("nv_attributes must match the frozen exact set")
    for index, value in enumerate(attributes):
        _closed_text(value, f"$.tpm.nv_attributes[{index}]")
    for name in TPM_FIELDS[7:]:
        _require_sha256(tpm[name], f"$.tpm.{name}")
        if not fixture and _is_synthetic_digest(tpm[name]):
            raise ProvisioningPlanError(f"$.tpm.{name} is a synthetic sentinel")
    root["mutations"] = validate_mutations(
        root["mutations"],
        tpm,
        fixture,
        _require_sha256,
        _closed_text,
        ProvisioningPlanError,
    )
    return root
