# SLSA & SBOM Integration Research for rattler-build

> Research document — March 2026

## Table of Contents

1. [What rattler-build already has](#what-rattler-build-already-has)
2. [SBOM Standards Overview](#sbom-standards-overview)
3. [SLSA Framework Overview](#slsa-framework-overview)
4. [The in-toto Attestation Framework](#the-in-toto-attestation-framework)
5. [GitHub Actions Attestation Infrastructure](#github-actions-attestation-infrastructure)
6. [Gap Analysis: What Enterprise Customers Want](#gap-analysis-what-enterprise-customers-want)
7. [Integration Proposals for rattler-build](#integration-proposals-for-rattler-build)
8. [Implementation Roadmap](#implementation-roadmap)

---

## What rattler-build already has

rattler-build is in a **strong starting position**. Here's what already exists:

### Package metadata (stored in `info/`)

| File | Contents |
|------|----------|
| `info/index.json` | Name, version, build string, dependencies, constraints, platform |
| `info/about.json` | Homepage, license (SPDX), summary, repository URL |
| `info/paths.json` | Full file inventory with **SHA256 hashes** and sizes |
| `info/recipe/` | The original `recipe.yaml` + rendered recipe |
| `info/licenses/` | Copies of actual license files |

### Sigstore attestations (CEP-27)

rattler-build already supports Sigstore-based attestations following
[CEP-27](https://conda.org/learn/ceps/cep-0027):

- **Automatic attestation** via `--generate-attestation` when publishing to
  prefix.dev
- **Manual attestation** via GitHub's `actions/attest` with the Conda-specific
  predicate (`https://schemas.conda.org/attestations-publish-1.schema.json`)
- **Source attestation verification** (experimental) — verifies that source
  tarballs were signed by expected publishers (e.g., `github:pallets/flask`)
- Attestations are recorded in Sigstore's transparency log
- Verification via `gh attestation verify`, `cosign`, or `sigstore-python`

### What's captured in an attestation today

- SHA256 hash of the `.conda` package
- Full filename (`{name}-{version}-{buildstring}.conda`)
- Target channel URL
- CI workflow identity (GitHub Actions OIDC)
- Repository + Git commit that ran the CI

### What's NOT yet captured

- **No formal SBOM** (SPDX or CycloneDX) — dependency info exists in
  `index.json` but not in a standard SBOM format
- **No SLSA provenance predicate** — the current attestation uses a
  Conda-specific predicate, not the standard SLSA provenance predicate
- **No build environment capture** — compiler versions, OS details, env vars,
  and tool versions used during the build aren't recorded in a structured way
- **No dependency graph** — only direct `run` dependencies are listed, not the
  full resolved dependency tree
- **No VEX (Vulnerability Exploitability Exchange)** data

---

## SBOM Standards Overview

There are two dominant SBOM standards. Both are widely supported by tooling.

### SPDX (Software Package Data Exchange)

- **Maintained by**: Linux Foundation
- **ISO standard**: ISO/IEC 5962:2021 (the only internationally recognized SBOM
  standard)
- **Latest version**: SPDX 3.0 (April 2024), though many tools still use 2.3
- **Formats**: JSON, YAML, RDF/XML, Tag-Value
- **Primary focus**: License compliance and IP management
- **Key strength**: Extensive license expression support (SPDX license IDs are
  already used by rattler-build!)
- **SPDX 3.0 profiles**: Licensing, Security, Build, Usage, AI, Dataset

Since rattler-build already validates SPDX license expressions in recipes, there
is natural alignment here.

### CycloneDX (CDX)

- **Maintained by**: OWASP Foundation
- **Latest version**: 1.7 (October 2025; became ECMA-424 at 1.6)
- **Formats**: JSON, XML, Protocol Buffers
- **Primary focus**: Security and vulnerability management
- **Key strengths**:
  - Native VEX (Vulnerability Exploitability Exchange) support
  - SaaSBOM support
  - Lighter-weight / more developer-friendly
  - Better dependency relationship modeling
  - Formulation support (captures HOW something was built, not just what)

### Which one for rattler-build?

| Consideration | SPDX | CycloneDX |
|---|---|---|
| License compliance (enterprise) | Excellent | Good |
| Security/vulnerability focus | Good (3.0) | Excellent |
| Provenance citations | No | Yes (1.7 Citations) |
| Build process capture | Partial (3.0 Build profile) | Excellent (Formulation) |
| ISO standard | Yes | No |
| Already aligned (SPDX licenses) | Yes | N/A |
| Government requirements (US EO 14028) | Both accepted | Both accepted |
| Tooling ecosystem | Broad | Broad |

**Recommendation**: Support **both** formats for output, but start with
**CycloneDX** for the first implementation because:
1. Its "Formulation" feature can capture the build recipe/process natively
2. Better VEX integration for security scanning
3. More lightweight and developer-friendly
4. Protocol Buffers support is useful for programmatic consumption

Then add SPDX output for enterprises that require the ISO standard.

### What goes into a conda package SBOM?

A complete SBOM for a conda package should include:

```
Package Identity
├── name, version, build string, platform
├── PURL (pkg:conda/channel/name@version)
├── SHA256 of the .conda artifact
│
Dependencies (from resolved environment)
├── Direct runtime dependencies (from index.json `depends`)
├── Transitive dependencies (full solve)
├── Build dependencies (compilers, tools)
├── Host dependencies (libraries linked against)
│
Source Information
├── Source URL(s) + SHA256
├── Patches applied
├── Source attestation status (if verified)
│
Build Environment
├── rattler-build version
├── Compiler identities + versions
├── OS / platform details
├── Build timestamp
│
License Information
├── SPDX expression for the package
├── License expressions for all dependencies
│
Formulation (CycloneDX-specific)
├── The rendered recipe.yaml
├── Variant configuration used
├── Build script executed
```

---

## SLSA Framework Overview

[SLSA](https://slsa.dev) (Supply-chain Levels for Software Artifacts) is a
framework for ensuring the integrity of software artifacts throughout the supply
chain. It focuses primarily on **build provenance** — proving that an artifact
was built from specific sources by a specific builder.

### SLSA Build Track Levels (v1.0 / v1.1 / v1.2)

| Level | Name | What it means |
|-------|------|---------------|
| **L0** | No SLSA | No provenance |
| **L1** | Provenance Exists | Build process documented; provenance exists (can be unsigned) |
| **L2** | Hosted Build + Signed Provenance | Built on a hosted platform; provenance is signed and tamper-evident |
| **L3** | Hardened Build Platform | Build platform provides isolation between builds; signing keys are separate from build steps |

### SLSA Source Track (NEW in v1.2, November 2025)

SLSA v1.2 introduced a **Source Track**, covering threats from authoring,
reviewing, and managing source code:

| Level | Name | What it means |
|-------|------|---------------|
| **Source L1** | Version controlled | Source is in a version control system |
| **Source L2** | History and provenance | Branch protection, verified commits |
| **Source L3** | Continuous enforcement | Technical controls continuously enforced |

### Version timeline

- **v1.0** (2023): Build Track only
- **v1.1** (April 2025): Backwards-compatible clarifications, VSA verifier
  metadata
- **v1.2** (November 2025): Source Track introduction, backwards-compatible with
  v1.1

### Where rattler-build stands today

With the current Sigstore attestation support:

- **On GitHub Actions with `--generate-attestation`**: rattler-build effectively
  achieves **SLSA Build L2** — the build runs on a hosted platform (GitHub
  Actions) and the attestation is signed via Sigstore OIDC
- **For L3**: Would need to use GitHub's reusable workflows (to isolate the
  signing from the build) or the SLSA BYOB framework

However, the current attestation uses a **Conda-specific predicate**, not the
standard **SLSA Provenance predicate**. This matters because:
- Standard SLSA tooling (`slsa-verifier`) won't recognize it
- Enterprises using SLSA policy engines expect the standard format
- It can't be automatically evaluated against SLSA level requirements

### SLSA Provenance Predicate Format

The standard SLSA provenance predicate (used with the in-toto framework) looks
like:

```json
{
  "_type": "https://in-toto.io/Statement/v1",
  "subject": [
    {
      "name": "my-package-1.0.0-h1234_0.conda",
      "digest": { "sha256": "abc123..." }
    }
  ],
  "predicateType": "https://slsa.dev/provenance/v1",
  "predicate": {
    "buildDefinition": {
      "buildType": "https://prefix.dev/rattler-build/v1",
      "externalParameters": {
        "recipe": "recipe.yaml",
        "variants": { "python": "3.12" },
        "channel": "https://prefix.dev/conda-forge"
      },
      "internalParameters": {
        "rattler_build_version": "0.35.0",
        "archive_type": "conda"
      },
      "resolvedDependencies": [
        {
          "uri": "git+https://github.com/org/repo@refs/heads/main",
          "digest": { "gitCommit": "abc123..." }
        },
        {
          "uri": "https://conda.anaconda.org/conda-forge/linux-64/openssl-3.2.0-h1234.conda",
          "digest": { "sha256": "..." }
        }
      ]
    },
    "runDetails": {
      "builder": {
        "id": "https://github.com/prefix-dev/rattler-build-action@v0.2.34"
      },
      "metadata": {
        "invocationId": "https://github.com/org/repo/actions/runs/12345",
        "startedOn": "2026-03-01T10:00:00Z",
        "finishedOn": "2026-03-01T10:05:00Z"
      }
    }
  }
}
```

Key points:
- `buildType` is a URI controlled by the build tool (rattler-build would define
  its own)
- `externalParameters` captures what the *user* specified (recipe, variants,
  channels)
- `internalParameters` captures build tool details
- `resolvedDependencies` lists ALL inputs (source code, build deps, host deps)
  with digests
- `runDetails.builder.id` identifies the build platform

---

## The in-toto Attestation Framework

[in-toto](https://github.com/in-toto/attestation) is the underlying framework
that both SLSA provenance and GitHub attestations are built on. It provides a
layered architecture:

```
┌─────────────────────────────────────┐
│  Bundle (groups multiple attests)   │
├─────────────────────────────────────┤
│  Envelope (DSSE — authentication)   │
├─────────────────────────────────────┤
│  Statement (subject + predicate)    │
├─────────────────────────────────────┤
│  Predicate (type-specific metadata) │
└─────────────────────────────────────┘
```

### Predicate types relevant to rattler-build

| Predicate Type | URI | Use Case |
|---|---|---|
| **SLSA Provenance** | `https://slsa.dev/provenance/v1` | Build provenance (who built it, from what) |
| **SPDX** | `https://spdx.dev/Document` | SBOM as an attestation |
| **SCAI** | `https://in-toto.io/attestation/scai/attribute-report/v0.2` | Granular build attributes |
| **Test Result** | `https://in-toto.io/attestation/test-result/v0.1` | Test results |
| **Vuln Scan** | `https://in-toto.io/attestation/vulns/v0.1` | Vulnerability scan results |
| **VSA** | `https://slsa.dev/verification_summary/v1` | Verification Summary Attestation |
| **Conda Publish** | `https://schemas.conda.org/attestations-publish-1.schema.json` | Current conda attestation (CEP-27) |

**Key insight**: in-toto is designed to be **extensible**. rattler-build can:
1. Continue using the Conda-specific predicate for channel publishing
2. **Also** generate a standard SLSA provenance predicate for interoperability
3. **Also** attach an SBOM predicate (SPDX or CycloneDX)
4. Bundle all of these together in a single attestation bundle

This means a single rattler-build package could carry:
- A Conda publish attestation (for prefix.dev / conda channel verification)
- A SLSA provenance attestation (for enterprise SLSA compliance)
- An SBOM attestation (for dependency tracking / vulnerability management)

---

## GitHub Actions Attestation Infrastructure

GitHub provides first-class infrastructure for generating and storing
attestations, and rattler-build can hook into it.

### How it works

1. **`actions/attest`** — The core GitHub Action for creating attestations
   - Creates an in-toto statement with any predicate type
   - Signs it using Sigstore with the GitHub OIDC token
   - Stores the attestation in the GitHub Attestation API
   - For public repos: also logs to Sigstore's public transparency log
   - For private repos (Enterprise Cloud): uses GitHub's own Sigstore instance
   - Note: `actions/attest-build-provenance` (v4+) is now just a wrapper
     around `actions/attest`

1. **`actions/attest-sbom`** — Dedicated action for SBOM attestations
   - Accepts an external SPDX or CycloneDX JSON file
   - Creates a signed in-toto attestation with the SBOM as predicate
   - This is the key integration point: rattler-build generates the SBOM,
     `actions/attest-sbom` signs and stores it

2. **SLSA Build Levels achievable on GitHub Actions**:
   - **L2**: Use `actions/attest` directly in your build job ← rattler-build can
     do this today
   - **L3**: Use a **reusable workflow** that isolates the signing from the build
     ← rattler-build-action could become this

3. **`gh attestation verify`** — CLI verification tool
   - Already works with any in-toto attestation stored in GitHub

### Build environment info available on GitHub Actions

GitHub Actions exposes extensive environment information that rattler-build could
capture:

```yaml
# Available via environment variables:
GITHUB_REPOSITORY        # org/repo
GITHUB_SHA               # commit hash
GITHUB_REF               # branch/tag
GITHUB_WORKFLOW          # workflow name
GITHUB_RUN_ID            # unique run ID
GITHUB_RUN_ATTEMPT       # retry number
RUNNER_OS                # Linux, Windows, macOS
RUNNER_ARCH              # X64, ARM64
RUNNER_ENVIRONMENT       # github-hosted or self-hosted
ImageOS                  # e.g., ubuntu22
ImageVersion             # e.g., 20240225.1

# Available via OIDC token claims:
job_workflow_ref         # exact workflow file + ref
runner_environment       # github-hosted vs self-hosted
repository_visibility    # public, private, internal
```

### SLSA BYOB (Bring Your Own Builder) Framework

The [SLSA BYOB framework](https://slsa.dev/blog/2023/08/bring-your-own-builder-github)
allows third-party build tools like rattler-build to generate SLSA L3
provenance on GitHub Actions:

- The framework handles signing key management, build isolation, and attestation
  creation
- Build tool authors only need to define their `buildType` and parameters
- Users get SLSA L3 provenance without trusting the build tool's signing
  infrastructure
- Provenance can be verified with `slsa-verifier`

This is the most promising path for rattler-build to offer SLSA L3 on GitHub
Actions.

### What rattler-build can export from GitHub Actions

| Data | How to get it | Use in attestation |
|------|---------------|-------------------|
| Repository + commit | `GITHUB_REPOSITORY`, `GITHUB_SHA` | Source identity |
| Workflow identity | OIDC token `job_workflow_ref` | Builder identity |
| Runner details | `RUNNER_OS`, `RUNNER_ARCH`, `ImageOS` | Build environment |
| Run metadata | `GITHUB_RUN_ID`, timestamps | Invocation tracking |
| Build inputs | Recipe file, variant config | External parameters |
| Resolved deps | rattler's solver output | Resolved dependencies |
| Package outputs | `.conda` files + SHA256 | Subjects |

---

## Gap Analysis: What Enterprise Customers Want

Based on industry trends and regulatory requirements (US Executive Order 14028,
EU Cyber Resilience Act):

| Requirement | Current State | Gap |
|---|---|---|
| **SBOM for every package** | `index.json` has deps, `paths.json` has file hashes | No standard SBOM format (SPDX/CDX) |
| **SLSA provenance** | Conda-specific attestation via Sigstore | Not standard SLSA provenance format |
| **SLSA L3** | L2 achievable today | Need reusable workflow or BYOB |
| **Full dependency graph** | Only direct runtime deps | Missing transitive deps, build/host deps |
| **Build environment details** | Not captured | Need compiler versions, OS info, tool versions |
| **Reproducible builds** | Rendered recipe stored | Need full environment lockfile |
| **VEX data** | Not supported | Need vulnerability status for deps |
| **Source provenance** | Experimental source attestation | Need to graduate from experimental |
| **Policy enforcement** | Manual verification | Need machine-readable policy (VSA) |
| **Audit trail** | Sigstore transparency log | Good — but only for prefix.dev uploads |

---

## Integration Proposals for rattler-build

### Proposal 1: SBOM Generation (`rattler-build sbom`)

Add a new subcommand or flag to generate SBOMs:

```bash
# Generate SBOM for a built package
rattler-build sbom ./output/my-package-1.0.0-h1234_0.conda --format cyclonedx

# Generate SBOM during build
rattler-build build -r recipe.yaml --sbom cyclonedx

# Generate SBOM for an environment (all packages)
rattler-build sbom --environment /path/to/env --format spdx
```

**What the SBOM would contain:**
- Package identity with PURL (`pkg:conda/conda-forge/numpy@1.26.0?subdir=linux-64`)
- All runtime dependencies with versions and hashes
- Build and host dependencies (optionally)
- Source URLs and checksums
- License information (already SPDX-validated)
- Build tool identity (rattler-build version)
- For CycloneDX: Formulation section with the rendered recipe

**Implementation approach:**
- Use the [`cyclonedx-rust-cargo`](https://github.com/CycloneDX/cyclonedx-rust-cargo)
  or [`cyclonedx-bom`](https://crates.io/crates/cyclonedx-bom) Rust crate for
  CycloneDX output
- Use [`spdx-rs`](https://crates.io/crates/spdx-rs) for SPDX output
- Store the SBOM in `info/sbom.cdx.json` or `info/sbom.spdx.json` inside the
  package

### Proposal 2: SLSA Provenance Predicate

In addition to the existing Conda-specific attestation, generate a standard SLSA
provenance predicate:

```bash
# Build with SLSA provenance
rattler-build build -r recipe.yaml --slsa-provenance

# Publish with both Conda attestation + SLSA provenance
rattler-build publish ./recipe.yaml \
  --to https://prefix.dev/my-channel \
  --generate-attestation \
  --slsa-provenance
```

**What it would capture:**
- `buildType`: `https://prefix.dev/rattler-build/v1`
- `externalParameters`: recipe file, variant config, channels, target platform
- `internalParameters`: rattler-build version, archive type, compression level
- `resolvedDependencies`: all source URLs + hashes, all build/host/run
  dependencies with exact versions and hashes
- `builder.id`: the CI workflow identity
- `metadata`: invocation ID, timestamps

**This would make rattler-build packages verifiable with `slsa-verifier` and
compatible with any SLSA policy engine.**

### Proposal 3: Build Environment Capture

Automatically collect and store build environment details:

```json
{
  "rattler_build_version": "0.35.0",
  "platform": "linux-64",
  "build_os": "Ubuntu 22.04.3 LTS",
  "build_arch": "x86_64",
  "compilers": {
    "c": {"name": "gcc", "version": "12.3.0"},
    "cxx": {"name": "g++", "version": "12.3.0"},
    "fortran": {"name": "gfortran", "version": "12.3.0"}
  },
  "ci_environment": {
    "provider": "github-actions",
    "runner_os": "Linux",
    "runner_arch": "X64",
    "runner_environment": "github-hosted",
    "image_os": "ubuntu22",
    "image_version": "20240225.1",
    "workflow_ref": "org/repo/.github/workflows/build.yml@refs/heads/main",
    "run_id": "12345678",
    "run_attempt": "1"
  },
  "timestamp": "2026-03-01T10:00:00Z"
}
```

This could be stored as `info/build_environment.json` in the package and/or
included in the SLSA provenance `internalParameters`.

### Proposal 4: SLSA L3 via GitHub Actions Reusable Workflow

Create a reusable workflow that:
1. Accepts recipe path and build configuration as inputs
2. Runs the build in an isolated job
3. Generates SLSA provenance using `actions/attest` or the SLSA BYOB framework
4. Publishes with both Conda attestation and SLSA provenance

```yaml
# User's workflow
jobs:
  build:
    uses: prefix-dev/rattler-build-action/.github/workflows/build-and-attest.yml@v1
    with:
      recipe: recipe.yaml
      channel: https://prefix.dev/my-channel
    permissions:
      id-token: write
      contents: read
      attestations: write
```

This gives users SLSA L3 "for free" by using rattler-build's official reusable
workflow.

### Proposal 5: Attestation Bundle

Bundle multiple attestations together for a single package:

```
my-package-1.0.0-h1234_0.conda.intoto.jsonl
├── Conda publish attestation (CEP-27)
├── SLSA provenance attestation
├── SBOM attestation (CycloneDX or SPDX as in-toto predicate)
└── (optional) Test result attestation
```

The in-toto Bundle format supports exactly this use case — multiple attestations
for the same subject.

---

## Implementation Roadmap

### Phase 1: SBOM Generation (Highest Enterprise Value)

**Goal**: `rattler-build build --sbom cyclonedx` produces a CycloneDX SBOM

1. Add CycloneDX JSON output using the `cyclonedx-bom` Rust crate
2. Populate from existing package metadata (`index.json`, `paths.json`,
   `about.json`, rendered recipe)
3. Include PURL identifiers for conda packages
4. Store as `info/sbom.cdx.json` inside the `.conda` package
5. Add `--sbom spdx` output using `spdx-rs` crate
6. Add `rattler-build sbom` subcommand for generating SBOMs from existing
   packages

### Phase 2: SLSA Provenance Predicate

**Goal**: Standard SLSA provenance alongside existing Conda attestation

1. Define `https://prefix.dev/rattler-build/v1` build type specification
2. Implement SLSA provenance predicate generation
3. Capture build environment details (CI env vars, compiler versions)
4. Add `--slsa-provenance` flag to build and publish commands
5. Store provenance as `info/provenance.slsa.json`
6. Test verification with `slsa-verifier`

### Phase 3: GitHub Actions L3 Integration

**Goal**: Official reusable workflow for SLSA L3

1. Create `prefix-dev/rattler-build-action` reusable workflow
2. Integrate with GitHub's `actions/attest` for signing
3. Evaluate SLSA BYOB framework integration
4. Document the L3 setup for users

### Phase 4: Advanced Features

**Goal**: Full supply chain security suite

1. Attestation bundles (multiple predicates per package)
2. VEX generation for known vulnerabilities in dependencies
3. Policy engine integration (e.g., OPA/Rego policies for SLSA verification)
4. `rattler-build verify` command for offline verification
5. Graduate source attestation verification from experimental

---

## References

### SLSA
- [SLSA Specification v1.0](https://slsa.dev/spec/v1.0/)
- [SLSA Security Levels](https://slsa.dev/spec/v1.0/levels)
- [SLSA Provenance](https://slsa.dev/spec/v1.0/provenance)
- [SLSA BYOB Framework](https://slsa.dev/blog/2023/08/bring-your-own-builder-github)

### SBOM Standards
- [CycloneDX Specification](https://cyclonedx.org/specification/overview/)
- [SPDX Specification](https://spdx.github.io/spdx-spec/v2.3/)
- [SBOM Generation Tools Compared (2026)](https://sbomify.com/2026/01/26/sbom-generation-tools-comparison/)
- [Best SBOM Tools 2025 (Kusari)](https://www.kusari.dev/blog/best-sbom-tools-2025)

### in-toto
- [in-toto Attestation Framework](https://github.com/in-toto/attestation)
- [in-toto and SLSA relationship](https://slsa.dev/blog/2023/05/in-toto-and-slsa)
- [SPDX predicate type](https://github.com/in-toto/attestation/blob/main/spec/predicates/spdx.md)

### GitHub
- [GitHub Artifact Attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)
- [Reach SLSA L3 with GitHub Artifact Attestations](https://github.blog/enterprise-software/devsecops/enhance-build-security-and-reach-slsa-level-3-with-github-artifact-attestations/)
- [actions/attest-build-provenance](https://github.com/actions/attest-build-provenance)
- [SLSA L3 with reusable workflows](https://docs.github.com/actions/security-guides/using-artifact-attestations-and-reusable-workflows-to-achieve-slsa-v1-build-level-3)

### Conda Ecosystem
- [CEP-27: Conda Attestations](https://conda.org/learn/ceps/cep-0027)
- [Securing the Conda Supply Chain with Sigstore](https://prefix.dev/blog/securing-the-conda-package-supply-chain-with-sigstore)
- [Anaconda SBOM](https://www.anaconda.com/docs/psm/on-prem/6.8.0/user/sbom)
- [CycloneDX Python (historical conda support)](https://github.com/CycloneDX/cyclonedx-python)

### Regulatory
- [US Executive Order 14028](https://www.whitehouse.gov/briefing-room/presidential-actions/2021/05/12/executive-order-on-improving-the-nations-cybersecurity/)
- [CISA SBOM](https://www.cisa.gov/sbom)
- [EU Cyber Resilience Act](https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act)
