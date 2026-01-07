# Sigstore attestations

Sigstore is a way to cryptographically sign packages (or any binary artifacts). It was pioneered in the container space, but has been adopted by many packaging ecosystems: PyPI, Python releases, Rust crates, Homebrew, and Rubygems.

Rattler-build supports creating Sigstore attestations for conda packages, allowing you to cryptographically sign your packages and provide verifiable provenance information.

## What does a Sigstore attestation provide?

A Sigstore attestation ties the producer of a package (for example, a GitHub Actions workflow) to the package artifact. When the attestation is created, metadata about the artifact is registered in a "Transparency Log" which can be inspected at any time. The metadata contains information such as:

- The name and SHA256 hash of the package
- The CI workflow that built the package
- The repository Git hash that contained the CI workflow
- The organization and username that built the package

Using this information, you can attest that a given package was created and uploaded by a certain GitHub organization, and this is transparently logged on both the server and the Sigstore public good instance.

The attestation follows [CEP-27](https://conda.org/learn/ceps/cep-0027), which standardizes the in-toto attestation format with a Conda-specific predicate containing:

- The SHA256 hash of the package (guaranteed to be unique)
- The full filename (`{name}-{version}-{buildstring}.conda`)
- Optionally, the target channel URL (e.g., `https://prefix.dev/my-channel`)

## Automatic attestation generation

The easiest way to create Sigstore attestations is using the `--generate-attestation` flag when publishing to [prefix.dev](https://prefix.dev):

```bash
rattler-build publish ./my-recipe.yaml --to https://prefix.dev/my-channel --generate-attestation
```

This automatically:

1. Builds your package(s) from the recipe
2. Creates a Sigstore attestation using the OIDC identity from your CI environment
3. Uploads both the package and attestation to prefix.dev

!!! note "Requirements"
    The `--generate-attestation` flag only works when:

    - Uploading to prefix.dev channels
    - Using Trusted Publishing (OIDC authentication)
    - Running in a supported CI environment (e.g., GitHub Actions)

### GitHub Actions example

Here's a complete example workflow using automatic attestation generation:

```yaml title=".github/workflows/build.yml"
name: Build and publish with attestation

on: [push]

jobs:
  build:
    runs-on: ubuntu-latest
    # These permissions are needed for OIDC authentication
    permissions:
      id-token: write
      contents: read

    steps:
      - uses: actions/checkout@v4

      - name: Set up rattler-build
        uses: prefix-dev/rattler-build-action@v0.2.34

      - name: Build and publish with attestation
        run: |
          rattler-build publish ./recipe.yaml \
            --to https://prefix.dev/my-channel \
            --generate-attestation
```

## Manual attestation with GitHub Actions

If you need more control over the attestation process, you can use GitHub's official attest action to create the attestation separately:

```yaml title=".github/workflows/build.yml"
name: Package and sign

on: [push]

jobs:
  build:
    runs-on: ubuntu-latest
    # These permissions are needed to create a sigstore certificate
    permissions:
      id-token: write
      contents: read
      attestations: write

    steps:
      - uses: actions/checkout@v4

      - name: Build conda package
        uses: prefix-dev/rattler-build-action@v0.2.34

      # Use GitHub's official attest action with the Conda predicate
      - uses: actions/attest@v1
        id: attest
        with:
          subject-path: "**/*.conda"
          predicate-type: "https://schemas.conda.org/attestations-publish-1.schema.json"
          predicate: '{"targetChannel": "https://prefix.dev/my-channel"}'

      # Upload with the attestation bundle
      - name: Upload the package
        run: |
          rattler-build upload prefix -c my-channel ./output/**/*.conda \
            --attestation ${{ steps.attest.outputs.bundle-path }}
```

This approach gives you full control over the attestation creation and allows you to customize the predicate or add additional attestation metadata.

## Verifying attestations

Once packages are published with attestations, they can be verified using several tools:

### Using the GitHub CLI

```bash
gh attestation verify my-package-0.1.0-h123_0.conda \
  --owner my-org \
  --predicate-type "https://schemas.conda.org/attestations-publish-1.schema.json"
```

### Using cosign

```bash
cosign verify-blob \
  --certificate-identity-regexp "https://github.com/my-org/.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  my-package-0.1.0-h123_0.conda
```

### Using sigstore-python

```bash
pip install sigstore
sigstore verify identity \
  --cert-identity "https://github.com/my-org/my-repo/.github/workflows/build.yml@refs/heads/main" \
  --cert-oidc-issuer "https://token.actions.githubusercontent.com" \
  my-package-0.1.0-h123_0.conda
```

## Viewing attestations

You can find attestations for packages published to prefix.dev in several places:

- **GitHub**: View attestations in your repository's attestations tab (e.g., `https://github.com/my-org/my-repo/attestations`)
- **Sigstore public goods instance**: Search by package hash at [search.sigstore.dev](https://search.sigstore.dev)
- **prefix.dev**: View attestations on the package page

## Security benefits

Sigstore attestations provide several security benefits:

1. **Unforgeability**: Attestations cryptographically bind a package to its producer, preventing forgery of provenance metadata.

2. **Transparency**: All attestations are logged in a public, append-only transparency log. This means attackers cannot perform targeted attacks without leaving a public trace.

3. **No long-lived keys**: Sigstore uses ephemeral keys bound to identities (like GitHub Actions workflows), eliminating the risk of key compromise.

4. **Build provenance**: Machine identities in the attestation provide verifiable information about exactly which workflow, repository, and commit produced the package.

## Further reading

- [CEP-27: Standardizing a publish attestation for the conda ecosystem](https://conda.org/learn/ceps/cep-0027)
- [Securing the Conda Package Supply Chain with Sigstore](https://prefix.dev/blog/securing-the-conda-package-supply-chain-with-sigstore) (prefix.dev blog)
- [Sigstore documentation](https://docs.sigstore.dev/)
- [Example repository with full source code](https://github.com/prefix-dev/sigstore-example)
