# Activation scripts and other special files

A conda package can contain "special" files in the prefix. These files are scripts that are executed during activation, installation or uninstallation process.

If possible, they should be avoided since they execute arbitrary code at installation time and slow down the installation and activation process.

## Activation scripts

The activation scripts are executed when the environment containing the package is activated (e.g. when doing `micromamba activate myenv` or `pixi run ...`).

The scripts are located in special folders:

- `etc/conda/activate.d/{script.sh/bat}` - scripts in this folder are executed before the environment is activated.
- `etc/conda/deactivate.d/{script.sh/bat}` - scripts in this folder are executed when the environment is deactivated.

The scripts are executed in lexicographical order, so you can prefix them with numbers to control the order of execution.

To add a script to the package, just make sure that you install the file in this folder, e.g. on Linux:

```sh
mkdir -p $PREFIX/etc/conda/activate.d
cp activate-mypkg.sh $PREFIX/etc/conda/activate.d/10-activate-mypkg.sh

mkdir -p $PREFIX/etc/conda/deactivate.d
cp deactivate-mypkg.sh $PREFIX/etc/conda/deactivate.d/10-deactivate-mypkg.sh
```

## Post-link and pre-unlink scripts

The post-link and pre-unlink scripts are executed when the package is installed or uninstalled.
They are both heavily discouraged and currently not implemented in `rattler`, `rattler-build` and `pixi`.

To create a `post-link` script for your package, you need to add `<package_name>-post-link.{sh/bat}` to the `bin/` folder of your package.
The same for `pre-unlink` scripts, just with the name `<package_name>-pre-unlink.{sh/bat}`.

For example, for `mypkg`, you would add `mypkg-post-link.sh` to the `bin/` folder of your package.