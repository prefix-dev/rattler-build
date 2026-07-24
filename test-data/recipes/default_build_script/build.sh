#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$PREFIX/share/default_build_script"
echo default-build-script > "$PREFIX/share/default_build_script/marker.txt"
