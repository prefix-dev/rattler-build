# Build-step cache example

This example has two named steps and demonstrates both cache comparison modes:

- `generate` hashes `input.txt` and `generated.txt`.
- `summarize` compares the mtimes and sizes of `generated.txt` and `summary.txt`.

Run it from this directory:

```console
rattler-build run summarize --recipe . --source-dir . --experimental
```

The first invocation executes `generate -> summarize`. Run the same command
again and both steps report a cache hit:

```text
Skipping build step generate (cache hit)
Skipping build step summarize (cache hit)
```

Now change the input and rerun:

```console
echo "new input" > input.txt
rattler-build run summarize --recipe . --source-dir . --experimental
```

`generate` reruns because its input hash changed. Its rewritten
`generated.txt` invalidates `summarize`, so that step reruns too. Deleting
`generated.txt` or `summary.txt` also forces the step that owns that output to
rerun.

Each step writes declarations to the path in `RATTLER_BUILD_STEP_CACHE`. For
example, `generate` writes:

```text
input-hash: input.txt
output-hash: generated.txt
```

The declaration stays human-writable. Rattler-build owns the neighboring
`.state.json` file containing the recorded fingerprints.
