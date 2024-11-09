#!/bin/bash
set -Eeuo pipefail
export CARGO_TARGET_DIR="target-pixi"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=$CONDA_PREFIX/bin/mold"

# on macOS we need to set these rust flags:
if [[ uname == "Darwin" ]]; then
    export CARGO_BUILD_RUSTFLAGS="-C link-arg=-Wl,-rpath,$CONDA_PREFIX/lib"
fi
export RATTLER_BUILD_PATH="$PIXI_PROJECT_ROOT/target-pixi/release/rattler-build"
