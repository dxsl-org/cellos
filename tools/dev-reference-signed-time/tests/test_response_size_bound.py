import unittest
from dataclasses import replace
import path_bootstrap

import cbor_codec
from cryptography.hazmat.primitives.asymmetric import utils
from protocol import encode_response, response_signing_bytes, response_signing_digest, ProtocolError
from protocol_crypto import P256_ORDER
from protocol_models import MAX_RESPONSE_BYTES, MAX_UNSIGNED_RESPONSE_BYTES, SignedResponse
from receipt import Receipt
from response_size_support import response_at_unsigned_limit
from state_codec import StateCodecError, decode_receipt, encode_receipt
from state_codec_support import response


class ResponseSizeBoundTests(unittest.TestCase):
    def setUp(self):
        self.maximum = response_at_unsigned_limit(response())
        self.oversized = replace(self.maximum, key_id=self.maximum.key_id + "k")

    def oversized_wire(self):
        values = cbor_codec.loads(response_signing_bytes(self.maximum))
        values[13] = self.oversized.key_id
        return cbor_codec.dumps(values)

    def test_exact_950_accepts_worst_case_71_byte_low_s_signature(self):
        signing_bytes = response_signing_bytes(self.maximum)
        self.assertEqual(MAX_UNSIGNED_RESPONSE_BYTES, 950)
        self.assertEqual(len(signing_bytes), MAX_UNSIGNED_RESPONSE_BYTES)

        signature = utils.encode_dss_signature(1 << 255, P256_ORDER // 2)
        self.assertEqual(len(signature), 71)
        signed = SignedResponse(
            *[getattr(self.maximum, field) for field in self.maximum.__dataclass_fields__],
            signature=signature,
        )
        encoded = encode_response(signed)
        self.assertEqual(MAX_RESPONSE_BYTES, 1024)
        self.assertEqual(len(encoded), MAX_RESPONSE_BYTES)

    def test_exact_951_is_rejected_at_signing_bytes_and_digest_boundaries(self):
        self.assertEqual(len(self.oversized_wire()), MAX_UNSIGNED_RESPONSE_BYTES + 1)
        for operation in (response_signing_bytes, response_signing_digest):
            with self.subTest(operation=operation.__name__):
                with self.assertRaises(ProtocolError):
                    operation(self.oversized)

    def test_state_receipt_encode_and_decode_enforce_exact_unsigned_limit(self):
        receipt = Receipt(b"h" * 32, self.maximum)
        item = encode_receipt(receipt)
        self.assertEqual(len(item["response_signing_bytes"]["B"]), MAX_UNSIGNED_RESPONSE_BYTES)
        self.assertEqual(decode_receipt(item), receipt)

        with self.assertRaises(StateCodecError):
            encode_receipt(Receipt(b"h" * 32, self.oversized))
        item["response_signing_bytes"] = {"B": self.oversized_wire()}
        with self.assertRaises(StateCodecError):
            decode_receipt(item)


if __name__ == "__main__":
    unittest.main()
