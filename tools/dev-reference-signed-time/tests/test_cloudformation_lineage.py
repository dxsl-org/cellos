from __future__ import annotations

import unittest

from path_bootstrap import ROOT  # noqa: F401
from cloudformation_support import TEMPLATE, actions, resource, statements


class LineageInfrastructureTests(unittest.TestCase):
    def test_transition_role_has_exact_separated_trust(self) -> None:
        properties = resource("LineageTransitionRole")["Properties"]
        self.assertEqual(
            properties["PermissionsBoundary"],
            {"Ref": "ExternalPermissionsBoundaryArn"},
        )
        self.assertEqual(
            properties["RoleName"],
            "cellos-dev-reference-signed-time-lineage-transition",
        )
        trust = statements(properties["AssumeRolePolicyDocument"])
        principal = {"Ref": "LineageTransitionTrustedPrincipalArn"}
        self.assertEqual(len(trust), 1)
        self.assertEqual(trust[0]["Principal"], {"AWS": principal})
        self.assertEqual(actions(trust[0]), {"sts:AssumeRole"})
        self.assertEqual(
            trust[0]["Condition"],
            {"ArnEquals": {"aws:PrincipalArn": principal}},
        )

    def test_all_external_principals_are_pairwise_separated(self) -> None:
        assertions = TEMPLATE["Rules"]["SeparatedExternalPrincipals"]["Assertions"]
        self.assertEqual(len(assertions), 15)
        pairs = [item["Assert"]["Fn::Not"][0]["Fn::Equals"] for item in assertions]
        for assertion in assertions:
            self.assertIn("Fn::Not", assertion["Assert"])
        for parameter in (
            "CloudFormationServiceRoleArn",
            "LineageTransitionTrustedPrincipalArn",
        ):
            self.assertEqual(sum({"Ref": parameter} in pair for pair in pairs), 5)

    def test_transition_policy_can_only_sign_lineage_and_cas_head(self) -> None:
        policy = resource("LineageTransitionPolicy")["Properties"]
        self.assertEqual(policy["Roles"], [{"Ref": "LineageTransitionRole"}])
        values = statements(policy["PolicyDocument"])
        sign = next(item for item in values if actions(item) == {"kms:Sign"})
        self.assertEqual(sign["Resource"], {"Fn::GetAtt": ["LineageKey", "Arn"]})
        self.assertEqual(
            sign["Condition"],
            {"StringEquals": {"kms:SigningAlgorithm": "ECDSA_SHA_256"}},
        )
        head = next(item for item in values if "dynamodb:UpdateItem" in actions(item))
        self.assertEqual(actions(head), {"dynamodb:UpdateItem"})
        self.assertEqual(head["Resource"], {"Fn::GetAtt": ["LineageTable", "Arn"]})
        self.assertEqual(
            head["Condition"]["ForAllValues:StringEquals"]["dynamodb:LeadingKeys"],
            ["lineage#cellos-dev-time-v1/head"],
        )
        self.assertNotIn(
            {"Fn::GetAtt": ["SigningKey", "Arn"]},
            [item.get("Resource") for item in values],
        )
        migration = next(
            item for item in values
            if item.get("Resource") == {"Fn::GetAtt": ["SignedTimeTable", "Arn"]}
        )
        self.assertEqual(actions(migration), {"dynamodb:UpdateItem"})
        self.assertEqual(
            migration["Condition"]["ForAllValues:StringEquals"][
                "dynamodb:LeadingKeys"
            ],
            ["source#cellos-dev-time-v1/state"],
        )

    def test_iam_reviewer_can_inspect_transition_role(self) -> None:
        policies = resource("IamReviewRole")["Properties"]["Policies"]
        values = [
            statement
            for policy in policies
            for statement in statements(policy["PolicyDocument"])
        ]
        inspect = next(
            item for item in values if item["Sid"] == "InspectOnlyDeclaredRoles"
        )
        self.assertIn(
            {
                "Fn::Sub": (
                    "arn:${AWS::Partition}:iam::${AWS::AccountId}:role/"
                    "cellos-dev-reference-signed-time-lineage-transition"
                )
            },
            inspect["Resource"],
        )

    def test_function_and_outputs_expose_exact_lineage_resources(self) -> None:
        variables = resource("SignedTimeFunction")["Properties"]["Environment"][
            "Variables"
        ]
        self.assertEqual(
            variables["SIGNED_TIME_LINEAGE_TABLE_NAME"], {"Ref": "LineageTable"},
        )
        self.assertEqual(
            variables["SIGNED_TIME_LINEAGE_KMS_KEY_ARN"],
            {"Fn::GetAtt": ["LineageKey", "Arn"]},
        )
        outputs = TEMPLATE["Outputs"]
        self.assertEqual(outputs["LineageTableName"]["Value"], {"Ref": "LineageTable"})
        self.assertEqual(
            outputs["LineageKeyArn"]["Value"],
            {"Fn::GetAtt": ["LineageKey", "Arn"]},
        )
        self.assertEqual(
            outputs["LineageTransitionRoleArn"]["Value"],
            {"Fn::GetAtt": ["LineageTransitionRole", "Arn"]},
        )


if __name__ == "__main__":
    unittest.main()
