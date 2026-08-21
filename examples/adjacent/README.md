# Named build-step prototype

Clone Adjacent, then execute the recipe directly in that checkout (replace the
recipe path with the path to this repository):

```console
git clone https://github.com/Evil-Spirit/Adjacent.git
cd Adjacent
rattler-build run build --recipe /path/to/rattler-build/examples/adjacent --source-dir . --experimental
rattler-build run lint  --recipe /path/to/rattler-build/examples/adjacent --source-dir . --experimental
rattler-build run test  --recipe /path/to/rattler-build/examples/adjacent --source-dir . --experimental
```

`test` executes `configure -> build -> test`; `lint` executes only `lint`.
`SRC_DIR` and each step's default working directory are the Adjacent checkout,
so its `build/` CMake cache is reused. The `configure` and `build` steps also
write input/output declarations to `RATTLER_BUILD_STEP_CACHE`. Running `build`
a second time skips both processes entirely. Editing a file under `src/` or
`include/` reruns `build`, changing `CMakeLists.txt` reruns both steps, and
deleting `build/adjacent_test` reruns its producing step.

Build and host prefixes remain under deterministic
`output/bld/rattler-build_*` directories; the isolated lint solve gets a
separate prefix from the parent-based build/test solve. Dependencies listed
under a step's `requirements.build` and `requirements.host` extend the
corresponding solve group. The lint task sets
`requirements.inherit: false`, so only its `clang-format` requirement is
solved. Its command is loaded from the recipe-local reusable step file
`steps/lint.yaml`.

Without `--source-dir`, `run` uses the recipe's fetched source in the persistent
rattler-build work directory instead.
