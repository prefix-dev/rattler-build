# Jinja functions that can be used in the recipe

`rattler-build` comes with a couple of useful helpers that can be used in the recipe.

## Functions

### The compiler function

The compiler function can be used to put together a compiler that works for the current platform and the compilation "target_platform".
The syntax looks like: `${{ compiler('c') }}` where `'c'` signifies the programming language that is used.

This function evaluates to `<compiler>_<target_platform> <compiler_version>`.
For example, when compiling _on_ linux and to linux-64, this function evaluates to `gcc_linux-64`.

The values can be influenced by the `variant_configuration`.
The `<lang>_compiler` and `<lang>_compiler_version` variables are the keys with influence, for example:

```yaml
c_compiler:
- clang
c_compiler_version:
- 9.0
```

Would select the `clang` compiler in version `9.0`. Note that the final output will still contain the `target_platform`, so that the full compiler will read `clang_linux-64 9.0` when compiling with `--target-platform linux-64`.

## The `pin_subpackage` function

- `${{ pin_subpackage("mypkg", min_pin="x.x", max_pin="x.x.x") }}` creates a pin to another output in the recipe.

## The `pin_compatible` function

- `pin_compatible` pins a package in the run requirements based on the resolved package of the `host` or `build` section.

## The `cdt` function

- `${{ cdt("mypkg") }}` creates a cross dependency to another output in the recipe.

This function helps add Core Dependency Tree packages as dependencies by converting packages as required according to hard-coded logic.

```yaml
# on x86_64 system
cdt('package-name') # outputs: package-name-cos6-x86_64
# on aarch64 system
cdt('package-name') # outputs: package-name-cos6-aarch64
```

## The `hash` variable

- `${{ hash }}` this is the variant hash and is useful in the build string computation

## The `version_to_buildstring` function
- `${{ python | version_to_buildstring }}` converts a version from the variant to a build string (removes `.` and takes only the first two elements of the version).

## The `env` object

You can use the `env` object to retrieve environment variables and forward them to your build script. There are two ways to do this:

- `${{ env.get("MY_ENV_VAR") }}` will return the value of the environment variable `MY_ENV_VAR` or throw an error if it is not set.
- `${{ env.get_default("MY_ENV_VAR", "default_value") }}` will return the value of the environment variable `MY_ENV_VAR` or `"default_value"` if it is not set.

You can also check for existence of an environment variable:

- `${{ env.exists("MY_ENV_VAR") }}` will return `true` if the environment variable `MY_ENV_VAR` is set and `false` otherwise.

## Default Jinja filters

- default jinja filters are available: `lower`, `upper`, indexing into characters: `https://myurl.com/{{ name[0] }}/{{ name | lower }}_${{ version }}.tar.gz`.
  A list of all the builtin filters can be found under: [Link](https://docs.rs/minijinja/latest/minijinja/filters/index.html#functions)
