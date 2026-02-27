# Packaging a Perl (CPAM) package

Packaging a Perl package is similar to packaging a Python package!

## Building a Perl Package

### A perl `noarch: generic` package

The following recipe is for the Perl package `Call::Context`. We use `perl` in the `host` requirements, and install the package using `make`.
The `noarch: generic` is used to indicate that the package is architecture-independent - since this is a pure Perl package, it can be installed and run on any platform (`noarch`).

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/perl-call-context.yaml"
```

### A perl package with a C extension

Some `perl` packages have native code extensions. In this example, we will build a package for the Perl package `Data::Dumper` using the `C` compiler.
The `c` compiler and `make` are required at build time in the `build` requirements to compile the native code extension.
We use `perl` in the `host` requirements, and install the package using `make`.

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/perl-data-dumper.yaml"
```
