#!/bin/bash
# Creates a minimal Mach-O binary with two rpaths of the same length.
# This is used by tests to create duplicate rpaths via builtin relink
# (overwriting @loader_path/../xxx with @loader_path/../lib), since
# both the linker and install_name_tool on modern macOS refuse to create
# duplicate LC_RPATH entries directly.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TMPFILE=$(mktemp /tmp/main_XXXXXX.c)

cat > "$TMPFILE" <<'EOF'
int main() { return 0; }
EOF

cc -o "$SCRIPT_DIR/duplicate-rpath-macos" "$TMPFILE" \
    -Wl,-rpath,@loader_path/../lib \
    -Wl,-rpath,@loader_path/../xxx

rm -f "$TMPFILE"

echo "Created: $SCRIPT_DIR/duplicate-rpath-macos"
echo "RPATHs:"
otool -l "$SCRIPT_DIR/duplicate-rpath-macos" | grep -A2 LC_RPATH
