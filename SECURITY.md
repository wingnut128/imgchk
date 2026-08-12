# Security Policy

## Reporting a Vulnerability

Please report security vulnerabilities privately by opening a
[security advisory](https://github.com/wingnut128/imgchk/security/advisories/new).

Do **not** open a public issue for security reports.

You can expect an initial response within a few days. If the issue is
confirmed, we will coordinate a fix and a disclosure timeline with you.

## Verifying Release Integrity

All releases include cryptographic attestations:

- **Build provenance**: Each binary carries a SLSA v1 attestation, keyless-signed via GitHub OIDC.
- **SBOM**: A CycloneDX software bill of materials is published with each release.

Both can be verified using the GitHub CLI. See the **Verifying releases** section in [README.md](README.md#verifying-releases) for commands and instructions.

## Supported Versions

Only the latest released version receives security fixes. Older versions
are not backported.
