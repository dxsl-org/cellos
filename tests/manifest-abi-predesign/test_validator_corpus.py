import copy
from pathlib import Path
from unittest import mock

from validator_test_support import ARTIFACTS, ROOT, VALIDATOR, ValidatorBehaviorTests, load


class CorpusValidatorBehaviorTests(ValidatorBehaviorTests):
    def test_canonical_artifacts_pass_without_readiness_claim(self):
        self.validate(scan=True)
        report = load("predesign-validation-report.json")
        self.assertEqual(report["terminal_state"], "PREDESIGN_COMPLETE / PHASE08_BLOCKED")
        self.assertFalse(report["phase08_readiness"])
        self.assertEqual(report["approval_claims"], [])

    def test_fixture_omission_and_hash_drift_fail(self):
        for mutate in (
            lambda d: d["fixtures"].pop(0),
            lambda d: d["fixtures"][0].update(bytes_hex="00"),
            lambda d: d["fixtures"][0].update(size_bytes=999),
            lambda d: d["fixtures"][0].update(sha256="0" * 64),
            lambda d: d.update(corpus_sha256="0" * 64),
        ):
            with self.subTest(mutate=mutate):
                doc = copy.deepcopy(self.corpus)
                mutate(doc)
                self.rejected(corpus=doc)

    def test_fixture_relabel_to_distinct_invalid_tri_state_fails(self):
        doc = copy.deepcopy(self.corpus)
        self.assertEqual(doc["fixtures"][0]["expected_tri_state"], "ValidV2")
        doc["fixtures"][0]["expected_tri_state"] = "Malformed"
        self.assertNotEqual(doc["fixtures"][0]["expected_tri_state"], "ValidV2")
        self.rejected(corpus=doc)

    def test_missing_absent_malformed_and_mutation_families_fail(self):
        for prefix in ("elf64-manifest-absent", "elf64-duplicate-manifest", "record-v2-one-bit-"):
            doc = copy.deepcopy(self.corpus)
            doc["fixtures"] = [x for x in doc["fixtures"] if not x["id"].startswith(prefix)]
            self.rejected(corpus=doc)

    def test_unknown_keys_duplicates_malformed_hex_and_unsorted_fail(self):
        mutations = []
        doc = copy.deepcopy(self.corpus); doc["future_layout"] = 24; mutations.append(doc)
        doc = copy.deepcopy(self.corpus); doc["fixtures"][1]["id"] = doc["fixtures"][0]["id"]; mutations.append(doc)
        doc = copy.deepcopy(self.corpus); doc["fixtures"][0]["bytes_hex"] = "xyz"; mutations.append(doc)
        doc = copy.deepcopy(self.corpus); doc["fixtures"][0], doc["fixtures"][1] = doc["fixtures"][1], doc["fixtures"][0]; mutations.append(doc)
        for doc in mutations:
            self.rejected(corpus=doc)

    def test_schema_numeric_bounds_are_type_safe(self):
        VALIDATOR.validate_schema(0.5, {"type": "number", "minimum": 0, "maximum": 1})
        for value in (float("nan"), float("inf"), float("-inf")):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "non-finite"):
                VALIDATOR.validate_schema(value, {"type": "number", "minimum": 0})
        with self.assertRaisesRegex(ValueError, "expected integer"):
            VALIDATOR.validate_schema(True, {"type": "integer", "minimum": 0})

    def test_schema_minimum_and_maximum_mutations_reject_before_bespoke_checks(self):
        mutations = []
        corpus = copy.deepcopy(self.corpus)
        corpus["fixtures"][0]["mutation_index"] = -1
        corpus["corpus_sha256"] = VALIDATOR.digest(corpus["fixtures"])
        mutations.append(("corpus minimum", corpus, self.inventory, self.matrix))
        corpus = copy.deepcopy(self.corpus)
        fixture = next(item for item in corpus["fixtures"] if item["expected_canonical"] is not None)
        fixture["expected_canonical"]["flags"] = 4096
        mutations.append(("corpus maximum", corpus, self.inventory, self.matrix))
        inventory = copy.deepcopy(self.inventory)
        occurrence = next(item for entry in inventory["entries"] for item in entry["classified_occurrences"])
        occurrence["line"] = 0
        mutations.append(("inventory minimum", self.corpus, inventory, self.matrix))
        matrix = copy.deepcopy(self.matrix)
        matrix["mandatory_hostile_tuples"].pop()
        mutations.append(("matrix minimum", self.corpus, self.inventory, matrix))
        matrix = copy.deepcopy(self.matrix)
        matrix["mandatory_hostile_tuples"].append(copy.deepcopy(matrix["mandatory_hostile_tuples"][0]))
        mutations.append(("matrix maximum", self.corpus, self.inventory, matrix))
        for name, corpus, inventory, matrix in mutations:
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, r"^schema "):
                VALIDATOR.validate_artifact_schemas(corpus, inventory, matrix, ROOT)

    def test_derived_source_state_and_occurrence_pins_fail_on_drift(self):
        corpus = copy.deepcopy(self.corpus)
        corpus["derived_source_state"]["derived_source_state_sha256"] = "0" * 64
        self.rejected(corpus=corpus)
        inventory = copy.deepcopy(self.inventory)
        inventory["derived_source_state"]["inputs"][0]["sha256"] = "0" * 64
        self.rejected(inventory=inventory)
        inventory = copy.deepcopy(self.inventory)
        state = inventory["derived_source_state"]
        state["inputs"].append({"path": ".agents/260822-phase08-manifest-predesign/artifacts/manifest-consumer-inventory.json", "sha256": VALIDATOR.file_digest(ARTIFACTS / "manifest-consumer-inventory.json")})
        state["inputs"].sort(key=lambda item: item["path"])
        state["derived_source_state_sha256"] = VALIDATOR.digest({"hash_algorithm": state["hash_algorithm"], "inputs": state["inputs"]})
        self.rejected(inventory=inventory)
        inventory = copy.deepcopy(self.inventory)
        inventory["entries"][0]["source_sha256"] = "0" * 64
        self.rejected(inventory=inventory)
        inventory = copy.deepcopy(self.inventory)
        inventory["entries"][0]["classified_occurrences"][0]["classification"]["roles"] = ["reader"]
        self.rejected(inventory=inventory)

    def test_current_source_content_modification_fails_derived_state_pin(self):
        pinned_path = self.corpus["derived_source_state"]["inputs"][0]["path"]
        original_file_digest = VALIDATOR.file_digest

        def modified_source_digest(path):
            if Path(path) == ROOT / pinned_path:
                return "0" * 64
            return original_file_digest(path)

        with mock.patch("manifest_abi_predesign.state.file_digest", side_effect=modified_source_digest):
            with self.assertRaisesRegex(ValueError, r"^corpus source input content drift$"):
                self.validate()

    def test_source_scan_drift_requires_occurrence_repin_and_reclassification(self):
        drift = copy.deepcopy(VALIDATOR.scan_sources(ROOT))
        drift[0]["token"] = "MANIFEST_MAGIC"
        doc = copy.deepcopy(self.inventory)
        doc["discovery_contract"]["required_match_sha256"] = VALIDATOR.digest(drift)
        doc["discovery_contract"]["source_scan_repin"]["repinned_match_sha256"] = VALIDATOR.digest(drift)
        with mock.patch.object(VALIDATOR, "scan_sources", return_value=drift):
            self.rejected(inventory=doc, scan=True)

    def test_authoritative_source_and_recomputed_corpus_substitution_fail(self):
        doc = copy.deepcopy(self.corpus)
        state = doc["derived_source_state"]
        state["inputs"][0] = {"path": "libs/api/src/abi.rs", "sha256": VALIDATOR.file_digest(ROOT / "libs/api/src/abi.rs")}
        state["inputs"].sort(key=lambda item: item["path"])
        state["derived_source_state_sha256"] = VALIDATOR.digest({"hash_algorithm": state["hash_algorithm"], "inputs": state["inputs"]})
        self.rejected(corpus=doc)
        doc = copy.deepcopy(self.corpus)
        fixture = next(item for item in doc["fixtures"] if item["id"] == "record-v1-zero")
        fixture["expected_tri_state"] = "Malformed"
        fixture["expected_canonical"] = None
        fixture["expected_policy_effect"] = "deny-before-task-publication"
        doc["corpus_sha256"] = VALIDATOR.digest(doc["fixtures"])
        self.rejected(corpus=doc)
