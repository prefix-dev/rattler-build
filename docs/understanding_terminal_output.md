# Understanding the terminal output

When you build a recipe with rattler-build, a lot of information is printed on the terminal. Some of that information uses short codes that we are explaining here.

Let's look at the logs together!

## Variant configuration

```
 ╭─ Finding outputs from recipe
 │ Found 1 variants
 │ 
 │ Build variant: curl-8.0.1-h60d57d3_0
 │ 
 │ ╭─────────────────┬─────────────╮
 │ │ Variant         ┆ Version     │
 │ ╞═════════════════╪═════════════╡
 │ │ target_platform ┆ "osx-arm64" │
 │ ╰─────────────────┴─────────────╯
 │
 ╰─────────────────── (took 0 seconds)
```

This first section shows you what variants are applied to your recipe (from the variant configuration files). In this case, we only have a single variant (`target_platform`) which is always given. A variant can also expand to a matrix of multiple variant builds, in which case you would see more than one "output" being built (e.g. for different versions of Python or Boost).

You can select a single variant to build from the CLI using the `--variant` override flag (e.g. `--variant python=3.12`). To pass in a variant configuration file you can use `-m` or `--variant-config` and point to a yaml file. If a `variants.yaml` file is placed _next_ to the recipe, it is loaded automatically.

You can [read more about variants here](variants.md).

## Fetching the source code

```
 ╭─ Running build for recipe: curl-8.0.1-h60d57d3_0
 │
 │ ╭─ Fetching source code
 │ │ Fetching source from url: http://curl.haxx.se/download/curl-8.0.1.tar.bz2
 │ │ Validated SHA256 values of the downloaded file!
 │ │ Found valid source cache file.
 │ │ Using extracted directory from cache: /Users/wolfv/Programs/rattler-build/output/src_cache/curl-8_0_1_9b6b1e96
 │ │ Copying source from url: /Users/wolfv/Programs/rattler-build/output/src_cache/curl-8_0_1_9b6b1e96 to /Users/wolfv/Progra
 │ │ ms/rattler-build/output/bld/rattler-build_curl_1764660286/work
 │ │
 │ ╰─────────────────── (took 0 seconds)
```

In this next section, the source code is fetched. The source is first downloaded to a cache directory located under the "output" directory (by default `./output/src_cache`, but you can change the output directory. From the cache it is copied into the `$SRC_DIR`, which is a temporary directory used during the build (in our case `/Users/wolfv/Programs/rattler-build/output/bld/rattler-build_curl_1764660286/work`). The `rattler-build_curl_1764660286` is the build folder that is created for this particular build (the `176...` is a timestamp). 

```
 │ ╭─ Resolving environments
 │ │ 
 │ │ Resolving build environment:
 │ │   Platform: osx-arm64 [__unix=0=0, __osx=15.6=0, __archspec=1=m2]
 │ │   Channels: 
 │ │    - file:///Users/wolfv/Programs/rattler-build/output/
 │ │    - conda-forge
 │ │   Specs:
 │ │    - clang_osx-arm64
 │ │    - make
 │ │    - perl
 │ │    - pkg-config
 │ │    - libtool
 │ │ 
 │ │ ╭─────────────────────────┬─────────────┬─────────────────────┬─────────────┬─────────────╮
 │ │ │ Package                 ┆ Version     ┆ Build               ┆ Channel     ┆        Size │
 │ │ ╞═════════════════════════╪═════════════╪═════════════════════╪═════════════╪═════════════╡
 │ │ │ bzip2                   ┆ 1.0.8       ┆ hd037594_8          ┆ conda-forge ┆  122.13 KiB │
 │ │ │ ca-certificates         ┆ 2025.11.12  ┆ hbd8a1cb_0          ┆ conda-forge ┆  148.86 KiB │
 │ │ │ cctools_impl_osx-arm64  ┆ 1030.6.3    ┆ llvm21_1_haddd2d4_1 ┆ conda-forge ┆  730.35 KiB │
 │ │ │ cctools_osx-arm64       ┆ 1030.6.3    ┆ llvm21_1_h6d92914_1 ┆ conda-forge ┆   22.26 KiB │
 │ │ │ clang                   ┆ 21.1.6      ┆ default_hf9bcbb7_0  ┆ conda-forge ┆   24.39 KiB │
 │ │ │ clang-21                ┆ 21.1.6      ┆ default_h489deba_0  ┆ conda-forge ┆  807.97 KiB │
 │ │ │ clang_impl_osx-arm64    ┆ 21.1.6      ┆ hdbf2fcc_26         ┆ conda-forge ┆   17.52 KiB │
 │ │ │ clang_osx-arm64         ┆ 21.1.6      ┆ h07b0088_26         ┆ conda-forge ┆   20.12 KiB │
 │ │ │ compiler-rt             ┆ 21.1.6      ┆ hce30654_0          ┆ conda-forge ┆   15.62 KiB │
 │ │ │ compiler-rt21           ┆ 21.1.6      ┆ h855ad52_0          ┆ conda-forge ┆   96.03 KiB │
 │ │ │ compiler-rt21_osx-arm64 ┆ 21.1.6      ┆ h2514db7_0          ┆ conda-forge ┆   10.31 MiB │
 │ │ │ ld64                    ┆ 956.6       ┆ llvm21_1_h5d6df6c_1 ┆ conda-forge ┆   20.66 KiB │
 │ │ │ ld64_osx-arm64          ┆ 956.6       ┆ llvm21_1_hde6573c_1 ┆ conda-forge ┆ 1013.34 KiB │
 │ │ │ libclang-cpp21.1        ┆ 21.1.6      ┆ default_h73dfc95_0  ┆ conda-forge ┆   13.04 MiB │
 │ │ │ libcxx                  ┆ 21.1.6      ┆ hf598326_0          ┆ conda-forge ┆  556.47 KiB │
 │ │ │ libffi                  ┆ 3.5.2       ┆ he5f378a_0          ┆ conda-forge ┆   39.31 KiB │
 │ │ │ libglib                 ┆ 2.86.2      ┆ hfe11c1f_1          ┆ conda-forge ┆    3.47 MiB │
 │ │ │ libiconv                ┆ 1.18        ┆ h23cfdf5_2          ┆ conda-forge ┆  732.79 KiB │
 │ │ │ libintl                 ┆ 0.25.1      ┆ h493aca8_0          ┆ conda-forge ┆   88.83 KiB │
 │ │ │ libllvm21               ┆ 21.1.6      ┆ h8e0c9ce_0          ┆ conda-forge ┆   28.04 MiB │
 │ │ │ libltdl                 ┆ 2.4.3a      ┆ h286801f_0          ┆ conda-forge ┆   36.45 KiB │
 │ │ │ liblzma                 ┆ 5.8.1       ┆ h39f12f2_2          ┆ conda-forge ┆   90.12 KiB │
 │ │ │ libtool                 ┆ 2.5.4       ┆ h286801f_0          ┆ conda-forge ┆  404.96 KiB │
 │ │ │ libxml2                 ┆ 2.15.1      ┆ hba2cd1d_0          ┆ conda-forge ┆   39.66 KiB │
 │ │ │ libxml2-16              ┆ 2.15.1      ┆ h8eac4d7_0          ┆ conda-forge ┆  453.31 KiB │
 │ │ │ libzlib                 ┆ 1.3.1       ┆ h8359307_2          ┆ conda-forge ┆   45.35 KiB │
 │ │ │ llvm-openmp             ┆ 21.1.6      ┆ h4a912ad_0          ┆ conda-forge ┆  279.50 KiB │
 │ │ │ llvm-tools              ┆ 21.1.6      ┆ h855ad52_0          ┆ conda-forge ┆   86.46 KiB │
 │ │ │ llvm-tools-21           ┆ 21.1.6      ┆ h91fd4e7_0          ┆ conda-forge ┆   17.42 MiB │
 │ │ │ make                    ┆ 4.4.1       ┆ hc9fafa5_2          ┆ conda-forge ┆  267.62 KiB │
 │ │ │ ncurses                 ┆ 6.5         ┆ h5e97a16_3          ┆ conda-forge ┆  778.35 KiB │
 │ │ │ openssl                 ┆ 3.6.0       ┆ h5503f6c_0          ┆ conda-forge ┆    2.96 MiB │
 │ │ │ pcre2                   ┆ 10.47       ┆ h30297fc_0          ┆ conda-forge ┆  830.30 KiB │
 │ │ │ perl                    ┆ 5.32.1      ┆ 7_h4614cfb_perl5    ┆ conda-forge ┆   13.77 MiB │
 │ │ │ pkg-config              ┆ 0.29.2      ┆ hde07d2e_1009       ┆ conda-forge ┆   48.56 KiB │
 │ │ │ sdkroot_env_osx-arm64   ┆ 14.5        ┆ hfa17104_3          ┆ conda-forge ┆    8.58 KiB │
 │ │ │ sigtool                 ┆ 0.1.3       ┆ h44b9a77_0          ┆ conda-forge ┆  205.34 KiB │
 │ │ │ tapi                    ┆ 1600.0.11.8 ┆ h997e182_0          ┆ conda-forge ┆  195.02 KiB │
 │ │ │ zstd                    ┆ 1.5.7       ┆ hd0aec43_5          ┆ conda-forge ┆  380.61 KiB │
 │ │ ╰─────────────────────────┴─────────────┴─────────────────────┴─────────────┴─────────────╯
 │ │ 
 │ │ Resolving host environment:
 │ │   Platform: osx-arm64 [__unix=0=0, __osx=15.6=0, __archspec=1=m2]
 │ │   Channels: 
 │ │    - file:///Users/wolfv/Programs/rattler-build/output/
 │ │    - conda-forge
 │ │   Specs:
 │ │    - zlib
 │ │ 
 │ │ ╭─────────┬─────────┬────────────┬─────────────┬───────────╮
 │ │ │ Package ┆ Version ┆ Build      ┆ Channel     ┆      Size │
 │ │ ╞═════════╪═════════╪════════════╪═════════════╪═══════════╡
 │ │ │ libzlib ┆ 1.3.1   ┆ h8359307_2 ┆ conda-forge ┆ 45.35 KiB │
 │ │ │ zlib    ┆ 1.3.1   ┆ h8359307_2 ┆ conda-forge ┆ 75.79 KiB │
 │ │ ╰─────────┴─────────┴────────────┴─────────────┴───────────╯
 │ │ 
 │ │ Finalized run dependencies (curl-8.0.1-h60d57d3_0):
 │ │ ╭──────────────────┬─────────────────────────────────────╮
 │ │ │ Name             ┆ Spec                                │
 │ │ ╞══════════════════╪═════════════════════════════════════╡
 │ │ │ Run dependencies ┆                                     │
 │ │ │ libzlib          ┆ >=1.3.1,<2.0a0 (RE of [host: zlib]) │
 │ │ ╰──────────────────┴─────────────────────────────────────╯
 │ │
 │ ╰─────────────────── (took 1 second)
```

In this section, the build and host environments are _resolved_ and the "run-exports" are collected in order to compute the final run dependencies. In this case, we did not have any explicit run dependencies, however, the `zlib` host dependency has a "run-export" of `zlib >=1.3.1,<2.0a0`.

For the resolution of the host and build environments, the variants are also taken into account (for example compiler variants will influence what compiler is chosen).

In the next step, the build and host environments are installed. The build environment goes to a folder called `build_env`, and the host environment to a folder called `host_env_placehold_placehold...`. The host environment is referenced by `$PREFIX`, and is where we install the new files to.

```
 │ Installing build environment
 │ ✔ Successfully updated the build environment
 │ 
 │ Installing host environment
 │ ✔ Successfully updated the host environment
```

## Running the build script

```
 │ ╭─ Running build script
 │ │ INFO: activate_cctools_osx-arm64.sh made the following environmental changes:
 │ │ +AR=arm64-apple-darwin20.0.0-ar
 │ │ +AS=arm64-apple-darwin20.0.0-as
 │ │ +CHECKSYMS=arm64-apple-darwin20.0.0-checksyms
 │ │ +INSTALL_NAME_TOOL=arm64-apple-darwin20.0.0-install_name_tool
 │ │ +LD=arm64-apple-darwin20.0.0-ld
 │ │ +LIBTOOL=arm64-apple-darwin20.0.0-libtool
 │ │ +LIPO=arm64-apple-darwin20.0.0-lipo
 │ │ +NM=arm64-apple-darwin20.0.0-nm
 │ │ +NMEDIT=arm64-apple-darwin20.0.0-nmedit
 │ │ +OTOOL=arm64-apple-darwin20.0.0-otool
 │ │ +PAGESTUFF=arm64-apple-darwin20.0.0-pagestuff
 │ │ +RANLIB=arm64-apple-darwin20.0.0-ranlib
 │ │ +REDO_PREBINDING=arm64-apple-darwin20.0.0-redo_prebinding
 │ │ +SEG_ADDR_TABLE=arm64-apple-darwin20.0.0-seg_addr_table
 │ │ +SEG_HACK=arm64-apple-darwin20.0.0-seg_hack
 │ │ +SEGEDIT=arm64-apple-darwin20.0.0-segedit
 │ │ +SIZE=arm64-apple-darwin20.0.0-size
 │ │ +STRINGS=arm64-apple-darwin20.0.0-strings
 │ │ +STRIP=arm64-apple-darwin20.0.0-strip
 │ │ INFO: activate_clang_osx-arm64.sh made the following environmental changes:
 │ │ -CONDA_BUILD_CROSS_COMPILATION=0
 │ │ +_CONDA_PYTHON_SYSCONFIGDATA_NAME=_sysconfigdata_arm64_apple_darwin20_0_0
 │ │ +ac_cv_func_malloc_0_nonnull=yes
 │ │ +ac_cv_func_realloc_0_nonnull=yes
 │ │ +build_alias=arm64-apple-darwin20.0.0
 │ │ +CC_FOR_BUILD=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-clang
 │ │ +CC=arm64-apple-darwin20.0.0-clang
 │ │ +CFLAGS=-ftree-vectorize -fPIC -fstack-protector-strong -O2 -pipe -isystem $PREFIX/include -fdebug-prefix-map=$SRC_DIR=/
 │ │ usr/local/src/conda/curl-8.0.1 -fdebug-prefix-map=$PREFIX=/usr/local/src/conda-prefix
 │ │ +CLANG=arm64-apple-darwin20.0.0-clang
 │ │ +CMAKE_ARGS=-DCMAKE_AR=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-ar -DCMAKE_CXX_COMPILER_AR=$BUILD_PREFIX/bin/arm64-app
 │ │ le-darwin20.0.0-ar -DCMAKE_C_COMPILER_AR=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-ar -DCMAKE_RANLIB=$BUILD_PREFIX/bin/
 │ │ arm64-apple-darwin20.0.0-ranlib -DCMAKE_CXX_COMPILER_RANLIB=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-ranlib -DCMAKE_C_
 │ │ COMPILER_RANLIB=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-ranlib -DCMAKE_LINKER=$BUILD_PREFIX/bin/arm64-apple-darwin20.
 │ │ 0.0-ld -DCMAKE_STRIP=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-strip -DCMAKE_INSTALL_NAME_TOOL=$BUILD_PREFIX/bin/arm64-
 │ │ apple-darwin20.0.0-install_name_tool -DCMAKE_LIBTOOL=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-libtool -DCMAKE_OSX_DEPL
 │ │ OYMENT_TARGET=11.0 -DCMAKE_BUILD_TYPE=Release -DCMAKE_FIND_FRAMEWORK=LAST -DCMAKE_FIND_APPBUNDLE=LAST -DCMAKE_INSTALL_PR
 │ │ EFIX=$PREFIX -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PROGRAM_PATH=$BUILD_PREFIX/bin;$PREFIX/bin
 │ │ +CMAKE_PREFIX_PATH=:$PREFIX
 │ │ +CONDA_TOOLCHAIN_BUILD=arm64-apple-darwin20.0.0
 │ │ +CONDA_TOOLCHAIN_HOST=arm64-apple-darwin20.0.0
 │ │ +CPP_FOR_BUILD=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-clang-cpp
 │ │ +CPP=arm64-apple-darwin20.0.0-clang-cpp
 │ │ +CPPFLAGS=-D_FORTIFY_SOURCE=2 -isystem $PREFIX/include -mmacosx-version-min=11.0
 │ │ +DEBUG_CFLAGS=-ftree-vectorize -fPIC -fstack-protector-strong -O2 -pipe -Og -g -Wall -Wextra -isystem $PREFIX/include -f
 │ │ debug-prefix-map=$SRC_DIR=/usr/local/src/conda/curl-8.0.1 -fdebug-prefix-map=$PREFIX=/usr/local/src/conda-prefix
 │ │ +host_alias=arm64-apple-darwin20.0.0
 │ │ +HOST=arm64-apple-darwin20.0.0
 │ │ +LDFLAGS_LD=-headerpad_max_install_names -dead_strip_dylibs -rpath $PREFIX/lib -L$PREFIX/lib
 │ │ +LDFLAGS=-Wl,-headerpad_max_install_names -Wl,-dead_strip_dylibs -Wl,-rpath,$PREFIX/lib -L$PREFIX/lib
 │ │ +MESON_ARGS=-Dbuildtype=release --prefix=$PREFIX -Dlibdir=lib
 │ │ +OBJC_FOR_BUILD=$BUILD_PREFIX/bin/arm64-apple-darwin20.0.0-clang
 │ │ +OBJC=arm64-apple-darwin20.0.0-clang
 │ │ checking whether to enable maintainer-specific portions of Makefiles... no
 │ │ checking whether make supports nested variables... yes
 │ │ checking whether to enable debug build options... no
 │ │ checking whether to enable compiler optimizer... (assumed) yes
 │ │ checking whether to enable strict compiler warnings... no
 │ │ checking whether to enable compiler warnings as errors... no
 │ │ checking whether to enable curl debug memory tracking... no
 │ │ checking whether to enable hiding of library internal symbols... yes
 │ │ checking whether to enable c-ares for DNS lookups... no
 │ │ checking whether to disable dependency on -lrt... (assumed no)
 │ │ checking whether to enable ECH support... no
 │ │ checking for path separator... :
 │ │ checking for sed... /Users/wolfv/.pixi/bin/sed
 │ │ checking for grep... /Users/wolfv/.pixi/bin/grep
 │ │ checking that grep -E works... yes
 │ │ checking for a BSD-compatible install... /Users/wolfv/.pixi/bin/install -c
 │ │ checking for arm64-apple-darwin20.0.0-gcc... arm64-apple-darwin20.0.0-clang
 │ │ checking whether the C compiler works... yes
 │ │ checking for C compiler default output file name... a.out
 │ │ checking for suffix of executables... 
 │ │ checking whether we are cross compiling... no
 │ │ checking for suffix of object files... o
 │ │ checking whether the compiler supports GNU C... yes
 │ │ checking whether arm64-apple-darwin20.0.0-clang accepts -g... yes
 │ │ checking for arm64-apple-darwin20.0.0-clang option to enable C11 features... none needed
 │ │ checking whether arm64-apple-darwin20.0.0-clang understands -c and -o together... yes
 │ │ checking how to run the C preprocessor... arm64-apple-darwin20.0.0-clang-cpp
 │ │ checking for stdio.h... yes
 │ │ checking for stdlib.h... yes
 │ │ checking for string.h... yes
 │ │ checking for inttypes.h... yes
 │ │ checking for stdint.h... yes
 │ │ checking for strings.h... yes
 │ │ checking for sys/stat.h... yes
 │ │ checking for sys/types.h... yes
 │ │ checking for unistd.h... yes
 │ │ checking for stdatomic.h... yes
 │ │ checking if _Atomic is available... yes
 │ │ checking for a sed that does not truncate output... (cached) /Users/wolfv/.pixi/bin/sed
 │ │ checking for code coverage support... no
 │ │ checking whether build environment is sane... yes
 │ │ checking for a race-free mkdir -p... /Users/wolfv/.pixi/bin/mkdir -p
 │ │ checking for gawk... gawk
 │ │ checking whether make sets $(MAKE)... yes
 │ │ checking whether make supports the include directive... yes (GNU style)
 │ │ checking dependency style of arm64-apple-darwin20.0.0-clang... gcc3
 │ │ checking curl version... 8.0.1
 │ │ checking for httpd... /usr/sbin/httpd
 │ │ checking for apachectl... /usr/sbin/apachectl
 │ │ checking for apxs... no
 │ │ configure: apxs not in PATH, httpd tests disabled
 │ │ checking for nghttpx... no
 │ │ checking for caddy... no
 │ │ checking build system type... aarch64-apple-darwin20.0.0
 │ │ checking host system type... aarch64-apple-darwin20.0.0
 │ │ checking for grep that handles long lines and -e... (cached) /Users/wolfv/.pixi/bin/grep
 │ │ checking for egrep... /Users/wolfv/.pixi/bin/grep -E
 │ │ checking if OS is AIX (to define _ALL_SOURCE)... no
 │ │ checking if _THREAD_SAFE is already defined... no
 │ │ checking if _THREAD_SAFE is actually needed... no
 │ │ checking if _THREAD_SAFE is onwards defined... no
 │ │ checking if _REENTRANT is already defined... no
 │ │ checking if _REENTRANT is actually needed... no
 │ │ checking if _REENTRANT is onwards defined... no
 │ │ checking for special C compiler options needed for large files... no
 │ │ checking for _FILE_OFFSET_BITS value needed for large files... no
 | | ... 
 | | ... more output from autotools
 | | ... 
 | | # COMPILATION STARTS - configuration is printed
 │ │ configure: Configured to build curl/libcurl:
 │ │   Host setup:       aarch64-apple-darwin20.0.0
 │ │   Install prefix:   $PREFIX
 │ │   Compiler:         arm64-apple-darwin20.0.0-clang
 │ │    CFLAGS:          -ftree-vectorize -fPIC -fstack-protector-strong -O2 -pipe -isystem $PREFIX/include -fdebug-prefix-ma
 │ │ p=$SRC_DIR=/usr/local/src/conda/curl-8.0.1 -fdebug-prefix-map=$PREFIX=/usr/local/src/conda-prefix -Qunused-arguments -Wn
 │ │ o-pointer-bool-conversion -Werror=partial-availability
 │ │    CPPFLAGS:        -D_FORTIFY_SOURCE=2 -isystem $PREFIX/include -mmacosx-version-min=11.0 -isystem $PREFIX/include
 │ │    LDFLAGS:         -Wl,-headerpad_max_install_names -Wl,-dead_strip_dylibs -Wl,-rpath,$PREFIX/lib -L$PREFIX/lib -framew
 │ │ ork CoreFoundation -framework SystemConfiguration -L$PREFIX/lib -framework CoreFoundation -framework Security
 │ │    LIBS:            -lldap -lz
 │ │   curl version:     8.0.1
 │ │   SSL:              enabled (Secure Transport)
 │ │   SSH:              no      (--with-{libssh,libssh2})
 │ │   zlib:             enabled
 │ │   brotli:           no      (--with-brotli)
 │ │   zstd:             no      (--with-zstd)
 │ │   GSS-API:          no      (--with-gssapi)
 │ │   GSASL:            no      (libgsasl not found)
 │ │   TLS-SRP:          no      (--enable-tls-srp)
 │ │   resolver:         POSIX threaded
 │ │   IPv6:             no      (--enable-ipv6)
 │ │   Unix sockets:     enabled
 │ │   IDN:              no      (--with-{libidn2,winidn})
 │ │   Build libcurl:    Shared=yes, Static=no
 │ │   Built-in manual:  no      (--enable-manual)
 │ │   --libcurl option: enabled (--disable-libcurl-option)
 │ │   Verbose errors:   enabled (--disable-verbose)
 │ │   Code coverage:    disabled
 │ │   SSPI:             no      (--enable-sspi)
 │ │   ca cert bundle:   no
 │ │   ca cert path:     
 │ │   ca fallback:      
 │ │   LDAP:             enabled (OpenLDAP)
 │ │   LDAPS:            enabled
 │ │   RTSP:             enabled
 │ │   RTMP:             no      (--with-librtmp)
 │ │   PSL:              no      (libpsl not found)
 │ │   Alt-svc:          enabled (--disable-alt-svc)
 │ │   Headers API:      enabled (--disable-headers-api)
 │ │   HSTS:             enabled (--disable-hsts)
 │ │   HTTP1:            enabled (internal)
 │ │   HTTP2:            no      (--with-nghttp2, --with-hyper)
 │ │   HTTP3:            no      (--with-ngtcp2, --with-quiche --with-msh3)
 │ │   ECH:              no      (--enable-ech)
 │ │   WebSockets:       no      (--enable-websockets)
 │ │   Protocols:        DICT FILE FTP FTPS GOPHER GOPHERS HTTP HTTPS IMAP IMAPS LDAP LDAPS MQTT POP3 POP3S RTSP SMB SMBS SMT
 │ │ P SMTPS TELNET TFTP
 │ │   Features:         AsynchDNS HSTS HTTPS-proxy Largefile NTLM NTLM_WB SSL UnixSockets alt-svc libz threadsafe
 │ │ Making all in lib
 │ │ make[1]: Entering directory '$SRC_DIR/lib'
 │ │ make  all-am
 │ │ make[2]: Entering directory '$SRC_DIR/lib'
 │ │   CC       libcurl_la-altsvc.lo
 │ │   CC       libcurl_la-amigaos.lo
 │ │   CC       libcurl_la-asyn-ares.lo
 │ │   CC       libcurl_la-bufref.lo
 │ │   CC       libcurl_la-asyn-thread.lo
 │ │   CC       libcurl_la-base64.lo
 │ │   CC       libcurl_la-cf-https-connect.lo
 │ │   CC       libcurl_la-c-hyper.lo
 │ │   CC       libcurl_la-cfilters.lo
 │ │   CC       libcurl_la-cf-socket.lo
 │ │   CC       libcurl_la-conncache.lo
 │ │   CC       libcurl_la-connect.lo
 | | ...
 | | ... more compilation ...
 | | ... 
  │ │ make[6]: Nothing to be done for 'install-exec-am'.
 │ │  /Users/wolfv/.pixi/bin/mkdir -p '$PREFIX/share/aclocal'
 │ │  /Users/wolfv/.pixi/bin/install -c -m 644 libcurl.m4 '$PREFIX/share/aclocal'
 │ │  /Users/wolfv/.pixi/bin/mkdir -p '$PREFIX/share/man/man3'
 │ │  /Users/wolfv/.pixi/bin/install -c -m 644 curl_easy_cleanup.3 curl_easy_duphandle.3 curl_easy_escape.3 curl_easy_getinfo '$PREFIX/share/man/man3'
 │ │  /Users/wolfv/.pixi/bin/install -c -m 644 libcurl-easy.3 libcurl-env.3 libcurl-errors.3 libcurl-multi.3 libcurl-security
 │ │ .3 libcurl-share.3 libcurl-symbols.3 libcurl-thread.3 libcurl-tutorial.3 libcurl-url.3 libcurl.3 '$PREFIX/share/man/man3
 │ │ '
 │ │ make[6]: Leaving directory '$SRC_DIR/docs/libcurl'
 │ │ make[1]: Leaving directory '$SRC_DIR'
 │ │
 │ ╰─────────────────── (took 59 seconds)
```

This section (already shortened) is the full build log of the curl. It depends heavily on the package and the tools that are used in the build script on what is printed here. But usually, you can expect some environment variables at the start (e.g. from compiler activation scripts), some output from configuring the compilation (if it is a compiled package), and output from pip, ninja, make or other tools to build the package. After building is completed, files should be installed into the $PREFIX.

## Package contents

This section shows some important diagnostic information.

```txt
 │ ╭─ Packaging new files
 │ │ Copying done!
 │ │ Relinking "libcurl.4.dylib"
 │ │ Relinking "curl"
```

This shows that we are "re-linking" both the `dylib` and the `curl` executable to make them relocatable binaries. In practice, this means that the `rpath` of the binaries is adjusted to make them look for shared libraries in relative locations (relative to their installation location, and thus "relocatable"). You can read more about relinking under [Debugging builds](debugging_builds.md).

```txt
 │ │ [lib/libcurl.4.dylib] links against:
 │ │  ├─ /usr/lib/libSystem.B.dylib (system)
 │ │  ├─ /System/Library/Frameworks/LDAP.framework/Versions/A/LDAP (system)
 │ │  ├─ lib/libcurl.4.dylib (package)
 │ │  ├─ /System/Library/Frameworks/Security.framework/Versions/A/Security (system)
 │ │  ├─ lib/libz.1.3.1.dylib (libzlib)
 │ │  └─ /System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (system)
 │ │ 
 │ │ [bin/curl] links against:
 │ │  ├─ /usr/lib/libSystem.B.dylib (system)
 │ │  └─ lib/libcurl.4.dylib (package)
```

We also see some output that shows us against _what_ the binaries are linking. Some of the shared libraries are provided by the system (under `/System/...` on macOS), and some are provided by other packages (such as `libz.1.3.1.dylib`).

We try to prevent linking against libraries that are not "allow-listed" on the system (ie. always installed on Linux/macOS/Windows) or not provided by any package from the dependencies (that is over-linking or under-depending). All linked libraries should either come from a declared dependency or from the allowed system libraries.

There are additional options to control this behavior in the [dynamic linking configuration](build_options.md/#dynamic-linking-configuration).

```
 │ │ Post-processing done!
 │ │ Writing test files
 │ │ Writing metadata for package
 │ │ Copying license files
 │ │ Copying recipe files
```

Here, data is written for the `info/` folder: we copy the test files into the correct locations and create a big `tests.yaml` file for later execution (when the package is finally assembled). We also write some metadata, like `about.json` or `index.json` files according to the metadata provided in the recipe.

Then, any license files matched by the globs given are copied into `info/licenses` and the recipe and all files next to it are copied to `info/recipe`.

```txt
 │ │ Files in package:
 │ │   ├─ bin/curl (198.28 KiB)
 │ │   ├─ bin/curl-config (8.92 KiB) [prefix:text]
 │ │   ├─ include/curl/curl.h (124.84 KiB)
 │ │   ├─ include/curl/curlver.h (2.97 KiB)
 │ │   ├─ include/curl/easy.h (3.93 KiB)
 │ │   ├─ include/curl/header.h (2.84 KiB)
 │ │   ├─ include/curl/mprintf.h (2.07 KiB)
 │ │   ├─ include/curl/multi.h (16.91 KiB)
 │ │   ├─ include/curl/options.h (2.34 KiB)
 │ │   ├─ include/curl/stdcheaders.h (1.33 KiB)
 │ │   ├─ include/curl/system.h (18.67 KiB)
 │ │   ├─ include/curl/typecheck-gcc.h (42.45 KiB)
 │ │   ├─ include/curl/urlapi.h (5.28 KiB)
 │ │   ├─ include/curl/websockets.h (2.68 KiB)
 │ │   ├─ lib/libcurl.4.dylib (541.50 KiB)
 │ │   ├─ lib/libcurl.dylib -> libcurl.4.dylib
 │ │   ├─ lib/pkgconfig/libcurl.pc (1.86 KiB) [prefix:text]
 │ │   ├─ info/about.json (453 B)
 │ │   ├─ info/hash_input.json (32 B)
 │ │   ├─ info/index.json (253 B)
 │ │   ├─ info/licenses/COPYING (1.06 KiB)
 │ │   ├─ info/paths.json (3.91 KiB)
 │ │   ├─ info/recipe/build.bat (371 B)
 │ │   ├─ info/recipe/build.sh (520 B)
 │ │   ├─ info/recipe/changes.patch (263 B)
 │ │   ├─ info/recipe/recipe.yaml (966 B)
 │ │   ├─ info/recipe/rendered_recipe.yaml (30.32 KiB)
 │ │   ├─ info/recipe/variant_config.yaml (27 B)
 │ │   └─ info/tests/tests.yaml (38 B)
 │ │ 
 │ │ Package statistics: 29 files (17 content, 12 metadata), total size: 1015.02 KiB
 │ │ Largest files:
 │ │   541.50 KiB - lib/libcurl.4.dylib
 │ │   198.28 KiB - bin/curl
 │ │   124.84 KiB - include/curl/curl.h
 │ │   42.45 KiB - include/curl/typecheck-gcc.h
 │ │   30.32 KiB - info/recipe/rendered_recipe.yaml
```

With everything said and done, we are listing the final files for the package! This is a good thing to check as these are the contents!

The files are highlighted:

- green: executable
- magenta: symlink (shows symlink target)
- yellow brackets: files that contain the value of `$PREFIX` as a string (`[prefix:text]` or `[prefix:bin]`).

Files that contain the $PREFIX (that is files that contain the current installation path, e.g. `/user/name/.../output/.../host_env_placehold_placehold...`) are marked with the yellow brackets. 

When a file contains the $PREFIX as a string, it will be replaced at installation time with the actual installation prefix (that is also the reason for the long placeholder string!). You can read more about prefix replacement in [Debugging Builds](internals.md#making-packages-relocatable-with-rattler-build).
Ideally, no file contains the installation prefix as string, so that there is no text replacement at installation time.

We then see a listing of the largest files in the package which can be a helpful sanity check.

Lastly the files are compressed and "packaged":

```txt
 │ │ Creating target folder '/Users/wolfv/Programs/rattler-build/output/osx-arm64'
 │ │ Compressing archive...
 │ │ Archive written to '/Users/wolfv/Programs/rattler-build/output/osx-arm64/curl-8.0.1-h60d57d3_0.conda'
 │ ╰─────────────────── (took 1 second)
 │
 ╰─────────────────── (took 62 seconds)
```

## Testing the package

Now, with the package completed, the tests can run!

```txt
 ╭─ Running script test for recipe: curl-8.0.1-h60d57d3_0.conda
 │ 
 │ Resolving test environment:
 │   Platform: osx-arm64 [__unix=0=0, __osx=15.6=0, __archspec=1=m2]
 │   Channels: 
 │    - file:///var/folders/ls/nd2_c1qn1tz8c8sccxmg83m00000gn/T/.tmpCi1Blz/
 │    - file:///Users/wolfv/Programs/rattler-build/output/
 │    - conda-forge
 │   Specs:
 │    - curl ==8.0.1 h60d57d3_0
 │ 
 │ ╭─────────┬─────────┬────────────┬─────────────┬────────────╮
 │ │ Package ┆ Version ┆ Build      ┆ Channel     ┆       Size │
 │ ╞═════════╪═════════╪════════════╪═════════════╪════════════╡
 │ │ curl    ┆ 8.0.1   ┆ h60d57d3_0 ┆ .tmpCi1Blz  ┆ 376.73 KiB │
 │ │ libzlib ┆ 1.3.1   ┆ h8359307_2 ┆ conda-forge ┆  45.35 KiB │
 │ ╰─────────┴─────────┴────────────┴─────────────┴────────────╯
 │ 
 │ Installing test environment
 │ ✔ Successfully updated the test environment
 │ Testing commands:
 │ curl 8.0.1 (aarch64-apple-darwin20.0.0) libcurl/8.0.1 SecureTransport zlib/1.3.1
 │ Release-Date: 2023-03-20
 │ Protocols: dict file ftp ftps gopher gophers http https imap imaps ldap ldaps mqtt pop3 pop3s rtsp smb smbs smtp smtps tel
 │ net tftp
 │ Features: alt-svc AsynchDNS HSTS HTTPS-proxy Largefile libz NTLM NTLM_WB SSL threadsafe UnixSockets
 │
 ╰─────────────────── (took 0 seconds)
 ✔ all tests passed!
```

This executes a "script" test which just runs `curl --version`.

## Build summary

Lastly, the build is summarized:

```
 ╭─ Build summary
 │
 │ ╭─ Build summary for recipe: curl-8.0.1-h60d57d3_0
 │ │ Variant configuration (hash: h60d57d3_0):
 │ │ ╭─────────────────┬─────────────╮
 │ │ │ target_platform ┆ "osx-arm64" │
 │ │ ╰─────────────────┴─────────────╯
 │ │ 
 │ │ Build dependencies:
 │ │ ╭─────────────────────────┬─────────────────┬─────────────┬─────────────────────┬─────────────┬─────────────╮
 │ │ │ Package                 ┆ Spec            ┆ Version     ┆ Build               ┆ Channel     ┆        Size │
 │ │ ╞═════════════════════════╪═════════════════╪═════════════╪═════════════════════╪═════════════╪═════════════╡
 │ │ │ clang_osx-arm64         ┆ clang_osx-arm64 ┆ 21.1.6      ┆ h07b0088_26         ┆ conda-forge ┆   20.12 KiB │
 │ │ │ libtool                 ┆ libtool         ┆ 2.5.4       ┆ h286801f_0          ┆ conda-forge ┆  404.96 KiB │
 │ │ │ make                    ┆ make            ┆ 4.4.1       ┆ hc9fafa5_2          ┆ conda-forge ┆  267.62 KiB │
 │ │ │ perl                    ┆ perl            ┆ 5.32.1      ┆ 7_h4614cfb_perl5    ┆ conda-forge ┆   13.77 MiB │
 │ │ │ pkg-config              ┆ pkg-config      ┆ 0.29.2      ┆ hde07d2e_1009       ┆ conda-forge ┆   48.56 KiB │
 │ │ │ bzip2                   ┆                 ┆ 1.0.8       ┆ hd037594_8          ┆ conda-forge ┆  122.13 KiB │
 │ │ │ ca-certificates         ┆                 ┆ 2025.11.12  ┆ hbd8a1cb_0          ┆ conda-forge ┆  148.86 KiB │
 │ │ │ cctools_impl_osx-arm64  ┆                 ┆ 1030.6.3    ┆ llvm21_1_haddd2d4_1 ┆ conda-forge ┆  730.35 KiB │
 │ │ │ cctools_osx-arm64       ┆                 ┆ 1030.6.3    ┆ llvm21_1_h6d92914_1 ┆ conda-forge ┆   22.26 KiB │
 │ │ │ clang                   ┆                 ┆ 21.1.6      ┆ default_hf9bcbb7_0  ┆ conda-forge ┆   24.39 KiB │
 │ │ │ clang-21                ┆                 ┆ 21.1.6      ┆ default_h489deba_0  ┆ conda-forge ┆  807.97 KiB │
 │ │ │ clang_impl_osx-arm64    ┆                 ┆ 21.1.6      ┆ hdbf2fcc_26         ┆ conda-forge ┆   17.52 KiB │
 │ │ │ compiler-rt             ┆                 ┆ 21.1.6      ┆ hce30654_0          ┆ conda-forge ┆   15.62 KiB │
 │ │ │ compiler-rt21           ┆                 ┆ 21.1.6      ┆ h855ad52_0          ┆ conda-forge ┆   96.03 KiB │
 │ │ │ compiler-rt21_osx-arm64 ┆                 ┆ 21.1.6      ┆ h2514db7_0          ┆ conda-forge ┆   10.31 MiB │
 │ │ │ ld64                    ┆                 ┆ 956.6       ┆ llvm21_1_h5d6df6c_1 ┆ conda-forge ┆   20.66 KiB │
 │ │ │ ld64_osx-arm64          ┆                 ┆ 956.6       ┆ llvm21_1_hde6573c_1 ┆ conda-forge ┆ 1013.34 KiB │
 │ │ │ libclang-cpp21.1        ┆                 ┆ 21.1.6      ┆ default_h73dfc95_0  ┆ conda-forge ┆   13.04 MiB │
 │ │ │ libcxx                  ┆                 ┆ 21.1.6      ┆ hf598326_0          ┆ conda-forge ┆  556.47 KiB │
 │ │ │ libffi                  ┆                 ┆ 3.5.2       ┆ he5f378a_0          ┆ conda-forge ┆   39.31 KiB │
 │ │ │ libglib                 ┆                 ┆ 2.86.2      ┆ hfe11c1f_1          ┆ conda-forge ┆    3.47 MiB │
 │ │ │ libiconv                ┆                 ┆ 1.18        ┆ h23cfdf5_2          ┆ conda-forge ┆  732.79 KiB │
 │ │ │ libintl                 ┆                 ┆ 0.25.1      ┆ h493aca8_0          ┆ conda-forge ┆   88.83 KiB │
 │ │ │ libllvm21               ┆                 ┆ 21.1.6      ┆ h8e0c9ce_0          ┆ conda-forge ┆   28.04 MiB │
 │ │ │ libltdl                 ┆                 ┆ 2.4.3a      ┆ h286801f_0          ┆ conda-forge ┆   36.45 KiB │
 │ │ │ liblzma                 ┆                 ┆ 5.8.1       ┆ h39f12f2_2          ┆ conda-forge ┆   90.12 KiB │
 │ │ │ libxml2                 ┆                 ┆ 2.15.1      ┆ hba2cd1d_0          ┆ conda-forge ┆   39.66 KiB │
 │ │ │ libxml2-16              ┆                 ┆ 2.15.1      ┆ h8eac4d7_0          ┆ conda-forge ┆  453.31 KiB │
 │ │ │ libzlib                 ┆                 ┆ 1.3.1       ┆ h8359307_2          ┆ conda-forge ┆   45.35 KiB │
 │ │ │ llvm-openmp             ┆                 ┆ 21.1.6      ┆ h4a912ad_0          ┆ conda-forge ┆  279.50 KiB │
 │ │ │ llvm-tools              ┆                 ┆ 21.1.6      ┆ h855ad52_0          ┆ conda-forge ┆   86.46 KiB │
 │ │ │ llvm-tools-21           ┆                 ┆ 21.1.6      ┆ h91fd4e7_0          ┆ conda-forge ┆   17.42 MiB │
 │ │ │ ncurses                 ┆                 ┆ 6.5         ┆ h5e97a16_3          ┆ conda-forge ┆  778.35 KiB │
 │ │ │ openssl                 ┆                 ┆ 3.6.0       ┆ h5503f6c_0          ┆ conda-forge ┆    2.96 MiB │
 │ │ │ pcre2                   ┆                 ┆ 10.47       ┆ h30297fc_0          ┆ conda-forge ┆  830.30 KiB │
 │ │ │ sdkroot_env_osx-arm64   ┆                 ┆ 14.5        ┆ hfa17104_3          ┆ conda-forge ┆    8.58 KiB │
 │ │ │ sigtool                 ┆                 ┆ 0.1.3       ┆ h44b9a77_0          ┆ conda-forge ┆  205.34 KiB │
 │ │ │ tapi                    ┆                 ┆ 1600.0.11.8 ┆ h997e182_0          ┆ conda-forge ┆  195.02 KiB │
 │ │ │ zstd                    ┆                 ┆ 1.5.7       ┆ hd0aec43_5          ┆ conda-forge ┆  380.61 KiB │
 │ │ ╰─────────────────────────┴─────────────────┴─────────────┴─────────────────────┴─────────────┴─────────────╯
 │ │ 
 │ │ Host dependencies:
 │ │ ╭─────────┬──────┬─────────┬────────────┬─────────────┬───────────╮
 │ │ │ Package ┆ Spec ┆ Version ┆ Build      ┆ Channel     ┆      Size │
 │ │ ╞═════════╪══════╪═════════╪════════════╪═════════════╪═══════════╡
 │ │ │ zlib    ┆ zlib ┆ 1.3.1   ┆ h8359307_2 ┆ conda-forge ┆ 75.79 KiB │
 │ │ │ libzlib ┆      ┆ 1.3.1   ┆ h8359307_2 ┆ conda-forge ┆ 45.35 KiB │
 │ │ ╰─────────┴──────┴─────────┴────────────┴─────────────┴───────────╯
 │ │ 
 │ │ Run dependencies:
 │ │ ╭──────────────────┬─────────────────────────────────────╮
 │ │ │ Name             ┆ Spec                                │
 │ │ ╞══════════════════╪═════════════════════════════════════╡
 │ │ │ Run dependencies ┆                                     │
 │ │ │ libzlib          ┆ >=1.3.1,<2.0a0 (RE of [host: zlib]) │
 │ │ ╰──────────────────┴─────────────────────────────────────╯
 │ │ 
 │ │ Artifact: /Users/wolfv/Programs/rattler-build/output/osx-arm64/curl-8.0.1-h60d57d3_0.conda (376.73 KiB)
 │ │
 │ ╰─────────────────── (took 0 seconds)
 │
 ╰─────────────────── (took 0 seconds)
```