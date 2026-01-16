# Activation scripts and other special files

A `conda` package can contain "special" files in the prefix. These files are scripts that are executed during activation, installation, or uninstallation process.

If possible, they should be avoided since they execute arbitrary code at installation time and slow down the installation and activation process.

## Activation scripts

The activation scripts are executed when the environment containing the package is activated (e.g. when doing `micromamba activate myenv` or `pixi run ...`).

The scripts are located in special folders:

- `etc/conda/activate.d/{script.*}` - scripts in this folder are executed when the environment is activated
- `etc/conda/deactivate.d/{script.*}` - scripts in this folder are executed when the environment is deactivated

The scripts are executed in lexicographical order, so you can prefix them with numbers to control the order of execution.

### Shell-specific file extensions

Different shells require different script file extensions. The activation system will only execute scripts that match the current shell:

| Shell      | Extension |
|------------|-----------|
| Bash       | `.sh`     |
| Zsh        | `.sh`     |
| Fish       | `.fish`   |
| Xonsh      | `.xsh` or `.sh` |
| PowerShell | `.ps1`    |
| Cmd.exe    | `.bat`    |
| NuShell    | `.nu`     |

To add a script to the package, just make sure that you install the file in this folder. For
example, on Linux:

```sh
mkdir -p $PREFIX/etc/conda/activate.d
cp activate-mypkg.sh $PREFIX/etc/conda/activate.d/10-activate-mypkg.sh

mkdir -p $PREFIX/etc/conda/deactivate.d
cp deactivate-mypkg.sh $PREFIX/etc/conda/deactivate.d/10-deactivate-mypkg.sh
```

For cross-platform packages that need to support multiple shells, you should provide scripts for each shell type you want to support:

```sh
# Unix shells (Bash, Zsh)
mkdir -p $PREFIX/etc/conda/activate.d
cp activate-mypkg.sh $PREFIX/etc/conda/activate.d/10-activate-mypkg.sh

# Windows Cmd.exe
cp activate-mypkg.bat $PREFIX/etc/conda/activate.d/10-activate-mypkg.bat

# Windows PowerShell
cp activate-mypkg.ps1 $PREFIX/etc/conda/activate.d/10-activate-mypkg.ps1
```

## Activation environment variables

If you only need to set environment variables when an environment is activated (rather than running arbitrary shell code), you can use JSON files in the `etc/conda/env_vars.d/` directory. This is more efficient and portable than using activation scripts.

### Package-specific environment variables

To set environment variables from your package, create a JSON file in `etc/conda/env_vars.d/`:

```text
<prefix>/etc/conda/env_vars.d/<package_name>.json
```

The JSON file should contain a simple object with string key-value pairs:

```json
{
  "MY_PACKAGE_HOME": "/path/to/data",
  "MY_PACKAGE_CONFIG": "default",
  "SOME_API_ENDPOINT": "https://api.example.com"
}
```

To include this in your package, add to your build script:

```sh
mkdir -p $PREFIX/etc/conda/env_vars.d
cat > $PREFIX/etc/conda/env_vars.d/mypkg.json << 'EOF'
{
  "MY_PKG_VAR": "some_value",
  "ANOTHER_VAR": "another_value"
}
EOF
```

Or copy a pre-existing file:

```sh
mkdir -p $PREFIX/etc/conda/env_vars.d
cp $RECIPE_DIR/env_vars.json $PREFIX/etc/conda/env_vars.d/mypkg.json
```

### Using the $PREFIX path in environment variables

Since JSON files contain static values, you need to expand the `$PREFIX` environment variable when creating the file during the build. Use a heredoc **without** quotes to allow bash variable expansion:

```sh
mkdir -p $PREFIX/etc/conda/env_vars.d
cat > $PREFIX/etc/conda/env_vars.d/mypkg.json << EOF
{
  "MY_PKG_DATA_DIR": "$PREFIX/share/mypkg",
  "MY_PKG_CONFIG": "$PREFIX/etc/mypkg.conf"
}
EOF
```

This will write the actual prefix path (e.g., `/home/user/host_env_placehold_plachold...`) into the JSON file, which be replaced at installation time.

On Windows (in `bld.bat`), use `%PREFIX%`:

```batch
mkdir %PREFIX%\etc\conda\env_vars.d
echo {"MY_PKG_DATA_DIR": "%PREFIX%\\share\\mypkg"} > %PREFIX%\etc\conda\env_vars.d\mypkg.json
```

!!! tip "Quoted vs unquoted heredocs"
    Note the difference: `<< 'EOF'` (quoted) prevents variable expansion, while `<< EOF` (unquoted) allows bash to expand `$PREFIX` before writing the file.

### Complete directory structure

Here's the complete structure of activation-related files in a conda environment:

```text
<conda-prefix>/
├── bin/                           # Executables (Unix)
├── Scripts/                       # Executables (Windows)
├── etc/conda/
│   ├── activate.d/               # Activation scripts (shell-specific)
│   │   ├── 10-pkg1-activate.sh   # Bash/Zsh script
│   │   ├── 10-pkg1-activate.bat  # Cmd.exe script
│   │   └── 10-pkg1-activate.ps1  # PowerShell script
│   ├── deactivate.d/             # Deactivation scripts
│   │   ├── 10-pkg1-deactivate.sh
│   │   └── 10-pkg1-deactivate.bat
│   └── env_vars.d/               # Package environment variables (JSON)
│       ├── pkg1.json
│       ├── pkg2.json
│       └── zzz-override.json     # Loaded last due to filename
└── conda-meta/
    ├── pkg1-1.0.0-h1234.json      # Package metadata
    ├── pkg2-2.0.0-h5678.json
    └── state                      # Environment-level state (JSON)
```

### Processing order

When an environment is activated, the activation system:

1. Reads all `.json` files from `etc/conda/env_vars.d/` in **lexicographical order**
2. Reads the `conda-meta/state` file (if it exists)
3. Merges all variables, with later files overriding earlier ones

This means:

- If multiple packages define the same variable, the package whose filename comes later alphabetically will win
- You can prefix filenames with numbers (e.g., `00-base.json`, `50-mypkg.json`) to control priority
- The `conda-meta/state` file always has the highest priority


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
