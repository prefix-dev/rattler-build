# Code Signing Example

This example shows how to build a small C project as a conda package with
code-signed binaries on macOS and Windows, using the `--signing-config-file`
flag to keep signing config **external to the recipe**.

## What's included

```
code-signing/
├── recipe.yaml                          # rattler-build recipe (no signing config!)
├── signing-macos.yaml                   # Standalone macOS signing config
├── signing-windows.yaml                 # Standalone Windows signing config
├── hello/                               # Minimal C project (executable + shared lib)
│   ├── CMakeLists.txt
│   └── src/
│       ├── main.c
│       └── greet.c
├── .github/workflows/build-and-sign.yml # Example GitHub Actions workflow
└── README.md
```

## How it works

1. The `recipe.yaml` builds a C executable (`hello`) and shared library
   (`libgreet`) using CMake. It contains **no signing configuration**.
2. A separate YAML file (e.g. `signing-macos.yaml`) defines the signing
   identity, keychain, and options.
3. At build time, pass `--signing-config-file signing-macos.yaml` to
   rattler-build. It signs all detected binaries after relinking but before
   creating the `.conda` archive.

This separation means:
- The same recipe works on any platform, signed or unsigned
- Signing details stay in CI config, not in the recipe
- Local developers can build without needing certificates

## Quick start (local, unsigned)

```bash
rattler-build build --recipe recipe.yaml
```

## Quick start (macOS, signed)

```bash
rattler-build build \
  --recipe recipe.yaml \
  --signing-config-file signing-macos.yaml
```

## Using in your own repo

1. Copy the contents of this directory into your repository.
2. Configure GitHub secrets (see the workflow file for the list).
3. Push -- the workflow builds and signs on macOS and Windows.

For Azure Trusted Signing instead of a local `.pfx` certificate, use a
signing config like:

```yaml
windows:
  azure_trusted_signing:
    endpoint: "https://wus2.codesigning.azure.net"
    account_name: "my-account"
    certificate_profile: "my-profile"
  timestamp_url: "http://timestamp.acs.microsoft.com"
```

See the [code signing documentation](https://rattler-build.prefix.dev/latest/code_signing/)
for full details.
