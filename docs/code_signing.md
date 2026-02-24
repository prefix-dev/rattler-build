# Code signing

Code signing lets you cryptographically sign native binaries (executables, shared
libraries) inside your conda packages. This is separate from
[Sigstore attestations](sigstore.md), which sign the _package archive_ itself
-- code signing signs the individual _binaries_ so that operating systems trust
them at runtime.

Rattler-build supports code signing for **macOS** (via `codesign`) and
**Windows** (via `signtool` or Azure Trusted Signing). Signing is configured in
the `build.signing` section of your recipe.

## Why sign binaries in a conda package?

Conda packages undergo _prefix replacement_ at install time -- the build-time
prefix path is rewritten to the install-time prefix. This byte-level
modification invalidates any code signature that was applied during the build
script. Rattler-build solves this by signing **after** relinking and prefix
detection, but **before** the package archive is created.

!!! warning "Signed binaries must not contain the build prefix"
    If a signed binary still contains the literal build prefix, conda's prefix
    replacement will corrupt the signature at install time. Rattler-build checks
    for this automatically and will error if it detects the build prefix in any
    signed binary. To resolve this, ensure your build process does not embed
    absolute paths into binaries, or use `build.prefix_detection` to exclude
    those files from prefix replacement.

## Pipeline order

The signing step runs at a specific point in the packaging pipeline:

```
build script
  → file collection
    → relinking (rpath fixups, ad-hoc codesign on macOS)
      → prefix detection
        → regex post-processing
          → **code signing** ← signs here
            → metadata generation
              → archive creation (.conda)
```

Because relinking invalidates existing signatures, rattler-build first applies
an ad-hoc signature (`codesign -s -`) on macOS during relinking, then overwrites
it with the real identity during the signing step using `--force`.

## macOS signing

macOS signing uses Apple's `codesign` tool. You must provide a signing identity,
which is typically a Developer ID certificate installed in a keychain.

### Basic configuration

```yaml title="recipe.yaml"
build:
  signing:
    macos:
      identity: "Developer ID Application: My Company (TEAMID)"
```

### Full configuration

```yaml title="recipe.yaml"
build:
  signing:
    macos:
      # Required: signing identity (use "-" for ad-hoc signing)
      identity: "Developer ID Application: My Company (TEAMID)"
      # Optional: path to a specific keychain
      keychain: "/path/to/signing.keychain-db"
      # Optional: entitlements plist file
      entitlements: "entitlements.plist"
      # Optional: additional codesign options
      options:
        - runtime  # enables hardened runtime
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `identity` | Yes | The signing identity string. Use a Developer ID certificate name, or `"-"` for ad-hoc signing. Supports Jinja templates (`${{ env.IDENTITY }}`). |
| `keychain` | No | Path to the keychain containing the certificate. If omitted, the default keychain search path is used. |
| `entitlements` | No | Path to an entitlements plist file. Required for some app sandbox or hardened runtime entitlements. |
| `options` | No | List of additional `--options` flags passed to `codesign` (e.g., `runtime` for hardened runtime). |

### Using Jinja templates

You can use Jinja templates to pull signing configuration from environment
variables, which is useful for CI/CD:

```yaml title="recipe.yaml"
build:
  signing:
    macos:
      identity: "${{ env.MACOS_SIGNING_IDENTITY }}"
      keychain: "${{ env.MACOS_KEYCHAIN_PATH }}"
```

## Windows signing

Windows signing supports two methods:

1. **Local certificate** (`signtool`) -- uses a `.pfx` / `.p12` certificate file
2. **Azure Trusted Signing** -- uses Microsoft's cloud-based signing service

Exactly one method must be configured. Shared settings (`timestamp_url`,
`digest_algorithm`) are specified at the `windows` level.

### Method 1: Local certificate (signtool)

Use this when you have a code signing certificate file (`.pfx` or `.p12`):

```yaml title="recipe.yaml"
build:
  signing:
    windows:
      signtool:
        certificate_file: "path/to/certificate.pfx"
        certificate_password: "${{ env.CERT_PASSWORD }}"
      timestamp_url: "http://timestamp.digicert.com"
      digest_algorithm: sha256
```

#### Signtool fields

| Field | Required | Description |
|-------|----------|-------------|
| `certificate_file` | Yes | Path to the `.pfx` / `.p12` certificate file. |
| `certificate_password` | No | Password for the certificate file. Use a Jinja template to read from an environment variable. |

### Method 2: Azure Trusted Signing

[Azure Trusted Signing](https://learn.microsoft.com/en-us/azure/trusted-signing/)
is a cloud-based signing service that eliminates the need to manage certificate
files locally. It requires Azure authentication (typically via `az login` or
OIDC in CI).

```yaml title="recipe.yaml"
build:
  signing:
    windows:
      azure_trusted_signing:
        endpoint: "${{ env.AZURE_SIGNING_ENDPOINT }}"
        account_name: "${{ env.AZURE_SIGNING_ACCOUNT }}"
        certificate_profile: "${{ env.AZURE_SIGNING_PROFILE }}"
      timestamp_url: "http://timestamp.acs.microsoft.com"
```

#### Azure Trusted Signing fields

| Field | Required | Description |
|-------|----------|-------------|
| `endpoint` | Yes | The Azure Trusted Signing endpoint URL (e.g., `https://wus2.codesigning.azure.net`). |
| `account_name` | Yes | The Azure Trusted Signing account name. |
| `certificate_profile` | Yes | The certificate profile name to use for signing. |

### Shared Windows fields

These fields apply regardless of which signing method is used:

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `timestamp_url` | No | — | RFC 3161 timestamp server URL. Strongly recommended for production builds. |
| `digest_algorithm` | No | `sha256` | The digest algorithm to use (e.g., `sha256`, `sha384`, `sha512`). |

!!! tip "Always use a timestamp server"
    Without a timestamp, signatures become invalid when the signing certificate
    expires. A timestamp cryptographically proves the signature was created while
    the certificate was still valid.

## Cross-platform configuration

You can configure signing for both platforms in a single recipe. Rattler-build
automatically selects the relevant configuration based on the target platform:

```yaml title="recipe.yaml"
build:
  signing:
    macos:
      identity: "${{ env.MACOS_SIGNING_IDENTITY }}"
      options:
        - runtime
    windows:
      signtool:
        certificate_file: "${{ env.WIN_CERT_PATH }}"
        certificate_password: "${{ env.WIN_CERT_PASSWORD }}"
      timestamp_url: "http://timestamp.digicert.com"
```

When building for macOS, only the `macos` section is used. When building for
Windows, only the `windows` section is used. On Linux, signing is skipped
entirely.

## CI/CD examples

### GitHub Actions (macOS)

```yaml title=".github/workflows/build.yml"
jobs:
  build-macos:
    runs-on: macos-latest
    env:
      MACOS_SIGNING_IDENTITY: "Developer ID Application: My Org (TEAMID)"
    steps:
      - uses: actions/checkout@v4

      # Import the certificate into a temporary keychain
      - name: Import certificate
        env:
          CERTIFICATE_P12: ${{ secrets.MACOS_CERTIFICATE_P12 }}
          CERTIFICATE_PASSWORD: ${{ secrets.MACOS_CERTIFICATE_PASSWORD }}
        run: |
          KEYCHAIN_PATH=$RUNNER_TEMP/signing.keychain-db
          KEYCHAIN_PASSWORD=$(openssl rand -base64 32)

          # Create and configure keychain
          security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
          security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
          security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"

          # Import certificate
          echo "$CERTIFICATE_P12" | base64 --decode > $RUNNER_TEMP/cert.p12
          security import $RUNNER_TEMP/cert.p12 \
            -k "$KEYCHAIN_PATH" -P "$CERTIFICATE_PASSWORD" \
            -T /usr/bin/codesign
          security set-key-partition-list -S apple-tool:,apple: \
            -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
          security list-keychains -d user -s "$KEYCHAIN_PATH"

      - name: Build package
        run: rattler-build build --recipe recipe.yaml
```

With this `recipe.yaml`:

```yaml title="recipe.yaml"
build:
  signing:
    macos:
      identity: "${{ env.MACOS_SIGNING_IDENTITY }}"
      options:
        - runtime
```

### GitHub Actions (Windows with Azure Trusted Signing)

```yaml title=".github/workflows/build.yml"
jobs:
  build-windows:
    runs-on: windows-latest
    permissions:
      id-token: write
    steps:
      - uses: actions/checkout@v4

      - name: Azure login
        uses: azure/login@v2
        with:
          client-id: ${{ secrets.AZURE_CLIENT_ID }}
          tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}

      - name: Build package
        env:
          AZURE_SIGNING_ENDPOINT: ${{ secrets.AZURE_SIGNING_ENDPOINT }}
          AZURE_SIGNING_ACCOUNT: ${{ secrets.AZURE_SIGNING_ACCOUNT }}
          AZURE_SIGNING_PROFILE: ${{ secrets.AZURE_SIGNING_PROFILE }}
        run: rattler-build build --recipe recipe.yaml
```

With this `recipe.yaml`:

```yaml title="recipe.yaml"
build:
  signing:
    windows:
      azure_trusted_signing:
        endpoint: "${{ env.AZURE_SIGNING_ENDPOINT }}"
        account_name: "${{ env.AZURE_SIGNING_ACCOUNT }}"
        certificate_profile: "${{ env.AZURE_SIGNING_PROFILE }}"
      timestamp_url: "http://timestamp.acs.microsoft.com"
```

## Which files are signed?

Rattler-build automatically detects signable binaries by inspecting file headers:

| Platform | Detected file types |
|----------|-------------------|
| macOS | Mach-O executables and dynamic libraries (`.dylib`) |
| Windows | PE executables (`.exe`) and dynamic libraries (`.dll`) |

Files that do not match these formats are silently skipped. After signing, each
binary's signature is verified to ensure it was applied correctly.

## Troubleshooting

### "Signed binary contains build prefix"

This error means a binary that was signed still contains the build-time prefix
path. Since conda replaces this path at install time, the signature would be
corrupted. To fix this:

- Ensure your build does not hardcode absolute paths into binaries
- Use relative paths or runtime path resolution instead
- If the file does not need prefix replacement, configure
  `build.prefix_detection.ignore` to skip it

### "codesign: no identity found"

The signing identity was not found in any accessible keychain. Check that:

- The certificate is imported into a keychain
- The keychain is unlocked and in the search path
- The identity string matches the certificate's Common Name exactly

### "signtool: certificate not found"

The certificate file path is incorrect or the password is wrong. Verify that:

- The `certificate_file` path is correct relative to the build working directory
- The `certificate_password` is set correctly (check Jinja template evaluation)

## See also

- [Sigstore attestations](sigstore.md) -- sign the package archive for supply-chain provenance
- [Advanced build options](build_options.md) -- other `build:` configuration
- [Prefix detection](build_options.md#prefix-replacement) -- control which files undergo prefix replacement
