from __future__ import annotations

import json
import unittest

from path_bootstrap import ROOT  # noqa: F401
from cloudformation_support import actions, resource, statements


class KmsAndRuntimePolicyTests(unittest.TestCase):
    def test_key_policy_has_only_exact_runtime_and_key_admin_principals(self) -> None:
        policy = resource("SigningKey")["Properties"]["KeyPolicy"]
        encoded = json.dumps(policy, sort_keys=True)
        self.assertNotIn(":root", encoded)
        self.assertNotIn("Enable IAM User Permissions", encoded)
        self.assertNotIn('"Principal": {"AWS": "*"}', encoded)
        values = statements(policy)
        self.assertEqual(len(values), 3)
        runtime = {"AWS": {"Fn::GetAtt": ["RuntimeRole", "Arn"]}}
        admin = {"AWS": {"Fn::GetAtt": ["KeyPolicyAdminRole", "Arn"]}}
        runtime_values = [item for item in values if item["Principal"] == runtime]
        admin_values = [item for item in values if item["Principal"] == admin]
        self.assertEqual(len(runtime_values), 2)
        self.assertEqual(len(admin_values), 1)
        self.assertEqual(set().union(*(actions(item) for item in runtime_values)), {"kms:Sign", "kms:GetPublicKey"})
        sign = next(item for item in runtime_values if actions(item) == {"kms:Sign"})
        self.assertEqual(sign["Condition"], {"StringEquals": {"kms:SigningAlgorithm": "ECDSA_SHA_256"}})
        self.assertNotIn("kms:Sign", actions(admin_values[0]))
        self.assertNotIn("kms:CreateGrant", actions(admin_values[0]))
        self.assertIn("kms:PutKeyPolicy", actions(admin_values[0]))

    def test_runtime_role_has_only_exact_table_log_and_key_operations(self) -> None:
        role = resource("RuntimeRole")["Properties"]
        self.assertEqual(role["PermissionsBoundary"], {"Ref": "ExternalPermissionsBoundaryArn"})
        trust = statements(role["AssumeRolePolicyDocument"])
        self.assertEqual(trust, [{
            "Sid": "OnlyLambdaService",
            "Effect": "Allow",
            "Principal": {"Service": "lambda.amazonaws.com"},
            "Action": "sts:AssumeRole",
        }])
        inline = statements(role["Policies"][0]["PolicyDocument"])
        self.assertEqual(len(inline), 6)
        by_sid = {item["Sid"]: item for item in inline}
        expected = {
            "ExactAllocatorTransactionReads": (
                {"dynamodb:GetItem"}, "SignedTimeTable", "TransactGetItems",
            ),
            "ExactAllocatorTransactionWrites": (
                {"dynamodb:ConditionCheckItem", "dynamodb:PutItem"},
                "SignedTimeTable", "TransactWriteItems",
            ),
            "ExactLineageTransactionReads": (
                {"dynamodb:GetItem"}, "LineageTable", "TransactGetItems",
            ),
            "ExactLineageTransactionChecks": (
                {"dynamodb:ConditionCheckItem"},
                "LineageTable", "TransactWriteItems",
            ),
        }
        for sid, (expected_actions, table, enclosing) in expected.items():
            statement = by_sid[sid]
            self.assertEqual(actions(statement), expected_actions)
            self.assertEqual(statement["Resource"], {"Fn::GetAtt": [table, "Arn"]})
            self.assertEqual(statement["Condition"], {
                "ForAnyValue:StringEquals": {
                    "dynamodb:EnclosingOperation": [enclosing],
                },
            })
        lineage_actions = set().union(*(
            actions(item)
            for item in inline
            if item.get("Resource") == {"Fn::GetAtt": ["LineageTable", "Arn"]}
        ))
        self.assertEqual(
            lineage_actions,
            {"dynamodb:GetItem", "dynamodb:ConditionCheckItem"},
        )
        identity = by_sid["ExactTableIdentityReads"]
        self.assertEqual(actions(identity), {"dynamodb:DescribeTable"})
        self.assertEqual(identity["Resource"], [
            {"Fn::GetAtt": ["SignedTimeTable", "Arn"]},
            {"Fn::GetAtt": ["LineageTable", "Arn"]},
        ])
        logs = next(item for item in inline if "logs:PutLogEvents" in actions(item))
        self.assertEqual(actions(logs), {"logs:CreateLogStream", "logs:PutLogEvents"})
        self.assertEqual(logs["Resource"], {
            "Fn::Sub": [
                "${LogGroupArn}:*",
                {"LogGroupArn": {"Fn::GetAtt": ["FunctionLogGroup", "Arn"]}},
            ]
        })
        key_statements = statements(resource("RuntimeSigningKeyPolicy")["Properties"]["PolicyDocument"])
        self.assertEqual(set().union(*(actions(item) for item in key_statements)), {"kms:Sign", "kms:GetPublicKey"})
        sign = next(item for item in key_statements if actions(item) == {"kms:Sign"})
        read = next(
            item for item in key_statements
            if actions(item) == {"kms:GetPublicKey"}
        )
        self.assertEqual(sign["Resource"], {"Fn::GetAtt": ["SigningKey", "Arn"]})
        self.assertEqual(read["Resource"], [
            {"Fn::GetAtt": ["SigningKey", "Arn"]},
            {"Fn::GetAtt": ["LineageKey", "Arn"]},
        ])
        sign = next(item for item in key_statements if actions(item) == {"kms:Sign"})
        self.assertEqual(sign["Condition"], {"StringEquals": {"kms:SigningAlgorithm": "ECDSA_SHA_256"}})

    def test_lineage_key_excludes_runtime_signing(self) -> None:
        policy = resource("LineageKey")["Properties"]["KeyPolicy"]
        encoded = json.dumps(policy, sort_keys=True)
        self.assertNotIn(":root", encoded)
        values = statements(policy)
        runtime = {"AWS": {"Fn::GetAtt": ["RuntimeRole", "Arn"]}}
        transition = {"AWS": {"Fn::GetAtt": ["LineageTransitionRole", "Arn"]}}
        admin = {"AWS": {"Fn::GetAtt": ["KeyPolicyAdminRole", "Arn"]}}
        runtime_values = [item for item in values if item["Principal"] == runtime]
        transition_values = [
            item for item in values if item["Principal"] == transition
        ]
        admin_values = [item for item in values if item["Principal"] == admin]
        self.assertEqual(
            [actions(item) for item in runtime_values], [{"kms:GetPublicKey"}],
        )
        self.assertEqual(
            [actions(item) for item in transition_values], [{"kms:Sign"}],
        )
        self.assertEqual(len(admin_values), 1)
        self.assertNotIn("kms:Sign", actions(admin_values[0]))


    def test_api_integrates_and_permits_only_the_published_alias_post_path(self) -> None:
        integration = resource("SignedTimeIntegration")["Properties"]
        self.assertEqual(integration["IntegrationType"], "AWS_PROXY")
        self.assertEqual(integration["IntegrationMethod"], "POST")
        self.assertEqual(integration["IntegrationUri"], {
            "Fn::Sub": [
                "arn:${AWS::Partition}:apigateway:${AWS::Region}:lambda:path/2015-03-31/functions/${AliasArn}/invocations",
                {"AliasArn": {"Fn::GetAtt": ["SignedTimeAlias", "AliasArn"]}},
            ]
        })
        self.assertEqual(resource("SignedTimeRoute")["Properties"]["RouteKey"], "POST /v1/time")
        permission = resource("AliasInvokePermission")["Properties"]
        self.assertEqual(permission["Action"], "lambda:InvokeFunction")
        self.assertEqual(permission["FunctionName"], {"Ref": "SignedTimeAlias"})
        self.assertEqual(permission["Principal"], "apigateway.amazonaws.com")
        self.assertEqual(permission["SourceAccount"], {"Ref": "AWS::AccountId"})
        self.assertEqual(permission["SourceArn"], {
            "Fn::Sub": "arn:${AWS::Partition}:execute-api:${AWS::Region}:${AWS::AccountId}:${SignedTimeApi}/$default/POST/v1/time"
        })
        for name, item in resource_map_by_type("AWS::ApiGatewayV2::Integration"):
            self.assertEqual(name, "SignedTimeIntegration")
            self.assertNotEqual(item["Properties"]["IntegrationUri"], {"Fn::GetAtt": ["SignedTimeFunction", "Arn"]})

    def test_no_other_lambda_permission_or_route_broadens_ingress(self) -> None:
        permission_names = [name for name, _ in resource_map_by_type("AWS::Lambda::Permission")]
        route_names = [name for name, _ in resource_map_by_type("AWS::ApiGatewayV2::Route")]
        self.assertEqual(permission_names, ["AliasInvokePermission"])
        self.assertEqual(route_names, ["SignedTimeRoute"])


def resource_map_by_type(type_name: str):
    from cloudformation_support import RESOURCES
    return [(name, item) for name, item in RESOURCES.items() if item["Type"] == type_name]


if __name__ == "__main__":
    unittest.main()
