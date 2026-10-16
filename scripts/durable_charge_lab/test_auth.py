"""Consumer-visible authentication/provisioning boundaries for the private lab."""
import copy
from pathlib import Path
import tempfile
import unittest

from auth import Keyring, canonical, strict_loads


class AuthenticationBoundaries(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'store'
        self.keys = Keyring.provision(self.root, 'test-scope')
        self.body = {'obligation': 'O1', 'transaction': 'T1', 'vector': [5, 8]}
        self.proof = self.keys.sign('destination', 'application-result', self.body)

    def test_reopen_preserves_pins_and_verifies_original_fact(self):
        reopened = Keyring.open(self.root, 'test-scope')
        self.assertEqual(reopened.public_roster(), self.keys.public_roster())
        self.assertEqual(reopened.verify(self.proof, 'destination', 'application-result'), self.body)

    def test_changed_fact_or_binding_never_verifies(self):
        changes = {'scope': 'another-scope', 'purpose': 'closure', 'role': 'authority',
                   'lane': 'production', 'version': 2, 'key_pin': '00' * 32,
                   'roster_pin': '00' * 32, 'body': {**self.body, 'vector': [7, 3]},
                   'signature': '00' * 64}
        for field, value in changes.items():
            with self.subTest(field=field):
                proof = copy.deepcopy(self.proof)
                proof[field] = value
                with self.assertRaises(ValueError):
                    self.keys.verify(proof, 'destination', 'application-result')

    def test_correct_signature_has_no_other_role_or_purpose_authority(self):
        for role, purpose in [('authority', 'application-result'), ('destination', 'closure')]:
            with self.subTest(role=role, purpose=purpose), self.assertRaises(ValueError):
                self.keys.verify(self.proof, role, purpose)

    def test_another_assignment_roster_cannot_validate_original_fact(self):
        other = Keyring.provision(Path(self.temp.name) / 'other', 'test-scope')
        with self.assertRaises(ValueError):
            other.verify(self.proof, 'destination', 'application-result')

    def test_returned_verified_body_cannot_mutate_signed_fact(self):
        body = self.keys.verify(self.proof, 'destination', 'application-result')
        body['vector'][0] = 99
        self.assertEqual(self.keys.verify(self.proof, 'destination', 'application-result'), self.body)

    def test_duplicate_provision_cannot_replace_pins(self):
        roster = self.keys.public_roster()
        with self.assertRaises(ValueError):
            Keyring.provision(self.root, 'test-scope')
        self.assertEqual(Keyring.open(self.root, 'test-scope').public_roster(), roster)

    def test_missing_key_is_not_regenerated(self):
        path = self.root / 'identities' / 'destination.key'
        path.unlink()
        with self.assertRaises(ValueError):
            Keyring.open(self.root, 'test-scope')
        self.assertFalse(path.exists())

    def test_swapped_private_key_does_not_change_pinned_assignment(self):
        other = Keyring.provision(Path(self.temp.name) / 'other', 'test-scope')
        del other
        target = self.root / 'identities' / 'destination.key'
        target.write_bytes((Path(self.temp.name) / 'other' / 'identities' / 'destination.key').read_bytes())
        with self.assertRaises(ValueError):
            Keyring.open(self.root, 'test-scope')

    def test_unsafe_or_symlink_key_is_refused(self):
        target = self.root / 'identities' / 'destination.key'
        target.chmod(0o644)
        with self.assertRaises(ValueError):
            Keyring.open(self.root, 'test-scope')
        target.chmod(0o600)
        replacement = Path(self.temp.name) / 'private-key'
        target.rename(replacement)
        target.symlink_to(replacement)
        with self.assertRaises(ValueError):
            Keyring.open(self.root, 'test-scope')

    def test_wrong_scope_cannot_open_existing_identity_set(self):
        with self.assertRaises(ValueError):
            Keyring.open(self.root, 'another-scope')


class CanonicalInputBoundaries(unittest.TestCase):
    def test_object_order_has_one_authenticated_encoding(self):
        self.assertEqual(canonical({'b': [1, 2], 'a': 3}), canonical({'a': 3, 'b': [1, 2]}))

    def test_duplicate_float_nonfinite_and_overflow_inputs_refuse(self):
        for raw in [b'{"key":1,"key":2}', b'{"x":1.0}', b'[NaN]', b'[Infinity]',
                    b'[18446744073709551616]', b'"\\ud800"', b'[' * 33 + b']' * 33]:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                strict_loads(raw)

    def test_non_json_values_and_cyclic_containers_refuse(self):
        cyclic = []
        cyclic.append(cyclic)
        for value in [1.0, {1: 'key'}, (1, 2), cyclic, 'x' * 65537]:
            with self.subTest(kind=type(value).__name__), self.assertRaises(ValueError):
                canonical(value)


if __name__ == '__main__':
    unittest.main()
