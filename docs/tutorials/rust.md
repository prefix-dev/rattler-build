# Building a Rust package

We're using `rattler-build` to build a Rust package for the `cargo-edit` utility.
This utility manages Cargo dependencies from the command line.

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/cargo-edit.yaml"
```

!!! note
    The `${{ compiler(...) }}` functions are very useful in the context of cross-compilation.
    When the function is evaluated it will insert the correct compiler (as selected with the variant config) as well the `target_platform`.
    The "rendered" compiler will look like `rust_linux-64` when you are targeting the `linux-64` platform.

    You can read more about this in the [cross-compilation](../compilers.md) section.

1. The `cargo-bundle-licenses` utility is used to bundle all the licenses of the dependencies into a `THIRDPARTY.yml` file.
   This file is then included in the package. You should always include this file in your package when you are redistributing it.
2. Running scripts in `bash` or `cmd.exe` to test the package build well, expects an exit code of `0` to pass the test.


To build this recipe, simply run:

```bash
rattler-build build \
    --recipe ./cargo-edit/recipe.yaml
```
