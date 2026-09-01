from __future__ import annotations

import unittest

from path_bootstrap import ROOT  # noqa: F401
from cloudformation_support import TEMPLATE, actions, allow_statements, inline_statements, resource, statements


ADMIN_ROLES = {
    "DeployAdminRole": "DeployTrustedPrincipalArn",
    "CodeSignApprovalRole": "CodeSignTrustedPrincipalArn",
    "IamReviewRole": "IamTrustedPrincipalArn",
    "KeyPolicyAdminRole": "KeyPolicyTrustedPrincipalArn",
}
ROLE_NAMES = {
    "DeployAdminRole": "cellos-dev-reference-signed-time-deploy-admin",
    "CodeSignApprovalRole": "cellos-dev-reference-signed-time-code-sign-approval",
    "IamReviewRole": "cellos-dev-reference-signed-time-iam-review",
    "KeyPolicyAdminRole": "cellos-dev-reference-signed-time-key-policy-admin",
}
CFN_MUTATIONS = {
    "cloudformation:CreateStack",
    "cloudformation:UpdateStack",
    "cloudformation:CreateChangeSet",
    "cloudformation:ExecuteChangeSet",
}
DIRECT_GUARDS = {
    "kms:Sign",
    "kms:CreateGrant",
    "lambda:UpdateFunctionCode",
    "lambda:UpdateFunctionConfiguration",
    "lambda:PutFunctionCodeSigningConfig",
    "iam:UpdateAssumeRolePolicy",
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:CreatePolicyVersion",
}

def contains_wildcard(value: object) -> bool:
    if isinstance(value, str):
        return "*" in value
    if isinstance(value, list):
        return any(contains_wildcard(item) for item in value)
    if isinstance(value, dict):
        return any(contains_wildcard(item) for item in value.values())
    return False



class AdministrationRoleTests(unittest.TestCase):
    def test_four_admin_roles_have_external_boundary_and_exact_trust(self) -> None:
        for role_name, principal_parameter in ADMIN_ROLES.items():
            properties = resource(role_name)["Properties"]
            self.assertEqual(properties["PermissionsBoundary"], {"Ref": "ExternalPermissionsBoundaryArn"})
            trust = statements(properties["AssumeRolePolicyDocument"])
            self.assertEqual(properties["RoleName"], ROLE_NAMES[role_name])
            self.assertEqual(len(trust), 1)
            self.assertEqual(trust[0]["Principal"], {"AWS": {"Ref": principal_parameter}})
            self.assertEqual(actions(trust[0]), {"sts:AssumeRole"})
            self.assertEqual(trust[0]["Condition"], {"ArnEquals": {"aws:PrincipalArn": {"Ref": principal_parameter}}})

    def test_each_admin_explicitly_denies_direct_signing_and_mutation(self) -> None:
        runtime_arn = {"Fn::GetAtt": ["RuntimeRole", "Arn"]}
        for role_name in ADMIN_ROLES:
            denied = [item for item in inline_statements(role_name) if item["Effect"] == "Deny"]
            broad = [item for item in denied if item.get("Resource") == "*"]
            guarded_actions = set().union(*(actions(item) for item in broad))
            self.assertTrue(DIRECT_GUARDS <= guarded_actions, role_name)
            runtime = [item for item in denied if item.get("Resource") == runtime_arn]
            self.assertEqual(len(runtime), 1, role_name)
            self.assertEqual(actions(runtime[0]), {"sts:AssumeRole", "iam:PassRole"})
        for role_name in ("DeployAdminRole", "CodeSignApprovalRole", "IamReviewRole"):
            denied_actions = set().union(*(
                actions(item)
                for item in inline_statements(role_name)
                if item["Effect"] == "Deny"
            ))
            self.assertIn("kms:PutKeyPolicy", denied_actions)
        key_denies = set().union(*(
            actions(item)
            for item in inline_statements("KeyPolicyAdminRole")
            if item["Effect"] == "Deny"
        ))
        self.assertNotIn("kms:PutKeyPolicy", key_denies)

    def test_non_deploy_admins_deny_all_indirect_cloudformation_mutation(self) -> None:
        for role_name in ("CodeSignApprovalRole", "IamReviewRole", "KeyPolicyAdminRole"):
            matches = [
                item for item in inline_statements(role_name)
                if item["Effect"] == "Deny" and CFN_MUTATIONS <= actions(item)
            ]
            self.assertEqual(len(matches), 1, role_name)
            self.assertEqual(matches[0]["Resource"], "*")
            self.assertNotIn("Condition", matches[0])

    def test_deploy_role_splits_actions_by_supported_condition_and_resource(self) -> None:
        values = inline_statements("DeployAdminRole")
        create_deny = next(item for item in values if actions(item) == {"cloudformation:CreateStack"})
        self.assertEqual(create_deny["Effect"], "Deny")
        self.assertNotIn("Condition", create_deny)
        update_deny = next(item for item in values if item["Effect"] == "Deny" and "cloudformation:UpdateStack" in actions(item))
        self.assertEqual(actions(update_deny), {"cloudformation:UpdateStack", "cloudformation:CreateChangeSet"})
        self.assertEqual(update_deny["Condition"], {
            "StringNotEquals": {"cloudformation:RoleArn": {"Ref": "CloudFormationServiceRoleArn"}}
        })
        allows = [item for item in values if item["Effect"] == "Allow"]
        update = next(item for item in allows if "cloudformation:UpdateStack" in actions(item))
        self.assertEqual(actions(update), {"cloudformation:UpdateStack", "cloudformation:CreateChangeSet"})
        self.assertEqual(update["Resource"], {"Ref": "AWS::StackId"})
        self.assertEqual(update["Condition"], {
            "StringEquals": {"cloudformation:RoleArn": {"Ref": "CloudFormationServiceRoleArn"}}
        })
        inspect = next(item for item in allows if "cloudformation:DescribeStacks" in actions(item))
        self.assertEqual(actions(inspect), {"cloudformation:DescribeStacks", "cloudformation:GetTemplate"})
        self.assertEqual(inspect["Resource"], {"Ref": "AWS::StackId"})
        self.assertNotIn("Condition", inspect)
        changes = next(item for item in allows if "cloudformation:ExecuteChangeSet" in actions(item))
        self.assertEqual(actions(changes), {
            "cloudformation:DescribeChangeSet", "cloudformation:DeleteChangeSet", "cloudformation:ExecuteChangeSet",
        })
        self.assertEqual(changes["Resource"], {
            "Fn::Sub": "arn:${AWS::Partition}:cloudformation:${AWS::Region}:${AWS::AccountId}:changeSet/cellos-dev-reference-signed-time-*/*"
        })
        self.assertNotIn("Condition", changes)
        self.assertFalse(any("cloudformation:CreateStack" in actions(item) for item in allows))
        pass_role = next(item for item in allows if actions(item) == {"iam:PassRole"})
        self.assertEqual(pass_role["Resource"], {"Ref": "CloudFormationServiceRoleArn"})
        self.assertEqual(pass_role["Condition"], {
            "StringEquals": {"iam:PassedToService": "cloudformation.amazonaws.com"}
        })

    def test_no_admin_allow_can_sign_or_assume_the_runtime(self) -> None:
        runtime_arn = {"Fn::GetAtt": ["RuntimeRole", "Arn"]}
        for role_name in ADMIN_ROLES:
            for item in inline_statements(role_name):
                if item["Effect"] != "Allow":
                    continue
                self.assertNotIn("kms:Sign", actions(item), role_name)
                self.assertFalse(
                    "sts:AssumeRole" in actions(item) and item.get("Resource") == runtime_arn,
                    role_name,
                )

    def test_wildcard_allows_are_limited_to_required_resource_conventions(self) -> None:
        wildcard_allows = [
            (name, item)
            for name, item in allow_statements()
            if contains_wildcard(item.get("Resource"))
        ]
        self.assertEqual(
            {name for name, _ in wildcard_allows},
            {"SigningKey", "LineageKey", "RuntimeRole", "DeployAdminRole"},
        )
        runtime = [item for name, item in wildcard_allows if name == "RuntimeRole"]
        self.assertEqual(len(runtime), 1)
        self.assertEqual(actions(runtime[0]), {"logs:CreateLogStream", "logs:PutLogEvents"})
        deploy = [item for name, item in wildcard_allows if name == "DeployAdminRole"]
        self.assertEqual(len(deploy), 1)
        self.assertEqual(actions(deploy[0]), {
            "cloudformation:DescribeChangeSet", "cloudformation:DeleteChangeSet", "cloudformation:ExecuteChangeSet",
        })
        for name, item in wildcard_allows:
            if name not in {"SigningKey", "LineageKey"}:
                continue
            self.assertTrue(actions(item) <= {
                "kms:Sign", "kms:GetPublicKey", "kms:DescribeKey", "kms:GetKeyPolicy", "kms:PutKeyPolicy",
                "kms:EnableKey", "kms:DisableKey", "kms:ScheduleKeyDeletion", "kms:CancelKeyDeletion",
                "kms:TagResource", "kms:UntagResource",
            })

    def test_admin_allows_are_narrow_and_role_specific(self) -> None:
        code_allows = [item for item in inline_statements("CodeSignApprovalRole") if item["Effect"] == "Allow"]
        self.assertEqual(len(code_allows), 1)
        self.assertEqual(actions(code_allows[0]), {"signer:GetSigningProfile"})
        self.assertEqual(code_allows[0]["Resource"], {
            "Fn::Join": [
                "/",
                [
                    {"Fn::Select": [0, {"Fn::Split": ["/", {"Ref": "SignerProfileVersionArn"}]}]},
                    {"Fn::Select": [1, {"Fn::Split": ["/", {"Ref": "SignerProfileVersionArn"}]}]},
                    {"Fn::Select": [2, {"Fn::Split": ["/", {"Ref": "SignerProfileVersionArn"}]}]},
                ],
            ]
        })
        key_policy = resource("KeyPolicyAdminReadPolicy")["Properties"]
        self.assertEqual(key_policy["Roles"], [{"Ref": "KeyPolicyAdminRole"}])
        key_allow = statements(key_policy["PolicyDocument"])[0]
        self.assertEqual(actions(key_allow), {"kms:DescribeKey", "kms:GetKeyPolicy"})
        self.assertEqual(key_allow["Resource"], [
            {"Fn::GetAtt": ["SigningKey", "Arn"]},
            {"Fn::GetAtt": ["LineageKey", "Arn"]},
        ])



if __name__ == "__main__":
    unittest.main()
