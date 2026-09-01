"""Exact inline mutation descriptors for provisioning approval."""

import hashlib

MUTATION_ORDER = (
    "tpm-create-stable-identity",
    "tpm-create-active-relay-key",
    "tpm-create-pending-relay-key",
    "tpm-define-non-orderly-counter",
    "stirot-provision-image-policy",
    "stm32-apply-option-bytes",
    "stm32-program-otp",
    "stm32-close-lifecycle",
    "stm32-close-debug",
)
MUTATION_FIELDS = {
    "name",
    "target_kind",
    "target_identifier",
    "address_space",
    "address",
    "width_bits",
    "write_mask_hex",
    "requested_value_hex",
    "expected_readback_hex",
    "authorization_policy_sha256",
    "irreversible",
    "recovery_consequence",
}
TARGET_KINDS = (
    "tpm-persistent-handle",
    "tpm-persistent-handle",
    "tpm-persistent-handle",
    "tpm-nv-index",
    "stm32-stirot-policy",
    "stm32-option-bytes",
    "stm32-otp-region",
    "stm32-lifecycle-state",
    "stm32-debug-policy",
)
TARGET_IDENTIFIERS = (
    "stable_identity_handle",
    "active_relay_handle",
    "pending_relay_handle",
    "nv_counter_index",
    "stirot-image-policy",
    "option-byte-bank",
    "otp-provisioning-region",
    "lifecycle-state",
    "debug-authentication-policy",
)
TPM_TEMPLATE_FIELDS = (
    "stable_identity_template_sha256",
    "active_relay_template_sha256",
    "pending_relay_template_sha256",
    "nv_public_template_sha256",
)


def _exact_fields(value: object, expected: set[str], path: str, error_type) -> dict:
    if not isinstance(value, dict):
        raise error_type(f"{path} must be an object")
    actual = set(value)
    if actual != expected:
        raise error_type(
            f"{path} fields differ: missing={sorted(expected - actual)} "
            f"unexpected={sorted(actual - expected)}"
        )
    return value


def _exact_hex(value: object, digits: int, path: str, error_type) -> None:
    if (
        not isinstance(value, str)
        or len(value) != digits
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise error_type(f"{path} must be exact lowercase hex with {digits} digits")


def validate_mutations(
    mutations: object,
    tpm: dict,
    fixture: bool,
    require_sha256,
    closed_text,
    error_type,
) -> list[dict]:
    names = (
        tuple(item.get("name") if isinstance(item, dict) else None for item in mutations)
        if isinstance(mutations, list)
        else ()
    )
    if names != MUTATION_ORDER:
        raise error_type("mutations must match the frozen order exactly")
    for index, mutation in enumerate(mutations):
        path = f"$.mutations[{index}]"
        row = _exact_fields(mutation, MUTATION_FIELDS, path, error_type)
        if row["target_kind"] != TARGET_KINDS[index]:
            raise error_type(f"{path}.target_kind differs")
        expected_space = "software-harness" if fixture else (
            "tpm-resource" if index < 4 else "stm32-mmio"
        )
        if row["address_space"] != expected_space:
            raise error_type(f"{path}.address_space differs")
        address = row["address"]
        if not isinstance(address, str) or len(address) != 10 or not address.startswith("0x"):
            raise error_type(f"{path}.address must be canonical 32-bit hex")
        _exact_hex(address[2:], 8, f"{path}.address", error_type)
        reference = TARGET_IDENTIFIERS[index]
        expected_identifier = (
            tpm[reference] if index < 4 else f"{reference}@{address}"
        )
        if row["target_identifier"] != expected_identifier:
            raise error_type(f"{path}.target_identifier differs")
        if index < 4 and address != row["target_identifier"]:
            raise error_type(f"{path}.address must equal the TPM target")
        width = row["width_bits"]
        if not isinstance(width, int) or isinstance(width, bool) or width < 8 or width > 32768 or width % 8:
            raise error_type(f"{path}.width_bits must be a byte-aligned 8..32768 value")
        digits = width // 4
        for field in ("write_mask_hex", "requested_value_hex", "expected_readback_hex"):
            _exact_hex(row[field], digits, f"{path}.{field}", error_type)
        mask = int(row["write_mask_hex"], 16)
        requested = int(row["requested_value_hex"], 16)
        readback = int(row["expected_readback_hex"], 16)
        if mask == 0:
            raise error_type(f"{path}.write_mask_hex must select at least one bit")
        outside_mask = ((1 << width) - 1) ^ mask
        if requested & outside_mask:
            raise error_type(f"{path}.requested_value_hex contains bits outside write_mask_hex")
        if index >= 4 and requested & mask != readback & mask:
            raise error_type(f"{path}.expected_readback_hex conflicts with masked write")
        require_sha256(row["authorization_policy_sha256"], f"{path}.authorization_policy_sha256")
        request_digest = hashlib.sha256(
            bytes.fromhex(row["requested_value_hex"])
        ).hexdigest()
        if index < 4 and request_digest != tpm[TPM_TEMPLATE_FIELDS[index]]:
            raise error_type(f"{path}.requested_value_hex conflicts with TPM template")
        if row["authorization_policy_sha256"] != tpm["authorization_policy_sha256"]:
            raise error_type(f"{path}.authorization_policy_sha256 differs from TPM policy")
        closed_text(row["recovery_consequence"], f"{path}.recovery_consequence")
        if row["irreversible"] is not True:
            raise error_type(f"{path}.irreversible must be true")
    return mutations
