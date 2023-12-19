# Jinja functions that can be used in the recipe

`rattler-build` comes with a couple of useful helpers that can be used in the recipe.

- `${{ compiler('c') }}`
- `${{ pin_subpackage("mypkg", min_pin="x.x", max_pin="x.x.x") }}` creates a pin to another output in the recipe.

<!-- `pin_compatible` (not implemented yet). -->

- `${{ hash }}` this is the variant hash and is useful in the build string computation
- `${{ python | version_to_buildstring }}` converts a version from the variant to a build string (removes `.` and takes only the first two elements of the version).
- default jinja filters are available: `lower`, `upper`, indexing into characters: `https://myurl.com/{{ name[0] }}/{{ name | lower }}_${{ version }}.tar.gz`.
  A list of all the builtin filters can be found under: [Link](https://docs.rs/minijinja/latest/minijinja/filters/index.html#functions)
