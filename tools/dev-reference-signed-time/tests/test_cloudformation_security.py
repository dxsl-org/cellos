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
        self.assertEqual(len(inline), 2)
        table = next(item for item in inline if "dynamodb:TransactGetItems" in actions(item))
        self.assertEqual(actions(table), {"dynamodb:TransactGetItems", "dynamodb:TransactWriteItems"})
        self.assertEqual(table["Resource"], {"Fn::GetAtt": ["SignedTimeTable", "Arn"]})
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
        for item in key_statements:
            self.assertEqual(item["Resource"], {"Fn::GetAtt": ["SigningKey", "Arn"]})
        sign = next(item for item in key_statements if actions(item) == {"kms:Sign"})
        self.assertEqual(sign["Condition"], {"StringEquals": {"kms:SigningAlgorithm": "ECDSA_SHA_256"}})

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
