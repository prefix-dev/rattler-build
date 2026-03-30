<h1>
  <a href="https://prefix.dev/tools/rattler-build">
    <img alt="banner" src="https://github.com/user-attachments/assets/456f8ef1-1c7b-463d-ad88-de3496b05db2">
  </a>
</h1>

# rattler_build_variant_config

Variant configuration system for rattler-build, handling build matrices, zip keys, and loading of variant YAML files.

This crate provides functionality for:

- Loading variant configurations from YAML files (`variants.yaml`)
- Loading legacy `conda_build_config.yaml` files with selector support
- Computing all possible variant combinations (build matrices)
- Handling "zip keys" to synchronize related variants
