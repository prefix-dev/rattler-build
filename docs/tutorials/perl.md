# Packaging a Perl (CPAM) package

Packaging a Perl package is similar to packaging a Python package!

## Building a Perl Package

### A perl `noarch: generic` package

The following recipe is for the Perl package `Call::Context`. We use `perl` in the `host` requirements, and install the package using `make`.
The `noarch: generic` is used to indicate that the package is architecture-independent - since this is a pure Perl package, it can be installed and run on any platform (`noarch`).

```yaml title="recipe.yaml"
context:
  version: 0.03

package:
  name: perl-call-context
  version: ${{ version }}

source:
  url: https://cpan.metacpan.org/authors/id/F/FE/FELIPE/Call-Context-${{ version }}.tar.gz
  sha256: 0ee6bf46bc72755adb7a6b08e79d12e207de5f7809707b3c353b58cb2f0b5a26

build:
  number: 0
  noarch: generic
  script:
    - perl Makefile.PL INSTALLDIRS=vendor NO_PERLLOCAL=1 NO_PACKLIST=1
    - make
    - make test
    - make install

requirements:
  build:
    - make
  host:
    - perl

tests:
  - perl:
      uses:
        - Call::Context

about:
  license: GPL-1.0-or-later OR Artistic-1.0-Perl
  summary: Sanity-check calling context
  homepage: http://metacpan.org/pod/Call-Context
```

### A perl package with a C extension

Some `perl` packages have native code extensions. In this example, we will build a package for the Perl package `Data::Dumper` using the `C` compiler.
The `c` compiler and `make` are required at build time in the `build` requirements to compile the native code extension.
We use `perl` in the `host` requirements, and install the package using `make`.

```yaml title="recipe.yaml"
context:
  version: "2.183"

package:
  name: "perl-data-dumper"
  version: ${{ version }}

source:
  url: https://cpan.metacpan.org/authors/id/N/NW/NWCLARK/Data-Dumper-${{ version }}.tar.gz
  sha256: e42736890b7dae1b37818d9c5efa1f1fdc52dec04f446a33a4819bf1d4ab5ad3

build:
  number: 0
  script:
    - perl Makefile.PL INSTALLDIRS=vendor NO_PERLLOCAL=1 NO_PACKLIST=1
    - make
    - make test
    - make install VERBINST=1

requirements:
  build:
    - ${{ compiler('c') }}
    - make
  host:
    - perl
    - perl-extutils-makemaker

tests:
  - perl:
      uses:
        - Data::Dumper

about:
  homepage: https://metacpan.org/pod/Data::Dumper
  license: GPL-1.0-or-later OR Artistic-1.0-Perl
  summary: 'seeds germane, yet not germinated'
```
