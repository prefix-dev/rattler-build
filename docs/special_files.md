# Activation scripts and other special files

A `conda` package can contain "special" files in the prefix. These files are scripts that are executed during activation, installation, or uninstallation process.

If possible, they should be avoided since they execute arbitrary code at installation time and slow down the installation and activation process.

## Activation scripts

The activation scripts are executed when the environment containing the package is activated (e.g. when doing `micromamba activate myenv` or `pixi run ...`).

The scripts are located in special folders:

- `etc/conda/activate.d/{script.sh/bat}` - scripts in this folder are executed before the environment is activated
- `etc/conda/deactivate.d/{script.sh/bat}` - scripts in this folder are executed when the environment is deactivated

The scripts are executed in lexicographical order, so you can prefix them with numbers to control the order of execution.

To add a script to the package, just make sure that you install the file in this folder. For
example, on Linux:

```sh
mkdir -p $PREFIX/etc/conda/activate.d
cp activate-mypkg.sh $PREFIX/etc/conda/activate.d/10-activate-mypkg.sh

mkdir -p $PREFIX/etc/conda/deactivate.d
cp deactivate-mypkg.sh $PREFIX/etc/conda/deactivate.d/10-deactivate-mypkg.sh
```

## Post-link and pre-unlink scripts

The `post-link` and `pre-unlink` scripts are executed when the package is installed or uninstalled. They are both heavily discouraged but implemented for compatibility with conda in `rattler-build` since version 0.17.

For a `post-link` script to be executed when a package is installed, the built package needs to have a `.<package_name>-post-link.{sh/bat}` in its `bin/` folder. The same is applicable for `pre-unlink` scripts, just with the name `.<package_name>-pre-unlink.{sh/bat}` (note the leading period). For example, for a package `mypkg`, you would need to have a `.mypkg-post-link.sh` in its `bin/` folder.

To make sure the scripts are included in the correct location, use your recipe's [build script or `build/script` key](build_script.md). For example, assuming you have a `post-link.sh` script in your source, alongside the recipe in the recipe's folder, the following configuration will copy it correctly:

```yaml
build:
  ...
  script:
    - ...
    - mkdir -p $PREFIX/bin
    - cp $RECIPE_DIR/post-link.sh $PREFIX/bin/.mypkg-post-link.sh
    - chmod +x $PREFIX/bin/.mypkg-post-link.sh
```

The `$PREFIX` and `$RECIPE_DIR` environment variables will be [set during the build process](build_script.md#environment-variables) to help you specify the correct paths.
