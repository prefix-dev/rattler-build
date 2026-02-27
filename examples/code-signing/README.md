# Code Signing Example

This example shows how to build a small C project as a conda package with
code-signed binaries on macOS and Windows.

## What's included

```
code-signing/
├── recipe.yaml                          # rattler-build recipe with signing config
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
   (`libgreet`) using CMake.
2. The `build.signing` section tells rattler-build to sign all detected
   binaries after relinking but before creating the `.conda` archive.
3. The GitHub Actions workflow imports signing certificates and passes
   them to rattler-build via environment variables (read by Jinja templates
   in the recipe).

## Using in your own repo

1. Copy the contents of this directory into your repository.
2. Configure GitHub secrets (see the workflow file for the list).
3. Push -- the workflow builds and signs on macOS and Windows.

For Azure Trusted Signing instead of a local `.pfx` certificate, change the
recipe's `windows` section:

```yaml
signing:
  windows:
    azure_trusted_signing:
      endpoint: "${{ env.AZURE_SIGNING_ENDPOINT }}"
      account_name: "${{ env.AZURE_SIGNING_ACCOUNT }}"
      certificate_profile: "${{ env.AZURE_SIGNING_PROFILE }}"
    timestamp_url: "http://timestamp.acs.microsoft.com"
```

See the [code signing documentation](https://rattler-build.prefix.dev/latest/code_signing/)
for full details.
