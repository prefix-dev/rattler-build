# Experimental sandbox

!!! warning
    The sandbox feature is experimental and may not work as expected. The options might
    change in future releases.

Since the 0.34.0 release, `rattler-build` has a new experimental feature called
`sandbox`. With the sandbox feature enabled (via `--sandbox`), the build process
has much more restricted access to system resources on macOS and Linux.
As the sandbox feature is experimental it is disabled by default.

In particular, with the default configuration, the build process can read the entire filesystem,
but it cannot write outside of the build directories. The build process also cannot access the network.
In the future, we plan to enable the sandbox per default and restrict it further.

On macOS this is achieved by using the `sandbox-exec` command, which is part of the macOS system.
On Linux the sandbox is created using Linux namespaces.

To control the sandbox behavior, you can supply additional arguments to the CLI:

## Example

```bash
# run the build and sandbox the build process
rattler-build build --recipe ./example/recipe.yaml --sandbox

# to add more permissions to the sandbox
rattler-build build --recipe ./example/recipe.yaml --sandbox \
    --allow-read /some/path --allow-read /foo/bar --allow-network
```

## Options

- `--allow-network`: Allow network access (by default network access is disabled)
- `--allow-read-write /some/path`: Allow read and write access to the specified path (and all its subdirectories)
- `--allow-read /some/path`: Allow read access to the specified path (and all its subdirectories)
- `--allow-read-execute /some/path`: Allow read and execute access to the specified path (and all its subdirectories)
- `--overwrite-default-sandbox-config`: Ignore the default sandbox configuration and use only the supplied arguments

## Default sandbox configuration

### macOS

On macOS, by default, the sandbox configuration is as follows:

- Read access to the entire filesystem (`/`)
- Read and execute access to `/bin`, `/usr/bin`
- Write access to the build directories and `/tmp`, `/var/tmp`, and `$TMPDIR` (if defined)

### Linux

On Linux, by default, the sandbox configuration is as follows:

- Read access to the entire filesystem (`/`)
- Read and execute access to `/bin`, `/usr/bin`, `/lib`, `/usr/lib`, `/lib64`, `/usr/lib64`
- Write access to the build directories and `/tmp`, `/var/tmp`, and `$TMPDIR` (if defined)

### Windows

Sandboxing the build process is not yet supported on Windows, and thus all passed sandbox flags are entirely ignored.
