# Repackaging existing software

It's totally possible to repackage existing software using rattler-build, and make it easy to install
with conda, mamba or pixi.

Repackaging existing binaries is not recommended on `conda-forge`, but totally acceptable for your own channels / repositories.

## Example for `linkerd`

This example shows how to repackage the `linkerd` binary. The `linkerd` binary is a command line tool that is used to manage and monitor Kubernetes clusters, and a pre-built binary is available for download from Github releases. Alternatively, you could also follow the Go packaging tutorial to build linkerd from source!

```yaml
package:
  name: linkerd
  version: 25.5.2

source:
  - if: target_platform == "linux-64"
    then:
      url: https://github.com/linkerd/linkerd2/releases/download/edge-25.5.2/linkerd2-cli-edge-25.5.2-linux-amd64
      sha256: 55e7721ab0eb48217f239628b55517b7d663a962df18cdab180e5d42e45f83cb
      file_name: linkerd
  - if: target_platform == "osx-arm64"
    then:
      url: https://github.com/linkerd/linkerd2/releases/download/edge-25.5.2/linkerd2-cli-edge-25.5.2-darwin-arm64
      sha256: 405ddf3af0089bfece93d811c9bfb9f63e3a000e3f423163fc56690ef4d427cf
      file_name: linkerd
  # To support other platforms you can add more `if` statements here

build:
  script:
    # make linkerd binary executable
    - chmod +x linkerd
    # make sure that the `$PREFIX/bin` directory exists
    - mkdir -p $PREFIX/bin
    # move or copy the binary to the `$PREFIX/bin` directory
    - mv linkerd $PREFIX/bin/

tests:
  - script:
      - linkerd version
      # you can add more tests here

about:
  homepage: https://linkerd.io/
  license: Apache-2.0
  summary: Linkerd is an ultralight service mesh for Kubernetes.
  description: |
    Linkerd is an ultralight service mesh for Kubernetes.
    It adds observability, reliability, and security to your
    applications without requiring any code changes.
    Linkerd is open source and free to use.
  # Note: since we are downloading a binary, we don't have a license file.
  # You can put the license in the recipe directory, and it will be picked up from there.
  license_file: LICENSE
  # documentation: ...
  repository: https://github.com/linkerd/linkerd2
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
