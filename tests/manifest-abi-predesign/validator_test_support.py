import copy
import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/validate-manifest-abi-predesign.py"
ARTIFACTS = ROOT / ".agents/260822-phase08-manifest-predesign/artifacts"
SPEC = importlib.util.spec_from_file_location("manifest_predesign_validator", SCRIPT)
VALIDATOR = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(VALIDATOR)


def load(name):
    return json.loads((ARTIFACTS / name).read_text(encoding="utf-8"))


class ValidatorBehaviorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.corpus = load("manifest-v1-v2-corpus.json")
        cls.inventory = load("manifest-consumer-inventory.json")
        cls.matrix = load("manifest-downgrade-matrix.json")
        cls.plan = (ROOT / ".agents/260822-phase08-manifest-predesign/plan.md").read_text()

    def validate(self, corpus=None, inventory=None, matrix=None, scan=False):
        VALIDATOR.validate_loaded(corpus or copy.deepcopy(self.corpus), inventory or copy.deepcopy(self.inventory), matrix or copy.deepcopy(self.matrix), scan=scan)

    def rejected(self, corpus=None, inventory=None, matrix=None, scan=False):
        with self.assertRaises((ValueError, KeyError, TypeError)):
            self.validate(corpus, inventory, matrix, scan)
