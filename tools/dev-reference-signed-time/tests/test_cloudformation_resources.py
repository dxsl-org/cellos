from __future__ import annotations

import unittest

from path_bootstrap import ROOT  # noqa: F401
from cloudformation_support import RESOURCES, TEMPLATE, resource


class TemplateContractTests(unittest.TestCase):
    def test_parameters_are_required_constrained_and_nonsecret(self) -> None:
        expected = {
            "ArtifactS3Bucket",
            "ArtifactS3Key",
            "ArtifactS3ObjectVersion",
            "CodeSha256",
            "SignerProfileVersionArn",
            "ExternalPermissionsBoundaryArn",
            "CloudFormationServiceRoleArn",
            "DeployTrustedPrincipalArn",
            "CodeSignTrustedPrincipalArn",
            "IamTrustedPrincipalArn",
            "KeyPolicyTrustedPrincipalArn",
            "LineageTransitionTrustedPrincipalArn",
            "AlarmTopicArn",
        }
        parameters = TEMPLATE["Parameters"]
        self.assertEqual(set(parameters), expected)
        for parameter in parameters.values():
            self.assertNotIn("Default", parameter)
            self.assertNotIn("NoEcho", parameter)
            self.assertIn("AllowedPattern", parameter)
        self.assertEqual(parameters["CodeSha256"]["MinLength"], 44)
        self.assertEqual(parameters["CodeSha256"]["MaxLength"], 44)

    def test_architecture_is_explicitly_a_non_deployable_harness(self) -> None:
        metadata = TEMPLATE["Metadata"]["CellOSArchitecture"]
        self.assertEqual(metadata["Classification"], "SOFTWARE_HARNESS")
        self.assertIs(metadata["Deployable"], False)
        self.assertIs(metadata["LiveIsolationClaim"], False)
        controls = " ".join(metadata["ExternalControlsRequired"])
        for required in ("permissions boundary", "SCP", "CloudFormation service role", "key-policy administrator", "code-sign", "Operator"):
            self.assertIn(required, controls)
        self.assertIn("launch role, boundary and SCP contents", metadata["EvidenceRequirement"])
        self.assertIn("external requirements", metadata["SignerApprovalEvidence"])
        outputs = TEMPLATE["Outputs"]
        self.assertEqual(outputs["HarnessClassification"]["Value"], "SOFTWARE_HARNESS")
        self.assertIn("NOT_DEPLOYABLE", outputs["DeploymentReadiness"]["Value"])
        self.assertEqual(outputs["LiveIsolationEvidence"]["Value"], "REQUIRED_NOT_PROVIDED")
        self.assertEqual(
            outputs["KeyAdministratorRequirement"]["Value"],
            {"Fn::GetAtt": ["KeyPolicyAdminRole", "Arn"]},
        )

    def test_graph_has_exact_security_relevant_resource_cardinalities(self) -> None:
        counts: dict[str, int] = {}
        for item in RESOURCES.values():
            counts[item["Type"]] = counts.get(item["Type"], 0) + 1
        self.assertEqual(counts, {
            "AWS::DynamoDB::Table": 2,
            "AWS::IAM::Role": 6,
            "AWS::KMS::Key": 2,
            "AWS::IAM::Policy": 3,
            "AWS::Logs::LogGroup": 2,
            "AWS::Lambda::CodeSigningConfig": 1,
            "AWS::Lambda::Function": 1,
            "AWS::Lambda::Version": 1,
            "AWS::Lambda::Alias": 1,
            "AWS::ApiGatewayV2::Api": 1,
            "AWS::ApiGatewayV2::Integration": 1,
            "AWS::ApiGatewayV2::Route": 1,
            "AWS::ApiGatewayV2::Stage": 1,
            "AWS::Lambda::Permission": 1,
            "AWS::CloudWatch::Alarm": 5,
        })

    def test_tables_are_retained_deletion_protected_and_pitr_enabled(self) -> None:
        for name in ("SignedTimeTable", "LineageTable"):
            table = resource(name)
            self.assertEqual(table["DeletionPolicy"], "Retain")
            self.assertEqual(table["UpdateReplacePolicy"], "Retain")
            properties = table["Properties"]
            self.assertEqual(properties["BillingMode"], "PAY_PER_REQUEST")
            self.assertEqual(
                properties["AttributeDefinitions"],
                [{"AttributeName": "pk", "AttributeType": "S"}],
            )
            self.assertEqual(
                properties["KeySchema"],
                [{"AttributeName": "pk", "KeyType": "HASH"}],
            )
            self.assertIs(properties["DeletionProtectionEnabled"], True)
            self.assertEqual(properties["PointInTimeRecoverySpecification"], {
                "PointInTimeRecoveryEnabled": True,
                "RecoveryPeriodInDays": 35,
            })
            self.assertEqual(
                properties["SSESpecification"],
                {"SSEEnabled": True, "SSEType": "KMS"},
            )
            self.assertNotIn("KMSMasterKeyId", properties["SSESpecification"])
        self.assertNotEqual(
            resource("SignedTimeTable")["Properties"]["TableName"],
            resource("LineageTable")["Properties"]["TableName"],
        )

    def test_signing_keys_are_retained_p256_and_role_first(self) -> None:
        expected_dependencies = {
            "SigningKey": {"RuntimeRole", "KeyPolicyAdminRole"},
            "LineageKey": {
                "RuntimeRole", "LineageTransitionRole", "KeyPolicyAdminRole",
            },
        }
        for name, dependencies in expected_dependencies.items():
            key = resource(name)
            self.assertEqual(key["DeletionPolicy"], "Retain")
            self.assertEqual(key["UpdateReplacePolicy"], "Retain")
            self.assertEqual(set(key["DependsOn"]), dependencies)
            properties = key["Properties"]
            self.assertEqual(properties["KeySpec"], "ECC_NIST_P256")
            self.assertEqual(properties["KeyUsage"], "SIGN_VERIFY")
            self.assertIs(properties["MultiRegion"], False)
            self.assertEqual(properties["PendingWindowInDays"], 30)
            self.assertIs(properties["BypassPolicyLockoutSafetyCheck"], True)
        attached = resource("RuntimeSigningKeyPolicy")
        for retained_name in (
            "RuntimeRole", "KeyPolicyAdminRole", "LineageTransitionRole",
            "RuntimeSigningKeyPolicy", "LineageTransitionPolicy",
        ):
            retained = resource(retained_name)
            self.assertEqual(retained["DeletionPolicy"], "Retain")
            self.assertEqual(retained["UpdateReplacePolicy"], "Retain")
        self.assertEqual(set(attached["DependsOn"]), {"SigningKey", "LineageKey"})
        self.assertEqual(attached["Properties"]["Roles"], [{"Ref": "RuntimeRole"}])

    def test_function_requires_exact_signed_versioned_artifact(self) -> None:
        config = resource("FunctionCodeSigningConfig")["Properties"]
        self.assertEqual(config["AllowedPublishers"]["SigningProfileVersionArns"], [{"Ref": "SignerProfileVersionArn"}])
        self.assertEqual(config["CodeSigningPolicies"]["UntrustedArtifactOnDeployment"], "Enforce")
        function = resource("SignedTimeFunction")
        self.assertEqual(function["DependsOn"], ["RuntimeSigningKeyPolicy", "FunctionLogGroup", "FunctionCodeSigningConfig"])
        self.assertEqual(function["DeletionPolicy"], "Retain")
        self.assertEqual(function["UpdateReplacePolicy"], "Retain")
        properties = function["Properties"]
        self.assertEqual(properties["PackageType"], "Zip")
        self.assertEqual(properties["FunctionName"], "cellos-dev-reference-signed-time")
        self.assertEqual(properties["Handler"], "handler.lambda_handler")
        self.assertEqual(properties["CodeSigningConfigArn"], {
            "Fn::GetAtt": ["FunctionCodeSigningConfig", "CodeSigningConfigArn"]
        })
        self.assertEqual(properties["Code"], {
            "S3Bucket": {"Ref": "ArtifactS3Bucket"},
            "S3Key": {"Ref": "ArtifactS3Key"},
            "S3ObjectVersion": {"Ref": "ArtifactS3ObjectVersion"},
        })
        self.assertNotIn("SIGNED_TIME_LOG_STREAM", properties["Environment"]["Variables"])
        version = resource("SignedTimeVersion")
        self.assertEqual(version["Properties"]["CodeSha256"], {"Ref": "CodeSha256"})
        self.assertEqual(version["DeletionPolicy"], "Retain")
        self.assertEqual(version["UpdateReplacePolicy"], "Retain")
        self.assertEqual(resource("SignedTimeAlias")["Properties"]["FunctionVersion"], {"Fn::GetAtt": ["SignedTimeVersion", "Version"]})

    def test_logs_and_basic_alarms_are_retained_and_bounded(self) -> None:
        for name in ("FunctionLogGroup", "ApiAccessLogGroup"):
            log = resource(name)
            self.assertEqual(log["DeletionPolicy"], "Retain")
            self.assertEqual(log["UpdateReplacePolicy"], "Retain")
            self.assertEqual(log["Properties"]["RetentionInDays"], 365)
        self.assertNotIn("FunctionLogStream", RESOURCES)
        alarms = [
            item["Properties"]
            for item in RESOURCES.values()
            if item["Type"] == "AWS::CloudWatch::Alarm"
        ]
        self.assertEqual({item["MetricName"] for item in alarms}, {
            "Errors", "Throttles", "5xx", "SystemErrors", "ThrottledRequests",
        })
        for alarm in alarms:
            self.assertEqual(alarm["AlarmActions"], [{"Ref": "AlarmTopicArn"}])
        self.assertEqual(resource("ApiServerErrorsAlarm")["Properties"]["Dimensions"], [
            {"Name": "ApiId", "Value": {"Ref": "SignedTimeApi"}},
            {"Name": "Stage", "Value": "$default"},
        ])
        for name in ("TableSystemErrorsAlarm", "TableThrottledRequestsAlarm"):
            self.assertEqual(resource(name)["Properties"]["Dimensions"], [
                {"Name": "TableName", "Value": {"Ref": "SignedTimeTable"}},
                {"Name": "Operation", "Value": "TransactWriteItems"},
            ])


if __name__ == "__main__":
    unittest.main()
