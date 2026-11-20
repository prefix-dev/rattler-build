# Sigstore attestations

Sigstore is a way to cryptographically sign packages (or any binary artifacts). It was pioneered in the container space, but has been adopted by many packaging ecosystems: PyPI, Python releases, Rust crates, Homebrew, and Rubygems.

Rattler-Build supports creating Sigstore attestations for conda packages, allowing you to cryptographically sign your packages and provide verifiable provenance information.

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

!!! warning "One attestation per package"
    Per [CEP-27](https://conda.org/learn/ceps/cep-0027), a Conda publish
    attestation describes exactly **one** `.conda` artifact — its `subject`
    contains a single name and digest. If a build produces multiple packages
    (for example a multi-output recipe, or a matrix build across platforms),
    each package needs its own attestation bundle. Tools that accept a glob
    like `**/*.conda` and emit a single bundle covering all matches do **not**
    produce a CEP-27 compliant attestation, and uploads using such a bundle
    will fail to verify.

## Automatic attestation generation

The easiest way to create Sigstore attestations is using the
`--generate-attestation` flag. The same flag is available on both
`rattler-build publish` (which builds and uploads in one step) and on
`rattler-build upload prefix` (which uploads pre-built packages):

```bash
# Build and publish in one go
rattler-build publish ./my-recipe.yaml --to https://prefix.dev/my-channel --generate-attestation

# Or upload one or more pre-built packages
rattler-build upload prefix -c my-channel ./output/**/*.conda --generate-attestation
```

In both cases rattler-build will:

1. Take each `.conda` file individually,
2. Mint a fresh OIDC token from the CI runner and create one CEP-27 compliant
   Sigstore attestation **per package**,
3. Upload the package and its bundle to prefix.dev together.

You do **not** need `actions/attest`, `cosign`, or any other external tool
when you use this flag — rattler-build handles the per-package signing and
the one-attestation-per-package requirement automatically. This is the
recommended path for multi-output recipes and matrix builds.

Pass `--store-github-attestation` (on `upload prefix`) to additionally push
the generated bundle to GitHub's attestation API so it shows up in your
repository's attestation tab.

!!! note "Requirements"
    The `--generate-attestation` flag only works when:

    - Uploading to prefix.dev channels
    - Using Trusted Publishing (OIDC authentication) — it cannot be combined
      with an API key
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

## Attesting without uploading

When packages reach their channel through a different path, for example via a
staging channel and a separate service that copies them, you can sign the
attestation in CI without uploading anything:

```bash
rattler-build package attest output/linux-64/*.conda \
  --target-channel https://prefix.dev/my-channel \
  --store-github-attestation
```

For every package this writes a `<filename>.sigstore.json` bundle next to the
package (or into `--output-dir`). It uses the same CEP-27 conda publish
predicate as `--generate-attestation`, with `targetChannel` set to
`--target-channel`. With `--store-github-attestation`, the bundle is also
stored in the GitHub attestation API of `GITHUB_REPOSITORY`, using
`GITHUB_TOKEN`. A channel can later retrieve it by the package's SHA-256
digest:

```bash
gh api repos/OWNER/REPO/attestations/sha256:<digest>
```

The job needs `id-token: write` to sign, and `attestations: write` to store
the bundle on GitHub. Set `--target-channel` to the channel the package will
finally be served from, because prefix.dev checks it when the attestation is
attached.

## Manual attestation with GitHub Actions

!!! tip "Prefer `--generate-attestation` when possible"
    For most users — including multi-package and matrix builds — the
    `--generate-attestation` flag described above already does everything
    in this section automatically and correctly produces one bundle per
    package. Reach for the manual flow only when you need to customize the
    predicate, sign with an external trust root, or otherwise can't use
    rattler-build's built-in signing.

If you need more control over the attestation process, you can use GitHub's
official attest action to create the attestation separately. Because each
attestation must cover exactly one package (see the warning above), the
attestation and upload steps need to run **once per `.conda` artifact**.

The example below builds a single recipe and signs the resulting package:

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

      # Use GitHub's official attest action with the Conda predicate.
      # `subject-path` MUST point at a single `.conda` file — globs that
      # resolve to more than one file produce a non-compliant bundle.
      - uses: actions/attest@v1
        id: attest
        with:
          subject-path: ./output/linux-64/my-package-0.1.0-h123_0.conda
          predicate-type: "https://schemas.conda.org/attestations-publish-1.schema.json"
          predicate: '{"targetChannel": "https://prefix.dev/my-channel"}'

      # Upload the package together with its attestation bundle
      - name: Upload the package
        run: |
          rattler-build upload prefix -c my-channel \
            ./output/linux-64/my-package-0.1.0-h123_0.conda \
            --attestation ${{ steps.attest.outputs.bundle-path }}
```

### Multiple packages

If your build produces more than one `.conda` file (for example a multi-output
recipe, or several subdirs from a matrix build), `actions/attest` must be
invoked **once per package**, with `subject-path` pointing at exactly that
single file. There is no built-in way to produce one bundle for several
packages — that would not match the CEP-27 schema.

The most common pattern is a [matrix strategy](https://docs.github.com/actions/using-jobs/using-a-matrix-for-your-jobs)
that fans out one job per package, calls `actions/attest` once per job, and
then uploads the resulting `(package, bundle)` pair.

This approach gives you full control over the attestation creation and allows
you to customize the predicate or add additional attestation metadata.

## Source attestation verification

In addition to signing built packages, Rattler-Build can also **verify** the attestations of source archives during the build. This lets you ensure that the source code you're building from was produced by a trusted publisher (e.g., a specific GitHub Actions workflow).

### How it works

Add an `attestations` block to a URL source in your recipe:

```yaml title="recipe.yaml"
source:
  url: https://files.pythonhosted.org/packages/.../flask-3.1.1.tar.gz
  sha256: "6489f1..."
  attestations:
    publishers:
      - github:pallets/flask
```

When Rattler-Build downloads the source, it will:

1. Fetch the Sigstore attestation bundle (automatically derived for PyPI packages and uploaded GitHub release assets, or from `bundle_url`)
2. Verify the bundle cryptographically against the Sigstore trust root (fetched via TUF): the signing certificate chain at a verified signing time, the transparency log inclusion proof (or, for GitHub release attestations, an RFC 3161 timestamp), and the signature over the in-toto statement
3. Verify that the downloaded archive's SHA-256 is a subject of that signed statement
4. Check that **every** listed publisher matches a verified attestation

If any step fails, the build is aborted. A cached source is re-verified whenever its `attestations` configuration changes.

If `publishers` is omitted, the attestation must still verify cryptographically, but any signer is accepted and a warning is printed. Always list the publishers you expect.

### PyPI sources

For packages hosted on PyPI, the attestation bundle URL is automatically derived from the [PyPI attestation API](https://docs.pypi.org/api/integrity/). You only need to specify the publisher:

```yaml
source:
  url: https://files.pythonhosted.org/packages/.../flask-3.1.1.tar.gz
  sha256: "6489f1..."
  attestations:
    publishers:
      - github:pallets/flask
```

### GitHub release sources

For an archive **uploaded as a GitHub release asset**, Rattler-Build queries the repository attestations API for a release attestation whose subject is the archive's SHA-256. No unstable attestation download ID or `bundle_url` is needed.

For example, uv publishes an attested `source.tar.gz`. This binds verification to the `astral-sh/uv` repository:

```yaml
context:
  version: "0.12.1"

source:
  url: https://github.com/astral-sh/uv/releases/download/${{ version }}/source.tar.gz
  sha256: 5f0503f896f1209f6114835a7754369c2484331b0caab4c1ecd4579dfd2a31b0
  attestations:
    publishers:
      - github:astral-sh/uv
```

Internally, Rattler-Build requests:

```text
https://api.github.com/repos/astral-sh/uv/attestations/sha256:5f0503f896f1209f6114835a7754369c2484331b0caab4c1ecd4579dfd2a31b0?predicate_type=release
```

It verifies both the SHA-256 subject and the signed repository subject `pkg:github/astral-sh/uv@0.12.1`. The release certificate itself has GitHub's fixed identity `https://dotcom.releases.github.com`, so repository binding comes from that signed package URL rather than a workflow SAN.

GitHub's dynamically generated source archives (the "Source code" `.zip` and `.tar.gz` links) are **not** covered by a release attestation. Their release attestation contains the source commit's SHA-1, but that does not cryptographically authenticate the generated archive bytes. To make source verification possible, upload a `source.tar.gz` as a release asset; GitHub then includes its SHA-256 in the immutable release attestation.

You can also point `bundle_url` at any other attestation. For example, uv additionally publishes SLSA build provenance (via `actions/attest-build-provenance`), which is signed on the Sigstore public-good instance by the release workflow's identity. Query the attestations API with a different `predicate_type` to verify that instead:

```yaml
context:
  version: "0.12.1"
  sha256: 5f0503f896f1209f6114835a7754369c2484331b0caab4c1ecd4579dfd2a31b0

source:
  url: https://github.com/astral-sh/uv/releases/download/${{ version }}/source.tar.gz
  sha256: ${{ sha256 }}
  attestations:
    bundle_url: https://api.github.com/repos/astral-sh/uv/attestations/sha256:${{ sha256 }}?predicate_type=provenance
    publishers:
      - github:astral-sh/uv@refs/heads/main
```

A standalone `.sigstore.json` file published next to a release asset works the same way. Both DSSE (in-toto) bundles and message-signature bundles (e.g. from `cosign sign-blob --bundle`) are supported.

GitHub's API allows 60 unauthenticated requests per hour. Set `GITHUB_TOKEN` (or `GH_TOKEN`) to authenticate these requests, e.g. in CI. The token is only ever sent to `api.github.com`.

### Requiring more than one attestation

`attestations` also takes a list. Every entry is verified on its own and all of them must pass, so you can require independent kinds of evidence for the same file. A release attestation only proves the asset is part of the immutable release at a tag; build provenance proves which workflow and ref built it. Requiring both says that the file in the release was built by the release workflow:

```yaml
source:
  url: https://github.com/astral-sh/uv/releases/download/${{ version }}/source.tar.gz
  sha256: ${{ sha256 }}
  attestations:
    # The release attestation, found automatically
    - publishers:
        - github:astral-sh(115962839)/uv(699532645)@refs/tags/${{ version }}
    # The build provenance of the release workflow
    - bundle_url: https://api.github.com/repos/astral-sh/uv/attestations/sha256:${{ sha256 }}?predicate_type=provenance
      publishers:
        - github:astral-sh(115962839)/uv(699532645)@refs/heads/main
```

Each entry has its own publishers, because a ref means something different for each kind of attestation: the release tag for a release attestation, the ref of the signing workflow run for build provenance.

### Publisher format

Publishers are specified in `github:owner/repo` or `gitlab:owner/repo` format (owner and repository are matched case-insensitively). For workflow-produced attestations, the repository is matched against the Sigstore certificate's Subject Alternative Name (SAN) and the OIDC issuer must be GitHub Actions or GitLab respectively. For GitHub immutable release attestations, the certificate identifies GitHub's release service, so the repository is instead matched against the signed `pkg:github/owner/repo@tag` subject.

You can additionally pin the git ref the attestation was produced from by appending `@<ref>`:

```yaml
attestations:
  publishers:
    - github:pallets/flask@refs/tags/${{ version }}
```

For workflow-produced attestations (PyPI, build provenance, `.sigstore.json` bundles), the ref is compared with the ref encoded at the end of the certificate SAN (`...workflow.yml@refs/tags/3.1.1`). This is the ref of the signing *workflow run*: projects that release through `workflow_dispatch` on a branch are signed with that branch ref (e.g. `refs/heads/main`), not the tag. For GitHub release attestations, only tag refs (`refs/tags/<tag>`) are supported and are compared with the release tag.

### Pinning the repository ID, owner ID and environment

A repository name is not permanent: if the owner renames or deletes the account or repository, someone else can register the same name and publish attestations that match `github:owner/repo`. GitHub and GitLab give every repository and owner an immutable numeric ID that is never reused, and Sigstore records them in the attestation. Pin them by writing each ID in parentheses after its name:

```yaml
attestations:
  publishers:
    - github:pallets(16748505)/flask(596892)@refs/tags/${{ version }}
```

The ID after the owner is the `owner_id` (the user or organization), the one after the repository is the `repository_id`. Each is optional on its own.

To also pin the deployment environment, use the mapping form of a publisher:

```yaml
attestations:
  publishers:
    - github: pallets/flask
      ref: refs/tags/${{ version }}  # optional, same as the `@<ref>` suffix
      repository_id: 596892
      owner_id: 16748505
      environment: release
```

The mapping takes exactly one of `github` or `gitlab` (`owner/repo`, without IDs in parentheses), and optionally:

- `ref`: the git ref, as described above
- `repository_id`: the immutable ID of the repository
- `owner_id`: the immutable ID of the user or organization that owns it
- `environment`: the deployment environment the signing workflow ran in, e.g. `release`. Environments can require reviewers or restrict which branches deploy, so pinning one means the attestation passed those protections.

Every pinned value must be present in the attestation and equal, in addition to the repository name. For workflow-produced attestations the values come from the signing certificate; for GitHub release attestations the IDs come from the signed release statement, which does not record an environment, so an `environment` pin never matches a release attestation.

You don't need to look the IDs up: when a publisher without pinned IDs verifies, Rattler-Build logs the pinned publisher string to paste into the recipe. The IDs match the ones in GitHub's immutable OIDC subject claims (`repo:pallets@16748505/flask@596892:...`).

The rendered recipe stored in the package (`info/recipe/rendered_recipe.yaml`) always records the IDs the attestation was verified with: its `finalized_sources` section fills in the `repository_id` and `owner_id` of every publisher that did not pin them, so you can later check exactly which repository a package's source was attested by.

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
