# Phase 07: Authenticate Software Evidence Pipeline Threat Model

## Backend: GitHub Actions Native Attestations (Sigstore)

### 1. Runner Impersonation
**Threat:** An attacker compromises a GitHub Actions runner or spins up a malicious runner in a different context to sign a forged prequalification catalog.
**Mitigation:**
- **OIDC Subject Identity:** GitHub Actions provisions an ephemeral OIDC token for each job, securely binding the repository name, workflow file path, runner environment, and git reference (branch/tag/commit) into the identity.
- **Verification Rule:** The offline validator MUST inspect the `certificate_identity` (the workflow ref) and `certificate_oidc_issuer` in the signed bundle to ensure the attestation originated *specifically* from the authorized repository (e.g., `https://github.com/dmin/cellos/.github/workflows/ci.yml@refs/heads/main`).
- By restricting the valid identity, any attestation signed by a runner outside the designated workflow and repository will be cryptographically rejected.

### 2. Replay Attacks
**Threat:** An attacker intercepts a valid attestation bundle from a prior (successful but outdated) CI run and presents it as the evidence for a current, tampered release or build.
**Mitigation:**
- **Digest Binding:** The attestation intrinsically binds the exact SHA-256 digest of the artifact (the prequalification catalog) to the signature payload.
- **Workflow Inputs:** The offline validator verifies that the provided artifact matches the digest recorded in the bundle's subject. Thus, if the artifact is modified, the signature is invalid over the new artifact, and old bundles cannot be replayed over new, differing catalogs without triggering a digest mismatch.
- **Revision Tracking:** To completely defeat replay of old *valid* artifacts, the deployment orchestrator or downstream consumer must also assert the git revision (SHA) contained in the attestation matches the intended deployment baseline.
