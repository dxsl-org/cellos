import copy

from validator_test_support import ROOT, VALIDATOR, ValidatorBehaviorTests, load


class InventoryValidatorBehaviorTests(ValidatorBehaviorTests):
    def test_inventory_omission_relabel_and_unclassified_match_fail(self):
        for mutate in (
            lambda d: d["entries"].pop(),
            lambda d: d["entries"][0].update(roles=["reader"]),
            lambda d: d["entries"][0].update(emitted_version_class="v3"),
            lambda d: d["unclassified_matches"].append("fake.rs"),
            lambda d: d["discovery_contract"]["required_manual_paths"].pop(),
        ):
            doc = copy.deepcopy(self.inventory)
            mutate(doc)
            self.rejected(inventory=doc)

    def test_future_version_and_promotional_claims_fail(self):
        for value in ("v3", 3, "Tier2 route", "owner consent embedded", "APPROVED"):
            doc = copy.deepcopy(self.inventory)
            doc["entries"][0]["notes"] = value
            self.rejected(inventory=doc)

    def test_matrix_omission_duplicate_wrong_outcome_and_different_elf_coverage_fail(self):
        mutations = []
        doc = copy.deepcopy(self.matrix); doc["rows"].pop(); mutations.append(doc)
        doc = copy.deepcopy(self.matrix); doc["rows"][1] = copy.deepcopy(doc["rows"][0]); mutations.append(doc)
        doc = copy.deepcopy(self.matrix); doc["rows"][0]["expected_result"] = "preserve-existing-v1v2-policy"; mutations.append(doc)
        doc = copy.deepcopy(self.matrix); doc["mandatory_hostile_tuples"].pop(); mutations.append(doc)
        doc = copy.deepcopy(self.matrix); doc["mandatory_hostile_tuples"][0]["scenario"] = "SAS fallback"; mutations.append(doc)
        doc = copy.deepcopy(self.matrix); doc["mandatory_hostile_tuples"][-1]["artifact_binding"] = "same-final-elf"; mutations.append(doc)
        for doc in mutations:
            self.rejected(matrix=doc)

    def test_parent_phase08_dependency_is_exact(self):
        parent = (ROOT / ".agents/260821-0642-app-tiers-completion/plan.md").read_text()
        VALIDATOR.validate_parent_plan_text(parent)
        with self.assertRaises(ValueError):
            VALIDATOR.validate_parent_plan_text(parent.replace("| 03,05,07 |", "| 05,07 |"))

    def test_fake_route_and_embedded_authority_fields_fail(self):
        for key, value in (("tier2_route", "enabled"), ("owner_consent", True), ("approval", "APPROVED")):
            doc = copy.deepcopy(self.matrix)
            doc["rows"][0][key] = value
            self.rejected(matrix=doc)

    def test_each_dependency_is_mandatory(self):
        VALIDATOR.validate_plan_text(self.plan)
        for dependency in ("phase_03:", "phase_05:", "phase_07:"):
            with self.assertRaises(ValueError):
                VALIDATOR.validate_plan_text(self.plan.replace(dependency, "removed_phase:"))

    def test_exact_blocked_terminal_is_mandatory(self):
        with self.assertRaises(ValueError):
            VALIDATOR.validate_plan_text(self.plan.replace('completion_state: "PREDESIGN_COMPLETE / PHASE08_BLOCKED"', 'completion_state: "PHASE08_READY"'))

    def test_each_artifact_is_closed_validated_against_its_own_schema(self):
        VALIDATOR.validate_artifact_schemas(self.corpus, self.inventory, self.matrix, ROOT)
        for name, document, field, replacement in (
            ("corpus", self.corpus, "schema_version", 2),
            ("inventory", self.inventory, "discovery_contract", {"required_match_count": 133}),
            ("matrix", self.matrix, "matrix_id", "substituted-matrix"),
        ):
            with self.subTest(name=name):
                altered = copy.deepcopy(document)
                altered[field] = replacement
                documents = {"corpus": copy.deepcopy(self.corpus), "inventory": copy.deepcopy(self.inventory), "matrix": copy.deepcopy(self.matrix)}
                documents[name] = altered
                with self.assertRaises(ValueError):
                    VALIDATOR.validate_artifact_schemas(documents["corpus"], documents["inventory"], documents["matrix"], ROOT)

    def test_stale_inventory_occurrence_v2_schema_constant_fails_before_scan(self):
        doc = copy.deepcopy(self.inventory)
        doc["discovery_contract"]["required_match_count"] = 133
        doc["discovery_contract"]["required_match_sha256"] = "e15cbdcaed1f537aea8a23c1425e0fe9de87b2b2f2def9160e4ababac8e0652d"
        self.rejected(inventory=doc)

    def test_immutable_base_revision_and_report_digest_bindings_reject_disagreement(self):
        matrix = copy.deepcopy(self.matrix)
        matrix["base_revision"] = "0" * 40
        self.rejected(matrix=matrix)
        report = load("predesign-validation-report.json")
        report["base_revision"] = "0" * 40
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(report, self.corpus, self.inventory, self.matrix, ROOT)
        report = load("predesign-validation-report.json")
        report["derived_source_state_digests"]["corpus"] = "0" * 64
        with self.assertRaises(ValueError):
            VALIDATOR.validate_report(report, self.corpus, self.inventory, self.matrix, ROOT)
