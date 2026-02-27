# Repackaging existing software

It's totally possible to repackage existing software using rattler-build, and make it easy to install
with conda, mamba or pixi.

Repackaging existing binaries is not recommended on `conda-forge`, but totally acceptable for your own channels / repositories.

## Example for `linkerd`

This example shows how to repackage the `linkerd` binary. The `linkerd` binary is a command line tool that is used to manage and monitor Kubernetes clusters, and a pre-built binary is available for download from Github releases. Alternatively, you could also follow the Go packaging tutorial to build linkerd from source!

```yaml
--8<-- "docs/snippets/recipes/linkerd.yaml"
```

!!! note

    To repackage the `linkerd` package on `osx-arm64` for `linux-64`, you can pass the `--target-platform` argument to `rattler-build`:

    ```bash
    rattler-build build --target-platform linux-64 linkerd
    ```

## Adding system requirements

Some packages have system requirements (e.g. on `glibc` on Linux, or the macOS SDK on macOS).

You can add system requirements like this to the `run` section by depending on virtual packages:

```yaml
requirements:
  run:
    - ${{ "__glibc >=2.17" if linux }}
    - ${{ "__osx >=10.15" if osx }}
```
