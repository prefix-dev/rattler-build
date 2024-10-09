# Compilers and cross-compilation

To use a compiler in your project, it's best to use the `${{ compiler('lang')
}}` template function. The compiler function works by taking a language,
determining the configured compiler for that language, and adding some
information about the target platform to the selected compiler. To configure a
compiler for a specific language, the `variant_config.yaml` file can be used.

For example, in a recipe that uses a C-compiler, you can use the following code:

```yaml
requirements:
  build:
    - ${{ compiler('c') }}
```

To set the compiler that you want to use, create a variant config that looks
like the following:

```yaml
c_compiler:
  - gcc

# optionally you can specify a version
c_compiler_version:
  - 9.3.0
```

When the template function is evaluated, it will look something like:
`gcc_linux-64 9.3.0`. You can define your own compilers. For example, for Rust
you can use `${{ compiler('rust') }}` and `rust_compiler_{version}` in your
variant config.

## Cross-compilation

Cross-compilation is supported by `rattler-build` and the compiler template
function is part of what makes it possible. When you want to cross-compile from
`linux-64` to `linux-aarch64` (i.e. intel to ARM), you can pass `--target-platform
linux-aarch64` to the `rattler-build` command. This will cause the compiler
template function to select a compiler that is configured for `linux-aarch64`.
The above example would resolve to `gcc_linux-aarch64 9.3.0`. Provided that the
package is available for `linux-64` (your build platform), the compilation
should succeed.

The distinction between the `build` and `host` sections begins to make sense when
thinking about cross-compilation. The `build` environment is resolved to
packages that need to _run_ at compilation time. For example, `cmake`, `gcc`,
and `autotools` are all tools that need to be executed. Therefore, the `build`
environment resolves to packages for the `linux-64` architecture (in our
example). On the other hand, the `host` packages resolve to `linux-aarch64` -
those are packages that we want to link against.

```yaml
# packages that need to run at build time (cmake, gcc, autotools, etc.)
# in the platform that rattler-build is executed on (the build_platform)
build:
  - cmake
  - ${{ compiler('c') }}
# packages that we want to link against in the architecture we are
# cross-compiling to the target_platform
host:
  - libcurl
  - openssl
```
