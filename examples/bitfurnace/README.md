# Example that uses `bitfurnace` (experimental)

The idea of `bitfurnace` is to provide ways to provide scaffolding for common build systems in a cross-platform way.

Instead of writing `bash` or `cmd.exe` scripts, you can write a small "python" script that will be executed by `bitfurnace`.

```python
from bitfurnace.cmake import CMake

class Recipe(CMake):
    pass

Recipe().run_all_stages()
```

## zstd CMake example


```py
from bitfurnace.cmake import CMake

class Recipe(CMake):
    cmakelists_dir = src_dir / "build" / "cmake"
    workdir = src_dir / "cmake_build"

    test_cmd = "ninja"
    default_test_args = ["test"]

    cmake_configure_args = {
        "ZSTD_LEGACY_SUPPORT": True,
        "ZSTD_BUILD_PROGRAMS": False,
        "ZSTD_BUILD_CONTRIB": False,
        "ZSTD_PROGRAMS_LINK_SHARED": True,
    }
```

## Autotools example

```py
from bitfurnace.autotools import Autotools

class Recipe(Autotools):

    def get_configure_args(self):
        configure_args = []

        if features.static:
            configure_args += ['--enable-static', '--disable-shared']
        else:
            configure_args += ['--disable-static', '--enable-shared']

        if target_platform.startswith('osx'):
            configure_args += ['--with-iconv']
        else:
            configure_args += ['--without-iconv']
            if features.zstd:
                self.ldflags += ['-pthread']


        configure_args += [
            '--without-cng',
            '--without-nettle',
            '--without-expat',
        ]

        return configure_args
```

## Meson example

```py
from bitfurnace.meson import Meson

class Recipe(Meson)
    pass
```
